use std::collections::BTreeSet;
use std::io::{self, Write};

use oxcruise_core::{DepKinds, ModuleKind, Severity};

use crate::CruiseResult;

fn q(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Graphviz: dashed edges are type-only, dotted are dynamic, colored edges break a rule.
pub fn render(result: &CruiseResult, out: &mut dyn Write) -> io::Result<()> {
    let bad: BTreeSet<&str> = result
        .violations
        .iter()
        .flat_map(|v| [Some(v.from.as_str()), v.to.as_deref()])
        .flatten()
        .collect();

    writeln!(out, "digraph oxcruise {{")?;
    writeln!(out, "  rankdir=LR;")?;
    writeln!(
        out,
        "  node [shape=box, fontname=\"Helvetica\", fontsize=10];"
    )?;
    writeln!(out, "  edge [fontname=\"Helvetica\", fontsize=9];")?;
    for m in &result.modules {
        let shape = match m.kind {
            ModuleKind::Core => "ellipse",
            ModuleKind::Stub => "component",
            _ => "box",
        };
        let color = if bad.contains(m.path.as_str()) {
            ", color=red, fontcolor=red"
        } else {
            ""
        };
        writeln!(out, "  {} [shape={shape}{color}];", q(&m.path))?;
    }
    for m in &result.modules {
        for d in &m.dependencies {
            let mut attrs = vec![];
            if d.kinds.contains(DepKinds::TYPE_ONLY) {
                attrs.push("style=dashed".to_string());
            } else if d.kinds.contains(DepKinds::DYNAMIC_IMPORT) {
                attrs.push("style=dotted".to_string());
            }
            if let Some(worst) = d.rules.iter().map(|r| r.severity).min() {
                attrs.push(format!(
                    "color={}",
                    if worst == Severity::Error {
                        "red"
                    } else {
                        "orange"
                    }
                ));
                attrs.push(format!("label={}", q(&d.rules[0].name)));
            }
            let attrs = if attrs.is_empty() {
                String::new()
            } else {
                format!(" [{}]", attrs.join(", "))
            };
            writeln!(out, "  {} -> {}{attrs};", q(&m.path), q(&d.to))?;
        }
    }
    writeln!(out, "}}")
}
