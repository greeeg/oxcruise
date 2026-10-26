//! Path patterns for rules: globs and regexes, with captures.
//!
//! * A glob may contain `{name}`, which captures one path segment on the
//!   `from` side and is substituted on the `to` side: `src/features/{f}/**`.
//! * A regex may use groups on the `from` side and `$1`..`$9` on the `to`
//!   side (dependency-cruiser style), which is what the importer produces.
//!
//! Everything compiles to the `regex` crate, so matching is linear-time.

use std::sync::Mutex;

use oxcruise_config::PathSpec;
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
    items: Vec<Item>,
}

#[derive(Debug)]
enum Item {
    Static(Regex),
    Template(Template),
}

#[derive(Debug)]
struct Template {
    source: String,
    is_regex: bool,
    cache: Mutex<FxHashMap<String, Option<Regex>>>,
}

impl PathPatterns {
    pub fn new(specs: &[PathSpec], role: Role) -> Result<Self, String> {
        let mut items = vec![];
        for s in specs {
            let (src, is_regex) = match s {
                PathSpec::Glob(g) => (g.strip_prefix("./").unwrap_or(g).to_string(), false),
                PathSpec::Regex(r) => (r.regex.clone(), true),
            };
            let has_refs = role == Role::Referencing && references(&src, is_regex);
            if has_refs {
                items.push(Item::Template(Template {
                    source: src,
                    is_regex,
                    cache: Mutex::default(),
                }));
            } else {
                let re = if is_regex {
                    src.clone()
                } else {
                    glob_to_regex(&src, role == Role::Capturing)
                };
                items.push(Item::Static(
                    Regex::new(&re).map_err(|e| format!("invalid pattern `{src}`: {e}"))?,
                ));
            }
        }
        Ok(Self { items })
    }

    pub fn empty() -> Self {
        Self { items: vec![] }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Does any pattern match `path`? Patterns that refer to captures that
    /// `caps` lacks never match.
    pub fn is_match(&self, path: &str, caps: &Captures) -> bool {
        self.items.iter().any(|i| match i {
            Item::Static(re) => re.is_match(path),
            Item::Template(t) => t.regex(caps).is_some_and(|re| re.is_match(path)),
        })
    }

    /// Captures of the first matching pattern, or `None` if none matches.
    /// An empty pattern list matches everything with no captures.
    pub fn capture(&self, path: &str) -> Option<Captures> {
        if self.items.is_empty() {
            return Some(Captures::default());
        }
        for i in &self.items {
            if let Item::Static(re) = i
                && let Some(c) = re.captures(path)
            {
                let mut caps = Captures::default();
                for (idx, name) in re.capture_names().enumerate().skip(1) {
                    if let Some(m) = c.get(idx) {
                        caps.insert(idx.to_string(), m.as_str().to_string());
                        if let Some(n) = name {
                            caps.insert(n.to_string(), m.as_str().to_string());
                        }
                    }
                }
                return Some(caps);
            }
        }
        None
    }
}

impl Template {
    fn regex(&self, caps: &Captures) -> Option<Regex> {
        let (substituted, key) = substitute(&self.source, self.is_regex, caps)?;
        let mut cache = self.cache.lock().expect("poisoned");
        cache
            .entry(key)
            .or_insert_with(|| {
                let re = if self.is_regex {
                    substituted
                } else {
                    glob_to_regex(&substituted, false)
                };
                Regex::new(&re).ok()
            })
            .clone()
    }
}

fn references(src: &str, is_regex: bool) -> bool {
    if is_regex {
        let b = src.as_bytes();
        b.windows(2).any(|w| w[0] == b'$' && w[1].is_ascii_digit())
    } else {
        placeholder_spans(src).next().is_some()
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
fn substitute(src: &str, is_regex: bool, caps: &Captures) -> Option<(String, String)> {
    let mut out = String::new();
    let mut key = String::new();
    if is_regex {
        let b = src.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i] == b'$' && b.get(i + 1).is_some_and(u8::is_ascii_digit) {
                let n = (b[i + 1] - b'0').to_string();
                let v = caps.get(&n)?;
                out.push_str(&regex::escape(v));
                key.push_str(v);
                key.push('\0');
                i += 2;
            } else {
                // push the whole char, not a byte
                let ch = src[i..].chars().next()?;
                out.push(ch);
                i += ch.len_utf8();
            }
        }
    } else {
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
    }
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

    fn glob(s: &str) -> PathSpec {
        PathSpec::glob(s)
    }
    fn re(s: &str) -> PathSpec {
        PathSpec::regex(s)
    }
    fn pats(specs: &[PathSpec], role: Role) -> PathPatterns {
        PathPatterns::new(specs, role).unwrap()
    }
    fn none() -> Captures {
        Captures::default()
    }

    #[test]
    fn glob_semantics() {
        let p = pats(
            &[
                glob("src/*.ts"),
                glob("lib"),
                glob("**/*.test.ts"),
                glob("a/**/z.ts"),
                glob("x/**"),
            ],
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
        let p = pats(
            &[glob("src/*.{ts,tsx}"), glob("[ab]/f.js")],
            Role::Referencing,
        );
        assert!(p.is_match("src/a.ts", &none()) && p.is_match("src/a.tsx", &none()));
        assert!(!p.is_match("src/a.js", &none()));
        assert!(p.is_match("a/f.js", &none()) && !p.is_match("c/f.js", &none()));
    }

    #[test]
    fn named_captures_in_globs() {
        let from = pats(&[glob("src/features/{feature}/**")], Role::Capturing);
        let caps = from.capture("src/features/billing/ui/x.ts").unwrap();
        assert_eq!(caps["feature"], "billing");
        assert!(from.capture("src/other/x.ts").is_none());

        let to = pats(&[glob("src/features/{feature}/**")], Role::Referencing);
        assert!(to.is_match("src/features/billing/api.ts", &caps));
        assert!(!to.is_match("src/features/auth/api.ts", &caps));
        // a placeholder without a capture never matches
        assert!(!to.is_match("src/features/billing/api.ts", &none()));
    }

    #[test]
    fn regex_backreferences() {
        let from = pats(&[re(r"^src/([^/]+)/")], Role::Capturing);
        let caps = from.capture("src/auth/login.ts").unwrap();
        assert_eq!(caps["1"], "auth");
        let to = pats(&[re(r"^src/$1/")], Role::Referencing);
        assert!(to.is_match("src/auth/x.ts", &caps));
        assert!(!to.is_match("src/billing/x.ts", &caps));
    }

    #[test]
    fn captured_text_is_escaped() {
        let from = pats(&[glob("pkg/{name}/**")], Role::Capturing);
        let caps = from.capture("pkg/a.b/x.ts").unwrap();
        let to = pats(&[glob("pkg/{name}/**")], Role::Referencing);
        assert!(to.is_match("pkg/a.b/y.ts", &caps));
        assert!(!to.is_match("pkg/aXb/y.ts", &caps));
    }

    #[test]
    fn empty_matches_everything_for_capture() {
        assert!(PathPatterns::empty().capture("anything").is_some());
        assert!(!PathPatterns::empty().is_match("anything", &none()));
        assert!(PathPatterns::new(&[re("(")], Role::Capturing).is_err());
    }
}
