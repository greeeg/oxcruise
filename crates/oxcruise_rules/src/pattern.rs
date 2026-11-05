//! Path patterns for rules: globs with `!` negation and captures.
//!
//! * A glob may contain `{name}`, which captures one path segment on the
//!   `from` side and is substituted on the `to` side: `src/features/{f}/**`.
//! * A leading `!` negates a glob. A list matches a path when some positive glob
//!   matches and no negated glob does; a list of only negations means "everything
//!   except". Negated globs may refer to captures too.
//!
//! Everything compiles to the `regex` crate, so matching is linear-time.

use std::sync::{Arc, RwLock};

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
    /// The text that every path matching a positive glob starts with.
    prefix: String,
}

#[derive(Debug)]
enum Item {
    Static(Regex),
    Template(Template),
}

#[derive(Debug)]
struct Template {
    source: String,
    exact: bool,
    cache: RwLock<FxHashMap<String, Option<Arc<Regex>>>>,
}

impl Item {
    fn compile(glob: &str, capturing: bool, exact: bool) -> Result<Item, String> {
        let src = glob.strip_prefix("./").unwrap_or(glob);
        if !capturing && placeholder_spans(src).next().is_some() {
            return Ok(Item::Template(Template {
                source: src.to_string(),
                exact,
                cache: RwLock::default(),
            }));
        }
        Regex::new(&glob_regex(src, capturing, exact))
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
        Self::build(globs, role, false)
    }

    /// Like [`PathPatterns::new`], but a glob matches only the paths it names. `src/pages` then
    /// does not match `src/pages/a`, as it does elsewhere. For rules that match one folder or file
    /// at a time.
    pub fn exact(globs: &[String], role: Role) -> Result<Self, String> {
        Self::build(globs, role, true)
    }

    fn build(globs: &[String], role: Role, exact: bool) -> Result<Self, String> {
        let (mut positive, mut negative) = (vec![], vec![]);
        let mut prefix: Option<&str> = None;
        for g in globs {
            match g.strip_prefix('!') {
                Some(neg) => negative.push(Item::compile(neg, false, exact)?),
                None => {
                    positive.push(Item::compile(g, role == Role::Capturing, exact)?);
                    let literal = literal_prefix(g.strip_prefix("./").unwrap_or(g));
                    prefix = Some(match prefix {
                        Some(p) => common_prefix(p, literal),
                        None => literal,
                    });
                }
            }
        }
        Ok(Self {
            positive,
            negative,
            prefix: prefix.unwrap_or_default().to_string(),
        })
    }

    pub fn empty() -> Self {
        Self {
            positive: vec![],
            negative: vec![],
            prefix: String::new(),
        }
    }

    /// The text that every path this list matches starts with: the part of its globs before the
    /// first wildcard or capture. Empty if there is none, or if the list only negates. A cheap
    /// filter for the paths to try.
    pub fn literal_prefix(&self) -> &str {
        &self.prefix
    }

    pub fn is_empty(&self) -> bool {
        self.positive.is_empty() && self.negative.is_empty()
    }

    /// Does some glob refer to a capture? If not, a match depends on the path alone.
    pub fn refers_to_captures(&self) -> bool {
        self.positive
            .iter()
            .chain(&self.negative)
            .any(|i| matches!(i, Item::Template(_)))
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
                // no groups: nothing to extract, and `is_match` is cheaper than `captures`
                Item::Static(re) if re.captures_len() == 1 => {
                    re.is_match(path).then(Captures::default)
                }
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
    /// Shared, not cloned: a `Regex` clone starts a new scratch-space pool, which costs more than
    /// a match.
    fn regex(&self, caps: &Captures) -> Option<Arc<Regex>> {
        thread_local! {
            static KEY: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
        }
        // the key alone decides a hit; it is built in a reused buffer, and the substituted glob
        // and an owned key only on a miss
        KEY.with_borrow_mut(|key| {
            key.clear();
            if !cache_key(&self.source, caps, key) {
                return None;
            }
            if let Some(hit) = self.cache.read().expect("poisoned").get(key.as_str()) {
                return hit.clone();
            }
            let (substituted, _) = substitute(&self.source, caps)?;
            let compiled = Regex::new(&glob_regex(&substituted, false, self.exact))
                .ok()
                .map(Arc::new);
            self.cache
                .write()
                .expect("poisoned")
                .entry(key.clone())
                .or_insert(compiled)
                .clone()
        })
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

/// Write the cache key of a template under `caps` into `key`: the captured values it refers to.
/// `false` when a placeholder has no capture.
fn cache_key(src: &str, caps: &Captures, key: &mut String) -> bool {
    for (_, _, name) in placeholder_spans(src) {
        let Some(v) = caps.get(name) else {
            return false;
        };
        key.push_str(v);
        key.push('\0');
    }
    true
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

/// The text of `glob` before its first wildcard, `{` or escape.
fn literal_prefix(glob: &str) -> &str {
    &glob[..glob.find(['*', '?', '[', '{', '\\']).unwrap_or(glob.len())]
}

fn common_prefix<'a>(a: &'a str, b: &str) -> &'a str {
    let len = a
        .char_indices()
        .zip(b.chars())
        .take_while(|((_, x), y)| x == y)
        .last()
        .map_or(0, |((i, c), _)| i + c.len_utf8());
    &a[..len]
}

/// A glob that is text and `{name}` references only, such as `{c}.test.tsx`. Once the captures
/// are known it names one path, so it needs no regex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Literal {
    parts: Vec<LiteralPart>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum LiteralPart {
    Text(String),
    Capture(String),
}

impl Literal {
    /// `None` if `glob` has a wildcard, an alternation or an escape.
    pub fn parse(glob: &str) -> Option<Literal> {
        let src = glob.strip_prefix("./").unwrap_or(glob);
        let mut parts = vec![];
        let mut last = 0;
        for (start, end, name) in placeholder_spans(src) {
            parts.push(LiteralPart::Text(src[last..start].to_string()));
            parts.push(LiteralPart::Capture(name.to_string()));
            last = end;
        }
        parts.push(LiteralPart::Text(src[last..].to_string()));
        let plain = parts.iter().all(|p| match p {
            LiteralPart::Text(t) => !t.contains(['*', '?', '[', ']', '{', '}', '\\']),
            LiteralPart::Capture(_) => true,
        });
        plain.then_some(Literal { parts })
    }

    /// The path, with each reference replaced by its capture. `None` if `caps` lacks one.
    pub fn resolve(&self, caps: &Captures) -> Option<String> {
        let mut out = String::new();
        for p in &self.parts {
            match p {
                LiteralPart::Text(t) => out.push_str(t),
                LiteralPart::Capture(name) => out.push_str(caps.get(name)?),
            }
        }
        Some(out)
    }
}

/// `src`, with each `{name}` that `caps` has replaced by its value. For messages.
pub fn fill_captures(src: &str, caps: &Captures) -> String {
    let mut out = String::new();
    let mut last = 0;
    for (start, end, name) in placeholder_spans(src) {
        out.push_str(&src[last..start]);
        match caps.get(name) {
            Some(v) => out.push_str(v),
            None => out.push_str(&src[start..end]),
        }
        last = end;
    }
    out.push_str(&src[last..]);
    out
}

/// Translate a glob to an anchored regex. A glob also matches everything beneath a
/// directory it matches. With `capturing`, `{ident}` becomes a named group.
pub fn glob_to_regex(glob: &str, capturing: bool) -> String {
    glob_regex(glob, capturing, false)
}

/// [`glob_to_regex`], where `exact` makes a glob match only the paths it names.
fn glob_regex(glob: &str, capturing: bool, exact: bool) -> String {
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
    if !ends_with_globstar_dir && !exact {
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

    /// `tests/fixtures/glob-cases.json` is also run by the JS `matchesGlob` (apps/oxcruise).
    #[test]
    fn shared_glob_table() {
        let table: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/glob-cases.json")).unwrap();
        for case in table["cases"].as_array().unwrap() {
            let (glob, path, want) = (
                case[0].as_str().unwrap(),
                case[1].as_str().unwrap(),
                case[2].as_bool().unwrap(),
            );
            let p = pats(&[glob], Role::Referencing);
            assert_eq!(
                p.is_match(path, &none()),
                want,
                "PathPatterns: {glob} vs {path}"
            );
            let m = oxcruise_graph::PathMatcher::new(&[glob]).unwrap();
            assert_eq!(m.is_match(path), want, "PathMatcher: {glob} vs {path}");
        }
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

    fn exact(globs: &[&str], role: Role) -> PathPatterns {
        let globs: Vec<String> = globs.iter().map(|g| g.to_string()).collect();
        PathPatterns::exact(&globs, role).unwrap()
    }

    #[test]
    fn exact_globs_match_only_what_they_name() {
        let p = exact(&["src/pages"], Role::Referencing);
        assert!(p.is_match("src/pages", &none()));
        assert!(!p.is_match("src/pages/a", &none()));
        // the ordinary form also matches below the folder
        assert!(pats(&["src/pages"], Role::Referencing).is_match("src/pages/a", &none()));
        // `**` still reaches below, because the glob says so
        let deep = exact(&["src/**"], Role::Referencing);
        assert!(deep.is_match("src", &none()) && deep.is_match("src/a/b.ts", &none()));
        // captures and templates are exact too
        let c = exact(&["src/{f}"], Role::Capturing);
        assert_eq!(c.capture("src/a").unwrap()["f"], "a");
        assert!(c.capture("src/a/b").is_none());
        let t = exact(&["{f}.test.ts"], Role::Referencing);
        let caps: Captures = [("f".to_string(), "a".to_string())].into_iter().collect();
        assert!(t.is_match("a.test.ts", &caps) && !t.is_match("a.test.ts/x", &caps));
    }

    #[test]
    fn the_literal_prefix_is_the_text_before_the_first_wildcard() {
        let prefix = |globs: &[&str]| exact(globs, Role::Capturing).literal_prefix().to_string();
        assert_eq!(prefix(&["src/pages/{p}"]), "src/pages/");
        assert_eq!(prefix(&["src/**/x.ts"]), "src/");
        assert_eq!(prefix(&["src/a/*.ts", "src/b/*.ts"]), "src/");
        assert_eq!(prefix(&["src/a", "lib/b"]), "");
        assert_eq!(prefix(&["!src/a"]), "");
        assert_eq!(prefix(&["src/a", "!src/b"]), "src/a");
        assert_eq!(prefix(&["ü/{x}", "ü/y"]), "ü/");
    }

    #[test]
    fn literals_name_one_path_once_the_captures_are_known() {
        let caps: Captures = [("c".to_string(), "Button".to_string())]
            .into_iter()
            .collect();
        let lit = Literal::parse("{c}.test.tsx").unwrap();
        assert_eq!(lit.resolve(&caps).unwrap(), "Button.test.tsx");
        assert_eq!(lit.resolve(&Captures::default()), None);
        assert_eq!(
            Literal::parse("api/index.ts")
                .unwrap()
                .resolve(&caps)
                .unwrap(),
            "api/index.ts"
        );
        for glob in ["*.ts", "index.{ts,tsx}", "a?.ts", "[ab].ts", "a\\*.ts"] {
            assert!(Literal::parse(glob).is_none(), "{glob}");
        }
        assert_eq!(fill_captures("{c}.{ts,tsx}", &caps), "Button.{ts,tsx}");
        assert_eq!(fill_captures("{d}.ts", &caps), "{d}.ts");
    }
}
