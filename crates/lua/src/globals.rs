//! Global Lua functions and the syslua table

use crate::types::{EnvDecl, EnvMergeStrategy, EnvValue, FileDecl};
use mlua::{Lua, Result as LuaResult, Table, Value};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use sys_platform::Platform;

/// Shared state for collecting declarations during Lua evaluation
pub struct Declarations {
    pub files: Vec<FileDecl>,
    pub envs: Vec<EnvDecl>,
}

impl Default for Declarations {
    fn default() -> Self {
        Self::new()
    }
}

impl Declarations {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            envs: Vec::new(),
        }
    }
}

/// Set up the syslua global table with platform information
pub fn setup_syslua_global(lua: &Lua, platform: &Platform) -> LuaResult<()> {
    let syslua = lua.create_table()?;

    // Platform information
    syslua.set("platform", platform.platform.as_str())?;
    syslua.set("os", platform.os.as_str())?;
    syslua.set("arch", platform.arch.as_str())?;
    syslua.set("hostname", platform.hostname.as_str())?;
    syslua.set("username", platform.username.as_str())?;

    // Boolean helpers
    syslua.set("is_linux", platform.is_linux())?;
    syslua.set("is_darwin", platform.is_darwin())?;
    syslua.set("is_windows", platform.is_windows())?;

    // Version
    syslua.set("version", env!("CARGO_PKG_VERSION"))?;

    lua.globals().set("syslua", syslua)?;

    Ok(())
}

/// Set up the file{} global function
pub fn setup_file_function(
    lua: &Lua,
    declarations: Rc<RefCell<Declarations>>,
    config_dir: PathBuf,
) -> LuaResult<()> {
    let file_fn = lua.create_function(move |_, spec: Table| {
        let path_str: String = spec
            .get::<String>("path")
            .map_err(|_| mlua::Error::runtime("file{} requires 'path' field"))?;

        // Expand ~ in path
        let path = sys_platform::expand_path(&path_str)
            .map_err(|e| mlua::Error::runtime(e.to_string()))?;

        // Get optional fields
        let symlink: Option<String> = spec.get("symlink").ok();
        let content: Option<String> = spec.get("content").ok();
        let copy: Option<String> = spec.get("copy").ok();
        let mode: Option<u32> = spec.get("mode").ok();

        // Expand paths for symlink and copy, resolving relative paths against config dir
        let symlink = symlink
            .map(|s| sys_platform::expand_path_with_base(&s, &config_dir))
            .transpose()
            .map_err(|e| mlua::Error::runtime(e.to_string()))?;

        let copy = copy
            .map(|s| sys_platform::expand_path_with_base(&s, &config_dir))
            .transpose()
            .map_err(|e| mlua::Error::runtime(e.to_string()))?;

        let decl = FileDecl {
            path,
            symlink,
            content,
            copy,
            mode,
        };

        // Validate the declaration
        decl.validate()
            .map_err(|e| mlua::Error::runtime(e.to_string()))?;

        // Add to declarations
        declarations.borrow_mut().files.push(decl);

        Ok(())
    })?;

    lua.globals().set("file", file_fn)?;

    Ok(())
}

/// Set up the env{} global function
///
/// Usage from Lua:
/// ```lua
/// env {
///     EDITOR = "nvim",              -- simple value (replaces existing)
///     PATH = { "~/.local/bin" },    -- array = prepend to PATH
///     MANPATH = { append = "/usr/share/man" },  -- explicit append
/// }
/// ```
pub fn setup_env_function(lua: &Lua, declarations: Rc<RefCell<Declarations>>) -> LuaResult<()> {
    let env_fn = lua.create_function(move |_, spec: Table| {
        for pair in spec.pairs::<String, Value>() {
            let (name, value) = pair?;

            let env_decl = parse_env_value(&name, value)?;
            declarations.borrow_mut().envs.push(env_decl);
        }

        Ok(())
    })?;

    lua.globals().set("env", env_fn)?;

    Ok(())
}

/// Parse a Lua value into an EnvDecl
fn parse_env_value(name: &str, value: Value) -> Result<EnvDecl, mlua::Error> {
    match value {
        // Simple string value: EDITOR = "nvim"
        Value::String(s) => {
            let value_str = s.to_str()?.to_string();
            // Expand ~ in the value
            let expanded = expand_env_path(&value_str);
            Ok(EnvDecl::new(name, expanded))
        }

        // Array of strings: PATH = { "~/.local/bin", "~/.cargo/bin" }
        // This means prepend these paths
        Value::Table(t) => {
            // Check if it's a table with explicit strategy keys
            // Use raw_get to check for nil explicitly
            let prepend_val: Value = t.get("prepend")?;
            if !matches!(prepend_val, Value::Nil) {
                return parse_strategy_value(name, prepend_val, EnvMergeStrategy::Prepend);
            }

            let append_val: Value = t.get("append")?;
            if !matches!(append_val, Value::Nil) {
                return parse_strategy_value(name, append_val, EnvMergeStrategy::Append);
            }

            // Otherwise treat as array of prepend values
            let mut values = Vec::new();
            for item in t.sequence_values::<String>() {
                let path = item?;
                let expanded = expand_env_path(&path);
                values.push(EnvValue::prepend(expanded));
            }

            if values.is_empty() {
                return Err(mlua::Error::runtime(format!(
                    "env var '{}' has empty array value",
                    name
                )));
            }

            Ok(EnvDecl {
                name: name.to_string(),
                values,
            })
        }

        _ => Err(mlua::Error::runtime(format!(
            "env var '{}' must be a string or table, got {:?}",
            name,
            value.type_name()
        ))),
    }
}

/// Parse a value with an explicit merge strategy
fn parse_strategy_value(
    name: &str,
    value: Value,
    strategy: EnvMergeStrategy,
) -> Result<EnvDecl, mlua::Error> {
    match value {
        Value::String(s) => {
            let value_str = s.to_str()?.to_string();
            let expanded = expand_env_path(&value_str);
            Ok(EnvDecl {
                name: name.to_string(),
                values: vec![EnvValue {
                    value: expanded,
                    strategy,
                }],
            })
        }
        Value::Table(t) => {
            let mut values = Vec::new();
            for item in t.sequence_values::<String>() {
                let path = item?;
                let expanded = expand_env_path(&path);
                values.push(EnvValue {
                    value: expanded,
                    strategy: strategy.clone(),
                });
            }
            Ok(EnvDecl {
                name: name.to_string(),
                values,
            })
        }
        _ => Err(mlua::Error::runtime(format!(
            "env var '{}' strategy value must be a string or array",
            name
        ))),
    }
}

/// Expand ~ in environment variable paths
fn expand_env_path(value: &str) -> String {
    if let Some(stripped) = value.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return format!("{}/{}", home.display(), stripped);
        }
    }
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_syslua_global() {
        let lua = Lua::new();
        let platform = Platform::detect().unwrap();

        setup_syslua_global(&lua, &platform).unwrap();

        let syslua: Table = lua.globals().get("syslua").unwrap();

        let os: String = syslua.get("os").unwrap();
        assert!(!os.is_empty());

        let is_darwin: bool = syslua.get("is_darwin").unwrap();
        let is_linux: bool = syslua.get("is_linux").unwrap();
        let is_windows: bool = syslua.get("is_windows").unwrap();

        // Exactly one should be true
        assert_eq!(
            [is_darwin, is_linux, is_windows]
                .iter()
                .filter(|&&x| x)
                .count(),
            1
        );
    }

    #[test]
    fn test_file_function_symlink() {
        let lua = Lua::new();
        let declarations = Rc::new(RefCell::new(Declarations::new()));
        let config_dir = PathBuf::from("/home/user/config");

        setup_file_function(&lua, declarations.clone(), config_dir).unwrap();

        lua.load(
            r#"
            file {
                path = "~/.gitconfig",
                symlink = "./dotfiles/gitconfig",
            }
        "#,
        )
        .exec()
        .unwrap();

        let decls = declarations.borrow();
        assert_eq!(decls.files.len(), 1);

        let file = &decls.files[0];
        assert!(file.path.to_string_lossy().contains(".gitconfig"));
        assert!(file.symlink.is_some());
        assert_eq!(file.kind(), "symlink");
    }

    #[test]
    fn test_file_function_content() {
        let lua = Lua::new();
        let declarations = Rc::new(RefCell::new(Declarations::new()));
        let config_dir = PathBuf::from("/home/user/config");

        setup_file_function(&lua, declarations.clone(), config_dir).unwrap();

        lua.load(
            r#"
            file {
                path = "/tmp/test.txt",
                content = "Hello, world!",
            }
        "#,
        )
        .exec()
        .unwrap();

        let decls = declarations.borrow();
        assert_eq!(decls.files.len(), 1);

        let file = &decls.files[0];
        assert_eq!(file.content.as_deref(), Some("Hello, world!"));
    }

    #[test]
    fn test_file_function_validation_error() {
        let lua = Lua::new();
        let declarations = Rc::new(RefCell::new(Declarations::new()));
        let config_dir = PathBuf::from("/home/user/config");

        setup_file_function(&lua, declarations.clone(), config_dir).unwrap();

        // Missing required field
        let result = lua
            .load(
                r#"
            file {
                path = "/tmp/test.txt",
            }
        "#,
            )
            .exec();

        assert!(result.is_err());
    }

    #[test]
    fn test_env_function_simple() {
        let lua = Lua::new();
        let declarations = Rc::new(RefCell::new(Declarations::new()));

        setup_env_function(&lua, declarations.clone()).unwrap();

        lua.load(
            r#"
            env {
                EDITOR = "nvim",
            }
        "#,
        )
        .exec()
        .unwrap();

        let decls = declarations.borrow();
        assert_eq!(decls.envs.len(), 1);

        let env = &decls.envs[0];
        assert_eq!(env.name, "EDITOR");
        assert_eq!(env.values.len(), 1);
        assert_eq!(env.values[0].value, "nvim");
        assert!(!env.is_path_like());
    }

    #[test]
    fn test_env_function_path_prepend() {
        let lua = Lua::new();
        let declarations = Rc::new(RefCell::new(Declarations::new()));

        setup_env_function(&lua, declarations.clone()).unwrap();

        lua.load(
            r#"
            env {
                PATH = { "/usr/local/bin", "/opt/bin" },
            }
        "#,
        )
        .exec()
        .unwrap();

        let decls = declarations.borrow();
        assert_eq!(decls.envs.len(), 1);

        let env = &decls.envs[0];
        assert_eq!(env.name, "PATH");
        assert_eq!(env.values.len(), 2);
        assert!(env.is_path_like());
        assert!(matches!(env.values[0].strategy, EnvMergeStrategy::Prepend));
    }

    #[test]
    fn test_env_function_explicit_append() {
        let lua = Lua::new();
        let declarations = Rc::new(RefCell::new(Declarations::new()));

        setup_env_function(&lua, declarations.clone()).unwrap();

        lua.load(
            r#"
            env {
                MANPATH = { append = "/usr/share/man" },
            }
        "#,
        )
        .exec()
        .unwrap();

        let decls = declarations.borrow();
        assert_eq!(decls.envs.len(), 1);

        let env = &decls.envs[0];
        assert_eq!(env.name, "MANPATH");
        assert!(matches!(env.values[0].strategy, EnvMergeStrategy::Append));
    }

    #[test]
    fn test_env_function_tilde_expansion() {
        let lua = Lua::new();
        let declarations = Rc::new(RefCell::new(Declarations::new()));

        setup_env_function(&lua, declarations.clone()).unwrap();

        lua.load(
            r#"
            env {
                PATH = { "~/.local/bin" },
            }
        "#,
        )
        .exec()
        .unwrap();

        let decls = declarations.borrow();
        let env = &decls.envs[0];

        // Should have expanded ~ to home directory
        assert!(!env.values[0].value.starts_with("~/"));
        assert!(env.values[0].value.contains(".local/bin"));
    }

    #[test]
    fn test_env_function_multiple() {
        let lua = Lua::new();
        let declarations = Rc::new(RefCell::new(Declarations::new()));

        setup_env_function(&lua, declarations.clone()).unwrap();

        lua.load(
            r#"
            env {
                EDITOR = "nvim",
                PAGER = "less",
            }
        "#,
        )
        .exec()
        .unwrap();

        let decls = declarations.borrow();
        assert_eq!(decls.envs.len(), 2);
    }
}
