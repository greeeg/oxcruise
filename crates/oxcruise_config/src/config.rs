use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Severity of a rule. `off` disables it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    #[serde(alias = "ignore")]
    Off,
    Info,
    Warn,
    #[default]
    Error,
}

impl Level {
    pub fn to_severity(self) -> Option<oxcruise_core::Severity> {
        use oxcruise_core::Severity;
        match self {
            Level::Off => None,
            Level::Info => Some(Severity::Info),
            Level::Warn => Some(Severity::Warn),
            Level::Error => Some(Severity::Error),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum OneOrMany<T> {
    One(T),
    Many(Vec<T>),
}

impl<T: Clone> OneOrMany<T> {
    pub fn to_vec(&self) -> Vec<T> {
        match self {
            OneOrMany::One(t) => vec![t.clone()],
            OneOrMany::Many(v) => v.clone(),
        }
    }
}

/// A path pattern: a glob string, or `{ "regex": "..." }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PathSpec {
    Glob(String),
    Regex(RegexSpec),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RegexSpec {
    /// A regular expression (Rust `regex` syntax, linear time).
    pub regex: String,
}

impl PathSpec {
    pub fn glob(s: impl Into<String>) -> Self {
        PathSpec::Glob(s.into())
    }
    pub fn regex(s: impl Into<String>) -> Self {
        PathSpec::Regex(RegexSpec { regex: s.into() })
    }
}

/// A rule entry: `"error"`, or `["error", { ...options }]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum RuleEntry {
    Level(Level),
    WithOptions(Vec<serde_json::Value>),
}

impl RuleEntry {
    /// Split into severity and options. Errors name what is wrong.
    pub fn parts(&self) -> Result<(Level, serde_json::Value), String> {
        match self {
            RuleEntry::Level(l) => Ok((*l, serde_json::Value::Null)),
            RuleEntry::WithOptions(v) => {
                let level = v
                    .first()
                    .ok_or("rule entry array must start with a severity")?;
                let level: Level = serde_json::from_value(level.clone())
                    .map_err(|e| format!("invalid severity {level}: {e}"))?;
                if v.len() > 2 {
                    return Err("rule entry takes a severity and at most one options object".into());
                }
                Ok((level, v.get(1).cloned().unwrap_or(serde_json::Value::Null)))
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct FollowConfig {
    /// Parse files in `node_modules` instead of keeping them as leaves.
    pub npm: bool,
    /// Keep matching modules in the graph but do not explore their dependencies.
    pub do_not_follow: Vec<PathSpec>,
    /// Explore at most this many hops from the entries.
    pub max_depth: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum TsconfigSetting {
    /// `"auto"` (nearest tsconfig per file) or `"off"`.
    Mode(TsconfigMode),
    /// A path to one tsconfig used for every file.
    Path(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TsconfigMode {
    Auto,
    Off,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct ResolveSection {
    pub tsconfig: Option<TsconfigSetting>,
    /// Extra export/import condition names, added to `import`, `require`, `node`, `default`.
    pub conditions: Vec<String>,
    pub main_fields: Vec<String>,
    /// Explicit extension list, e.g. `[".ts", ".tsx", ".js"]`.
    pub extensions: Vec<String>,
    /// Aliases: `{ "~": "./src" }`.
    pub alias: BTreeMap<String, OneOrMany<String>>,
    /// Resolve workspace packages to their source (adds the `source` condition first).
    pub prefer_source: bool,
    /// Condition names used by `preferSource`. Defaults to `["source"]`.
    pub source_conditions: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct ExtractSection {
    /// Callee names treated like `require`.
    pub exotic_require_strings: Vec<String>,
    /// Callee names whose first string argument is a dependency, e.g. `jest.mock`.
    pub calls: Vec<String>,
    /// Detect `@import` and `import("x")` in JSDoc comments.
    pub jsdoc: bool,
    /// Skip `/// <reference ... />` directives.
    pub skip_triple_slash: bool,
    /// Do not treat `new URL("./x", import.meta.url)` and workers as dependencies.
    pub skip_urls: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct CycleOptions {
    /// Drop type-only edges before looking for cycles (runtime-only cycles).
    pub ignore_type_only: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct Options {
    /// Fail when there are more warnings than this.
    pub max_warnings: Option<u32>,
    /// Warn about rules whose `from`/`to` matched nothing.
    pub report_dead_rules: bool,
    pub cycles: CycleOptions,
    /// Honour `.gitignore` while walking entries.
    pub respect_gitignore: bool,
}

/// How two modules relate, for the `relation` condition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Relation {
    /// `to` is in the same folder as `from`.
    SameFolder,
    /// `to` is below the folder of `from`.
    DescendantOfFrom,
    /// `to` is below the parent folder of `from`.
    DescendantOfFromParent,
    /// `to` is in a sibling folder of `from`'s folder.
    Sibling,
    SamePackage,
    CrossPackage,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum RelationSpec {
    /// Any of these relations holds.
    Any(Vec<Relation>),
    Spec(RelationObj),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(deny_unknown_fields, default)]
pub struct RelationObj {
    pub any: Vec<Relation>,
    /// None of these relations holds.
    pub not: Vec<Relation>,
}

/// Conditions on the importing module.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct FromCondition {
    pub path: Option<OneOrMany<PathSpec>>,
    pub path_not: Option<OneOrMany<PathSpec>>,
    /// No dependencies in or out.
    pub orphan: Option<bool>,
    pub number_of_dependents_less_than: Option<u32>,
    pub number_of_dependents_more_than: Option<u32>,
}

/// A module or edge on a cycle or path: module predicates plus edge predicates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct ViaCondition {
    pub path: Option<OneOrMany<PathSpec>>,
    pub path_not: Option<OneOrMany<PathSpec>>,
    pub dependency_types: Vec<String>,
    pub dependency_types_not: Vec<String>,
    pub type_only: Option<bool>,
    pub dynamic: Option<bool>,
}

/// Conditions on the dependency and the module it points to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct ToCondition {
    pub path: Option<OneOrMany<PathSpec>>,
    pub path_not: Option<OneOrMany<PathSpec>>,
    pub relation: Option<RelationSpec>,
    pub orphan: Option<bool>,
    pub dependency_types: Vec<String>,
    pub dependency_types_not: Vec<String>,
    pub more_than_one_dependency_type: Option<bool>,
    pub could_not_resolve: Option<bool>,
    pub dynamic: Option<bool>,
    pub type_only: Option<bool>,
    pub exotically_required: Option<bool>,
    /// The dependency is part of a cycle.
    pub circular: Option<bool>,
    /// The cycle goes through a module (other than the importer) or an edge matching this.
    pub via: Option<ViaCondition>,
    /// Every module (importer included) and every edge of the cycle matches this.
    pub via_only: Option<ViaCondition>,
    /// `to` is reachable from `from` through other modules.
    pub reachable: Option<bool>,
    pub number_of_dependents_less_than: Option<u32>,
    pub number_of_dependents_more_than: Option<u32>,
}

/// A path condition given as a glob string, or the full object form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum FromSpec {
    Path(String),
    Cond(FromCondition),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ToSpec {
    Path(String),
    Cond(Box<ToCondition>),
}

/// An ad-hoc rule: dependencies from `from` to `to` are violations (`forbid`)
/// or the only permitted ones (`allow`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DeclarativeRule {
    pub name: String,
    pub comment: Option<String>,
    #[serde(default)]
    pub severity: Level,
    pub from: Option<FromSpec>,
    pub to: Option<ToSpec>,
}

/// A requirement: every `module` must have a dependency matching `to`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RequireRule {
    pub name: String,
    pub comment: Option<String>,
    #[serde(default)]
    pub severity: Level,
    pub module: FromSpec,
    pub to: ToSpec,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Override {
    /// Globs of the importing module this override applies to.
    pub files: Vec<String>,
    #[serde(default)]
    pub rules: BTreeMap<String, RuleEntry>,
}

/// A JS/TS plugin: a path or package name, or the same with the name its rules are registered under.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum JsPlugin {
    /// `"./tools/acme.ts"` or `"acme"` (resolved as `oxcruise-plugin-acme`, then `acme`).
    Path(String),
    Aliased(AliasedPlugin),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AliasedPlugin {
    /// Rules are named `<name>/<rule>`, instead of using the plugin's own `meta.name`.
    pub name: String,
    pub specifier: String,
}

impl JsPlugin {
    pub fn specifier(&self) -> &str {
        match self {
            JsPlugin::Path(p) => p,
            JsPlugin::Aliased(a) => &a.specifier,
        }
    }
    pub fn alias(&self) -> Option<&str> {
        match self {
            JsPlugin::Path(_) => None,
            JsPlugin::Aliased(a) => Some(&a.name),
        }
    }
}

/// The oxcruise configuration file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct OxcruiseConfig {
    #[serde(rename = "$schema")]
    #[schemars(rename = "$schema")]
    pub schema: Option<String>,
    /// Presets (`oxcruise:recommended`, `oxcruise:strict`) or paths to other config files.
    pub extends: Vec<String>,
    /// Files, directories or globs to start from.
    pub entries: Vec<String>,
    pub exclude: Vec<PathSpec>,
    pub include_only: Vec<PathSpec>,
    pub follow: FollowConfig,
    pub resolve: ResolveSection,
    pub extract: ExtractSection,
    /// Built-in and plugin rules by name.
    pub rules: BTreeMap<String, RuleEntry>,
    /// Ad-hoc forbidden dependencies.
    pub forbid: Vec<DeclarativeRule>,
    /// If non-empty, every dependency must match one of these.
    pub allow: Vec<DeclarativeRule>,
    pub require: Vec<RequireRule>,
    pub overrides: Vec<Override>,
    /// JS/TS plugin modules (paths or package names).
    pub js_plugins: Vec<JsPlugin>,
    pub options: Options,
}

pub fn json_schema() -> schemars::Schema {
    schemars::schema_for!(OxcruiseConfig)
}

impl OxcruiseConfig {
    /// JSON with everything that equals its default removed, for writing config files.
    pub fn to_minimal_json(&self) -> serde_json::Value {
        let mut v = serde_json::to_value(self).expect("config serialises");
        let default = serde_json::to_value(OxcruiseConfig::default()).expect("default serialises");
        prune(&mut v, Some(&default));
        v
    }
}

/// Returns `true` if `v` should be removed from its parent.
fn prune(v: &mut serde_json::Value, default: Option<&serde_json::Value>) -> bool {
    use serde_json::Value;
    match v {
        Value::Object(map) => {
            // Rebuild rather than `remove`: with `preserve_order`, removal reorders keys.
            let old = std::mem::take(map);
            for (k, mut v) in old {
                if !prune(&mut v, default.and_then(|d| d.get(&k))) {
                    map.insert(k, v);
                }
            }
            map.is_empty()
        }
        Value::Array(items) => {
            for item in items.iter_mut() {
                prune(item, None);
            }
            items.is_empty()
        }
        Value::Null => true,
        other => default == Some(&*other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_json_drops_defaults_but_keeps_explicit_false() {
        let cfg: OxcruiseConfig = serde_json::from_str(
            r#"{"entries":["src"],"forbid":[{"name":"x","to":{"circular":false,"path":"a"}}],"follow":{"npm":false}}"#,
        )
        .unwrap();
        let v = cfg.to_minimal_json();
        assert_eq!(
            v,
            serde_json::json!({
                "entries": ["src"],
                "forbid": [{"name": "x", "severity": "error", "to": {"circular": false, "path": "a"}}]
            })
        );
        // field order follows the struct, not removal order
        let keys: Vec<_> = v["forbid"][0]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(keys, ["name", "severity", "to"]);
    }

    #[test]
    fn schema_has_the_main_sections() {
        let s = serde_json::to_value(json_schema()).unwrap();
        let props = s["properties"].as_object().unwrap();
        for k in [
            "$schema",
            "extends",
            "entries",
            "rules",
            "forbid",
            "allow",
            "require",
            "overrides",
            "jsPlugins",
            "options",
        ] {
            assert!(props.contains_key(k), "schema lacks {k}");
        }
    }
}
