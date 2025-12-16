use crate::consts::APP_NAME;
use std::path::PathBuf;

#[cfg(windows)]
pub fn root_dir() -> PathBuf {
  let drive = std::env::var("SYSTEMDRIVE").expect("SYSTEMDRIVE not set");
  PathBuf::from(drive).join(APP_NAME)
}

#[cfg(not(windows))]
pub fn root_dir() -> PathBuf {
  PathBuf::from("/").join(APP_NAME)
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

#[cfg(test)]
#[cfg(not(windows))]
mod tests {
  use super::*;
  use serial_test::serial;

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
}
