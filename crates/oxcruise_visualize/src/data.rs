use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use oxcruise_core::{DepKinds, ModuleKind, Severity};
use oxcruise_report::{CruiseResult, ExportOut, Summary, ViolationKind, ViolationOut};
use serde::Serialize;

/// Bumped when the viewer cannot read older data.
pub const VIZ_VERSION: u32 = 1;

/// Folder that holds npm packages and Node built-ins, so they do not mix with project folders.
pub const EXTERNAL_DIR: &str = "(external)";

/// `modules.kind`
const KIND_LOCAL: u8 = 0;
const KIND_PACKAGE: u8 = 1;
const KIND_BUILTIN: u8 = 2;

/// `modules.flags`
const MODULE_NOT_FOLLOWED: u8 = 1;
const MODULE_ORPHAN: u8 = 2;
const MODULE_PARSE_ERROR: u8 = 4;

/// `edges.kinds`: how the dependency is written.
const KIND_TYPE_ONLY: u16 = 1;
const KIND_DYNAMIC: u16 = 2;
const KIND_REQUIRE: u16 = 4;
const KIND_EXPORT: u16 = 8;
const KIND_IMPORT: u16 = 16;

/// `edges.flags`
const EDGE_CIRCULAR: u8 = 1;
const EDGE_TYPE_ONLY: u8 = 2;
const EDGE_ERROR: u8 = 4;
const EDGE_WARN: u8 = 8;

/// The columnar payload of the viewer. Every column has one entry per row, so large graphs
/// stay small and decode without building an object per module.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VizData {
    pub version: u32,
    pub root: String,
    /// Milliseconds since the Unix epoch.
    pub generated_at: u64,
    pub summary: Summary,
    /// Whether `exports` was extracted.
    pub symbols: bool,
    pub dirs: Dirs,
    pub modules: Modules,
    pub packages: Vec<String>,
    pub rules: Vec<String>,
    pub edges: Edges,
    pub violations: Vec<Violation>,
    /// Per module, `[name, used, isType]`; empty unless `symbols`.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub exports: Vec<Vec<(String, bool, bool)>>,
}

/// The folder tree. Index 0 is the root, whose name is empty.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Dirs {
    pub parent: Vec<u32>,
    pub name: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Modules {
    pub dir: Vec<u32>,
    pub name: Vec<String>,
    pub kind: Vec<u8>,
    /// Bytes of the file; 0 for externals. The viewer does not show it.
    pub size: Vec<u32>,
    pub flags: Vec<u8>,
    /// Index into `packages`, or -1.
    pub package: Vec<i32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Edges {
    pub from: Vec<u32>,
    pub to: Vec<u32>,
    pub kinds: Vec<u16>,
    pub flags: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Violation {
    /// Index into `rules`.
    pub rule: u32,
    pub severity: Severity,
    pub kind: ViolationKind,
    /// The module, absent for a file that is not a module (`file` has its path).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    pub message: String,
    /// Modules of the cycle or the path, in order.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub cycle: Vec<u32>,
}

impl VizData {
    /// Build the payload, reading file sizes from disk below `result.root`.
    pub fn from_result(result: &CruiseResult) -> VizData {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis() as u64);
        Self::build(result, &|path| file_size(&result.root, path), now)
    }

    /// Build the payload with a chosen way to get sizes and time (the tests make both stable).
    pub fn build(
        result: &CruiseResult,
        size_of: &dyn Fn(&str) -> u32,
        generated_at: u64,
    ) -> VizData {
        let mut dirs = Dirs {
            parent: vec![0],
            name: vec![String::new()],
        };
        let mut dir_index: HashMap<(u32, String), u32> = HashMap::new();
        let mut modules = Modules::default();
        let mut packages: Vec<String> = vec![];
        let mut package_index: HashMap<&str, i32> = HashMap::new();
        let mut index_of: HashMap<&str, u32> = HashMap::with_capacity(result.modules.len());

        for m in &result.modules {
            let external = m.kind != ModuleKind::Local;
            let mut parent = 0u32;
            // `node_modules/react/index.js` is `(external)/react/index.js`
            let shown = match m.path.rfind("node_modules/") {
                Some(i) if external => &m.path[i + "node_modules/".len()..],
                _ => m.path.as_str(),
            };
            let mut parts: Vec<&str> = shown.split('/').filter(|p| !p.is_empty()).collect();
            let name = parts.pop().unwrap_or(&m.path).to_string();
            let folders = external
                .then_some(EXTERNAL_DIR)
                .into_iter()
                .chain(parts.iter().copied());
            for part in folders {
                parent = *dir_index
                    .entry((parent, part.to_string()))
                    .or_insert_with(|| {
                        dirs.parent.push(parent);
                        dirs.name.push(part.to_string());
                        dirs.parent.len() as u32 - 1
                    });
            }
            // `m.package` is the workspace package owning a file; an installed package has none, so
            // its name comes from the path (`react`, `@scope/name`)
            let installed = (m.kind == ModuleKind::Package)
                .then(|| npm_package_name(shown))
                .flatten();
            let package = match m.package.as_deref().or(installed) {
                Some(p) => *package_index.entry(p).or_insert_with(|| {
                    packages.push(p.to_string());
                    packages.len() as i32 - 1
                }),
                None => -1,
            };
            let mut flags = 0;
            if m.not_followed {
                flags |= MODULE_NOT_FOLLOWED;
            }
            if m.orphan {
                flags |= MODULE_ORPHAN;
            }
            if m.parse_error.is_some() {
                flags |= MODULE_PARSE_ERROR;
            }
            index_of.insert(&m.path, modules.dir.len() as u32);
            modules.dir.push(parent);
            modules.name.push(name);
            modules.kind.push(match m.kind {
                ModuleKind::Package => KIND_PACKAGE,
                ModuleKind::Builtin => KIND_BUILTIN,
                _ => KIND_LOCAL,
            });
            modules
                .size
                .push(if external { 0 } else { size_of(&m.path) });
            modules.flags.push(flags);
            modules.package.push(package);
        }

        let mut edges = Edges::default();
        for (from, m) in result.modules.iter().enumerate() {
            for d in &m.dependencies {
                let Some(&to) = index_of.get(d.to.as_str()) else {
                    continue; // unresolved
                };
                let mut flags = 0;
                if d.circular {
                    flags |= EDGE_CIRCULAR;
                }
                if d.kinds
                    .intersects(DepKinds::TYPE_ONLY | DepKinds::TYPE_IMPORT)
                {
                    flags |= EDGE_TYPE_ONLY;
                }
                if d.rules.iter().any(|r| r.severity == Severity::Error) {
                    flags |= EDGE_ERROR;
                } else if !d.rules.is_empty() {
                    flags |= EDGE_WARN;
                }
                edges.from.push(from as u32);
                edges.to.push(to);
                edges.kinds.push(edge_kinds(d.kinds));
                edges.flags.push(flags);
            }
        }

        let mut rules: Vec<String> = vec![];
        let mut rule_index: HashMap<&str, u32> = HashMap::new();
        let violations = result
            .violations
            .iter()
            .map(|v| {
                let rule = *rule_index.entry(v.rule.as_str()).or_insert_with(|| {
                    rules.push(v.rule.clone());
                    rules.len() as u32 - 1
                });
                violation(v, rule, &index_of)
            })
            .collect();

        let exports = if result.modules.iter().any(|m| !m.exports.is_empty()) {
            result
                .modules
                .iter()
                .map(|m| m.exports.iter().map(export).collect())
                .collect()
        } else {
            vec![]
        };

        VizData {
            version: VIZ_VERSION,
            root: result.root.to_string_lossy().into_owned(),
            generated_at,
            summary: result.summary.clone(),
            symbols: !exports.is_empty(),
            dirs,
            modules,
            packages,
            rules,
            edges,
            violations,
            exports,
        }
    }
}

/// `react/index.js` -> `react`, `@scope/name/lib/a.js` -> `@scope/name`; the path is after `node_modules/`.
fn npm_package_name(shown: &str) -> Option<&str> {
    let mut end = shown.find('/')?;
    if shown.starts_with('@') {
        end += 1 + shown[end + 1..].find('/')?;
    }
    Some(&shown[..end])
}

fn file_size(root: &Path, path: &str) -> u32 {
    std::fs::metadata(root.join(path)).map_or(0, |m| m.len().min(u32::MAX as u64) as u32)
}

fn edge_kinds(k: DepKinds) -> u16 {
    let mut out = 0;
    for (flag, bit) in [
        (DepKinds::TYPE_ONLY | DepKinds::TYPE_IMPORT, KIND_TYPE_ONLY),
        (DepKinds::DYNAMIC_IMPORT, KIND_DYNAMIC),
        (DepKinds::REQUIRE, KIND_REQUIRE),
        (DepKinds::EXPORT, KIND_EXPORT),
        (DepKinds::IMPORT, KIND_IMPORT),
    ] {
        if k.intersects(flag) {
            out |= bit;
        }
    }
    out
}

fn export(x: &ExportOut) -> (String, bool, bool) {
    (x.name.clone(), x.used, x.is_type)
}

fn violation(v: &ViolationOut, rule: u32, index_of: &HashMap<&str, u32>) -> Violation {
    let module = |p: &String| index_of.get(p.as_str()).copied();
    let from = module(&v.from);
    Violation {
        rule,
        severity: v.severity,
        kind: v.kind,
        from,
        file: from.is_none().then(|| v.from.clone()),
        to: v.to.as_ref().and_then(module),
        line: v.line,
        message: v.message.clone(),
        cycle: v.cycle.iter().filter_map(|h| module(&h.module)).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::npm_package_name;

    #[test]
    fn package_name_from_installed_path() {
        assert_eq!(npm_package_name("react/index.js"), Some("react"));
        assert_eq!(npm_package_name("react/cjs/react.js"), Some("react"));
        assert_eq!(
            npm_package_name("@scope/name/lib/a.js"),
            Some("@scope/name")
        );
        assert_eq!(npm_package_name("index.js"), None);
        assert_eq!(npm_package_name("@scope/index.js"), None);
    }
}
