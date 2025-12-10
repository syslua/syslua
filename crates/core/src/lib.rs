//! sys-core: Core logic for sys.lua
//!
//! This crate provides the manifest, plan, and apply functionality for sys.lua.

mod env;
mod error;
mod manifest;
mod plan;

pub use env::{generate_env_script, source_command, write_env_scripts};
pub use error::CoreError;
pub use manifest::Manifest;
pub use plan::{ApplyOptions, FileChange, FileChangeKind, Plan, apply, compute_plan};

// Re-export types from sys-lua for convenience
pub use sys_lua::{EnvDecl, EnvMergeStrategy, EnvValue, FileDecl};
// Re-export Shell from sys-platform
pub use sys_platform::Shell;

/// Result type for core operations
pub type Result<T> = std::result::Result<T, CoreError>;
