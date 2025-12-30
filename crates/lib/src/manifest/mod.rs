//! Build and bind manifest types.
//!
//! Manifests are the evaluated result of Lua configuration, containing all
//! defined builds, binds, and their dependencies ready for execution.

mod types;

pub use types::*;
