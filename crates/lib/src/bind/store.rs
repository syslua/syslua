use std::path::PathBuf;

use crate::{store::paths::StorePaths, util::hash::ObjectHash};

/// Generate the store bind directory name for a binding.
pub fn bind_dir_name(hash: &ObjectHash) -> String {
  let hash = hash.0.as_str();
  hash.to_string()
}

pub fn bind_path(hash: &ObjectHash, system: bool) -> PathBuf {
  let store = if system {
    StorePaths::system_store_path()
  } else {
    StorePaths::user_store_path()
  };
  store.join("bind").join(bind_dir_name(hash))
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn object_dir_name_with_version() {
    // 24 char hash, first 20 chars = abc123def45678901234
    let hash = ObjectHash("abc123def45678901234".to_string());
    let name = bind_dir_name(&hash);
    assert_eq!(name, "abc123def45678901234");
  }

  #[test]
  fn object_dir_name_without_version() {
    let hash = ObjectHash("abc123def45678901234".to_string());
    let name = bind_dir_name(&hash);
    assert_eq!(name, "abc123def45678901234");
  }

  #[test]
  fn object_dir_name_short_hash() {
    let hash = ObjectHash("abc".to_string());
    let name = bind_dir_name(&hash);
    assert_eq!(name, "abc");
  }

  #[test]
  fn object_path_includes_obj_dir() {
    let hash = ObjectHash("abc123def45678901234".to_string());
    let path = bind_path(&hash, false);
    assert!(path.ends_with("store/bind/abc123def45678901234"));
  }
}
