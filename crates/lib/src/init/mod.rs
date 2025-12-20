//! Initialize a new syslua configuration directory.
//!
//! This module provides the core logic for the `sys init` command, which
//! scaffolds a new configuration directory with:
//! - `init.lua` entry point with examples
//! - `.luarc.json` for LuaLS IDE integration
//! - Store structure and type definitions

mod templates;

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;
use tracing::warn;

use crate::platform::paths::{cache_dir, data_dir, root_dir};

pub use templates::{GLOBALS_D_LUA, INIT_LUA_TEMPLATE, LUARC_JSON_TEMPLATE};

/// Errors that can occur during initialization.
#[derive(Debug, Error)]
pub enum InitError {
  #[error("file already exists: {}", path.display())]
  PathExists { path: PathBuf },

  #[error("failed to create directory {}: {source}", path.display())]
  CreateDir { path: PathBuf, source: std::io::Error },

  #[error("failed to write file {}: {source}", path.display())]
  WriteFile { path: PathBuf, source: std::io::Error },

  #[error("failed to canonicalize path {}: {source}", path.display())]
  Canonicalize { path: PathBuf, source: std::io::Error },
}

/// Options for initializing a configuration directory.
pub struct InitOptions {
  /// Path to the configuration directory to create
  pub config_path: PathBuf,
  /// Whether running as elevated (affects store location)
  pub system: bool,
}

/// Result of a successful initialization.
#[derive(Debug)]
pub struct InitResult {
  /// The configuration directory (canonicalized)
  pub config_dir: PathBuf,
  /// Path to created init.lua
  pub init_lua: PathBuf,
  /// Path to created .luarc.json
  pub luarc_json: PathBuf,
  /// Path to types directory
  pub types_dir: PathBuf,
  /// Path to store directory
  pub store_dir: PathBuf,
}

/// Initialize a new syslua configuration directory.
///
/// Creates the configuration directory structure with template files,
/// and sets up the store and type definitions for LuaLS integration.
///
/// # Errors
///
/// Returns an error if:
/// - `init.lua` or `.luarc.json` already exist
/// - Directory creation fails
/// - File writing fails
pub fn init(options: &InitOptions) -> Result<InitResult, InitError> {
  let config_dir = &options.config_path;

  // Create config directory if it doesn't exist (needed for canonicalize)
  fs::create_dir_all(config_dir).map_err(|e| InitError::CreateDir {
    path: config_dir.clone(),
    source: e,
  })?;

  // Canonicalize for consistent paths
  let config_dir = config_dir.canonicalize().map_err(|e| InitError::Canonicalize {
    path: options.config_path.clone(),
    source: e,
  })?;

  let init_lua = config_dir.join("init.lua");
  let luarc_json = config_dir.join(".luarc.json");

  // Check for existing files
  if init_lua.exists() {
    return Err(InitError::PathExists { path: init_lua });
  }
  if luarc_json.exists() {
    return Err(InitError::PathExists { path: luarc_json });
  }

  // Determine base directory for store/types
  let base_dir = if options.system { root_dir() } else { data_dir() };

  let store_dir = base_dir.join("store");
  let types_dir = base_dir.join("types");
  let snapshots_dir = base_dir.join("snapshots");

  // Create store structure
  fs::create_dir_all(store_dir.join("build")).map_err(|e| InitError::CreateDir {
    path: store_dir.join("build"),
    source: e,
  })?;
  fs::create_dir_all(store_dir.join("bind")).map_err(|e| InitError::CreateDir {
    path: store_dir.join("bind"),
    source: e,
  })?;
  fs::create_dir_all(&types_dir).map_err(|e| InitError::CreateDir {
    path: types_dir.clone(),
    source: e,
  })?;
  fs::create_dir_all(&snapshots_dir).map_err(|e| InitError::CreateDir {
    path: snapshots_dir.clone(),
    source: e,
  })?;

  // Write init.lua
  fs::write(&init_lua, INIT_LUA_TEMPLATE).map_err(|e| InitError::WriteFile {
    path: init_lua.clone(),
    source: e,
  })?;

  // Write .luarc.json with types path substituted
  let types_path_str = types_dir.to_string_lossy();
  let luarc_content = LUARC_JSON_TEMPLATE.replace("{types_path}", &types_path_str);
  fs::write(&luarc_json, luarc_content).map_err(|e| InitError::WriteFile {
    path: luarc_json.clone(),
    source: e,
  })?;

  // Write globals.d.lua to types directory
  let globals_path = types_dir.join("globals.d.lua");
  fs::write(&globals_path, GLOBALS_D_LUA).map_err(|e| InitError::WriteFile {
    path: globals_path,
    source: e,
  })?;

  Ok(InitResult {
    config_dir,
    init_lua,
    luarc_json,
    types_dir,
    store_dir,
  })
}

/// Update .luarc.json with resolved input paths for LuaLS integration.
///
/// Preserves user-added library entries while adding/updating syslua-managed paths.
/// If .luarc.json doesn't exist or is malformed, logs a warning and skips.
///
/// Syslua-managed entries are identified by paths starting with:
/// - The types directory (e.g., `~/.local/share/syslua/types`)
/// - The inputs cache directory (e.g., `~/.cache/syslua/inputs`)
///
/// # Arguments
///
/// * `config_dir` - Directory containing .luarc.json
/// * `input_paths` - Iterator of input paths to add to library
/// * `system` - Whether running as elevated (affects types/cache paths)
pub fn update_luarc_inputs<'a, I>(config_dir: &Path, input_paths: I, system: bool)
where
  I: IntoIterator<Item = &'a Path>,
{
  let luarc_path = config_dir.join(".luarc.json");

  // Skip if .luarc.json doesn't exist
  if !luarc_path.exists() {
    return;
  }

  // Read existing content
  let content = match fs::read_to_string(&luarc_path) {
    Ok(c) => c,
    Err(e) => {
      warn!(
        path = %luarc_path.display(),
        error = %e,
        "failed to read .luarc.json, skipping update"
      );
      return;
    }
  };

  // Parse JSON
  let mut luarc: serde_json::Value = match serde_json::from_str(&content) {
    Ok(v) => v,
    Err(e) => {
      warn!(
        path = %luarc_path.display(),
        error = %e,
        "failed to parse .luarc.json, skipping update"
      );
      return;
    }
  };

  // Determine managed path prefixes
  let base_dir = if system { root_dir() } else { data_dir() };
  let types_path = base_dir.join("types");
  let inputs_cache_dir = cache_dir().join("inputs");

  let types_prefix = types_path.to_string_lossy().to_string();
  let cache_prefix = inputs_cache_dir.to_string_lossy().to_string();

  // Get current library entries, filter out syslua-managed ones
  let current_library = luarc
    .get("workspace")
    .and_then(|w| w.get("library"))
    .and_then(|l| l.as_array())
    .cloned()
    .unwrap_or_default();

  let user_entries: Vec<serde_json::Value> = current_library
    .into_iter()
    .filter(|entry| {
      if let Some(path) = entry.as_str() {
        !path.starts_with(&types_prefix) && !path.starts_with(&cache_prefix)
      } else {
        true // Keep non-string entries as-is
      }
    })
    .collect();

  // Build new library array: types first, then inputs, then user entries
  let mut new_library: Vec<serde_json::Value> = vec![serde_json::Value::String(types_prefix)];

  // Add each input's path
  for input_path in input_paths {
    new_library.push(serde_json::Value::String(input_path.to_string_lossy().to_string()));
  }

  // Append user entries
  new_library.extend(user_entries);

  // Update workspace.library (create workspace if missing)
  if let Some(obj) = luarc.as_object_mut() {
    if let Some(workspace) = obj.get_mut("workspace") {
      if let Some(ws_obj) = workspace.as_object_mut() {
        ws_obj.insert("library".to_string(), serde_json::Value::Array(new_library));
      }
    } else {
      // workspace doesn't exist, create it
      let mut workspace = serde_json::Map::new();
      workspace.insert("library".to_string(), serde_json::Value::Array(new_library));
      obj.insert("workspace".to_string(), serde_json::Value::Object(workspace));
    }
  }

  // Write back with pretty formatting
  let new_content = match serde_json::to_string_pretty(&luarc) {
    Ok(c) => c,
    Err(e) => {
      warn!(
        path = %luarc_path.display(),
        error = %e,
        "failed to serialize .luarc.json, skipping update"
      );
      return;
    }
  };

  if let Err(e) = fs::write(&luarc_path, new_content) {
    warn!(
      path = %luarc_path.display(),
      error = %e,
      "failed to write .luarc.json"
    );
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serial_test::serial;
  use tempfile::TempDir;

  #[test]
  #[serial]
  fn init_creates_all_files() {
    let temp = TempDir::new().unwrap();
    let config_dir = temp.path().join("config");
    let data_dir = temp.path().join("data");

    temp_env::with_vars(
      [
        ("XDG_DATA_HOME", Some(data_dir.to_str().unwrap())),
        ("HOME", Some(temp.path().to_str().unwrap())),
      ],
      || {
        let options = InitOptions {
          config_path: config_dir.clone(),
          system: false,
        };

        let result = init(&options).unwrap();

        // Verify config files exist
        assert!(result.init_lua.exists(), "init.lua should exist");
        assert!(result.luarc_json.exists(), ".luarc.json should exist");

        // Verify store structure exists
        assert!(result.store_dir.join("build").exists(), "store/build should exist");
        assert!(result.store_dir.join("bind").exists(), "store/bind should exist");
        assert!(result.types_dir.exists(), "types dir should exist");
        assert!(
          result.types_dir.join("globals.d.lua").exists(),
          "globals.d.lua should exist"
        );
      },
    );
  }

  #[test]
  #[serial]
  fn init_fails_if_init_lua_exists() {
    let temp = TempDir::new().unwrap();
    let config_dir = temp.path().join("config");
    let data_dir = temp.path().join("data");

    // Create existing init.lua
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(config_dir.join("init.lua"), "-- existing").unwrap();

    temp_env::with_vars(
      [
        ("XDG_DATA_HOME", Some(data_dir.to_str().unwrap())),
        ("HOME", Some(temp.path().to_str().unwrap())),
      ],
      || {
        let options = InitOptions {
          config_path: config_dir.clone(),
          system: false,
        };

        let result = init(&options);
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(matches!(err, InitError::PathExists { .. }));
        assert!(err.to_string().contains("init.lua"));
      },
    );
  }

  #[test]
  #[serial]
  fn init_fails_if_luarc_exists() {
    let temp = TempDir::new().unwrap();
    let config_dir = temp.path().join("config");
    let data_dir = temp.path().join("data");

    // Create existing .luarc.json
    fs::create_dir_all(&config_dir).unwrap();
    fs::write(config_dir.join(".luarc.json"), "{}").unwrap();

    temp_env::with_vars(
      [
        ("XDG_DATA_HOME", Some(data_dir.to_str().unwrap())),
        ("HOME", Some(temp.path().to_str().unwrap())),
      ],
      || {
        let options = InitOptions {
          config_path: config_dir.clone(),
          system: false,
        };

        let result = init(&options);
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(matches!(err, InitError::PathExists { .. }));
        assert!(err.to_string().contains(".luarc.json"));
      },
    );
  }

  #[test]
  #[serial]
  fn init_luarc_contains_correct_types_path() {
    let temp = TempDir::new().unwrap();
    let config_dir = temp.path().join("config");
    let data_dir = temp.path().join("data");

    temp_env::with_vars(
      [
        ("XDG_DATA_HOME", Some(data_dir.to_str().unwrap())),
        ("HOME", Some(temp.path().to_str().unwrap())),
      ],
      || {
        let options = InitOptions {
          config_path: config_dir.clone(),
          system: false,
        };

        let result = init(&options).unwrap();

        // Read .luarc.json and verify it contains the types path
        let luarc_content = fs::read_to_string(&result.luarc_json).unwrap();
        let types_path_str = result.types_dir.to_string_lossy();

        assert!(
          luarc_content.contains(&*types_path_str),
          ".luarc.json should contain types path: {}",
          types_path_str
        );
        assert!(
          !luarc_content.contains("{types_path}"),
          "placeholder should be replaced"
        );
      },
    );
  }

  #[test]
  #[serial]
  fn init_creates_parent_directories() {
    let temp = TempDir::new().unwrap();
    let config_dir = temp.path().join("nested").join("path").join("config");
    let data_dir = temp.path().join("data");

    temp_env::with_vars(
      [
        ("XDG_DATA_HOME", Some(data_dir.to_str().unwrap())),
        ("HOME", Some(temp.path().to_str().unwrap())),
      ],
      || {
        let options = InitOptions {
          config_path: config_dir.clone(),
          system: false,
        };

        let result = init(&options).unwrap();

        assert!(result.init_lua.exists(), "init.lua should exist in nested path");
      },
    );
  }
}
