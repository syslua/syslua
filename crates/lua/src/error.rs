//! Error types for sys-lua

use thiserror::Error;

/// Errors that can occur during Lua evaluation
#[derive(Debug, Error)]
pub enum LuaError {
    #[error("Lua runtime error: {0}")]
    Runtime(#[from] mlua::Error),

    #[error("Config file not found: {0}")]
    ConfigNotFound(String),

    #[error("Invalid file declaration: {0}")]
    InvalidFileDecl(String),

    #[error("Platform error: {0}")]
    Platform(#[from] sys_platform::PlatformError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Evaluation error at {location}: {message}")]
    EvalError { location: String, message: String },
}
