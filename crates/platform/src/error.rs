//! Error types for sys-platform

use thiserror::Error;

/// Errors that can occur in platform operations
#[derive(Debug, Error)]
pub enum PlatformError {
    #[error("Failed to determine home directory")]
    NoHomeDirectory,

    #[error("Failed to get hostname: {0}")]
    Hostname(String),

    #[error("Failed to get username: {0}")]
    Username(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Path error: {0}")]
    InvalidPath(String),
}
