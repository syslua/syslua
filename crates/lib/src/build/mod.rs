//! Build definition and execution.
//!
//! Builds are hermetic, reproducible operations that produce artifacts in the store.
//! Each build is identified by a content hash of its definition, ensuring that
//! identical inputs always produce the same output path.
//!
//! # Characteristics
//!
//! - **Pure**: Builds have no side effects outside their output directory
//! - **Cached**: Once built, outputs are stored by hash and never rebuilt
//! - **Composable**: Builds can depend on outputs from other builds
//!
//! # Submodules
//!
//! - [`execute`] - Build execution engine
//! - [`lua`] - Lua context (`BuildCtx`) exposed to build scripts
//! - [`store`] - Build artifact storage and retrieval

pub mod execute;
pub mod lua;
pub mod store;
mod types;

pub use types::*;
