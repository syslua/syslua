//! Configuration file evaluation.
//!
//! This module provides the `evaluate_config` function which takes a path to a
//! Lua configuration file and returns the resulting `Manifest` containing all
//! builds and bindings defined in the configuration.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use mlua::prelude::*;
use tracing::{debug, info};

use crate::init::update_luarc_inputs;
use crate::inputs::resolve::{ResolveError, resolve_inputs, save_lock_file_if_changed};
use crate::inputs::{InputDecl, InputDecls, InputOverride, ResolvedInput, ResolvedInputs};
use crate::lua::{loaders, runtime};
use crate::manifest::Manifest;
use crate::platform;

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
/// 4. Builds package.path from all inputs' `lua/` directories
/// 5. Calls each input's `setup(inputs)` function in dependency order
/// 6. Calls the root config's `setup(inputs)` function last
/// 7. Returns the manifest containing all registered builds and bindings
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

        Some(result.inputs)
      };

      // Build and set package.path from all lua/ directories
      if let Some(ref inputs) = resolved {
        let package_path = build_package_path(config_dir, inputs);
        set_package_path(&lua, &package_path)?;

        // Call input setup() functions in dependency order
        call_input_setups(&lua, inputs)?;
      }

      // Build Lua inputs table for setup()
      let inputs_table = build_inputs_table(&lua, resolved.as_ref())?;

      // Call root config's setup(inputs) last
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

/// Build package.path from all lua/ directories.
///
/// Constructs a package.path string that includes:
/// 1. Config directory's lua/ (if exists) - highest priority
/// 2. All inputs' lua/ directories in declaration order
///
/// # Arguments
/// * `config_dir` - Directory containing the config file
/// * `resolved` - Map of resolved inputs
///
/// # Returns
/// A semicolon-separated package.path string
fn build_package_path(config_dir: &Path, resolved: &ResolvedInputs) -> String {
  let mut paths = Vec::new();

  // 1. Config directory's lua/ (if exists) - highest priority
  let config_lua_dir = config_dir.join("lua");
  if config_lua_dir.is_dir() {
    let lua_dir_str = config_lua_dir.to_string_lossy().replace("\\", "/");
    paths.push(format!("{}/?.lua", lua_dir_str));
    paths.push(format!("{}/?/init.lua", lua_dir_str));
  }

  // 2. Collect all lua/ paths from resolved inputs (including transitive)
  collect_lua_paths(resolved, &mut paths);

  paths.join(";")
}

/// Recursively collect lua/ paths from resolved inputs.
fn collect_lua_paths(inputs: &ResolvedInputs, paths: &mut Vec<String>) {
  for input in inputs.values() {
    let lua_dir = input.path.join("lua");
    if lua_dir.is_dir() {
      let lua_dir_str = lua_dir.to_string_lossy().replace("\\", "/");
      paths.push(format!("{}/?.lua", lua_dir_str));
      paths.push(format!("{}/?/init.lua", lua_dir_str));
    }

    // Recursively add transitive dependencies' lua/ paths
    if !input.inputs.is_empty() {
      collect_lua_paths(&input.inputs, paths);
    }
  }
}

/// Set package.path in the Lua runtime.
///
/// Prepends the new paths to the existing package.path.
fn set_package_path(lua: &Lua, new_paths: &str) -> LuaResult<()> {
  if new_paths.is_empty() {
    return Ok(());
  }

  let package: LuaTable = lua.globals().get("package")?;
  let current_path: String = package.get("path")?;

  let combined_path = format!("{};{}", new_paths, current_path);
  package.set("path", combined_path.as_str())?;

  debug!(package_path = %new_paths, "set package.path");
  Ok(())
}

/// Call setup() functions for all inputs in dependency order.
///
/// Walks the input tree depth-first, calling each input's setup() function
/// after its dependencies have been set up. This ensures that libraries can
/// rely on their dependencies being initialized before their own setup runs.
fn call_input_setups(lua: &Lua, resolved: &ResolvedInputs) -> LuaResult<()> {
  for (name, input) in resolved {
    call_input_setup_recursive(lua, name, input)?;
  }
  Ok(())
}

/// Recursively call setup() for an input and its dependencies.
fn call_input_setup_recursive(lua: &Lua, name: &str, input: &ResolvedInput) -> LuaResult<()> {
  // First, recursively call setup for transitive dependencies
  for (dep_name, dep_input) in &input.inputs {
    call_input_setup_recursive(lua, dep_name, dep_input)?;
  }

  // Then call this input's setup() if it has one
  let init_path = input.path.join("init.lua");
  if init_path.exists() {
    // Load the input's init.lua
    let init_result = loaders::load_file_with_dir(lua, &init_path)?;

    if let LuaValue::Table(init_table) = init_result {
      // Check if it has a setup function
      let setup_value: LuaValue = init_table.get("setup")?;
      if let LuaValue::Function(setup_fn) = setup_value {
        // Build inputs table for this input's dependencies
        let inputs_table = build_inputs_table(lua, Some(&input.inputs))?;

        debug!(input = name, "calling input setup()");
        setup_fn.call::<()>(inputs_table)?;
      }
    }
  }

  Ok(())
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
fn build_input_entry(lua: &Lua, input: &ResolvedInput) -> LuaResult<LuaTable> {
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
  fn test_require_from_input_lua_dir() -> Result<(), EvalError> {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path();

    // Create a local input with lua/<namespace>/ structure
    let local_input = config_dir.join("my-lib");
    let lua_dir = local_input.join("lua").join("mylib");
    fs::create_dir_all(&lua_dir).unwrap();
    fs::write(local_input.join("init.lua"), "return {}").unwrap();
    fs::write(lua_dir.join("init.lua"), "return { name = 'mylib', version = '1.0.0' }").unwrap();

    let config_path = config_dir.join("init.lua");
    fs::write(
      &config_path,
      r#"
        return {
          inputs = {
            mylib = "path:./my-lib",
          },
          setup = function(inputs)
            -- require("mylib") should load from lua/mylib/init.lua via package.path
            local lib = require("mylib")
            assert(lib.name == "mylib", "expected name='mylib', got " .. tostring(lib.name))
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
  fn test_require_submodule_from_lua_dir() -> Result<(), EvalError> {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path();

    // Create a local input with lua/<namespace>/ structure and submodule
    let local_input = config_dir.join("my-lib");
    let lua_dir = local_input.join("lua").join("mylib");
    fs::create_dir_all(&lua_dir).unwrap();
    fs::write(local_input.join("init.lua"), "return {}").unwrap();
    fs::write(lua_dir.join("init.lua"), "return { name = 'mylib' }").unwrap();
    fs::write(lua_dir.join("utils.lua"), "return { helper = 'works' }").unwrap();

    let config_path = config_dir.join("init.lua");
    fs::write(
      &config_path,
      r#"
        return {
          inputs = {
            mylib = "path:./my-lib",
          },
          setup = function(inputs)
            -- require("mylib.utils") should load from lua/mylib/utils.lua via package.path
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
  fn test_config_lua_dir_conflicts_with_input() -> Result<(), EvalError> {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path();

    // Create config's own lua/ directory with a module
    let config_lua_dir = config_dir.join("lua").join("mymod");
    fs::create_dir_all(&config_lua_dir).unwrap();
    fs::write(config_lua_dir.join("init.lua"), "return { source = 'config' }").unwrap();

    // Create an input with the same module name in its lua/ directory
    let local_input = config_dir.join("my-lib");
    let input_lua_dir = local_input.join("lua").join("mymod");
    fs::create_dir_all(&input_lua_dir).unwrap();
    fs::write(local_input.join("init.lua"), "return {}").unwrap();
    fs::write(input_lua_dir.join("init.lua"), "return { source = 'input' }").unwrap();

    let config_path = config_dir.join("init.lua");
    fs::write(
      &config_path,
      r#"
        return {
          inputs = {
            mylib = "path:./my-lib",
          },
          setup = function(inputs)
            -- This should not be reached due to namespace conflict
            local mod = require("mymod")
          end,
        }
      "#,
    )
    .unwrap();

    // Should fail with namespace conflict
    let result = evaluate_config(&config_path);
    assert!(result.is_err(), "expected namespace conflict error");

    let err_msg = result.unwrap_err().to_string();
    assert!(
      err_msg.contains("namespace conflict") || err_msg.contains("NamespaceConflict"),
      "expected namespace conflict error, got: {}",
      err_msg
    );

    Ok(())
  }

  #[test]
  fn test_input_setup_is_called() -> Result<(), EvalError> {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path();

    // Create a local input with a setup function that sets a global
    let local_input = config_dir.join("my-lib");
    fs::create_dir(&local_input).unwrap();
    fs::write(
      local_input.join("init.lua"),
      r#"
        return {
          setup = function(inputs)
            -- Set a global to prove setup was called
            _G.MY_LIB_SETUP_CALLED = true
          end,
        }
      "#,
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
            -- Verify input's setup was called before our setup
            assert(_G.MY_LIB_SETUP_CALLED == true, "input setup should have been called")
          end,
        }
      "#,
    )
    .unwrap();

    evaluate_config(&config_path)?;
    Ok(())
  }

  #[test]
  fn test_transitive_input_setup_order() -> Result<(), EvalError> {
    let temp_dir = TempDir::new().unwrap();
    let config_dir = temp_dir.path();

    // Create utils (no deps)
    let utils_dir = config_dir.join("utils");
    fs::create_dir(&utils_dir).unwrap();
    fs::write(
      utils_dir.join("init.lua"),
      r#"
        return {
          setup = function(inputs)
            _G.SETUP_ORDER = _G.SETUP_ORDER or {}
            table.insert(_G.SETUP_ORDER, "utils")
          end,
        }
      "#,
    )
    .unwrap();

    // Create lib_a which depends on utils
    let lib_a_dir = config_dir.join("lib_a");
    fs::create_dir(&lib_a_dir).unwrap();

    // Need to escape the path for Lua
    let utils_path = utils_dir.canonicalize().unwrap();
    let utils_path_str = utils_path.to_string_lossy().replace('\\', "/");
    fs::write(
      lib_a_dir.join("init.lua"),
      format!(
        r#"
        return {{
          inputs = {{
            utils = "path:{}",
          }},
          setup = function(inputs)
            _G.SETUP_ORDER = _G.SETUP_ORDER or {{}}
            table.insert(_G.SETUP_ORDER, "lib_a")
          end,
        }}
      "#,
        utils_path_str
      ),
    )
    .unwrap();

    let config_path = config_dir.join("init.lua");
    fs::write(
      &config_path,
      r#"
        return {
          inputs = {
            lib_a = "path:./lib_a",
          },
          setup = function(inputs)
            _G.SETUP_ORDER = _G.SETUP_ORDER or {}
            table.insert(_G.SETUP_ORDER, "config")
            
            -- Verify order: utils first (dep), then lib_a, then config
            assert(#_G.SETUP_ORDER == 3, "expected 3 setups, got " .. #_G.SETUP_ORDER)
            assert(_G.SETUP_ORDER[1] == "utils", "expected utils first, got " .. tostring(_G.SETUP_ORDER[1]))
            assert(_G.SETUP_ORDER[2] == "lib_a", "expected lib_a second, got " .. tostring(_G.SETUP_ORDER[2]))
            assert(_G.SETUP_ORDER[3] == "config", "expected config third, got " .. tostring(_G.SETUP_ORDER[3]))
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
    let lua_dir = local_input.join("lua").join("mylib");
    fs::create_dir_all(&lua_dir).unwrap();
    fs::write(local_input.join("init.lua"), "return {}").unwrap();
    fs::write(lua_dir.join("init.lua"), "return { name = 'my-lib' }").unwrap();

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
