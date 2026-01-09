//! Build artifact storage.
//!
//! Provides path resolution for build outputs in the store (`<store>/build/<hash>/`).

use std::path::{Path, PathBuf};

use tracing::warn;

use crate::platform::link::link_dir;
use crate::platform::paths::{parent_store_dir, store_dir};
use crate::util::hash::ObjectHash;

pub fn build_dir_name(hash: &ObjectHash) -> String {
  hash.0.clone()
}

pub fn build_dir_path(hash: &ObjectHash) -> PathBuf {
  let dir_name = build_dir_name(hash);
  let primary = store_dir().join("build").join(&dir_name);

  // If exists in primary store, use it
  if primary.exists() {
    return primary;
  }

  // Check parent store for fallback
  if let Some(parent) = parent_store_dir() {
    let fallback = parent.join("build").join(&dir_name);
    if fallback.exists() {
      // Create symlink in primary store pointing to parent
      if let Err(e) = link_dir(&fallback, &primary) {
        warn!(hash = %hash.0, error = %e, "Failed to link from parent store, using direct path");
        return fallback;
      }
      return primary;
    }
  }

  // Return primary path even if doesn't exist (for new builds)
  primary
}

pub fn build_exists_in_store(hash: &ObjectHash, store_path: &Path) -> bool {
  let dir_name = build_dir_name(hash);
  let build_path = store_path.join("build").join(dir_name);
  build_path.exists()
}

#[cfg(test)]
mod tests {
  use super::*;
  use serial_test::serial;

  #[test]
  fn test_build_dir_name() {
    let hash = ObjectHash("abc123def45678901234".to_string());
    let name = build_dir_name(&hash);
    assert_eq!(name, "abc123def45678901234");
  }

  #[test]
  #[serial]
  fn test_build_path_includes_build_dir() {
    temp_env::with_vars(
      [("SYSLUA_STORE", Some("/test/store")), ("SYSLUA_ROOT", None::<&str>)],
      || {
        let hash = ObjectHash("abc123def45678901234".to_string());
        let path = build_dir_path(&hash);
        assert_eq!(path, PathBuf::from("/test/store/build/abc123def45678901234"));
      },
    );
  }

  #[test]
  #[serial]
  fn build_dir_path_falls_back_to_parent_store() {
    let temp = tempfile::tempdir().unwrap();
    let parent_store = temp.path().join("parent");
    let user_store = temp.path().join("user");

    // Create build in parent store only
    let hash = ObjectHash("abc123def45678901234".to_string());
    let parent_build = parent_store.join("build").join(&hash.0);
    std::fs::create_dir_all(&parent_build).unwrap();
    std::fs::write(parent_build.join("marker.txt"), "exists").unwrap();

    temp_env::with_vars(
      [
        ("SYSLUA_STORE", Some(user_store.to_str().unwrap())),
        ("SYSLUA_PARENT_STORE", Some(parent_store.to_str().unwrap())),
        ("SYSLUA_ROOT", None::<&str>),
      ],
      || {
        let path = build_dir_path(&hash);

        // Should return path in user store (symlinked from parent)
        assert!(path.starts_with(&user_store));

        // The symlink should exist and point to parent content
        assert!(path.join("marker.txt").exists());
      },
    );
  }

  #[test]
  #[serial]
  fn build_dir_path_prefers_primary_store() {
    let temp = tempfile::tempdir().unwrap();
    let parent_store = temp.path().join("parent");
    let user_store = temp.path().join("user");

    let hash = ObjectHash("abc123def45678901234".to_string());

    // Create build in BOTH stores
    let parent_build = parent_store.join("build").join(&hash.0);
    std::fs::create_dir_all(&parent_build).unwrap();
    std::fs::write(parent_build.join("marker.txt"), "parent").unwrap();

    let user_build = user_store.join("build").join(&hash.0);
    std::fs::create_dir_all(&user_build).unwrap();
    std::fs::write(user_build.join("marker.txt"), "user").unwrap();

    temp_env::with_vars(
      [
        ("SYSLUA_STORE", Some(user_store.to_str().unwrap())),
        ("SYSLUA_PARENT_STORE", Some(parent_store.to_str().unwrap())),
        ("SYSLUA_ROOT", None::<&str>),
      ],
      || {
        let path = build_dir_path(&hash);

        // Should return user store path (primary)
        assert!(path.starts_with(&user_store));

        // Should contain user content, not parent
        let content = std::fs::read_to_string(path.join("marker.txt")).unwrap();
        assert_eq!(content, "user");
      },
    );
  }
}
