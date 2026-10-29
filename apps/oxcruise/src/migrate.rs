use std::io::Write;
use std::path::Path;

use oxcruise_config::{ConfigLoader, JsConfigLoader};

use crate::config_io;
use crate::result::CliRunResult;

const DEPCRUISE_NAMES: &[&str] = &[
    ".dependency-cruiser.json",
    ".dependency-cruiser.js",
    ".dependency-cruiser.cjs",
    ".dependency-cruiser.mjs",
];

/// Convert a dependency-cruiser config. `source` is `--migrate=FILE` (default: discover it in
/// `cwd`), `target` is `-c FILE` (default `oxcruise.config.ts`).
pub fn run(
    source: Option<&Path>,
    target: Option<&Path>,
    cwd: &Path,
    js: &dyn JsConfigLoader,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> CliRunResult {
    let source = match source {
        Some(p) if p.is_absolute() => p.to_path_buf(),
        Some(p) => cwd.join(p),
        None => match DEPCRUISE_NAMES
            .iter()
            .map(|n| cwd.join(n))
            .find(|p| p.is_file())
        {
            Some(p) => p,
            None => {
                let _ = writeln!(
                    err,
                    "error: no .dependency-cruiser.* config found in {}; pass --migrate=FILE",
                    cwd.display()
                );
                return CliRunResult::InvalidOptions;
            }
        },
    };
    let target = config_io::write_target(cwd, target);
    if target.exists() {
        let _ = writeln!(err, "error: {} already exists", target.display());
        return CliRunResult::InvalidOptions;
    }

    let loaded = match ConfigLoader::new(js).import_depcruise(&source) {
        Ok(l) => l,
        Err(e) => {
            let _ = writeln!(err, "error: {e}");
            return CliRunResult::InvalidOptions;
        }
    };
    let mut header = vec![format!(
        "Migrated from {} by `oxcruise --migrate`.",
        source
            .file_name()
            .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
    )];
    // the loader prefixes warnings with the file they came from
    let prefix = format!("{}: ", source.display());
    for w in &loaded.warnings {
        header.push(format!("TODO: {}", w.strip_prefix(&prefix).unwrap_or(w)));
    }
    if let Err(e) = config_io::write_new(&target, loaded.config.to_minimal_json(), &header) {
        let _ = writeln!(err, "error: {e}");
        return CliRunResult::InvalidOptions;
    }
    for w in &loaded.warnings {
        let _ = writeln!(err, "warning: {w}");
    }
    let _ = writeln!(out, "wrote {}", target.display());
    CliRunResult::Success
}
