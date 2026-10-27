//! `oxcruise migrate`: convert a `.dependency-cruiser.*` configuration.
//!
//! oxcruise's rule language is much smaller than dependency-cruiser's, so the conversion is
//! conservative: path regexes become globs when they have a common shape, and everything else
//! is reported as a named warning. A rule that uses a condition oxcruise does not have, or a
//! regex that does not convert, is dropped entirely, because dropping just the condition would
//! silently widen the rule.

use std::collections::BTreeSet;

use serde_json::Value;

use crate::regex_glob::{placeholders, regex_to_globs};
use crate::{Category, ForbidRule, Kind, Level, OneOrMany, OxcruiseConfig, ToCondition, ToSpec};

/// dependency-cruiser's recommended rule names and the oxcruise built-in that does the same job.
const BUILTIN_EQUIVALENTS: &[(&str, &str)] = &[
    ("no-circular", "no-circular"),
    ("no-orphans", "no-orphans"),
    ("no-deprecated-core", "no-deprecated"),
    ("no-deprecated-npm", "no-deprecated"),
    ("no-non-package-json", "no-undeclared-deps"),
    ("not-to-unresolvable", "no-unresolvable"),
    ("no-duplicate-dep-types", "no-duplicate-deps"),
    ("not-to-test", "no-test-imports"),
    ("not-to-spec", "no-test-imports"),
    ("not-to-dev-dep", "no-dev-deps-in-prod"),
    ("optional-deps-used", "no-optional-deps"),
    ("peer-deps-used", "no-peer-deps"),
];

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
                    if let Some(rule) = cx.rule(r) {
                        cfg.forbid.push(rule);
                    }
                }
            }
            "allowed" => {
                if val.as_array().is_some_and(|a| !a.is_empty()) {
                    cx.warn(
                        "`allowed` rules were dropped: oxcruise has no allow-lists. \
                         Describe the architecture with the `layers` rule, or add `forbid` rules",
                    );
                }
            }
            "allowedSeverity" => {}
            "required" => {
                if val.as_array().is_some_and(|a| !a.is_empty()) {
                    cx.warn(
                        "`required` rules were dropped: oxcruise has no `require` rules. \
                         Write a JS plugin rule if you need this check",
                    );
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

fn level(s: &str) -> Option<Level> {
    match s {
        "error" => Some(Level::Error),
        "warn" => Some(Level::Warn),
        "info" => Some(Level::Warn),
        "ignore" | "off" => Some(Level::Off),
        _ => None,
    }
}

struct Importer {
    warnings: Vec<String>,
}

impl Importer {
    fn warn(&mut self, m: impl Into<String>) {
        let m = m.into();
        if !self.warnings.contains(&m) {
            self.warnings.push(m);
        }
    }

    fn severity(&mut self, r: &Value, rule: &str) -> Level {
        match r.get("severity").and_then(Value::as_str) {
            Some("info") => {
                self.warn(format!(
                    "rule `{rule}`: severity `info` does not exist in oxcruise and was mapped to `warn`"
                ));
                Level::Warn
            }
            Some(s) => level(s).unwrap_or(Level::Warn),
            None => Level::Warn,
        }
    }

    /// Regexes to globs. `None` (after a warning) when one does not convert.
    fn globs(&mut self, v: &Value, rule: &str) -> Option<Vec<String>> {
        let mut out = vec![];
        for re in strings(v) {
            match regex_to_globs(&re) {
                Ok(globs) => out.extend(globs),
                Err(e) => {
                    self.warn(format!(
                        "rule `{rule}` was dropped: the regex {e} does not convert to a glob. \
                         Rewrite it by hand, or write a JS plugin rule"
                    ));
                    return None;
                }
            }
        }
        Some(out)
    }

    /// `path` plus `pathNot` as one list: negations get a leading `!`.
    fn path_list(&mut self, o: &serde_json::Map<String, Value>, rule: &str) -> Option<Vec<String>> {
        let mut list = vec![];
        if let Some(p) = o.get("path") {
            list.extend(self.globs(p, rule)?);
        }
        if let Some(p) = o.get("pathNot") {
            list.extend(self.globs(p, rule)?.into_iter().map(|g| format!("!{g}")));
        }
        Some(list)
    }

    fn rule(&mut self, r: &Value) -> Option<ForbidRule> {
        let name = r
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("unnamed")
            .to_string();
        if r.get("scope").and_then(Value::as_str) == Some("folder") {
            self.warn(format!(
                "rule `{name}` was dropped: folder-scope rules are not supported"
            ));
            return None;
        }

        let mut unsupported: Vec<String> = vec![];
        let mut cond = ToCondition::default();

        let from_obj = r.get("from").and_then(Value::as_object);
        let mut from = vec![];
        if let Some(o) = from_obj {
            for k in o.keys() {
                if !matches!(k.as_str(), "path" | "pathNot") {
                    unsupported.push(format!("from.{k}"));
                }
            }
            from = self.path_list(o, &name)?;
        }

        let to_obj = r.get("to").and_then(Value::as_object);
        let mut to_paths = vec![];
        if let Some(o) = to_obj {
            to_paths = self.path_list(o, &name)?;
            for (k, v) in o {
                match k.as_str() {
                    "path" | "pathNot" => {}
                    "dynamic" => cond.dynamic = v.as_bool(),
                    "preCompilationOnly" => {
                        cond.type_only = v.as_bool();
                        self.warn(format!(
                            "rule `{name}`: `preCompilationOnly` was mapped to `typeOnly` \
                             (type-only imports; oxcruise has exact type-only information)"
                        ));
                    }
                    "couldNotResolve" if v.as_bool() == Some(true) => {
                        cond.kind = Some(OneOrMany::One(Kind::Unresolved));
                    }
                    "reachable" if v.as_bool() == Some(true) => cond.reachable = Some(true),
                    other => unsupported.push(format!("to.{other}")),
                }
            }
        }

        if !unsupported.is_empty() {
            let conditions = unsupported
                .iter()
                .map(|c| format!("`{c}`"))
                .collect::<Vec<_>>()
                .join(", ");
            let hint = match BUILTIN_EQUIVALENTS.iter().find(|(n, _)| *n == name) {
                Some((_, builtin)) => format!("the built-in rule `{builtin}` covers this"),
                None => "write a JS plugin rule for it".into(),
            };
            self.warn(format!(
                "rule `{name}` was dropped: {conditions} {} not supported ({hint})",
                if unsupported.len() == 1 { "is" } else { "are" }
            ));
            return None;
        }

        // `$1` on the `to` side must refer to a capture the `from` side defines.
        let defined: BTreeSet<String> = from.iter().flat_map(|g| placeholders(g)).collect();
        if let Some(missing) = to_paths
            .iter()
            .flat_map(|g| placeholders(g))
            .find(|p| !defined.contains(p))
        {
            self.warn(format!(
                "rule `{name}` was dropped: `to` refers to capture group `{}` that `from` does not define as `([^/]+)`",
                missing.trim_start_matches('c')
            ));
            return None;
        }

        let paths = |list: Vec<String>| match list.len() {
            0 => None,
            1 => Some(OneOrMany::One(list.into_iter().next().expect("one"))),
            _ => Some(OneOrMany::Many(list)),
        };
        let to = {
            let has_cond = cond != ToCondition::default();
            match (paths(to_paths), has_cond) {
                (None, false) => None,
                (Some(p), false) => Some(ToSpec::Paths(p)),
                (p, true) => {
                    cond.path = p;
                    Some(ToSpec::Cond(cond))
                }
            }
        };
        Some(ForbidRule {
            severity: self.severity(r, &name),
            name,
            message: r.get("comment").and_then(Value::as_str).map(str::to_string),
            from: paths(from),
            to,
        })
    }

    fn extends(&mut self, v: &Value, cfg: &mut OxcruiseConfig) {
        for e in strings(v) {
            let file = e.rsplit('/').next().unwrap_or(&e);
            let name = file
                .strip_suffix(".js")
                .or_else(|| file.strip_suffix(".cjs"));
            match name.unwrap_or(file) {
                // the default categories are what `recommended` stands for
                "recommended" => {}
                "recommended-strict" => {
                    cfg.categories.insert(Category::Suspicious, Level::Error);
                }
                "recommended-warn-only" => {
                    cfg.categories.insert(Category::Correctness, Level::Warn);
                }
                _ if e.starts_with('.') => cfg.extends.push(e),
                _ => self.warn(format!(
                    "`extends: {e}` is not a known preset or relative path and was ignored"
                )),
            }
        }
    }

    fn options(&mut self, o: &Value, cfg: &mut OxcruiseConfig) {
        let Some(o) = o.as_object() else { return };
        for (k, v) in o {
            match k.as_str() {
                "doNotFollow" => {
                    cfg.do_not_follow.extend(self.option_globs(k, v));
                    for t in v.get("dependencyTypes").map(strings).unwrap_or_default() {
                        // node_modules are always leaves in oxcruise
                        if t != "npm" {
                            self.warn(format!(
                                "`options.doNotFollow.dependencyTypes` entry `{t}` is not supported"
                            ));
                        }
                    }
                }
                "exclude" => cfg.ignore_patterns.extend(self.option_globs(k, v)),
                "includeOnly" => self.warn(
                    "`options.includeOnly` was dropped: oxcruise has no include list. \
                     Start from the right `entries` and list what to leave out in `ignorePatterns`",
                ),
                "maxDepth" => {
                    if num(v).is_some_and(|n| n > 0) {
                        self.warn("`options.maxDepth` was dropped: use the `--max-depth` flag");
                    }
                }
                "tsConfig" => {
                    if let Some(f) = v.get("fileName").and_then(Value::as_str) {
                        cfg.resolve.tsconfig = Some(f.to_string());
                    }
                }
                "enhancedResolveOptions" => {
                    let Some(oo) = v.as_object() else { continue };
                    for (ek, ev) in oo {
                        match ek.as_str() {
                            // oxcruise always tries import|require, node and default
                            "conditionNames" => {
                                cfg.resolve.conditions = strings(ev)
                                    .into_iter()
                                    .filter(|c| {
                                        !matches!(c.as_str(), "import" | "require" | "node" | "default")
                                    })
                                    .collect();
                            }
                            "exportsFields" => {}
                            "extensions" | "mainFields" => self.warn(format!(
                                "`options.enhancedResolveOptions.{ek}` was dropped: oxcruise uses the resolver defaults"
                            )),
                            other => self.warn(format!(
                                "`options.enhancedResolveOptions.{other}` is not supported and was ignored"
                            )),
                        }
                    }
                }
                "exoticRequireStrings" => cfg.calls.extend(strings(v)),
                "detectJSDocImports" => {
                    // always on in oxcruise
                    if v.as_bool() == Some(false) {
                        self.warn("`options.detectJSDocImports: false` is not supported: JSDoc imports are always detected");
                    }
                }
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

    /// `exclude` and `doNotFollow` take a regex, an array, or `{ path }`.
    fn option_globs(&mut self, key: &str, v: &Value) -> Vec<String> {
        let mut out = vec![];
        for re in strings(v.get("path").unwrap_or(v)) {
            match regex_to_globs(&re) {
                Ok(globs) => out.extend(globs),
                Err(e) => self.warn(format!(
                    "`options.{key}` entry dropped: the regex {e} does not convert to a glob. Add it by hand"
                )),
            }
        }
        out
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
