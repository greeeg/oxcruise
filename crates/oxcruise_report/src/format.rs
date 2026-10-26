use std::fmt;
use std::str::FromStr;

/// What `-f/--format` selects.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OutputFormat {
    /// Human-readable diagnostics with source snippets (default).
    Human,
    /// `err` plus the rule documentation.
    ErrLong,
    /// Native versioned JSON.
    Json,
    /// dependency-cruiser's cruise-result JSON.
    JsonDepcruise,
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
            "unknown output format `{}`; expected err, err-long, json, json-depcruise, github, dot, mermaid, text or plugin/reporter",
            self.0
        )
    }
}

impl std::error::Error for ParseFormatError {}

impl FromStr for OutputFormat {
    type Err = ParseFormatError;

    fn from_str(s: &str) -> Result<Self, ParseFormatError> {
        Ok(match s {
            "err" => Self::Human,
            "err-long" => Self::ErrLong,
            "json" => Self::Json,
            "json-depcruise" => Self::JsonDepcruise,
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
        assert_eq!("err-long".parse(), Ok(OutputFormat::ErrLong));
        assert_eq!(
            "acme/json-lines".parse(),
            Ok(OutputFormat::External("acme/json-lines".into()))
        );
        assert!("nope".parse::<OutputFormat>().is_err());
        assert!("/abs/path".parse::<OutputFormat>().is_err());
    }
}
