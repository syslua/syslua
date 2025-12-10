//! Content-addressed store for syslua
//!
//! The store holds all installed packages and derivation outputs.
//! Layout:
//! ```text
//! <store_root>/
//! ├── obj/<name>-<version>-<hash>/  # Immutable content
//! ├── pkg/<name>/<version>/<platform>/  # Symlinks to obj/
//! ├── drv/<hash>.drv                    # Derivation descriptions
//! └── drv-out/<hash>                    # Maps drv hash → output hash
//! ```

use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tracing::{debug, info};

use sys_platform::{Platform, StorePaths, SysluaPaths};

use crate::hash::short_hash;
use crate::{Error, Result};

/// The syslua store
pub struct Store {
    paths: SysluaPaths,
    /// Additional PATH entries from activations
    path_entries: Mutex<Vec<String>>,
    /// Scripts to source from activations
    source_scripts: Mutex<Vec<String>>,
}

impl Store {
    /// Create a new store using the detected paths (root or user)
    pub fn new() -> Self {
        Self {
            paths: SysluaPaths::detect(),
            path_entries: Mutex::new(Vec::new()),
            source_scripts: Mutex::new(Vec::new()),
        }
    }

    /// Create a store with explicit user-level paths
    pub fn user() -> Self {
        Self {
            paths: SysluaPaths::user(),
            path_entries: Mutex::new(Vec::new()),
            source_scripts: Mutex::new(Vec::new()),
        }
    }

    /// Create a store with explicit root-level paths
    pub fn root() -> Self {
        Self {
            paths: SysluaPaths::root(),
            path_entries: Mutex::new(Vec::new()),
            source_scripts: Mutex::new(Vec::new()),
        }
    }

    /// Initialize the store directories
    pub fn init(&self) -> Result<()> {
        let store = &self.paths.store;

        fs::create_dir_all(&store.root)?;
        fs::create_dir_all(&store.obj)?;
        fs::create_dir_all(&store.pkg)?;
        fs::create_dir_all(&store.drv)?;
        fs::create_dir_all(&store.drv_out)?;
        fs::create_dir_all(self.paths.inputs_cache())?;
        fs::create_dir_all(self.paths.downloads_cache())?;

        info!("Initialized store at {}", store.root.display());
        Ok(())
    }

    /// Get the store paths
    pub fn paths(&self) -> &SysluaPaths {
        &self.paths
    }

    /// Get the store paths (mutable)
    pub fn store_paths(&self) -> &StorePaths {
        &self.paths.store
    }

    /// Check if a derivation output already exists in the store
    pub fn has_object(&self, name: &str, version: &str, hash: &str) -> bool {
        let short = short_hash(hash);
        let obj_path = self.paths.store.object_path(name, version, short);
        obj_path.exists()
    }

    /// Get the path where an object should be stored
    pub fn object_path(&self, name: &str, version: &str, hash: &str) -> PathBuf {
        let short = short_hash(hash);
        self.paths.store.object_path(name, version, short)
    }

    /// Register a derivation output in the store
    ///
    /// Creates the package symlink: pkg/<name>/<version>/<platform>/ → obj/<name>-<version>-<hash>/
    pub fn register_package(
        &self,
        name: &str,
        version: &str,
        hash: &str,
        platform: &Platform,
    ) -> Result<PathBuf> {
        let short = short_hash(hash);
        let obj_path = self.paths.store.object_path(name, version, short);
        let pkg_path = self
            .paths
            .store
            .package_path(name, version, &platform.to_string());

        // Ensure parent directories exist
        if let Some(parent) = pkg_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Remove existing symlink if present
        if pkg_path.exists() || pkg_path.is_symlink() {
            fs::remove_file(&pkg_path).ok();
        }

        // Create symlink
        symlink(&obj_path, &pkg_path).map_err(|e| {
            Error::Store(format!(
                "Failed to create symlink {} -> {}: {}",
                pkg_path.display(),
                obj_path.display(),
                e
            ))
        })?;

        debug!(
            "Registered {} -> {}",
            pkg_path.display(),
            obj_path.display()
        );

        Ok(pkg_path)
    }

    /// Get download cache path for a URL
    pub fn download_cache_path(&self, filename: &str) -> PathBuf {
        self.paths.downloads_cache().join(filename)
    }

    /// Get input cache path
    pub fn input_cache_path(&self, name: &str) -> PathBuf {
        self.paths.inputs_cache().join(name)
    }

    /// Get all bin directories in the store for PATH
    pub fn collect_bin_paths(&self) -> Result<Vec<PathBuf>> {
        let mut bins = Vec::new();
        let pkg_dir = &self.paths.store.pkg;

        if !pkg_dir.exists() {
            return Ok(bins);
        }

        // Walk pkg/<name>/<version>/<platform>/bin
        for name_entry in fs::read_dir(pkg_dir)? {
            let name_entry = name_entry?;
            if !name_entry.file_type()?.is_dir() {
                continue;
            }

            for version_entry in fs::read_dir(name_entry.path())? {
                let version_entry = version_entry?;
                if !version_entry.file_type()?.is_dir() {
                    continue;
                }

                for platform_entry in fs::read_dir(version_entry.path())? {
                    let platform_entry = platform_entry?;
                    let bin_path = platform_entry.path().join("bin");
                    if bin_path.exists() && bin_path.is_dir() {
                        // Resolve symlink to get actual path
                        let resolved = fs::canonicalize(&bin_path).unwrap_or(bin_path);
                        bins.push(resolved);
                    }
                }
            }
        }

        Ok(bins)
    }

    /// Add a path entry from an activation
    pub fn add_path_entry(&self, path: &str) -> Result<()> {
        let mut entries = self.path_entries.lock().unwrap();
        if !entries.contains(&path.to_string()) {
            entries.push(path.to_string());
        }
        Ok(())
    }

    /// Add a script to source from an activation
    pub fn add_source_script(&self, script: &str) -> Result<()> {
        let mut scripts = self.source_scripts.lock().unwrap();
        if !scripts.contains(&script.to_string()) {
            scripts.push(script.to_string());
        }
        Ok(())
    }

    /// Generate env.sh script with PATH additions and source scripts
    pub fn generate_env_script(&self) -> Result<()> {
        let bin_paths = self.collect_bin_paths()?;
        let extra_paths = self.path_entries.lock().unwrap().clone();
        let source_scripts = self.source_scripts.lock().unwrap().clone();

        // Combine all PATH additions
        let mut all_paths: Vec<String> = bin_paths
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        all_paths.extend(extra_paths);

        let mut script = String::from("# Generated by syslua - do not edit\n");
        script.push_str("# Source this file in your shell profile\n\n");

        if !all_paths.is_empty() {
            script.push_str(&format!("export PATH=\"{}:$PATH\"\n", all_paths.join(":")));
        }

        // Add source scripts
        for src in &source_scripts {
            script.push_str("\n# Source activation script\n");
            script.push_str(&format!("if [ -f \"{}\" ]; then\n", src));
            script.push_str(&format!("  . \"{}\"\n", src));
            script.push_str("fi\n");
        }

        // Ensure parent directory exists
        if let Some(parent) = self.paths.env_script.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::write(&self.paths.env_script, script)?;
        info!("Generated {}", self.paths.env_script.display());

        Ok(())
    }

    /// Get the env script path
    pub fn env_script_path(&self) -> &Path {
        &self.paths.env_script
    }
}

impl Default for Store {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_store() -> (Store, TempDir) {
        let temp = TempDir::new().unwrap();
        let paths = SysluaPaths {
            store: StorePaths::new(temp.path().join("store")),
            config: temp.path().join("config"),
            cache: temp.path().join("cache"),
            env_script: temp.path().join("env.sh"),
            is_root: false,
        };
        let store = Store {
            paths,
            path_entries: Mutex::new(Vec::new()),
            source_scripts: Mutex::new(Vec::new()),
        };
        (store, temp)
    }

    #[test]
    fn test_store_init() {
        let (store, _temp) = test_store();
        store.init().unwrap();

        assert!(store.paths.store.root.exists());
        assert!(store.paths.store.obj.exists());
        assert!(store.paths.store.pkg.exists());
    }

    #[test]
    fn test_object_path() {
        let (store, _temp) = test_store();
        let path = store.object_path("ripgrep", "15.1.0", "abc123def456");
        assert!(path
            .to_string_lossy()
            .contains("ripgrep-15.1.0-abc123def456"));
    }
}
