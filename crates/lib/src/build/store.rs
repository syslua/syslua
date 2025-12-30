//! Build artifact storage.
//!
//! Provides path resolution for build outputs in the store (`<store>/build/<hash>/`).

use std::path::{Path, PathBuf};

use crate::{platform::paths::store_dir, util::hash::ObjectHash};

pub fn build_dir_name(hash: &ObjectHash) -> String {
  hash.0.clone()
}

pub fn build_dir_path(hash: &ObjectHash) -> PathBuf {
  store_dir().join("build").join(build_dir_name(hash))
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
}
