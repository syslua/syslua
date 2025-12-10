//! sys-lua: Lua configuration evaluation for sys.lua
//!
//! This crate provides the Lua runtime and API for evaluating sys.lua configurations.

mod error;
mod eval;
mod globals;
mod types;

pub use error::LuaError;
pub use eval::{EvalContext, evaluate_config};
pub use types::{EnvDecl, EnvMergeStrategy, EnvValue, FileDecl};

/// Result type for Lua operations
pub type Result<T> = std::result::Result<T, LuaError>;
