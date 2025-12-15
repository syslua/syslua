//! syslua-core: Core types and logic for sys.lua
//!
//! This crate provides the fundamental types used throughout sys.lua:
//! - `Derivation`: immutable build recipes that produce store content
//! - `Activation`: describes what to do with derivation outputs
//! - `Manifest`: the complete set of derivations and activations
//! - `Snapshot`: rollback journal for restoring previous system state
//!
//! The types are designed to be Lua-runtime agnostic. The `syslua-lua` crate
//! handles conversion between Lua values and these types.

pub mod bind;
pub mod build;
pub mod consts;
pub mod error;
pub mod eval;
pub mod execute;
pub mod inputs;
pub mod lua;
pub mod manifest;
pub mod placeholder;
pub mod platform;
pub mod snapshot;
pub mod store;
pub mod types;
