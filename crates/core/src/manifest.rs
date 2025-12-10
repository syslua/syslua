//! Manifest types representing desired system state

use serde::{Deserialize, Serialize};
use std::path::Path;
use sys_lua::{EnvDecl, FileDecl};

/// A manifest representing the desired system state
///
/// This is the intermediate representation produced by evaluating a Lua config.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Manifest {
    /// File declarations
    pub files: Vec<FileDecl>,
    /// Environment variable declarations
    pub envs: Vec<EnvDecl>,
}

impl Manifest {
    /// Create a new empty manifest
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            envs: Vec::new(),
        }
    }

    /// Create a manifest from a Lua config file
    pub fn from_config(config_path: &Path) -> Result<Self, sys_lua::LuaError> {
        let result = sys_lua::evaluate_config(config_path)?;

        Ok(Self {
            files: result.files,
            envs: result.envs,
        })
    }

    /// Add a file declaration
    pub fn add_file(&mut self, file: FileDecl) {
        self.files.push(file);
    }

    /// Add an environment variable declaration
    pub fn add_env(&mut self, env: EnvDecl) {
        self.envs.push(env);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use tempfile::NamedTempFile;

    #[test]
    fn test_manifest_from_config() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(
            temp_file,
            r#"
            file {{
                path = "/tmp/test.txt",
                content = "Hello!",
            }}
        "#
        )
        .unwrap();

        let manifest = Manifest::from_config(temp_file.path()).unwrap();
        assert_eq!(manifest.files.len(), 1);
    }

    #[test]
    fn test_manifest_add_file() {
        let mut manifest = Manifest::new();

        manifest.add_file(FileDecl {
            path: PathBuf::from("/tmp/test.txt"),
            symlink: None,
            content: Some("Hello".to_string()),
            copy: None,
            mode: None,
        });

        assert_eq!(manifest.files.len(), 1);
    }

    #[test]
    fn test_manifest_add_env() {
        let mut manifest = Manifest::new();

        manifest.add_env(EnvDecl::new("EDITOR", "nvim"));

        assert_eq!(manifest.envs.len(), 1);
        assert_eq!(manifest.envs[0].name, "EDITOR");
    }

    #[test]
    fn test_manifest_from_config_with_env() {
        let mut temp_file = NamedTempFile::new().unwrap();
        writeln!(
            temp_file,
            r#"
            file {{
                path = "/tmp/test.txt",
                content = "Hello!",
            }}
            env {{
                EDITOR = "nvim",
            }}
        "#
        )
        .unwrap();

        let manifest = Manifest::from_config(temp_file.path()).unwrap();
        assert_eq!(manifest.files.len(), 1);
        assert_eq!(manifest.envs.len(), 1);
    }
}
