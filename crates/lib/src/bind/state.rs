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

const STATE_FILENAME: &str = "state.json";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BindState {
  pub outputs: HashMap<String, String>,
}

impl BindState {
  pub fn new(outputs: HashMap<String, String>) -> Self {
    Self { outputs }
  }

  pub fn empty() -> Self {
    Self {
      outputs: HashMap::new(),
    }
  }
}

#[derive(Debug, Error)]
pub enum BindStateError {
  #[error("failed to read bind state: {0}")]
  Read(#[source] io::Error),

  #[error("failed to write bind state: {0}")]
  Write(#[source] io::Error),

  #[error("failed to create bind state directory: {0}")]
  CreateDir(#[source] io::Error),

  #[error("failed to parse bind state: {0}")]
  Parse(#[source] serde_json::Error),

  #[error("failed to serialize bind state: {0}")]
  Serialize(#[source] serde_json::Error),

  #[error("failed to remove bind state: {0}")]
  Remove(#[source] io::Error),
}

fn bind_state_path(hash: &ObjectHash) -> PathBuf {
  bind_dir_path(hash).join(STATE_FILENAME)
}

pub fn save_bind_state(hash: &ObjectHash, state: &BindState) -> Result<(), BindStateError> {
  let dir = bind_dir_path(hash);
  let path = dir.join(STATE_FILENAME);

  info!(
    hash = %hash.0,
    path = %path.display(),
    output_count = state.outputs.len(),
    "saving bind state"
  );
  debug!(outputs = ?state.outputs, "bind state outputs");

  fs::create_dir_all(&dir).map_err(BindStateError::CreateDir)?;

  let content = serde_json::to_string_pretty(state).map_err(BindStateError::Serialize)?;

  let temp_path = dir.join("state.json.tmp");
  fs::write(&temp_path, &content).map_err(BindStateError::Write)?;
  fs::rename(&temp_path, &path).map_err(BindStateError::Write)?;

  info!(hash = %hash.0, "bind state saved successfully");
  Ok(())
}

pub fn load_bind_state(hash: &ObjectHash) -> Result<Option<BindState>, BindStateError> {
  let path = bind_state_path(hash);

  info!(
    hash = %hash.0,
    path = %path.display(),
    "loading bind state"
  );

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

pub fn remove_bind_state(hash: &ObjectHash) -> Result<(), BindStateError> {
  let dir = bind_dir_path(hash);

  info!(
    hash = %hash.0,
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

pub fn bind_state_exists(hash: &ObjectHash) -> bool {
  bind_state_path(hash).exists()
}

#[cfg(test)]
mod tests {
  use super::*;
  use serial_test::serial;
  use tempfile::TempDir;

  fn with_temp_store<F>(f: F)
  where
    F: FnOnce(&TempDir),
  {
    let temp_dir = TempDir::new().unwrap();
    temp_env::with_var("SYSLUA_STORE", Some(temp_dir.path().to_str().unwrap()), || {
      f(&temp_dir);
    });
  }

  fn test_bind_state_path(hash: &ObjectHash) -> PathBuf {
    bind_dir_path(hash).join(STATE_FILENAME)
  }

  #[test]
  #[serial]
  fn save_and_load_roundtrip() {
    with_temp_store(|_| {
      let hash = ObjectHash("abc123def456789012345678".to_string());
      let mut outputs = HashMap::new();
      outputs.insert("link".to_string(), "/home/user/.config/nvim".to_string());
      outputs.insert("target".to_string(), "/store/obj/nvim-abc123".to_string());

      let state = BindState::new(outputs);
      save_bind_state(&hash, &state).unwrap();

      let loaded = load_bind_state(&hash).unwrap().unwrap();
      assert_eq!(state, loaded);
    });
  }

  #[test]
  #[serial]
  fn load_nonexistent_returns_none() {
    with_temp_store(|_| {
      let hash = ObjectHash("nonexistent123456789012".to_string());
      let result = load_bind_state(&hash).unwrap();
      assert!(result.is_none());
    });
  }

  #[test]
  #[serial]
  fn remove_cleans_up_directory() {
    with_temp_store(|_| {
      let hash = ObjectHash("abc123def456789012345678".to_string());
      let state = BindState::empty();

      save_bind_state(&hash, &state).unwrap();
      assert!(bind_state_exists(&hash));

      remove_bind_state(&hash).unwrap();
      assert!(!bind_state_exists(&hash));
    });
  }

  #[test]
  #[serial]
  fn load_bind_state_handles_invalid_json() {
    with_temp_store(|_| {
      let hash = ObjectHash("corrupt_json_test123456".to_string());

      let state_path = test_bind_state_path(&hash);
      if let Some(parent) = state_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
      }
      std::fs::write(&state_path, "{ this is not valid json }").unwrap();

      let result = load_bind_state(&hash);
      assert!(result.is_err());

      match result {
        Err(BindStateError::Parse(_)) => {}
        Err(other) => panic!("expected Parse error, got: {}", other),
        Ok(_) => panic!("expected error, got Ok"),
      }
    });
  }

  #[test]
  #[serial]
  fn load_bind_state_handles_wrong_schema() {
    with_temp_store(|_| {
      let hash = ObjectHash("wrong_schema_test12345".to_string());

      let state_path = test_bind_state_path(&hash);
      if let Some(parent) = state_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
      }
      std::fs::write(&state_path, r#"{"unexpected": "structure"}"#).unwrap();

      let result = load_bind_state(&hash);
      assert!(result.is_err());
    });
  }

  #[test]
  #[serial]
  fn load_bind_state_handles_empty_file() {
    with_temp_store(|_| {
      let hash = ObjectHash("empty_file_test1234567".to_string());

      let state_path = test_bind_state_path(&hash);
      if let Some(parent) = state_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
      }
      std::fs::write(&state_path, "").unwrap();

      let result = load_bind_state(&hash);
      assert!(result.is_err());
    });
  }

  #[test]
  #[serial]
  fn load_bind_state_handles_null_json() {
    with_temp_store(|_| {
      let hash = ObjectHash("null_json_test12345678".to_string());

      let state_path = test_bind_state_path(&hash);
      if let Some(parent) = state_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
      }
      std::fs::write(&state_path, "null").unwrap();

      let result = load_bind_state(&hash);
      assert!(result.is_err());
    });
  }

  #[test]
  #[serial]
  fn load_bind_state_handles_array_instead_of_object() {
    with_temp_store(|_| {
      let hash = ObjectHash("array_json_test1234567".to_string());

      let state_path = test_bind_state_path(&hash);
      if let Some(parent) = state_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
      }
      std::fs::write(&state_path, r#"["item1", "item2"]"#).unwrap();

      let result = load_bind_state(&hash);
      assert!(result.is_err());
    });
  }

  #[test]
  #[serial]
  fn load_bind_state_handles_outputs_with_wrong_type() {
    with_temp_store(|_| {
      let hash = ObjectHash("wrong_output_type12345".to_string());

      let state_path = test_bind_state_path(&hash);
      if let Some(parent) = state_path.parent() {
        std::fs::create_dir_all(parent).unwrap();
      }
      std::fs::write(&state_path, r#"{"outputs": {"key": 12345}}"#).unwrap();

      let result = load_bind_state(&hash);
      assert!(result.is_err());
    });
  }
}
