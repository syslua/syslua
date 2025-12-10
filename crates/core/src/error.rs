//! Error types for sys-core

use thiserror::Error;

/// Errors that can occur in core operations
#[derive(Debug, Error)]
pub enum CoreError {
    #[error("Lua evaluation error: {0}")]
    Lua(#[from] sys_lua::LuaError),

    #[error("Platform error: {0}")]
    Platform(#[from] sys_platform::PlatformError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("File operation failed for '{path}': {message}")]
    FileOperation { path: String, message: String },

    #[error("Symlink target does not exist: {0}")]
    SymlinkTargetMissing(String),

    #[error("Cannot overwrite existing file without --force: {0}")]
    FileExists(String),
}
