//! napi entry points, called from `src-js/cli.ts` and the programmatic API.

use std::sync::Arc;

use napi::bindgen_prelude::{Buffer, spawn_blocking};
use napi_derive::napi;

use oxcruise_report::{Summary, violation_outs};

use crate::CliRunner;
use crate::js_plugins::graph_buffer;
#[allow(unused_imports)]
use crate::js_plugins::host::{
    Callbacks, GraphCb, Host, HostConfigLoader, PairCb, RenderRequest, RunRulesRequest, StringCb,
};

/// What a run produced, for callers that want the output as data (the `cruise()` API).
#[napi(object)]
pub struct RunOutcome {
    pub exit_code: u32,
    pub stdout: String,
    pub stderr: String,
}

/// A run's graph and violations, for the `cruise()` API: the graph travels as the binary buffer
/// the plugin host reads, so a large graph is never rendered to JSON and parsed again.
#[napi(object)]
pub struct CruiseOutcome {
    /// 0 or 1 when the run produced a result (1: it found errors), 2 when it failed.
    pub exit_code: u32,
    /// Warnings, or the reason the run failed.
    pub stderr: String,
    /// The graph in the layout of `graph_buffer`; absent when the run failed.
    pub graph: Option<Buffer>,
    /// JSON `{ violations, summary }`; absent when the run failed.
    pub report: Option<String>,
}

fn runner(args: Vec<String>, host: Arc<Host>) -> CliRunner {
    // The host config loader borrows the host for the duration of the run.
    CliRunner::new(args)
        .with_js_config_loader(Box::new(OwnedHostLoader(host.clone())))
        .with_plugins(host.clone(), host)
}

struct OwnedHostLoader(Arc<Host>);

impl oxcruise_config::JsConfigLoader for OwnedHostLoader {
    fn load(
        &self,
        path: &std::path::Path,
    ) -> Result<serde_json::Value, oxcruise_config::ConfigError> {
        HostConfigLoader(&self.0).load(path)
    }
}

/// Run the CLI with `args`, writing to the process stdout/stderr. Resolves to the exit code.
#[napi(
    ts_args_type = "args: string[], loadPlugin: (request: string) => Promise<string>, runRules: (request: RunRulesRequest) => Promise<string>, renderReport: (request: RenderRequest) => Promise<string>, loadJsConfig: (path: string) => Promise<string>",
    ts_return_type = "Promise<number>"
)]
pub async fn run(
    args: Vec<String>,
    load_plugin: StringCb,
    run_rules: GraphCb,
    render_report: PairCb,
    load_js_config: StringCb,
) -> napi::Result<u32> {
    let host = Arc::new(Host::new(Callbacks {
        load_plugin,
        run_rules,
        render_report,
        load_js_config,
    }));
    let result = spawn_blocking(move || runner(args, host).run_stdio())
        .await
        .map_err(|e| napi::Error::from_reason(e.to_string()))?;
    Ok(u32::from(result.code()))
}

/// Like [`run`], but returns what the CLI printed instead of printing it.
#[napi(
    ts_args_type = "args: string[], loadPlugin: (request: string) => Promise<string>, runRules: (request: RunRulesRequest) => Promise<string>, renderReport: (request: RenderRequest) => Promise<string>, loadJsConfig: (path: string) => Promise<string>",
    ts_return_type = "Promise<RunOutcome>"
)]
pub async fn run_captured(
    args: Vec<String>,
    load_plugin: StringCb,
    run_rules: GraphCb,
    render_report: PairCb,
    load_js_config: StringCb,
) -> napi::Result<RunOutcome> {
    let host = Arc::new(Host::new(Callbacks {
        load_plugin,
        run_rules,
        render_report,
        load_js_config,
    }));
    spawn_blocking(move || {
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let result = runner(args, host).run(&mut out, &mut err);
        RunOutcome {
            exit_code: u32::from(result.code()),
            stdout: String::from_utf8_lossy(&out).into_owned(),
            stderr: String::from_utf8_lossy(&err).into_owned(),
        }
    })
    .await
    .map_err(|e| napi::Error::from_reason(e.to_string()))
}

/// Run a check and return its graph and violations instead of a rendered report.
#[napi(
    ts_args_type = "args: string[], loadPlugin: (request: string) => Promise<string>, runRules: (request: RunRulesRequest) => Promise<string>, renderReport: (request: RenderRequest) => Promise<string>, loadJsConfig: (path: string) => Promise<string>",
    ts_return_type = "Promise<CruiseOutcome>"
)]
pub async fn run_cruise(
    args: Vec<String>,
    load_plugin: StringCb,
    run_rules: GraphCb,
    render_report: PairCb,
    load_js_config: StringCb,
) -> napi::Result<CruiseOutcome> {
    let host = Arc::new(Host::new(Callbacks {
        load_plugin,
        run_rules,
        render_report,
        load_js_config,
    }));
    spawn_blocking(move || {
        let mut err = Vec::new();
        let outcome = runner(args, host).analyze_only(&mut err);
        let stderr = String::from_utf8_lossy(&err).into_owned();
        match outcome {
            Ok(a) => CruiseOutcome {
                exit_code: u32::from(a.errors > 0),
                stderr,
                graph: Some(Buffer::from(graph_buffer::encode(&a.graph, &a.workspace))),
                report: Some(
                    serde_json::json!({
                        "violations": violation_outs(&a.graph, &a.violations),
                        "summary": Summary::new(&a.graph, &a.violations, a.suppressed),
                    })
                    .to_string(),
                ),
            },
            Err(result) => CruiseOutcome {
                exit_code: u32::from(result.code()),
                stderr,
                graph: None,
                report: None,
            },
        }
    })
    .await
    .map_err(|e| napi::Error::from_reason(e.to_string()))
}
