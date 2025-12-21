//! Content-addressed input store with dependency linking.
//!
//! This module manages the storage of fetched inputs in a content-addressed store
//! and creates `.inputs/` directories with symlinks to dependencies.
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
//!       cli/
//!         ripgrep.lua
//!       .inputs/
//!         utils -> ../utils-e5f6g7h8/   # Symlink to pkgs's utils
//!     utils-e5f6g7h8/             # pkgs's utils (v1.0)
//!       init.lua
//!     utils-i9j0k1l2/             # user's utils (different version)
//!       init.lua
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
//! Same URL+rev always produces the same store directory. Multiple `.inputs/`
//! symlinks can point to the same store directory.
//!
//! # Cross-Platform
//!
//! - **Unix**: Standard symlinks via `std::os::unix::fs::symlink`
//! - **Windows**: Directory symlinks or junctions (via `junction` crate if available)
//! - **Fallback**: Copy files if symlinks/junctions fail

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use thiserror::Error;
#[cfg(windows)]
use tracing::warn;
use tracing::{debug, trace};

use crate::platform::paths::cache_dir;

/// Length of hash suffix used in store directory names.
const STORE_HASH_LEN: usize = 8;

/// Name of the inputs directory within each store entry.
const INPUTS_DIR_NAME: &str = ".inputs";

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

  /// Failed to create a symlink.
  #[error("failed to create symlink from '{from}' to '{to}': {source}")]
  CreateSymlink {
    from: PathBuf,
    to: PathBuf,
    #[source]
    source: io::Error,
  },

  /// Failed to read a symlink.
  #[error("failed to read symlink '{path}': {source}")]
  ReadSymlink {
    path: PathBuf,
    #[source]
    source: io::Error,
  },

  /// Failed to remove a file or directory.
  #[error("failed to remove '{path}': {source}")]
  Remove {
    path: PathBuf,
    #[source]
    source: io::Error,
  },

  /// Store entry not found.
  #[error("store entry not found: {0}")]
  NotFound(String),

  /// Failed to copy directory.
  #[error("failed to copy directory from '{from}' to '{to}': {source}")]
  CopyDir {
    from: PathBuf,
    to: PathBuf,
    #[source]
    source: io::Error,
  },
}

/// The input store manager.
///
/// Handles content-addressed storage of inputs and dependency linking.
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

  /// Create the `.inputs/` directory and symlinks for an input's dependencies.
  ///
  /// # Arguments
  ///
  /// * `input_path` - Path to the input's store directory
  /// * `dependencies` - Map of dependency names to their store paths
  pub fn link_dependencies(
    &self,
    input_path: &Path,
    dependencies: &BTreeMap<String, PathBuf>,
  ) -> Result<(), StoreError> {
    if dependencies.is_empty() {
      return Ok(());
    }

    let inputs_dir = input_path.join(INPUTS_DIR_NAME);

    // Create .inputs directory if it doesn't exist
    if !inputs_dir.exists() {
      fs::create_dir(&inputs_dir).map_err(|e| StoreError::CreateDir {
        path: inputs_dir.clone(),
        source: e,
      })?;
    }

    for (dep_name, dep_store_path) in dependencies {
      let link_path = inputs_dir.join(dep_name);

      // Skip if link already exists and points to the right place
      if link_path.exists() || link_path.symlink_metadata().is_ok() {
        // Use read_dir_link which handles both symlinks and junctions
        if let Some(target) = read_dir_link(&link_path) {
          // Normalize both paths for comparison
          let expected = compute_relative_link(input_path, dep_store_path);
          // For junctions, the target is absolute, so also compare against the absolute path
          let expected_abs = inputs_dir.join(&expected);
          if target == expected || target == expected_abs {
            trace!(dep = dep_name, "dependency link already exists");
            continue;
          }
        }
        // Remove existing link/file to recreate
        remove_path(&link_path)?;
      }

      // Create symlink using relative path
      let relative_target = compute_relative_link(input_path, dep_store_path);
      create_dir_link(&relative_target, &link_path)?;

      debug!(
        dep = dep_name,
        target = %relative_target.display(),
        link = %link_path.display(),
        "linked dependency"
      );
    }

    Ok(())
  }

  /// Get the dependencies linked in an input's `.inputs/` directory.
  ///
  /// Returns a map of dependency names to their resolved store paths.
  pub fn get_linked_dependencies(&self, input_path: &Path) -> Result<BTreeMap<String, PathBuf>, StoreError> {
    let inputs_dir = input_path.join(INPUTS_DIR_NAME);

    if !inputs_dir.exists() {
      return Ok(BTreeMap::new());
    }

    let mut deps = BTreeMap::new();

    let entries = fs::read_dir(&inputs_dir).map_err(|e| StoreError::ReadSymlink {
      path: inputs_dir.clone(),
      source: e,
    })?;

    for entry in entries {
      let entry = entry.map_err(|e| StoreError::ReadSymlink {
        path: inputs_dir.clone(),
        source: e,
      })?;

      let name = entry.file_name().to_string_lossy().to_string();
      let link_path = entry.path();

      // Read the symlink/junction target and resolve to absolute path
      if link_path.symlink_metadata().is_ok() {
        // Use read_dir_link which handles both symlinks and junctions
        if let Some(target) = read_dir_link(&link_path) {
          // Resolve relative symlink to absolute path
          let resolved = if target.is_relative() {
            inputs_dir
              .join(&target)
              .canonicalize()
              .unwrap_or(inputs_dir.join(&target))
          } else {
            // For junctions, target is already absolute
            target.canonicalize().unwrap_or(target)
          };

          deps.insert(name, resolved);
        }
      }
    }

    Ok(deps)
  }

  /// Remove an input's `.inputs/` directory and all symlinks.
  pub fn unlink_dependencies(&self, input_path: &Path) -> Result<(), StoreError> {
    let inputs_dir = input_path.join(INPUTS_DIR_NAME);

    if inputs_dir.exists() {
      fs::remove_dir_all(&inputs_dir).map_err(|e| StoreError::Remove {
        path: inputs_dir,
        source: e,
      })?;
    }

    Ok(())
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

/// Compute the relative path from `.inputs/` directory to the dependency store path.
///
/// The link is created at `input_path/.inputs/<dep_name>`, and should point to
/// the dependency's store directory which is a sibling of `input_path` in the store.
fn compute_relative_link(input_path: &Path, to_dir: &Path) -> PathBuf {
  // Simple case: both are in the same store directory, use relative path
  // input_path/.inputs/dep -> ../../dep_store_name
  if let (Some(input_parent), Some(to_name)) = (input_path.parent(), to_dir.file_name())
    && let Some(to_parent) = to_dir.parent()
    && input_parent == to_parent
  {
    // Same parent directory (the store), use relative path
    // From input_path/.inputs/ go up twice then into sibling
    return PathBuf::from("../..").join(to_name);
  }

  // Fallback to absolute path if relative path can't be computed
  to_dir.to_path_buf()
}

/// Create a directory symlink (cross-platform).
#[cfg(unix)]
fn create_dir_link(target: &Path, link: &Path) -> Result<(), StoreError> {
  std::os::unix::fs::symlink(target, link).map_err(|e| StoreError::CreateSymlink {
    from: target.to_path_buf(),
    to: link.to_path_buf(),
    source: e,
  })
}

/// Create a directory symlink (Windows).
///
/// Tries symlink first (requires developer mode or admin), then junction,
/// then falls back to copying.
#[cfg(windows)]
fn create_dir_link(target: &Path, link: &Path) -> Result<(), StoreError> {
  eprintln!("[DEBUG] create_dir_link called");
  eprintln!("[DEBUG]   target: {}", target.display());
  eprintln!("[DEBUG]   link: {}", link.display());

  // First try symlink (may require elevated permissions)
  let symlink_result = std::os::windows::fs::symlink_dir(target, link);
  eprintln!("[DEBUG]   symlink_dir result: {:?}", symlink_result);
  if symlink_result.is_ok() {
    eprintln!("[DEBUG]   symlink succeeded, returning");
    return Ok(());
  }

  // For relative targets, we need to resolve to absolute for junction/copy
  let absolute_target = if target.is_relative() {
    if let Some(link_parent) = link.parent() {
      let joined = link_parent.join(target);
      eprintln!("[DEBUG]   relative target, link_parent: {}", link_parent.display());
      eprintln!("[DEBUG]   joined path: {}", joined.display());
      joined
    } else {
      target.to_path_buf()
    }
  } else {
    target.to_path_buf()
  };

  eprintln!("[DEBUG]   absolute_target (before canonicalize): {}", absolute_target.display());
  eprintln!("[DEBUG]   absolute_target exists: {}", absolute_target.exists());

  // Canonicalize to resolve ".." components (required for junctions)
  let canonical_result = absolute_target.canonicalize();
  eprintln!("[DEBUG]   canonicalize result: {:?}", canonical_result);
  let absolute_target = canonical_result.map_err(|e| StoreError::CreateSymlink {
    from: target.to_path_buf(),
    to: link.to_path_buf(),
    source: e,
  })?;

  eprintln!("[DEBUG]   absolute_target (after canonicalize): {}", absolute_target.display());

  // Try junction (works without admin on Windows 7+)
  let junction_result = junction::create(&absolute_target, link);
  eprintln!("[DEBUG]   junction::create result: {:?}", junction_result);
  if junction_result.is_ok() {
    eprintln!("[DEBUG]   junction succeeded, returning");
    return Ok(());
  }

  // Last resort: copy the directory
  eprintln!("[DEBUG]   trying copy_dir_all");
  copy_dir_all(&absolute_target, link).map_err(|e| StoreError::CopyDir {
    from: absolute_target.clone(),
    to: link.to_path_buf(),
    source: e,
  })?;

  eprintln!("[DEBUG]   copy succeeded");
  warn!(
    target = %target.display(),
    link = %link.display(),
    "fell back to copying directory (symlinks and junctions not available)"
  );

  Ok(())
}

/// Read the target of a directory link (symlink or junction) on Windows.
///
/// Returns `None` if the path is not a symlink or junction.
#[cfg(windows)]
fn read_dir_link(link: &Path) -> Option<PathBuf> {
  // First try reading as a standard symlink
  if let Ok(target) = fs::read_link(link) {
    return Some(target);
  }

  // Try reading as a junction
  if let Ok(target) = junction::get_target(link) {
    return Some(target);
  }

  None
}

/// Read the target of a directory link (symlink) on Unix.
#[cfg(unix)]
fn read_dir_link(link: &Path) -> Option<PathBuf> {
  fs::read_link(link).ok()
}

/// Copy a directory recursively.
#[cfg(windows)]
fn copy_dir_all(src: &Path, dst: &Path) -> io::Result<()> {
  fs::create_dir_all(dst)?;
  for entry in fs::read_dir(src)? {
    let entry = entry?;
    let ty = entry.file_type()?;
    let dst_path = dst.join(entry.file_name());
    if ty.is_dir() {
      copy_dir_all(&entry.path(), &dst_path)?;
    } else {
      fs::copy(entry.path(), dst_path)?;
    }
  }
  Ok(())
}

/// Remove a path (file, directory, or symlink).
fn remove_path(path: &Path) -> Result<(), StoreError> {
  if path.is_dir()
    && !path
      .symlink_metadata()
      .map(|m| m.file_type().is_symlink())
      .unwrap_or(false)
  {
    fs::remove_dir_all(path)
  } else {
    fs::remove_file(path)
  }
  .map_err(|e| StoreError::Remove {
    path: path.to_path_buf(),
    source: e,
  })
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

  mod link_dependencies {
    use super::*;

    #[test]
    fn creates_inputs_dir() {
      let temp = TempDir::new().unwrap();
      let store = InputStore::with_path(temp.path().to_path_buf());
      store.ensure_store_dir().unwrap();

      let input_path = store.compute_store_path("pkgs", "git:https://example.com/pkgs", "abc123");
      fs::create_dir_all(&input_path).unwrap();

      let mut deps = BTreeMap::new();
      let utils_path = store.compute_store_path("utils", "git:https://example.com/utils", "def456");
      fs::create_dir_all(&utils_path).unwrap();
      deps.insert("utils".to_string(), utils_path.clone());

      store.link_dependencies(&input_path, &deps).unwrap();

      assert!(input_path.join(".inputs").exists());
      assert!(input_path.join(".inputs").is_dir());
    }

    #[test]
    #[cfg(unix)]
    fn creates_symlinks() {
      let temp = TempDir::new().unwrap();
      let store = InputStore::with_path(temp.path().to_path_buf());
      store.ensure_store_dir().unwrap();

      let input_path = store.compute_store_path("pkgs", "git:https://example.com/pkgs", "abc123");
      fs::create_dir_all(&input_path).unwrap();

      let utils_path = store.compute_store_path("utils", "git:https://example.com/utils", "def456");
      fs::create_dir_all(&utils_path).unwrap();
      // Create a file in utils to verify the symlink works
      fs::write(utils_path.join("init.lua"), "return {}").unwrap();

      let mut deps = BTreeMap::new();
      deps.insert("utils".to_string(), utils_path.clone());

      store.link_dependencies(&input_path, &deps).unwrap();

      let link_path = input_path.join(".inputs/utils");
      assert!(link_path.symlink_metadata().unwrap().file_type().is_symlink());

      // Verify the symlink resolves correctly
      let resolved = link_path.canonicalize().unwrap();
      assert_eq!(resolved, utils_path.canonicalize().unwrap());

      // Verify we can read through the symlink
      assert!(link_path.join("init.lua").exists());
    }

    #[test]
    fn empty_deps_is_noop() {
      let temp = TempDir::new().unwrap();
      let store = InputStore::with_path(temp.path().to_path_buf());
      store.ensure_store_dir().unwrap();

      let input_path = store.compute_store_path("pkgs", "git:https://example.com/pkgs", "abc123");
      fs::create_dir_all(&input_path).unwrap();

      let deps = BTreeMap::new();
      store.link_dependencies(&input_path, &deps).unwrap();

      // .inputs directory should not be created for empty deps
      assert!(!input_path.join(".inputs").exists());
    }

    #[test]
    fn idempotent() {
      let temp = TempDir::new().unwrap();
      let store = InputStore::with_path(temp.path().to_path_buf());
      store.ensure_store_dir().unwrap();

      let input_path = store.compute_store_path("pkgs", "git:https://example.com/pkgs", "abc123");
      fs::create_dir_all(&input_path).unwrap();

      let utils_path = store.compute_store_path("utils", "git:https://example.com/utils", "def456");
      fs::create_dir_all(&utils_path).unwrap();

      let mut deps = BTreeMap::new();
      deps.insert("utils".to_string(), utils_path);

      // Link twice - should not error
      store.link_dependencies(&input_path, &deps).unwrap();
      store.link_dependencies(&input_path, &deps).unwrap();

      assert!(input_path.join(".inputs/utils").exists());
    }
  }

  mod get_linked_dependencies {
    use super::*;

    #[test]
    fn returns_empty_when_no_inputs_dir() {
      let temp = TempDir::new().unwrap();
      let store = InputStore::with_path(temp.path().to_path_buf());
      store.ensure_store_dir().unwrap();

      let input_path = store.compute_store_path("pkgs", "git:https://example.com/pkgs", "abc123");
      fs::create_dir_all(&input_path).unwrap();

      let deps = store.get_linked_dependencies(&input_path).unwrap();
      assert!(deps.is_empty());
    }

    #[test]
    #[cfg(unix)]
    fn returns_linked_deps() {
      let temp = TempDir::new().unwrap();
      let store = InputStore::with_path(temp.path().to_path_buf());
      store.ensure_store_dir().unwrap();

      let input_path = store.compute_store_path("pkgs", "git:https://example.com/pkgs", "abc123");
      fs::create_dir_all(&input_path).unwrap();

      let utils_path = store.compute_store_path("utils", "git:https://example.com/utils", "def456");
      fs::create_dir_all(&utils_path).unwrap();

      let mut deps = BTreeMap::new();
      deps.insert("utils".to_string(), utils_path.clone());

      store.link_dependencies(&input_path, &deps).unwrap();

      let got_deps = store.get_linked_dependencies(&input_path).unwrap();
      assert_eq!(got_deps.len(), 1);
      assert!(got_deps.contains_key("utils"));
    }
  }

  mod unlink_dependencies {
    use super::*;

    #[test]
    fn removes_inputs_dir() {
      let temp = TempDir::new().unwrap();
      let store = InputStore::with_path(temp.path().to_path_buf());
      store.ensure_store_dir().unwrap();

      let input_path = store.compute_store_path("pkgs", "git:https://example.com/pkgs", "abc123");
      fs::create_dir_all(&input_path).unwrap();

      let utils_path = store.compute_store_path("utils", "git:https://example.com/utils", "def456");
      fs::create_dir_all(&utils_path).unwrap();

      let mut deps = BTreeMap::new();
      deps.insert("utils".to_string(), utils_path);

      store.link_dependencies(&input_path, &deps).unwrap();
      assert!(input_path.join(".inputs").exists());

      store.unlink_dependencies(&input_path).unwrap();
      assert!(!input_path.join(".inputs").exists());
    }

    #[test]
    fn noop_when_no_inputs_dir() {
      let temp = TempDir::new().unwrap();
      let store = InputStore::with_path(temp.path().to_path_buf());
      store.ensure_store_dir().unwrap();

      let input_path = store.compute_store_path("pkgs", "git:https://example.com/pkgs", "abc123");
      fs::create_dir_all(&input_path).unwrap();

      // Should not error when .inputs doesn't exist
      store.unlink_dependencies(&input_path).unwrap();
    }
  }

  mod compute_relative_link {
    use super::*;

    #[test]
    fn same_parent_uses_relative() {
      let store_dir = PathBuf::from("/cache/inputs/store");
      let from = store_dir.join("pkgs-abc123");
      let to = store_dir.join("utils-def456");

      let relative = compute_relative_link(&from, &to);

      // From pkgs-abc123/.inputs/ go up twice (out of .inputs, out of pkgs) then into sibling
      assert_eq!(relative, PathBuf::from("../../utils-def456"));
    }
  }
}
