//! sys-platform: OS abstraction layer for sys.lua
//!
//! Provides platform detection, path resolution, and OS-specific functionality.

mod error;
mod paths;
mod platform;
mod shell;

pub use error::PlatformError;
pub use paths::{expand_path, expand_path_with_base};
pub use platform::{Arch, Os, Platform};
pub use shell::Shell;

/// Result type for platform operations
pub type Result<T> = std::result::Result<T, PlatformError>;
