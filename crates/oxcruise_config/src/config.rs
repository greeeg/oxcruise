use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The level of a rule. `off` disables the rule, `warn` reports violations as warnings, and `error` reports them as errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Off,
    Warn,
    #[default]
    Error,
}

impl Level {
    pub fn to_violation_level(self) -> Option<oxcruise_core::ViolationLevel> {
        use oxcruise_core::ViolationLevel;
        match self {
            Level::Off => None,
            Level::Warn => Some(ViolationLevel::Warn),
            Level::Error => Some(ViolationLevel::Error),
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

/// Reads [`Paths`] into a list, for fields that are lists in Rust.
fn paths<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<String>, D::Error> {
    Ok(Paths::deserialize(d)?.to_vec())
}

/// The setting of a rule: a level, or an object with a `level` and the options of the rule.
/// For example `"error"` or `{ "level": "error", "includeTypeOnly": true }`.
/// In `rules`, an object without `level` means `error`.
/// In `overrides`, an object without `level` keeps the level that the rule has and replaces the options.
#[derive(Debug, Clone, PartialEq)]
pub enum RuleEntry {
    Level(Level),
    Object {
        level: Option<Level>,
        options: serde_json::Map<String, serde_json::Value>,
    },
}

// Not derived: an untagged enum reports "did not match any variant" and loses what was wrong.
impl<'de> Deserialize<'de> for RuleEntry {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        match serde_json::Value::deserialize(d)? {
            serde_json::Value::Array(items) => Err(D::Error::custom(array_form_message(&items))),
            serde_json::Value::Object(mut options) => {
                let level = match options.remove("level") {
                    Some(l) => Some(
                        serde_json::from_value(l)
                            .map_err(|e| D::Error::custom(format!("invalid level: {e}")))?,
                    ),
                    None => None,
                };
                Ok(RuleEntry::Object { level, options })
            }
            other => serde_json::from_value(other)
                .map(RuleEntry::Level)
                .map_err(D::Error::custom),
        }
    }
}

/// The error for `["error", { ... }]`: it shows the same entry as an object.
fn array_form_message(items: &[serde_json::Value]) -> String {
    let mut message = String::from(
        "a rule entry is a level, or an object with a `level` and the options of the rule",
    );
    let object = match items {
        [level] => Some(serde_json::json!({ "level": level })),
        [level, serde_json::Value::Object(options)] => {
            let mut object = serde_json::Map::new();
            object.insert("level".into(), level.clone());
            object.extend(options.clone());
            Some(serde_json::Value::Object(object))
        }
        _ => None,
    };
    if let Some(object) = object {
        let old = serde_json::Value::Array(items.to_vec());
        message.push_str(&format!("; write {object} instead of {old}"));
    }
    message
}

impl Serialize for RuleEntry {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self {
            RuleEntry::Level(l) => l.serialize(s),
            RuleEntry::Object { level, options } => {
                let mut map =
                    s.serialize_map(Some(options.len() + usize::from(level.is_some())))?;
                if let Some(level) = level {
                    map.serialize_entry("level", level)?;
                }
                for (k, v) in options {
                    map.serialize_entry(k, v)?;
                }
                map.end()
            }
        }
    }
}

impl JsonSchema for RuleEntry {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "RuleEntry".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let level = generator.subschema_for::<Level>();
        schemars::json_schema!({
            "description": "The setting of a rule: a level, or an object with a `level` and the options of the rule.",
            "anyOf": [
                level,
                {
                    "type": "object",
                    "properties": { "level": level },
                    "additionalProperties": true
                }
            ]
        })
    }
}

impl RuleEntry {
    /// Split into the level (`None` when the object has no `level`) and the options (`null` when
    /// there are none).
    pub fn parts(&self) -> (Option<Level>, serde_json::Value) {
        match self {
            RuleEntry::Level(l) => (Some(*l), serde_json::Value::Null),
            RuleEntry::Object { level, options } if options.is_empty() => {
                (*level, serde_json::Value::Null)
            }
            RuleEntry::Object { level, options } => {
                (*level, serde_json::Value::Object(options.clone()))
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

/// How oxcruise resolves imports.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct ResolveSection {
    /// The tsconfig for every file. By default, each file uses the nearest tsconfig.
    pub tsconfig: Option<String>,
    /// Export and import condition names. Oxcruise tries them before `import`, `require`, `node` and `default`.
    /// For example, use `["source"]` to resolve workspace packages to their source.
    pub conditions: Vec<String>,
    /// Path aliases. For example `{ "~": "./src" }`.
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
    /// `to` is in the same workspace package as `from`.
    SamePackage,
    /// `to` is in another workspace package than `from`.
    CrossPackage,
}

/// What a dependency points to.
pub use oxcruise_core::ModuleKind as Kind;

/// Conditions on the dependency and the module it points to.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct ToCondition {
    /// Globs that the target of the dependency must match.
    pub path: Option<Paths>,
    /// What the dependency points to. A dependency matches when it has any of these kinds.
    /// By default, every kind matches, except for `reachable` rules, which check only `local` and `package` modules.
    pub kind: Option<OneOrMany<Kind>>,
    /// The dependency is only `import type` or `export type`.
    pub type_only: Option<bool>,
    /// The dependency is a dynamic `import()`.
    pub dynamic: Option<bool>,
    /// How `to` relates to `from`. A dependency matches when it has any of these relations.
    pub relation: Option<OneOrMany<Relation>>,
    /// `to` is reachable from `from` through other modules.
    pub reachable: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum ToSpec {
    /// Globs the dependency's target must match.
    Paths(Paths),
    /// An object with conditions on the dependency and its target.
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

/// A dependency that the project does not allow.
/// An import from a module that matches `from` to a target that matches `to` is a violation.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct ForbidRule {
    /// The name of the rule. Reports and `overrides` use it.
    pub name: String,
    /// Why the dependency is forbidden. Oxcruise shows it instead of the generic message.
    pub message: Option<String>,
    /// The level of the rule.
    #[serde(default)]
    pub level: Level,
    /// Globs of the importing module. `{name}` captures a path segment, and `to` can use the capture.
    pub from: Option<Paths>,
    /// The target of the dependency: globs, or an object with conditions.
    pub to: Option<ToSpec>,
}

// Not derived, to give `severity` a hint: the key was renamed to `level`.
impl<'de> Deserialize<'de> for ForbidRule {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            name: String,
            message: Option<String>,
            #[serde(default)]
            level: Level,
            from: Option<Paths>,
            to: Option<ToSpec>,
        }
        let value = serde_json::Value::deserialize(d)?;
        if value.get("severity").is_some() {
            return Err(D::Error::custom(
                "unknown field `severity`: it was renamed to `level`",
            ));
        }
        let w: Wire = serde_json::from_value(value).map_err(D::Error::custom)?;
        Ok(ForbidRule {
            name: w.name,
            message: w.message,
            level: w.level,
            from: w.from,
            to: w.to,
        })
    }
}

/// Changes the settings of rules for some modules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Override {
    /// Globs of the importing modules that this override applies to.
    #[serde(deserialize_with = "paths")]
    #[schemars(with = "Paths")]
    pub files: Vec<String>,
    /// The rules to change, by name.
    #[serde(default)]
    pub rules: BTreeMap<String, RuleEntry>,
}

/// A JS or TS plugin: a path or package name, or an object that also gives the prefix of its rules.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum JsPlugin {
    /// A path, such as `"./tools/acme.ts"`, or a package name, such as `"acme"`.
    /// For a package name, oxcruise tries `oxcruise-plugin-acme` first and then `acme`.
    Path(String),
    /// An object with the specifier of the plugin and the prefix of its rules.
    Aliased(AliasedPlugin),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AliasedPlugin {
    /// The prefix of the rules: `<name>/<rule>`. It replaces the `meta.name` of the plugin.
    pub name: String,
    /// A path or a package name.
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

/// The oxcruise config file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct OxcruiseConfig {
    /// The URL or path of the JSON schema, for editors.
    #[serde(rename = "$schema")]
    #[schemars(rename = "$schema")]
    pub schema: Option<String>,
    /// Paths to other config files. Their settings are the base, and this file changes them.
    pub extends: Vec<String>,
    /// Files, directories or globs where the graph starts. The default is `["."]`.
    #[serde(deserialize_with = "paths")]
    #[schemars(with = "Paths")]
    pub entries: Vec<String>,
    /// Globs of modules to leave out of the graph. Oxcruise also uses the rules of `.gitignore` when it looks for modules.
    #[serde(deserialize_with = "paths")]
    #[schemars(with = "Paths")]
    pub ignore_patterns: Vec<String>,
    /// Globs of modules that stay in the graph. Oxcruise does not parse their imports.
    #[serde(deserialize_with = "paths")]
    #[schemars(with = "Paths")]
    pub do_not_follow: Vec<String>,
    /// The level of each category of built-in rules. The categories are `correctness`, `suspicious`,
    /// `style`, `restriction` and `nursery`. The default is `correctness: error, suspicious: warn`.
    pub categories: BTreeMap<Category, Level>,
    /// The built-in and plugin rules, by name. A rule listed here wins over its category.
    pub rules: BTreeMap<String, RuleEntry>,
    /// Forbidden dependencies, written as rules.
    pub forbid: Vec<ForbidRule>,
    /// Settings that change rules for some modules.
    pub overrides: Vec<Override>,
    /// How oxcruise resolves imports.
    pub resolve: ResolveSection,
    /// Names of functions whose first string argument is a dependency, for example `jest.mock`.
    pub calls: Vec<String>,
    /// Extract exports and the names that each import uses, for `json` output and plugins.
    /// Rules that need them enable this option.
    pub symbols: bool,
    /// JS or TS plugins: paths or package names.
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
                "forbid": [{"name": "x", "level": "error", "to": {"typeOnly": false, "path": "a"}}]
            })
        );
        // field order follows the struct, not removal order
        let keys: Vec<_> = v["forbid"][0]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        assert_eq!(keys, ["name", "level", "to"]);
    }

    fn entry(json: &str) -> Result<RuleEntry, String> {
        serde_json::from_str(json).map_err(|e| e.to_string())
    }

    #[test]
    fn a_rule_entry_is_a_level_or_an_object() {
        assert_eq!(
            entry(r#""warn""#).unwrap().parts(),
            (Some(Level::Warn), serde_json::Value::Null)
        );
        let (level, options) = entry(r#"{"level":"warn","max":3}"#).unwrap().parts();
        assert_eq!(
            (level, options),
            (Some(Level::Warn), serde_json::json!({"max": 3}))
        );
        // no level: the caller decides (`error` in `rules`, the level so far in an override)
        let (level, options) = entry(r#"{"max":3}"#).unwrap().parts();
        assert_eq!((level, options), (None, serde_json::json!({"max": 3})));
        assert_eq!(
            entry(r#"{"level":"off"}"#).unwrap().parts(),
            (Some(Level::Off), serde_json::Value::Null)
        );
        assert!(
            entry(r#"{"level":"loud"}"#)
                .unwrap_err()
                .contains("invalid level")
        );
        assert!(entry("3").is_err());
    }

    #[test]
    fn a_rule_entry_serialises_flat() {
        for json in [r#""error""#, r#"{"level":"warn","max":3}"#, r#"{"max":3}"#] {
            let e = entry(json).unwrap();
            assert_eq!(serde_json::to_string(&e).unwrap(), json);
        }
    }

    #[test]
    fn the_array_form_is_an_error_that_shows_the_object_form() {
        let e = entry(r#"["error", {"includeTypeOnly": true}]"#).unwrap_err();
        assert!(
            e.contains(r#"write {"includeTypeOnly":true,"level":"error"} instead of ["error",{"includeTypeOnly":true}]"#)
                || e.contains(r#"write {"level":"error","includeTypeOnly":true} instead of ["error",{"includeTypeOnly":true}]"#),
            "{e}"
        );
        assert!(
            entry(r#"["warn"]"#)
                .unwrap_err()
                .contains(r#"write {"level":"warn"}"#)
        );
        assert!(entry("[]").unwrap_err().contains("a rule entry is a level"));
    }

    #[test]
    fn forbid_takes_level_and_hints_at_the_old_severity() {
        let ok: ForbidRule =
            serde_json::from_str(r#"{"name":"x","level":"warn","to":"a"}"#).unwrap();
        assert_eq!(ok.level, Level::Warn);
        let old = serde_json::from_str::<ForbidRule>(r#"{"name":"x","severity":"warn","to":"a"}"#)
            .unwrap_err()
            .to_string();
        assert!(
            old.contains("`severity`") && old.contains("renamed to `level`"),
            "{old}"
        );
        let unknown = serde_json::from_str::<ForbidRule>(r#"{"name":"x","nope":1}"#)
            .unwrap_err()
            .to_string();
        assert!(unknown.contains("unknown field `nope`"), "{unknown}");
    }

    #[test]
    fn every_glob_list_takes_one_glob() {
        let one: OxcruiseConfig = serde_json::from_str(
            r#"{"entries":"src","ignorePatterns":"x/**","doNotFollow":"y/**",
                "overrides":[{"files":"src/legacy/**","rules":{}}]}"#,
        )
        .unwrap();
        let many: OxcruiseConfig = serde_json::from_str(
            r#"{"entries":["src"],"ignorePatterns":["x/**"],"doNotFollow":["y/**"],
                "overrides":[{"files":["src/legacy/**"],"rules":{}}]}"#,
        )
        .unwrap();
        assert_eq!(one, many);
        assert_eq!(one.ignore_patterns, ["x/**"]);
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
            "symbols",
            "jsPlugins",
        ] {
            assert!(props.contains_key(k), "schema lacks {k}");
        }
        assert_eq!(props.len(), 13, "unexpected extra keys: {props:?}");
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
