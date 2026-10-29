use std::collections::BTreeMap;

use oxcruise_config::Paths;
use oxcruise_core::{EdgeId, ModuleKind};
use oxcruise_macros::declare_rule;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::meta::{Rule, RuleCtx, parse_options};
use crate::pattern::{Captures, PathPatterns, Role};

#[derive(Debug, Default, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct LayersConfig {
    /// Layer name to the paths it contains. Layers should not overlap; if they do the first by name wins.
    pub layers: BTreeMap<String, Paths>,
    /// Layer name to the layers it may import. A layer missing here is unrestricted.
    /// Importing within the same layer is always allowed.
    pub allow: BTreeMap<String, Vec<String>>,
}

declare_rule! {
    /// ### What it does
    ///
    /// Enforces a layered architecture: each layer may import only the layers it lists in `allow`.
    ///
    /// ### Example
    ///
    /// ```json
    /// { "layers": { "ui": "src/ui/**", "domain": "src/domain/**" },
    ///   "allow":  { "ui": ["domain"], "domain": [] } }
    /// ```
    Layers, category = restriction, version = "0.1.0", config = LayersConfig
}

#[derive(Debug, Default)]
pub struct Layers {
    layers: Vec<(String, PathPatterns)>,
    allow: BTreeMap<String, Vec<String>>,
}

impl Layers {
    fn layer_of(&self, path: &str) -> Option<&str> {
        let none = Captures::default();
        self.layers
            .iter()
            .find(|(_, p)| p.is_match(path, &none))
            .map(|(n, _)| n.as_str())
    }
}

impl Rule for Layers {
    fn from_configuration(value: serde_json::Value) -> Result<Self, String> {
        let c: LayersConfig = parse_options("layers", value)?;
        for (from, tos) in &c.allow {
            for name in std::iter::once(from).chain(tos) {
                if !c.layers.contains_key(name) {
                    return Err(format!(
                        "invalid options for `layers`: `allow` refers to unknown layer `{name}`"
                    ));
                }
            }
        }
        let mut layers = vec![];
        for (name, specs) in &c.layers {
            layers.push((
                name.clone(),
                PathPatterns::new(&specs.to_vec(), Role::Referencing)?,
            ));
        }
        Ok(Self {
            layers,
            allow: c.allow,
        })
    }

    fn check_edge(&self, e: EdgeId, cx: &RuleCtx<'_>) {
        let edge = cx.edge(e);
        let (from, to) = (cx.module(edge.from), cx.module(edge.to));
        if to.kind != ModuleKind::Source {
            return;
        }
        let (Some(a), Some(b)) = (self.layer_of(&from.path), self.layer_of(&to.path)) else {
            return;
        };
        if a == b {
            return;
        }
        let Some(allowed) = self.allow.get(a) else {
            return;
        };
        if !allowed.iter().any(|x| x == b) {
            cx.report_edge(
                e,
                format!("layer `{a}` must not import layer `{b}` (`{}`)", to.path),
            );
        }
    }
}
