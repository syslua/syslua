//! sys-lua: Lua runtime for sys.lua configuration
//!
//! This crate provides the Lua runtime environment with:
//! - Global functions: derive{}, activate{}
//! - System information: syslua table
//! - Two-phase evaluation: M.inputs extraction + M.setup(inputs) call
//! - DerivationCtx: context passed to derivation config functions during realization
//! - ActivationCtx: context passed to activation config functions during apply

mod error;
mod globals;
mod manifest;
mod runtime;

pub use error::{Error, Result};
pub use globals::{
    opts_to_lua_table, Activation, ActivationAction, ActivationCtx, Collector, Derivation,
    DerivationCtx, OptsValue, Shell,
};
pub use manifest::Manifest;
pub use runtime::Runtime;
