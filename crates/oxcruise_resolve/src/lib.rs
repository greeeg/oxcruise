//! Resolution and classification of dependencies.
//!
//! Wraps one shared [`oxc_resolver::Resolver`] and turns its answer into what
//! oxcruise needs: a graph target plus depcruise-compatible dependency types.

mod classify;
mod manifest;
mod options;
mod workspace;

pub use classify::{Resolved, Target};
pub use manifest::Manifest;
pub use options::{ResolveConfig, ResolveMode, TsconfigMode};
pub use workspace::{Workspace, WorkspacePackage};

use std::path::{Path, PathBuf};

use oxc_resolver::{ResolveError, Resolver as OxcResolver};
use oxcruise_core::DepKinds;

pub struct Resolver {
    esm: OxcResolver,
    cjs: OxcResolver,
    config: ResolveConfig,
    manifests: manifest::ManifestCache,
    workspace: Workspace,
}

impl Resolver {
    pub fn new(config: ResolveConfig) -> Self {
        let workspace = Workspace::discover(&config.root);
        let esm = OxcResolver::new(options::to_oxc(&config, ResolveMode::Esm));
        let cjs = OxcResolver::new(options::to_oxc(&config, ResolveMode::Cjs));
        Self {
            esm,
            cjs,
            config,
            manifests: manifest::ManifestCache::default(),
            workspace,
        }
    }

    pub fn root(&self) -> &Path {
        &self.config.root
    }

    pub fn workspace(&self) -> &Workspace {
        &self.workspace
    }

    /// Resolve `specifier` as written in `from_file` (an absolute path).
    pub fn resolve(&self, from_file: &Path, specifier: &str, mode: ResolveMode) -> Resolved {
        if classify::is_skipped_specifier(specifier) {
            return Resolved {
                target: Target::Skip,
                kinds: DepKinds::empty(),
            };
        }
        match self.oxc(mode).resolve_file(from_file, specifier) {
            Ok(res) => classify::resolved(self, from_file, specifier, res.path()),
            Err(ResolveError::Builtin { resolved, .. }) => classify::core(&resolved),
            Err(e) => classify::unresolved(&e),
        }
    }

    pub(crate) fn oxc(&self, mode: ResolveMode) -> &OxcResolver {
        match mode {
            ResolveMode::Esm => &self.esm,
            ResolveMode::Cjs => &self.cjs,
        }
    }
    pub(crate) fn config(&self) -> &ResolveConfig {
        &self.config
    }
    pub(crate) fn manifest_for(&self, dir: &Path) -> Option<std::sync::Arc<Manifest>> {
        self.manifests.nearest(dir)
    }

    /// Path relative to the cruise root, with `/` separators. Paths outside the
    /// root keep their `..` segments.
    pub fn relative(&self, path: &Path) -> String {
        relative_to(&self.config.root, path)
    }
}

pub fn relative_to(root: &Path, path: &Path) -> String {
    let rel = match path.strip_prefix(root) {
        Ok(r) => r.to_path_buf(),
        Err(_) => diff_paths(path, root).unwrap_or_else(|| PathBuf::from(path)),
    };
    rel.to_string_lossy().replace('\\', "/")
}

fn diff_paths(path: &Path, base: &Path) -> Option<PathBuf> {
    use std::path::Component;
    let (mut a, mut b) = (path.components(), base.components());
    let mut out: Vec<Component> = vec![];
    loop {
        match (a.next(), b.next()) {
            (None, None) => break,
            (Some(x), None) => {
                out.push(x);
                out.extend(a);
                break;
            }
            (None, Some(_)) => out.push(Component::ParentDir),
            (Some(x), Some(y)) if out.is_empty() && x == y => {}
            (Some(x), Some(_)) => {
                out.push(Component::ParentDir);
                for _ in b {
                    out.push(Component::ParentDir);
                }
                out.push(x);
                out.extend(a);
                break;
            }
        }
    }
    Some(out.iter().map(|c| c.as_os_str()).collect())
}
