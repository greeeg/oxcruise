use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use oxcruise_core::DepKinds;
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
