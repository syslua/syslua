//! Error types for sys-core

use std::path::PathBuf;
use thiserror::Error;

/// Result type for sys-core operations
pub type Result<T> = std::result::Result<T, Error>;

/// Errors that can occur during sys-core operations
#[derive(Error, Debug)]
pub enum Error {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Hash mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },

    #[error("Store error: {0}")]
    Store(String),

    #[error("Path not found: {0}")]
    PathNotFound(PathBuf),

    #[error("Unsupported archive format: {0}")]
    UnsupportedArchive(String),

    #[error("Derivation error: {0}")]
    Derivation(String),
}
