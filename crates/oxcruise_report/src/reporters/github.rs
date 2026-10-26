use std::io::{self, Write};

use oxcruise_core::Severity;

use crate::CruiseResult;

fn data(s: &str) -> String {
    s.replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A")
}

fn property(s: &str) -> String {
    data(s).replace(':', "%3A").replace(',', "%2C")
}

/// GitHub Actions workflow commands: `::error file=..,line=..,col=..,title=..::message`.
pub fn render(result: &CruiseResult, out: &mut dyn Write) -> io::Result<()> {
    for v in &result.violations {
        let level = match v.severity {
            Severity::Error => "error",
            Severity::Warn => "warning",
            _ => "notice",
        };
        let mut props = format!("file={}", property(&v.from));
        if let Some(l) = v.line {
            props.push_str(&format!(",line={l}"));
        }
        if let Some(c) = v.col {
            props.push_str(&format!(",col={c}"));
        }
        props.push_str(&format!(",title={}", property(&v.rule)));
        writeln!(out, "::{level} {props}::{}", data(&v.message))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes() {
        assert_eq!(data("a%b\nc"), "a%25b%0Ac");
        assert_eq!(property("a:b,c"), "a%3Ab%2Cc");
    }
}
