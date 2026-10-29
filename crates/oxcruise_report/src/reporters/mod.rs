mod dot;
mod err;
mod github;
mod mermaid;
mod text;

use std::io::{self, Write};

use crate::{CruiseResult, OutputFormat};

#[derive(Debug, Clone, Default)]
pub struct ReporterOptions {
    /// ANSI colors in `default` output.
    pub color: bool,
    /// Rule documentation for `long`: `(rule name, docs)`.
    pub rule_docs: Vec<(String, String)>,
}

/// Plugin-provided reporters (`-f acme/json-lines`). The `oxcruise` app implements this
/// on top of its napi callbacks; the core reporters never depend on it.
pub trait ExternalReporters: Send + Sync {
    /// Render `result_json` (the native `json` format) with the named reporter.
    fn render(&self, name: &str, result_json: &str) -> Result<String, String>;
}

/// Render `result` in `format` to `out`.
pub fn render(
    format: &OutputFormat,
    result: &CruiseResult,
    opts: &ReporterOptions,
    external: Option<&dyn ExternalReporters>,
    out: &mut dyn Write,
) -> io::Result<()> {
    match format {
        OutputFormat::Human => err::render(result, opts, false, out),
        OutputFormat::Long => err::render(result, opts, true, out),
        OutputFormat::Json => {
            serde_json::to_writer_pretty(&mut *out, result).map_err(io::Error::other)?;
            writeln!(out)
        }
        OutputFormat::Github => github::render(result, out),
        OutputFormat::Dot => dot::render(result, out),
        OutputFormat::Mermaid => mermaid::render(result, out),
        OutputFormat::Text => text::render(result, out),
        OutputFormat::External(name) => {
            let host = external.ok_or_else(|| {
                io::Error::other(format!(
                    "reporter `{name}` is provided by a plugin, which needs the npm package (`npx oxcruise`)"
                ))
            })?;
            let json = serde_json::to_string(result).map_err(io::Error::other)?;
            let rendered = host.render(name, &json).map_err(io::Error::other)?;
            out.write_all(rendered.as_bytes())
        }
    }
}
