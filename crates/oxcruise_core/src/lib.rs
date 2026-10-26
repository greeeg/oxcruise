//! Data model shared by every oxcruise crate. No I/O lives here.

mod graph;
mod kinds;
mod loc;
mod violation;

pub use graph::{Edge, EdgeId, Graph, Module, ModuleId, ModuleKind};
pub use kinds::{DepKinds, ModuleSystem, kinds_typescript};
pub use loc::{LineIndex, Loc, Span};
pub use violation::{CycleStep, Severity, Violation};
