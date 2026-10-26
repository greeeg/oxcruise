//! The oxcruise configuration model and everything needed to get one from disk.

mod config;
pub mod depcruise;
mod jsonc;
mod loader;
mod merge;
mod presets;

pub use config::*;
pub use jsonc::strip_jsonc;
pub use loader::{
    ConfigError, ConfigLoader, JsConfigLoader, LoadedConfig, NodeSubprocessLoader, discover,
};
pub use presets::preset;
