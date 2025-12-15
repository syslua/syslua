use crate::consts::APP_NAME;
use crate::platform::paths::data_dir;
use std::path::PathBuf;

pub struct StorePaths {
  pub system: PathBuf,
  pub user: PathBuf,
}

impl StorePaths {
  pub fn current() -> Self {
    Self {
      system: Self::system_store_path(),
      user: Self::user_store_path(),
    }
  }

  pub fn system_store_path() -> PathBuf {
    if let Ok(path) = std::env::var("SYSLUA_SYSTEM_STORE") {
      return PathBuf::from(path);
    }

    Self::default_system_store_path()
  }

  #[cfg(windows)]
  pub fn default_system_store_path() -> PathBuf {
    let drive = std::env::var("SYSTEMDRIVE").expect("SYSTEMDRIVE not set");
    PathBuf::from(drive).join(APP_NAME).join("store")
  }

  #[cfg(not(windows))]
  pub fn default_system_store_path() -> PathBuf {
    PathBuf::from("/").join(APP_NAME).join("store")
  }

  pub fn user_store_path() -> PathBuf {
    if let Ok(path) = std::env::var("SYSLUA_USER_STORE") {
      return PathBuf::from(path);
    }

    Self::default_user_store_path()
  }

  pub fn default_user_store_path() -> PathBuf {
    data_dir().join("store")
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serial_test::serial;
  use temp_env::with_vars;

  #[test]
  #[serial]
  fn env_var_overrides_default_paths() {
    with_vars(
      [
        ("SYSLUA_SYSTEM_STORE", Some("/custom/system/store")),
        ("SYSLUA_USER_STORE", Some("/custom/user/store")),
      ],
      || {
        let paths = StorePaths::current();
        assert_eq!(paths.system, PathBuf::from("/custom/system/store"));
        assert_eq!(paths.user, PathBuf::from("/custom/user/store"));
      },
    )
  }

  #[test]
  #[serial]
  #[cfg(not(windows))]
  fn default_system_store_at_root() {
    let path = StorePaths::default_system_store_path();
    assert_eq!(path, PathBuf::from("/").join(APP_NAME).join("store"));
  }
}
