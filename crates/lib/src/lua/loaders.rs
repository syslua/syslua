//! Custom Lua module loading with per-file `__dir` injection.
//!
//! This module provides custom implementations of `require`, `dofile`, and `loadfile`
//! that inject a `__dir` variable into each loaded file's environment. The `__dir`
//! variable contains the directory path of the currently executing Lua file.
//!
//! # Implementation Strategy
//!
//! Rather than reimplementing `require` entirely, we hook into `package.searchers[2]`
//! (the Lua file loader) with a custom function that:
//! 1. Uses Lua's built-in `package.searchpath` for path resolution
//! 2. Loads files with a custom environment containing `__dir`
//! 3. Preserves all other `require` behavior (caching, preload, C loaders)
//!
//! For `dofile` and `loadfile`, we replace them entirely with Rust functions.

use mlua::prelude::*;
use std::fs;
use std::path::Path;

/// Registry key for storing the current `__dir` value.
/// This is used by `dofile` to resolve relative paths.
const CURRENT_DIR_KEY: &str = "__syslua_current_dir";

/// Load a Lua file with a custom environment containing `__dir`.
///
/// Creates an environment table with:
/// - `__dir` set to the parent directory of the file
/// - A metatable with `__index` pointing to `_G` for global access
///
/// # Arguments
/// * `lua` - The Lua state
/// * `path` - Path to the Lua file to load
///
/// # Returns
/// The result of evaluating the Lua file
pub fn load_file_with_dir(lua: &Lua, path: &Path) -> LuaResult<LuaValue> {
  // Canonicalize the path to resolve . and .. components
  let canonical_path = path
    .canonicalize()
    .map_err(|e| LuaError::external(format!("cannot resolve '{}': {}", path.display(), e)))?;

  let content = fs::read_to_string(&canonical_path)
    .map_err(|e| LuaError::external(format!("cannot read '{}': {}", canonical_path.display(), e)))?;

  let dir = canonical_path
    .parent()
    .unwrap_or(Path::new("."))
    .to_string_lossy()
    .into_owned();

  // Store current __dir in registry for nested dofile calls
  let prev_dir: Option<String> = lua.named_registry_value(CURRENT_DIR_KEY)?;
  lua.set_named_registry_value(CURRENT_DIR_KEY, dir.clone())?;

  // Create environment table with __dir
  let env = lua.create_table()?;
  env.set("__dir", dir)?;

  // Inherit from _G via metatable
  let mt = lua.create_table()?;
  mt.set("__index", lua.globals())?;
  mt.set("__newindex", lua.globals())?;
  env.set_metatable(Some(mt))?;

  // Load and execute with custom environment
  let result = lua
    .load(&content)
    .set_name(format!("@{}", canonical_path.display()))
    .set_environment(env)
    .eval::<LuaValue>();

  // Restore previous __dir (ignore errors during cleanup to avoid masking the original error)
  let _ = lua.set_named_registry_value(CURRENT_DIR_KEY, prev_dir);

  result
}

/// Load a Lua file and return it as a function (without executing).
///
/// Similar to `load_file_with_dir` but returns the chunk as a callable function
/// instead of executing it immediately.
pub fn load_file_as_function(lua: &Lua, path: &Path) -> LuaResult<LuaFunction> {
  // Canonicalize the path to resolve . and .. components
  let canonical_path = path
    .canonicalize()
    .map_err(|e| LuaError::external(format!("cannot resolve '{}': {}", path.display(), e)))?;

  let content = fs::read_to_string(&canonical_path)
    .map_err(|e| LuaError::external(format!("cannot read '{}': {}", canonical_path.display(), e)))?;

  let dir = canonical_path
    .parent()
    .unwrap_or(Path::new("."))
    .to_string_lossy()
    .into_owned();

  // Create environment table with __dir
  let env = lua.create_table()?;
  env.set("__dir", dir)?;

  // Inherit from _G via metatable
  let mt = lua.create_table()?;
  mt.set("__index", lua.globals())?;
  mt.set("__newindex", lua.globals())?;
  env.set_metatable(Some(mt))?;

  lua
    .load(&content)
    .set_name(format!("@{}", canonical_path.display()))
    .set_environment(env)
    .into_function()
}

/// Get the current `__dir` from the registry.
///
/// Returns `None` if no file is currently being loaded.
fn get_current_dir(lua: &Lua) -> LuaResult<Option<String>> {
  lua.named_registry_value(CURRENT_DIR_KEY)
}

/// Resolve a potentially relative path against the current `__dir`.
///
/// If the path is absolute, returns it as-is.
/// If the path is relative and there's a current `__dir`, resolves against it.
/// Otherwise, resolves against the current working directory.
fn resolve_path(lua: &Lua, path_str: &str) -> LuaResult<std::path::PathBuf> {
  let path = Path::new(path_str);

  if path.is_absolute() {
    return Ok(path.to_path_buf());
  }

  // Try to resolve against current __dir
  if let Some(current_dir) = get_current_dir(lua)? {
    let resolved = Path::new(&current_dir).join(path);
    if resolved.exists() {
      return Ok(resolved);
    }
  }

  // Fall back to the path as-is (will be resolved against CWD)
  Ok(path.to_path_buf())
}

/// Create a custom Lua file searcher for `package.searchers[2]`.
///
/// This searcher uses `package.searchpath` to find modules but loads them
/// with our custom `load_file_with_dir` to inject `__dir`.
fn create_lua_searcher(lua: &Lua) -> LuaResult<LuaFunction> {
  lua.create_function(|lua, modname: String| {
    let package: LuaTable = lua.globals().get("package")?;
    let path: String = package.get("path")?;

    // Use package.searchpath to resolve the file path
    let searchpath: LuaFunction = package.get("searchpath")?;
    let result: LuaMultiValue = searchpath.call((modname.clone(), path))?;

    // searchpath returns (filepath) on success or (nil, errmsg) on failure
    let first = result.into_iter().next();

    match first {
      Some(LuaValue::String(filepath_lua)) => {
        let filepath = filepath_lua.to_str()?.to_string();
        let path_clone = filepath.clone();

        // Create a loader function that loads the file with __dir
        let loader = lua.create_function(move |lua, _: LuaMultiValue| {
          let path = Path::new(&path_clone);
          load_file_with_dir(lua, path)
        })?;

        // Return (loader, filepath) - the filepath is passed to the loader as extra data
        Ok((LuaValue::Function(loader), filepath))
      }
      _ => {
        // Module not found - return nil and error message
        let errmsg = format!("\n\tno file for module '{}'", modname);
        Ok((LuaValue::Nil, errmsg))
      }
    }
  })
}

/// Create a custom `dofile` function.
///
/// This version resolves relative paths against the current `__dir`,
/// allowing for intuitive relative imports.
fn create_dofile(lua: &Lua) -> LuaResult<LuaFunction> {
  lua.create_function(|lua, path: Option<String>| match path {
    Some(path_str) => {
      let resolved = resolve_path(lua, &path_str)?;
      load_file_with_dir(lua, &resolved)
    }
    None => Err(LuaError::external("dofile() without path not supported")),
  })
}

/// Create a custom `loadfile` function.
///
/// This version resolves relative paths against the current `__dir`
/// and returns a function with `__dir` injected into its environment.
fn create_loadfile(lua: &Lua) -> LuaResult<LuaFunction> {
  lua.create_function(|lua, (path, mode, env): (String, Option<String>, Option<LuaTable>)| {
    // We only support text mode
    if let Some(ref m) = mode
      && m != "t"
      && m != "bt"
    {
      return Err(LuaError::external(format!(
        "loadfile mode '{}' not supported (only 't' and 'bt' allowed)",
        m
      )));
    }

    // If a custom env is provided, we can't inject __dir the same way
    // For now, we ignore the custom env parameter and always inject __dir
    if env.is_some() {
      return Err(LuaError::external("loadfile with custom environment not supported"));
    }

    let resolved = resolve_path(lua, &path)?;
    load_file_as_function(lua, &resolved)
  })
}

/// Install custom module loaders into the Lua runtime.
///
/// This function:
/// 1. Replaces `package.searchers[2]` with our custom Lua file searcher
/// 2. Replaces `dofile` with our custom version
/// 3. Replaces `loadfile` with our custom version
///
/// After calling this, all Lua files loaded via `require`, `dofile`, or `loadfile`
/// will have access to the `__dir` variable containing their parent directory.
pub fn install_loaders(lua: &Lua) -> LuaResult<()> {
  // Replace package.searchers[2] with our custom Lua searcher
  let package: LuaTable = lua.globals().get("package")?;
  let searchers: LuaTable = package.get("searchers")?;
  searchers.set(2, create_lua_searcher(lua)?)?;

  // Replace dofile
  lua.globals().set("dofile", create_dofile(lua)?)?;

  // Replace loadfile
  lua.globals().set("loadfile", create_loadfile(lua)?)?;

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::fs;
  use tempfile::TempDir;

  fn create_test_runtime() -> LuaResult<Lua> {
    let lua = Lua::new();
    lua
      .load(r#"package.path = package.path .. ";./?.lua;./?/init.lua""#)
      .exec()?;
    install_loaders(&lua)?;
    Ok(lua)
  }

  #[test]
  fn test_load_file_with_dir_sets_dir() -> LuaResult<()> {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.lua");
    fs::write(&file_path, "return __dir").unwrap();

    let lua = create_test_runtime()?;
    let result: String = load_file_with_dir(&lua, &file_path)?.to_string().unwrap().to_string();

    // Use canonicalize to resolve symlinks (e.g., /var -> /private/var on macOS)
    let expected = temp_dir.path().canonicalize().unwrap();
    assert_eq!(result, expected.to_string_lossy());
    Ok(())
  }

  #[test]
  fn test_require_injects_dir() -> LuaResult<()> {
    let temp_dir = TempDir::new().unwrap();

    // Create a module file
    let mod_path = temp_dir.path().join("mymod.lua");
    fs::write(&mod_path, "return { dir = __dir }").unwrap();

    let lua = create_test_runtime()?;

    // Add temp dir to package.path
    let package: LuaTable = lua.globals().get("package")?;
    let path: String = package.get("path")?;
    let new_path = format!("{}/?.lua;{}", temp_dir.path().display(), path);
    package.set("path", new_path)?;

    // Require the module
    let result: LuaTable = lua.load("return require('mymod')").eval()?;
    let dir: String = result.get("dir")?;

    // Use canonicalize to resolve symlinks (e.g., /var -> /private/var on macOS)
    let expected = temp_dir.path().canonicalize().unwrap();
    assert_eq!(dir, expected.to_string_lossy());
    Ok(())
  }

  #[test]
  fn test_require_caches_modules() -> LuaResult<()> {
    let temp_dir = TempDir::new().unwrap();

    // Create a module that returns a unique table each time
    let mod_path = temp_dir.path().join("counter.lua");
    fs::write(&mod_path, "return {}").unwrap();

    let lua = create_test_runtime()?;

    // Add temp dir to package.path
    let package: LuaTable = lua.globals().get("package")?;
    let path: String = package.get("path")?;
    let new_path = format!("{}/?.lua;{}", temp_dir.path().display(), path);
    package.set("path", new_path)?;

    // Require twice and check we get the same table
    let code = r#"
            local a = require('counter')
            local b = require('counter')
            return a == b
        "#;
    let result: bool = lua.load(code).eval()?;

    assert!(result, "require should cache modules");
    Ok(())
  }

  /// Escape a path for embedding in a Lua string literal.
  /// On Windows, backslashes need to be doubled to avoid being interpreted as escape sequences.
  fn escape_path_for_lua(path: &Path) -> String {
    path.display().to_string().replace('\\', "\\\\")
  }

  #[test]
  fn test_dofile_with_absolute_path() -> LuaResult<()> {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.lua");
    fs::write(&file_path, "return __dir").unwrap();

    let lua = create_test_runtime()?;

    let code = format!("return dofile('{}')", escape_path_for_lua(&file_path));
    let result: String = lua.load(&code).eval()?;

    // Use canonicalize to resolve symlinks (e.g., /var -> /private/var on macOS)
    let expected = temp_dir.path().canonicalize().unwrap();
    assert_eq!(result, expected.to_string_lossy());
    Ok(())
  }

  #[test]
  fn test_dofile_with_relative_path_from_file() -> LuaResult<()> {
    let temp_dir = TempDir::new().unwrap();

    // Create main.lua that dofiles a relative path
    let main_path = temp_dir.path().join("main.lua");
    fs::write(&main_path, "return dofile('./sub.lua')").unwrap();

    // Create sub.lua in the same directory
    let sub_path = temp_dir.path().join("sub.lua");
    fs::write(&sub_path, "return __dir").unwrap();

    let lua = create_test_runtime()?;
    let result: String = load_file_with_dir(&lua, &main_path)?.to_string().unwrap().to_string();

    // Use canonicalize to resolve symlinks (e.g., /var -> /private/var on macOS)
    let expected = temp_dir.path().canonicalize().unwrap();
    assert_eq!(result, expected.to_string_lossy());
    Ok(())
  }

  #[test]
  fn test_dofile_nested_relative_paths() -> LuaResult<()> {
    let temp_dir = TempDir::new().unwrap();

    // Create directory structure:
    // temp_dir/
    //   main.lua -> dofiles subdir/a.lua
    //   subdir/
    //     a.lua -> dofiles b.lua (relative to subdir)
    //     b.lua -> returns __dir
    let subdir = temp_dir.path().join("subdir");
    fs::create_dir(&subdir).unwrap();

    fs::write(temp_dir.path().join("main.lua"), "return dofile('./subdir/a.lua')").unwrap();
    fs::write(subdir.join("a.lua"), "return dofile('./b.lua')").unwrap();
    fs::write(subdir.join("b.lua"), "return __dir").unwrap();

    let lua = create_test_runtime()?;
    let result: String = load_file_with_dir(&lua, &temp_dir.path().join("main.lua"))?
      .to_string()
      .unwrap()
      .to_string();

    // b.lua should see __dir as the subdir, not the root
    // Use canonicalize to resolve symlinks (e.g., /var -> /private/var on macOS)
    let expected = subdir.canonicalize().unwrap();
    assert_eq!(result, expected.to_string_lossy());
    Ok(())
  }

  #[test]
  fn test_loadfile_returns_function() -> LuaResult<()> {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.lua");
    fs::write(&file_path, "return 42").unwrap();

    let lua = create_test_runtime()?;

    let code = format!(
      r#"
            local f = loadfile('{}')
            return type(f), f()
        "#,
      escape_path_for_lua(&file_path)
    );
    let result: LuaMultiValue = lua.load(&code).eval()?;
    let values: Vec<LuaValue> = result.into_iter().collect();

    assert_eq!(values[0].to_string().unwrap(), "function");
    assert_eq!(values[1].as_i32().unwrap(), 42);
    Ok(())
  }

  #[test]
  fn test_loadfile_has_dir() -> LuaResult<()> {
    let temp_dir = TempDir::new().unwrap();
    let file_path = temp_dir.path().join("test.lua");
    fs::write(&file_path, "return __dir").unwrap();

    let lua = create_test_runtime()?;

    let code = format!("return loadfile('{}')()", escape_path_for_lua(&file_path));
    let result: String = lua.load(&code).eval()?;

    // Use canonicalize to resolve symlinks (e.g., /var -> /private/var on macOS)
    let expected = temp_dir.path().canonicalize().unwrap();
    assert_eq!(result, expected.to_string_lossy());
    Ok(())
  }

  #[test]
  fn test_module_not_found_error() -> LuaResult<()> {
    let lua = create_test_runtime()?;

    let result = lua.load("require('nonexistent_module_xyz')").exec();

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("nonexistent_module_xyz"));
    Ok(())
  }

  #[test]
  fn test_dofile_file_not_found_error() -> LuaResult<()> {
    let lua = create_test_runtime()?;

    let result = lua.load("dofile('/nonexistent/path/file.lua')").exec();

    assert!(result.is_err());
    Ok(())
  }
}
