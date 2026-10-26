use crate::{ExtractSection, FollowConfig, Options, OxcruiseConfig, ResolveSection};

fn pick<T: Clone>(base: &[T], child: &[T]) -> Vec<T> {
    if child.is_empty() {
        base.to_vec()
    } else {
        child.to_vec()
    }
}

fn concat<T: Clone + PartialEq>(base: &[T], child: &[T]) -> Vec<T> {
    let mut out = base.to_vec();
    for c in child {
        if !out.contains(c) {
            out.push(c.clone());
        }
    }
    out
}

impl OxcruiseConfig {
    /// Layer `child` on top of `self` (`self` is the extended config).
    ///
    /// * Lists of patterns and entries: the child replaces when non-empty.
    /// * Section objects: merged field by field.
    /// * `rules`: merged by rule name, the child's entry wins.
    /// * `forbid`/`allow`/`require`/`overrides`/`jsPlugins`: concatenated, base first.
    pub fn layered(&self, child: &OxcruiseConfig) -> OxcruiseConfig {
        let mut rules = self.rules.clone();
        rules.extend(child.rules.clone());
        OxcruiseConfig {
            schema: child.schema.clone().or_else(|| self.schema.clone()),
            extends: vec![],
            entries: pick(&self.entries, &child.entries),
            exclude: pick(&self.exclude, &child.exclude),
            include_only: pick(&self.include_only, &child.include_only),
            follow: FollowConfig {
                npm: child.follow.npm || self.follow.npm,
                do_not_follow: pick(&self.follow.do_not_follow, &child.follow.do_not_follow),
                max_depth: child.follow.max_depth.or(self.follow.max_depth),
            },
            resolve: ResolveSection {
                tsconfig: child
                    .resolve
                    .tsconfig
                    .clone()
                    .or_else(|| self.resolve.tsconfig.clone()),
                conditions: pick(&self.resolve.conditions, &child.resolve.conditions),
                main_fields: pick(&self.resolve.main_fields, &child.resolve.main_fields),
                extensions: pick(&self.resolve.extensions, &child.resolve.extensions),
                alias: {
                    let mut a = self.resolve.alias.clone();
                    a.extend(child.resolve.alias.clone());
                    a
                },
                prefer_source: child.resolve.prefer_source || self.resolve.prefer_source,
                source_conditions: pick(
                    &self.resolve.source_conditions,
                    &child.resolve.source_conditions,
                ),
            },
            extract: ExtractSection {
                exotic_require_strings: concat(
                    &self.extract.exotic_require_strings,
                    &child.extract.exotic_require_strings,
                ),
                calls: concat(&self.extract.calls, &child.extract.calls),
                jsdoc: child.extract.jsdoc || self.extract.jsdoc,
                skip_triple_slash: child.extract.skip_triple_slash
                    || self.extract.skip_triple_slash,
            },
            rules,
            forbid: concat(&self.forbid, &child.forbid),
            allow: concat(&self.allow, &child.allow),
            require: concat(&self.require, &child.require),
            overrides: concat(&self.overrides, &child.overrides),
            js_plugins: concat(&self.js_plugins, &child.js_plugins),
            options: Options {
                max_warnings: child.options.max_warnings.or(self.options.max_warnings),
                report_dead_rules: child.options.report_dead_rules
                    || self.options.report_dead_rules,
                cycles: crate::CycleOptions {
                    ignore_type_only: child.options.cycles.ignore_type_only
                        || self.options.cycles.ignore_type_only,
                },
                respect_gitignore: child.options.respect_gitignore
                    || self.options.respect_gitignore,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Level, OxcruiseConfig, RuleEntry};

    fn cfg(json: &str) -> OxcruiseConfig {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn layering() {
        let base = cfg(
            r#"{"entries":["src"],"rules":{"a":"warn","b":"error"},"forbid":[{"name":"x"}],"options":{"maxWarnings":3}}"#,
        );
        let child = cfg(
            r#"{"entries":["lib"],"rules":{"a":"off"},"forbid":[{"name":"y"}],"options":{"reportDeadRules":true}}"#,
        );
        let m = base.layered(&child);
        assert_eq!(m.entries, ["lib"]);
        assert_eq!(m.rules["a"], RuleEntry::Level(Level::Off));
        assert_eq!(m.rules["b"], RuleEntry::Level(Level::Error));
        assert_eq!(
            m.forbid.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["x", "y"]
        );
        assert_eq!(m.options.max_warnings, Some(3));
        assert!(m.options.report_dead_rules);
        let empty_child = cfg("{}");
        assert_eq!(base.layered(&empty_child).entries, ["src"]);
    }
}
