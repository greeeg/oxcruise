use std::collections::BTreeMap;
use std::io::{self, Write};

use oxcruise_core::{DepKinds, Severity};

use crate::CruiseResult;

fn label(s: &str) -> String {
    s.replace('"', "#quot;")
}

/// Mermaid flowchart: dashed links are type-only, dotted are dynamic, red links break a rule.
pub fn render(result: &CruiseResult, out: &mut dyn Write) -> io::Result<()> {
    let ids: BTreeMap<&str, usize> = result
        .modules
        .iter()
        .enumerate()
        .map(|(i, m)| (m.path.as_str(), i))
        .collect();
    writeln!(out, "flowchart LR")?;
    for m in &result.modules {
        writeln!(out, "  n{}[\"{}\"]", ids[m.path.as_str()], label(&m.path))?;
    }
    let mut link = 0usize;
    let mut red = vec![];
    for m in &result.modules {
        for d in &m.dependencies {
            let Some(to) = ids.get(d.to.as_str()) else {
                continue;
            };
            let arrow = if d.kinds.contains(DepKinds::TYPE_ONLY) {
                "-.->"
            } else if d.kinds.contains(DepKinds::DYNAMIC_IMPORT) {
                "-..->"
            } else {
                "-->"
            };
            writeln!(out, "  n{} {arrow} n{to}", ids[m.path.as_str()])?;
            if d.rules.iter().any(|r| r.severity == Severity::Error) {
                red.push(link);
            }
            link += 1;
        }
    }
    for l in red {
        writeln!(out, "  linkStyle {l} stroke:red,stroke-width:2px")?;
    }
    Ok(())
}
