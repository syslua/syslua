//! Path resolution for syslua directories
//!
//! Handles the distinction between root (system-wide) and user installations:
//! - Root: `/syslua/store/`
//! - User: `~/.local/share/syslua/store/`

use std::path::PathBuf;

/// Paths within the syslua store
#[derive(Debug, Clone)]
pub struct StorePaths {
    /// Root of the store (e.g., `/syslua/store/` or `~/.local/share/syslua/store/`)
    pub root: PathBuf,
    /// Object storage: `<store>/obj/<name>-<version>-<hash>/`
    pub obj: PathBuf,
    /// Package symlinks: `<store>/pkg/<name>/<version>/<platform>/`
    pub pkg: PathBuf,
    /// Derivation descriptions: `<store>/drv/`
    pub drv: PathBuf,
    /// Derivation output mappings: `<store>/drv-out/`
    pub drv_out: PathBuf,
}

impl StorePaths {
    /// Create store paths from a root directory
    pub fn new(root: PathBuf) -> Self {
        Self {
            obj: root.join("obj"),
            pkg: root.join("pkg"),
            drv: root.join("drv"),
            drv_out: root.join("drv-out"),
            root,
        }
    }

    /// Get the path for a specific object
    pub fn object_path(&self, name: &str, version: &str, hash: &str) -> PathBuf {
        self.obj.join(format!("{}-{}-{}", name, version, hash))
    }

    /// Get the symlink path for a package
    pub fn package_path(&self, name: &str, version: &str, platform: &str) -> PathBuf {
        self.pkg.join(name).join(version).join(platform)
    }
}

/// All syslua-related paths for a given installation
#[derive(Debug, Clone)]
pub struct SysluaPaths {
    /// Store paths
    pub store: StorePaths,
    /// Config directory (e.g., `~/.config/syslua/` or `/etc/syslua/`)
    pub config: PathBuf,
    /// Cache directory (e.g., `~/.cache/syslua/`)
    pub cache: PathBuf,
    /// Environment script location
    pub env_script: PathBuf,
    /// Whether this is a root (system-wide) installation
    pub is_root: bool,
}

impl SysluaPaths {
    /// Create paths for a user installation
    pub fn user() -> Self {
        let data_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("~/.local/share"))
            .join("syslua");

        let config_dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("syslua");

        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("~/.cache"))
            .join("syslua");

        Self {
            store: StorePaths::new(data_dir.join("store")),
            env_script: data_dir.join("env.sh"),
            config: config_dir,
            cache: cache_dir,
            is_root: false,
        }
    }

    /// Create paths for a root (system-wide) installation
    pub fn root() -> Self {
        let root = PathBuf::from("/syslua");

        Self {
            store: StorePaths::new(root.join("store")),
            env_script: root.join("env.sh"),
            config: PathBuf::from("/etc/syslua"),
            cache: root.join("cache"),
            is_root: true,
        }
    }

    /// Detect whether we're running as root and return appropriate paths
    pub fn detect() -> Self {
        if is_root_user() {
            Self::root()
        } else {
            Self::user()
        }
    }

    /// Get the inputs cache directory
    pub fn inputs_cache(&self) -> PathBuf {
        self.cache.join("inputs")
    }

    /// Get the downloads cache directory
    pub fn downloads_cache(&self) -> PathBuf {
        self.cache.join("downloads")
    }
}

/// Check if the current process is running as root/administrator
#[cfg(unix)]
fn is_root_user() -> bool {
    unsafe { libc::geteuid() == 0 }
}

#[cfg(windows)]
fn is_root_user() -> bool {
    // On Windows, we could check for administrator privileges
    // For now, always return false (user mode)
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_paths() {
        let paths = SysluaPaths::user();
        assert!(!paths.is_root);
        assert!(paths.store.root.to_string_lossy().contains("syslua"));
    }

    #[test]
    fn test_root_paths() {
        let paths = SysluaPaths::root();
        assert!(paths.is_root);
        assert_eq!(paths.store.root, PathBuf::from("/syslua/store"));
    }

    #[test]
    fn test_object_path() {
        let store = StorePaths::new(PathBuf::from("/syslua/store"));
        let path = store.object_path("ripgrep", "15.1.0", "abc123");
        assert_eq!(
            path,
            PathBuf::from("/syslua/store/obj/ripgrep-15.1.0-abc123")
        );
    }

    #[test]
    fn test_package_path() {
        let store = StorePaths::new(PathBuf::from("/syslua/store"));
        let path = store.package_path("ripgrep", "15.1.0", "aarch64-darwin");
        assert_eq!(
            path,
            PathBuf::from("/syslua/store/pkg/ripgrep/15.1.0/aarch64-darwin")
        );
    }
}
