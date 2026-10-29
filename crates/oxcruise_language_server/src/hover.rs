//! Hover on an import specifier: where it resolves to and the rules that hit it.

use lsp_types::{Hover, HoverContents, MarkupContent, MarkupKind, Position, Range};

use crate::LspAnalysis;
use crate::diagnostics::rule_docs_url;
use crate::source::{Encoding, Source};

pub fn hover(
    analysis: &LspAnalysis,
    rel_path: &str,
    source: &Source,
    at: Position,
    enc: Encoding,
) -> Option<Hover> {
    let graph = &analysis.graph;
    let module = graph.find(rel_path)?;
    let line_text = source.line(at.line)?;
    let byte = enc.byte_offset(line_text, at.character) as u32 + 1;

    let edge = graph.out_edges(module).iter().find(|e| {
        e.loc.is_some_and(|l| {
            let len = e.specifier.len() as u32 + 2;
            l.line == at.line + 1 && (l.col..=l.col + len).contains(&byte)
        })
    })?;
    let loc = edge.loc?;
    let target = graph.module(edge.to);

    let kind = format!("{:?}", target.kind).to_lowercase();
    let mut text = format!(
        "**oxcruise** · `{}` resolves to `{}` ({kind})",
        edge.specifier, target.path
    );
    for v in analysis
        .violations
        .iter()
        .filter(|v| v.from == rel_path && v.line == Some(loc.line) && v.col == Some(loc.col))
    {
        let rule = match rule_docs_url(&v.rule) {
            Some(url) => format!("[{}]({url})", v.rule),
            None => format!("`{}`", v.rule),
        };
        text.push_str(&format!(
            "\n\n{rule} ({}): {}",
            v.severity.as_str(),
            v.message
        ));
    }

    let (l, start) = source.position(loc.line, loc.col, enc);
    let end = source
        .position(loc.line, loc.col + edge.specifier.len() as u32 + 2, enc)
        .1;
    Some(Hover {
        contents: HoverContents::Markup(MarkupContent {
            kind: MarkupKind::Markdown,
            value: text,
        }),
        range: Some(Range::new(Position::new(l, start), Position::new(l, end))),
    })
}
