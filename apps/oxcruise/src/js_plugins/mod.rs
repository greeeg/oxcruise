//! Bridge to JS plugin rules, plugin reporters and JS configs.
//!
//! `graph_buffer` has no napi dependency and is always built (and tested); the host that
//! calls into Node exists only with the `napi` feature.

pub mod graph_buffer;

#[cfg(all(feature = "napi", target_pointer_width = "64"))]
pub mod host;
