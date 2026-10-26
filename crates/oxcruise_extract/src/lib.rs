//! Per-file dependency extraction.
//!
//! One file in, one [`FileFacts`] out. The oxc arena is owned by the caller and
//! can be reset as soon as [`extract`] returns, because facts own all their data.

mod comments;
mod facts;
mod sfc;
mod visitor;

use std::path::Path;

use oxc_allocator::Allocator;
use oxc_parser::Parser;
use oxc_span::SourceType;
use oxcruise_core::{LineIndex, Span};

pub use facts::{
    ExportFact, ExportKind, ExtractOptions, FileFacts, ImportFact, RawDependency, Suppression,
};

/// Extensions oxcruise will parse as source.
pub const SOURCE_EXTENSIONS: &[&str] = &[
    "js", "mjs", "cjs", "jsx", "ts", "mts", "cts", "tsx", "vue", "svelte", "astro",
];

pub fn is_source_path(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| SOURCE_EXTENSIONS.contains(&e))
}

/// Parse `source` and collect its dependencies.
///
/// `path` only selects the language (by extension); nothing is read from disk.
pub fn extract(
    path: &Path,
    source: &str,
    opts: &ExtractOptions,
    allocator: &Allocator,
) -> FileFacts {
    let line_index = LineIndex::new(source);
    let mut builder = facts::FactsBuilder::new(&line_index, opts);

    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if matches!(ext, "vue" | "svelte" | "astro") {
        for block in sfc::split(ext, source) {
            parse_block(
                allocator,
                &source[block.start..block.end],
                block.start as u32,
                block.source_type,
                &mut builder,
            );
        }
    } else {
        match SourceType::from_path(path) {
            Ok(st) => parse_block(allocator, source, 0, st, &mut builder),
            Err(e) => builder.parse_error = Some(e.to_string()),
        }
    }
    builder.finish()
}

fn parse_block(
    allocator: &Allocator,
    text: &str,
    offset: u32,
    source_type: SourceType,
    builder: &mut facts::FactsBuilder<'_>,
) {
    let ret = Parser::new(allocator, text, source_type).parse();
    if ret.fatal_error {
        let msg = ret
            .diagnostics
            .first()
            .map_or_else(|| "parse error".to_string(), |d| d.message.to_string());
        builder.parse_error.get_or_insert(msg);
        return;
    }
    visitor::collect_module_record(&ret.module_record, offset, builder);
    visitor::collect_ast(&ret.program, offset, builder);
    comments::collect(&ret.program, text, offset, builder);
}

pub(crate) fn shift(span: oxc_span::Span, offset: u32) -> Span {
    Span::new(span.start + offset, span.end + offset)
}
