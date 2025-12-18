use std::path::PathBuf;

use crate::{store::paths::StorePaths, util::hash::ObjectHash};

/// Generate the store object directory name for a build.
///
/// Format: `<name>-<version>-<hash>` or `<name>-<hash>` if no version.
/// Hash is truncated to first 16 characters.
pub fn build_dir_name(id: &str, hash: &ObjectHash) -> String {
  let hash = hash.0.as_str();
  format!("{}-{}", id, hash)
}

/// Generate the full store path for a build's output directory.
///
/// Returns the path within the system or user store based on the `system` parameter.
pub fn build_path(id: &str, hash: &ObjectHash, system: bool) -> PathBuf {
  let store = if system {
    StorePaths::system_store_path()
  } else {
    StorePaths::user_store_path()
  };
  store.join("obj").join(build_dir_name(id, hash))
}

#[cfg(test)]
mod tests {
  use crate::util::hash::ObjectHash;

  use super::*;

  #[test]
  fn object_dir_name_with_version() {
    // 24 char hash, first 20 chars = abc123def45678901234
    let hash = ObjectHash("abc123def45678901234".to_string());
    let name = build_dir_name("ripgrep-14.1.0", &hash);
    assert_eq!(name, "ripgrep-14.1.0-abc123def45678901234");
  }

  #[test]
  fn object_dir_name_without_version() {
    let hash = ObjectHash("abc123def45678901234".to_string());
    let name = build_dir_name("my-config", &hash);
    assert_eq!(name, "my-config-abc123def45678901234");
  }

  #[test]
  fn object_dir_name_short_hash() {
    let hash = ObjectHash("abc".to_string());
    let name = build_dir_name("test-1.0", &hash);
    assert_eq!(name, "test-1.0-abc");
  }

  #[test]
  fn object_path_includes_obj_dir() {
    use std::path::Path;

    let id = "test-1.0";
    let hash = ObjectHash("abc123def45678901234".to_string());
    let path = build_path(id, &hash, false);
    // Check that path ends with obj/name-version-{hash}
    // Note: We don't check for "store" because SYSLUA_USER_STORE env var can override to any path
    let expected_suffix = Path::new("obj").join("test-1.0-abc123def45678901234");
    assert!(
      path.ends_with(&expected_suffix),
      "Path {:?} should end with {:?}",
      path,
      expected_suffix
    );
  }
}
