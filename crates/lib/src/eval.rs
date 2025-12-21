//! Configuration file evaluation.
//!
//! This module provides the `evaluate_config` function which takes a path to a
//! Lua configuration file and returns the resulting `Manifest` containing all
//! builds and bindings defined in the configuration.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use mlua::prelude::*;
use tracing::info;

use crate::init::update_luarc_inputs;
use crate::inputs::resolve::{ResolveError, resolve_inputs, save_lock_file_if_changed};
use crate::inputs::{InputDecl, InputDecls, InputOverride, ResolvedInputs};
use crate::lua::{loaders, runtime};
use crate::manifest::Manifest;
use crate::platform;

/// Registry key for storing input name → path mappings.
const INPUT_PATHS_REGISTRY_KEY: &str = "__syslua_input_paths";

/// Errors that can occur during config evaluation.
#[derive(Debug, thiserror::Error)]
pub enum EvalError {
  /// Lua evaluation error.
  #[error("lua error: {0}")]
  Lua(#[from] LuaError),

  /// Input resolution error.
  #[error("input resolution error: {0}")]
  InputResolution(#[from] ResolveError),
}

/// Evaluate a Lua configuration file and return the resulting manifest.
///
/// This function:
/// 1. Creates a new Lua runtime with the `sys` global
/// 2. Loads and executes the configuration file
/// 3. Resolves all declared inputs (fetching git repos, resolving paths)
/// 4. Calls the `setup(inputs)` function with resolved inputs
/// 5. Returns the manifest containing all registered builds and bindings
///
/// # Arguments
/// * `path` - Path to the Lua configuration file
///
/// # Returns
/// The `Manifest` containing all builds and bindings defined in the config,
/// or an `EvalError` if evaluation fails.
///
/// # Example
/// ```ignore
/// use std::path::Path;
/// use syslua_lib::eval::evaluate_config;
///
/// let manifest = evaluate_config(Path::new("init.lua"))?;
/// println!("Builds: {}", manifest.builds.len());
/// println!("Bindings: {}", manifest.bindings.len());
/// ```
pub fn evaluate_config(path: &Path) -> Result<Manifest, EvalError> {
  let manifest = Rc::new(RefCell::new(Manifest::default()));
  let config_dir = path.parent().unwrap_or(Path::new("."));

  // Create runtime and evaluate in a block to ensure lua is dropped
  // before we try to unwrap the manifest Rc
  {
    let lua = runtime::create_runtime(manifest.clone())?;
    let config = loaders::load_file_with_dir(&lua, path)?;

    // Config should return a table with { inputs, setup }
    if let LuaValue::Table(config_table) = config {
      // Get the setup function
      let setup: LuaFunction = config_table
        .get("setup")
        .map_err(|_| LuaError::external("config must return a table with a 'setup' function"))?;

      // Extract raw inputs table (supports both simple URLs and extended syntax)
      let input_decls = extract_raw_inputs(&config_table)?;

      // Resolve inputs (fetch git repos, resolve paths) with transitive dependencies
      let resolved = if input_decls.is_empty() {
        info!("no inputs to resolve");
        None
      } else {
        info!(
          count = input_decls.len(),
          "resolving inputs with transitive dependencies"
        );
        let result = resolve_inputs(&input_decls, config_dir, None)?;

        // Save lock file if it changed
        save_lock_file_if_changed(&result, config_dir)?;

        // Update .luarc.json with resolved input paths for LuaLS
        let system = platform::is_elevated();
        let input_paths: Vec<_> = result.inputs.values().map(|i| i.path.as_path()).collect();
        update_luarc_inputs(config_dir, input_paths, system);

        // Register input searcher so require("input-name") works
        register_input_searcher(&lua, &result.inputs)?;

        Some(result.inputs)
      };

      // Build Lua inputs table for setup()
      let inputs_table = build_inputs_table(&lua, resolved.as_ref())?;

      // Call setup(inputs) to register builds and binds
      setup.call::<()>(inputs_table)?;
    } else {
      return Err(LuaError::external("config must return a table with 'inputs' and 'setup' fields").into());
    }

    // lua is dropped here, releasing its references to manifest
  }

  // Now we should have the only reference to manifest
  Ok(
    Rc::try_unwrap(manifest)
      .expect("manifest still has references")
      .into_inner(),
  )
}

/// Extract raw inputs from the config table.
///
/// Supports both simple URL strings and extended table syntax:
/// ```lua
/// inputs = {
///   -- Simple: just a URL
///   utils = "git:https://github.com/org/utils.git",
///
///   -- Extended: URL with transitive overrides
///   pkgs = {
///     url = "git:https://github.com/org/pkgs.git",
///     inputs = {
///       utils = { follows = "utils" },
///     },
///   },
/// }
/// ```
fn extract_raw_inputs(config_table: &LuaTable) -> LuaResult<InputDecls> {
  let mut decls = std::collections::BTreeMap::new();

  let inputs_value: LuaValue = config_table.get("inputs")?;
  if let LuaValue::Table(inputs_table) = inputs_value {
    for pair in inputs_table.pairs::<String, LuaValue>() {
      let (name, value) = pair?;
      let decl = parse_input_decl(&name, value)?;
      decls.insert(name, decl);
    }
  }
  // If inputs is nil or not a table, return empty map (no inputs)

  Ok(decls)
}

/// Parse a single input declaration from a Lua value.
fn parse_input_decl(name: &str, value: LuaValue) -> LuaResult<InputDecl> {
  match value {
    LuaValue::String(url) => {
      let url_str = url.to_str()?.to_string();
      Ok(InputDecl::Url(url_str))
    }
    LuaValue::Table(table) => {
      // Extended syntax: { url = "...", inputs = { ... } }
      let url: Option<String> = table.get("url")?;
      let inputs_value: LuaValue = table.get("inputs")?;

      let overrides = match inputs_value {
        LuaValue::Nil => std::collections::BTreeMap::new(),
        LuaValue::Table(inputs_table) => parse_input_overrides(name, &inputs_table)?,
        _ => {
          return Err(LuaError::external(format!(
            "input '{}': inputs field must be a table",
            name
          )));
        }
      };

      Ok(InputDecl::Extended { url, inputs: overrides })
    }
    _ => Err(LuaError::external(format!(
      "input '{}' must be a string URL or a table",
      name
    ))),
  }
}

/// Parse input overrides from a table.
fn parse_input_overrides(
  parent_name: &str,
  table: &LuaTable,
) -> LuaResult<std::collections::BTreeMap<String, InputOverride>> {
  let mut overrides = std::collections::BTreeMap::new();

  for pair in table.pairs::<String, LuaValue>() {
    let (name, value) = pair?;
    let override_ = parse_single_override(parent_name, &name, value)?;
    overrides.insert(name, override_);
  }

  Ok(overrides)
}

/// Parse a single input override.
fn parse_single_override(parent_name: &str, name: &str, value: LuaValue) -> LuaResult<InputOverride> {
  match value {
    LuaValue::String(url) => {
      // String is interpreted as a URL override
      let url_str = url.to_str()?.to_string();
      Ok(InputOverride::Url(url_str))
    }
    LuaValue::Table(table) => {
      // Check for follows
      let follows: Option<String> = table.get("follows")?;
      if let Some(follows_path) = follows {
        return Ok(InputOverride::Follows(follows_path));
      }

      // Check for url
      let url: Option<String> = table.get("url")?;
      if let Some(url_str) = url {
        return Ok(InputOverride::Url(url_str));
      }

      Err(LuaError::external(format!(
        "input '{}': override '{}' must have either 'url' or 'follows' field",
        parent_name, name
      )))
    }
    _ => Err(LuaError::external(format!(
      "input '{}': override '{}' must be a string URL or a table with 'url' or 'follows'",
      parent_name, name
    ))),
  }
}

/// Convert InputDecls to a simple HashMap<String, String> for backwards compatibility.
/// Build a Lua table representing resolved inputs for setup().
///
/// Each input becomes: `inputs.name = { path = "/path/to/input", rev = "abc123", inputs = {...} }`
/// The nested `inputs` table contains the input's resolved transitive dependencies.
fn build_inputs_table(lua: &Lua, resolved: Option<&ResolvedInputs>) -> LuaResult<LuaTable> {
  let inputs = lua.create_table()?;

  if let Some(resolved_inputs) = resolved {
    for (name, input) in resolved_inputs {
      let entry = build_input_entry(lua, input)?;
      inputs.set(name.as_str(), entry)?;
    }
  }

  Ok(inputs)
}

/// Build a Lua table entry for a single resolved input.
///
/// Creates: `{ path = "...", rev = "...", inputs = {...} }`
fn build_input_entry(lua: &Lua, input: &crate::inputs::ResolvedInput) -> LuaResult<LuaTable> {
  let entry = lua.create_table()?;
  entry.set("path", input.path.to_string_lossy().as_ref())?;
  entry.set("rev", input.rev.as_str())?;

  // Recursively build nested inputs table for transitive dependencies
  if !input.inputs.is_empty() {
    let nested_inputs = lua.create_table()?;
    for (dep_name, dep_input) in &input.inputs {
      let dep_entry = build_input_entry(lua, dep_input)?;
      nested_inputs.set(dep_name.as_str(), dep_entry)?;
    }
    entry.set("inputs", nested_inputs)?;
  }

  Ok(entry)
}

/// Register a custom searcher for inputs.
///
/// This searcher handles:
/// - Exact matches: `require("input_name")` → `input_path/init.lua`
/// - Submodules: `require("input_name.foo.bar")` → `input_path/foo/bar.lua`
///   or `input_path/foo/bar/init.lua`
///
/// All files are loaded via `load_file_with_dir()` to ensure `sys.dir` is injected.
fn register_input_searcher(lua: &Lua, resolved: &ResolvedInputs) -> LuaResult<()> {
  // Store input name → base path mappings in registry
  // Only include inputs that have an init.lua (are Lua libraries)
  let inputs_registry = lua.create_table()?;
  for (name, input) in resolved {
    let init_path = input.path.join("init.lua");
    if init_path.exists() {
      // Store the base path (directory), not the init.lua path
      inputs_registry.set(name.as_str(), input.path.to_string_lossy().as_ref())?;
    }
  }
  lua.set_named_registry_value(INPUT_PATHS_REGISTRY_KEY, inputs_registry)?;

  // Create the searcher function
  let searcher = lua.create_function(|lua, module_name: String| {
    let inputs: LuaTable = lua.named_registry_value(INPUT_PATHS_REGISTRY_KEY)?;

    // Check each input to see if module_name matches or is a submodule
    for pair in inputs.pairs::<String, String>() {
      let (input_name, input_base_path) = pair?;

      let file_path = if module_name == input_name {
        // Exact match: require("cool_lib") → cool_lib/init.lua
        let path = Path::new(&input_base_path).join("init.lua");
        if path.exists() { Some(path) } else { None }
      } else if let Some(suffix) = module_name.strip_prefix(&format!("{}.", input_name)) {
        // Submodule: require("cool_lib.foo.bar")
        // Search order:
        //   1. cool_lib/foo/bar.lua
        //   2. cool_lib/foo/bar/init.lua
        //   3. cool_lib/lua/foo/bar.lua      (LuaRocks-style)
        //   4. cool_lib/lua/foo/bar/init.lua (LuaRocks-style)
        let relative = suffix.replace('.', std::path::MAIN_SEPARATOR_STR);
        let base = Path::new(&input_base_path);

        // Try module.lua first (at root)
        let as_file = base.join(format!("{}.lua", relative));
        if as_file.exists() {
          Some(as_file)
        } else {
          // Try module/init.lua (at root)
          let as_dir = base.join(&relative).join("init.lua");
          if as_dir.exists() {
            Some(as_dir)
          } else {
            // Try lua/module.lua (LuaRocks-style)
            let lua_as_file = base.join("lua").join(format!("{}.lua", relative));
            if lua_as_file.exists() {
              Some(lua_as_file)
            } else {
              // Try lua/module/init.lua (LuaRocks-style)
              let lua_as_dir = base.join("lua").join(&relative).join("init.lua");
              if lua_as_dir.exists() { Some(lua_as_dir) } else { None }
            }
          }
        }
      } else {
        None
      };

      if let Some(path) = file_path {
        let path_str = path.to_string_lossy().to_string();
        let path_for_loader = path_str.clone();

        // Create loader function that uses load_file_with_dir for sys.dir injection
        let loader = lua.create_function(move |lua, _: LuaMultiValue| {
          loaders::load_file_with_dir(lua, Path::new(&path_for_loader))
        })?;

        // Return loader function on success
        return Ok(LuaValue::Function(loader));
      }
    }

    // Not an input module - return error string for message accumulation
    let err_msg = format!("\n\tno input '{}'", module_name);
    Ok(LuaValue::String(lua.create_string(&err_msg)?))
  })?;

  // Insert searcher at position 2, shifting existing searchers
  let package: LuaTable = lua.globals().get("package")?;
  let searchers: LuaTable = package.get("searchers")?;
  let len = searchers.len()?;
  for i in (2..=len).rev() {
    let v: LuaValue = searchers.get(i)?;
    searchers.set(i + 1, v)?;
  }
  searchers.set(2, searcher)?;

  Ok(())
}

#[cfg(test)]
mod tests {
  use crate::util::hash::Hashable;

  use super::*;
  use std::fs;
  use tempfile::TempDir;

  #[test]
  fn test_evaluate_empty_config() -> Result<(), EvalError> {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("init.lua");
    fs::write(
      &config_path,
      r#"
        return {
          inputs = {},
          setup = function(inputs)
            -- empty setup
          end,
        }
      "#,
    )
    .unwrap();

    let manifest = evaluate_config(&config_path)?;
    assert!(manifest.builds.is_empty());
    assert!(manifest.bindings.is_empty());
    Ok(())
  }

  #[test]
  fn test_evaluate_config_with_build() -> Result<(), EvalError> {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("init.lua");
    fs::write(
      &config_path,
      r#"
        return {
          inputs = {},
          setup = function(inputs)
            sys.build({
              id = "test",
              create = function(build_inputs, ctx)
                return { out = "/store/test" }
              end,
            })
          end,
        }
      "#,
    )
    .unwrap();

    let manifest = evaluate_config(&config_path)?;
    assert_eq!(manifest.builds.len(), 1);
    assert!(manifest.bindings.is_empty());

    let build = manifest.builds.values().next().unwrap();
    assert_eq!(build.id, Some("test".to_string()));
    Ok(())
  }

  #[test]
  fn test_evaluate_config_with_bind() -> Result<(), EvalError> {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("init.lua");
    fs::write(
      &config_path,
      r#"
        return {
          inputs = {},
          setup = function(inputs)
            sys.bind({
              id = "test",
              create = function(bind_inputs, ctx)
                ctx:exec({ bin = "echo test" })
              end,
              destroy = function(outputs, ctx)
                ctx:exec({ bin = "echo destroy" })
              end,
            })
          end,
        }
      "#,
    )
    .unwrap();

    let manifest = evaluate_config(&config_path)?;
    assert!(manifest.builds.is_empty());
    assert_eq!(manifest.bindings.len(), 1);
    Ok(())
  }

  #[test]
  fn test_evaluate_config_computes_stable_hash() -> Result<(), EvalError> {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("init.lua");
    fs::write(
      &config_path,
      r#"
        return {
          inputs = {},
          setup = function(inputs)
            sys.build({
              id = "test",
              create = function(build_inputs, ctx)
                return { out = "/store/test" }
              end,
            })
          end,
        }
      "#,
    )
    .unwrap();

    let manifest1 = evaluate_config(&config_path)?;
    let manifest2 = evaluate_config(&config_path)?;

    let hash1 = manifest1.compute_hash().unwrap();
    let hash2 = manifest2.compute_hash().unwrap();

    assert_eq!(hash1, hash2);
    Ok(())
  }

  #[test]
  fn test_evaluate_config_missing_setup_fails() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("init.lua");
    fs::write(
      &config_path,
      r#"
        return {
          inputs = {},
        }
      "#,
    )
    .unwrap();

    let result = evaluate_config(&config_path);
    assert!(result.is_err());
  }

  #[test]
  fn test_evaluate_config_not_table_fails() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("init.lua");
    fs::write(&config_path, r#"return "not a table""#).unwrap();

    let result = evaluate_config(&config_path);
    assert!(result.is_err());
  }

  #[test]
  fn test_evaluate_config_with_path_input() -> Result<(), EvalError> {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path();

    // Create a local input directory with init.lua
    let local_input = config_dir.join("my-input");
    fs::create_dir(&local_input).unwrap();
    fs::write(local_input.join("init.lua"), "return { foo = 'bar' }").unwrap();

    let config_path = config_dir.join("init.lua");
    fs::write(
      &config_path,
      r#"
        return {
          inputs = {
            myinput = "path:./my-input",
          },
          setup = function(inputs)
            -- Verify input was resolved
            assert(inputs.myinput, "myinput should be present")
            assert(inputs.myinput.path, "myinput should have path")
            assert(inputs.myinput.rev == "local", "path input should have rev='local'")
          end,
        }
      "#,
    )
    .unwrap();

    let manifest = evaluate_config(&config_path)?;
    assert!(manifest.builds.is_empty());

    // Verify lock file was created
    let lock_path = config_dir.join("syslua.lock");
    assert!(lock_path.exists(), "lock file should be created");

    Ok(())
  }

  #[test]
  fn test_require_input_exact_match() -> Result<(), EvalError> {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path();

    // Create a local input directory with init.lua
    let local_input = config_dir.join("my-lib");
    fs::create_dir(&local_input).unwrap();
    fs::write(
      local_input.join("init.lua"),
      "return { name = 'my-lib', version = '1.0.0' }",
    )
    .unwrap();

    let config_path = config_dir.join("init.lua");
    fs::write(
      &config_path,
      r#"
        return {
          inputs = {
            mylib = "path:./my-lib",
          },
          setup = function(inputs)
            -- require("mylib") should load the input's init.lua
            local lib = require("mylib")
            assert(lib.name == "my-lib", "expected name='my-lib', got " .. tostring(lib.name))
            assert(lib.version == "1.0.0", "expected version='1.0.0', got " .. tostring(lib.version))
          end,
        }
      "#,
    )
    .unwrap();

    evaluate_config(&config_path)?;
    Ok(())
  }

  #[test]
  fn test_require_input_submodule() -> Result<(), EvalError> {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path();

    // Create a local input directory with init.lua and utils.lua
    let local_input = config_dir.join("my-lib");
    fs::create_dir(&local_input).unwrap();
    fs::write(local_input.join("init.lua"), "return { name = 'my-lib' }").unwrap();
    fs::write(local_input.join("utils.lua"), "return { helper = 'works' }").unwrap();

    let config_path = config_dir.join("init.lua");
    fs::write(
      &config_path,
      r#"
        return {
          inputs = {
            mylib = "path:./my-lib",
          },
          setup = function(inputs)
            -- require("mylib.utils") should load the input's utils.lua
            local utils = require("mylib.utils")
            assert(utils.helper == "works", "expected helper='works', got " .. tostring(utils.helper))
          end,
        }
      "#,
    )
    .unwrap();

    evaluate_config(&config_path)?;
    Ok(())
  }

  #[test]
  fn test_require_input_nested_submodule() -> Result<(), EvalError> {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path();

    // Create a local input directory with nested structure
    let local_input = config_dir.join("my-lib");
    fs::create_dir_all(local_input.join("sub")).unwrap();
    fs::write(local_input.join("init.lua"), "return { name = 'my-lib' }").unwrap();
    fs::write(local_input.join("sub").join("module.lua"), "return { nested = true }").unwrap();

    let config_path = config_dir.join("init.lua");
    fs::write(
      &config_path,
      r#"
        return {
          inputs = {
            mylib = "path:./my-lib",
          },
          setup = function(inputs)
            -- require("mylib.sub.module") should load the input's sub/module.lua
            local mod = require("mylib.sub.module")
            assert(mod.nested == true, "expected nested=true")
          end,
        }
      "#,
    )
    .unwrap();

    evaluate_config(&config_path)?;
    Ok(())
  }

  #[test]
  fn test_require_input_submodule_init() -> Result<(), EvalError> {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path();

    // Create a local input directory with sub/init.lua
    let local_input = config_dir.join("my-lib");
    fs::create_dir_all(local_input.join("sub")).unwrap();
    fs::write(local_input.join("init.lua"), "return { name = 'my-lib' }").unwrap();
    fs::write(local_input.join("sub").join("init.lua"), "return { sub_init = true }").unwrap();

    let config_path = config_dir.join("init.lua");
    fs::write(
      &config_path,
      r#"
        return {
          inputs = {
            mylib = "path:./my-lib",
          },
          setup = function(inputs)
            -- require("mylib.sub") should load the input's sub/init.lua
            local sub = require("mylib.sub")
            assert(sub.sub_init == true, "expected sub_init=true")
          end,
        }
      "#,
    )
    .unwrap();

    evaluate_config(&config_path)?;
    Ok(())
  }

  #[test]
  fn test_require_input_has_dir_injection() -> Result<(), EvalError> {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path();

    // Create a local input directory with init.lua that returns sys.dir
    let local_input = config_dir.join("my-lib");
    fs::create_dir(&local_input).unwrap();
    fs::write(local_input.join("init.lua"), "return { dir = sys.dir }").unwrap();

    let config_path = config_dir.join("init.lua");
    fs::write(
      &config_path,
      r#"
        return {
          inputs = {
            mylib = "path:./my-lib",
          },
          setup = function(inputs)
            -- require("mylib") should have sys.dir set correctly
            local lib = require("mylib")
            assert(lib.dir, "sys.dir should be set")
            -- sys.dir should end with "my-lib" (the input directory)
            assert(lib.dir:match("my%-lib$"), "sys.dir should end with 'my-lib', got: " .. lib.dir)
          end,
        }
      "#,
    )
    .unwrap();

    evaluate_config(&config_path)?;
    Ok(())
  }

  #[test]
  fn test_require_input_submodule_has_dir_injection() -> Result<(), EvalError> {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path();

    // Create a local input directory with sub/module.lua that returns sys.dir
    let local_input = config_dir.join("my-lib");
    fs::create_dir_all(local_input.join("sub")).unwrap();
    fs::write(local_input.join("init.lua"), "return {}").unwrap();
    fs::write(local_input.join("sub").join("module.lua"), "return { dir = sys.dir }").unwrap();

    let config_path = config_dir.join("init.lua");
    fs::write(
      &config_path,
      r#"
        return {
          inputs = {
            mylib = "path:./my-lib",
          },
          setup = function(inputs)
            -- require("mylib.sub.module") should have sys.dir set to the sub directory
            local mod = require("mylib.sub.module")
            assert(mod.dir, "sys.dir should be set")
            -- sys.dir should end with "sub" (the module's parent directory)
            assert(mod.dir:match("sub$"), "sys.dir should end with 'sub', got: " .. mod.dir)
          end,
        }
      "#,
    )
    .unwrap();

    evaluate_config(&config_path)?;
    Ok(())
  }

  #[test]
  fn test_require_input_lua_subdir_file() -> Result<(), EvalError> {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path();

    // Create a local input directory with lua/submodule.lua (LuaRocks-style)
    let local_input = config_dir.join("my-lib");
    fs::create_dir_all(local_input.join("lua")).unwrap();
    fs::write(local_input.join("init.lua"), "return { name = 'my-lib' }").unwrap();
    fs::write(
      local_input.join("lua").join("submodule.lua"),
      "return { lua_subdir = true }",
    )
    .unwrap();

    let config_path = config_dir.join("init.lua");
    fs::write(
      &config_path,
      r#"
        return {
          inputs = {
            mylib = "path:./my-lib",
          },
          setup = function(inputs)
            -- require("mylib.submodule") should load lua/submodule.lua
            local sub = require("mylib.submodule")
            assert(sub.lua_subdir == true, "expected lua_subdir=true")
          end,
        }
      "#,
    )
    .unwrap();

    evaluate_config(&config_path)?;
    Ok(())
  }

  #[test]
  fn test_require_input_lua_subdir_init() -> Result<(), EvalError> {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path();

    // Create a local input directory with lua/submodule/init.lua (LuaRocks-style)
    let local_input = config_dir.join("my-lib");
    fs::create_dir_all(local_input.join("lua").join("submodule")).unwrap();
    fs::write(local_input.join("init.lua"), "return { name = 'my-lib' }").unwrap();
    fs::write(
      local_input.join("lua").join("submodule").join("init.lua"),
      "return { lua_subdir_init = true }",
    )
    .unwrap();

    let config_path = config_dir.join("init.lua");
    fs::write(
      &config_path,
      r#"
        return {
          inputs = {
            mylib = "path:./my-lib",
          },
          setup = function(inputs)
            -- require("mylib.submodule") should load lua/submodule/init.lua
            local sub = require("mylib.submodule")
            assert(sub.lua_subdir_init == true, "expected lua_subdir_init=true")
          end,
        }
      "#,
    )
    .unwrap();

    evaluate_config(&config_path)?;
    Ok(())
  }

  #[test]
  fn test_require_input_lua_subdir_nested() -> Result<(), EvalError> {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path();

    // Create a local input directory with lua/foo/bar.lua (nested in lua/)
    let local_input = config_dir.join("my-lib");
    fs::create_dir_all(local_input.join("lua").join("foo")).unwrap();
    fs::write(local_input.join("init.lua"), "return { name = 'my-lib' }").unwrap();
    fs::write(
      local_input.join("lua").join("foo").join("bar.lua"),
      "return { nested = true }",
    )
    .unwrap();

    let config_path = config_dir.join("init.lua");
    fs::write(
      &config_path,
      r#"
        return {
          inputs = {
            mylib = "path:./my-lib",
          },
          setup = function(inputs)
            -- require("mylib.foo.bar") should load lua/foo/bar.lua
            local bar = require("mylib.foo.bar")
            assert(bar.nested == true, "expected nested=true")
          end,
        }
      "#,
    )
    .unwrap();

    evaluate_config(&config_path)?;
    Ok(())
  }

  #[test]
  fn test_require_input_lua_subdir_has_dir_injection() -> Result<(), EvalError> {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path();

    // Create a local input directory with lua/submodule.lua that returns sys.dir
    let local_input = config_dir.join("my-lib");
    fs::create_dir_all(local_input.join("lua")).unwrap();
    fs::write(local_input.join("init.lua"), "return {}").unwrap();
    fs::write(
      local_input.join("lua").join("submodule.lua"),
      "return { dir = sys.dir }",
    )
    .unwrap();

    let config_path = config_dir.join("init.lua");
    fs::write(
      &config_path,
      r#"
        return {
          inputs = {
            mylib = "path:./my-lib",
          },
          setup = function(inputs)
            -- require("mylib.submodule") should have sys.dir set to lua/
            local mod = require("mylib.submodule")
            assert(mod.dir, "sys.dir should be set")
            -- sys.dir should end with "lua" (the module's parent directory)
            assert(mod.dir:match("lua$"), "sys.dir should end with 'lua', got: " .. mod.dir)
          end,
        }
      "#,
    )
    .unwrap();

    evaluate_config(&config_path)?;
    Ok(())
  }

  #[test]
  fn test_require_input_root_takes_precedence_over_lua_subdir() -> Result<(), EvalError> {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path();

    // Create a local input directory with BOTH:
    // - submodule.lua (at root)
    // - lua/submodule.lua (in lua/)
    // Root should take precedence
    let local_input = config_dir.join("my-lib");
    fs::create_dir_all(local_input.join("lua")).unwrap();
    fs::write(local_input.join("init.lua"), "return { name = 'my-lib' }").unwrap();
    fs::write(local_input.join("submodule.lua"), "return { source = 'root' }").unwrap();
    fs::write(
      local_input.join("lua").join("submodule.lua"),
      "return { source = 'lua_subdir' }",
    )
    .unwrap();

    let config_path = config_dir.join("init.lua");
    fs::write(
      &config_path,
      r#"
        return {
          inputs = {
            mylib = "path:./my-lib",
          },
          setup = function(inputs)
            -- require("mylib.submodule") should load root submodule.lua, not lua/submodule.lua
            local sub = require("mylib.submodule")
            assert(sub.source == "root", "expected source='root', got: " .. sub.source)
          end,
        }
      "#,
    )
    .unwrap();

    evaluate_config(&config_path)?;
    Ok(())
  }

  #[test]
  fn test_require_non_input_falls_through() -> Result<(), EvalError> {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path();

    // Create an input
    let local_input = config_dir.join("my-lib");
    fs::create_dir(&local_input).unwrap();
    fs::write(local_input.join("init.lua"), "return { name = 'my-lib' }").unwrap();

    let config_path = config_dir.join("init.lua");
    fs::write(
      &config_path,
      r#"
        return {
          inputs = {
            mylib = "path:./my-lib",
          },
          setup = function(inputs)
            -- require("mylib") should work via input searcher
            local lib = require("mylib")
            assert(lib.name == "my-lib", "input should load")
            
            -- require("nonexistent") should fail with proper error message
            -- that includes both "no input" and "no file" messages
            local ok, err = pcall(function() require("nonexistent") end)
            assert(not ok, "require('nonexistent') should fail")
            assert(err:match("no input 'nonexistent'"), "error should mention 'no input', got: " .. err)
          end,
        }
      "#,
    )
    .unwrap();

    evaluate_config(&config_path)?;
    Ok(())
  }

  #[test]
  fn test_extended_input_syntax_with_url() -> Result<(), EvalError> {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path();

    // Create a local input directory
    let local_input = config_dir.join("my-lib");
    fs::create_dir(&local_input).unwrap();
    fs::write(local_input.join("init.lua"), "return { name = 'my-lib' }").unwrap();

    let config_path = config_dir.join("init.lua");
    fs::write(
      &config_path,
      r#"
        return {
          inputs = {
            -- Extended syntax with just url (no overrides)
            mylib = {
              url = "path:./my-lib",
            },
          },
          setup = function(inputs)
            local lib = require("mylib")
            assert(lib.name == "my-lib", "expected name='my-lib', got " .. tostring(lib.name))
          end,
        }
      "#,
    )
    .unwrap();

    evaluate_config(&config_path)?;
    Ok(())
  }

  #[test]
  fn test_extended_input_syntax_with_overrides_is_parsed() -> Result<(), EvalError> {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path();

    // Create two local inputs
    let utils_dir = config_dir.join("utils");
    fs::create_dir(&utils_dir).unwrap();
    fs::write(utils_dir.join("init.lua"), "return { name = 'utils' }").unwrap();

    let pkgs_dir = config_dir.join("pkgs");
    fs::create_dir(&pkgs_dir).unwrap();
    fs::write(pkgs_dir.join("init.lua"), "return { name = 'pkgs' }").unwrap();

    let config_path = config_dir.join("init.lua");
    fs::write(
      &config_path,
      r#"
        return {
          inputs = {
            utils = "path:./utils",
            -- Extended syntax with overrides (currently parsed but not fully resolved)
            pkgs = {
              url = "path:./pkgs",
              inputs = {
                utils = { follows = "utils" },
              },
            },
          },
          setup = function(inputs)
            -- Both inputs should be resolved
            assert(inputs.utils, "utils should be resolved")
            assert(inputs.pkgs, "pkgs should be resolved")
          end,
        }
      "#,
    )
    .unwrap();

    // This should work even though follows isn't fully implemented yet
    // (we're just testing that the parsing works)
    evaluate_config(&config_path)?;
    Ok(())
  }
}
