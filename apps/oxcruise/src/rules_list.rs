//! `--rules`: the built-in rules, as a table by category or as JSON.

use std::io::Write;

use oxcruise_config::{Category, Level};
use oxcruise_rules::builtin_rules;

pub fn json(out: &mut dyn Write) {
    let list: Vec<_> = builtin_rules()
        .into_iter()
        .map(|r| {
            serde_json::json!({
                "name": r.name, "category": r.category.as_str(), "version": r.version,
                "docs": r.docs,
            })
        })
        .collect();
    let _ = writeln!(
        out,
        "{}",
        serde_json::to_string_pretty(&list).expect("json")
    );
}

pub fn table(out: &mut dyn Write) {
    let rules = builtin_rules();
    let width = rules.iter().map(|r| r.name.len()).max().unwrap_or(0);
    for category in Category::ALL {
        let level = match category.default_level() {
            Level::Off => "off",
            Level::Warn => "warn",
            Level::Error => "error",
        };
        let _ = writeln!(out, "{} (default: {level})", category.as_str());
        for r in rules.iter().filter(|r| r.category == category) {
            let _ = writeln!(out, "  {:width$}  {}", r.name, summary(r.docs));
        }
        let _ = writeln!(out);
    }
    let _ = writeln!(out, "{} rules", rules.len());
}

/// The first sentence of the "What it does" paragraph.
fn summary(docs: &str) -> String {
    let mut paragraph = Vec::new();
    let mut lines = docs.lines().skip_while(|l| l.trim() != "### What it does");
    lines.next();
    for line in lines.skip_while(|l| l.trim().is_empty()) {
        if line.trim().is_empty() || line.starts_with('#') {
            break;
        }
        paragraph.push(line.trim());
    }
    let text = paragraph.join(" ");
    match text.find(". ") {
        Some(i) => text[..=i].to_string(),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_is_the_first_sentence() {
        let docs = "### What it does\n\nReports a thing, such as `a`.\nThen more. And more.\n\n### Why\n\nx";
        assert_eq!(summary(docs), "Reports a thing, such as `a`.");
    }
}
