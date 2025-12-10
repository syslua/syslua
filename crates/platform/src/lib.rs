//! Platform detection and system abstractions for sys.lua
//!
//! This crate provides cross-platform abstractions for:
//! - OS and architecture detection
//! - Path resolution (store, config, cache)
//! - User information

mod paths;
mod platform;

pub use paths::{StorePaths, SysluaPaths};
pub use platform::{Arch, Os, Platform, PlatformInfo};
