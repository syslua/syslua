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

use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use mlua::prelude::*;
use serde::{Deserialize, Serialize};
use sha2::Digest;

use crate::{
  action::{Action, ActionCtx, actions::exec::ExecOpts},
  bind::lua::{bind_inputs_ref_to_lua, lua_value_to_bind_inputs_def},
  manifest::Manifest,
  outputs::lua::{outputs_to_lua_table, parse_outputs},
  util::hash::{HashError, Hashable, ObjectHash},
};

pub enum BindInputsSpec {
  Value(LuaValue),
  Function(LuaFunction),
  Nil,
}

impl FromLua for BindInputsSpec {
  fn from_lua(value: LuaValue, _lua: &Lua) -> LuaResult<Self> {
    match value {
      LuaValue::Boolean(t) => Ok(BindInputsSpec::Value(LuaValue::Boolean(t))),
      LuaValue::Integer(n) => Ok(BindInputsSpec::Value(LuaValue::Integer(n))),
      LuaValue::Number(n) => Ok(BindInputsSpec::Value(LuaValue::Number(n))),
      LuaValue::String(s) => Ok(BindInputsSpec::Value(LuaValue::String(s))),
      LuaValue::Table(t) => Ok(BindInputsSpec::Value(LuaValue::Table(t))),
      LuaValue::Function(f) => Ok(BindInputsSpec::Function(f)),
      LuaValue::Nil => Ok(BindInputsSpec::Nil),
      _ => Err(LuaError::FromLuaConversionError {
        from: value.type_name(),
        to: "BindInputsSpec".to_string(),
        message: Some("expected boolean, number, string, table, function, or nil".to_string()),
      }),
    }
  }
}

pub struct BindSpec {
  pub id: Option<String>,
  pub inputs: Option<BindInputsSpec>,
  pub create: LuaFunction,
  pub update: Option<LuaFunction>,
  pub destroy: LuaFunction,
  pub check: Option<LuaFunction>,
  pub replace: bool,
}

impl FromLua for BindSpec {
  fn from_lua(value: LuaValue, _lua: &Lua) -> LuaResult<Self> {
    let table = match value {
      LuaValue::Table(t) => t,
      _ => {
        return Err(LuaError::FromLuaConversionError {
          from: value.type_name(),
          to: "BindSpec".to_string(),
          message: Some("expected table".to_string()),
        });
      }
    };

    let id: Option<String> = table.get("id")?;
    let inputs: Option<BindInputsSpec> = table.get("inputs")?;
    let create: LuaFunction = table
      .get("create")
      .map_err(|_| LuaError::external("bind requires a `create` function"))?;
    let update: Option<LuaFunction> = table.get("update")?;
    let destroy: LuaFunction = table
      .get("destroy")
      .map_err(|_| LuaError::external("bind requires a `destroy` function"))?;
    let check: Option<LuaFunction> = table.get("check")?;

    if update.is_some() && id.is_none() {
      return Err(LuaError::FromLuaConversionError {
        from: "table",
        to: "BindSpec".to_string(),
        message: Some("binds with 'update' must have an 'id'".to_string()),
      });
    }

    let replace: bool = table.get("replace").unwrap_or(false);

    Ok(BindSpec {
      id,
      inputs,
      create,
      update,
      destroy,
      check,
      replace,
    })
  }
}

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
pub enum BindInputsDef {
  /// A string value.
  String(String),
  /// A numeric value (f64 to match Lua's number type).
  Number(f64),
  /// A boolean value.
  Boolean(bool),
  /// A table (map) with string keys.
  Table(BTreeMap<String, BindInputsDef>),
  /// An array (sequence) of values.
  Array(Vec<BindInputsDef>),
  /// A reference to a build, stored as its [`ObjectHash`].
  Build(ObjectHash),
  /// A reference to a binding, stored as its [`ObjectHash`].
  Bind(ObjectHash),
}

impl BindInputsDef {
  pub fn from_spec(_lua: &Lua, manifest: &Rc<RefCell<Manifest>>, spec: BindInputsSpec) -> LuaResult<Self> {
    match spec {
      BindInputsSpec::Value(v) => Ok(lua_value_to_bind_inputs_def(v, &manifest.borrow())?),
      BindInputsSpec::Function(f) => {
        let result = f.call::<LuaValue>(())?;
        if result.is_nil() {
          Ok(BindInputsDef::Table(BTreeMap::new()))
        } else {
          Ok(lua_value_to_bind_inputs_def(result, &manifest.borrow())?)
        }
      }
      BindInputsSpec::Nil => Ok(BindInputsDef::Table(BTreeMap::new())),
    }
  }
}

/// Result of running a bind's check callback.
///
/// This is the runtime result after executing check actions and resolving
/// all placeholders. It contains the final boolean drift status and optional
/// message.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BindCheckResult {
  /// Whether the bind has drifted from expected state.
  pub drifted: bool,
  /// Optional message describing the drift.
  pub message: Option<String>,
}

/// Placeholder patterns for check result.
///
/// This is stored in [`BindDef`] and contains placeholder strings that will
/// be resolved at execution time to produce a [`BindCheckResult`].
///
/// Unlike other callbacks, the check callback's output is not persisted -
/// it's used only to determine if the bind has drifted from its expected state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BindCheckOutputs {
  /// Placeholder pattern for drifted boolean (resolves to "true" or "false").
  pub drifted: String,
  /// Optional placeholder pattern for message.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub message: Option<String>,
}

/// The evaluated, serializable definition of a binding.
///
/// This is the manifest-side representation produced by evaluating a [`BindSpec`].
/// Unlike `BindSpec`, this type is fully serializable and contains no Lua closures.
///
/// # Content Addressing
///
/// A [`ObjectHash`] is computed from the JSON serialization of this struct.
/// Two `BindDef`s with identical content produce the same hash, enabling
/// deduplication in the manifest.
///
/// # Apply vs Destroy
///
/// - `apply_actions`: Run when the binding is created or updated
/// - `update_actions`: Run when the binding is updated (optional)
/// - `destroy_actions`: Run when the binding is removed
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
  /// Unique identifier for the binding. Only required if using `update`.
  pub id: Option<String>,
  /// Resolved inputs (with BuildRef/BindRef converted to hashes).
  pub inputs: Option<BindInputsDef>,
  /// Named outputs from the binding (e.g., `{"path": "$${action:0}"}`).
  pub outputs: Option<BTreeMap<String, String>>,
  /// The sequence of actions to execute during `create`.
  pub create_actions: Vec<Action>,
  /// Actions to execute during `update`.
  pub update_actions: Option<Vec<Action>>,
  /// Actions to execute during `destroy` (cleanup).
  pub destroy_actions: Vec<Action>,
  /// Actions to execute during `check` (drift detection).
  /// If None, the bind has no check capability.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub check_actions: Option<Vec<Action>>,
  /// Output patterns for check result (with placeholders).
  /// Contains `drifted` (string "true"/"false") and optional `message`.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub check_outputs: Option<BindCheckOutputs>,
}

impl Hashable for BindDef {
  fn compute_hash(&self) -> Result<ObjectHash, HashError> {
    #[derive(Serialize)]
    struct BindDefHashable<'a> {
      id: &'a Option<String>,
      inputs: &'a Option<BindInputsDef>,
      outputs: &'a Option<BTreeMap<String, String>>,
      create_actions: &'a Vec<Action>,
      update_actions: &'a Option<Vec<Action>>,
      destroy_actions: &'a Vec<Action>,
    }

    let hashable = BindDefHashable {
      id: &self.id,
      inputs: &self.inputs,
      outputs: &self.outputs,
      create_actions: &self.create_actions,
      update_actions: &self.update_actions,
      destroy_actions: &self.destroy_actions,
    };

    let serialized = serde_json::to_string(&hashable)?;
    let mut hasher = sha2::Sha256::new();
    sha2::Digest::update(&mut hasher, serialized.as_bytes());
    let full = format!("{:x}", hasher.finalize());
    Ok(ObjectHash(full[..crate::consts::OBJ_HASH_PREFIX_LEN].to_string()))
  }
}

impl BindDef {
  pub fn from_spec(lua: &Lua, manifest: &Rc<RefCell<Manifest>>, spec: BindSpec) -> LuaResult<Self> {
    let inputs = match spec.inputs {
      Some(input_spec) => Some(BindInputsDef::from_spec(lua, manifest, input_spec)?),
      None => None,
    };

    let mut create_ctx = BindCtx::new();
    let create_ctx_userdata = lua.create_userdata(create_ctx)?;

    // Prepare inputs argument for create function
    let inputs_arg: LuaValue = match &inputs {
      Some(inputs) => bind_inputs_ref_to_lua(lua, inputs, &manifest.borrow())?,
      None => LuaValue::Table(lua.create_table()?), // Empty table if no inputs
    };

    // Call: create(inputs, ctx) -> outputs (optional)
    let create_result: LuaValue = spec.create.call((&inputs_arg, &create_ctx_userdata))?;

    // Extract outputs from create return value (optional for binds)
    let outputs: Option<BTreeMap<String, String>> = match create_result {
      LuaValue::Table(t) => {
        let parsed = parse_outputs(t)?;
        if parsed.is_empty() { None } else { Some(parsed) }
      }
      LuaValue::Nil => None,
      _ => {
        return Err(LuaError::external("bind create must return a table of outputs or nil"));
      }
    };

    // Extract create actions from ActionCtx
    create_ctx = create_ctx_userdata.take()?;
    let create_actions = create_ctx.into_actions();

    // Create outputs argument for destroy function
    // The outputs contain $${out} placeholders that will be resolved at runtime
    let outputs_arg: LuaValue = match &outputs {
      Some(outs) => {
        let outputs_table = outputs_to_lua_table(lua, outs)?;
        LuaValue::Table(outputs_table)
      }
      None => LuaValue::Table(lua.create_table()?),
    };

    let update_actions = if let Some(update_fn) = spec.update {
      let update_ctx = BindCtx::new();
      let update_ctx_userdata = lua.create_userdata(update_ctx)?;

      // Call: update(outputs, inputs, ctx) -> outputs (must match create's output keys)
      let update_result: LuaValue = update_fn.call((&outputs_arg, &inputs_arg, &update_ctx_userdata))?;

      // Validate update returns same output shape as create
      match (&update_result, &outputs) {
        (LuaValue::Table(update_table), Some(create_outputs)) => {
          let update_outputs = parse_outputs(update_table.clone())?;
          let create_keys: std::collections::HashSet<_> = create_outputs.keys().collect();
          let update_keys: std::collections::HashSet<_> = update_outputs.keys().collect();

          if create_keys != update_keys {
            return Err(LuaError::external(format!(
              "update must return same output keys as create. create: {:?}, update: {:?}",
              create_keys, update_keys
            )));
          }
        }
        (LuaValue::Table(update_table), None) => {
          let update_outputs = parse_outputs(update_table.clone())?;
          if !update_outputs.is_empty() {
            return Err(LuaError::external(format!(
              "update returned outputs but create did not. update keys: {:?}",
              update_outputs.keys().collect::<Vec<_>>()
            )));
          }
        }
        (LuaValue::Nil, Some(create_outputs)) => {
          if !create_outputs.is_empty() {
            return Err(LuaError::external(format!(
              "update returned nil but create returned outputs. create keys: {:?}",
              create_outputs.keys().collect::<Vec<_>>()
            )));
          }
        }
        (LuaValue::Nil, None) => {
          // Both return nil/empty, that's fine
        }
        (other, _) => {
          return Err(LuaError::external(format!(
            "update must return a table of outputs or nil, got: {:?}",
            other.type_name()
          )));
        }
      }

      let update_ctx: BindCtx = update_ctx_userdata.take()?;
      let update_actions = update_ctx.into_actions();
      if update_actions.is_empty() {
        None
      } else {
        Some(update_actions)
      }
    } else {
      None
    };

    // Call destroy function
    let destroy_actions = {
      let destroy_ctx = BindCtx::new();
      let destroy_ctx_userdata = lua.create_userdata(destroy_ctx)?;

      // Call: destroy(outputs, ctx) -> ignored
      let _: LuaValue = spec.destroy.call((outputs_arg.clone(), &destroy_ctx_userdata))?;

      let destroy_ctx: BindCtx = destroy_ctx_userdata.take()?;
      destroy_ctx.into_actions()
    };

    // Call optional check function
    let (check_actions, check_outputs) = if let Some(check_fn) = spec.check {
      let check_ctx = BindCtx::new();
      let check_ctx_userdata = lua.create_userdata(check_ctx)?;

      // Call: check(outputs, inputs, ctx) -> { drifted, message? }
      let check_result: LuaValue = check_fn.call((&outputs_arg, &inputs_arg, &check_ctx_userdata))?;

      let (drifted, message) = match check_result {
        LuaValue::Table(t) => {
          let drifted: String = t
            .get("drifted")
            .map_err(|_| LuaError::external("check must return a table with `drifted` field"))?;
          let message: Option<String> = t.get("message")?;
          (drifted, message)
        }
        _ => {
          return Err(LuaError::external(
            "check must return a table with `drifted` (and optional `message`) fields",
          ));
        }
      };

      let check_ctx: BindCtx = check_ctx_userdata.take()?;
      let actions = check_ctx.into_actions();

      if actions.is_empty() && drifted != "true" && drifted != "false" {
        (None, None)
      } else {
        (Some(actions), Some(BindCheckOutputs { drifted, message }))
      }
    } else {
      (None, None)
    };

    // Create BindDef
    Ok(BindDef {
      id: spec.id,
      inputs,
      create_actions,
      update_actions,
      outputs,
      destroy_actions,
      check_actions,
      check_outputs,
    })
  }
}

/// Context for bind `create`, `update`, and `destroy` functions.
///
/// Provides `exec` and `out` for recording bind actions.
/// Note: `fetch_url` is intentionally not available in binds - binds should
/// only modify system state using build outputs, not download new content.
#[derive(Default)]
pub struct BindCtx(ActionCtx);

impl BindCtx {
  /// Create a new empty bind context.
  pub fn new() -> Self {
    Self(ActionCtx::new())
  }

  /// Returns a placeholder string that resolves to the bind's output directory.
  pub fn out(&self) -> &'static str {
    self.0.out()
  }

  /// Record a command execution action and return a placeholder for its output.
  pub fn exec(&mut self, opts: impl Into<ExecOpts>) -> String {
    self.0.exec(opts)
  }

  /// Consume the context and return the recorded actions.
  pub fn into_actions(self) -> Vec<Action> {
    self.0.into_actions()
  }
}

/// Marker type name for BindRef metatables in Lua.
///
/// This constant is used to identify Lua userdata that represents a reference
/// to a binding. BindRefs are Lua tables with metatables that allow accessing
/// bind outputs (e.g., `bind.status` or `bind.path`).
pub const BIND_REF_TYPE: &str = "BindRef";

pub struct BindRef {
  pub hash: ObjectHash,
  pub outputs: Option<BTreeMap<String, String>>,
}

impl BindRef {
  pub fn from_def(def: &BindDef) -> Result<Self, LuaError> {
    let hash = match def.compute_hash() {
      Ok(it) => it,
      Err(err) => return Err(LuaError::external(format!("failed to compute bind hash: {}", err))),
    };
    Ok(Self {
      hash,
      outputs: def.outputs.clone(),
    })
  }
}

impl IntoLua for BindRef {
  /// Convert this BindRef to a Lua table.
  ///
  /// Creates a table with:
  /// - `hash`: The bind's content-addressed hash
  /// - `outputs`: Table of output keys mapped to `$${bind:hash:key}` placeholders
  /// - Metatable with `__type = "BindRef"`
  fn into_lua(self, lua: &Lua) -> LuaResult<LuaValue> {
    let ref_table = lua.create_table()?;
    ref_table.set("hash", self.hash.0.as_str())?;
    // Convert outputs to Lua table with placeholders for runtime resolution
    if let Some(outs) = &self.outputs {
      let outputs_table = lua.create_table()?;
      for k in outs.keys() {
        let placeholder = format!("$${{bind:{}:{}}}", self.hash.0, k);
        outputs_table.set(k.as_str(), placeholder.as_str())?;
      }
      ref_table.set("outputs", outputs_table)?;
    }
    // Set metatable with __type marker
    let mt = lua.create_table()?;
    mt.set("__type", BIND_REF_TYPE)?;
    ref_table.set_metatable(Some(mt))?;
    Ok(LuaValue::Table(ref_table))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  mod bind_def {
    use crate::{action::actions::exec::ExecOpts, consts::OBJ_HASH_PREFIX_LEN};

    use super::*;

    fn simple_def() -> BindDef {
      BindDef {
        id: None,
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
        check_actions: None,
        check_outputs: None,
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
        id: None,
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
        check_actions: None,
        check_outputs: None,
      };

      let def2 = BindDef {
        id: None,
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
        check_actions: None,
        check_outputs: None,
      };

      assert_ne!(def1.compute_hash().unwrap(), def2.compute_hash().unwrap());
    }

    #[test]
    fn serialization_roundtrip_preserves_all_fields() {
      let mut env = BTreeMap::new();
      env.insert("HOME".to_string(), "/home/user".to_string());

      let def = BindDef {
        id: Some("test-bind".to_string()),
        inputs: Some(BindInputsDef::String("test".to_string())),
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
        check_actions: Some(vec![Action::Exec(ExecOpts {
          bin: "test".to_string(),
          args: Some(vec!["-L".to_string(), "/dest".to_string()]),
          env: None,
          cwd: None,
        })]),
        check_outputs: Some(BindCheckOutputs {
          drifted: "$${action:0}".to_string(),
          message: Some("link check".to_string()),
        }),
      };

      let json = serde_json::to_string(&def).unwrap();
      let deserialized: BindDef = serde_json::from_str(&json).unwrap();

      assert_eq!(def, deserialized);
    }

    #[test]
    fn check_does_not_affect_hash() {
      let def1 = simple_def();

      let mut def2 = simple_def();
      def2.check_actions = Some(vec![Action::Exec(ExecOpts {
        bin: "test".to_string(),
        args: Some(vec!["-f".to_string(), "/some/path".to_string()]),
        env: None,
        cwd: None,
      })]);
      def2.check_outputs = Some(BindCheckOutputs {
        drifted: "$${action:0}".to_string(),
        message: Some("file missing".to_string()),
      });

      assert_eq!(def1.compute_hash().unwrap(), def2.compute_hash().unwrap());
    }
  }
}
