//! Build types for syslua.
//!
//! This module defines the core types for builds, which are reproducible artifacts
//! that can be cached and reused. Builds follow a two-tier architecture:
//!
//! - [`BuildSpec`]: The Lua-side specification containing closures (not serializable)
//! - [`BuildDef`]: The evaluated, serializable definition stored in manifests
//!
//! Builds are identified by content-addressed hashes ([`BuildHash`]) computed from
//! their [`BuildDef`]. This enables deduplication and caching.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
  action::Action,
  util::hash::{Hashable, ObjectHash},
};

/// Marker type name for BuildRef metatables in Lua.
///
/// This constant is used to identify Lua userdata that represents a reference
/// to a build. BuildRefs are Lua tables with metatables that allow accessing
/// build outputs (e.g., `build.out` or `build.bin`).
pub const BUILD_REF_TYPE: &str = "BuildRef";

/// A resolved, serializable input value.
///
/// This is the manifest-side representation of inputs. All values are fully
/// resolved and can be serialized to JSON.
///
/// # Primitive Types
///
/// - [`String`](BuildInputs::String): Text values
/// - [`Number`](BuildInputs::Number): Floating-point numbers
/// - [`Boolean`](BuildInputs::Boolean): True/false values
///
/// # Collection Types
///
/// - [`Table`](BuildInputs::Table): Key-value maps (Lua tables with string keys)
/// - [`Array`](BuildInputs::Array): Ordered sequences (Lua tables with numeric keys)
///
/// # Reference Types
///
/// - [`Build`](BuildInputs::Build): Reference to a build by its hash
///
/// # Reference Storage
///
/// When storing references to builds or bindings, only the hash is stored
/// (not the full definition). This:
/// - Keeps the manifest compact
/// - Avoids circular reference issues during serialization
/// - Enables efficient dependency tracking
///
/// # Example
///
/// ```json
/// {
///   "Table": {
///     "name": { "String": "myapp" },
///     "debug": { "Boolean": false },
///     "rust": { "Build": "a1b2c3d4e5f6789012ab" }
///   }
/// }
/// ```
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum BuildInputs {
  /// A string value.
  String(String),
  /// A numeric value (f64 to match Lua's number type).
  Number(f64),
  /// A boolean value.
  Boolean(bool),
  /// A table (map) with string keys.
  Table(BTreeMap<String, BuildInputs>),
  /// An array (sequence) of values.
  Array(Vec<BuildInputs>),
  /// A reference to a build, stored as its [`ObjectHash`].
  Build(ObjectHash),
}

/// The evaluated, serializable definition of a build.
///
/// This is the manifest-side representation produced by evaluating a [`BuildSpec`].
/// Unlike `BuildSpec`, this type is fully serializable and contains no Lua closures.
///
/// # Content Addressing
///
/// A [`BuildHash`] is computed from the JSON serialization of this struct.
/// Two `BuildDef`s with identical content produce the same hash, enabling
/// deduplication in the manifest.
///
/// # Placeholders
///
/// String fields may contain placeholders that resolve at execution time:
/// - `$${action:N}`: Output of action at index N
/// - `$${build:hash:output}`: Output from another build
/// - `$${bind:hash:output}`: Output from a binding
///
/// Shell variables like `$HOME` pass through unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BuildDef {
  /// Human-readable identifier for the build. Does not need to be unique.
  pub id: String,
  /// Resolved inputs (with BuildRef/BindRef converted to hashes).
  pub inputs: Option<BuildInputs>,
  /// Named outputs from the build (e.g., `{"out": "$${action:2}", "bin": "..."}`).
  pub outputs: Option<BTreeMap<String, String>>,
  /// The sequence of actions to execute during `create`.
  pub create_actions: Vec<Action>,
}

impl Hashable for BuildDef {}

#[cfg(test)]
mod tests {
  use super::*;

  mod build_def {
    use crate::{action::actions::exec::ExecOpts, consts::OBJ_HASH_PREFIX_LEN};

    use super::*;

    fn simple_def() -> BuildDef {
      BuildDef {
        id: "ripgrep-15.1.0".to_string(),
        inputs: None,
        create_actions: vec![Action::FetchUrl {
          url: "https://example.com/rg.tar.gz".to_string(),
          sha256: "abc123".to_string(),
        }],
        outputs: None,
      }
    }

    #[test]
    fn hash_is_deterministic() {
      let def = simple_def();

      let hash1 = def.compute_hash().unwrap();
      let hash2 = def.compute_hash().unwrap();

      assert_eq!(hash1, hash2);
    }

    #[test]
    fn hash_is_truncated() {
      let def = simple_def();
      let hash = def.compute_hash().unwrap();
      assert_eq!(hash.0.len(), OBJ_HASH_PREFIX_LEN);
    }

    #[test]
    fn hash_changes_when_name_differs() {
      let def1 = simple_def();

      let mut def2 = simple_def();
      def2.id = "fd".to_string();

      assert_ne!(def1.compute_hash().unwrap(), def2.compute_hash().unwrap());
    }

    #[test]
    fn hash_changes_when_actions_differ() {
      let def1 = simple_def();

      let mut def2 = simple_def();
      def2.create_actions.push(Action::Exec(ExecOpts {
        bin: "make".to_string(),
        args: None,
        env: None,
        cwd: None,
      }));

      assert_ne!(def1.compute_hash().unwrap(), def2.compute_hash().unwrap());
    }

    #[test]
    fn hash_changes_when_action_order_differs() {
      // Action order matters for reproducibility - same actions in different
      // order should produce different hashes
      let def1 = BuildDef {
        id: "test".to_string(),
        inputs: None,
        create_actions: vec![
          Action::Exec(ExecOpts {
            bin: "step1".to_string(),
            args: None,
            env: None,
            cwd: None,
          }),
          Action::Exec(ExecOpts {
            bin: "step2".to_string(),
            args: None,
            env: None,
            cwd: None,
          }),
        ],
        outputs: None,
      };

      let def2 = BuildDef {
        id: "test".to_string(),
        inputs: None,
        create_actions: vec![
          Action::Exec(ExecOpts {
            bin: "step2".to_string(),
            args: None,
            env: None,
            cwd: None,
          }),
          Action::Exec(ExecOpts {
            bin: "step1".to_string(),
            args: None,
            env: None,
            cwd: None,
          }),
        ],
        outputs: None,
      };

      assert_ne!(def1.compute_hash().unwrap(), def2.compute_hash().unwrap());
    }

    #[test]
    fn serialization_roundtrip_preserves_all_fields() {
      let mut env = BTreeMap::new();
      env.insert("CC".to_string(), "gcc".to_string());

      let def = BuildDef {
        id: "complex".to_string(),
        inputs: Some(BuildInputs::String("test".to_string())),
        create_actions: vec![
          Action::FetchUrl {
            url: "https://example.com/src.tar.gz".to_string(),
            sha256: "abc123".to_string(),
          },
          Action::Exec(ExecOpts {
            bin: "make".to_string(),
            args: Some(vec!["install".to_string()]),
            env: Some(env),
            cwd: Some("/build".to_string()),
          }),
        ],
        outputs: Some(BTreeMap::from([("out".to_string(), "$${action:1}".to_string())])),
      };

      let json = serde_json::to_string(&def).unwrap();
      let deserialized: BuildDef = serde_json::from_str(&json).unwrap();

      assert_eq!(def, deserialized);
    }
  }
}
