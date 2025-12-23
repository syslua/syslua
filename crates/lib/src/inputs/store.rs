//! Content-addressed input store.
//!
//! This module manages the storage of fetched inputs in a content-addressed store.
//!
//! # Store Structure
//!
//! Inputs are stored in a content-addressed manner:
//!
//! ```text
//! ~/.cache/syslua/inputs/
//!   store/
//!     pkgs-a1b2c3d4/              # {name}-{hash(url+rev)[:8]}
//!       init.lua
//!       lua/
//!         pkgs/
//!           init.lua              # require("pkgs") loads this
//!     utils-e5f6g7h8/             # pkgs's utils (v1.0)
//!       init.lua
//!       lua/
//!         utils/
//!           init.lua
//! ```
//!
//! # Store Naming
//!
//! Store paths use `{name}-{hash[:8]}` format where:
//! - `name`: The input name (human-readable prefix)
//! - `hash[:8]`: First 8 characters of SHA-256 hash of `url+rev`
//!
//! # Deduplication
//!
//! Same URL+rev always produces the same store directory.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::platform::paths::cache_dir;

/// Length of hash suffix used in store directory names.
const STORE_HASH_LEN: usize = 8;

/// Errors that can occur during store operations.
#[derive(Debug, Error)]
pub enum StoreError {
  /// Failed to create a directory.
  #[error("failed to create directory '{path}': {source}")]
  CreateDir {
    path: PathBuf,
    #[source]
    source: io::Error,
  },

  /// Store entry not found.
  #[error("store entry not found: {0}")]
  NotFound(String),
}

/// The input store manager.
///
/// Handles content-addressed storage of inputs.
#[derive(Debug, Clone)]
pub struct InputStore {
  /// Base path to the store directory.
  store_dir: PathBuf,
}

impl Default for InputStore {
  fn default() -> Self {
    Self::new()
  }
}

impl InputStore {
  /// Create a new input store using the default cache directory.
  pub fn new() -> Self {
    Self {
      store_dir: cache_dir().join("inputs").join("store"),
    }
  }

  /// Create a new input store with a custom base path.
  pub fn with_path(store_dir: PathBuf) -> Self {
    Self { store_dir }
  }

  /// Get the store directory path.
  pub fn store_dir(&self) -> &Path {
    &self.store_dir
  }

  /// Ensure the store directory exists.
  pub fn ensure_store_dir(&self) -> Result<(), StoreError> {
    if !self.store_dir.exists() {
      fs::create_dir_all(&self.store_dir).map_err(|e| StoreError::CreateDir {
        path: self.store_dir.clone(),
        source: e,
      })?;
    }
    Ok(())
  }

  /// Compute the store path for an input.
  ///
  /// The path is `{store_dir}/{name}-{hash[:8]}` where hash is SHA-256 of `url+rev`.
  ///
  /// # Arguments
  ///
  /// * `name` - The input name (human-readable prefix)
  /// * `url` - The input URL
  /// * `rev` - The resolved revision (commit hash or "local")
  pub fn compute_store_path(&self, name: &str, url: &str, rev: &str) -> PathBuf {
    let hash = compute_input_hash(url, rev);
    self.store_dir.join(format!("{}-{}", name, hash))
  }

  /// Compute the store label for an input.
  ///
  /// The label is `{name}-{hash[:8]}`, used as a unique identifier.
  pub fn compute_store_label(name: &str, url: &str, rev: &str) -> String {
    let hash = compute_input_hash(url, rev);
    format!("{}-{}", name, hash)
  }

  /// Check if a store entry exists.
  pub fn exists(&self, name: &str, url: &str, rev: &str) -> bool {
    let path = self.compute_store_path(name, url, rev);
    path.exists()
  }

  /// Get the path to an existing store entry, or None if it doesn't exist.
  pub fn get(&self, name: &str, url: &str, rev: &str) -> Option<PathBuf> {
    let path = self.compute_store_path(name, url, rev);
    if path.exists() { Some(path) } else { None }
  }
}

/// Compute the hash suffix for a store path.
///
/// Returns the first 8 characters of SHA-256(`url + ":" + rev`).
fn compute_input_hash(url: &str, rev: &str) -> String {
  let mut hasher = Sha256::new();
  hasher.update(url.as_bytes());
  hasher.update(b":");
  hasher.update(rev.as_bytes());
  let full = format!("{:x}", hasher.finalize());
  full[..STORE_HASH_LEN].to_string()
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  mod compute_input_hash {
    use super::*;

    #[test]
    fn deterministic() {
      let h1 = compute_input_hash("git:https://example.com/repo", "abc123");
      let h2 = compute_input_hash("git:https://example.com/repo", "abc123");
      assert_eq!(h1, h2);
    }

    #[test]
    fn different_for_different_urls() {
      let h1 = compute_input_hash("git:https://example.com/repo1", "abc123");
      let h2 = compute_input_hash("git:https://example.com/repo2", "abc123");
      assert_ne!(h1, h2);
    }

    #[test]
    fn different_for_different_revs() {
      let h1 = compute_input_hash("git:https://example.com/repo", "abc123");
      let h2 = compute_input_hash("git:https://example.com/repo", "def456");
      assert_ne!(h1, h2);
    }

    #[test]
    fn correct_length() {
      let h = compute_input_hash("git:https://example.com/repo", "abc123");
      assert_eq!(h.len(), STORE_HASH_LEN);
    }
  }

  mod input_store {
    use super::*;

    #[test]
    fn compute_store_path_format() {
      let temp = TempDir::new().unwrap();
      let store = InputStore::with_path(temp.path().to_path_buf());

      let path = store.compute_store_path("pkgs", "git:https://example.com", "abc123");

      // Should be store_dir/pkgs-{8char hash}
      let name = path.file_name().unwrap().to_str().unwrap();
      assert!(name.starts_with("pkgs-"));
      assert_eq!(name.len(), 4 + 1 + STORE_HASH_LEN); // "pkgs" + "-" + hash
    }

    #[test]
    fn compute_store_label() {
      let label = InputStore::compute_store_label("utils", "git:https://example.com", "def456");
      assert!(label.starts_with("utils-"));
      assert_eq!(label.len(), 5 + 1 + STORE_HASH_LEN);
    }

    #[test]
    fn exists_returns_false_for_missing() {
      let temp = TempDir::new().unwrap();
      let store = InputStore::with_path(temp.path().to_path_buf());

      assert!(!store.exists("pkgs", "git:https://example.com", "abc123"));
    }

    #[test]
    fn exists_returns_true_for_existing() {
      let temp = TempDir::new().unwrap();
      let store = InputStore::with_path(temp.path().to_path_buf());
      store.ensure_store_dir().unwrap();

      // Create the store entry directory
      let path = store.compute_store_path("pkgs", "git:https://example.com", "abc123");
      fs::create_dir_all(&path).unwrap();

      assert!(store.exists("pkgs", "git:https://example.com", "abc123"));
    }

    #[test]
    fn get_returns_none_for_missing() {
      let temp = TempDir::new().unwrap();
      let store = InputStore::with_path(temp.path().to_path_buf());

      assert!(store.get("pkgs", "git:https://example.com", "abc123").is_none());
    }

    #[test]
    fn get_returns_path_for_existing() {
      let temp = TempDir::new().unwrap();
      let store = InputStore::with_path(temp.path().to_path_buf());
      store.ensure_store_dir().unwrap();

      // Create the store entry directory
      let expected_path = store.compute_store_path("pkgs", "git:https://example.com", "abc123");
      fs::create_dir_all(&expected_path).unwrap();

      let got = store.get("pkgs", "git:https://example.com", "abc123");
      assert_eq!(got, Some(expected_path));
    }
  }
}
