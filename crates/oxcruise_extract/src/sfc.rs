//! Splits single-file components (`.vue`, `.svelte`, `.astro`) into the script
//! blocks that oxc can parse. Offsets are byte positions in the original file so
//! dependency locations can be mapped back.

use oxc_span::SourceType;

pub(crate) struct Block {
    pub start: usize,
    pub end: usize,
    pub source_type: SourceType,
}

pub(crate) fn split(ext: &str, source: &str) -> Vec<Block> {
    let mut blocks = vec![];
    let mut scan_from = 0;
    if ext == "astro"
        && let Some(b) = astro_frontmatter(source)
    {
        scan_from = b.end;
        blocks.push(b);
    }
    blocks.extend(script_tags(source, scan_from));
    blocks
}

/// The `---` fenced frontmatter at the top of an Astro file (TypeScript).
fn astro_frontmatter(source: &str) -> Option<Block> {
    let trimmed = source.trim_start();
    let lead = source.len() - trimmed.len();
    let rest = trimmed.strip_prefix("---")?;
    let end_rel = rest.find("\n---")?;
    let start = lead + 3;
    Some(Block {
        start,
        end: start + end_rel,
        source_type: SourceType::ts(),
    })
}

fn script_tags(source: &str, from: usize) -> Vec<Block> {
    let mut out = vec![];
    let lower = source.to_ascii_lowercase();
    let mut pos = from;
    while let Some(i) = lower[pos..].find("<script") {
        let tag_start = pos + i;
        let after_name = tag_start + "<script".len();
        // `<scripts>` or `<script-foo>` are not script tags.
        match lower.as_bytes().get(after_name) {
            Some(b' ' | b'\t' | b'\n' | b'\r' | b'>' | b'/') => {}
            _ => {
                pos = after_name;
                continue;
            }
        }
        let Some(gt) = find_tag_end(source, after_name) else {
            break;
        };
        let attrs = &source[after_name..gt];
        let self_closing = attrs.trim_end().ends_with('/');
        let body_start = gt + 1;
        if self_closing {
            pos = body_start;
            continue;
        }
        let Some(close) = lower[body_start..].find("</script") else {
            break;
        };
        let body_end = body_start + close;
        pos = body_end;
        if let Some(st) = source_type_from_attrs(attrs) {
            out.push(Block {
                start: body_start,
                end: body_end,
                source_type: st,
            });
        }
    }
    out
}

/// Index of the `>` that closes the tag opening at `from`, skipping quoted attribute values.
fn find_tag_end(source: &str, from: usize) -> Option<usize> {
    let mut quote: Option<u8> = None;
    for (i, &b) in source.as_bytes().iter().enumerate().skip(from) {
        match (quote, b) {
            (None, b'"' | b'\'') => quote = Some(b),
            (Some(q), _) if q == b => quote = None,
            (None, b'>') => return Some(i),
            _ => {}
        }
    }
    None
}

fn source_type_from_attrs(attrs: &str) -> Option<SourceType> {
    let lang = attr(attrs, "lang").or_else(|| attr(attrs, "type"));
    let ty = attr(attrs, "type");
    // `<script type="application/json">`, `text/template`, ... are not code.
    if let Some(t) = ty {
        let t = t.to_ascii_lowercase();
        if !(t.is_empty() || t.contains("javascript") || t.contains("typescript") || t == "module")
        {
            return None;
        }
    }
    Some(match lang.map(str::to_ascii_lowercase).as_deref() {
        Some("ts" | "typescript") => SourceType::ts(),
        Some("tsx") => SourceType::tsx(),
        Some("jsx") => SourceType::jsx(),
        _ => SourceType::mjs(),
    })
}

fn attr<'a>(attrs: &'a str, name: &str) -> Option<&'a str> {
    let mut from = 0;
    while let Some(i) = attrs[from..].find(name) {
        let at = from + i;
        from = at + name.len();
        let boundary = at == 0 || attrs.as_bytes()[at - 1].is_ascii_whitespace();
        let rest = attrs[at + name.len()..].trim_start();
        if !boundary || !rest.starts_with('=') {
            continue;
        }
        let rest = rest[1..].trim_start();
        return Some(match rest.chars().next() {
            Some(q @ ('"' | '\'')) => {
                let inner = &rest[1..];
                &inner[..inner.find(q)?]
            }
            _ => rest.split(|c: char| c.is_whitespace() || c == '>').next()?,
        });
    }
    None
}
