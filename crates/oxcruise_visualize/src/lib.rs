//! The site that `oxcruise --visualize` writes: a compact data file plus a prebuilt viewer.

mod data;
mod site;

pub use data::{VIZ_VERSION, VizData};
pub use site::{data_script, write_site};
