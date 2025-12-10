//! Declaration types collected from Lua config

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// A file declaration from the Lua config
///
/// Represents a file that sys.lua should manage. Only one of
/// `symlink`, `content`, or `copy` should be set.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileDecl {
    /// Target path for the file (with ~ expanded)
    pub path: PathBuf,

    /// Create a symbolic link to this target
    #[serde(skip_serializing_if = "Option::is_none")]
    pub symlink: Option<PathBuf>,

    /// Inline file content
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,

    /// Copy file from this source
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copy: Option<PathBuf>,

    /// Unix file permissions (e.g., 0o755)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<u32>,
}

impl FileDecl {
    /// Validate that exactly one of symlink, content, or copy is set
    pub fn validate(&self) -> Result<(), String> {
        let count = [
            self.symlink.is_some(),
            self.content.is_some(),
            self.copy.is_some(),
        ]
        .iter()
        .filter(|&&x| x)
        .count();

        if count == 0 {
            return Err(format!(
                "File declaration for '{}' must specify one of: symlink, content, or copy",
                self.path.display()
            ));
        }

        if count > 1 {
            return Err(format!(
                "File declaration for '{}' cannot specify multiple of: symlink, content, copy",
                self.path.display()
            ));
        }

        Ok(())
    }

    /// Get a description of the file type for display
    pub fn kind(&self) -> &'static str {
        if self.symlink.is_some() {
            "symlink"
        } else if self.content.is_some() {
            "content"
        } else if self.copy.is_some() {
            "copy"
        } else {
            "unknown"
        }
    }
}

/// How to handle a PATH-like environment variable
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum EnvMergeStrategy {
    /// Replace any existing value
    #[default]
    Replace,
    /// Prepend to existing PATH-like variable
    Prepend,
    /// Append to existing PATH-like variable
    Append,
}

/// A single environment variable value
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvValue {
    /// The value to set
    pub value: String,
    /// How to merge with existing value (for PATH-like vars)
    #[serde(default)]
    pub strategy: EnvMergeStrategy,
}

impl EnvValue {
    /// Create a new replace-style env value
    pub fn replace(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            strategy: EnvMergeStrategy::Replace,
        }
    }

    /// Create a new prepend-style env value (for PATH-like vars)
    pub fn prepend(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            strategy: EnvMergeStrategy::Prepend,
        }
    }

    /// Create a new append-style env value (for PATH-like vars)
    pub fn append(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            strategy: EnvMergeStrategy::Append,
        }
    }
}

/// An environment variable declaration from the Lua config
///
/// Represents an environment variable that sys.lua should manage.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EnvDecl {
    /// Environment variable name
    pub name: String,
    /// Values to set (multiple for PATH-like prepend/append)
    pub values: Vec<EnvValue>,
}

impl EnvDecl {
    /// Create a new environment variable with a single replace value
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            values: vec![EnvValue::replace(value)],
        }
    }

    /// Create a new PATH-like environment variable with prepend values
    pub fn path_prepend(name: impl Into<String>, paths: Vec<String>) -> Self {
        Self {
            name: name.into(),
            values: paths.into_iter().map(EnvValue::prepend).collect(),
        }
    }

    /// Check if this is a PATH-like variable (has prepend/append values)
    pub fn is_path_like(&self) -> bool {
        self.values.iter().any(|v| {
            matches!(
                v.strategy,
                EnvMergeStrategy::Prepend | EnvMergeStrategy::Append
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_decl_validate_symlink() {
        let decl = FileDecl {
            path: PathBuf::from("/home/user/.gitconfig"),
            symlink: Some(PathBuf::from("./dotfiles/gitconfig")),
            content: None,
            copy: None,
            mode: None,
        };
        assert!(decl.validate().is_ok());
        assert_eq!(decl.kind(), "symlink");
    }

    #[test]
    fn test_file_decl_validate_content() {
        let decl = FileDecl {
            path: PathBuf::from("/home/user/.gitconfig"),
            symlink: None,
            content: Some("[user]\nname = Test".to_string()),
            copy: None,
            mode: None,
        };
        assert!(decl.validate().is_ok());
        assert_eq!(decl.kind(), "content");
    }

    #[test]
    fn test_file_decl_validate_empty() {
        let decl = FileDecl {
            path: PathBuf::from("/home/user/.gitconfig"),
            symlink: None,
            content: None,
            copy: None,
            mode: None,
        };
        assert!(decl.validate().is_err());
    }

    #[test]
    fn test_file_decl_validate_multiple() {
        let decl = FileDecl {
            path: PathBuf::from("/home/user/.gitconfig"),
            symlink: Some(PathBuf::from("./dotfiles/gitconfig")),
            content: Some("content".to_string()),
            copy: None,
            mode: None,
        };
        assert!(decl.validate().is_err());
    }

    #[test]
    fn test_env_decl_simple() {
        let decl = EnvDecl::new("EDITOR", "nvim");
        assert_eq!(decl.name, "EDITOR");
        assert_eq!(decl.values.len(), 1);
        assert_eq!(decl.values[0].value, "nvim");
        assert!(!decl.is_path_like());
    }

    #[test]
    fn test_env_decl_path_prepend() {
        let decl = EnvDecl::path_prepend("PATH", vec!["~/.local/bin".to_string()]);
        assert_eq!(decl.name, "PATH");
        assert!(decl.is_path_like());
        assert!(matches!(decl.values[0].strategy, EnvMergeStrategy::Prepend));
    }

    #[test]
    fn test_env_value_strategies() {
        let replace = EnvValue::replace("value");
        assert!(matches!(replace.strategy, EnvMergeStrategy::Replace));

        let prepend = EnvValue::prepend("value");
        assert!(matches!(prepend.strategy, EnvMergeStrategy::Prepend));

        let append = EnvValue::append("value");
        assert!(matches!(append.strategy, EnvMergeStrategy::Append));
    }

    #[test]
    fn test_env_merge_strategy_default() {
        let strategy = EnvMergeStrategy::default();
        assert!(matches!(strategy, EnvMergeStrategy::Replace));
    }
}
