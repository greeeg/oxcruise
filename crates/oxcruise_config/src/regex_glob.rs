//! Best-effort conversion of dependency-cruiser path regexes to globs, for `oxcruise migrate`.
//!
//! Only the shapes real configs use are handled: `^`/`(^|/)` anchors, a trailing `$`, literal
//! text (escaped or in a one-character class), `[^/]+` and `.*`, `(a|b)` groups and top-level
//! `a|b`. Anything else is an error that names what was not understood, so the caller can warn
//! and drop the rule instead of widening it.
//!
//! * `^src/` -> `src/**`
//! * `[.]spec[.]ts$` -> `**/*.spec.ts`
//! * `(^|/)x/` -> `**/x/**`
//! * `^src/([^/]+)/` -> `src/{c1}/**`, and `$1` on the `to` side -> `{c1}`

/// Convert `re` to one glob per top-level alternative.
pub fn regex_to_globs(re: &str) -> Result<Vec<String>, String> {
    let chars: Vec<char> = re.chars().collect();
    let mut globs = vec![];
    let mut group = 0;
    for alt in split_alternatives(&chars)? {
        globs.push(convert_alternative(alt, &mut group).map_err(|e| format!("`{re}` ({e})"))?);
    }
    Ok(globs)
}

/// The `{cN}` capture names a glob refers to.
pub fn placeholders(glob: &str) -> Vec<String> {
    let mut out = vec![];
    let mut rest = glob;
    while let Some(start) = rest.find("{c") {
        let Some(len) = rest[start..].find('}') else {
            break;
        };
        let name = &rest[start + 1..start + len];
        if name.len() > 1 && name[1..].bytes().all(|b| b.is_ascii_digit()) {
            out.push(name.to_string());
        }
        rest = &rest[start + len..];
    }
    out
}

/// Split at `|` outside groups and classes.
fn split_alternatives(chars: &[char]) -> Result<Vec<&[char]>, String> {
    let (mut depth, mut start, mut i) = (0usize, 0usize, 0usize);
    let mut out = vec![];
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 1,
            '[' => {
                while i < chars.len() && chars[i] != ']' {
                    i += 1;
                }
            }
            '(' => depth += 1,
            ')' => depth = depth.checked_sub(1).ok_or("unbalanced `)`")?,
            '|' if depth == 0 => {
                out.push(&chars[start..i]);
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    if depth != 0 {
        return Err("unbalanced `(`".into());
    }
    out.push(&chars[start..]);
    Ok(out)
}

fn convert_alternative(mut chars: &[char], group: &mut usize) -> Result<String, String> {
    let prefix = if chars.starts_with(&['(', '^', '|', '/', ')']) {
        chars = &chars[5..];
        "**/"
    } else if chars.first() == Some(&'^') {
        chars = &chars[1..];
        ""
    } else {
        "**/*"
    };
    let end_anchored =
        chars.last() == Some(&'$') && chars.len() > 1 && chars[chars.len() - 2] != '\\';
    if end_anchored {
        chars = &chars[..chars.len() - 1];
    }
    let body = convert_atoms(chars, group)?;
    let mut glob = format!("{prefix}{body}");
    if !end_anchored && !body.is_empty() {
        if body.ends_with('/') {
            glob.push_str("**");
        } else if !body.ends_with('*') {
            glob.push('*');
        }
    }
    // `**/*` followed by a body that starts with `*` would otherwise read as `***`
    while glob.contains("***") {
        glob = glob.replace("***", "**");
    }
    Ok(glob)
}

fn convert_atoms(chars: &[char], group: &mut usize) -> Result<String, String> {
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        match chars[i] {
            '\\' => {
                let c = *chars.get(i + 1).ok_or("trailing `\\`")?;
                if c.is_ascii_alphanumeric() {
                    return Err(format!("unsupported escape `\\{c}`"));
                }
                push_literal(&mut out, c);
                i += 2;
            }
            '[' => {
                let close = chars[i..]
                    .iter()
                    .position(|&c| c == ']')
                    .ok_or("unterminated `[`")?;
                let class: String = chars[i + 1..i + close].iter().collect();
                i += close + 1;
                match class.chars().collect::<Vec<_>>().as_slice() {
                    ['^', '/'] => {
                        match chars.get(i) {
                            Some('+' | '*') => i += 1,
                            _ => return Err("`[^/]` without `+` or `*`".into()),
                        }
                        out.push('*');
                    }
                    [c] if *c != '^' => push_literal(&mut out, *c),
                    _ => return Err(format!("unsupported character class `[{class}]`")),
                }
            }
            '.' => match chars.get(i + 1) {
                Some('*' | '+') => {
                    out.push_str("**");
                    i += 2;
                }
                _ => return Err("unsupported `.`".into()),
            },
            '(' => {
                let (inner, next) = group_body(chars, i)?;
                i = next;
                let (capturing, inner) = match inner.strip_prefix(&['?', ':']) {
                    Some(rest) => (false, rest),
                    None => (true, inner),
                };
                let number = if capturing {
                    *group += 1;
                    *group
                } else {
                    0
                };
                if matches!(chars.get(i), Some('?' | '*' | '+' | '{')) {
                    return Err("a quantifier after a group".into());
                }
                let alternatives = split_alternatives(inner)?;
                if alternatives.len() == 1 {
                    if capturing && alternatives[0] == ['[', '^', '/', ']', '+'] {
                        out.push_str(&format!("{{c{number}}}"));
                    } else {
                        // plain grouping; nested groups still count, in order
                        out.push_str(&convert_atoms(alternatives[0], group)?);
                    }
                } else {
                    let mut alts = vec![];
                    for alt in alternatives {
                        if alt.is_empty() {
                            return Err("an empty alternative".into());
                        }
                        let alt = convert_atoms(alt, group)?;
                        if alt.contains(',') {
                            return Err("a `,` inside an alternative".into());
                        }
                        alts.push(alt);
                    }
                    out.push_str(&format!("{{{}}}", alts.join(",")));
                }
            }
            '$' => match chars.get(i + 1) {
                Some(d) if d.is_ascii_digit() => {
                    out.push_str(&format!("{{c{d}}}"));
                    i += 2;
                }
                _ => return Err("`$` in the middle of a pattern".into()),
            },
            '^' | '|' | ')' | '{' | '}' | '*' | '+' | '?' => {
                return Err(format!("unsupported `{}`", chars[i]));
            }
            c => {
                push_literal(&mut out, c);
                i += 1;
            }
        }
    }
    Ok(out)
}

/// The text between the `(` at `open` and its matching `)`, and the index after it.
fn group_body(chars: &[char], open: usize) -> Result<(&[char], usize), String> {
    let (mut depth, mut i) = (0usize, open);
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 1,
            '[' => {
                while i < chars.len() && chars[i] != ']' {
                    i += 1;
                }
            }
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Ok((&chars[open + 1..i], i + 1));
                }
            }
            _ => {}
        }
        i += 1;
    }
    Err("unbalanced `(`".into())
}

fn push_literal(out: &mut String, c: char) {
    if matches!(c, '*' | '?' | '[' | ']' | '{' | '}' | '\\' | '!' | ',') {
        out.push('\\');
    }
    out.push(c);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(re: &str) -> String {
        let mut g = regex_to_globs(re).unwrap();
        assert_eq!(g.len(), 1, "{re}: {g:?}");
        g.remove(0)
    }

    #[test]
    fn common_shapes() {
        assert_eq!(one("^src/"), "src/**");
        assert_eq!(one("^src"), "src*");
        assert_eq!(one("[.]spec[.]ts$"), "**/*.spec.ts");
        assert_eq!(one(r"\.spec\.ts$"), "**/*.spec.ts");
        assert_eq!(one("(^|/)x/"), "**/x/**");
        assert_eq!(one("(^|/)node_modules/"), "**/node_modules/**");
        assert_eq!(one("^src/.*"), "src/**");
        assert_eq!(one("^(src|lib)/"), "{src,lib}/**");
        assert_eq!(one(r"\.(js|ts)$"), "**/*.{js,ts}");
        assert_eq!(one("^src/[^/]+/index[.]ts$"), "src/*/index.ts");
    }

    #[test]
    fn captures_and_backreferences() {
        assert_eq!(one("^src/([^/]+)/"), "src/{c1}/**");
        assert_eq!(one("^src/$1/"), "src/{c1}/**");
        assert_eq!(
            one("^packages/([^/]+)/src/(a|b)/"),
            "packages/{c1}/src/{a,b}/**"
        );
        assert_eq!(placeholders("src/{c1}/{c2}/x"), ["c1", "c2"]);
        assert!(placeholders("src/{a,b}/**").is_empty());
    }

    #[test]
    fn top_level_alternatives_become_several_globs() {
        assert_eq!(regex_to_globs("^src/|^lib/").unwrap(), ["src/**", "lib/**"]);
    }

    #[test]
    fn unsupported_shapes_are_errors() {
        for re in [
            r"^src/\d+/",
            "^src/[a-z]+/",
            "^a.b",
            "^a(b)?c",
            "a{2}",
            "^a(?=b)",
        ] {
            assert!(regex_to_globs(re).is_err(), "{re} should not convert");
        }
    }
}
