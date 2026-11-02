use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

/// Matches `/`-separated paths relative to the cruise root against many globs at once.
///
/// Globs use `*` within a segment and `**` across segments. A glob also
/// matches everything beneath a directory it matches, so `src/legacy` and
/// `src/legacy/**` are equivalent. A leading `!` negates a glob, as in the rule patterns: a
/// path matches when some positive glob matches and no negated one does, and a list of only
/// negations means "everything except".
#[derive(Debug, Clone)]
pub struct PathMatcher {
    positive: GlobSet,
    negative: GlobSet,
    /// No positive glob: the negated ones then select everything else.
    only_negations: bool,
    empty: bool,
}

impl Default for PathMatcher {
    fn default() -> Self {
        Self::new::<&str>(&[]).expect("empty matcher")
    }
}

impl PathMatcher {
    pub fn new<S: AsRef<str>>(patterns: &[S]) -> Result<Self, String> {
        let (mut positive, mut negative) = (GlobSetBuilder::new(), GlobSetBuilder::new());
        let mut any_positive = false;
        for p in patterns {
            let (set, g) = match p.as_ref().strip_prefix('!') {
                Some(neg) => (&mut negative, neg),
                None => {
                    any_positive = true;
                    (&mut positive, p.as_ref())
                }
            };
            let g = g.strip_prefix("./").unwrap_or(g);
            // `dir/**` also matches `dir` itself, like the rule patterns
            let base = g.strip_suffix("/**").unwrap_or(g);
            let pats = [
                g.to_string(),
                format!("{}/**", base.trim_end_matches('/')),
                base.to_string(),
            ];
            for pat in pats {
                let glob = GlobBuilder::new(&pat)
                    .literal_separator(true)
                    .build()
                    .map_err(|e| format!("invalid glob `{g}`: {e}"))?;
                set.add(glob);
            }
        }
        Ok(Self {
            positive: positive.build().map_err(|e| e.to_string())?,
            negative: negative.build().map_err(|e| e.to_string())?,
            only_negations: !patterns.is_empty() && !any_positive,
            empty: patterns.is_empty(),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.empty
    }

    pub fn is_match(&self, path: &str) -> bool {
        !self.empty
            && (self.only_negations || self.positive.is_match(path))
            && !self.negative.is_match(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn globs() {
        let m = PathMatcher::new(&["src/*.ts", "lib"]).unwrap();
        assert!(m.is_match("src/a.ts"));
        assert!(!m.is_match("src/deep/a.ts"));
        assert!(m.is_match("lib"));
        assert!(m.is_match("lib/deep/a.ts"));
        assert!(!m.is_match("library/a.ts"));
        let m = PathMatcher::new(&["**/*.test.ts"]).unwrap();
        assert!(m.is_match("a/b/c.test.ts"));
        assert!(m.is_match("c.test.ts"));
    }

    #[test]
    fn negation_follows_the_rule_patterns() {
        let m = PathMatcher::new(&["src/**", "!src/keep/**"]).unwrap();
        assert!(m.is_match("src/a.ts"));
        assert!(!m.is_match("src/keep/a.ts"));
        assert!(!m.is_match("lib/a.ts"));
        // only negations: everything except
        let m = PathMatcher::new(&["!src/keep"]).unwrap();
        assert!(m.is_match("lib/a.ts"));
        assert!(!m.is_match("src/keep/deep/a.ts"));
    }

    #[test]
    fn empty_and_invalid() {
        assert!(!PathMatcher::default().is_match("anything"));
        assert!(PathMatcher::new(&["a/[b"]).is_err());
    }
}
