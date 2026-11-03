//! The oxcruise configuration model and everything needed to get one from disk.

mod config;
pub mod depcruise;
mod loader;
mod merge;
mod regex_glob;

pub use config::*;
pub use loader::{
    ConfigError, ConfigLoader, JsConfigLoader, LoadedConfig, NodeSubprocessLoader, discover,
};
pub use oxcruise_core::strip_jsonc;
pub use regex_glob::{placeholders, regex_to_globs};
