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

use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

use mlua::prelude::*;
use serde::{Deserialize, Serialize};

use crate::{
  action::{Action, ActionCtx, actions::exec::ExecOpts},
  manifest::Manifest,
  util::hash::{Hashable, ObjectHash},
};

/// Lua-side specification for build inputs.
///
/// This enum captures what the user provided in the `inputs` field of `sys.build{}`:
/// - A static value (table, string, etc.)
/// - A function that will be called to get the value
/// - Nil (no inputs)
pub enum BuildInputsSpec {
  Value(LuaValue),
  Function(LuaFunction),
  Nil,
}

impl FromLua for BuildInputsSpec {
  fn from_lua(value: LuaValue, _lua: &Lua) -> LuaResult<Self> {
    match value {
      LuaValue::Boolean(b) => Ok(BuildInputsSpec::Value(LuaValue::Boolean(b))),
      LuaValue::Integer(n) => Ok(BuildInputsSpec::Value(LuaValue::Integer(n))),
      LuaValue::Number(n) => Ok(BuildInputsSpec::Value(LuaValue::Number(n))),
      LuaValue::String(s) => Ok(BuildInputsSpec::Value(LuaValue::String(s))),
      LuaValue::Table(t) => Ok(BuildInputsSpec::Value(LuaValue::Table(t))),
      LuaValue::Function(f) => Ok(BuildInputsSpec::Function(f)),
      LuaValue::Nil => Ok(BuildInputsSpec::Nil),
      _ => Err(LuaError::FromLuaConversionError {
        from: value.type_name(),
        to: "BuildInputsSpec".to_string(),
        message: Some("expected boolean, number, string, table, function, or nil".to_string()),
      }),
    }
  }
}

/// Lua-side specification for a build.
///
/// This struct captures what the user provided in the `sys.build{}` call.
/// It contains Lua closures and is not serializable.
pub struct BuildSpec {
  pub id: Option<String>,
  pub inputs: Option<BuildInputsSpec>,
  pub create: LuaFunction,
}

impl FromLua for BuildSpec {
  fn from_lua(value: LuaValue, _lua: &Lua) -> LuaResult<Self> {
    let table = match value {
      LuaValue::Table(t) => t,
      _ => {
        return Err(LuaError::FromLuaConversionError {
          from: value.type_name(),
          to: "BuildSpec".to_string(),
          message: Some("expected table".to_string()),
        });
      }
    };

    let id: Option<String> = table.get("id")?;
    let inputs: Option<BuildInputsSpec> = table.get("inputs")?;
    let create: LuaFunction = table
      .get("create")
      .map_err(|_| LuaError::external("build spec requires 'create' function"))?;

    Ok(BuildSpec { id, inputs, create })
  }
}

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

impl BuildInputs {
  pub fn from_spec(
    manifest: &Rc<RefCell<Manifest>>,
    spec: BuildInputsSpec,
    lua_value_to_def: impl Fn(LuaValue, &Manifest) -> LuaResult<BuildInputs>,
  ) -> LuaResult<Option<Self>> {
    match spec {
      BuildInputsSpec::Value(v) => Ok(Some(lua_value_to_def(v, &manifest.borrow())?)),
      BuildInputsSpec::Function(f) => {
        let result = f.call::<LuaValue>(())?;
        if result.is_nil() {
          Ok(None)
        } else {
          Ok(Some(lua_value_to_def(result, &manifest.borrow())?))
        }
      }
      BuildInputsSpec::Nil => Ok(None),
    }
  }
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
  /// Optional human-readable identifier for the build. Does not need to be unique.
  pub id: Option<String>,
  /// Resolved inputs (with BuildRef/BindRef converted to hashes).
  pub inputs: Option<BuildInputs>,
  /// Named outputs from the build (e.g., `{"out": "$${action:2}", "bin": "..."}`).
  pub outputs: Option<BTreeMap<String, String>>,
  /// The sequence of actions to execute during `create`.
  pub create_actions: Vec<Action>,
}

impl Hashable for BuildDef {}

impl BuildDef {
  pub fn from_spec(
    lua: &Lua,
    manifest: &Rc<RefCell<Manifest>>,
    spec: BuildSpec,
    lua_value_to_def: impl Fn(LuaValue, &Manifest) -> LuaResult<BuildInputs>,
    inputs_def_to_lua: impl Fn(&Lua, &BuildInputs, &Manifest) -> LuaResult<LuaValue>,
    parse_outputs: impl Fn(LuaTable) -> LuaResult<BTreeMap<String, String>>,
  ) -> LuaResult<Self> {
    let inputs = match spec.inputs {
      Some(input_spec) => BuildInputs::from_spec(manifest, input_spec, &lua_value_to_def)?,
      None => None,
    };

    let ctx = BuildCtx::new();
    let ctx_userdata = lua.create_userdata(ctx)?;

    let inputs_arg: LuaValue = match &inputs {
      Some(inputs) => inputs_def_to_lua(lua, inputs, &manifest.borrow())?,
      None => LuaValue::Table(lua.create_table()?),
    };

    let result: LuaValue = spec.create.call((inputs_arg, &ctx_userdata))?;

    let outputs: BTreeMap<String, String> = match result {
      LuaValue::Table(t) => {
        let parsed = parse_outputs(t)?;
        if parsed.is_empty() {
          return Err(LuaError::external("build create must return a non-empty outputs table"));
        }
        parsed
      }
      LuaValue::Nil => {
        return Err(LuaError::external(
          "build create must return a non-empty outputs table, got nil",
        ));
      }
      _ => {
        return Err(LuaError::external("build create must return a table of outputs"));
      }
    };

    let ctx: BuildCtx = ctx_userdata.take()?;

    Ok(BuildDef {
      id: spec.id,
      inputs,
      create_actions: ctx.into_actions(),
      outputs: Some(outputs),
    })
  }
}

/// Context for build `create` functions.
///
/// Provides `fetch_url`, `exec`, and `out` for recording build actions.
/// This is a newtype wrapper around [`ActionCtx`] that exposes the full
/// set of build-specific methods.
#[derive(Default)]
pub struct BuildCtx(ActionCtx);

impl BuildCtx {
  /// Create a new empty build context.
  pub fn new() -> Self {
    Self(ActionCtx::new())
  }

  /// Returns a placeholder string that resolves to the build's output directory.
  pub fn out(&self) -> &'static str {
    self.0.out()
  }

  /// Record a URL fetch action and return a placeholder for its output.
  ///
  /// This method is only available in build contexts, not bind contexts.
  pub fn fetch_url(&mut self, url: &str, sha256: &str) -> String {
    self.0.fetch_url(url, sha256)
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

/// Marker type name for BuildRef metatables in Lua.
///
/// This constant is used to identify Lua userdata that represents a reference
/// to a build. BuildRefs are Lua tables with metatables that allow accessing
/// build outputs (e.g., `build.out` or `build.bin`).
pub const BUILD_REF_TYPE: &str = "BuildRef";

/// A reference to a build, returned to Lua after creating a build.
///
/// This struct encapsulates the data returned to Lua code after a `sys.build{}` call.
/// It contains the id, hash, and outputs with placeholders for runtime resolution.
pub struct BuildRef {
  /// Optional human-readable identifier for the build.
  pub id: Option<String>,
  /// The content-addressed hash of the build definition.
  pub hash: ObjectHash,
  /// Named outputs from the build (keys only, values become placeholders).
  pub outputs: BTreeMap<String, String>,
}

impl BuildRef {
  /// Create a BuildRef from a BuildDef.
  ///
  /// Computes the content-addressed hash from the definition.
  pub fn from_def(def: &BuildDef) -> Result<Self, LuaError> {
    let hash = match def.compute_hash() {
      Ok(it) => it,
      Err(err) => return Err(LuaError::external(format!("failed to compute build hash: {}", err))),
    };
    Ok(Self {
      id: def.id.clone(),
      hash,
      // BuildDef always has outputs (enforced during creation)
      outputs: def.outputs.clone().unwrap_or_default(),
    })
  }
}

impl IntoLua for BuildRef {
  /// Convert this BuildRef to a Lua table.
  ///
  /// Creates a table with:
  /// - `id`: The build's human-readable identifier (optional)
  /// - `hash`: The build's content-addressed hash
  /// - `outputs`: Table of output keys mapped to `$${build:hash:key}` placeholders
  /// - Metatable with `__type = "BuildRef"`
  fn into_lua(self, lua: &Lua) -> LuaResult<LuaValue> {
    let ref_table = lua.create_table()?;
    ref_table.set("id", self.id.as_deref())?;
    ref_table.set("hash", self.hash.0.as_str())?;

    // Convert outputs to Lua table with placeholders for runtime resolution
    let outputs_table = lua.create_table()?;
    for k in self.outputs.keys() {
      let placeholder = format!("$${{build:{}:{}}}", self.hash.0, k);
      outputs_table.set(k.as_str(), placeholder.as_str())?;
    }
    ref_table.set("outputs", outputs_table)?;

    // Set metatable with __type marker
    let mt = lua.create_table()?;
    mt.set("__type", BUILD_REF_TYPE)?;
    ref_table.set_metatable(Some(mt))?;

    Ok(LuaValue::Table(ref_table))
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  mod build_def {
    use crate::{action::actions::exec::ExecOpts, consts::OBJ_HASH_PREFIX_LEN};

    use super::*;

    fn simple_def() -> BuildDef {
      BuildDef {
        id: Some("ripgrep-15.1.0".to_string()),
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
      def2.id = Some("fd".to_string());

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
        id: Some("test".to_string()),
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
        id: Some("test".to_string()),
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
        id: Some("complex".to_string()),
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
