//! Snapshot storage for syslua.
//!
//! Handles reading and writing snapshots to disk.
//!
//! # Storage Layout
//!
//! ```text
//! {data_dir}/snapshots/
//! ├── index.json          # SnapshotIndex: list + current pointer
//! └── <id>.json           # Individual Snapshot files
//! ```

use std::fs;
use std::io;
use std::path::PathBuf;

use crate::platform::paths::data_dir;
use crate::platform::paths::root_dir;

use super::types::{
  SNAPSHOT_INDEX_VERSION, Snapshot, SnapshotError, SnapshotIndex, SnapshotMetadata, generate_snapshot_id,
};

/// Directory name for snapshots within the data directory.
const SNAPSHOTS_DIR: &str = "snapshots";

/// Index file name.
const INDEX_FILENAME: &str = "index.json";

/// Manages snapshot storage on disk.
///
/// Provides operations for saving, loading, and listing snapshots.
/// Uses atomic write operations to prevent corruption.
#[derive(Debug, Clone)]
pub struct SnapshotStore {
  /// Base path for snapshot storage (e.g., `~/.local/share/syslua/snapshots`).
  base_path: PathBuf,
}

impl SnapshotStore {
  /// Create a new snapshot store at the given base path.
  pub fn new(base_path: PathBuf) -> Self {
    Self { base_path }
  }

  /// Get the base path of this store (for debugging).
  pub fn base_path(&self) -> &PathBuf {
    &self.base_path
  }

  /// Create a snapshot store at the default location.
  ///
  /// Uses the platform-specific data directory:
  /// - Linux/macOS: `~/.local/share/syslua/snapshots`
  /// - Windows: `%APPDATA%\syslua\snapshots`
  pub fn default_store(system: &bool) -> Self {
    if *system {
      return Self::new(root_dir().join(SNAPSHOTS_DIR));
    }
    Self::new(data_dir().join(SNAPSHOTS_DIR))
  }

  /// Get the path to the index file.
  fn index_path(&self) -> PathBuf {
    self.base_path.join(INDEX_FILENAME)
  }

  /// Get the path to a snapshot file by ID.
  fn snapshot_path(&self, id: &str) -> PathBuf {
    self.base_path.join(format!("{}.json", id))
  }

  /// Ensure the snapshots directory exists.
  fn ensure_dir(&self) -> Result<(), SnapshotError> {
    fs::create_dir_all(&self.base_path).map_err(SnapshotError::CreateDir)
  }

  /// Load the snapshot index.
  ///
  /// Returns an empty index if the file doesn't exist.
  pub fn load_index(&self) -> Result<SnapshotIndex, SnapshotError> {
    let path = self.index_path();

    let content = match fs::read_to_string(&path) {
      Ok(content) => content,
      Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(SnapshotIndex::new()),
      Err(e) => return Err(SnapshotError::Read(e)),
    };

    let index: SnapshotIndex = serde_json::from_str(&content).map_err(SnapshotError::Parse)?;

    if index.version != SNAPSHOT_INDEX_VERSION {
      return Err(SnapshotError::UnsupportedVersion(index.version));
    }

    Ok(index)
  }

  /// Save the snapshot index.
  ///
  /// Uses atomic write (write to temp, then rename) to prevent corruption.
  fn save_index(&self, index: &SnapshotIndex) -> Result<(), SnapshotError> {
    self.ensure_dir()?;

    let path = self.index_path();
    let temp_path = self.base_path.join("index.json.tmp");

    let content = serde_json::to_string_pretty(index).map_err(SnapshotError::Serialize)?;
    fs::write(&temp_path, &content).map_err(SnapshotError::Write)?;
    fs::rename(&temp_path, &path).map_err(SnapshotError::Write)?;

    Ok(())
  }

  /// Get the current snapshot ID.
  pub fn current_id(&self) -> Result<Option<String>, SnapshotError> {
    let index = self.load_index()?;
    Ok(index.current)
  }

  /// Load the current snapshot.
  ///
  /// Returns `Ok(None)` if no snapshot has been applied yet.
  pub fn load_current(&self) -> Result<Option<Snapshot>, SnapshotError> {
    let index = self.load_index()?;
    match index.current {
      Some(id) => Ok(Some(self.load_snapshot(&id)?)),
      None => Ok(None),
    }
  }

  /// Load a snapshot by ID.
  pub fn load_snapshot(&self, id: &str) -> Result<Snapshot, SnapshotError> {
    let path = self.snapshot_path(id);

    let content = fs::read_to_string(&path).map_err(|e| {
      if e.kind() == io::ErrorKind::NotFound {
        SnapshotError::NotFound(id.to_string())
      } else {
        SnapshotError::Read(e)
      }
    })?;

    let snapshot: Snapshot = serde_json::from_str(&content).map_err(SnapshotError::Parse)?;
    Ok(snapshot)
  }

  /// Save a snapshot.
  ///
  /// Writes the snapshot file and updates the index.
  /// Does NOT set the snapshot as current - use `set_current` for that.
  pub fn save_snapshot(&self, snapshot: &Snapshot) -> Result<(), SnapshotError> {
    self.ensure_dir()?;

    // Write snapshot file atomically
    let path = self.snapshot_path(&snapshot.id);
    let temp_path = self.base_path.join(format!("{}.json.tmp", snapshot.id));

    let content = serde_json::to_string_pretty(snapshot).map_err(SnapshotError::Serialize)?;
    fs::write(&temp_path, &content).map_err(SnapshotError::Write)?;
    fs::rename(&temp_path, &path).map_err(SnapshotError::Write)?;

    // Update index
    let mut index = self.load_index()?;
    index.add(snapshot.to_metadata());
    self.save_index(&index)?;

    Ok(())
  }

  /// Save a snapshot and set it as current.
  ///
  /// This is a convenience method that combines `save_snapshot` and `set_current`.
  pub fn save_and_set_current(&self, snapshot: &Snapshot) -> Result<(), SnapshotError> {
    self.ensure_dir()?;

    // Write snapshot file atomically
    let path = self.snapshot_path(&snapshot.id);
    let temp_path = self.base_path.join(format!("{}.json.tmp", snapshot.id));

    let content = serde_json::to_string_pretty(snapshot).map_err(SnapshotError::Serialize)?;
    fs::write(&temp_path, &content).map_err(SnapshotError::Write)?;
    fs::rename(&temp_path, &path).map_err(SnapshotError::Write)?;

    // Update index and set current
    let mut index = self.load_index()?;
    index.add(snapshot.to_metadata());
    index.current = Some(snapshot.id.clone());
    self.save_index(&index)?;

    Ok(())
  }

  /// Set the current snapshot by ID.
  ///
  /// Returns an error if the snapshot doesn't exist.
  pub fn set_current(&self, id: &str) -> Result<(), SnapshotError> {
    // Verify snapshot exists
    if !self.snapshot_path(id).exists() {
      return Err(SnapshotError::NotFound(id.to_string()));
    }

    let mut index = self.load_index()?;
    index.set_current(id)?;
    self.save_index(&index)?;

    Ok(())
  }

  /// Clear the current snapshot pointer without removing any snapshots.
  ///
  /// This is used when rollback fails and the system is in an inconsistent state.
  /// The next apply will see no current state and do a full fresh apply (self-healing).
  pub fn clear_current(&self) -> Result<(), SnapshotError> {
    let mut index = self.load_index()?;
    index.current = None;
    self.save_index(&index)
  }

  /// List all snapshots.
  ///
  /// Returns snapshots in chronological order (oldest first).
  pub fn list(&self) -> Result<Vec<SnapshotMetadata>, SnapshotError> {
    let index = self.load_index()?;
    Ok(index.snapshots)
  }

  /// Delete a snapshot by ID.
  ///
  /// Removes the snapshot file and updates the index.
  /// If the deleted snapshot was current, clears the current pointer.
  pub fn delete_snapshot(&self, id: &str) -> Result<(), SnapshotError> {
    let path = self.snapshot_path(id);

    // Remove file (ignore if not found)
    match fs::remove_file(&path) {
      Ok(()) => {}
      Err(e) if e.kind() == io::ErrorKind::NotFound => {}
      Err(e) => return Err(SnapshotError::Write(e)),
    }

    // Update index
    let mut index = self.load_index()?;
    index.remove(id);
    self.save_index(&index)?;

    Ok(())
  }

  /// Generate a new unique snapshot ID.
  pub fn generate_id() -> String {
    generate_snapshot_id()
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::manifest::Manifest;
  use tempfile::TempDir;

  fn temp_store() -> (TempDir, SnapshotStore) {
    let temp_dir = TempDir::new().unwrap();
    let store = SnapshotStore::new(temp_dir.path().to_path_buf());
    (temp_dir, store)
  }

  fn make_snapshot(id: &str) -> Snapshot {
    Snapshot::new(id.to_string(), None, Manifest::default())
  }

  #[test]
  fn load_index_empty_when_not_exists() {
    let (_temp, store) = temp_store();
    let index = store.load_index().unwrap();
    assert!(index.is_empty());
    assert!(index.current.is_none());
  }

  #[test]
  fn save_and_load_snapshot_roundtrip() {
    let (_temp, store) = temp_store();
    let snapshot = make_snapshot("test123");

    store.save_snapshot(&snapshot).unwrap();
    let loaded = store.load_snapshot("test123").unwrap();

    assert_eq!(snapshot.id, loaded.id);
    assert_eq!(snapshot.manifest, loaded.manifest);
  }

  #[test]
  fn load_snapshot_not_found() {
    let (_temp, store) = temp_store();
    let result = store.load_snapshot("nonexistent");
    assert!(matches!(result, Err(SnapshotError::NotFound(_))));
  }

  #[test]
  fn save_updates_index() {
    let (_temp, store) = temp_store();
    let snapshot = make_snapshot("test123");

    store.save_snapshot(&snapshot).unwrap();

    let index = store.load_index().unwrap();
    assert_eq!(index.len(), 1);
    assert_eq!(index.snapshots[0].id, "test123");
  }

  #[test]
  fn save_and_set_current() {
    let (_temp, store) = temp_store();
    let snapshot = make_snapshot("test123");

    store.save_and_set_current(&snapshot).unwrap();

    let index = store.load_index().unwrap();
    assert_eq!(index.current, Some("test123".to_string()));
  }

  #[test]
  fn load_current_when_none() {
    let (_temp, store) = temp_store();
    let result = store.load_current().unwrap();
    assert!(result.is_none());
  }

  #[test]
  fn load_current_after_set() {
    let (_temp, store) = temp_store();
    let snapshot = make_snapshot("test123");

    store.save_and_set_current(&snapshot).unwrap();
    let loaded = store.load_current().unwrap().unwrap();

    assert_eq!(loaded.id, "test123");
  }

  #[test]
  fn set_current_validates_exists() {
    let (_temp, store) = temp_store();
    let result = store.set_current("nonexistent");
    assert!(matches!(result, Err(SnapshotError::NotFound(_))));
  }

  #[test]
  fn list_snapshots_in_order() {
    let (_temp, store) = temp_store();

    // Create snapshots with different timestamps by manipulating created_at
    let mut snap1 = make_snapshot("first");
    snap1.created_at = 1000;
    let mut snap2 = make_snapshot("second");
    snap2.created_at = 2000;
    let mut snap3 = make_snapshot("third");
    snap3.created_at = 3000;

    // Save out of order
    store.save_snapshot(&snap2).unwrap();
    store.save_snapshot(&snap1).unwrap();
    store.save_snapshot(&snap3).unwrap();

    let list = store.list().unwrap();
    assert_eq!(list.len(), 3);
    assert_eq!(list[0].id, "first");
    assert_eq!(list[1].id, "second");
    assert_eq!(list[2].id, "third");
  }

  #[test]
  fn delete_snapshot() {
    let (_temp, store) = temp_store();
    let snapshot = make_snapshot("test123");

    store.save_and_set_current(&snapshot).unwrap();
    store.delete_snapshot("test123").unwrap();

    let index = store.load_index().unwrap();
    assert!(index.is_empty());
    assert!(index.current.is_none());

    // File should be gone
    assert!(store.load_snapshot("test123").is_err());
  }

  #[test]
  fn delete_nonexistent_succeeds() {
    let (_temp, store) = temp_store();
    // Should not error
    store.delete_snapshot("nonexistent").unwrap();
  }

  #[test]
  fn generate_id_is_unique() {
    let id1 = SnapshotStore::generate_id();
    std::thread::sleep(std::time::Duration::from_millis(2));
    let id2 = SnapshotStore::generate_id();
    assert_ne!(id1, id2);
  }

  #[test]
  fn multiple_snapshots_same_store() {
    let (_temp, store) = temp_store();

    store.save_snapshot(&make_snapshot("snap1")).unwrap();
    store.save_snapshot(&make_snapshot("snap2")).unwrap();
    store.save_snapshot(&make_snapshot("snap3")).unwrap();

    let list = store.list().unwrap();
    assert_eq!(list.len(), 3);

    // Each can be loaded independently
    assert!(store.load_snapshot("snap1").is_ok());
    assert!(store.load_snapshot("snap2").is_ok());
    assert!(store.load_snapshot("snap3").is_ok());
  }

  #[test]
  fn clear_current_removes_pointer() {
    let (_temp, store) = temp_store();
    let snapshot = make_snapshot("test123");

    // Set up a current snapshot
    store.save_and_set_current(&snapshot).unwrap();
    assert_eq!(store.current_id().unwrap(), Some("test123".to_string()));

    // Clear the current pointer
    store.clear_current().unwrap();

    // Current should be None, but snapshot should still exist
    assert!(store.current_id().unwrap().is_none());
    assert!(store.load_current().unwrap().is_none());
    assert!(store.load_snapshot("test123").is_ok());

    // Index should still contain the snapshot
    let list = store.list().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "test123");
  }

  // Corrupt snapshot handling tests

  #[test]
  fn load_index_handles_corrupted_json() {
    let (temp, store) = temp_store();

    // Create the snapshots directory and write corrupted index
    fs::create_dir_all(&store.base_path).unwrap();
    let index_path = temp.path().join(INDEX_FILENAME);
    fs::write(&index_path, "not valid json {{{").unwrap();

    // Should return an error, not panic
    let result = store.load_index();
    assert!(result.is_err());
  }

  #[test]
  fn load_index_handles_wrong_schema() {
    let (temp, store) = temp_store();

    fs::create_dir_all(&store.base_path).unwrap();
    let index_path = temp.path().join(INDEX_FILENAME);
    // Valid JSON but wrong structure (missing version and snapshots fields)
    fs::write(&index_path, r#"{"foo": "bar"}"#).unwrap();

    let result = store.load_index();
    assert!(result.is_err());
  }

  #[test]
  fn load_index_handles_empty_file() {
    let (temp, store) = temp_store();

    fs::create_dir_all(&store.base_path).unwrap();
    let index_path = temp.path().join(INDEX_FILENAME);
    fs::write(&index_path, "").unwrap();

    let result = store.load_index();
    assert!(result.is_err());
  }

  #[test]
  fn load_index_handles_unsupported_version() {
    let (temp, store) = temp_store();

    fs::create_dir_all(&store.base_path).unwrap();
    let index_path = temp.path().join(INDEX_FILENAME);
    // Valid index structure but unsupported version
    fs::write(&index_path, r#"{"version": 99999, "snapshots": [], "current": null}"#).unwrap();

    let result = store.load_index();
    assert!(matches!(result, Err(SnapshotError::UnsupportedVersion(99999))));
  }

  #[test]
  fn load_current_handles_missing_snapshot_file() {
    let (_temp, store) = temp_store();

    // Create a valid index pointing to a non-existent snapshot
    let mut index = SnapshotIndex::new();
    index.add(SnapshotMetadata {
      id: "nonexistent123".to_string(),
      created_at: 12345,
      config_path: None,
      build_count: 0,
      bind_count: 0,
    });
    index.current = Some("nonexistent123".to_string());

    // Save the index directly
    fs::create_dir_all(&store.base_path).unwrap();
    let index_content = serde_json::to_string_pretty(&index).unwrap();
    fs::write(store.base_path.join(INDEX_FILENAME), &index_content).unwrap();

    // load_current should fail because the snapshot file doesn't exist
    let result = store.load_current();
    assert!(matches!(result, Err(SnapshotError::NotFound(_))));
  }

  #[test]
  fn load_snapshot_handles_corrupted_json() {
    let (_temp, store) = temp_store();

    fs::create_dir_all(&store.base_path).unwrap();
    // Write a corrupted snapshot file
    fs::write(store.base_path.join("corrupt123.json"), "garbage data").unwrap();

    let result = store.load_snapshot("corrupt123");
    assert!(result.is_err());
    // Should be a parse error, not NotFound
    match result {
      Err(SnapshotError::Parse(_)) => {} // Expected
      Err(other) => panic!("expected Parse error, got: {}", other),
      Ok(_) => panic!("expected error, got Ok"),
    }
  }

  #[test]
  fn load_snapshot_handles_wrong_schema() {
    let (_temp, store) = temp_store();

    fs::create_dir_all(&store.base_path).unwrap();
    // Valid JSON but wrong structure
    fs::write(
      store.base_path.join("wrongschema.json"),
      r#"{"unexpected": "structure"}"#,
    )
    .unwrap();

    let result = store.load_snapshot("wrongschema");
    assert!(result.is_err());
  }

  #[test]
  fn load_snapshot_handles_empty_file() {
    let (_temp, store) = temp_store();

    fs::create_dir_all(&store.base_path).unwrap();
    fs::write(store.base_path.join("empty.json"), "").unwrap();

    let result = store.load_snapshot("empty");
    assert!(result.is_err());
  }

  #[test]
  fn load_snapshot_handles_null_json() {
    let (_temp, store) = temp_store();

    fs::create_dir_all(&store.base_path).unwrap();
    fs::write(store.base_path.join("nullsnap.json"), "null").unwrap();

    let result = store.load_snapshot("nullsnap");
    assert!(result.is_err());
  }
}
