use std::io::Write;
use std::path::{Path, PathBuf};

use oxcruise_config::{ConfigLoader, JsConfigLoader};

use crate::args::MigrateArgs;
use crate::result::CliRunResult;

const DEPCRUISE_NAMES: &[&str] = &[
    ".dependency-cruiser.json",
    ".dependency-cruiser.js",
    ".dependency-cruiser.cjs",
    ".dependency-cruiser.mjs",
];

pub fn run(
    args: &MigrateArgs,
    cwd: &Path,
    js: &dyn JsConfigLoader,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> CliRunResult {
    let source = match &args.config {
        Some(p) => {
            if p.is_absolute() {
                p.clone()
            } else {
                cwd.join(p)
            }
        }
        None => match DEPCRUISE_NAMES
            .iter()
            .map(|n| cwd.join(n))
            .find(|p| p.is_file())
        {
            Some(p) => p,
            None => {
                let _ = writeln!(
                    err,
                    "error: no .dependency-cruiser.* config found in {}; pass --config",
                    cwd.display()
                );
                return CliRunResult::InvalidOptions;
            }
        },
    };
    let target: PathBuf = match &args.output {
        Some(p) => {
            if p.is_absolute() {
                p.clone()
            } else {
                cwd.join(p)
            }
        }
        None => cwd.join(if args.json {
            ".oxcruiserc.json"
        } else {
            "oxcruise.config.ts"
        }),
    };
    if target.exists() && !args.force {
        let _ = writeln!(
            err,
            "error: {} already exists; use --force to overwrite",
            target.display()
        );
        return CliRunResult::InvalidOptions;
    }

    let loaded = match ConfigLoader::new(js).import_depcruise(&source) {
        Ok(l) => l,
        Err(e) => {
            let _ = writeln!(err, "error: {e}");
            return CliRunResult::InvalidOptions;
        }
    };
    let mut json = loaded.config.to_minimal_json();
    let body = if args.json {
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
        s.push_str(&format!(
            "// Migrated from {} by `oxcruise migrate`.\n",
            source
                .file_name()
                .map_or_else(String::new, |n| n.to_string_lossy().into_owned())
        ));
        for w in &loaded.warnings {
            // the loader prefixes warnings with the file they came from
            let prefix = format!("{}: ", source.display());
            s.push_str(&format!(
                "// TODO: {}\n",
                w.strip_prefix(&prefix).unwrap_or(w)
            ));
        }
        s.push_str("import { defineConfig } from \"oxcruise\";\n\nexport default defineConfig(");
        s.push_str(&serde_json::to_string_pretty(&json).expect("json"));
        s.push_str(");\n");
        s
    };
    if let Err(e) = std::fs::write(&target, body) {
        let _ = writeln!(err, "error: cannot write {}: {e}", target.display());
        return CliRunResult::InvalidOptions;
    }
    for w in &loaded.warnings {
        let _ = writeln!(err, "warning: {w}");
    }
    let _ = writeln!(out, "wrote {}", target.display());
    CliRunResult::Success
}
