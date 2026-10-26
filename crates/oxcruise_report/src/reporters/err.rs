use std::io::{self, Write};

use oxcruise_core::Severity;

use super::ReporterOptions;
use crate::{CruiseResult, ViolationKind, ViolationOut};

struct Paint(bool);

impl Paint {
    fn wrap(&self, code: &str, s: &str) -> String {
        if self.0 {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }
    fn bold(&self, s: &str) -> String {
        self.wrap("1", s)
    }
    fn dim(&self, s: &str) -> String {
        self.wrap("2", s)
    }
    fn severity(&self, s: Severity, text: &str) -> String {
        match s {
            Severity::Error => self.wrap("1;31", text),
            Severity::Warn => self.wrap("1;33", text),
            _ => self.wrap("1;36", text),
        }
    }
}

pub fn render(
    result: &CruiseResult,
    opts: &ReporterOptions,
    long: bool,
    out: &mut dyn Write,
) -> io::Result<()> {
    let p = Paint(opts.color);

    for m in result.modules.iter().filter(|m| m.parse_error.is_some()) {
        writeln!(
            out,
            "{} could not parse {}: {}",
            p.severity(Severity::Warn, "warning:"),
            m.path,
            m.parse_error.as_deref().unwrap_or("")
        )?;
    }
    for w in &result.warnings {
        writeln!(out, "{} {w}", p.severity(Severity::Warn, "warning:"))?;
    }

    for v in &result.violations {
        writeln!(
            out,
            "{}{}: {}",
            p.severity(v.severity, v.severity.as_str()),
            p.dim(&format!("[{}]", v.rule)),
            p.bold(&v.message)
        )?;
        match (v.line, v.col) {
            (Some(l), Some(c)) => writeln!(out, "  {} {}:{l}:{c}", p.dim("-->"), v.from)?,
            _ => writeln!(out, "  {} {}", p.dim("-->"), v.from)?,
        }
        snippet(result, v, &p, out)?;
        if let Some(to) = &v.to
            && v.kind == ViolationKind::Dependency
        {
            writeln!(out, "   {} imports {to}", p.dim("="))?;
        }
        cycle(v, &p, out)?;
        writeln!(out)?;
    }

    if long {
        let mut seen: Vec<&str> = vec![];
        for v in &result.violations {
            if seen.contains(&v.rule.as_str()) {
                continue;
            }
            seen.push(&v.rule);
            if let Some((_, docs)) = opts.rule_docs.iter().find(|(n, _)| *n == v.rule) {
                writeln!(out, "{}", p.bold(&format!("About `{}`", v.rule)))?;
                for line in docs.lines() {
                    writeln!(out, "  {line}")?;
                }
                writeln!(out)?;
            }
        }
    }

    let s = &result.summary;
    let total = s.error + s.warn + s.info;
    if total == 0 {
        writeln!(
            out,
            "{} no problems found in {} modules, {} dependencies",
            p.wrap("1;32", "✔"),
            s.total_modules,
            s.total_dependencies
        )?;
    } else {
        writeln!(
            out,
            "{} {total} problem{} ({} error{}, {} warning{}, {} info) in {} modules, {} dependencies",
            p.wrap("1;31", "✖"),
            if total == 1 { "" } else { "s" },
            s.error,
            if s.error == 1 { "" } else { "s" },
            s.warn,
            if s.warn == 1 { "" } else { "s" },
            s.info,
            s.total_modules,
            s.total_dependencies,
        )?;
    }
    if s.suppressed > 0 {
        writeln!(out, "  {} suppressed by comments", s.suppressed)?;
    }
    Ok(())
}

fn snippet(
    result: &CruiseResult,
    v: &ViolationOut,
    p: &Paint,
    out: &mut dyn Write,
) -> io::Result<()> {
    let (Some(line), Some(col)) = (v.line, v.col) else {
        return Ok(());
    };
    let Ok(text) = std::fs::read_to_string(result.root.join(&v.from)) else {
        return Ok(());
    };
    let Some(src) = text.lines().nth(line as usize - 1) else {
        return Ok(());
    };
    let gutter = line.to_string();
    let pad = " ".repeat(gutter.len());
    let len = v.specifier_len.unwrap_or(1) as usize;
    writeln!(out, " {pad} {}", p.dim("|"))?;
    writeln!(out, " {} {} {src}", p.dim(&gutter), p.dim("|"))?;
    writeln!(
        out,
        " {pad} {} {}{}",
        p.dim("|"),
        " ".repeat(col as usize - 1),
        p.severity(v.severity, &"^".repeat(len.max(1)))
    )
}

/// A cycle drawn as a loop, like oxlint's no-cycle.
fn cycle(v: &ViolationOut, p: &Paint, out: &mut dyn Write) -> io::Result<()> {
    if v.cycle.is_empty() {
        return Ok(());
    }
    let n = v.cycle.len();
    let label = if v.kind == ViolationKind::Reachability {
        "path"
    } else {
        "cycle"
    };
    writeln!(out, "   {} {label}:", p.dim("="))?;
    if n == 1 {
        return writeln!(
            out,
            "     {} {} {}",
            p.dim("╰─▶"),
            v.cycle[0].module,
            p.dim("(imports itself)")
        );
    }
    for (i, hop) in v.cycle.iter().enumerate() {
        let lead = match (i, v.kind) {
            (0, ViolationKind::Cycle) => "╭─▶",
            (_, ViolationKind::Cycle) if i == n - 1 => "╰──",
            (_, ViolationKind::Cycle) => "│  ",
            (0, _) => "   ",
            _ => " └▶",
        };
        writeln!(out, "     {} {}", p.dim(lead), hop.module)?;
    }
    Ok(())
}
