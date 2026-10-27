//! Built-in rules.

mod dependency_types;
mod internal_modules;
mod layers;
mod no_circular;
mod no_cross_package_relative;
mod no_dev_deps_in_prod;
mod no_orphans;
mod no_test_imports;

pub use dependency_types::*;
pub use internal_modules::{InternalModules, InternalModulesConfig};
pub use layers::{Layers, LayersConfig};
pub use no_circular::{NoCircular, NoCircularConfig};
pub use no_cross_package_relative::NoCrossPackageRelative;
pub use no_dev_deps_in_prod::{NoDevDepsInProd, NoDevDepsInProdConfig};
pub use no_orphans::{NoOrphans, NoOrphansConfig};
pub use no_test_imports::NoTestImports;

/// `foo.test.ts`, `foo.spec.tsx`, ... (any JS/TS extension)
pub(crate) fn file_has_marker(path: &str, marker: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let mut parts = name.rsplitn(3, '.');
    let ext = parts.next().unwrap_or("");
    let mid = parts.next().unwrap_or("");
    mid == marker
        && matches!(
            ext,
            "js" | "mjs" | "cjs" | "jsx" | "ts" | "mts" | "cts" | "tsx"
        )
}

pub(crate) fn in_test_dir(path: &str) -> bool {
    path.split('/')
        .any(|s| matches!(s, "test" | "tests" | "__tests__" | "__mocks__"))
}
