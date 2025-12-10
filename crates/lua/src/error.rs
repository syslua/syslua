//! Error types for sys-lua

use thiserror::Error;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Lua error: {0}")]
    Lua(#[from] mlua::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Config error: {0}")]
    Config(String),

    #[error("Evaluation error: {0}")]
    Eval(String),

    #[error("Missing required field: {0}")]
    MissingField(String),

    #[error("Invalid entry point: {0}")]
    InvalidEntryPoint(String),
}

pub type Result<T> = std::result::Result<T, Error>;
