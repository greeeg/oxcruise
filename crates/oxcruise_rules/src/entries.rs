//! Entry points: the modules whose exports are public and from which "used" is decided.
//!
//! One [`EntrySet`] is computed per run, only when a rule asks for it, so the symbol rules
//! agree on it. A rule's own `entries` option adds to it for that rule alone.
//!
//! Declared entries come from the files each `package.json` publishes (`main`, `exports`, `bin`,
//! ..., mapped from the build output back to source), from the conventions of the tools the
//! package uses, from plugins, and from modules marked `oxcruise-ignore-file`.

use oxcruise_core::{DeclaredManifest, Graph, ModuleId, ModuleKind};
use oxcruise_graph::analyses::BitSet;

use crate::conventions::{self, ConventionFilter};
use crate::meta::RuleMeta;
use crate::pattern::{Captures, PathPatterns, Role};
use crate::rules::{UnusedExports, UnusedModules};

#[derive(Debug, Clone)]
pub struct EntrySet {
    /// Source modules that no other module imports: nothing inside the project uses their
    /// exports, so they are the project's outermost layer.
    pub roots: BitSet,
    /// Entries that were declared: by a manifest, a convention, a plugin.
    pub declared: BitSet,
}

impl EntrySet {
    /// Roots and declared entries.
    pub fn all(&self) -> BitSet {
        let mut all = self.roots.clone();
        for m in self.declared.iter() {
            all.insert(m);
        }
        all
    }
}

pub struct EntryInputs<'a> {
    pub manifests: &'a [DeclaredManifest],
    /// One filter per rule that asked for entries; a convention applies when any allows it.
    pub conventions: &'a [ConventionFilter],
    /// Globs from JS plugins, relative to the cruise root.
    pub plugin_entries: &'a [String],
}

pub fn compute(graph: &Graph, inputs: &EntryInputs<'_>) -> EntrySet {
    let n = graph.module_count();
    let mut roots = BitSet::new(n);
    let mut declared = BitSet::new(n);
    let none = Captures::default();
    let plugins = PathPatterns::new(inputs.plugin_entries, Role::Referencing).ok();

    for m in graph.module_ids() {
        let module = graph.module(m);
        if module.kind != ModuleKind::Local {
            continue;
        }
        if graph
            .in_edge_ids(m)
            .iter()
            .all(|&e| graph.edge(e).from == m)
        {
            roots.insert(m.idx());
        }
        // `oxcruise-ignore-file` on a module keeps it public
        let kept = module
            .suppressed
            .iter()
            .any(|r| matches!(r.as_str(), "*" | UnusedModules::NAME | UnusedExports::NAME));
        if kept
            || plugins
                .as_ref()
                .is_some_and(|p| p.is_match(&module.path, &none))
        {
            declared.insert(m.idx());
        }
    }

    // source modules by the manifest that governs them
    let mut owned_by: Vec<Vec<ModuleId>> = vec![vec![]; inputs.manifests.len()];
    for m in graph.module_ids() {
        let module = graph.module(m);
        if module.kind == ModuleKind::Local
            && let Some(i) = module.manifest
        {
            owned_by[i as usize].push(m);
        }
    }

    for (i, manifest) in inputs.manifests.iter().enumerate() {
        let owned = &owned_by[i];
        // the files the package publishes
        for entry in &manifest.entry_files {
            for path in source_candidates(manifest, entry) {
                if let Some(m) = graph.find(&path)
                    && graph.module(m).kind == ModuleKind::Local
                {
                    declared.insert(m.idx());
                    break;
                }
            }
        }
        // wildcard `exports`: every module that one of the patterns can name
        let wildcards: Vec<String> = manifest
            .entry_files
            .iter()
            .filter(|e| e.contains('*'))
            .flat_map(|e| source_candidates(manifest, e))
            .collect();
        if !wildcards.is_empty()
            && let Ok(p) = PathPatterns::new(&wildcards, Role::Referencing)
        {
            for &m in owned {
                if p.is_match(&graph.module(m).path, &none) {
                    declared.insert(m.idx());
                }
            }
        }
        // what the tools of the package load, and files named like programs
        let active: Vec<_> = conventions::active(manifest, inputs.conventions).collect();
        let prefix = if manifest.rel_dir.is_empty() {
            String::new()
        } else {
            format!("{}/", manifest.rel_dir)
        };
        let program = format!("{prefix}{{src/,}}{{index,main,cli}}.*");
        if let Ok(p) = conventions::entry_patterns(manifest, &active)
            && !p.is_empty()
        {
            for &m in owned {
                let module = graph.module(m);
                if module.kind == ModuleKind::Local && p.is_match(&module.path, &none) {
                    declared.insert(m.idx());
                }
            }
        }
        if let Ok(p) = PathPatterns::new(&[program], Role::Referencing) {
            for &m in owned {
                let module = graph.module(m);
                if module.kind == ModuleKind::Local && p.is_match(&module.path, &none) {
                    declared.insert(m.idx());
                }
            }
        }
    }
    EntrySet { roots, declared }
}

/// Build output directories, tried when the tsconfig does not say where the build goes.
const OUT_DIRS: &[&str] = &["dist", "build", "lib", "out", "esm", "cjs", "types"];

/// Where the source of the published file `entry` may be, as paths relative to the cruise root,
/// most likely first. `dist/index.js` maps to `src/index.ts`, `index.tsx`, ...
fn source_candidates(manifest: &DeclaredManifest, entry: &str) -> Vec<String> {
    let mut bases: Vec<String> = vec![entry.to_string()];
    let mut under = |dir: &str, rest: &str| {
        bases.push(if dir.is_empty() {
            rest.to_string()
        } else {
            format!("{dir}/{rest}")
        });
    };
    if let Some(out) = &manifest.out_dir
        && let Some(rest) = entry.strip_prefix(&format!("{out}/"))
    {
        under(manifest.root_dir.as_deref().unwrap_or(""), rest);
    }
    if let Some((first, rest)) = entry.split_once('/')
        && OUT_DIRS.contains(&first)
    {
        under("src", rest);
        under("", rest);
    }
    let prefix = if manifest.rel_dir.is_empty() {
        String::new()
    } else {
        format!("{}/", manifest.rel_dir)
    };
    let mut out: Vec<String> = vec![];
    for base in bases {
        for candidate in with_source_extensions(&base) {
            let full = format!("{prefix}{candidate}");
            if !out.contains(&full) {
                out.push(full);
            }
        }
    }
    out
}

/// `a/b.js` -> `a/b.js`, `a/b.ts`, `a/b.tsx`, `a/b.jsx`; `a/b.d.ts` -> `a/b.ts`, `a/b.tsx`.
fn with_source_extensions(path: &str) -> Vec<String> {
    const MAP: &[(&str, &[&str])] = &[
        (".d.mts", &[".mts"]),
        (".d.cts", &[".cts"]),
        (".d.ts", &[".ts", ".tsx"]),
        (".mjs", &[".mts"]),
        (".cjs", &[".cts"]),
        (".js", &[".ts", ".tsx", ".jsx"]),
    ];
    let mut out = vec![path.to_string()];
    if let Some((suffix, to)) = MAP.iter().find(|(s, _)| path.ends_with(s)) {
        let stem = &path[..path.len() - suffix.len()];
        out.extend(to.iter().map(|ext| format!("{stem}{ext}")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(entries: &[&str], dirs: (Option<&str>, Option<&str>)) -> DeclaredManifest {
        DeclaredManifest {
            rel_path: "package.json".into(),
            rel_dir: String::new(),
            name: None,
            deps: vec![],
            declared: vec![],
            entry_files: entries.iter().map(|s| s.to_string()).collect(),
            out_dir: dirs.0.map(Into::into),
            root_dir: dirs.1.map(Into::into),
        }
    }

    #[test]
    fn build_output_maps_back_to_source() {
        let m = manifest(&[], (Some("build"), Some("lib")));
        let c = source_candidates(&m, "build/cli/main.js");
        assert_eq!(c[0], "build/cli/main.js");
        assert!(c.contains(&"lib/cli/main.ts".to_string()), "{c:?}");

        let m = manifest(&[], (None, None));
        let c = source_candidates(&m, "dist/index.d.mts");
        assert!(c.contains(&"src/index.mts".to_string()), "{c:?}");
        assert!(c.contains(&"index.mts".to_string()), "{c:?}");
        let c = source_candidates(&m, "src/index.ts");
        assert_eq!(c, ["src/index.ts"]);
    }

    #[test]
    fn manifests_in_subdirectories_prefix_their_candidates() {
        let mut m = manifest(&[], (None, None));
        m.rel_dir = "packages/a".into();
        let c = source_candidates(&m, "dist/index.js");
        assert!(c.contains(&"packages/a/src/index.ts".to_string()), "{c:?}");
    }
}
