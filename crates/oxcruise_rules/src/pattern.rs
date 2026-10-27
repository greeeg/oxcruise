//! Path patterns for rules: globs with `!` negation and captures.
//!
//! * A glob may contain `{name}`, which captures one path segment on the
//!   `from` side and is substituted on the `to` side: `src/features/{f}/**`.
//! * A leading `!` negates a glob. A list matches a path when some positive glob
//!   matches and no negated glob does; a list of only negations means "everything
//!   except". Negated globs may refer to captures too.
//!
//! Everything compiles to the `regex` crate, so matching is linear-time.

use std::sync::Mutex;

use regex::Regex;
use rustc_hash::FxHashMap;

pub type Captures = FxHashMap<String, String>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Defines captures (`from` side).
    Capturing,
    /// Refers to captures (`to` side) and ordinary standalone patterns.
    Referencing,
}

#[derive(Debug)]
pub struct PathPatterns {
    positive: Vec<Item>,
    negative: Vec<Item>,
}

#[derive(Debug)]
enum Item {
    Static(Regex),
    Template(Template),
}

#[derive(Debug)]
struct Template {
    source: String,
    cache: Mutex<FxHashMap<String, Option<Regex>>>,
}

impl Item {
    fn compile(glob: &str, capturing: bool) -> Result<Item, String> {
        let src = glob.strip_prefix("./").unwrap_or(glob);
        if !capturing && placeholder_spans(src).next().is_some() {
            return Ok(Item::Template(Template {
                source: src.to_string(),
                cache: Mutex::default(),
            }));
        }
        Regex::new(&glob_to_regex(src, capturing))
            .map(Item::Static)
            .map_err(|e| format!("invalid pattern `{glob}`: {e}"))
    }

    fn is_match(&self, path: &str, caps: &Captures) -> bool {
        match self {
            Item::Static(re) => re.is_match(path),
            Item::Template(t) => t.regex(caps).is_some_and(|re| re.is_match(path)),
        }
    }
}

impl PathPatterns {
    pub fn new(globs: &[String], role: Role) -> Result<Self, String> {
        let (mut positive, mut negative) = (vec![], vec![]);
        for g in globs {
            match g.strip_prefix('!') {
                Some(neg) => negative.push(Item::compile(neg, false)?),
                None => positive.push(Item::compile(g, role == Role::Capturing)?),
            }
        }
        Ok(Self { positive, negative })
    }

    pub fn empty() -> Self {
        Self {
            positive: vec![],
            negative: vec![],
        }
    }

    pub fn is_empty(&self) -> bool {
        self.positive.is_empty() && self.negative.is_empty()
    }

    /// Does the list match `path`? Patterns that refer to captures that `caps` lacks never
    /// match, so a negated one then excludes nothing. An empty list matches nothing.
    pub fn is_match(&self, path: &str, caps: &Captures) -> bool {
        if self.is_empty() {
            return false;
        }
        let included =
            self.positive.is_empty() || self.positive.iter().any(|i| i.is_match(path, caps));
        included && !self.negative.iter().any(|i| i.is_match(path, caps))
    }

    /// Captures of the first matching positive glob, or `None` if the list does not match.
    /// An empty list matches everything with no captures.
    pub fn capture(&self, path: &str) -> Option<Captures> {
        if self.is_empty() {
            return Some(Captures::default());
        }
        let mut caps = Captures::default();
        if !self.positive.is_empty() {
            caps = self.positive.iter().find_map(|i| match i {
                Item::Static(re) => re.captures(path).map(|c| {
                    let mut caps = Captures::default();
                    for (idx, name) in re.capture_names().enumerate().skip(1) {
                        if let (Some(m), Some(n)) = (c.get(idx), name) {
                            caps.insert(n.to_string(), m.as_str().to_string());
                        }
                    }
                    caps
                }),
                Item::Template(_) => None,
            })?;
        }
        (!self.negative.iter().any(|i| i.is_match(path, &caps))).then_some(caps)
    }
}

impl Template {
    fn regex(&self, caps: &Captures) -> Option<Regex> {
        let (substituted, key) = substitute(&self.source, caps)?;
        let mut cache = self.cache.lock().expect("poisoned");
        cache
            .entry(key)
            .or_insert_with(|| Regex::new(&glob_to_regex(&substituted, false)).ok())
            .clone()
    }
}

/// `(start, end, name)` of each `{ident}` placeholder.
fn placeholder_spans(src: &str) -> impl Iterator<Item = (usize, usize, &str)> {
    let mut idx = 0;
    std::iter::from_fn(move || {
        while let Some(rel) = src[idx..].find('{') {
            let start = idx + rel;
            if let Some(close) = src[start..].find('}') {
                let name = &src[start + 1..start + close];
                idx = start + close + 1;
                if is_ident(name) {
                    return Some((start, idx, name));
                }
            } else {
                break;
            }
        }
        None
    })
}

fn is_ident(s: &str) -> bool {
    let mut c = s.chars();
    c.next()
        .is_some_and(|f| f.is_ascii_alphabetic() || f == '_')
        && c.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Replace references with captured text. Returns the new source and a cache key.
fn substitute(src: &str, caps: &Captures) -> Option<(String, String)> {
    let mut out = String::new();
    let mut key = String::new();
    let mut last = 0;
    for (s, e, name) in placeholder_spans(src) {
        out.push_str(&src[last..s]);
        let v = caps.get(name)?;
        // glob metacharacters in captured text are inert after glob_to_regex only if escaped
        out.push_str(&escape_glob(v));
        key.push_str(v);
        key.push('\0');
        last = e;
    }
    out.push_str(&src[last..]);
    Some((out, key))
}

fn escape_glob(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if matches!(c, '*' | '?' | '[' | ']' | '{' | '}' | '\\') {
                vec!['\\', c]
            } else {
                vec![c]
            }
        })
        .collect()
}

/// Translate a glob to an anchored regex. A glob also matches everything beneath a
/// directory it matches. With `capturing`, `{ident}` becomes a named group.
pub fn glob_to_regex(glob: &str, capturing: bool) -> String {
    let chars: Vec<char> = glob.chars().collect();
    let mut out = String::from("^");
    let mut i = 0;
    let mut ends_with_globstar_dir = false;
    while i < chars.len() {
        let c = chars[i];
        ends_with_globstar_dir = false;
        match c {
            '*' if chars.get(i + 1) == Some(&'*') => {
                let at_seg_start = i == 0 || chars[i - 1] == '/';
                let followed_by_slash = chars.get(i + 2) == Some(&'/');
                if at_seg_start && followed_by_slash {
                    out.push_str("(?:.*/)?");
                    i += 3;
                    continue;
                }
                if i > 0 && chars[i - 1] == '/' && i + 2 == chars.len() {
                    // trailing `/**`: the preceding `/` was already emitted, make it optional
                    out.pop();
                    out.push_str("(?:/.*)?");
                    ends_with_globstar_dir = true;
                } else {
                    out.push_str(".*");
                }
                i += 2;
                continue;
            }
            '*' => out.push_str("[^/]*"),
            '?' => out.push_str("[^/]"),
            '[' => {
                if let Some(close) = chars[i..].iter().position(|&c| c == ']') {
                    let class: String = chars[i..=i + close].iter().collect();
                    out.push_str(&class.replacen("[!", "[^", 1));
                    i += close + 1;
                    continue;
                }
                out.push_str("\\[");
            }
            '{' => {
                if let Some(close) = chars[i..].iter().position(|&c| c == '}') {
                    let inner: String = chars[i + 1..i + close].iter().collect();
                    if is_ident(&inner) {
                        if capturing {
                            out.push_str(&format!("(?P<{inner}>[^/]+)"));
                        } else {
                            out.push_str("[^/]+");
                        }
                    } else {
                        let alts: Vec<String> = inner
                            .split(',')
                            .map(|a| {
                                glob_to_regex(a, false)
                                    .trim_start_matches('^')
                                    .trim_end_matches("(?:/.*)?$")
                                    .to_string()
                            })
                            .collect();
                        out.push_str(&format!("(?:{})", alts.join("|")));
                    }
                    i += close + 1;
                    continue;
                }
                out.push_str("\\{");
            }
            '\\' if i + 1 < chars.len() => {
                out.push_str(&regex::escape(&chars[i + 1].to_string()));
                i += 2;
                continue;
            }
            c => out.push_str(&regex::escape(&c.to_string())),
        }
        i += 1;
    }
    if !ends_with_globstar_dir {
        out.push_str("(?:/.*)?");
    }
    out.push('$');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pats(globs: &[&str], role: Role) -> PathPatterns {
        let globs: Vec<String> = globs.iter().map(|g| g.to_string()).collect();
        PathPatterns::new(&globs, role).unwrap()
    }
    fn none() -> Captures {
        Captures::default()
    }

    #[test]
    fn glob_semantics() {
        let p = pats(
            &["src/*.ts", "lib", "**/*.test.ts", "a/**/z.ts", "x/**"],
            Role::Referencing,
        );
        for yes in [
            "src/a.ts",
            "lib",
            "lib/deep/a.ts",
            "a.test.ts",
            "d/e/a.test.ts",
            "a/z.ts",
            "a/b/c/z.ts",
            "x",
            "x/y/z",
        ] {
            assert!(p.is_match(yes, &none()), "{yes} should match");
        }
        for no in [
            "src/deep/a.ts",
            "library/a.ts",
            "a/b/y.ts",
            "xx/y",
            "src/a.tsx",
        ] {
            assert!(!p.is_match(no, &none()), "{no} should not match");
        }
    }

    #[test]
    fn braces_and_classes() {
        let p = pats(&["src/*.{ts,tsx}", "[ab]/f.js"], Role::Referencing);
        assert!(p.is_match("src/a.ts", &none()) && p.is_match("src/a.tsx", &none()));
        assert!(!p.is_match("src/a.js", &none()));
        assert!(p.is_match("a/f.js", &none()) && !p.is_match("c/f.js", &none()));
    }

    #[test]
    fn named_captures_in_globs() {
        let from = pats(&["src/features/{feature}/**"], Role::Capturing);
        let caps = from.capture("src/features/billing/ui/x.ts").unwrap();
        assert_eq!(caps["feature"], "billing");
        assert!(from.capture("src/other/x.ts").is_none());

        let to = pats(&["src/features/{feature}/**"], Role::Referencing);
        assert!(to.is_match("src/features/billing/api.ts", &caps));
        assert!(!to.is_match("src/features/auth/api.ts", &caps));
        // a placeholder without a capture never matches
        assert!(!to.is_match("src/features/billing/api.ts", &none()));
    }

    #[test]
    fn negation_excludes_from_the_positive_globs() {
        let p = pats(
            &["src/**", "!src/generated/**", "!**/*.d.ts"],
            Role::Referencing,
        );
        assert!(p.is_match("src/a.ts", &none()));
        assert!(!p.is_match("src/generated/a.ts", &none()));
        assert!(!p.is_match("src/types.d.ts", &none()));
        assert!(!p.is_match("lib/a.ts", &none()));
    }

    #[test]
    fn only_negations_mean_everything_except() {
        let p = pats(&["!**/*.spec.ts", "!dist/**"], Role::Referencing);
        assert!(p.is_match("src/a.ts", &none()));
        assert!(!p.is_match("src/a.spec.ts", &none()));
        assert!(!p.is_match("dist/a.js", &none()));
        // from side: matches without captures, still honours the negations
        let from = pats(&["!**/*.spec.ts"], Role::Capturing);
        assert!(from.capture("src/a.ts").unwrap().is_empty());
        assert!(from.capture("src/a.spec.ts").is_none());
    }

    #[test]
    fn negated_globs_refer_to_captures() {
        let from = pats(&["src/features/{feature}/**"], Role::Capturing);
        let caps = from.capture("src/features/billing/x.ts").unwrap();
        let to = pats(
            &["src/features/**", "!src/features/{feature}/**"],
            Role::Referencing,
        );
        assert!(to.is_match("src/features/auth/api.ts", &caps));
        assert!(!to.is_match("src/features/billing/api.ts", &caps));
    }

    #[test]
    fn negations_on_the_from_side_see_its_captures() {
        let from = pats(
            &["src/features/{f}/**", "!src/features/{f}/shared/**"],
            Role::Capturing,
        );
        assert_eq!(from.capture("src/features/a/x.ts").unwrap()["f"], "a");
        assert!(from.capture("src/features/a/shared/x.ts").is_none());
    }

    #[test]
    fn captured_text_is_escaped() {
        let from = pats(&["pkg/{name}/**"], Role::Capturing);
        let caps = from.capture("pkg/a.b/x.ts").unwrap();
        let to = pats(&["pkg/{name}/**"], Role::Referencing);
        assert!(to.is_match("pkg/a.b/y.ts", &caps));
        assert!(!to.is_match("pkg/aXb/y.ts", &caps));
    }

    #[test]
    fn empty_matches_everything_for_capture() {
        assert!(PathPatterns::empty().capture("anything").is_some());
        assert!(!PathPatterns::empty().is_match("anything", &none()));
    }
}
