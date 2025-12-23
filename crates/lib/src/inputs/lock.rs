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
//!   "root": "root",
//!   "nodes": {
//!     "root": {
//!       "inputs": {
//!         "utils": "utils-abc123",
//!         "pkgs": "pkgs-def456"
//!       }
//!     },
//!     "utils-abc123": {
//!       "type": "git",
//!       "url": "git:https://github.com/org/utils.git",
//!       "rev": "abc123...",
//!       "lastModified": 1733667300,
//!       "inputs": {}
//!     },
//!     "pkgs-def456": {
//!       "type": "git",
//!       "url": "git:https://github.com/org/pkgs.git",
//!       "rev": "def456...",
//!       "lastModified": 1733667400,
//!       "inputs": {
//!         "utils": "utils-abc123"
//!       }
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

use super::store::InputStore;
use super::types::LockNode;

/// Current lock file format version.
pub const LOCK_VERSION: u32 = 1;

/// Lock file name.
pub const LOCK_FILENAME: &str = "syslua.lock";

/// Root node label in lock files.
pub const ROOT_NODE_LABEL: &str = "root";

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

// =============================================================================
// Version 1 (Current) Types
// =============================================================================

/// A V1 lock file with graph-based transitive dependencies.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LockFileV1 {
  /// Lock file format version.
  pub version: u32,
  /// Label of the root node (always "root").
  pub root: String,
  /// All nodes in the dependency graph, keyed by label.
  pub nodes: BTreeMap<String, LockNode>,
}

impl Default for LockFileV1 {
  fn default() -> Self {
    Self::new()
  }
}

impl LockFileV1 {
  /// Create a new empty lock file.
  pub fn new() -> Self {
    let mut nodes = BTreeMap::new();
    nodes.insert(ROOT_NODE_LABEL.to_string(), LockNode::root(BTreeMap::new()));
    Self {
      version: LOCK_VERSION,
      root: ROOT_NODE_LABEL.to_string(),
      nodes,
    }
  }

  /// Get the root node.
  pub fn root_node(&self) -> Option<&LockNode> {
    self.nodes.get(&self.root)
  }

  /// Get a mutable reference to the root node.
  pub fn root_node_mut(&mut self) -> Option<&mut LockNode> {
    let root = self.root.clone();
    self.nodes.get_mut(&root)
  }

  /// Get a node by its label.
  pub fn get_node(&self, label: &str) -> Option<&LockNode> {
    self.nodes.get(label)
  }

  /// Insert or update a node.
  pub fn insert_node(&mut self, label: String, node: LockNode) {
    self.nodes.insert(label, node);
  }

  /// Add a direct input to the root node.
  ///
  /// # Arguments
  ///
  /// * `name` - The input name (as declared in config)
  /// * `url` - The input URL
  /// * `rev` - The resolved revision
  /// * `type_` - The input type ("git" or "path")
  /// * `last_modified` - Optional last modified timestamp
  pub fn add_root_input(&mut self, name: &str, url: &str, rev: &str, type_: &str, last_modified: Option<u64>) {
    let label = InputStore::compute_store_label(name, url, rev);

    // Add to root's inputs
    if let Some(root) = self.root_node_mut() {
      root.inputs.insert(name.to_string(), label.clone());
    }

    // Add the node if it doesn't exist
    self
      .nodes
      .entry(label)
      .or_insert_with(|| LockNode::input(type_, url, rev, last_modified, BTreeMap::new()));
  }

  /// Add a transitive input (dependency of another input).
  ///
  /// # Arguments
  ///
  /// * `parent_label` - The label of the parent node
  /// * `dep_name` - The dependency name (as declared in parent's inputs)
  /// * `url` - The dependency URL
  /// * `rev` - The resolved revision
  /// * `type_` - The input type ("git" or "path")
  /// * `last_modified` - Optional last modified timestamp
  pub fn add_transitive_input(
    &mut self,
    parent_label: &str,
    dep_name: &str,
    url: &str,
    rev: &str,
    type_: &str,
    last_modified: Option<u64>,
  ) {
    // Compute the label from the dep_name (not parent path)
    let label = InputStore::compute_store_label(dep_name, url, rev);

    // Add to parent's inputs
    if let Some(parent) = self.nodes.get_mut(parent_label) {
      parent.inputs.insert(dep_name.to_string(), label.clone());
    }

    // Add the node if it doesn't exist
    self
      .nodes
      .entry(label)
      .or_insert_with(|| LockNode::input(type_, url, rev, last_modified, BTreeMap::new()));
  }

  /// Get the label for a root input by name.
  pub fn get_root_input_label(&self, name: &str) -> Option<&str> {
    self
      .root_node()
      .and_then(|root| root.inputs.get(name).map(|s| s.as_str()))
  }

  /// Get a root input by name.
  pub fn get_root_input(&self, name: &str) -> Option<&LockNode> {
    self.get_root_input_label(name).and_then(|label| self.nodes.get(label))
  }

  /// Get all root input names.
  pub fn root_input_names(&self) -> Vec<&str> {
    self
      .root_node()
      .map(|root| root.inputs.keys().map(|s| s.as_str()).collect())
      .unwrap_or_default()
  }

  /// Remove a root input and clean up orphaned nodes.
  pub fn remove_root_input(&mut self, name: &str) -> bool {
    if let Some(root) = self.root_node_mut()
      && root.inputs.remove(name).is_some()
    {
      self.remove_orphaned_nodes();
      return true;
    }
    false
  }

  /// Collect all node labels reachable from the root.
  ///
  /// Performs a depth-first traversal of the dependency graph starting from
  /// the root node. Returns a set of all reachable node labels.
  fn collect_reachable_nodes(&self) -> std::collections::HashSet<String> {
    use std::collections::HashSet;

    let mut reachable = HashSet::new();
    let mut stack = vec![self.root.clone()];

    while let Some(label) = stack.pop() {
      if reachable.insert(label.clone())
        && let Some(node) = self.nodes.get(&label)
      {
        for dep_label in node.inputs.values() {
          stack.push(dep_label.clone());
        }
      }
    }

    reachable
  }

  /// Remove nodes that are no longer reachable from the root.
  ///
  /// This cleans up orphaned transitive dependencies that are no longer
  /// referenced after a root input is removed.
  ///
  /// Returns the number of nodes removed.
  pub fn remove_orphaned_nodes(&mut self) -> usize {
    let reachable = self.collect_reachable_nodes();
    let all_labels: Vec<String> = self.nodes.keys().cloned().collect();
    let mut removed = 0;

    for label in all_labels {
      if !reachable.contains(&label) {
        self.nodes.remove(&label);
        removed += 1;
      }
    }

    removed
  }
}

// =============================================================================
// Unified Lock File (handles both versions)
// =============================================================================

/// A lock file that can be any version.
///
/// This is the main type used by the resolution system. It reads all formats, but only
/// writes the latest version.
#[derive(Debug, Clone, PartialEq)]
pub struct LockFile {
  inner: LockFileV1,
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
      inner: LockFileV1::new(),
    }
  }

  /// Create a lock file from a V1 structure.
  pub fn from_v1(v1: LockFileV1) -> Self {
    Self { inner: v1 }
  }

  /// Get a reference to the underlying V1 structure.
  pub fn as_v1(&self) -> &LockFileV1 {
    &self.inner
  }

  /// Get a mutable reference to the underlying V1 structure.
  pub fn as_v1_mut(&mut self) -> &mut LockFileV1 {
    &mut self.inner
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

    // First, try to parse as a generic value to check version
    let value: serde_json::Value = serde_json::from_str(&content).map_err(LockError::Parse)?;

    let version = value.get("version").and_then(|v| v.as_u64()).unwrap_or(0) as u32;

    if version != LOCK_VERSION {
      return Err(LockError::UnsupportedVersion(version));
    }

    let v1: LockFileV1 = serde_json::from_value(value).map_err(LockError::Parse)?;
    Ok(Some(Self::from_v1(v1)))
  }

  /// Save the lock file to the given path.
  ///
  /// Always writes latest format.
  pub fn save(&self, path: &Path) -> Result<(), LockError> {
    let content = serde_json::to_string_pretty(&self.inner).map_err(LockError::Serialize)?;
    fs::write(path, content).map_err(LockError::Write)?;
    Ok(())
  }

  // ==========================================================================
  // Compatibility API (for existing code that uses V1-style access)
  // ==========================================================================

  /// Get a locked input by name (V1 compatibility).
  ///
  /// Returns the LockedInput if found, or None.
  pub fn get(&self, name: &str) -> Option<LockedInput> {
    self.inner.get_root_input(name).and_then(|node| {
      if node.is_root() {
        None
      } else {
        Some(LockedInput {
          type_: node.type_.clone().unwrap_or_default(),
          url: node.url.clone().unwrap_or_default(),
          rev: node.rev.clone().unwrap_or_default(),
          last_modified: node.last_modified,
        })
      }
    })
  }

  /// Insert or update a locked input (V1 compatibility).
  ///
  /// This adds/updates a root input in the lock file.
  pub fn insert(&mut self, name: String, input: LockedInput) {
    self
      .inner
      .add_root_input(&name, &input.url, &input.rev, &input.type_, input.last_modified);
  }

  /// Get all input names (for backwards compatibility).
  pub fn input_names(&self) -> Vec<String> {
    self.inner.root_input_names().iter().map(|s| s.to_string()).collect()
  }

  /// Access the inputs map for backwards compatibility.
  ///
  /// Note: This recreates a flat view from the graph structure.
  pub fn inputs(&self) -> BTreeMap<String, LockedInput> {
    let mut map = BTreeMap::new();
    for name in self.inner.root_input_names() {
      if let Some(input) = self.get(name) {
        map.insert(name.to_string(), input);
      }
    }
    map
  }

  /// Remove a root input by name.
  pub fn remove(&mut self, name: &str) -> bool {
    self.inner.remove_root_input(name)
  }
}

/// Load a lock file from an input's directory.
///
/// This is used to load per-input lock files (`<input>/syslua.lock`) that
/// pin an input's transitive dependencies.
///
/// # Arguments
///
/// * `input_path` - Path to the input's root directory
///
/// # Returns
///
/// `Some(LockFile)` if a lock file exists and is valid, `None` otherwise.
/// Errors during parsing are logged and treated as missing lock file.
pub fn load_input_lock(input_path: &Path) -> Option<LockFile> {
  let lock_path = input_path.join(LOCK_FILENAME);
  match LockFile::load(&lock_path) {
    Ok(Some(lock)) => Some(lock),
    Ok(None) => None,
    Err(e) => {
      tracing::warn!(
        path = %lock_path.display(),
        error = %e,
        "failed to load input lock file, treating as unlocked"
      );
      None
    }
  }
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

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  mod locked_input {
    use super::*;

    #[test]
    fn new_creates_input() {
      let input = LockedInput::new("git", "git:https://example.com/repo.git", "abc123");
      assert_eq!(input.type_, "git");
      assert_eq!(input.url, "git:https://example.com/repo.git");
      assert_eq!(input.rev, "abc123");
      assert!(input.last_modified.is_none());
    }

    #[test]
    fn with_last_modified() {
      let input = LockedInput::new("git", "git:https://example.com", "abc").with_last_modified(12345);
      assert_eq!(input.last_modified, Some(12345));
    }
  }

  mod lock_file_v1 {
    use super::*;

    #[test]
    fn new_creates_root_node() {
      let lock = LockFileV1::new();
      assert_eq!(lock.version, LOCK_VERSION);
      assert_eq!(lock.root, ROOT_NODE_LABEL);
      assert!(lock.root_node().is_some());
      assert!(lock.root_node().unwrap().is_root());
    }

    #[test]
    fn add_root_input() {
      let mut lock = LockFileV1::new();
      lock.add_root_input("pkgs", "git:https://example.com/pkgs", "abc123", "git", Some(12345));

      assert!(lock.get_root_input("pkgs").is_some());
      let node = lock.get_root_input("pkgs").unwrap();
      assert_eq!(node.url.as_deref(), Some("git:https://example.com/pkgs"));
      assert_eq!(node.rev.as_deref(), Some("abc123"));
    }

    #[test]
    fn add_transitive_input() {
      let mut lock = LockFileV1::new();
      lock.add_root_input("pkgs", "git:https://example.com/pkgs", "abc123", "git", None);

      let pkgs_label = lock.get_root_input_label("pkgs").unwrap().to_string();
      lock.add_transitive_input(
        &pkgs_label,
        "utils",
        "git:https://example.com/utils",
        "def456",
        "git",
        None,
      );

      let pkgs_node = lock.get_node(&pkgs_label).unwrap();
      assert!(pkgs_node.inputs.contains_key("utils"));

      let utils_label = pkgs_node.inputs.get("utils").unwrap();
      let utils_node = lock.get_node(utils_label).unwrap();
      assert_eq!(utils_node.url.as_deref(), Some("git:https://example.com/utils"));
    }

    #[test]
    fn root_input_names() {
      let mut lock = LockFileV1::new();
      lock.add_root_input("pkgs", "git:a", "abc", "git", None);
      lock.add_root_input("utils", "git:b", "def", "git", None);

      let names = lock.root_input_names();
      assert_eq!(names.len(), 2);
      assert!(names.contains(&"pkgs"));
      assert!(names.contains(&"utils"));
    }

    #[test]
    fn remove_root_input() {
      let mut lock = LockFileV1::new();
      lock.add_root_input("pkgs", "git:a", "abc", "git", None);

      assert!(lock.remove_root_input("pkgs"));
      assert!(lock.get_root_input("pkgs").is_none());
      assert!(!lock.remove_root_input("pkgs")); // Already removed
    }

    #[test]
    fn remove_root_input_cleans_up_orphaned_transitive_deps() {
      let mut lock = LockFileV1::new();

      // Add pkgs which depends on utils
      lock.add_root_input("pkgs", "git:https://example.com/pkgs", "abc123", "git", None);
      let pkgs_label = lock.get_root_input_label("pkgs").unwrap().to_string();
      lock.add_transitive_input(
        &pkgs_label,
        "utils",
        "git:https://example.com/utils",
        "def456",
        "git",
        None,
      );

      // Verify utils node exists
      let utils_label = lock.get_node(&pkgs_label).unwrap().inputs.get("utils").unwrap().clone();
      assert!(lock.get_node(&utils_label).is_some());

      // Remove pkgs - should also remove orphaned utils
      assert!(lock.remove_root_input("pkgs"));

      // pkgs node should be gone
      assert!(lock.get_node(&pkgs_label).is_none());
      // utils node should also be gone (orphaned)
      assert!(lock.get_node(&utils_label).is_none());
      // Only root should remain
      assert_eq!(lock.nodes.len(), 1);
      assert!(lock.nodes.contains_key(ROOT_NODE_LABEL));
    }

    #[test]
    fn remove_root_input_preserves_shared_deps() {
      let mut lock = LockFileV1::new();

      // Add pkgs_a which depends on utils
      lock.add_root_input("pkgs_a", "git:https://example.com/pkgs_a", "aaa", "git", None);
      let pkgs_a_label = lock.get_root_input_label("pkgs_a").unwrap().to_string();
      lock.add_transitive_input(
        &pkgs_a_label,
        "utils",
        "git:https://example.com/utils",
        "def456",
        "git",
        None,
      );

      // Add pkgs_b which also depends on the same utils
      lock.add_root_input("pkgs_b", "git:https://example.com/pkgs_b", "bbb", "git", None);
      let pkgs_b_label = lock.get_root_input_label("pkgs_b").unwrap().to_string();
      lock.add_transitive_input(
        &pkgs_b_label,
        "utils",
        "git:https://example.com/utils",
        "def456",
        "git",
        None,
      );

      // Get the shared utils label
      let utils_label = lock
        .get_node(&pkgs_a_label)
        .unwrap()
        .inputs
        .get("utils")
        .unwrap()
        .clone();

      // Remove pkgs_a
      assert!(lock.remove_root_input("pkgs_a"));

      // pkgs_a should be gone
      assert!(lock.get_node(&pkgs_a_label).is_none());
      // utils should still exist (still referenced by pkgs_b)
      assert!(lock.get_node(&utils_label).is_some());
      // pkgs_b should still exist
      assert!(lock.get_root_input("pkgs_b").is_some());
    }

    #[test]
    fn remove_orphaned_nodes_handles_deep_chains() {
      let mut lock = LockFileV1::new();

      // Create a chain: root -> pkgs -> lib_a -> lib_b -> lib_c
      lock.add_root_input("pkgs", "git:https://example.com/pkgs", "abc", "git", None);
      let pkgs_label = lock.get_root_input_label("pkgs").unwrap().to_string();

      lock.add_transitive_input(
        &pkgs_label,
        "lib_a",
        "git:https://example.com/lib_a",
        "aaa",
        "git",
        None,
      );
      let lib_a_label = lock.get_node(&pkgs_label).unwrap().inputs.get("lib_a").unwrap().clone();

      lock.add_transitive_input(
        &lib_a_label,
        "lib_b",
        "git:https://example.com/lib_b",
        "bbb",
        "git",
        None,
      );
      let lib_b_label = lock
        .get_node(&lib_a_label)
        .unwrap()
        .inputs
        .get("lib_b")
        .unwrap()
        .clone();

      lock.add_transitive_input(
        &lib_b_label,
        "lib_c",
        "git:https://example.com/lib_c",
        "ccc",
        "git",
        None,
      );
      let lib_c_label = lock
        .get_node(&lib_b_label)
        .unwrap()
        .inputs
        .get("lib_c")
        .unwrap()
        .clone();

      // Verify all nodes exist (root + pkgs + lib_a + lib_b + lib_c = 5)
      assert_eq!(lock.nodes.len(), 5);

      // Remove pkgs - entire chain should be cleaned up
      assert!(lock.remove_root_input("pkgs"));

      // Only root should remain
      assert_eq!(lock.nodes.len(), 1);
      assert!(lock.nodes.contains_key(ROOT_NODE_LABEL));
      assert!(lock.get_node(&pkgs_label).is_none());
      assert!(lock.get_node(&lib_a_label).is_none());
      assert!(lock.get_node(&lib_b_label).is_none());
      assert!(lock.get_node(&lib_c_label).is_none());
    }
  }

  mod lock_file {
    use super::*;

    #[test]
    fn new_creates_empty() {
      let lock = LockFile::new();
      assert!(lock.input_names().is_empty());
    }

    #[test]
    fn insert_and_get() {
      let mut lock = LockFile::new();
      lock.insert(
        "pkgs".to_string(),
        LockedInput::new("git", "git:https://example.com", "abc123"),
      );

      let input = lock.get("pkgs").unwrap();
      assert_eq!(input.type_, "git");
      assert_eq!(input.rev, "abc123");
    }

    #[test]
    fn save_and_load_roundtrip() {
      let temp_dir = TempDir::new().unwrap();
      let lock_path = temp_dir.path().join(LOCK_FILENAME);

      let mut original = LockFile::new();
      original.insert(
        "pkgs".to_string(),
        LockedInput::new("git", "git:https://github.com/org/repo.git", "a1b2c3d4").with_last_modified(1733667300),
      );

      original.save(&lock_path).unwrap();
      let loaded = LockFile::load(&lock_path).unwrap().unwrap();

      // Compare the inputs
      let orig_pkgs = original.get("pkgs").unwrap();
      let load_pkgs = loaded.get("pkgs").unwrap();
      assert_eq!(orig_pkgs.url, load_pkgs.url);
      assert_eq!(orig_pkgs.rev, load_pkgs.rev);
      assert_eq!(orig_pkgs.last_modified, load_pkgs.last_modified);
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
    fn v1_json_format() {
      let mut lock = LockFileV1::new();
      lock.add_root_input(
        "pkgs",
        "git:https://example.com/pkgs",
        "abc123",
        "git",
        Some(1234567890),
      );

      let json = serde_json::to_string_pretty(&lock).unwrap();

      assert!(json.contains(r#""version": 1"#));
      assert!(json.contains(r#""root": "root""#));
      assert!(json.contains(r#""nodes""#));
      assert!(json.contains(r#""type": "git""#));
      assert!(json.contains(r#""lastModified": 1234567890"#));
    }
  }
}
