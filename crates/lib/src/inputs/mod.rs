//! Input resolution and management.
//!
//! This module handles resolving external inputs (git repositories and local paths)
//! that are declared in the config's `M.inputs` table.
//!
//! # Modules
//!
//! - [`source`] - URL parsing for input sources
//! - [`lock`] - Lock file management for reproducible builds
//! - [`fetch`] - Git fetch and path resolution operations
//! - [`resolve`] - High-level resolution orchestration
//! - [`types`] - Core input types (declarations, overrides, resolved inputs)
//! - [`graph`] - Dependency graph building and traversal
//! - [`store`] - Content-addressed input store with dependency linking

pub mod fetch;
pub mod graph;
pub mod lock;
pub mod resolve;
pub mod source;
pub mod store;
mod types;

pub use types::*;
