use std::path::PathBuf;

use crate::consts::APP_NAME;
use crate::platform::is_elevated;

#[cfg(windows)]
pub fn root_dir() -> PathBuf {
  if let Ok(root) = std::env::var("SYSLUA_ROOT") {
    return PathBuf::from(root);
  }

  if is_elevated() {
    let drive = std::env::var("SYSTEMDRIVE").expect("SYSTEMDRIVE not set");
    PathBuf::from(format!("{}\\", drive)).join(APP_NAME)
  } else {
    data_dir()
  }
}

#[cfg(not(windows))]
pub fn root_dir() -> PathBuf {
  if let Ok(root) = std::env::var("SYSLUA_ROOT") {
    return PathBuf::from(root);
  }

  if is_elevated() {
    PathBuf::from("/").join(APP_NAME)
  } else {
    data_dir()
  }
}

/// Returns the user's home directory
#[cfg(windows)]
pub fn home_dir() -> PathBuf {
  let userprofile = std::env::var("USERPROFILE").expect("USERPROFILE not set");
  PathBuf::from(userprofile)
}

/// Returns the user's home directory
#[cfg(not(windows))]
pub fn home_dir() -> PathBuf {
  let home = std::env::var("HOME").expect("HOME not set");
  PathBuf::from(home)
}

/// Returns the directory for configuration files for the application
#[cfg(windows)]
pub fn config_dir() -> PathBuf {
  let appdata = std::env::var("APPDATA").expect("APPDATA not set");
  PathBuf::from(appdata).join(APP_NAME)
}

/// Returns the directory for configuration files for the application
#[cfg(not(windows))]
pub fn config_dir() -> PathBuf {
  let config_home = std::env::var("XDG_CONFIG_HOME")
    .map(PathBuf::from)
    .unwrap_or_else(|_| home_dir().join(".config"));
  config_home.join(APP_NAME)
}

/// Returns the directory for data files for the application
#[cfg(windows)]
pub fn data_dir() -> PathBuf {
  let appdata = std::env::var("APPDATA").expect("APPDATA not set");
  PathBuf::from(appdata).join(APP_NAME)
}

/// Returns the directory for data files for the application
#[cfg(not(windows))]
pub fn data_dir() -> PathBuf {
  let data_home = std::env::var("XDG_DATA_HOME")
    .map(PathBuf::from)
    .unwrap_or_else(|_| home_dir().join(".local").join("share"));
  data_home.join(APP_NAME)
}

/// Returns the directory for local data files for the application
#[cfg(windows)]
pub fn local_data_dir() -> PathBuf {
  let local_appdata = std::env::var("LOCALAPPDATA").expect("LOCALAPPDATA not set");
  PathBuf::from(local_appdata).join(APP_NAME)
}

/// Returns the directory for local data files for the application
#[cfg(not(windows))]
pub fn local_data_dir() -> PathBuf {
  data_dir()
}

/// Returns the directory for cache files for the application
#[cfg(windows)]
pub fn cache_dir() -> PathBuf {
  let local_appdata = std::env::var("LOCALAPPDATA").expect("LOCALAPPDATA not set");
  PathBuf::from(local_appdata).join(APP_NAME).join("Cache")
}

/// Returns the directory for cache files for the application
#[cfg(not(windows))]
pub fn cache_dir() -> PathBuf {
  let cache_home = std::env::var("XDG_CACHE_HOME")
    .map(PathBuf::from)
    .unwrap_or_else(|_| home_dir().join(".cache"));
  cache_home.join(APP_NAME)
}

pub fn store_dir() -> PathBuf {
  std::env::var("SYSLUA_STORE")
    .map(PathBuf::from)
    .unwrap_or_else(|_| root_dir().join("store"))
}

/// Returns the parent/fallback store directory for read-only lookups.
/// Used for store layering where user stores fall back to system store.
pub fn parent_store_dir() -> Option<PathBuf> {
  std::env::var("SYSLUA_PARENT_STORE").map(PathBuf::from).ok()
}

pub fn snapshots_dir() -> PathBuf {
  std::env::var("SYSLUA_SNAPSHOTS")
    .map(PathBuf::from)
    .unwrap_or_else(|_| root_dir().join("snapshots"))
}

pub fn plans_dir() -> PathBuf {
  std::env::var("SYSLUA_PLANS")
    .map(PathBuf::from)
    .unwrap_or_else(|_| root_dir().join("plans"))
}

#[cfg(test)]
#[cfg(not(windows))]
mod tests {
  use serial_test::serial;

  use super::*;

  #[test]
  #[serial]
  fn xdg_config_home_takes_precedence() {
    temp_env::with_vars(
      [
        ("XDG_CONFIG_HOME", Some("/custom/config")),
        ("HOME", Some("/home/user")),
      ],
      || {
        assert_eq!(config_dir(), PathBuf::from("/custom/config").join(APP_NAME));
      },
    );
  }

  #[test]
  #[serial]
  fn xdg_fallback_to_home_directories() {
    temp_env::with_vars(
      [
        ("XDG_CONFIG_HOME", None::<&str>),
        ("XDG_DATA_HOME", None::<&str>),
        ("XDG_CACHE_HOME", None::<&str>),
        ("HOME", Some("/home/user")),
      ],
      || {
        assert_eq!(config_dir(), PathBuf::from("/home/user/.config").join(APP_NAME));
        assert_eq!(data_dir(), PathBuf::from("/home/user/.local/share").join(APP_NAME));
        assert_eq!(cache_dir(), PathBuf::from("/home/user/.cache").join(APP_NAME));
      },
    );
  }

  #[test]
  #[serial]
  fn parent_store_dir_returns_none_when_unset() {
    temp_env::with_vars([("SYSLUA_PARENT_STORE", None::<&str>)], || {
      assert!(parent_store_dir().is_none());
    });
  }

  #[test]
  #[serial]
  fn parent_store_dir_returns_path_when_set() {
    temp_env::with_vars([("SYSLUA_PARENT_STORE", Some("/parent/store"))], || {
      assert_eq!(parent_store_dir(), Some(PathBuf::from("/parent/store")));
    });
  }
}
