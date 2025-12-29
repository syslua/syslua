//! syslua-lib: Core types and logic for SysLua
//!
//! This crate provides the fundamental types used throughout SysLua:
//! - `Build`: immutable build recipes that produce store content
//! - `Bind`: describes what to do with derivation outputs
//! - `Manifest`: the complete set of derivations and activations
//! - `Snapshot`: rollback journal for restoring previous system state

pub mod action;
pub mod bind;
pub mod build;
pub mod consts;
pub mod eval;
pub mod execute;
pub mod init;
pub mod inputs;
pub mod lua;
pub mod manifest;
pub mod outputs;
pub mod placeholder;
pub mod platform;
pub mod snapshot;
pub mod update;
pub mod util;
