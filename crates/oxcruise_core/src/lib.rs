//! Data model shared by every oxcruise crate. No I/O lives here.

mod file_tree;
mod graph;
mod jsonc;
mod kinds;
mod loc;
mod symbols;
mod violation;

pub use file_tree::{FileTree, FolderId, base_name, parent_path};
pub use graph::{Edge, EdgeId, Graph, Module, ModuleId, ModuleKind};
pub use jsonc::strip_jsonc;
pub use kinds::{DepKinds, ModuleSystem, kinds_typescript};
pub use loc::{LineIndex, Loc, Span};
pub use symbols::{
    DeclaredDep, DeclaredManifest, DepSection, Export, ExportKind, ExportMember, UsedNames,
};
pub use violation::{CycleStep, Fix, Subject, Violation, ViolationLevel};
