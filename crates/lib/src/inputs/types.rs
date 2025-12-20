//! Input types for declaration and resolution.
//!
//! This module defines the types used throughout the input resolution process:
//! - [`InputDecl`] - Parsed input declaration from Lua (before resolution)
//! - [`InputOverride`] - Override specification for transitive dependencies
//! - [`ResolvedInput`] - A fully resolved input with path, revision, and transitive deps

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Maximum depth for follows chain resolution.
/// Prevents infinite loops in malformed configurations.
pub const MAX_FOLLOWS_DEPTH: usize = 10;

/// A parsed input declaration (before resolution).
///
/// Inputs can be declared in two forms:
/// 1. Simple string URL: `"git:https://github.com/org/repo.git"`
/// 2. Extended table with URL and overrides
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputDecl {
  /// Simple URL string (current behavior).
  ///
  /// ```lua
  /// inputs = {
  ///   utils = "git:https://github.com/org/utils.git",
  /// }
  /// ```
  Url(String),

  /// Extended declaration with optional URL and input overrides.
  ///
  /// ```lua
  /// inputs = {
  ///   pkgs = {
  ///     url = "git:https://github.com/org/pkgs.git",
  ///     inputs = {
  ///       utils = { follows = "my_utils" },
  ///     },
  ///   },
  /// }
  /// ```
  Extended {
    /// The URL of the input. Can be `None` if this is a pure follows override.
    url: Option<String>,
    /// Overrides for transitive dependencies.
    inputs: BTreeMap<String, InputOverride>,
  },
}

impl InputDecl {
  /// Get the URL from the declaration, if present.
  pub fn url(&self) -> Option<&str> {
    match self {
      InputDecl::Url(url) => Some(url),
      InputDecl::Extended { url, .. } => url.as_deref(),
    }
  }

  /// Get the input overrides, if any.
  pub fn overrides(&self) -> Option<&BTreeMap<String, InputOverride>> {
    match self {
      InputDecl::Url(_) => None,
      InputDecl::Extended { inputs, .. } => {
        if inputs.is_empty() {
          None
        } else {
          Some(inputs)
        }
      }
    }
  }

  /// Check if this declaration has any overrides.
  pub fn has_overrides(&self) -> bool {
    matches!(self, InputDecl::Extended { inputs, .. } if !inputs.is_empty())
  }
}

/// An override specification for a transitive dependency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InputOverride {
  /// Override with a different URL.
  ///
  /// ```lua
  /// inputs = {
  ///   utils = { url = "git:https://github.com/myorg/utils.git" },
  /// }
  /// ```
  Url(String),

  /// Follow another input (use its resolved value).
  ///
  /// ```lua
  /// inputs = {
  ///   utils = { follows = "other_lib/utils" },
  /// }
  /// ```
  ///
  /// The follows path can be:
  /// - A direct input name: `"my_utils"`
  /// - A path to a transitive dep: `"other_lib/utils"`
  Follows(String),
}

impl InputOverride {
  /// Check if this is a follows override.
  pub fn is_follows(&self) -> bool {
    matches!(self, InputOverride::Follows(_))
  }

  /// Get the follows path, if this is a follows override.
  pub fn follows_path(&self) -> Option<&str> {
    match self {
      InputOverride::Follows(path) => Some(path),
      InputOverride::Url(_) => None,
    }
  }
}

/// A resolved input ready for use.
///
/// Contains the local path, resolved revision, and any transitive dependencies.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedInput {
  /// Absolute path to the input's root directory in the cache.
  pub path: PathBuf,

  /// The resolved revision (git commit hash or "local" for path inputs).
  pub rev: String,

  /// Resolved transitive dependencies of this input.
  ///
  /// Each entry maps the dependency name (as declared in the input's init.lua)
  /// to its resolved state. This allows isolated dependency resolution where
  /// different inputs can use different versions of the same dependency.
  pub inputs: ResolvedInputs,
}

impl ResolvedInput {
  /// Create a new resolved input without transitive dependencies.
  pub fn new(path: PathBuf, rev: String) -> Self {
    Self {
      path,
      rev,
      inputs: BTreeMap::new(),
    }
  }

  /// Create a new resolved input with transitive dependencies.
  pub fn with_inputs(path: PathBuf, rev: String, inputs: ResolvedInputs) -> Self {
    Self { path, rev, inputs }
  }
}

/// Map of input names to their resolved state.
pub type ResolvedInputs = BTreeMap<String, ResolvedInput>;

/// Map of input names to their declarations.
pub type InputDecls = BTreeMap<String, InputDecl>;

/// A node in the dependency graph for lock file serialization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LockNode {
  /// Input type: "git" or "path".
  #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
  pub type_: Option<String>,

  /// Original URL from config.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub url: Option<String>,

  /// Pinned revision (git commit hash or "local" for path inputs).
  #[serde(skip_serializing_if = "Option::is_none")]
  pub rev: Option<String>,

  /// Unix timestamp of when this input was last modified/fetched.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub last_modified: Option<u64>,

  /// References to dependency nodes (input name -> node label).
  #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
  pub inputs: BTreeMap<String, String>,
}

impl LockNode {
  /// Create a new root node.
  pub fn root(inputs: BTreeMap<String, String>) -> Self {
    Self {
      type_: None,
      url: None,
      rev: None,
      last_modified: None,
      inputs,
    }
  }

  /// Create a new input node.
  pub fn input(
    type_: &str,
    url: &str,
    rev: &str,
    last_modified: Option<u64>,
    inputs: BTreeMap<String, String>,
  ) -> Self {
    Self {
      type_: Some(type_.to_string()),
      url: Some(url.to_string()),
      rev: Some(rev.to_string()),
      last_modified,
      inputs,
    }
  }

  /// Check if this is the root node.
  pub fn is_root(&self) -> bool {
    self.type_.is_none() && self.url.is_none()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  mod input_decl {
    use super::*;

    #[test]
    fn url_variant_returns_url() {
      let decl = InputDecl::Url("git:https://example.com/repo.git".to_string());
      assert_eq!(decl.url(), Some("git:https://example.com/repo.git"));
      assert!(decl.overrides().is_none());
      assert!(!decl.has_overrides());
    }

    #[test]
    fn extended_with_url_returns_url() {
      let decl = InputDecl::Extended {
        url: Some("git:https://example.com/repo.git".to_string()),
        inputs: BTreeMap::new(),
      };
      assert_eq!(decl.url(), Some("git:https://example.com/repo.git"));
      assert!(decl.overrides().is_none()); // Empty overrides returns None
      assert!(!decl.has_overrides());
    }

    #[test]
    fn extended_with_overrides() {
      let mut inputs = BTreeMap::new();
      inputs.insert("utils".to_string(), InputOverride::Follows("my_utils".to_string()));

      let decl = InputDecl::Extended {
        url: Some("git:https://example.com/repo.git".to_string()),
        inputs,
      };

      assert!(decl.has_overrides());
      let overrides = decl.overrides().unwrap();
      assert_eq!(overrides.len(), 1);
      assert!(matches!(
        overrides.get("utils"),
        Some(InputOverride::Follows(path)) if path == "my_utils"
      ));
    }

    #[test]
    fn extended_without_url() {
      let mut inputs = BTreeMap::new();
      inputs.insert("utils".to_string(), InputOverride::Follows("my_utils".to_string()));

      let decl = InputDecl::Extended { url: None, inputs };

      assert!(decl.url().is_none());
      assert!(decl.has_overrides());
    }
  }

  mod input_override {
    use super::*;

    #[test]
    fn follows_variant() {
      let override_ = InputOverride::Follows("other/utils".to_string());
      assert!(override_.is_follows());
      assert_eq!(override_.follows_path(), Some("other/utils"));
    }

    #[test]
    fn url_variant() {
      let override_ = InputOverride::Url("git:https://example.com".to_string());
      assert!(!override_.is_follows());
      assert!(override_.follows_path().is_none());
    }
  }

  mod resolved_input {
    use super::*;

    #[test]
    fn new_without_inputs() {
      let input = ResolvedInput::new(PathBuf::from("/path/to/input"), "abc123".to_string());
      assert_eq!(input.path, PathBuf::from("/path/to/input"));
      assert_eq!(input.rev, "abc123");
      assert!(input.inputs.is_empty());
    }

    #[test]
    fn with_transitive_inputs() {
      let mut transitive = BTreeMap::new();
      transitive.insert(
        "utils".to_string(),
        ResolvedInput::new(PathBuf::from("/path/to/utils"), "def456".to_string()),
      );

      let input = ResolvedInput::with_inputs(PathBuf::from("/path/to/input"), "abc123".to_string(), transitive);

      assert_eq!(input.inputs.len(), 1);
      assert!(input.inputs.contains_key("utils"));
    }
  }

  mod lock_node {
    use super::*;

    #[test]
    fn root_node() {
      let mut inputs = BTreeMap::new();
      inputs.insert("pkgs".to_string(), "pkgs-abc123".to_string());

      let node = LockNode::root(inputs);
      assert!(node.is_root());
      assert!(node.type_.is_none());
      assert!(node.url.is_none());
      assert_eq!(node.inputs.len(), 1);
    }

    #[test]
    fn input_node() {
      let node = LockNode::input(
        "git",
        "git:https://example.com",
        "abc123",
        Some(1234567890),
        BTreeMap::new(),
      );

      assert!(!node.is_root());
      assert_eq!(node.type_.as_deref(), Some("git"));
      assert_eq!(node.url.as_deref(), Some("git:https://example.com"));
      assert_eq!(node.rev.as_deref(), Some("abc123"));
      assert_eq!(node.last_modified, Some(1234567890));
    }
  }
}
