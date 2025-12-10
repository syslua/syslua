//! Path expansion utilities

use crate::error::PlatformError;
use std::path::{Path, PathBuf};

/// Expand a path, resolving `~` to the user's home directory
///
/// # Examples
///
/// ```
/// use sys_platform::expand_path;
///
/// let path = expand_path("~/.config/syslua/init.lua").unwrap();
/// assert!(path.starts_with(dirs::home_dir().unwrap()));
/// ```
pub fn expand_path<P: AsRef<Path>>(path: P) -> Result<PathBuf, PlatformError> {
    let path = path.as_ref();
    let path_str = path.to_string_lossy();

    if path_str.starts_with("~/") {
        let home = dirs::home_dir().ok_or(PlatformError::NoHomeDirectory)?;
        Ok(home.join(&path_str[2..]))
    } else if path_str == "~" {
        dirs::home_dir().ok_or(PlatformError::NoHomeDirectory)
    } else {
        Ok(path.to_path_buf())
    }
}

/// Expand a path relative to a base directory
///
/// - `~` is expanded to home directory
/// - Relative paths (not starting with `/` or `~`) are resolved relative to `base`
/// - Absolute paths are returned as-is
///
/// # Examples
///
/// ```
/// use sys_platform::expand_path_with_base;
/// use std::path::Path;
///
/// // Relative path resolved against base
/// let path = expand_path_with_base("./dotfiles/gitconfig", "/home/user/config").unwrap();
/// assert_eq!(path.to_string_lossy(), "/home/user/config/dotfiles/gitconfig");
///
/// // ~ is expanded regardless of base
/// let path = expand_path_with_base("~/.gitconfig", "/some/base").unwrap();
/// assert!(path.starts_with(dirs::home_dir().unwrap()));
///
/// // Absolute paths ignore base
/// let path = expand_path_with_base("/etc/hosts", "/some/base").unwrap();
/// assert_eq!(path.to_string_lossy(), "/etc/hosts");
/// ```
pub fn expand_path_with_base<P: AsRef<Path>, B: AsRef<Path>>(
    path: P,
    base: B,
) -> Result<PathBuf, PlatformError> {
    let path = path.as_ref();
    let path_str = path.to_string_lossy();

    // Handle ~ expansion first
    if path_str.starts_with('~') {
        return expand_path(path);
    }

    // Handle absolute paths
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }

    // Windows: check for drive letter
    #[cfg(windows)]
    if path_str.len() >= 2 && path_str.chars().nth(1) == Some(':') {
        return Ok(path.to_path_buf());
    }

    // Relative path - resolve against base
    let base = base.as_ref();
    let full_path = base.join(path);

    // Canonicalize to resolve . and .. components
    // But don't require the path to exist (use normalize instead)
    Ok(normalize_path(&full_path))
}

/// Normalize a path by resolving `.` and `..` components without requiring the path to exist
fn normalize_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();

    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                // Pop the last component if possible
                if !components.is_empty() {
                    components.pop();
                }
            }
            std::path::Component::CurDir => {
                // Skip . components
            }
            other => {
                components.push(other);
            }
        }
    }

    components.iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_tilde() {
        let home = dirs::home_dir().expect("No home directory");

        let expanded = expand_path("~/.config").unwrap();
        assert_eq!(expanded, home.join(".config"));

        let expanded = expand_path("~/").unwrap();
        assert_eq!(expanded, home.join(""));

        let expanded = expand_path("~").unwrap();
        assert_eq!(expanded, home);
    }

    #[test]
    fn test_expand_absolute() {
        let path = expand_path("/etc/hosts").unwrap();
        assert_eq!(path, PathBuf::from("/etc/hosts"));
    }

    #[test]
    fn test_expand_relative() {
        let path = expand_path("./foo/bar").unwrap();
        assert_eq!(path, PathBuf::from("./foo/bar"));
    }

    #[test]
    fn test_expand_with_base_relative() {
        let path = expand_path_with_base("./dotfiles/gitconfig", "/home/user/config").unwrap();
        assert_eq!(path, PathBuf::from("/home/user/config/dotfiles/gitconfig"));
    }

    #[test]
    fn test_expand_with_base_tilde() {
        let home = dirs::home_dir().expect("No home directory");
        let path = expand_path_with_base("~/.gitconfig", "/some/base").unwrap();
        assert_eq!(path, home.join(".gitconfig"));
    }

    #[test]
    fn test_expand_with_base_absolute() {
        let path = expand_path_with_base("/etc/hosts", "/some/base").unwrap();
        assert_eq!(path, PathBuf::from("/etc/hosts"));
    }

    #[test]
    fn test_expand_with_base_parent_dir() {
        let path = expand_path_with_base("../other/file", "/home/user/config").unwrap();
        assert_eq!(path, PathBuf::from("/home/user/other/file"));
    }

    #[test]
    fn test_normalize_path() {
        assert_eq!(
            normalize_path(Path::new("/foo/bar/../baz")),
            PathBuf::from("/foo/baz")
        );

        assert_eq!(
            normalize_path(Path::new("/foo/./bar")),
            PathBuf::from("/foo/bar")
        );

        assert_eq!(
            normalize_path(Path::new("/foo/bar/../../baz")),
            PathBuf::from("/baz")
        );
    }
}
