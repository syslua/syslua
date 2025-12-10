//! Lua configuration evaluation

use crate::error::LuaError;
use crate::globals::{Declarations, setup_env_function, setup_file_function, setup_syslua_global};
use crate::types::{EnvDecl, FileDecl};
use mlua::Lua;
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use sys_platform::Platform;

/// Context for evaluating a Lua configuration file
pub struct EvalContext {
    /// Platform information
    pub platform: Platform,
    /// Directory containing the config file (for resolving relative paths)
    pub config_dir: PathBuf,
}

impl EvalContext {
    /// Create a new evaluation context
    pub fn new(config_path: &Path) -> Result<Self, LuaError> {
        let platform = Platform::detect()?;

        let config_dir = config_path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));

        // Make config_dir absolute
        let config_dir = if config_dir.is_absolute() {
            config_dir
        } else {
            std::env::current_dir()?.join(config_dir)
        };

        Ok(Self {
            platform,
            config_dir,
        })
    }
}

/// Result of evaluating a Lua configuration
pub struct EvalResult {
    /// File declarations collected during evaluation
    pub files: Vec<FileDecl>,
    /// Environment variable declarations collected during evaluation
    pub envs: Vec<EnvDecl>,
}

/// Evaluate a Lua configuration file and return the collected declarations
///
/// # Example
///
/// ```ignore
/// use sys_lua::evaluate_config;
/// use std::path::Path;
///
/// let result = evaluate_config(Path::new("init.lua"))?;
/// for file in result.files {
///     println!("File: {}", file.path.display());
/// }
/// ```
pub fn evaluate_config(config_path: &Path) -> Result<EvalResult, LuaError> {
    // Read the config file
    if !config_path.exists() {
        return Err(LuaError::ConfigNotFound(config_path.display().to_string()));
    }

    let config_source = std::fs::read_to_string(config_path)?;

    // Create evaluation context
    let ctx = EvalContext::new(config_path)?;

    // Evaluate the config
    evaluate_config_string(&config_source, &ctx)
}

/// Evaluate a Lua configuration from a string
///
/// This is useful for testing or when the config is embedded.
pub fn evaluate_config_string(source: &str, ctx: &EvalContext) -> Result<EvalResult, LuaError> {
    let lua = Lua::new();

    // Set up the global syslua table
    setup_syslua_global(&lua, &ctx.platform)?;

    // Create shared declarations state
    let declarations = Rc::new(RefCell::new(Declarations::new()));

    // Set up the file{} function
    setup_file_function(&lua, declarations.clone(), ctx.config_dir.clone())?;

    // Set up the env{} function
    setup_env_function(&lua, declarations.clone())?;

    // Execute the config
    lua.load(source).exec()?;

    // Extract the declarations
    let decls = declarations.borrow();

    Ok(EvalResult {
        files: decls.files.clone(),
        envs: decls.envs.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_evaluate_config_string() {
        let ctx = EvalContext {
            platform: Platform::detect().unwrap(),
            config_dir: PathBuf::from("/tmp"),
        };

        let result = evaluate_config_string(
            r#"
            file {
                path = "~/.gitconfig",
                symlink = "./dotfiles/gitconfig",
            }

            file {
                path = "/tmp/test.txt",
                content = "Hello!",
            }
        "#,
            &ctx,
        )
        .unwrap();

        assert_eq!(result.files.len(), 2);
    }

    #[test]
    fn test_evaluate_config_file() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(
            temp_file,
            r#"
            file {{
                path = "/tmp/test.txt",
                content = "Hello from file!",
            }}
        "#
        )
        .unwrap();

        let result = evaluate_config(temp_file.path()).unwrap();

        assert_eq!(result.files.len(), 1);
        assert_eq!(result.files[0].content.as_deref(), Some("Hello from file!"));
    }

    #[test]
    fn test_evaluate_config_not_found() {
        let result = evaluate_config(Path::new("/nonexistent/path/config.lua"));
        assert!(matches!(result, Err(LuaError::ConfigNotFound(_))));
    }

    #[test]
    fn test_platform_conditionals() {
        let ctx = EvalContext {
            platform: Platform::detect().unwrap(),
            config_dir: PathBuf::from("/tmp"),
        };

        // This should work regardless of platform
        let result = evaluate_config_string(
            r#"
            if syslua.is_darwin or syslua.is_linux or syslua.is_windows then
                file {
                    path = "/tmp/platform-test.txt",
                    content = syslua.platform,
                }
            end
        "#,
            &ctx,
        )
        .unwrap();

        assert_eq!(result.files.len(), 1);
        assert!(result.files[0].content.is_some());
    }

    #[test]
    fn test_evaluate_env_declarations() {
        let ctx = EvalContext {
            platform: Platform::detect().unwrap(),
            config_dir: PathBuf::from("/tmp"),
        };

        let result = evaluate_config_string(
            r#"
            env {
                EDITOR = "nvim",
                PATH = { "/usr/local/bin" },
            }
        "#,
            &ctx,
        )
        .unwrap();

        assert_eq!(result.envs.len(), 2);
    }

    #[test]
    fn test_evaluate_mixed_declarations() {
        let ctx = EvalContext {
            platform: Platform::detect().unwrap(),
            config_dir: PathBuf::from("/tmp"),
        };

        let result = evaluate_config_string(
            r#"
            file {
                path = "/tmp/test.txt",
                content = "Hello!",
            }

            env {
                EDITOR = "nvim",
            }
        "#,
            &ctx,
        )
        .unwrap();

        assert_eq!(result.files.len(), 1);
        assert_eq!(result.envs.len(), 1);
    }
}
