use std::path::{Path, PathBuf};
use std::process::Command;

use crate::{OxcruiseConfig, depcruise, preset, strip_jsonc};

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("cannot read {}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid config {}: {message}", path.display())]
    Invalid { path: PathBuf, message: String },
    #[error("loading {} needs Node.js, which was not found on PATH. Use a .json/.jsonc config or install node", path.display())]
    NodeMissing { path: PathBuf },
    #[error("evaluating {} failed:\n{message}", path.display())]
    Js { path: PathBuf, message: String },
    #[error("unknown preset `{0}`")]
    UnknownPreset(String),
    #[error("`extends` cycle through {}", .0.display())]
    ExtendsCycle(PathBuf),
}

/// Evaluates a JS/TS config module and returns its default export as JSON.
///
/// The standalone binary uses [`NodeSubprocessLoader`]; the napi build plugs in
/// a callback into the host Node process instead.
pub trait JsConfigLoader: Sync {
    fn load(&self, path: &Path) -> Result<serde_json::Value, ConfigError>;
}

/// Runs `node` with a tiny embedded loader script.
pub struct NodeSubprocessLoader;

const LOADER_JS: &str = r#"
import { pathToFileURL } from "node:url";
const mod = await import(pathToFileURL(process.argv[1]).href);
let cfg = mod.default ?? mod;
if (typeof cfg === "function") cfg = cfg();
cfg = await cfg;
if (cfg && typeof cfg === "object" && "default" in cfg && Object.keys(cfg).length === 1) cfg = cfg.default;
process.stdout.write(JSON.stringify(cfg ?? {}));
"#;

impl JsConfigLoader for NodeSubprocessLoader {
    fn load(&self, path: &Path) -> Result<serde_json::Value, ConfigError> {
        let mut cmd = Command::new("node");
        cmd.arg("--no-warnings");
        if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("ts" | "mts" | "cts")
        ) {
            cmd.arg("--experimental-strip-types");
        }
        cmd.args(["--input-type=module", "-e", LOADER_JS]).arg(path);
        let out = cmd.output().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                ConfigError::NodeMissing {
                    path: path.to_path_buf(),
                }
            } else {
                ConfigError::Io {
                    path: path.to_path_buf(),
                    source: e,
                }
            }
        })?;
        if !out.status.success() {
            return Err(ConfigError::Js {
                path: path.to_path_buf(),
                message: String::from_utf8_lossy(&out.stderr).trim().to_string(),
            });
        }
        serde_json::from_slice(&out.stdout).map_err(|e| ConfigError::Js {
            path: path.to_path_buf(),
            message: format!("config did not serialise to JSON: {e}"),
        })
    }
}

#[derive(Debug)]
pub struct LoadedConfig {
    pub config: OxcruiseConfig,
    /// The file the config came from, if any.
    pub path: Option<PathBuf>,
    pub warnings: Vec<String>,
    /// Loaded from a `.dependency-cruiser.*` file through the importer.
    pub imported_from_depcruise: bool,
}

const NATIVE_NAMES: &[&str] = &[
    "oxcruise.config.ts",
    "oxcruise.config.mts",
    "oxcruise.config.js",
    "oxcruise.config.mjs",
    ".oxcruiserc.json",
    ".oxcruiserc.jsonc",
];
const DEPCRUISE_NAMES: &[&str] = &[
    ".dependency-cruiser.json",
    ".dependency-cruiser.js",
    ".dependency-cruiser.cjs",
    ".dependency-cruiser.mjs",
];

/// Find a config file in `dir` (not its parents).
pub fn discover(dir: &Path) -> Option<PathBuf> {
    NATIVE_NAMES
        .iter()
        .chain(DEPCRUISE_NAMES)
        .map(|n| dir.join(n))
        .find(|p| p.is_file())
}

pub struct ConfigLoader<'a> {
    js: &'a dyn JsConfigLoader,
}

impl<'a> ConfigLoader<'a> {
    pub fn new(js: &'a dyn JsConfigLoader) -> Self {
        Self { js }
    }

    pub fn load(&self, path: &Path) -> Result<LoadedConfig, ConfigError> {
        let mut warnings = vec![];
        let mut stack = vec![];
        let (config, imported) = self.load_rec(path, &mut stack, &mut warnings)?;
        Ok(LoadedConfig {
            config,
            path: Some(path.to_path_buf()),
            warnings,
            imported_from_depcruise: imported,
        })
    }

    fn read_value(&self, path: &Path) -> Result<serde_json::Value, ConfigError> {
        match path.extension().and_then(|e| e.to_str()) {
            Some("json" | "jsonc") | None => {
                let text = std::fs::read_to_string(path).map_err(|e| ConfigError::Io {
                    path: path.to_path_buf(),
                    source: e,
                })?;
                serde_json::from_str(&strip_jsonc(&text)).map_err(|e| ConfigError::Invalid {
                    path: path.to_path_buf(),
                    message: e.to_string(),
                })
            }
            _ => self.js.load(path),
        }
    }

    fn load_rec(
        &self,
        path: &Path,
        stack: &mut Vec<PathBuf>,
        warnings: &mut Vec<String>,
    ) -> Result<(OxcruiseConfig, bool), ConfigError> {
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if stack.contains(&canonical) {
            return Err(ConfigError::ExtendsCycle(canonical));
        }
        stack.push(canonical);

        let value = self.read_value(path)?;
        let is_depcruise = depcruise::looks_like_depcruise(path, &value);
        let own: OxcruiseConfig = if is_depcruise {
            let (cfg, w) = depcruise::import(&value);
            warnings.extend(w.into_iter().map(|w| format!("{}: {w}", path.display())));
            cfg
        } else {
            serde_json::from_value(value).map_err(|e| ConfigError::Invalid {
                path: path.to_path_buf(),
                message: e.to_string(),
            })?
        };

        let dir = path.parent().unwrap_or(Path::new("."));
        let mut merged = OxcruiseConfig::default();
        for ext in &own.extends {
            let base = self.load_extends(ext, dir, stack, warnings)?;
            merged = merged.layered(&base);
        }
        let result = merged.layered(&own);
        stack.pop();
        Ok((result, is_depcruise))
    }

    fn load_extends(
        &self,
        spec: &str,
        dir: &Path,
        stack: &mut Vec<PathBuf>,
        warnings: &mut Vec<String>,
    ) -> Result<OxcruiseConfig, ConfigError> {
        if spec.starts_with("oxcruise:") {
            let p = preset(spec).ok_or_else(|| ConfigError::UnknownPreset(spec.to_string()))?;
            // presets may extend other presets
            let mut merged = OxcruiseConfig::default();
            for e in &p.extends {
                merged = merged.layered(&self.load_extends(e, dir, stack, warnings)?);
            }
            return Ok(merged.layered(&p));
        }
        let path = dir.join(spec);
        Ok(self.load_rec(&path, stack, warnings)?.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct NoJs;
    impl JsConfigLoader for NoJs {
        fn load(&self, path: &Path) -> Result<serde_json::Value, ConfigError> {
            panic!("unexpected js load of {}", path.display())
        }
    }

    #[test]
    fn json_with_extends_and_preset() {
        let d = tempfile::tempdir().unwrap();
        std::fs::write(
            d.path().join("base.json"),
            r#"{ "entries": ["src"], "rules": { "no-circular": "warn" } }"#,
        )
        .unwrap();
        std::fs::write(
            d.path().join(".oxcruiserc.jsonc"),
            r#"{
              // comment
              "extends": ["oxcruise:recommended", "./base.json"],
              "rules": { "no-orphans": "off", },
            }"#,
        )
        .unwrap();
        let found = discover(d.path()).unwrap();
        assert!(found.ends_with(".oxcruiserc.jsonc"));
        let loaded = ConfigLoader::new(&NoJs).load(&found).unwrap();
        let c = loaded.config;
        assert_eq!(c.entries, ["src"]);
        // base.json overrides the preset's no-circular; own config turns orphans off
        assert_eq!(
            c.rules["no-circular"].parts().unwrap().0,
            crate::Level::Warn
        );
        assert_eq!(c.rules["no-orphans"].parts().unwrap().0, crate::Level::Off);
        assert!(c.rules.contains_key("no-unresolvable"));
    }

    #[test]
    fn errors_are_specific() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("c.json");
        std::fs::write(&p, r#"{ "entires": [] }"#).unwrap();
        let err = ConfigLoader::new(&NoJs).load(&p).unwrap_err().to_string();
        assert!(
            err.contains("entires") && err.contains("unknown field"),
            "{err}"
        );

        std::fs::write(&p, r#"{ "extends": ["oxcruise:nope"] }"#).unwrap();
        assert!(matches!(
            ConfigLoader::new(&NoJs).load(&p),
            Err(ConfigError::UnknownPreset(_))
        ));

        std::fs::write(&p, r#"{ "extends": ["./c.json"] }"#).unwrap();
        assert!(matches!(
            ConfigLoader::new(&NoJs).load(&p),
            Err(ConfigError::ExtendsCycle(_))
        ));
    }

    #[test]
    fn node_subprocess_loads_ts_and_cjs() {
        if Command::new("node").arg("--version").output().is_err() {
            eprintln!("node not installed; skipping");
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let ts = d.path().join("oxcruise.config.ts");
        std::fs::write(
            &ts,
            "const entries: string[] = ['src'];\nexport default { entries, rules: { 'no-circular': 'error' as const } };\n",
        )
        .unwrap();
        let cjs = d.path().join("x.config.cjs");
        std::fs::write(&cjs, "module.exports = { entries: ['lib'] };\n").unwrap();
        let l = ConfigLoader::new(&NodeSubprocessLoader);
        assert_eq!(l.load(&ts).unwrap().config.entries, ["src"]);
        assert_eq!(l.load(&cjs).unwrap().config.entries, ["lib"]);

        let bad = d.path().join("bad.config.mjs");
        std::fs::write(&bad, "throw new Error('boom');\n").unwrap();
        let err = l.load(&bad).unwrap_err().to_string();
        assert!(err.contains("boom"), "{err}");
    }
}
