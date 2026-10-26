use std::io::{self, Write};

use oxcruise_core::ModuleKind;

use crate::CruiseResult;

/// One `from -> to` line per dependency, with the kinds that are not obvious.
pub fn render(result: &CruiseResult, out: &mut dyn Write) -> io::Result<()> {
    for m in result
        .modules
        .iter()
        .filter(|m| m.kind == ModuleKind::Source)
    {
        for d in &m.dependencies {
            let flags: Vec<&str> = d
                .kinds
                .names()
                .filter(|n| matches!(*n, "type-only" | "dynamic-import"))
                .collect();
            if flags.is_empty() {
                writeln!(out, "{} -> {}", m.path, d.to)?;
            } else {
                writeln!(out, "{} -> {} [{}]", m.path, d.to, flags.join(" "))?;
            }
        }
    }
    Ok(())
}
