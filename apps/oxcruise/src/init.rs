use std::io::Write;
use std::path::Path;

use oxcruise_config::Category;

use crate::config_io;
use crate::result::CliRunResult;

/// Write a starter config with every category at its default level, so it is easy to turn one on.
pub fn run(
    target: Option<&Path>,
    cwd: &Path,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> CliRunResult {
    let target = config_io::write_target(cwd, target);
    let categories: serde_json::Map<String, serde_json::Value> = Category::ALL
        .iter()
        .map(|c| {
            let level = serde_json::to_value(c.default_level()).expect("level serialises");
            (c.as_str().to_string(), level)
        })
        .collect();
    let config = serde_json::json!({ "categories": categories, "forbid": [] });
    match config_io::write_new(&target, config, &[]) {
        Ok(()) => {
            let _ = writeln!(out, "wrote {}", target.display());
            CliRunResult::Success
        }
        Err(e) => {
            let _ = writeln!(err, "error: {e}");
            CliRunResult::InvalidOptions
        }
    }
}
