//! Importer for `.dependency-cruiser.*` configuration.
//!
//! The mapping is deliberately conservative: regexes are kept as regexes,
//! conditions map one-to-one, and anything that cannot be honoured is reported
//! as a named warning. A rule that uses an unsupported *condition* is skipped
//! entirely, because dropping the condition would silently widen the rule.

use std::path::Path;

use serde_json::Value;

use crate::{
    DeclarativeRule, FromCondition, FromSpec, Level, OneOrMany, OxcruiseConfig, PathSpec,
    RequireRule, ToCondition, ToSpec, TsconfigSetting, ViaCondition,
};

pub fn looks_like_depcruise(path: &Path, v: &Value) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.starts_with(".dependency-cruiser") {
        return true;
    }
    let Some(o) = v.as_object() else { return false };
    !o.contains_key("rules")
        && ["forbidden", "allowed", "required"]
            .iter()
            .any(|k| o.contains_key(*k))
}

/// Convert a depcruise config object. Returns the config and one warning per
/// thing that did not map.
pub fn import(v: &Value) -> (OxcruiseConfig, Vec<String>) {
    let mut cx = Importer { warnings: vec![] };
    let mut cfg = OxcruiseConfig::default();
    let Some(obj) = v.as_object() else {
        cx.warn("config is not an object");
        return (cfg, cx.warnings);
    };

    for (key, val) in obj {
        match key.as_str() {
            "forbidden" => {
                for r in val.as_array().into_iter().flatten() {
                    if let Some(rule) = cx.rule(r, "forbidden") {
                        cfg.forbid.push(rule);
                    }
                }
            }
            "allowed" => {
                let sev = obj
                    .get("allowedSeverity")
                    .and_then(Value::as_str)
                    .map_or(Level::Warn, level);
                for r in val.as_array().into_iter().flatten() {
                    if let Some(mut rule) = cx.rule(r, "allowed") {
                        rule.severity = sev;
                        if r.get("name").is_none() {
                            rule.name = "not-in-allowed".into();
                        }
                        cfg.allow.push(rule);
                    }
                }
            }
            "allowedSeverity" => {}
            "required" => {
                for r in val.as_array().into_iter().flatten() {
                    if let Some(rule) = cx.required(r) {
                        cfg.require.push(rule);
                    }
                }
            }
            "options" => cx.options(val, &mut cfg),
            "extends" => cx.extends(val, &mut cfg),
            "$schema" => {}
            other => cx.warn(format!(
                "top-level key `{other}` is not supported and was ignored"
            )),
        }
    }
    (cfg, cx.warnings)
}

fn level(s: &str) -> Level {
    match s {
        "error" => Level::Error,
        "warn" => Level::Warn,
        "info" => Level::Info,
        _ => Level::Off,
    }
}

struct Importer {
    warnings: Vec<String>,
}

impl Importer {
    fn warn(&mut self, m: impl Into<String>) {
        self.warnings.push(m.into());
    }

    fn rule(&mut self, r: &Value, section: &str) -> Option<DeclarativeRule> {
        let name = r
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("unnamed")
            .to_string();
        if r.get("scope").and_then(Value::as_str) == Some("folder") {
            self.warn(format!(
                "{section} rule `{name}` skipped: folder-scope rules are not supported yet"
            ));
            return None;
        }
        let from = match r.get("from") {
            Some(f) => Some(self.from(f, &name)?),
            None => None,
        };
        let to = match r.get("to") {
            Some(t) => Some(self.to(t, &name)?),
            None => None,
        };
        Some(DeclarativeRule {
            name,
            comment: r.get("comment").and_then(Value::as_str).map(str::to_string),
            severity: r
                .get("severity")
                .and_then(Value::as_str)
                .map_or(Level::Warn, level),
            from,
            to,
        })
    }

    fn required(&mut self, r: &Value) -> Option<RequireRule> {
        let name = r
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("unnamed")
            .to_string();
        let module = self.from(r.get("module")?, &name)?;
        let to = self.to(r.get("to")?, &name)?;
        Some(RequireRule {
            name,
            comment: r.get("comment").and_then(Value::as_str).map(str::to_string),
            severity: r
                .get("severity")
                .and_then(Value::as_str)
                .map_or(Level::Warn, level),
            module,
            to,
        })
    }

    fn from(&mut self, f: &Value, rule: &str) -> Option<FromSpec> {
        let o = f.as_object()?;
        let mut c = FromCondition::default();
        for (k, v) in o {
            match k.as_str() {
                "path" => c.path = specs(v),
                "pathNot" => c.path_not = specs(v),
                "orphan" => c.orphan = v.as_bool(),
                "numberOfDependentsLessThan" => c.number_of_dependents_less_than = num(v),
                "numberOfDependentsMoreThan" => c.number_of_dependents_more_than = num(v),
                other => {
                    self.warn(format!("rule `{rule}` skipped: unsupported `from.{other}`"));
                    return None;
                }
            }
        }
        Some(FromSpec::Cond(c))
    }

    fn to(&mut self, t: &Value, rule: &str) -> Option<ToSpec> {
        let o = t.as_object()?;
        let mut c = ToCondition::default();
        for (k, v) in o {
            match k.as_str() {
                "path" => c.path = specs(v),
                "pathNot" => c.path_not = specs(v),
                "orphan" => c.orphan = v.as_bool(),
                "dependencyTypes" => c.dependency_types = strings(v),
                "dependencyTypesNot" => c.dependency_types_not = strings(v),
                "moreThanOneDependencyType" => c.more_than_one_dependency_type = v.as_bool(),
                "couldNotResolve" => c.could_not_resolve = v.as_bool(),
                "dynamic" => c.dynamic = v.as_bool(),
                "exoticallyRequired" => c.exotically_required = v.as_bool(),
                "preCompilationOnly" => {
                    c.type_only = v.as_bool();
                    self.warn(format!(
                        "rule `{rule}`: `preCompilationOnly` was mapped to `typeOnly` (type-only imports; \
                         oxcruise has exact type-only information)"
                    ));
                }
                "circular" => c.circular = v.as_bool(),
                "via" => c.via = self.via(v),
                "viaOnly" => c.via_only = self.via(v),
                "reachable" => c.reachable = v.as_bool(),
                "numberOfDependentsLessThan" => c.number_of_dependents_less_than = num(v),
                "numberOfDependentsMoreThan" => c.number_of_dependents_more_than = num(v),
                other => {
                    self.warn(format!("rule `{rule}` skipped: unsupported `to.{other}`"));
                    return None;
                }
            }
        }
        Some(ToSpec::Cond(Box::new(c)))
    }

    fn via(&mut self, v: &Value) -> Option<ViaCondition> {
        // depcruise accepts a bare regex string or array as shorthand for `{ path }`.
        if v.is_string() || v.is_array() {
            return Some(ViaCondition {
                path: specs(v),
                ..Default::default()
            });
        }
        let o = v.as_object()?;
        let mut c = ViaCondition::default();
        for (k, v) in o {
            match k.as_str() {
                "path" => c.path = specs(v),
                "pathNot" => c.path_not = specs(v),
                "dependencyTypes" => c.dependency_types = strings(v),
                "dependencyTypesNot" => c.dependency_types_not = strings(v),
                "dynamic" => c.dynamic = v.as_bool(),
                other => self.warn(format!("`via.{other}` is not supported and was ignored")),
            }
        }
        Some(c)
    }

    fn extends(&mut self, v: &Value, cfg: &mut OxcruiseConfig) {
        for e in strings(v) {
            let name = e.rsplit('/').next().unwrap_or(&e);
            let preset = match name {
                "recommended-strict" | "recommended-strict.js" | "recommended-strict.cjs" => {
                    "oxcruise:strict"
                }
                "recommended" | "recommended.js" | "recommended.cjs" => "oxcruise:recommended",
                "recommended-warn-only"
                | "recommended-warn-only.js"
                | "recommended-warn-only.cjs" => {
                    self.warn(format!(
                        "`extends: {e}` mapped to oxcruise:recommended; severities are not downgraded to warn"
                    ));
                    "oxcruise:recommended"
                }
                _ if e.starts_with('.') => {
                    cfg.extends.push(e);
                    continue;
                }
                _ => {
                    self.warn(format!(
                        "`extends: {e}` is not a known preset or relative path and was ignored"
                    ));
                    continue;
                }
            };
            cfg.extends.push(preset.to_string());
        }
    }

    fn options(&mut self, o: &Value, cfg: &mut OxcruiseConfig) {
        let Some(o) = o.as_object() else { return };
        for (k, v) in o {
            match k.as_str() {
                "doNotFollow" => {
                    for p in path_of(v) {
                        cfg.follow.do_not_follow.push(p);
                    }
                    let types = v.get("dependencyTypes").map(strings).unwrap_or_default();
                    for t in types {
                        if t != "npm" {
                            self.warn(format!(
                                "`options.doNotFollow.dependencyTypes` entry `{t}` is not supported"
                            ));
                        }
                    }
                }
                "exclude" => cfg.exclude.extend(path_of(v)),
                "includeOnly" => cfg.include_only.extend(path_of(v)),
                "maxDepth" => {
                    // 0 means unlimited in depcruise
                    cfg.follow.max_depth = num(v).filter(|n| *n > 0);
                }
                "tsConfig" => {
                    if let Some(f) = v.get("fileName").and_then(Value::as_str) {
                        cfg.resolve.tsconfig = Some(TsconfigSetting::Path(f.to_string()));
                    }
                }
                "enhancedResolveOptions" => {
                    if let Some(oo) = v.as_object() {
                        for (ek, ev) in oo {
                            match ek.as_str() {
                                "conditionNames" => cfg.resolve.conditions = strings(ev),
                                "extensions" => cfg.resolve.extensions = strings(ev),
                                "mainFields" => cfg.resolve.main_fields = strings(ev),
                                "exportsFields" => {}
                                other => self.warn(format!(
                                    "`options.enhancedResolveOptions.{other}` is not supported and was ignored"
                                )),
                            }
                        }
                    }
                }
                "exoticRequireStrings" => cfg.extract.exotic_require_strings = strings(v),
                "detectJSDocImports" => cfg.extract.jsdoc = v.as_bool().unwrap_or(false),
                "tsPreCompilationDeps" => {
                    if v.as_bool() == Some(false) {
                        self.warn("`options.tsPreCompilationDeps: false` is not supported: oxcruise always sees type-only imports");
                    }
                }
                "webpackConfig" | "babelConfig" => {
                    self.warn(format!(
                        "`options.{k}` is not supported; use `resolve.alias` for webpack aliases"
                    ));
                }
                "combinedDependencies"
                | "cache"
                | "moduleSystems"
                | "externalModuleResolutionStrategy"
                | "knownViolations" => {
                    self.warn(format!("`options.{k}` is not supported and was ignored"));
                }
                // output-only or implementation details of depcruise
                "outputType"
                | "prefix"
                | "reporterOptions"
                | "progress"
                | "skipAnalysisNotInRules"
                | "baseDir"
                | "builtInModules"
                | "metrics"
                | "forceDeriveDependents"
                | "parser"
                | "focus"
                | "reaches"
                | "highlight"
                | "collapse"
                | "affected"
                | "mono"
                | "enhancedResolveOptionsCache"
                | "extraExtensionsToScan" => {}
                other => self.warn(format!(
                    "`options.{other}` is not supported and was ignored"
                )),
            }
        }
    }
}

fn num(v: &Value) -> Option<u32> {
    v.as_u64().and_then(|n| u32::try_from(n).ok())
}

fn strings(v: &Value) -> Vec<String> {
    match v {
        Value::String(s) => vec![s.clone()],
        Value::Array(a) => a
            .iter()
            .filter_map(|x| x.as_str().map(str::to_string))
            .collect(),
        _ => vec![],
    }
}

/// depcruise path conditions are regexes, as a string or an array of strings.
fn specs(v: &Value) -> Option<OneOrMany<PathSpec>> {
    match v {
        Value::String(s) => Some(OneOrMany::One(PathSpec::regex(s))),
        Value::Array(a) => Some(OneOrMany::Many(
            a.iter()
                .filter_map(|x| x.as_str())
                .map(PathSpec::regex)
                .collect(),
        )),
        _ => None,
    }
}

/// `exclude`, `includeOnly` and `doNotFollow` take a regex, an array, or `{ path }`.
fn path_of(v: &Value) -> Vec<PathSpec> {
    let inner = v.get("path").unwrap_or(v);
    strings(inner).into_iter().map(PathSpec::regex).collect()
}
