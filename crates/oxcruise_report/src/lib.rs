//! Output formats. Every reporter renders one owned [`CruiseResult`], which is also
//! what the native `json` format serialises.

mod format;
mod model;
mod reporters;

pub use format::{OutputFormat, ParseFormatError};
pub use model::{
    CruiseResult, CycleHop, DepOut, ModuleOut, RESULT_VERSION, RuleRef, Summary, ViolationKind,
    ViolationOut, violation_outs,
};
pub use reporters::{ExternalReporters, ReporterOptions, render};
