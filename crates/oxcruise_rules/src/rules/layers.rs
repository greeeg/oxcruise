use std::collections::BTreeMap;

use oxcruise_config::Paths;
use oxcruise_core::{EdgeId, ModuleKind};
use oxcruise_macros::declare_rule;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::meta::{Hooks, Rule, RuleCtx, RuleMeta, parse_options};
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
    /// Enforces a layered architecture: a layer can import only the layers that `allow` lists for it.
    /// The rule checks dependencies on local modules. Packages and built-in modules have no layer.
    ///
    /// ### Options
    ///
    /// * `layers`: maps the name of a layer to the globs of its modules. When layers overlap, the first layer by name wins.
    /// * `allow`: maps the name of a layer to the layers that it can import.
    ///   A layer that `allow` does not list has no limit.
    ///   A module can always import a module in its own layer.
    ///   A module in no layer has no limit.
    ///   If `allow` names a layer that `layers` does not define, the config is not valid.
    ///
    /// ### Example
    ///
    /// The options of the rule:
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
    const HOOKS: Hooks = Hooks::EDGE;

    fn from_configuration(value: serde_json::Value) -> Result<Self, String> {
        let c: LayersConfig = parse_options(Self::NAME, value)?;
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
        if to.kind != ModuleKind::Local {
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
