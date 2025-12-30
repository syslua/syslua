//! Lua runtime and evaluation.
//!
//! This module provides the Lua execution environment for syslua configurations.
//! It manages the Lua VM lifecycle, registers global functions and types, and
//! evaluates user configuration files.
//!
//! # Submodules
//!
//! - [`entrypoint`] - Configuration file loading and evaluation
//! - [`globals`] - Global Lua functions (`build()`, `bind()`, `input()`, etc.)
//! - [`helpers`] - Lua helper modules exposed to user scripts
//! - [`runtime`] - Low-level Lua VM management

pub mod entrypoint;
pub mod globals;
pub mod helpers;
pub mod runtime;
