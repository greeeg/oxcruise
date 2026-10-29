//! Locating, loading and writing the config for a run.

use std::path::Path;

use oxcruise_config::{ConfigLoader, JsConfigLoader, OxcruiseConfig, discover};

pub struct Loaded {
    pub config: OxcruiseConfig,
    pub warnings: Vec<String>,
}

/// "No file found" gives the defaults.
pub fn load(
    cwd: &Path,
    explicit: Option<&Path>,
    js: &dyn JsConfigLoader,
) -> Result<Loaded, String> {
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
            None => {
                return Ok(Loaded {
                    config: OxcruiseConfig::default(),
                    warnings: vec![],
                });
            }
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

/// Write `json` as a new config file, never over an existing one. A `.json` target gets a
/// `$schema`; anything else gets `defineConfig(...)` preceded by one `//` line per header line.
pub fn write_new(target: &Path, json: serde_json::Value, header: &[String]) -> Result<(), String> {
    if target.exists() {
        return Err(format!("{} already exists", target.display()));
    }
    let is_json = target.extension().is_some_and(|e| e == "json");
    let body = if is_json {
        let mut json = json;
        if let Some(o) = json.as_object_mut() {
            let mut with_schema = serde_json::Map::new();
            with_schema.insert(
                "$schema".into(),
                "./node_modules/oxcruise/configuration_schema.json".into(),
            );
            with_schema.extend(std::mem::take(o));
            json = with_schema.into();
        }
        serde_json::to_string_pretty(&json).expect("json") + "\n"
    } else {
        let mut s = String::new();
        for line in header {
            s.push_str(&format!("// {line}\n"));
        }
        s.push_str("import { defineConfig } from \"oxcruise\";\n\nexport default defineConfig(");
        s.push_str(&serde_json::to_string_pretty(&json).expect("json"));
        s.push_str(");\n");
        s
    };
    std::fs::write(target, body).map_err(|e| format!("cannot write {}: {e}", target.display()))
}

/// `-c FILE` (relative to `cwd`), or `oxcruise.config.ts`: where `--init` and `--migrate` write.
pub fn write_target(cwd: &Path, config: Option<&Path>) -> std::path::PathBuf {
    match config {
        Some(p) if p.is_absolute() => p.to_path_buf(),
        Some(p) => cwd.join(p),
        None => cwd.join("oxcruise.config.ts"),
    }
}
