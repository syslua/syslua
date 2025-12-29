use std::path::PathBuf;

use crate::{platform::paths::store_dir, util::hash::ObjectHash};

pub fn bind_dir_name(hash: &ObjectHash) -> String {
  hash.0.clone()
}

pub fn bind_dir_path(hash: &ObjectHash) -> PathBuf {
  store_dir().join("bind").join(bind_dir_name(hash))
}

#[cfg(test)]
mod tests {
  use super::*;
  use serial_test::serial;

  #[test]
  fn test_bind_dir_name() {
    let hash = ObjectHash("abc123def45678901234".to_string());
    let name = bind_dir_name(&hash);
    assert_eq!(name, "abc123def45678901234");
  }

  #[test]
  #[serial]
  fn test_bind_path_includes_bind_dir() {
    temp_env::with_vars(
      [("SYSLUA_STORE", Some("/test/store")), ("SYSLUA_ROOT", None::<&str>)],
      || {
        let hash = ObjectHash("abc123def45678901234".to_string());
        let path = bind_dir_path(&hash);
        assert_eq!(path, PathBuf::from("/test/store/bind/abc123def45678901234"));
      },
    );
  }
}
