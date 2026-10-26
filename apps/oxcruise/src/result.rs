use std::process::{ExitCode, Termination};

/// How a run ended, which decides the process exit code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliRunResult {
    /// Nothing to report as failure (also `--rules`, `fmt`, `migrate`).
    Success,
    /// Error-level violations were found.
    FoundErrors { errors: usize, compat: bool },
    /// `--deny-warnings` and there are warnings.
    NoWarningsAllowed { warnings: usize },
    /// `--max-warnings N` exceeded.
    MaxWarningsExceeded { warnings: usize, max: u32 },
    /// Bad flags, unreadable config, invalid rules...
    InvalidOptions,
}

impl Termination for CliRunResult {
    fn report(self) -> ExitCode {
        ExitCode::from(self.code())
    }
}

impl CliRunResult {
    pub fn code(&self) -> u8 {
        match self {
            Self::Success => 0,
            Self::FoundErrors {
                errors,
                compat: true,
            } => (*errors).clamp(1, 255) as u8,
            Self::FoundErrors { .. }
            | Self::NoWarningsAllowed { .. }
            | Self::MaxWarningsExceeded { .. } => 1,
            Self::InvalidOptions => 2,
        }
    }
}
