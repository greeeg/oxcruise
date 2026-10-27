use globset::{GlobBuilder, GlobSet, GlobSetBuilder};

/// Matches `/`-separated paths relative to the cruise root against many globs at once.
///
/// Globs use `*` within a segment and `**` across segments. A glob also
/// matches everything beneath a directory it matches, so `src/legacy` and
/// `src/legacy/**` are equivalent.
#[derive(Debug, Clone)]
pub struct PathMatcher {
    globs: GlobSet,
    empty: bool,
}

impl Default for PathMatcher {
    fn default() -> Self {
        Self::new::<&str>(&[]).expect("empty matcher")
    }
}

impl PathMatcher {
    pub fn new<S: AsRef<str>>(patterns: &[S]) -> Result<Self, String> {
        let mut globs = GlobSetBuilder::new();
        for p in patterns {
            let g = p.as_ref();
            let g = g.strip_prefix("./").unwrap_or(g);
            for pat in [g.to_string(), format!("{}/**", g.trim_end_matches('/'))] {
                let glob = GlobBuilder::new(&pat)
                    .literal_separator(true)
                    .build()
                    .map_err(|e| format!("invalid glob `{g}`: {e}"))?;
                globs.add(glob);
            }
        }
        Ok(Self {
            globs: globs.build().map_err(|e| e.to_string())?,
            empty: patterns.is_empty(),
        })
    }

    pub fn is_empty(&self) -> bool {
        self.empty
    }

    pub fn is_match(&self, path: &str) -> bool {
        !self.empty && self.globs.is_match(path)
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
    fn empty_and_invalid() {
        assert!(!PathMatcher::default().is_match("anything"));
        assert!(PathMatcher::new(&["a/[b"]).is_err());
    }
}
