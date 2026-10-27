use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Severity of a rule. `off` disables it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Off,
    Warn,
    #[default]
    Error,
}

impl Level {
    pub fn to_severity(self) -> Option<oxcruise_core::Severity> {
        use oxcruise_core::Severity;
        match self {
            Level::Off => None,
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

/// Globs: one string or a list. A leading `!` negates a glob.
pub type Paths = OneOrMany<String>;

/// A rule entry: `"error"`, or `["error", { ...options }]`.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum RuleEntry {
    Level(Level),
    WithOptions(Vec<serde_json::Value>),
}

// Not derived: an untagged enum reports "did not match any variant" and loses what was wrong.
impl<'de> Deserialize<'de> for RuleEntry {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        match serde_json::Value::deserialize(d)? {
            serde_json::Value::Array(items) => Ok(RuleEntry::WithOptions(items)),
            other => serde_json::from_value(other)
                .map(RuleEntry::Level)
                .map_err(D::Error::custom),
        }
    }
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

/// Rule groups, switched on or off together. Same names as oxlint.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    /// Code that is almost certainly wrong.
    Correctness,
    /// Probably wrong or useless.
    Suspicious,
    Style,
    /// Forbids a feature or pattern; opt-in.
    Restriction,
    Nursery,
}

impl Category {
    pub const ALL: [Category; 5] = [
        Category::Correctness,
        Category::Suspicious,
        Category::Style,
        Category::Restriction,
        Category::Nursery,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Category::Correctness => "correctness",
            Category::Suspicious => "suspicious",
            Category::Style => "style",
            Category::Restriction => "restriction",
            Category::Nursery => "nursery",
        }
    }

    /// The level a category has when the config does not mention it.
    pub fn default_level(self) -> Level {
        match self {
            Category::Correctness => Level::Error,
            Category::Suspicious => Level::Warn,
            Category::Style | Category::Restriction | Category::Nursery => Level::Off,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct ResolveSection {
    /// One tsconfig used for every file. Default: the nearest tsconfig of each file.
    pub tsconfig: Option<String>,
    /// Export/import condition names tried before `import`/`require`, `node` and `default`,
    /// e.g. `["source"]` to resolve workspace packages to their source.
    pub conditions: Vec<String>,
    /// Aliases: `{ "~": "./src" }`.
    pub alias: BTreeMap<String, OneOrMany<String>>,
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

/// What a dependency points to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// A file of the project.
    Local,
    /// A file inside `node_modules`.
    Package,
    /// A Node.js built-in module.
    Builtin,
    /// An import that could not be resolved.
    Unresolved,
}

/// Conditions on the dependency and the module it points to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct ToCondition {
    pub path: Option<Paths>,
    /// What the dependency points to; any of these.
    pub kind: Option<OneOrMany<Kind>>,
    /// The dependency is `import type` / `export type` only.
    pub type_only: Option<bool>,
    /// The dependency is a dynamic `import()`.
    pub dynamic: Option<bool>,
    /// How `to` relates to `from`; any of these.
    pub relation: Option<OneOrMany<Relation>>,
    /// `to` is reachable from `from` through other modules.
    pub reachable: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ToSpec {
    /// Globs the dependency's target must match.
    Paths(Paths),
    Cond(ToCondition),
}

// Not derived, for the same reason as `RuleEntry`: name the unknown field in `to`.
impl<'de> Deserialize<'de> for ToSpec {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        match serde_json::Value::deserialize(d)? {
            v @ serde_json::Value::Object(_) => serde_json::from_value(v)
                .map(ToSpec::Cond)
                .map_err(D::Error::custom),
            other => serde_json::from_value(other)
                .map(ToSpec::Paths)
                .map_err(D::Error::custom),
        }
    }
}

/// A forbidden dependency: an import from `from` to `to` is a violation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ForbidRule {
    pub name: String,
    /// Why the dependency is forbidden; shown instead of the generic message.
    pub message: Option<String>,
    #[serde(default)]
    pub severity: Level,
    /// Globs of the importing module. `{name}` captures a path segment for use in `to`.
    pub from: Option<Paths>,
    pub to: Option<ToSpec>,
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
    /// Paths to other config files, merged underneath this one.
    pub extends: Vec<String>,
    /// Files, directories or globs to start from. Default: `["."]`.
    pub entries: Vec<String>,
    /// Modules matching these globs are not part of the graph. `.gitignore` is always honoured.
    pub ignore_patterns: Vec<String>,
    /// Matching modules stay in the graph, but their imports are not parsed.
    pub do_not_follow: Vec<String>,
    /// Switch groups of built-in rules on or off. Default: `correctness: error, suspicious: warn`.
    pub categories: BTreeMap<Category, Level>,
    /// Built-in and plugin rules by name. A rule listed here wins over its category.
    pub rules: BTreeMap<String, RuleEntry>,
    /// Ad-hoc forbidden dependencies.
    pub forbid: Vec<ForbidRule>,
    pub overrides: Vec<Override>,
    pub resolve: ResolveSection,
    /// Callee names whose first string argument is a dependency, e.g. `jest.mock`.
    pub calls: Vec<String>,
    /// JS/TS plugin modules (paths or package names).
    pub js_plugins: Vec<JsPlugin>,
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
            r#"{"entries":["src"],"forbid":[{"name":"x","to":{"typeOnly":false,"path":"a"}}]}"#,
        )
        .unwrap();
        let v = cfg.to_minimal_json();
        assert_eq!(
            v,
            serde_json::json!({
                "entries": ["src"],
                "forbid": [{"name": "x", "severity": "error", "to": {"typeOnly": false, "path": "a"}}]
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
            "ignorePatterns",
            "doNotFollow",
            "categories",
            "rules",
            "forbid",
            "overrides",
            "resolve",
            "calls",
            "jsPlugins",
        ] {
            assert!(props.contains_key(k), "schema lacks {k}");
        }
        assert_eq!(props.len(), 12, "unexpected extra keys: {props:?}");
    }

    #[test]
    fn removed_keys_are_rejected_by_name() {
        for (json, key) in [
            (r#"{"options":{}}"#, "options"),
            (r#"{"follow":{}}"#, "follow"),
            (r#"{"exclude":[]}"#, "exclude"),
            (r#"{"includeOnly":[]}"#, "includeOnly"),
            (r#"{"allow":[]}"#, "allow"),
            (r#"{"require":[]}"#, "require"),
            (r#"{"extract":{}}"#, "extract"),
            (r#"{"resolve":{"mainFields":[]}}"#, "mainFields"),
            (r#"{"resolve":{"preferSource":true}}"#, "preferSource"),
            (r#"{"forbid":[{"name":"x","comment":"c"}]}"#, "comment"),
            (
                r#"{"forbid":[{"name":"x","to":{"circular":true}}]}"#,
                "circular",
            ),
            (r#"{"rules":{"a":"info"}}"#, "info"),
        ] {
            let err = serde_json::from_str::<OxcruiseConfig>(json)
                .unwrap_err()
                .to_string();
            assert!(err.contains(key), "{json}: {err}");
        }
    }
}
