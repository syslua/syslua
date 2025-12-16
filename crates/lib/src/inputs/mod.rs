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
//! - [`types`] - Core input types

pub mod fetch;
pub mod lock;
pub mod resolve;
pub mod source;
