//! The JSON schema of the config file, with the options of each built-in rule typed.

use serde_json::{Value, json};

use crate::registry::{builtin_option_schemas, builtin_rules};

/// The schema of `OxcruiseConfig`, where `rules` (and `overrides[].rules`) has one property per
/// built-in rule: a level, or an object with a `level` and the options of that rule. Plugin rules
/// keep the generic entry.
pub fn config_json_schema() -> Value {
    let mut root = serde_json::to_value(oxcruise_config::json_schema()).expect("schema serialises");
    let level = json!({ "$ref": "#/$defs/Level" });
    let mut rules = serde_json::Map::new();
    let mut defs = std::mem::take(
        root["$defs"]
            .as_object_mut()
            .expect("the config schema has $defs"),
    );
    let summaries: Vec<(&str, String)> = builtin_rules()
        .into_iter()
        .map(|r| (r.name, summary(r.docs)))
        .collect();
    for (name, options) in builtin_option_schemas() {
        let mut object = options.unwrap_or_else(
            || json!({ "type": "object", "properties": {}, "additionalProperties": false }),
        );
        if let Some(own) = object.as_object_mut().and_then(|o| o.remove("$defs")) {
            for (k, v) in own.as_object().into_iter().flatten() {
                match defs.get(k) {
                    Some(existing) => {
                        assert_eq!(existing, v, "`{k}` is defined twice, differently")
                    }
                    None => {
                        defs.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        object.as_object_mut().map(|o| o.remove("description"));
        if let Some(o) = object.as_object_mut() {
            o.remove("$schema");
            o.remove("title");
            o.remove("description");
            o.entry("properties").or_insert_with(|| json!({}));
            o["properties"]["level"] = level.clone();
        }
        let summary = &summaries
            .iter()
            .find(|(n, _)| *n == name)
            .expect("a rule")
            .1;
        let entry = json!({ "description": summary, "anyOf": [level, object] });
        rules.insert(name.to_string(), entry);
    }
    defs.insert(
        "Rules".into(),
        json!({
            "description": "The built-in and plugin rules, by name. A rule listed here wins over its category.",
            "type": "object",
            "properties": rules,
            "additionalProperties": { "$ref": "#/$defs/RuleEntry" },
            "default": {}
        }),
    );
    root["$defs"] = Value::Object(defs);
    root["properties"]["rules"] = json!({ "$ref": "#/$defs/Rules" });
    root["$defs"]["Override"]["properties"]["rules"] = json!({ "$ref": "#/$defs/Rules" });
    sort_keys(&mut root);
    root
}

/// The first paragraph of the "What it does" section of the docs of a rule.
fn summary(docs: &str) -> String {
    docs.split("### What it does")
        .nth(1)
        .and_then(|section| section.trim().split("\n\n").next())
        .unwrap_or_default()
        .replace('\n', " ")
}

/// Object keys in alphabetical order, so the schema does not depend on the features that other
/// crates enable in `serde_json`.
fn sort_keys(v: &mut Value) {
    match v {
        Value::Object(map) => {
            let mut entries: Vec<(String, Value)> = std::mem::take(map).into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            for (k, mut v) in entries {
                sort_keys(&mut v);
                map.insert(k, v);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(sort_keys),
        _ => {}
    }
}
