//! Bind state persistence for syslua.
//!
//! When a bind is applied, its resolved outputs are persisted to disk.
//! This allows destroy_actions to reference the actual resolved paths
//! via placeholders like `${out:link}`.
//!
//! # Storage Layout
//!
//! ```text
//! store/bind/<hash>/
//! └── state.json
//! ```
//!
//! # Example State File
//!
//! ```json
//! {
//!   "outputs": {
//!     "link": "/home/user/.config/nvim/init.lua",
//!     "target": "/syslua/store/build/abc123/init.lua"
//!   }
//! }
//! ```

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::{debug, info};

use crate::bind::store::bind_dir_path;
use crate::util::hash::ObjectHash;

/// State file name within bind directory.
const STATE_FILENAME: &str = "state.json";

/// Persisted state for an applied bind.
///
/// This captures the resolved outputs from when the bind was applied,
/// enabling destroy_actions to reference the actual paths that were created.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BindState {
  /// Resolved outputs from when the bind was applied.
  /// Keys are output names, values are resolved paths/values.
  pub outputs: HashMap<String, String>,
}

impl BindState {
  /// Create a new bind state with the given outputs.
  pub fn new(outputs: HashMap<String, String>) -> Self {
    Self { outputs }
  }

  /// Create an empty bind state.
  pub fn empty() -> Self {
    Self {
      outputs: HashMap::new(),
    }
  }
}

/// Errors that can occur when working with bind state.
#[derive(Debug, Error)]
pub enum BindStateError {
  /// Failed to read bind state file.
  #[error("failed to read bind state: {0}")]
  Read(#[source] io::Error),

  /// Failed to write bind state file.
  #[error("failed to write bind state: {0}")]
  Write(#[source] io::Error),

  /// Failed to create bind state directory.
  #[error("failed to create bind state directory: {0}")]
  CreateDir(#[source] io::Error),

  /// Failed to parse bind state JSON.
  #[error("failed to parse bind state: {0}")]
  Parse(#[source] serde_json::Error),

  /// Failed to serialize bind state.
  #[error("failed to serialize bind state: {0}")]
  Serialize(#[source] serde_json::Error),

  /// Failed to remove bind state.
  #[error("failed to remove bind state: {0}")]
  Remove(#[source] io::Error),
}

/// Get the bind state file path for a given bind hash.
fn bind_state_path(hash: &ObjectHash, system: bool) -> PathBuf {
  bind_dir_path(hash, system).join(STATE_FILENAME)
}

/// Save bind state after successful apply.
///
/// Creates the bind directory if it doesn't exist and writes the state file.
/// Uses atomic write (write to temp, then rename) to prevent corruption.
pub fn save_bind_state(hash: &ObjectHash, state: &BindState, system: bool) -> Result<(), BindStateError> {
  let dir = bind_dir_path(hash, system);
  let path = dir.join(STATE_FILENAME);

  info!(
    hash = %hash.0,
    system = system,
    path = %path.display(),
    output_count = state.outputs.len(),
    "saving bind state"
  );
  debug!(outputs = ?state.outputs, "bind state outputs");

  // Create directory if needed
  fs::create_dir_all(&dir).map_err(BindStateError::CreateDir)?;

  // Serialize state
  let content = serde_json::to_string_pretty(state).map_err(BindStateError::Serialize)?;

  // Write atomically: write to temp file, then rename
  let temp_path = dir.join("state.json.tmp");
  fs::write(&temp_path, &content).map_err(BindStateError::Write)?;
  fs::rename(&temp_path, &path).map_err(BindStateError::Write)?;

  info!(hash = %hash.0, "bind state saved successfully");
  Ok(())
}

/// Load bind state for destroy operations.
///
/// Returns `Ok(None)` if the state file doesn't exist (bind was never applied
/// or state was already cleaned up).
pub fn load_bind_state(hash: &ObjectHash, system: bool) -> Result<Option<BindState>, BindStateError> {
  let path = bind_state_path(hash, system);

  info!(
    hash = %hash.0,
    system = system,
    path = %path.display(),
    "loading bind state"
  );

  // Check if parent directory exists
  if let Some(parent) = path.parent() {
    debug!(
      parent_exists = parent.exists(),
      parent_path = %parent.display(),
      "checking bind state parent directory"
    );
  }

  let content = match fs::read_to_string(&path) {
    Ok(content) => {
      info!(hash = %hash.0, content_len = content.len(), "bind state file found");
      content
    }
    Err(e) if e.kind() == io::ErrorKind::NotFound => {
      info!(hash = %hash.0, path = %path.display(), "bind state file not found");
      return Ok(None);
    }
    Err(e) => {
      info!(hash = %hash.0, error = %e, "failed to read bind state file");
      return Err(BindStateError::Read(e));
    }
  };

  let state: BindState = serde_json::from_str(&content).map_err(BindStateError::Parse)?;
  debug!(outputs = ?state.outputs, "loaded bind state outputs");
  info!(
    hash = %hash.0,
    output_count = state.outputs.len(),
    "bind state loaded successfully"
  );
  Ok(Some(state))
}

/// Remove bind state after successful destroy.
///
/// Removes the entire bind directory including the state file.
/// Silently succeeds if the directory doesn't exist.
pub fn remove_bind_state(hash: &ObjectHash, system: bool) -> Result<(), BindStateError> {
  let dir = bind_dir_path(hash, system);

  info!(
    hash = %hash.0,
    system = system,
    path = %dir.display(),
    "removing bind state directory"
  );

  match fs::remove_dir_all(&dir) {
    Ok(()) => {
      info!(hash = %hash.0, "bind state directory removed successfully");
      Ok(())
    }
    Err(e) if e.kind() == io::ErrorKind::NotFound => {
      info!(hash = %hash.0, "bind state directory already gone");
      Ok(())
    }
    Err(e) => {
      info!(hash = %hash.0, error = %e, "failed to remove bind state directory");
      Err(BindStateError::Remove(e))
    }
  }
}

/// Check if bind state exists for a given hash.
pub fn bind_state_exists(hash: &ObjectHash, system: bool) -> bool {
  bind_state_path(hash, system).exists()
}

#[cfg(test)]
mod tests {
  use crate::util::hash::ObjectHash;

  use super::*;
  use tempfile::TempDir;

  fn with_temp_store<F>(f: F)
  where
    F: FnOnce(&TempDir),
  {
    let temp_dir = TempDir::new().unwrap();
    temp_env::with_var("SYSLUA_USER_STORE", Some(temp_dir.path().to_str().unwrap()), || {
      f(&temp_dir);
    });
  }

  /// Get the bind state file path for a given bind hash (test helper).
  fn test_bind_state_path(hash: &ObjectHash, system: bool) -> PathBuf {
    bind_dir_path(hash, system).join(STATE_FILENAME)
  }

  #[test]
  fn save_and_load_roundtrip() {
    with_temp_store(|_| {
      let hash = ObjectHash("abc123def456789012345678".to_string());
      let mut outputs = HashMap::new();
      outputs.insert("link".to_string(), "/home/user/.config/nvim".to_string());
      outputs.insert("target".to_string(), "/store/obj/nvim-abc123".to_string());

      let state = BindState::new(outputs);
      save_bind_state(&hash, &state, false).unwrap();

      let loaded = load_bind_state(&hash, false).unwrap().unwrap();
      assert_eq!(state, loaded);
    });
  }

  #[test]
  fn load_nonexistent_returns_none() {
    with_temp_store(|_| {
      let hash = ObjectHash("nonexistent123456789012".to_string());
      let result = load_bind_state(&hash, false).unwrap();
      assert!(result.is_none());
    });
  }

  #[test]
  fn remove_cleans_up_directory() {
    with_temp_store(|_| {
      let hash = ObjectHash("abc123def456789012345678".to_string());
      let state = BindState::empty();

      save_bind_state(&hash, &state, false).unwrap();
      assert!(bind_state_exists(&hash, false));

      remove_bind_state(&hash, false).unwrap();
      assert!(!bind_state_exists(&hash, false));
    });
  }

  // Corrupt state handling tests

  #[test]
  fn load_bind_state_handles_invalid_json() {
    with_temp_store(|_| {
      let hash = ObjectHash("corrupt_json_test123456".to_string());

      // Manually write invalid JSON to the state file path
      let state_path = test_bind_state_path(&hash, false);
      if let Some(parent) = state_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
      }
      std::fs::write(&state_path, "{ this is not valid json }").unwrap();

      // Should return an error, not panic
      let result = load_bind_state(&hash, false);
      assert!(result.is_err());

      // Verify the error is a parse error
      match result {
        Err(BindStateError::Parse(_)) => {} // Expected
        Err(other) => panic!("expected Parse error, got: {}", other),
        Ok(_) => panic!("expected error, got Ok"),
      }
    });
  }

  #[test]
  fn load_bind_state_handles_wrong_schema() {
    with_temp_store(|_| {
      let hash = ObjectHash("wrong_schema_test12345".to_string());

      let state_path = test_bind_state_path(&hash, false);
      if let Some(parent) = state_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
      }
      // Valid JSON but wrong structure (missing "outputs" field)
      std::fs::write(&state_path, r#"{"unexpected": "structure"}"#).unwrap();

      let result = load_bind_state(&hash, false);
      // Should error due to missing required field
      assert!(result.is_err());
    });
  }

  #[test]
  fn load_bind_state_handles_empty_file() {
    with_temp_store(|_| {
      let hash = ObjectHash("empty_file_test1234567".to_string());

      let state_path = test_bind_state_path(&hash, false);
      if let Some(parent) = state_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
      }
      std::fs::write(&state_path, "").unwrap();

      let result = load_bind_state(&hash, false);
      // Empty file is not valid JSON
      assert!(result.is_err());
    });
  }

  #[test]
  fn load_bind_state_handles_null_json() {
    with_temp_store(|_| {
      let hash = ObjectHash("null_json_test12345678".to_string());

      let state_path = test_bind_state_path(&hash, false);
      if let Some(parent) = state_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
      }
      // Valid JSON but null is not a valid BindState
      std::fs::write(&state_path, "null").unwrap();

      let result = load_bind_state(&hash, false);
      assert!(result.is_err());
    });
  }

  #[test]
  fn load_bind_state_handles_array_instead_of_object() {
    with_temp_store(|_| {
      let hash = ObjectHash("array_json_test1234567".to_string());

      let state_path = test_bind_state_path(&hash, false);
      if let Some(parent) = state_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
      }
      // Valid JSON array, but BindState expects an object
      std::fs::write(&state_path, r#"["item1", "item2"]"#).unwrap();

      let result = load_bind_state(&hash, false);
      assert!(result.is_err());
    });
  }

  #[test]
  fn load_bind_state_handles_outputs_with_wrong_type() {
    with_temp_store(|_| {
      let hash = ObjectHash("wrong_output_type12345".to_string());

      let state_path = test_bind_state_path(&hash, false);
      if let Some(parent) = state_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
      }
      // "outputs" should be a map of string to string, not string to number
      std::fs::write(&state_path, r#"{"outputs": {"key": 12345}}"#).unwrap();

      let result = load_bind_state(&hash, false);
      assert!(result.is_err());
    });
  }
}
