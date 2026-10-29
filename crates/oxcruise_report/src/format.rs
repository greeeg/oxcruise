use std::fmt;
use std::str::FromStr;

/// What `-f/--format` selects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable diagnostics with source snippets (default).
    Human,
    /// `default` plus the rule documentation.
    Long,
    /// Native versioned JSON.
    Json,
    /// GitHub Actions workflow annotations.
    Github,
    Dot,
    Mermaid,
    /// One `from -> to` line per dependency.
    Text,
    /// A reporter provided by a JS plugin: `plugin/name`.
    External(String),
}

#[derive(Debug, PartialEq, Eq)]
pub struct ParseFormatError(pub String);

impl fmt::Display for ParseFormatError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "unknown output format `{}`; expected default, long, json, github, dot, mermaid, text or plugin/reporter",
            self.0
        )
    }
}

impl std::error::Error for ParseFormatError {}

impl FromStr for OutputFormat {
    type Err = ParseFormatError;

    fn from_str(s: &str) -> Result<Self, ParseFormatError> {
        Ok(match s {
            "default" => Self::Human,
            "long" => Self::Long,
            "json" => Self::Json,
            "github" => Self::Github,
            "dot" => Self::Dot,
            "mermaid" => Self::Mermaid,
            "text" => Self::Text,
            other if other.contains('/') && !other.starts_with('/') => {
                Self::External(other.to_string())
            }
            other => return Err(ParseFormatError(other.to_string())),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses() {
        assert_eq!("long".parse(), Ok(OutputFormat::Long));
        // the old dependency-cruiser names are gone
        for old in ["err", "err-long", "json-depcruise"] {
            assert!(old.parse::<OutputFormat>().is_err(), "{old}");
        }
        assert_eq!(
            "acme/json-lines".parse(),
            Ok(OutputFormat::External("acme/json-lines".into()))
        );
        assert!("nope".parse::<OutputFormat>().is_err());
        assert!("/abs/path".parse::<OutputFormat>().is_err());
    }
}
