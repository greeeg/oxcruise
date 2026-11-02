use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use oxcruise_core::{DeclaredDep, DeclaredManifest, DepKinds, DepSection, LineIndex};
use rustc_hash::FxHashMap;
use serde::Deserialize;

/// The slice of `package.json` that classification needs.
#[derive(Debug, Default, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    #[serde(skip)]
    pub dir: PathBuf,
    pub name: Option<String>,
    #[serde(default)]
    pub dependencies: FxHashMap<String, serde_json::Value>,
    #[serde(default)]
    pub dev_dependencies: FxHashMap<String, serde_json::Value>,
    #[serde(default)]
    pub peer_dependencies: FxHashMap<String, serde_json::Value>,
    #[serde(default)]
    pub optional_dependencies: FxHashMap<String, serde_json::Value>,
    #[serde(default, alias = "bundleDependencies")]
    pub bundled_dependencies: Option<serde_json::Value>,
    #[serde(default)]
    pub deprecated: Option<serde_json::Value>,
    pub workspaces: Option<serde_json::Value>,
}

impl Manifest {
    pub fn read(dir: &Path) -> Option<Manifest> {
        let text = std::fs::read_to_string(dir.join("package.json")).ok()?;
        let mut m: Manifest = serde_json::from_str(&text).ok()?;
        m.dir = dir.to_path_buf();
        Some(m)
    }

    fn bundled(&self, name: &str) -> bool {
        match &self.bundled_dependencies {
            Some(serde_json::Value::Array(a)) => a.iter().any(|v| v.as_str() == Some(name)),
            _ => false,
        }
    }

    /// What this manifest declares, with the position of each entry in the file.
    /// `peers` are read from the installed packages in `node_modules` (nearest ancestor first).
    pub fn declared(&self, root: &Path) -> DeclaredManifest {
        let path = self.dir.join("package.json");
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let lines = LineIndex::new(&text);
        let mut deps: Vec<DeclaredDep> = scan_dependency_members(&text)
            .into_iter()
            .map(|(section, name, start, end)| DeclaredDep {
                peers: installed_peers(&self.dir, &name),
                loc: lines.loc(start as u32),
                range: (start as u32, end as u32),
                section,
                name,
            })
            .collect();
        deps.sort_by_key(|d| d.range.0);
        let mut declared: Vec<String> = deps.iter().map(|d| d.name.clone()).collect();
        declared.sort();
        declared.dedup();
        let (out_dir, root_dir) = tsconfig_dirs(&self.dir);
        DeclaredManifest {
            rel_path: crate::relative_to(root, &path),
            rel_dir: crate::relative_to(root, &self.dir),
            name: self.name.clone(),
            deps,
            declared,
            entry_files: entry_files(&text),
            out_dir,
            root_dir,
        }
    }

    /// The `npm*` kinds that `name` is declared under. Empty if undeclared.
    /// Declared under several sections yields several kinds.
    pub fn sections_for(&self, name: &str) -> DepKinds {
        let mut k = DepKinds::empty();
        if self.dependencies.contains_key(name) {
            k |= DepKinds::NPM;
        }
        if self.dev_dependencies.contains_key(name) {
            k |= DepKinds::NPM_DEV;
        }
        if self.peer_dependencies.contains_key(name) {
            k |= DepKinds::NPM_PEER;
        }
        if self.optional_dependencies.contains_key(name) {
            k |= DepKinds::NPM_OPTIONAL;
        }
        if self.bundled(name) {
            k |= DepKinds::NPM_BUNDLED;
        }
        k
    }
}

/// `package.json` files by directory, shared by all worker threads.
#[derive(Default)]
pub(crate) struct ManifestCache {
    by_dir: Mutex<FxHashMap<PathBuf, Option<Arc<Manifest>>>>,
}

impl ManifestCache {
    /// Manifest in `dir` itself (cached).
    pub fn at(&self, dir: &Path) -> Option<Arc<Manifest>> {
        if let Some(hit) = self.by_dir.lock().expect("poisoned").get(dir) {
            return hit.clone();
        }
        let loaded = Manifest::read(dir).map(Arc::new);
        self.by_dir
            .lock()
            .expect("poisoned")
            .insert(dir.to_path_buf(), loaded.clone());
        loaded
    }

    /// Nearest manifest in `dir` or an ancestor.
    pub fn nearest(&self, dir: &Path) -> Option<Arc<Manifest>> {
        dir.ancestors().find_map(|d| self.at(d))
    }
}

/// The files a package.json publishes: `main`, `module`, `types`, `bin`, `exports`, ...
fn entry_files(text: &str) -> Vec<String> {
    fn collect(v: &serde_json::Value, out: &mut Vec<String>) {
        match v {
            serde_json::Value::String(s) => out.push(s.clone()),
            serde_json::Value::Array(a) => a.iter().for_each(|v| collect(v, out)),
            serde_json::Value::Object(o) => o.values().for_each(|v| collect(v, out)),
            _ => {}
        }
    }
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(text) else {
        return vec![];
    };
    let mut raw = vec![];
    for key in [
        "main", "module", "types", "typings", "source", "bin", "exports",
    ] {
        if let Some(v) = doc.get(key) {
            collect(v, &mut raw);
        }
    }
    let mut out: Vec<String> = raw
        .into_iter()
        .filter(|s| !s.is_empty() && !s.contains("://") && !s.starts_with("../"))
        .map(|s| s.trim_start_matches("./").to_string())
        .collect();
    out.sort();
    out.dedup();
    out
}

/// `compilerOptions.outDir` and `rootDir` of the tsconfig.json in `dir`, as plain relative paths.
fn tsconfig_dirs(dir: &Path) -> (Option<String>, Option<String>) {
    let Ok(text) = std::fs::read_to_string(dir.join("tsconfig.json")) else {
        return (None, None);
    };
    let Ok(doc) = serde_json::from_str::<serde_json::Value>(&oxcruise_config::strip_jsonc(&text))
    else {
        return (None, None);
    };
    let get = |key: &str| {
        let v = doc.get("compilerOptions")?.get(key)?.as_str()?;
        let v = v.trim_start_matches("./").trim_end_matches('/');
        (v != "." && !v.starts_with("..")).then(|| v.to_string())
    };
    (get("outDir"), get("rootDir"))
}

/// Peer dependencies of the installed package `name`, found from `dir` upwards.
fn installed_peers(dir: &Path, name: &str) -> Vec<String> {
    #[derive(Deserialize, Default)]
    #[serde(rename_all = "camelCase")]
    struct Peers {
        #[serde(default)]
        peer_dependencies: FxHashMap<String, serde_json::Value>,
    }
    let Some(text) = dir.ancestors().find_map(|d| {
        std::fs::read_to_string(d.join("node_modules").join(name).join("package.json")).ok()
    }) else {
        return vec![];
    };
    let mut peers: Vec<String> = serde_json::from_str::<Peers>(&text)
        .unwrap_or_default()
        .peer_dependencies
        .into_keys()
        .collect();
    peers.sort();
    peers
}

/// `(section, package name, start, end)` of every `"name": "range"` member of the dependency
/// sections of a package.json, as byte offsets into `text`. Found by scanning the JSON text,
/// because a parsed document has no positions.
pub fn scan_dependency_members(text: &str) -> Vec<(DepSection, String, usize, usize)> {
    let b = text.as_bytes();
    let mut out = vec![];
    let mut i = skip_ws(b, 0);
    if b.get(i) != Some(&b'{') {
        return out;
    }
    i += 1;
    loop {
        i = skip_ws(b, i);
        if b.get(i) != Some(&b'"') {
            return out;
        }
        let Some((key, after_key)) = read_string(text, i) else {
            return out;
        };
        i = skip_ws(b, after_key);
        if b.get(i) != Some(&b':') {
            return out;
        }
        i = skip_ws(b, i + 1);
        let section = DepSection::ALL.into_iter().find(|s| s.as_str() == key);
        if let (Some(section), Some(b'{')) = (section, b.get(i)) {
            i += 1;
            loop {
                i = skip_ws(b, i);
                if b.get(i) != Some(&b'"') {
                    break;
                }
                let start = i;
                let Some((name, after)) = read_string(text, i) else {
                    return out;
                };
                i = skip_ws(b, after);
                if b.get(i) != Some(&b':') {
                    return out;
                }
                i = skip_value(b, skip_ws(b, i + 1));
                out.push((section, name, start, i));
                i = skip_ws(b, i);
                if b.get(i) == Some(&b',') {
                    i += 1;
                }
            }
            if b.get(i) == Some(&b'}') {
                i += 1;
            }
        } else {
            i = skip_value(b, i);
        }
        i = skip_ws(b, i);
        match b.get(i) {
            Some(b',') => i += 1,
            _ => return out,
        }
    }
}

fn skip_ws(b: &[u8], mut i: usize) -> usize {
    while b.get(i).is_some_and(u8::is_ascii_whitespace) {
        i += 1;
    }
    i
}

/// The string starting at the quote `i`: its unescaped value and the offset after the closing quote.
fn read_string(text: &str, i: usize) -> Option<(String, usize)> {
    let b = text.as_bytes();
    let mut j = i + 1;
    while j < b.len() {
        match b[j] {
            b'\\' => j += 2,
            b'"' => {
                let raw = &text[i..=j];
                let value = serde_json::from_str::<String>(raw).ok()?;
                return Some((value, j + 1));
            }
            _ => j += 1,
        }
    }
    None
}

/// Offset after the JSON value starting at `i`.
fn skip_value(b: &[u8], mut i: usize) -> usize {
    match b.get(i) {
        Some(b'"') => {
            i += 1;
            while i < b.len() {
                match b[i] {
                    b'\\' => i += 2,
                    b'"' => return i + 1,
                    _ => i += 1,
                }
            }
            b.len()
        }
        Some(b'{' | b'[') => {
            let mut depth = 0usize;
            while i < b.len() {
                match b[i] {
                    b'{' | b'[' => depth += 1,
                    b'}' | b']' => {
                        depth -= 1;
                        if depth == 0 {
                            return i + 1;
                        }
                    }
                    b'"' => {
                        i = skip_value(b, i);
                        continue;
                    }
                    _ => {}
                }
                i += 1;
            }
            b.len()
        }
        _ => {
            while i < b.len() && !matches!(b[i], b',' | b'}' | b']') {
                i += 1;
            }
            i
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_dependency_members_with_positions() {
        let text = r#"{
  "name": "x",
  "scripts": { "dependencies": "not a section" },
  "dependencies": {
    "a": "^1",
    "@s/b": "workspace:*"
  },
  "devDependencies": { "c": "1", "d": {"odd": [1, "}"]} },
  "keywords": ["dependencies"]
}"#;
        let found = scan_dependency_members(text);
        let names: Vec<_> = found
            .iter()
            .map(|(s, n, _, _)| (s.as_str(), n.as_str()))
            .collect();
        assert_eq!(
            names,
            [
                ("dependencies", "a"),
                ("dependencies", "@s/b"),
                ("devDependencies", "c"),
                ("devDependencies", "d")
            ]
        );
        let (_, _, start, end) = &found[1];
        assert_eq!(&text[*start..*end], r#""@s/b": "workspace:*""#);
        let (_, _, start, end) = &found[3];
        assert_eq!(&text[*start..*end], r#""d": {"odd": [1, "}"]}"#);
    }

    #[test]
    fn tolerates_broken_json() {
        assert!(scan_dependency_members("").is_empty());
        assert!(scan_dependency_members("[]").is_empty());
        let _ = scan_dependency_members(r#"{"dependencies": {"a": "#);
    }
}
