//! Lock file management for input resolution.
//!
//! The lock file (`syslua.lock`) pins input revisions to ensure reproducible
//! configuration evaluation. It's stored in the same directory as the config file.
//!
//! # Lock File Format
//!
//! ```json
//! {
//!   "version": 1,
//!   "inputs": {
//!     "syslua": {
//!       "type": "git",
//!       "url": "git:https://github.com/spirit-led-software/syslua.git",
//!       "rev": "a1b2c3d4...",
//!       "lastModified": 1733667300
//!     }
//!   }
//! }
//! ```

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Current lock file format version.
pub const LOCK_VERSION: u32 = 1;

/// Lock file name.
pub const LOCK_FILENAME: &str = "syslua.lock";

/// A lock file containing pinned input revisions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LockFile {
  /// Lock file format version.
  pub version: u32,
  /// Locked inputs, keyed by input name.
  pub inputs: BTreeMap<String, LockedInput>,
}

/// A locked input entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockedInput {
  /// Input type: "git" or "path".
  #[serde(rename = "type")]
  pub type_: String,

  /// Original URL from config (e.g., "git:https://..." or "path:~/...").
  pub url: String,

  /// Pinned revision (git commit hash or "local" for path inputs).
  pub rev: String,

  /// Unix timestamp of when this input was last modified/fetched.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub last_modified: Option<u64>,
}

/// Errors that can occur when working with lock files.
#[derive(Debug, Error)]
pub enum LockError {
  /// Failed to read the lock file.
  #[error("failed to read lock file: {0}")]
  Read(#[source] io::Error),

  /// Failed to write the lock file.
  #[error("failed to write lock file: {0}")]
  Write(#[source] io::Error),

  /// Failed to parse the lock file JSON.
  #[error("failed to parse lock file: {0}")]
  Parse(#[source] serde_json::Error),

  /// Failed to serialize the lock file.
  #[error("failed to serialize lock file: {0}")]
  Serialize(#[source] serde_json::Error),

  /// Lock file version is not supported.
  #[error("unsupported lock file version {0}, expected {LOCK_VERSION}")]
  UnsupportedVersion(u32),
}

impl Default for LockFile {
  fn default() -> Self {
    Self::new()
  }
}

impl LockFile {
  /// Create a new empty lock file.
  pub fn new() -> Self {
    Self {
      version: LOCK_VERSION,
      inputs: BTreeMap::new(),
    }
  }

  /// Load a lock file from the given path.
  ///
  /// Returns `Ok(None)` if the file doesn't exist.
  /// Returns `Ok(Some(lock))` if the file exists and was parsed successfully.
  /// Returns `Err` if the file exists but couldn't be read or parsed.
  pub fn load(path: &Path) -> Result<Option<Self>, LockError> {
    let content = match fs::read_to_string(path) {
      Ok(content) => content,
      Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
      Err(e) => return Err(LockError::Read(e)),
    };

    let lock: LockFile = serde_json::from_str(&content).map_err(LockError::Parse)?;

    if lock.version != LOCK_VERSION {
      return Err(LockError::UnsupportedVersion(lock.version));
    }

    Ok(Some(lock))
  }

  /// Save the lock file to the given path.
  ///
  /// The file is written with pretty-printed JSON for readability.
  pub fn save(&self, path: &Path) -> Result<(), LockError> {
    let content = serde_json::to_string_pretty(self).map_err(LockError::Serialize)?;
    fs::write(path, content).map_err(LockError::Write)?;
    Ok(())
  }

  /// Get a locked input by name.
  pub fn get(&self, name: &str) -> Option<&LockedInput> {
    self.inputs.get(name)
  }

  /// Insert or update a locked input.
  pub fn insert(&mut self, name: String, input: LockedInput) {
    self.inputs.insert(name, input);
  }
}

impl LockedInput {
  /// Create a new locked input entry.
  pub fn new(type_: &str, url: &str, rev: &str) -> Self {
    Self {
      type_: type_.to_string(),
      url: url.to_string(),
      rev: rev.to_string(),
      last_modified: None,
    }
  }

  /// Set the last modified timestamp.
  pub fn with_last_modified(mut self, timestamp: u64) -> Self {
    self.last_modified = Some(timestamp);
    self
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  mod lock_file {
    use super::*;

    #[test]
    fn insert_and_get() {
      let mut lock = LockFile::new();
      lock.insert(
        "syslua".to_string(),
        LockedInput::new("git", "git:https://example.com/repo.git", "abc123"),
      );

      let input = lock.get("syslua").unwrap();
      assert_eq!(input.type_, "git");
      assert_eq!(input.rev, "abc123");
    }

    #[test]
    fn save_and_load_roundtrip() {
      let temp_dir = TempDir::new().unwrap();
      let lock_path = temp_dir.path().join(LOCK_FILENAME);

      let mut original = LockFile::new();
      original.insert(
        "syslua".to_string(),
        LockedInput::new("git", "git:https://github.com/org/repo.git", "a1b2c3d4e5f6").with_last_modified(1733667300),
      );
      original.insert(
        "dotfiles".to_string(),
        LockedInput::new("path", "path:~/dotfiles", "local"),
      );

      original.save(&lock_path).unwrap();
      let loaded = LockFile::load(&lock_path).unwrap().unwrap();

      assert_eq!(original, loaded);
    }

    #[test]
    fn load_nonexistent_returns_none() {
      let temp_dir = TempDir::new().unwrap();
      let lock_path = temp_dir.path().join("nonexistent.lock");

      let result = LockFile::load(&lock_path).unwrap();
      assert!(result.is_none());
    }

    #[test]
    fn load_invalid_json_returns_error() {
      let temp_dir = TempDir::new().unwrap();
      let lock_path = temp_dir.path().join(LOCK_FILENAME);

      fs::write(&lock_path, "not valid json").unwrap();
      let result = LockFile::load(&lock_path);

      assert!(matches!(result, Err(LockError::Parse(_))));
    }

    #[test]
    fn load_unsupported_version_returns_error() {
      let temp_dir = TempDir::new().unwrap();
      let lock_path = temp_dir.path().join(LOCK_FILENAME);

      fs::write(&lock_path, r#"{"version": 999, "inputs": {}}"#).unwrap();
      let result = LockFile::load(&lock_path);

      assert!(matches!(result, Err(LockError::UnsupportedVersion(999))));
    }
  }

  mod serialization {
    use super::*;

    #[test]
    fn json_format_matches_spec() {
      let mut lock = LockFile::new();
      lock.insert(
        "syslua".to_string(),
        LockedInput::new("git", "git:https://github.com/org/repo.git", "abc123").with_last_modified(1733667300),
      );

      let json = serde_json::to_string_pretty(&lock).unwrap();

      // Check that the JSON has expected structure
      assert!(json.contains(r#""version": 1"#));
      assert!(json.contains(r#""type": "git""#));
      assert!(json.contains(r#""url": "git:https://github.com/org/repo.git""#));
      assert!(json.contains(r#""rev": "abc123""#));
      assert!(json.contains(r#""lastModified": 1733667300"#));
    }

    #[test]
    fn last_modified_omitted_when_none() {
      let lock_input = LockedInput::new("path", "path:~/foo", "local");
      let json = serde_json::to_string(&lock_input).unwrap();

      assert!(!json.contains("lastModified"));
    }
  }
}
