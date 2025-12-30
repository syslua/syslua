//! Bind definition and execution.
//!
//! Binds are the mechanism for deploying build outputs to the target system.
//! Unlike builds which are pure and cached, binds have side effects and manage
//! stateful resources like symlinks, files, and services.
//!
//! # Lifecycle
//!
//! Each bind has three lifecycle hooks defined in Lua:
//! - `check()` - Determine if the bind needs to be applied (returns boolean)
//! - `create()` - Apply the bind to the system
//! - `update()` - Update an existing bind (optional, defaults to remove + create)
//! - `remove()` - Remove the bind from the system
//!
//! # Submodules
//!
//! - [`execute`] - Bind execution engine
//! - [`lua`] - Lua context (`BindCtx`) exposed to bind scripts
//! - [`state`] - Bind state tracking for the current system
//! - [`store`] - Persistent bind metadata in the store

pub mod execute;
pub mod lua;
pub mod state;
pub mod store;
mod types;

pub use types::*;
