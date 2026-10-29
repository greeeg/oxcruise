use crate::{OxcruiseConfig, ResolveSection};

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
    /// * `categories`, `rules`, `resolve.alias`: merged by key, the child's entry wins.
    /// * `forbid`/`overrides`/`calls`/`jsPlugins`: concatenated, base first.
    pub fn layered(&self, child: &OxcruiseConfig) -> OxcruiseConfig {
        let mut categories = self.categories.clone();
        categories.extend(child.categories.clone());
        let mut rules = self.rules.clone();
        rules.extend(child.rules.clone());
        OxcruiseConfig {
            schema: child.schema.clone().or_else(|| self.schema.clone()),
            extends: vec![],
            entries: pick(&self.entries, &child.entries),
            ignore_patterns: pick(&self.ignore_patterns, &child.ignore_patterns),
            do_not_follow: pick(&self.do_not_follow, &child.do_not_follow),
            categories,
            rules,
            forbid: concat(&self.forbid, &child.forbid),
            overrides: concat(&self.overrides, &child.overrides),
            resolve: ResolveSection {
                tsconfig: child
                    .resolve
                    .tsconfig
                    .clone()
                    .or_else(|| self.resolve.tsconfig.clone()),
                conditions: pick(&self.resolve.conditions, &child.resolve.conditions),
                alias: {
                    let mut a = self.resolve.alias.clone();
                    a.extend(child.resolve.alias.clone());
                    a
                },
            },
            calls: concat(&self.calls, &child.calls),
            js_plugins: concat(&self.js_plugins, &child.js_plugins),
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{Category, Level, OxcruiseConfig, RuleEntry};

    fn cfg(json: &str) -> OxcruiseConfig {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn layering() {
        let base = cfg(
            r#"{"entries":["src"],"rules":{"a":"warn","b":"error"},"forbid":[{"name":"x"}],
               "categories":{"correctness":"warn","style":"warn"},"calls":["jest.mock"]}"#,
        );
        let child = cfg(
            r#"{"entries":["lib"],"rules":{"a":"off"},"forbid":[{"name":"y"}],
               "categories":{"correctness":"error"},"calls":["vi.mock"]}"#,
        );
        let m = base.layered(&child);
        assert_eq!(m.entries, ["lib"]);
        assert_eq!(m.rules["a"], RuleEntry::Level(Level::Off));
        assert_eq!(m.rules["b"], RuleEntry::Level(Level::Error));
        assert_eq!(m.categories[&Category::Correctness], Level::Error);
        assert_eq!(m.categories[&Category::Style], Level::Warn);
        assert_eq!(
            m.forbid.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
            ["x", "y"]
        );
        assert_eq!(m.calls, ["jest.mock", "vi.mock"]);
        let empty_child = cfg("{}");
        assert_eq!(base.layered(&empty_child).entries, ["src"]);
    }
}
