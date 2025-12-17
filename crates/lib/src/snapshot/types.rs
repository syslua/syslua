//! Snapshot types for syslua.
//!
//! Snapshots capture system state as a manifest of builds and binds.
//! They enable rollback, diff computation, and garbage collection.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::manifest::Manifest;

/// Current snapshot index format version.
pub const SNAPSHOT_INDEX_VERSION: u32 = 1;

/// A snapshot captures system state at a point in time.
///
/// Contains the full manifest of builds and binds that were applied,
/// enabling rollback and comparison between configurations.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
  /// Unique identifier (millisecond timestamp).
  pub id: String,

  /// Unix timestamp when the snapshot was created.
  pub created_at: u64,

  /// Path to the configuration file that produced this state.
  pub config_path: Option<PathBuf>,

  /// The manifest containing builds and binds.
  pub manifest: Manifest,
}

impl Snapshot {
  /// Create a new snapshot with the given manifest.
  pub fn new(id: String, config_path: Option<PathBuf>, manifest: Manifest) -> Self {
    Self {
      id,
      created_at: current_timestamp(),
      config_path,
      manifest,
    }
  }

  /// Get the number of builds in this snapshot.
  pub fn build_count(&self) -> usize {
    self.manifest.builds.len()
  }

  /// Get the number of binds in this snapshot.
  pub fn bind_count(&self) -> usize {
    self.manifest.bindings.len()
  }

  /// Convert to metadata (summary without full manifest).
  pub fn to_metadata(&self) -> SnapshotMetadata {
    SnapshotMetadata {
      id: self.id.clone(),
      created_at: self.created_at,
      config_path: self.config_path.clone(),
      build_count: self.build_count(),
      bind_count: self.bind_count(),
    }
  }
}

/// Summary information for a snapshot (without full manifest).
///
/// Used in the snapshot index for listing and quick lookups
/// without loading the entire manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotMetadata {
  /// Unique identifier (millisecond timestamp).
  pub id: String,

  /// Unix timestamp when the snapshot was created.
  pub created_at: u64,

  /// Path to the configuration file that produced this state.
  pub config_path: Option<PathBuf>,

  /// Number of builds in this snapshot.
  pub build_count: usize,

  /// Number of binds (activations) in this snapshot.
  pub bind_count: usize,
}

/// Index of all snapshots stored on disk.
///
/// The index tracks all snapshots and which one is currently active.
/// Stored as `snapshots/index.json`.
///
/// # Example
///
/// ```json
/// {
///   "version": 1,
///   "snapshots": [
///     {
///       "id": "1765208363188",
///       "created_at": 1733667300,
///       "build_count": 5,
///       "bind_count": 8
///     }
///   ],
///   "current": "1765208363188"
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SnapshotIndex {
  /// Index format version.
  pub version: u32,

  /// All snapshots, ordered by creation time (oldest first).
  pub snapshots: Vec<SnapshotMetadata>,

  /// ID of the currently active snapshot.
  pub current: Option<String>,
}

impl Default for SnapshotIndex {
  fn default() -> Self {
    Self::new()
  }
}

impl SnapshotIndex {
  /// Create a new empty index.
  pub fn new() -> Self {
    Self {
      version: SNAPSHOT_INDEX_VERSION,
      snapshots: Vec::new(),
      current: None,
    }
  }

  /// Add a snapshot to the index.
  ///
  /// Maintains chronological ordering by creation time.
  pub fn add(&mut self, metadata: SnapshotMetadata) {
    // Find insertion point to maintain sorted order
    let pos = self
      .snapshots
      .iter()
      .position(|s| s.created_at > metadata.created_at)
      .unwrap_or(self.snapshots.len());
    self.snapshots.insert(pos, metadata);
  }

  /// Remove a snapshot from the index by ID.
  pub fn remove(&mut self, id: &str) -> Option<SnapshotMetadata> {
    if let Some(pos) = self.snapshots.iter().position(|s| s.id == id) {
      let removed = self.snapshots.remove(pos);
      // Clear current if it was the removed snapshot
      if self.current.as_deref() == Some(id) {
        self.current = None;
      }
      Some(removed)
    } else {
      None
    }
  }

  /// Get snapshot metadata by ID.
  pub fn get(&self, id: &str) -> Option<&SnapshotMetadata> {
    self.snapshots.iter().find(|s| s.id == id)
  }

  /// Get the current snapshot metadata.
  pub fn get_current(&self) -> Option<&SnapshotMetadata> {
    self.current.as_ref().and_then(|id| self.get(id))
  }

  /// Set the current snapshot ID.
  ///
  /// Returns an error if the snapshot doesn't exist in the index.
  pub fn set_current(&mut self, id: &str) -> Result<(), SnapshotError> {
    if self.snapshots.iter().any(|s| s.id == id) {
      self.current = Some(id.to_string());
      Ok(())
    } else {
      Err(SnapshotError::NotFound(id.to_string()))
    }
  }

  /// Get the number of snapshots.
  pub fn len(&self) -> usize {
    self.snapshots.len()
  }

  /// Check if the index is empty.
  pub fn is_empty(&self) -> bool {
    self.snapshots.is_empty()
  }
}

/// Errors that can occur when working with snapshots.
#[derive(Debug, Error)]
pub enum SnapshotError {
  /// Failed to read a snapshot or index file.
  #[error("failed to read: {0}")]
  Read(#[source] std::io::Error),

  /// Failed to write a snapshot or index file.
  #[error("failed to write: {0}")]
  Write(#[source] std::io::Error),

  /// Failed to create the snapshots directory.
  #[error("failed to create directory: {0}")]
  CreateDir(#[source] std::io::Error),

  /// Failed to parse JSON.
  #[error("failed to parse: {0}")]
  Parse(#[source] serde_json::Error),

  /// Failed to serialize JSON.
  #[error("failed to serialize: {0}")]
  Serialize(#[source] serde_json::Error),

  /// Snapshot not found.
  #[error("snapshot not found: {0}")]
  NotFound(String),

  /// Unsupported index version.
  #[error("unsupported snapshot index version {0}, expected {SNAPSHOT_INDEX_VERSION}")]
  UnsupportedVersion(u32),
}

/// Get the current Unix timestamp in seconds.
fn current_timestamp() -> u64 {
  std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .expect("system time before Unix epoch")
    .as_secs()
}

/// Generate a new snapshot ID (millisecond timestamp).
pub fn generate_snapshot_id() -> String {
  std::time::SystemTime::now()
    .duration_since(std::time::UNIX_EPOCH)
    .expect("system time before Unix epoch")
    .as_millis()
    .to_string()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn snapshot_to_metadata() {
    let mut manifest = Manifest::default();
    manifest.builds.insert(
      crate::util::hash::ObjectHash("build1".to_string()),
      crate::build::BuildDef {
        name: "test-build".to_string(),
        version: None,
        inputs: None,
        apply_actions: vec![],
        outputs: None,
      },
    );

    let snapshot = Snapshot::new(
      "test123".to_string(),
      Some(PathBuf::from("/path/to/config.lua")),
      manifest,
    );

    let metadata = snapshot.to_metadata();
    assert_eq!(metadata.id, "test123");
    assert_eq!(metadata.build_count, 1);
    assert_eq!(metadata.bind_count, 0);
    assert_eq!(metadata.config_path, Some(PathBuf::from("/path/to/config.lua")));
  }

  #[test]
  fn snapshot_index_add_maintains_order() {
    let mut index = SnapshotIndex::new();

    // Add out of order
    index.add(SnapshotMetadata {
      id: "second".to_string(),
      created_at: 2000,
      config_path: None,
      build_count: 0,
      bind_count: 0,
    });
    index.add(SnapshotMetadata {
      id: "first".to_string(),
      created_at: 1000,
      config_path: None,
      build_count: 0,
      bind_count: 0,
    });
    index.add(SnapshotMetadata {
      id: "third".to_string(),
      created_at: 3000,
      config_path: None,
      build_count: 0,
      bind_count: 0,
    });

    assert_eq!(index.len(), 3);
    assert_eq!(index.snapshots[0].id, "first");
    assert_eq!(index.snapshots[1].id, "second");
    assert_eq!(index.snapshots[2].id, "third");
  }

  #[test]
  fn snapshot_index_remove() {
    let mut index = SnapshotIndex::new();
    index.add(SnapshotMetadata {
      id: "test".to_string(),
      created_at: 1000,
      config_path: None,
      build_count: 0,
      bind_count: 0,
    });
    index.current = Some("test".to_string());

    let removed = index.remove("test");
    assert!(removed.is_some());
    assert!(index.is_empty());
    assert!(index.current.is_none()); // Current cleared
  }

  #[test]
  fn snapshot_index_set_current() {
    let mut index = SnapshotIndex::new();
    index.add(SnapshotMetadata {
      id: "test".to_string(),
      created_at: 1000,
      config_path: None,
      build_count: 0,
      bind_count: 0,
    });

    assert!(index.set_current("test").is_ok());
    assert_eq!(index.current, Some("test".to_string()));

    // Setting non-existent ID fails
    assert!(index.set_current("nonexistent").is_err());
  }

  #[test]
  fn snapshot_index_get_current() {
    let mut index = SnapshotIndex::new();
    assert!(index.get_current().is_none());

    index.add(SnapshotMetadata {
      id: "test".to_string(),
      created_at: 1000,
      config_path: None,
      build_count: 5,
      bind_count: 3,
    });
    index.set_current("test").unwrap();

    let current = index.get_current().unwrap();
    assert_eq!(current.id, "test");
    assert_eq!(current.build_count, 5);
  }

  #[test]
  fn generate_snapshot_id_is_numeric() {
    let id = generate_snapshot_id();
    assert!(id.parse::<u128>().is_ok());
    assert!(id.len() >= 13); // Millisecond timestamps are at least 13 digits
  }
}
