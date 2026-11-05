//! Built-in rules.

mod circular_dependencies;
mod cross_package_relative_dependencies;
mod dependencies_on_tests;
mod dependency_types;
mod dev_dependencies_in_production;
mod folder_structure;
mod layers;
mod orphan_modules;
mod unused_exports;
mod unused_modules;
mod unused_packages;
mod visibility;

pub use circular_dependencies::{CircularDependencies, CircularDependenciesConfig};
pub use cross_package_relative_dependencies::CrossPackageRelativeDependencies;
pub use dependencies_on_tests::DependenciesOnTests;
pub use dependency_types::*;
pub use dev_dependencies_in_production::{
    DevDependenciesInProduction, DevDependenciesInProductionConfig,
};
pub use folder_structure::{FolderStructure, FolderStructureConfig};
pub use layers::{Layers, LayersConfig};
pub use orphan_modules::{OrphanModules, OrphanModulesConfig};
pub use unused_exports::{UnusedExports, UnusedExportsConfig};
pub use unused_modules::{UnusedModules, UnusedModulesConfig};
pub use unused_packages::{UnusedPackages, UnusedPackagesConfig};
pub use visibility::{Visibility, VisibilityConfig};

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

/// Tests: `*.test.*`, `*.spec.*`, test directories, and anything `extra` matches.
pub(crate) fn is_test_module(path: &str, extra: &crate::pattern::PathPatterns) -> bool {
    file_has_marker(path, "test")
        || file_has_marker(path, "spec")
        || in_test_dir(path)
        || extra.is_match(path, &crate::pattern::Captures::default())
}
