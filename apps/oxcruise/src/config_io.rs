//! Locating and loading the config for a run.

use std::path::Path;

use oxcruise_config::{ConfigLoader, JsConfigLoader, OxcruiseConfig, discover};

pub struct Loaded {
    pub config: OxcruiseConfig,
    pub warnings: Vec<String>,
}

/// `--no-config` and "no file found" both give the defaults.
pub fn load(
    cwd: &Path,
    explicit: Option<&Path>,
    no_config: bool,
    js: &dyn JsConfigLoader,
) -> Result<Loaded, String> {
    let default = || Loaded {
        config: OxcruiseConfig::default(),
        warnings: vec![],
    };
    if no_config {
        return Ok(default());
    }
    let path = match explicit {
        Some(p) => {
            let p = if p.is_absolute() {
                p.to_path_buf()
            } else {
                cwd.join(p)
            };
            if !p.is_file() {
                return Err(format!("config file {} does not exist", p.display()));
            }
            p
        }
        None => match discover(cwd) {
            Some(p) => p,
            None => return Ok(default()),
        },
    };
    let loaded = ConfigLoader::new(js)
        .load(&path)
        .map_err(|e| e.to_string())?;
    Ok(Loaded {
        config: loaded.config,
        warnings: loaded.warnings,
    })
}
