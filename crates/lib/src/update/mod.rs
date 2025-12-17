//! Input update orchestration.
//!
//! This module provides the core logic for the `sys update` command, which
//! re-resolves inputs (fetching latest revisions) and updates the lock file.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use thiserror::Error;
use tracing::info;

use crate::init::update_luarc_inputs;
use crate::inputs::lock::{LOCK_FILENAME, LockFile};
use crate::inputs::resolve::{
  ResolutionResult, ResolveError, ResolvedInputs, resolve_inputs, save_lock_file_if_changed,
};
use crate::lua::entrypoint::extract_inputs;
use crate::platform::paths::config_dir;

/// Options for the update operation.
#[derive(Debug, Default)]
pub struct UpdateOptions {
  /// Specific inputs to update. If empty, all inputs are updated.
  pub inputs: Vec<String>,
  /// If true, don't write lock file or update .luarc.json.
  pub dry_run: bool,
  /// Whether running as elevated (affects .luarc.json paths).
  pub system: bool,
}

/// Result of a successful update operation.
#[derive(Debug)]
pub struct UpdateResult {
  /// Inputs that were updated: name -> (old_rev, new_rev).
  pub updated: BTreeMap<String, (String, String)>,
  /// Inputs that remained unchanged.
  pub unchanged: Vec<String>,
  /// New inputs that were added (not previously locked).
  pub added: Vec<String>,
  /// Resolved inputs with their final paths and revisions.
  pub resolved: ResolvedInputs,
  /// Whether the lock file changed.
  pub lock_changed: bool,
}

/// Errors that can occur during update.
#[derive(Debug, Error)]
pub enum UpdateError {
  /// Config file not found.
  #[error("config file not found: {path}")]
  ConfigNotFound { path: String },

  /// Failed to extract inputs from config.
  #[error("failed to extract inputs from config: {0}")]
  ExtractInputs(#[from] mlua::Error),

  /// Failed to resolve inputs.
  #[error("failed to resolve inputs: {0}")]
  Resolve(#[from] ResolveError),

  /// Failed to load lock file.
  #[error("failed to load lock file: {0}")]
  LoadLock(#[source] crate::inputs::lock::LockError),

  /// Specified input not found in config.
  #[error("input '{name}' not found in config")]
  InputNotFound { name: String },
}

/// Find the config file path, with fallback resolution.
///
/// Priority order:
/// 1. Explicit path if provided and exists
/// 2. `./init.lua` in current directory
/// 3. `~/.config/syslua/init.lua` (user config dir)
///
/// # Errors
///
/// Returns `UpdateError::ConfigNotFound` if no config file can be found.
pub fn find_config_path(explicit: Option<&str>) -> Result<PathBuf, UpdateError> {
  // 1. Explicit path
  if let Some(path) = explicit {
    let p = PathBuf::from(path);
    if p.exists() {
      return Ok(p);
    }
    return Err(UpdateError::ConfigNotFound { path: path.to_string() });
  }

  // 2. ./init.lua in current directory
  let cwd_config = PathBuf::from("./init.lua");
  if cwd_config.exists() {
    return Ok(cwd_config);
  }

  // 3. ~/.config/syslua/init.lua
  let user_config = config_dir().join("init.lua");
  if user_config.exists() {
    return Ok(user_config);
  }

  Err(UpdateError::ConfigNotFound {
    path: "init.lua (tried ./init.lua and ~/.config/syslua/init.lua)".to_string(),
  })
}

/// Update inputs by re-resolving them (fetching latest revisions).
///
/// # Arguments
///
/// * `config_path` - Path to the config file
/// * `options` - Update options (which inputs to update, dry run, etc.)
///
/// # Returns
///
/// An `UpdateResult` containing information about what changed.
///
/// # Errors
///
/// Returns an error if:
/// - Config file cannot be parsed
/// - A specified input doesn't exist in the config
/// - Input resolution fails
pub fn update_inputs(config_path: &Path, options: &UpdateOptions) -> Result<UpdateResult, UpdateError> {
  let config_dir = config_path.parent().unwrap_or(Path::new("."));
  let config_path_str = config_path.to_string_lossy();

  info!(config = %config_path_str, "loading config for update");

  // Extract inputs from config
  let raw_inputs = extract_inputs(&config_path_str)?;

  // Validate that requested inputs exist in config
  for input_name in &options.inputs {
    if !raw_inputs.contains_key(input_name) {
      return Err(UpdateError::InputNotFound {
        name: input_name.clone(),
      });
    }
  }

  // Load existing lock file to compare revisions
  let lock_path = config_dir.join(LOCK_FILENAME);
  let old_lock = LockFile::load(&lock_path)
    .map_err(UpdateError::LoadLock)?
    .unwrap_or_default();

  // Build force_update set
  let force_update: HashSet<String> = options.inputs.iter().cloned().collect();

  info!(
    count = raw_inputs.len(),
    force_count = force_update.len(),
    "resolving inputs"
  );

  // Resolve inputs with force update
  let result: ResolutionResult = resolve_inputs(&raw_inputs, config_dir, Some(&force_update))?;

  // Compute what changed
  let mut updated = BTreeMap::new();
  let mut unchanged = Vec::new();
  let mut added = Vec::new();

  for (name, resolved) in &result.inputs {
    if let Some(old_entry) = old_lock.get(name) {
      if old_entry.rev != resolved.rev {
        updated.insert(name.clone(), (old_entry.rev.clone(), resolved.rev.clone()));
      } else {
        unchanged.push(name.clone());
      }
    } else {
      added.push(name.clone());
    }
  }

  // Write lock file and update .luarc.json (unless dry run)
  if !options.dry_run {
    save_lock_file_if_changed(&result, config_dir)?;
    update_luarc_inputs(config_dir, &result.inputs, options.system);
  }

  Ok(UpdateResult {
    updated,
    unchanged,
    added,
    resolved: result.inputs,
    lock_changed: result.lock_changed,
  })
}

#[cfg(test)]
mod tests {
  use super::*;
  use serial_test::serial;
  use std::fs;
  use tempfile::TempDir;

  mod find_config_path_tests {
    use super::*;

    #[test]
    fn explicit_path_found() {
      let temp = TempDir::new().unwrap();
      let config_path = temp.path().join("my-config.lua");
      fs::write(&config_path, "return {}").unwrap();

      let result = find_config_path(Some(config_path.to_str().unwrap()));
      assert!(result.is_ok());
      assert_eq!(result.unwrap(), config_path);
    }

    #[test]
    fn explicit_path_not_found() {
      let result = find_config_path(Some("/nonexistent/path/config.lua"));
      assert!(result.is_err());
      assert!(matches!(result.unwrap_err(), UpdateError::ConfigNotFound { .. }));
    }

    #[test]
    #[serial]
    fn cwd_fallback() {
      let temp = TempDir::new().unwrap();
      let config_path = temp.path().join("init.lua");
      fs::write(&config_path, "return {}").unwrap();

      // Change to temp directory
      let original_dir = std::env::current_dir().unwrap();
      std::env::set_current_dir(temp.path()).unwrap();

      let result = find_config_path(None);

      // Restore original directory
      std::env::set_current_dir(original_dir).unwrap();

      assert!(result.is_ok());
    }

    #[test]
    #[serial]
    #[cfg(not(windows))]
    fn config_dir_fallback() {
      let temp = TempDir::new().unwrap();
      let syslua_config = temp.path().join("syslua");
      fs::create_dir_all(&syslua_config).unwrap();
      fs::write(syslua_config.join("init.lua"), "return {}").unwrap();

      // Use a directory without init.lua as cwd
      let cwd = temp.path().join("empty");
      fs::create_dir_all(&cwd).unwrap();
      let original_dir = std::env::current_dir().unwrap();
      std::env::set_current_dir(&cwd).unwrap();

      temp_env::with_vars(
        [
          ("XDG_CONFIG_HOME", Some(temp.path().to_str().unwrap())),
          ("HOME", Some(temp.path().to_str().unwrap())),
        ],
        || {
          let result = find_config_path(None);
          assert!(result.is_ok());
        },
      );

      std::env::set_current_dir(original_dir).unwrap();
    }

    #[test]
    #[serial]
    #[cfg(windows)]
    fn config_dir_fallback() {
      let temp = TempDir::new().unwrap();
      let syslua_config = temp.path().join("syslua");
      fs::create_dir_all(&syslua_config).unwrap();
      fs::write(syslua_config.join("init.lua"), "return {}").unwrap();

      // Use a directory without init.lua as cwd
      let cwd = temp.path().join("empty");
      fs::create_dir_all(&cwd).unwrap();
      let original_dir = std::env::current_dir().unwrap();
      std::env::set_current_dir(&cwd).unwrap();

      temp_env::with_vars(
        [
          ("APPDATA", Some(temp.path().to_str().unwrap())),
          ("USERPROFILE", Some(temp.path().to_str().unwrap())),
        ],
        || {
          let result = find_config_path(None);
          assert!(result.is_ok());
        },
      );

      std::env::set_current_dir(original_dir).unwrap();
    }
  }

  mod update_inputs_tests {
    use super::*;

    #[test]
    #[serial]
    fn updates_path_input() {
      let temp = TempDir::new().unwrap();
      let config_dir = temp.path();

      // Create a local input directory
      let input_dir = config_dir.join("my-input");
      fs::create_dir(&input_dir).unwrap();

      // Create config that references the input
      let config_path = config_dir.join("init.lua");
      fs::write(
        &config_path,
        r#"
          return {
            inputs = {
              myinput = "path:./my-input",
            },
            setup = function(inputs) end,
          }
        "#,
      )
      .unwrap();

      temp_env::with_vars(
        [
          ("XDG_DATA_HOME", Some(temp.path().to_str().unwrap())),
          ("XDG_CACHE_HOME", Some(temp.path().to_str().unwrap())),
          ("HOME", Some(temp.path().to_str().unwrap())),
        ],
        || {
          let options = UpdateOptions::default();
          let result = update_inputs(&config_path, &options).unwrap();

          // First time should be "added"
          assert_eq!(result.added.len(), 1);
          assert!(result.added.contains(&"myinput".to_string()));
          assert!(result.updated.is_empty());
          assert!(result.lock_changed);
        },
      );
    }

    #[test]
    #[serial]
    fn dry_run_no_changes() {
      let temp = TempDir::new().unwrap();
      let config_dir = temp.path();

      // Create a local input directory
      let input_dir = config_dir.join("my-input");
      fs::create_dir(&input_dir).unwrap();

      // Create config
      let config_path = config_dir.join("init.lua");
      fs::write(
        &config_path,
        r#"
          return {
            inputs = {
              myinput = "path:./my-input",
            },
            setup = function(inputs) end,
          }
        "#,
      )
      .unwrap();

      temp_env::with_vars(
        [
          ("XDG_DATA_HOME", Some(temp.path().to_str().unwrap())),
          ("XDG_CACHE_HOME", Some(temp.path().to_str().unwrap())),
          ("HOME", Some(temp.path().to_str().unwrap())),
        ],
        || {
          let options = UpdateOptions {
            dry_run: true,
            ..Default::default()
          };

          let result = update_inputs(&config_path, &options).unwrap();

          // Lock file should NOT be created in dry run
          let lock_path = config_dir.join("syslua.lock");
          assert!(!lock_path.exists(), "lock file should not be created in dry run");

          // But result should still show what would happen
          assert!(result.lock_changed);
        },
      );
    }

    #[test]
    fn input_not_found_error() {
      let temp = TempDir::new().unwrap();
      let config_path = temp.path().join("init.lua");
      fs::write(
        &config_path,
        r#"
          return {
            inputs = {},
            setup = function(inputs) end,
          }
        "#,
      )
      .unwrap();

      let options = UpdateOptions {
        inputs: vec!["nonexistent".to_string()],
        ..Default::default()
      };

      let result = update_inputs(&config_path, &options);
      assert!(result.is_err());
      assert!(matches!(result.unwrap_err(), UpdateError::InputNotFound { .. }));
    }
  }
}
