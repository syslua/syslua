//! Configuration file evaluation.
//!
//! This module provides the `evaluate_config` function which takes a path to a
//! Lua configuration file and returns the resulting `Manifest` containing all
//! builds and bindings defined in the configuration.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use mlua::prelude::*;
use tracing::info;

use crate::inputs::resolve::{ResolveError, ResolvedInputs, resolve_inputs, save_lock_file_if_changed};
use crate::lua::{loaders, runtime};
use crate::manifest::Manifest;

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

      // Extract raw inputs table (name -> url)
      let raw_inputs = extract_raw_inputs(&config_table)?;

      // Resolve inputs (fetch git repos, resolve paths)
      let resolved = if raw_inputs.is_empty() {
        info!("no inputs to resolve");
        None
      } else {
        info!(count = raw_inputs.len(), "resolving inputs");
        let result = resolve_inputs(&raw_inputs, config_dir)?;

        // Save lock file if it changed
        save_lock_file_if_changed(&result, config_dir)?;

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
/// Returns a map of input name -> URL string.
fn extract_raw_inputs(config_table: &LuaTable) -> LuaResult<HashMap<String, String>> {
  let mut inputs = HashMap::new();

  let inputs_value: LuaValue = config_table.get("inputs")?;
  if let LuaValue::Table(inputs_table) = inputs_value {
    for pair in inputs_table.pairs::<String, String>() {
      let (name, url) = pair?;
      inputs.insert(name, url);
    }
  }
  // If inputs is nil or not a table, return empty map (no inputs)

  Ok(inputs)
}

/// Build a Lua table representing resolved inputs for setup().
///
/// Each input becomes: `inputs.name = { path = "/path/to/input", rev = "abc123" }`
fn build_inputs_table(lua: &Lua, resolved: Option<&ResolvedInputs>) -> LuaResult<LuaTable> {
  let inputs = lua.create_table()?;

  if let Some(resolved_inputs) = resolved {
    for (name, input) in resolved_inputs {
      let entry = lua.create_table()?;
      entry.set("path", input.path.to_string_lossy().as_ref())?;
      entry.set("rev", input.rev.as_str())?;
      inputs.set(name.as_str(), entry)?;
    }
  }

  Ok(inputs)
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
              name = "test",
              version = "1.0.0",
              apply = function(build_inputs, ctx)
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
    assert_eq!(build.name, "test");
    assert_eq!(build.version.as_deref(), Some("1.0.0"));
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
              apply = function(bind_inputs, ctx)
                ctx:cmd({ cmd = "echo test" })
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
              name = "test",
              version = "1.0.0",
              apply = function(build_inputs, ctx)
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
}
