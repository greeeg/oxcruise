//! The Node side of the plugin system, reached through napi `ThreadsafeFunction`s.
//!
//! Everything here is called from the *blocking* thread that runs the synchronous
//! `CliRunner`, never from a rayon worker or the JS thread, so `block_on` on a JS promise
//! cannot deadlock the event loop.

use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use napi::Status;
use napi::bindgen_prelude::{Buffer, Promise, block_on};
use napi::threadsafe_function::ThreadsafeFunction;
use oxcruise_config::{ConfigError, JsConfigLoader};
use oxcruise_core::{EdgeId, Graph, ModuleId};
use oxcruise_graph::analyses::ExportUsage;
use oxcruise_report::ExternalReporters;
use oxcruise_resolve::WorkspacePackage;
use oxcruise_rules::{
    AnalysisCache, ExternalRuleId, ExternalRuleSpec, ExternalRules, ExternalTarget,
    ExternalViolation, PluginRef,
};
use serde::Deserialize;

use super::graph_buffer;

/// `(string) -> Promise<string>`
pub type StringCb = ThreadsafeFunction<String, Promise<String>, String, Status, false>;
/// `({ graph, rules }) -> Promise<string>`
pub type GraphCb =
    ThreadsafeFunction<RunRulesRequest, Promise<String>, RunRulesRequest, Status, false>;
/// `({ name, resultJson }) -> Promise<string>`
pub type PairCb = ThreadsafeFunction<RenderRequest, Promise<String>, RenderRequest, Status, false>;

/// Everything the plugin rules need, sent once per run.
#[napi_derive::napi(object)]
pub struct RunRulesRequest {
    /// The graph in the layout of `graph_buffer`.
    pub graph: Buffer,
    /// JSON `[{ id, name, severity, options }]`.
    pub rules: String,
}

#[napi_derive::napi(object)]
pub struct RenderRequest {
    /// `plugin/reporter`
    pub name: String,
    /// The native `json` result.
    pub result_json: String,
}

/// The four functions `src-js/cli.ts` hands to `run()`.
pub struct Callbacks {
    pub load_plugin: StringCb,
    pub run_rules: GraphCb,
    pub render_report: PairCb,
    pub load_js_config: StringCb,
}

#[derive(Debug, Deserialize)]
struct LoadedPlugin {
    name: String,
    #[serde(default)]
    rules: Vec<String>,
    #[serde(default)]
    reporters: Vec<String>,
    /// Globs of modules the plugin declares as entry points.
    #[serde(default)]
    entries: Vec<String>,
    /// The plugin reads exports and used names (`needs: ["symbols"]`).
    #[serde(default)]
    symbols: bool,
}

pub struct Host {
    cb: Callbacks,
    plugins: Mutex<Vec<LoadedPlugin>>,
    /// The graph as last sent to the plugin rules, for `cruise()` to hand back to JS instead of
    /// encoding it again. `(modules, edges)` guards against a different graph.
    encoded: Mutex<Option<((usize, usize), Vec<u8>)>>,
    /// Keep a copy of the buffer in `encoded`; only `cruise()` reuses it.
    keep_encoded: AtomicBool,
}

impl Host {
    pub fn new(cb: Callbacks) -> Self {
        Self {
            cb,
            plugins: Mutex::new(vec![]),
            encoded: Mutex::new(None),
            keep_encoded: AtomicBool::new(false),
        }
    }

    /// Remember the buffer sent to plugin rules, for `take_encoded`.
    pub fn keep_encoded_graph(&self) {
        self.keep_encoded.store(true, Ordering::Relaxed);
    }

    /// The buffer of `graph` that plugin rules were given, if they ran on this graph.
    pub fn take_encoded(&self, graph: &Graph) -> Option<Vec<u8>> {
        let (shape, bytes) = self.encoded.lock().expect("poisoned").take()?;
        (shape == (graph.module_count(), graph.edge_count())).then_some(bytes)
    }
}

fn js_err(e: napi::Error) -> String {
    e.reason.clone()
}

impl ExternalRules for Host {
    fn load_plugins(&self, plugins: &[PluginRef], cwd: &Path) -> Result<(), String> {
        for plugin in plugins {
            let spec = &plugin.specifier;
            let request =
                serde_json::json!({ "specifier": spec, "alias": plugin.alias, "cwd": cwd })
                    .to_string();
            let reply = block_on(async {
                let promise = self.cb.load_plugin.call_async(request).await?;
                promise.await
            })
            .map_err(|e| format!("loading plugin `{spec}`: {}", js_err(e)))?;
            let plugin: LoadedPlugin = serde_json::from_str(&reply)
                .map_err(|e| format!("plugin `{spec}` returned invalid metadata: {e}"))?;
            self.plugins.lock().expect("poisoned").push(plugin);
        }
        Ok(())
    }

    fn entries(&self) -> Vec<String> {
        let plugins = self.plugins.lock().expect("poisoned");
        plugins
            .iter()
            .flat_map(|p| p.entries.iter().cloned())
            .collect()
    }

    fn needs_symbols(&self) -> bool {
        self.plugins
            .lock()
            .expect("poisoned")
            .iter()
            .any(|p| p.symbols)
    }

    fn rule_exists(&self, name: &str) -> bool {
        self.plugins
            .lock()
            .expect("poisoned")
            .iter()
            .any(|p| p.rules.iter().any(|r| is_qualified(name, &p.name, r)))
    }

    fn run(
        &self,
        graph: &Graph,
        workspace: &[WorkspacePackage],
        rules: &[ExternalRuleSpec],
        usage: Option<&ExportUsage>,
        analyses: &AnalysisCache,
    ) -> Result<Vec<ExternalViolation>, String> {
        let bytes = graph_buffer::encode_with(graph, workspace, usage, analyses);
        let shape = (graph.module_count(), graph.edge_count());
        if self.keep_encoded.load(Ordering::Relaxed) {
            *self.encoded.lock().expect("poisoned") = Some((shape, bytes.clone()));
        }
        let buffer = Buffer::from(bytes);
        let specs: Vec<_> = rules
            .iter()
            .map(|r| {
                serde_json::json!({
                    "id": r.id.0,
                    "name": r.name,
                    "severity": r.severity.as_str(),
                    "options": r.options,
                })
            })
            .collect();
        let reply = block_on(async {
            let promise = self
                .cb
                .run_rules
                .call_async(RunRulesRequest {
                    graph: buffer,
                    rules: serde_json::Value::Array(specs).to_string(),
                })
                .await?;
            promise.await
        })
        .map_err(js_err)?;
        parse_violations(&reply, rules, graph)
    }
}

/// Is `name` the plugin's `plugin/item`?
fn is_qualified(name: &str, plugin: &str, item: &str) -> bool {
    name.strip_prefix(plugin)
        .and_then(|rest| rest.strip_prefix('/'))
        == Some(item)
}

#[derive(Deserialize)]
struct RawViolation {
    id: u32,
    module: Option<u32>,
    /// With `module`: the index of the export in the module's exports.
    export: Option<u32>,
    edge: Option<u32>,
    message: String,
}

/// `null` (no violations) or `[{ id, module | edge, message }]`.
fn parse_violations(
    reply: &str,
    rules: &[ExternalRuleSpec],
    graph: &Graph,
) -> Result<Vec<ExternalViolation>, String> {
    let raw: Option<Vec<RawViolation>> = serde_json::from_str(reply)
        .map_err(|e| format!("plugin host returned invalid violations: {e}"))?;
    let mut out = vec![];
    for v in raw.unwrap_or_default() {
        if !rules.iter().any(|r| r.id.0 == v.id) {
            return Err(format!(
                "plugin host reported a violation for unknown rule id {}",
                v.id
            ));
        }
        let target = match (v.module, v.export, v.edge) {
            (Some(m), None, None) if (m as usize) < graph.module_count() => {
                ExternalTarget::Module(ModuleId(m))
            }
            (Some(m), Some(x), None)
                if (m as usize) < graph.module_count()
                    && (x as usize) < graph.module(ModuleId(m)).exports.len() =>
            {
                ExternalTarget::Export(ModuleId(m), x)
            }
            (None, None, Some(e)) if (e as usize) < graph.edge_count() => {
                ExternalTarget::Edge(EdgeId(e))
            }
            _ => {
                return Err(
                    "plugin violation must name exactly one existing module, export or dependency"
                        .into(),
                );
            }
        };
        out.push(ExternalViolation {
            rule: ExternalRuleId(v.id),
            target,
            message: v.message,
        });
    }
    Ok(out)
}

impl ExternalReporters for Host {
    fn render(&self, name: &str, result_json: &str) -> Result<String, String> {
        let known = self
            .plugins
            .lock()
            .expect("poisoned")
            .iter()
            .any(|p| p.reporters.iter().any(|r| is_qualified(name, &p.name, r)));
        if !known {
            return Err(format!(
                "no loaded plugin provides the reporter `{name}`; is its plugin listed in `jsPlugins`?"
            ));
        }
        block_on(async {
            let promise = self
                .cb
                .render_report
                .call_async(RenderRequest {
                    name: name.to_string(),
                    result_json: result_json.to_string(),
                })
                .await?;
            promise.await
        })
        .map_err(js_err)
    }
}

/// Evaluates JS/TS configs inside the host Node process, so `import { defineConfig } from "oxcruise"`
/// and TS syntax work without spawning a second Node.
pub struct HostConfigLoader<'a>(pub &'a Host);

impl JsConfigLoader for HostConfigLoader<'_> {
    fn load(&self, path: &Path) -> Result<serde_json::Value, ConfigError> {
        let reply = block_on(async {
            let promise = self
                .0
                .cb
                .load_js_config
                .call_async(path.to_string_lossy().into_owned())
                .await?;
            promise.await
        })
        .map_err(|e| ConfigError::Js {
            path: path.to_path_buf(),
            message: js_err(e),
        })?;
        serde_json::from_str(&reply).map_err(|e| ConfigError::Js {
            path: path.to_path_buf(),
            message: format!("config did not serialise to JSON: {e}"),
        })
    }
}
