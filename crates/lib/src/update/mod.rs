//! Input update orchestration.
//!
//! This module provides the core logic for the `sys update` command, which
//! re-resolves inputs (fetching latest revisions) and updates the lock file.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use thiserror::Error;
use tracing::info;

use crate::init::update_luarc_inputs;
use crate::inputs::ResolvedInputs;
use crate::inputs::lock::{LOCK_FILENAME, LockFile};
use crate::inputs::resolve::{ResolutionResult, ResolveError, resolve_inputs, save_lock_file_if_changed};
use crate::lua::entrypoint::extract_input_decls;
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
  /// Direct inputs that were updated: name -> (old_rev, new_rev).
  pub updated: BTreeMap<String, (String, String)>,
  /// Transitive inputs that were updated: full_path -> (old_rev, new_rev).
  pub transitive_updated: BTreeMap<String, (String, String)>,
  /// Direct inputs that remained unchanged.
  pub unchanged: Vec<String>,
  /// New direct inputs that were added (not previously locked).
  pub added: Vec<String>,
  /// New transitive inputs that were added.
  pub transitive_added: Vec<String>,
  /// Resolved inputs with their final paths and revisions (including transitive deps).
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
/// This function handles both direct and transitive dependencies:
/// - Direct inputs are force-updated if named, or all direct inputs if no names given
/// - Transitive dependencies are re-resolved but reuse lock entries when URLs match
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

  // Extract input declarations from config (supports extended syntax)
  let input_decls = extract_input_decls(&config_path_str)?;

  // Validate that requested inputs exist in config
  for input_name in &options.inputs {
    if !input_decls.contains_key(input_name) {
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
  // If no specific inputs named, force-update all direct inputs
  let force_update: HashSet<String> = if options.inputs.is_empty() {
    input_decls.keys().cloned().collect()
  } else {
    options.inputs.iter().cloned().collect()
  };

  info!(
    count = input_decls.len(),
    force_count = force_update.len(),
    "resolving inputs with transitive dependencies"
  );

  // Resolve inputs with force update (transitive resolution)
  let result: ResolutionResult = resolve_inputs(&input_decls, config_dir, Some(&force_update))?;

  // Compute what changed for direct inputs
  let mut updated = BTreeMap::new();
  let mut unchanged = Vec::new();
  let mut added = Vec::new();

  // Track transitive updates
  let mut transitive_updated = BTreeMap::new();
  let mut transitive_added = Vec::new();

  // Check direct inputs
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

    // Check transitive dependencies of this input
    collect_transitive_changes(
      name,
      &resolved.inputs,
      &old_lock,
      &mut transitive_updated,
      &mut transitive_added,
    );
  }

  // Write lock file and update .luarc.json (unless dry run)
  if !options.dry_run {
    save_lock_file_if_changed(&result, config_dir)?;

    // Collect all input paths (direct + transitive) for .luarc.json
    let input_paths: Vec<_> = collect_all_input_paths(&result.inputs);
    update_luarc_inputs(config_dir, input_paths, options.system);
  }

  Ok(UpdateResult {
    updated,
    transitive_updated,
    unchanged,
    added,
    transitive_added,
    resolved: result.inputs,
    lock_changed: result.lock_changed,
  })
}

/// Recursively collect transitive input changes.
fn collect_transitive_changes(
  parent_path: &str,
  transitive: &ResolvedInputs,
  old_lock: &LockFile,
  updated: &mut BTreeMap<String, (String, String)>,
  added: &mut Vec<String>,
) {
  for (name, resolved) in transitive {
    // Build the full path for the lock key (e.g., "pkgs/utils")
    let full_path = format!("{}/{}", parent_path, name);

    if let Some(old_entry) = old_lock.get(&full_path) {
      if old_entry.rev != resolved.rev {
        updated.insert(full_path.clone(), (old_entry.rev.clone(), resolved.rev.clone()));
      }
    } else {
      added.push(full_path.clone());
    }

    // Recurse into nested transitive deps
    collect_transitive_changes(&full_path, &resolved.inputs, old_lock, updated, added);
  }
}

/// Collect all input paths (direct and transitive) for .luarc.json.
fn collect_all_input_paths(inputs: &ResolvedInputs) -> Vec<&Path> {
  let mut paths = Vec::new();
  collect_paths_recursive(inputs, &mut paths);
  paths
}

fn collect_paths_recursive<'a>(inputs: &'a ResolvedInputs, paths: &mut Vec<&'a Path>) {
  for resolved in inputs.values() {
    paths.push(resolved.path.as_path());
    collect_paths_recursive(&resolved.inputs, paths);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::fs;
  use tempfile::TempDir;

  mod find_config_path_tests {
    use serial_test::serial;

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
    use serial_test::serial;

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

    #[test]
    #[serial]
    fn updates_input_with_transitive_deps() {
      let temp = TempDir::new().unwrap();
      let config_dir = temp.path();

      // Create lib_b (transitive dep of lib_a)
      let lib_b = config_dir.join("lib_b");
      fs::create_dir_all(&lib_b).unwrap();
      fs::write(
        lib_b.join("init.lua"),
        r#"
return {
  inputs = {},
  setup = function() end,
}
"#,
      )
      .unwrap();

      // Create lib_a which depends on lib_b
      let lib_a = config_dir.join("lib_a");
      fs::create_dir_all(&lib_a).unwrap();
      fs::write(
        lib_a.join("init.lua"),
        format!(
          r#"
return {{
  inputs = {{
    lib_b = "path:{}",
  }},
  setup = function() end,
}}
"#,
          lib_b.display()
        ),
      )
      .unwrap();

      // Create config that references lib_a
      let config_path = config_dir.join("init.lua");
      fs::write(
        &config_path,
        format!(
          r#"
return {{
  inputs = {{
    lib_a = "path:{}",
  }},
  setup = function(inputs) end,
}}
"#,
          lib_a.display()
        ),
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

          // lib_a should be added as a direct input
          assert_eq!(result.added.len(), 1);
          assert!(result.added.contains(&"lib_a".to_string()));

          // lib_b should be added as a transitive input
          assert_eq!(result.transitive_added.len(), 1);
          assert!(result.transitive_added.contains(&"lib_a/lib_b".to_string()));

          // Check that resolved includes transitive deps
          let lib_a_resolved = result.resolved.get("lib_a").unwrap();
          assert!(lib_a_resolved.inputs.contains_key("lib_b"));

          assert!(result.lock_changed);
        },
      );
    }

    #[test]
    #[serial]
    fn update_specific_input_only() {
      let temp = TempDir::new().unwrap();
      let config_dir = temp.path();

      // Create two independent inputs
      let input_a = config_dir.join("input_a");
      fs::create_dir_all(&input_a).unwrap();
      fs::write(
        input_a.join("init.lua"),
        r#"
return {
  inputs = {},
  setup = function() end,
}
"#,
      )
      .unwrap();

      let input_b = config_dir.join("input_b");
      fs::create_dir_all(&input_b).unwrap();
      fs::write(
        input_b.join("init.lua"),
        r#"
return {
  inputs = {},
  setup = function() end,
}
"#,
      )
      .unwrap();

      // Create config with both inputs
      let config_path = config_dir.join("init.lua");
      fs::write(
        &config_path,
        format!(
          r#"
return {{
  inputs = {{
    input_a = "path:{}",
    input_b = "path:{}",
  }},
  setup = function(inputs) end,
}}
"#,
          input_a.display(),
          input_b.display()
        ),
      )
      .unwrap();

      temp_env::with_vars(
        [
          ("XDG_DATA_HOME", Some(temp.path().to_str().unwrap())),
          ("XDG_CACHE_HOME", Some(temp.path().to_str().unwrap())),
          ("HOME", Some(temp.path().to_str().unwrap())),
        ],
        || {
          // First, update all to create the lock file
          let options = UpdateOptions::default();
          let _ = update_inputs(&config_path, &options).unwrap();

          // Now update only input_a
          let options = UpdateOptions {
            inputs: vec!["input_a".to_string()],
            ..Default::default()
          };
          let result = update_inputs(&config_path, &options).unwrap();

          // Both inputs should be in resolved (we still resolve everything)
          assert!(result.resolved.contains_key("input_a"));
          assert!(result.resolved.contains_key("input_b"));

          // Since both were already in the lock file, they should be unchanged
          // (path inputs don't change revision)
          assert!(result.updated.is_empty());
          assert!(result.added.is_empty());
        },
      );
    }
  }
}
