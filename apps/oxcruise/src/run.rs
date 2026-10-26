//! napi entry points, called from `src-js/cli.ts` and the programmatic API.

use std::sync::Arc;

use napi::bindgen_prelude::spawn_blocking;
use napi_derive::napi;

use crate::CliRunner;
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
