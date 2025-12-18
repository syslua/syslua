//! Bind types for syslua.
//!
//! This module defines the core types for bindings, which represent system state
//! changes that can be applied and destroyed. Unlike builds, bindings are not
//! cached - they represent mutable state like symlinks, config files, or services.
//!
//! Bindings follow a two-tier architecture:
//!
//! - [`BindSpec`]: The Lua-side specification containing closures (not serializable)
//! - [`BindDef`]: The evaluated, serializable definition stored in manifests
//!
//! Bindings are identified by content-addressed hashes ([`BindHash`]) computed from
//! their [`BindDef`]. This enables tracking which bindings need to be reapplied.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::{
  action::Action,
  util::hash::{Hashable, ObjectHash},
};

/// Marker type name for BindRef metatables in Lua.
///
/// This constant is used to identify Lua userdata that represents a reference
/// to a binding. BindRefs are Lua tables with metatables that allow accessing
/// bind outputs (e.g., `bind.status` or `bind.path`).
pub const BIND_REF_TYPE: &str = "BindRef";

/// A resolved, serializable input value.
///
/// This is the manifest-side representation of inputs. All values are fully
/// resolved and can be serialized to JSON.
///
/// # Primitive Types
///
/// - [`String`](BindInputs::String): Text values
/// - [`Number`](BindInputs::Number): Floating-point numbers
/// - [`Boolean`](BindInputs::Boolean): True/false values
///
/// # Collection Types
///
/// - [`Table`](BindInputs::Table): Key-value maps (Lua tables with string keys)
/// - [`Array`](BindInputs::Array): Ordered sequences (Lua tables with numeric keys)
///
/// # Reference Types
///
/// - [`Build`](BindInputs::Build): Reference to a build by its hash
/// - [`Bind`](BindInputs::Bind): Reference to a binding by its hash
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
pub enum BindInputs {
  /// A string value.
  String(String),
  /// A numeric value (f64 to match Lua's number type).
  Number(f64),
  /// A boolean value.
  Boolean(bool),
  /// A table (map) with string keys.
  Table(BTreeMap<String, BindInputs>),
  /// An array (sequence) of values.
  Array(Vec<BindInputs>),
  /// A reference to a build, stored as its [`ObjectHash`].
  Build(ObjectHash),
  /// A reference to a binding, stored as its [`ObjectHash`].
  Bind(ObjectHash),
}

/// The evaluated, serializable definition of a binding.
///
/// This is the manifest-side representation produced by evaluating a [`BindSpec`].
/// Unlike `BindSpec`, this type is fully serializable and contains no Lua closures.
///
/// # Content Addressing
///
/// A [`BindHash`] is computed from the JSON serialization of this struct.
/// Two `BindDef`s with identical content produce the same hash, enabling
/// deduplication in the manifest.
///
/// # Apply vs Destroy
///
/// - `apply_actions`: Run when the binding is created or updated
/// - `destroy_actions`: Run when the binding is removed (optional cleanup)
///
/// # Placeholders
///
/// String fields may contain placeholders that resolve at execution time:
/// - `$${action:N}`: Output of action at index N
/// - `$${build:hash:output}`: Output from a build
/// - `$${bind:hash:output}`: Output from another binding
///
/// Shell variables like `$HOME` pass through unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BindDef {
  /// The unique identifier for this binding definition.
  pub id: String,
  /// Resolved inputs (with BuildRef/BindRef converted to hashes).
  pub inputs: Option<BindInputs>,
  /// Named outputs from the binding (e.g., `{"path": "$${action:0}"}`).
  pub outputs: Option<BTreeMap<String, String>>,
  /// The sequence of actions to execute during `create`.
  pub create_actions: Vec<Action>,
  /// Actions to execute during `update`.
  pub update_actions: Option<Vec<Action>>,
  /// Actions to execute during `destroy` (cleanup).
  pub destroy_actions: Vec<Action>,
}

impl Hashable for BindDef {}

#[cfg(test)]
mod tests {
  use super::*;

  mod bind_def {
    use crate::{action::actions::exec::ExecOpts, consts::OBJ_HASH_PREFIX_LEN};

    use super::*;

    fn simple_def() -> BindDef {
      BindDef {
        id: "test-bind".to_string(),
        inputs: None,
        outputs: None,
        create_actions: vec![Action::Exec(ExecOpts {
          bin: "ln -s /src /dest".to_string(),
          args: None,
          env: None,
          cwd: None,
        })],
        update_actions: None,
        destroy_actions: vec![],
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
    fn hash_changes_when_apply_actions_differ() {
      let def1 = simple_def();

      let mut def2 = simple_def();
      def2.create_actions.push(Action::Exec(ExecOpts {
        bin: "echo done".to_string(),
        args: None,
        env: None,
        cwd: None,
      }));

      assert_ne!(def1.compute_hash().unwrap(), def2.compute_hash().unwrap());
    }

    #[test]
    fn hash_changes_when_destroy_actions_added() {
      // This is critical: adding destroy_actions should change the hash
      // because it changes what the bind does (cleanup behavior)
      let def1 = simple_def();

      let mut def2 = simple_def();
      def2.destroy_actions = vec![Action::Exec(ExecOpts {
        bin: "rm /dest".to_string(),
        args: None,
        env: None,
        cwd: None,
      })];

      assert_ne!(def1.compute_hash().unwrap(), def2.compute_hash().unwrap());
    }

    #[test]
    fn hash_changes_when_action_order_differs() {
      let def1 = BindDef {
        id: "test-bind".to_string(),
        inputs: None,
        outputs: None,
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
        update_actions: None,
        destroy_actions: vec![],
      };

      let def2 = BindDef {
        id: "test-bind".to_string(),
        inputs: None,
        outputs: None,
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
        update_actions: None,
        destroy_actions: vec![],
      };

      assert_ne!(def1.compute_hash().unwrap(), def2.compute_hash().unwrap());
    }

    #[test]
    fn serialization_roundtrip_preserves_all_fields() {
      let mut env = BTreeMap::new();
      env.insert("HOME".to_string(), "/home/user".to_string());

      let def = BindDef {
        id: "test-bind".to_string(),
        inputs: Some(BindInputs::String("test".to_string())),
        outputs: Some(BTreeMap::from([("link".to_string(), "$${action:0}".to_string())])),
        create_actions: vec![Action::Exec(ExecOpts {
          bin: "ln -s /src /dest".to_string(),
          args: None,
          env: Some(env),
          cwd: Some("/home".to_string()),
        })],
        update_actions: Some(vec![Action::Exec(ExecOpts {
          bin: "echo updated".to_string(),
          args: None,
          env: None,
          cwd: None,
        })]),
        destroy_actions: vec![Action::Exec(ExecOpts {
          bin: "rm /dest".to_string(),
          args: None,
          env: None,
          cwd: None,
        })],
      };

      let json = serde_json::to_string(&def).unwrap();
      let deserialized: BindDef = serde_json::from_str(&json).unwrap();

      assert_eq!(def, deserialized);
    }
  }
}
