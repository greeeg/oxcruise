use oxc_ast::ast::Program;
use oxc_span::GetSpan;
use oxcruise_core::{DepKinds, ModuleSystem, Span};

use crate::facts::{FactsBuilder, PendingSuppression, SuppressionKind};

pub(crate) fn collect(program: &Program<'_>, text: &str, offset: u32, b: &mut FactsBuilder<'_>) {
    let first_stmt = program.body.first().map(|s| s.span().start);
    for c in &program.comments {
        let content = c.content_span();
        let body = &text[content.start as usize..content.end as usize];
        let abs_start = c.span.start + offset;
        let abs_end = c.span.end + offset;

        if let Some(s) = parse_suppression(body, abs_start, abs_end) {
            b.suppressions.push(s);
            continue;
        }

        if c.is_line() && !b.opts.skip_triple_slash && first_stmt.is_none_or(|s| c.span.start < s) {
            triple_slash(body, content.start + offset, c.span.start + offset, b);
        }

        if b.opts.jsdoc && c.is_jsdoc() {
            jsdoc(body, content.start + offset, c.span.start + offset, b);
        }
    }
}

fn parse_suppression(body: &str, start: u32, end: u32) -> Option<PendingSuppression> {
    let body = body.trim().trim_start_matches('*').trim();
    let (kind, rest) = if let Some(r) = body.strip_prefix("oxcruise-ignore-next-line") {
        (SuppressionKind::NextLine, r)
    } else if let Some(r) = body.strip_prefix("oxcruise-ignore-line") {
        (SuppressionKind::Line, r)
    } else {
        let r = body.strip_prefix("oxcruise-ignore-file")?;
        (SuppressionKind::File, r)
    };
    // `-- reason` ends the rule list.
    let rest = rest.split("--").next().unwrap_or("");
    if !rest.is_empty() && !rest.starts_with(char::is_whitespace) {
        return None; // e.g. `oxcruise-ignore-lines`
    }
    let rules = rest
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    Some(PendingSuppression {
        kind,
        rules,
        start,
        end,
    })
}

/// `/// <reference path="./x.d.ts" />` and friends.
fn triple_slash(body: &str, content_start: u32, stmt_start: u32, b: &mut FactsBuilder<'_>) {
    // A line comment's content starts after `//`, so triple-slash bodies begin with `/`.
    let Some(rest) = body.strip_prefix('/') else {
        return;
    };
    let rest = rest.trim_start();
    let (tag, ms_kind) = if rest.starts_with("<reference") {
        ("<reference", None)
    } else if rest.starts_with("<amd-dependency") {
        (
            "<amd-dependency",
            Some(DepKinds::TRIPLE_SLASH_AMD_DEPENDENCY),
        )
    } else {
        return;
    };
    let attrs = &rest[tag.len()..];
    let attr_base = (body.len() - rest.len() + tag.len()) as u32 + content_start;
    for (attr, kind) in [
        ("path", DepKinds::TRIPLE_SLASH_FILE_REFERENCE),
        ("types", DepKinds::TRIPLE_SLASH_TYPE_REFERENCE),
    ] {
        let Some((value, vs, ve)) = attr_value(attrs, attr) else {
            continue;
        };
        let (kinds, ms) = match ms_kind {
            Some(k) if attr == "path" => (DepKinds::TRIPLE_SLASH_DIRECTIVE | k, ModuleSystem::Amd),
            Some(_) => continue,
            None => (DepKinds::TRIPLE_SLASH_DIRECTIVE | kind, ModuleSystem::Tsd),
        };
        let span = Span::new(attr_base + vs as u32, attr_base + ve as u32);
        b.push_dep(value, kinds, ms, span, stmt_start);
    }
}

/// Find `name="value"` (or single quotes) in `attrs`; returns the value and its byte range.
fn attr_value<'a>(attrs: &'a str, name: &str) -> Option<(&'a str, usize, usize)> {
    let mut search = 0;
    while let Some(i) = attrs[search..].find(name) {
        let at = search + i;
        search = at + name.len();
        let before_ok = at == 0 || attrs.as_bytes()[at - 1].is_ascii_whitespace();
        let after = attrs[at + name.len()..].trim_start();
        if !before_ok || !after.starts_with('=') {
            continue;
        }
        let after = after[1..].trim_start();
        let quote = after.chars().next().filter(|c| *c == '"' || *c == '\'')?;
        let val_start = attrs.len() - after.len() + 1;
        let len = attrs[val_start..].find(quote)?;
        return Some((
            &attrs[val_start..val_start + len],
            val_start,
            val_start + len,
        ));
    }
    None
}

/// JSDoc `@import {X} from "y"` tags and `import("y")` type expressions.
fn jsdoc(body: &str, content_start: u32, stmt_start: u32, b: &mut FactsBuilder<'_>) {
    let mut search = 0;
    while let Some(i) = body[search..].find("@import") {
        let at = search + i + "@import".len();
        search = at;
        if let Some(f) = body[at..].find("from") {
            let tail = &body[at + f + 4..];
            if let Some((v, s, e)) = first_quoted(tail) {
                let base = (at + f + 4) as u32 + content_start;
                b.push_dep(
                    v,
                    DepKinds::JSDOC | DepKinds::JSDOC_IMPORT_TAG | DepKinds::TYPE_ONLY,
                    ModuleSystem::Es6,
                    Span::new(base + s as u32, base + e as u32),
                    stmt_start,
                );
            }
        }
    }
    let mut search = 0;
    while let Some(i) = body[search..].find("import(") {
        let at = search + i + "import(".len();
        search = at;
        let before_ok = body[..at - "import(".len()]
            .chars()
            .next_back()
            .is_none_or(|c| !c.is_alphanumeric() && c != '_');
        if !before_ok {
            continue;
        }
        if let Some((v, s, e)) = first_quoted(&body[at..]) {
            // The quote must open the argument: `import("x")`, not `import(foo("x"))`.
            if body[at..at + s - 1].trim().is_empty() {
                let base = at as u32 + content_start;
                b.push_dep(
                    v,
                    DepKinds::JSDOC | DepKinds::JSDOC_BRACKET_IMPORT | DepKinds::TYPE_ONLY,
                    ModuleSystem::Es6,
                    Span::new(base + s as u32, base + e as u32),
                    stmt_start,
                );
            }
        }
    }
}

/// First `"..."` or `'...'` in `s`, as `(value, start, end)` of the value.
fn first_quoted(s: &str) -> Option<(&str, usize, usize)> {
    let (qi, q) = s.char_indices().find(|(_, c)| *c == '"' || *c == '\'')?;
    let rest = &s[qi + 1..];
    let len = rest.find(q)?;
    Some((&rest[..len], qi + 1, qi + 1 + len))
}
