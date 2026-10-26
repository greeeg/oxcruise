use globset::{GlobBuilder, GlobSet, GlobSetBuilder};
use regex::RegexSet;

/// A path pattern from config: a glob, or an explicit regular expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathPattern {
    Glob(String),
    Regex(String),
}

/// Matches `/`-separated paths relative to the cruise root against many patterns at once.
///
/// Globs use `*` within a segment and `**` across segments. A glob also
/// matches everything beneath a directory it matches, so `src/legacy` and
/// `src/legacy/**` are equivalent. Regexes use the linear-time `regex` crate.
#[derive(Debug, Clone)]
pub struct PathMatcher {
    globs: GlobSet,
    regexes: RegexSet,
    empty: bool,
}

impl Default for PathMatcher {
    fn default() -> Self {
        Self::new(&[]).expect("empty matcher")
    }
}

impl PathMatcher {
    pub fn new(patterns: &[PathPattern]) -> Result<Self, String> {
        let mut globs = GlobSetBuilder::new();
        let mut regexes = vec![];
        for p in patterns {
            match p {
                PathPattern::Glob(g) => {
                    let g = g.strip_prefix("./").unwrap_or(g);
                    for pat in [g.to_string(), format!("{}/**", g.trim_end_matches('/'))] {
                        let glob = GlobBuilder::new(&pat)
                            .literal_separator(true)
                            .build()
                            .map_err(|e| format!("invalid glob `{g}`: {e}"))?;
                        globs.add(glob);
                    }
                }
                PathPattern::Regex(r) => regexes.push(r.clone()),
            }
        }
        Ok(Self {
            globs: globs.build().map_err(|e| e.to_string())?,
            regexes: RegexSet::new(&regexes).map_err(|e| format!("invalid regex: {e}"))?,
            empty: patterns.is_empty(),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.empty
    }

    pub fn is_match(&self, path: &str) -> bool {
        !self.empty && (self.globs.is_match(path) || self.regexes.is_match(path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(s: &str) -> PathPattern {
        PathPattern::Glob(s.into())
    }

    #[test]
    fn globs() {
        let m = PathMatcher::new(&[g("src/*.ts"), g("lib")]).unwrap();
        assert!(m.is_match("src/a.ts"));
        assert!(!m.is_match("src/deep/a.ts"));
        assert!(m.is_match("lib"));
        assert!(m.is_match("lib/deep/a.ts"));
        assert!(!m.is_match("library/a.ts"));
        let m = PathMatcher::new(&[g("**/*.test.ts")]).unwrap();
        assert!(m.is_match("a/b/c.test.ts"));
        assert!(m.is_match("c.test.ts"));
    }

    #[test]
    fn regexes_and_empty() {
        let m = PathMatcher::new(&[PathPattern::Regex(r"^src/(a|b)\.ts$".into())]).unwrap();
        assert!(m.is_match("src/a.ts"));
        assert!(!m.is_match("src/c.ts"));
        assert!(!PathMatcher::default().is_match("anything"));
        assert!(PathMatcher::new(&[PathPattern::Regex("(".into())]).is_err());
    }
}
