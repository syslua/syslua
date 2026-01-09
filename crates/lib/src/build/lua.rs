//! Lua bindings for `sys.build{}`.
//!
//! This module provides:
//! - `BuildCtx` as LuaUserData with methods like `fetch_url` and `exec`
//! - `register_sys_build()` to register the `sys.build` function
//! - Helper functions for converting between Lua values and Rust types

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use mlua::prelude::*;

use crate::action::BUILD_CTX_METHODS_REGISTRY_KEY;
use crate::action::actions::exec::parse_exec_opts;
use crate::manifest::Manifest;
use crate::outputs::lua::parse_outputs;
use crate::{bind::BIND_REF_TYPE, util::hash::ObjectHash};

use super::{BUILD_REF_TYPE, BuildCtx, BuildDef, BuildInputs, BuildRef, BuildSpec};

impl LuaUserData for BuildCtx {
  fn add_fields<F: LuaUserDataFields<Self>>(fields: &mut F) {
    fields.add_field_method_get("out", |_, this| Ok(this.out().to_string()));
    fields.add_field_method_get("action_count", |_, this| Ok(this.action_count()));
  }

  fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
    methods.add_method_mut("fetch_url", |_, this, (url, sha256): (String, String)| {
      Ok(this.fetch_url(&url, &sha256))
    });

    methods.add_method_mut("exec", |_, this, (opts, args): (LuaValue, Option<LuaValue>)| {
      let cmd_opts = parse_exec_opts(opts, args)?;
      Ok(this.exec(cmd_opts))
    });

    // Fallback for custom registered methods (build-specific registry)
    methods.add_meta_method(mlua::MetaMethod::Index, |lua, _this, key: String| {
      let registry: LuaTable = lua.named_registry_value(BUILD_CTX_METHODS_REGISTRY_KEY)?;
      let func: LuaValue = registry.get(key.as_str())?;

      match func {
        LuaValue::Function(_) => Ok(func),
        LuaValue::Nil => Err(LuaError::external(format!(
          "unknown build ctx method '{}'. Use sys.register_build_ctx_method to add custom methods.",
          key
        ))),
        _ => Err(LuaError::external(format!(
          "build ctx method '{}' is not a function",
          key
        ))),
      }
    });
  }
}

/// Convert a Lua value to BuildInputsRef (for resolved/static inputs).
///
/// Handles primitives, arrays, tables, and specially-marked BuildRef/BindRef tables
/// (detected via metatable `__type` field).
///
/// Validates that any referenced builds/binds exist in the manifest.
pub fn lua_value_to_build_inputs_ref(value: LuaValue, manifest: &Manifest) -> LuaResult<BuildInputs> {
  match value {
    LuaValue::String(s) => Ok(BuildInputs::String(s.to_str()?.to_string())),
    LuaValue::Number(n) => Ok(BuildInputs::Number(n)),
    LuaValue::Integer(i) => Ok(BuildInputs::Number(i as f64)),
    LuaValue::Boolean(b) => Ok(BuildInputs::Boolean(b)),
    LuaValue::Table(t) => {
      // Check metatable for type marker (BuildRef or BindRef)
      if let Some(mt) = t.metatable()
        && let Ok(type_name) = mt.get::<String>("__type")
      {
        match type_name.as_str() {
          BUILD_REF_TYPE => return parse_build_ref_table(&t, manifest),
          BIND_REF_TYPE => {
            return Err(LuaError::external(
              "build inputs cannot reference binds: binds are side-effectful and cannot be inputs to immutable builds",
            ));
          }
          _ => {}
        }
      }

      // Check if it's an array (sequential integer keys starting at 1)
      let len = t.raw_len();
      let first_key: Result<LuaValue, _> = t.get(1i64);
      if len > 0 && first_key.is_ok() && first_key.unwrap() != LuaValue::Nil {
        // Treat as array
        let mut arr = Vec::with_capacity(len);
        for i in 1..=len {
          let val: LuaValue = t.get(i)?;
          arr.push(lua_value_to_build_inputs_ref(val, manifest)?);
        }
        Ok(BuildInputs::Array(arr))
      } else {
        // Treat as table/map
        let mut map = BTreeMap::new();
        for pair in t.pairs::<String, LuaValue>() {
          let (k, v) = pair?;
          map.insert(k, lua_value_to_build_inputs_ref(v, manifest)?);
        }
        Ok(BuildInputs::Table(map))
      }
    }
    LuaValue::Nil => Err(LuaError::external("nil values not allowed in inputs")),
    _ => Err(LuaError::external(format!(
      "unsupported input type: {:?}",
      value.type_name()
    ))),
  }
}

/// Parse a Lua table marked as BuildRef into InputsRef::Build.
///
/// Validates that the referenced build exists in the manifest.
pub fn parse_build_ref_table(t: &LuaTable, manifest: &Manifest) -> LuaResult<BuildInputs> {
  let hash: String = t.get("hash")?;
  let build_hash = ObjectHash(hash);

  // Validate build exists in manifest
  if !manifest.builds.contains_key(&build_hash) {
    return Err(LuaError::external(format!(
      "referenced build not found in manifest: {}",
      build_hash.0
    )));
  }

  Ok(BuildInputs::Build(build_hash))
}

/// Convert BindInputsRef to a Lua value for passing to the create function.
///
/// For Build/Bind references, looks up the definition in the manifest to
/// reconstruct the Lua table with placeholder outputs.
pub fn build_inputs_def_to_lua(lua: &Lua, inputs: &BuildInputs, manifest: &Manifest) -> LuaResult<LuaValue> {
  match inputs {
    BuildInputs::String(s) => Ok(LuaValue::String(lua.create_string(s)?)),
    BuildInputs::Number(n) => Ok(LuaValue::Number(*n)),
    BuildInputs::Boolean(b) => Ok(LuaValue::Boolean(*b)),
    BuildInputs::Array(arr) => {
      let table = lua.create_table()?;
      for (i, val) in arr.iter().enumerate() {
        table.set(i + 1, build_inputs_def_to_lua(lua, val, manifest)?)?;
      }
      Ok(LuaValue::Table(table))
    }
    BuildInputs::Table(map) => {
      let table = lua.create_table()?;
      for (k, v) in map {
        table.set(k.as_str(), build_inputs_def_to_lua(lua, v, manifest)?)?;
      }
      Ok(LuaValue::Table(table))
    }
    BuildInputs::Build(hash) => build_hash_to_lua(lua, hash, manifest),
  }
}

/// Convert a ObjectHash to a Lua table by looking up the BuildDef in the manifest.
///
/// Generates placeholder outputs from the BuildDef's output keys.
pub fn build_hash_to_lua(lua: &Lua, hash: &ObjectHash, manifest: &Manifest) -> LuaResult<LuaValue> {
  let build_def = manifest
    .builds
    .get(hash)
    .ok_or_else(|| LuaError::external(format!("build not found in manifest: {}", hash.0)))?;

  let table = lua.create_table()?;
  table.set("id", build_def.id.as_deref())?;
  table.set("hash", hash.0.as_str())?;

  // Generate placeholder outputs from BuildDef
  let outputs = lua.create_table()?;
  let hash = &hash.0;
  if let Some(def_outputs) = &build_def.outputs {
    for key in def_outputs.keys() {
      let placeholder = format!("$${{{{build:{}:{}}}}}", hash, key);
      outputs.set(key.as_str(), placeholder.as_str())?;
    }
  }
  table.set("outputs", outputs)?;

  // Set metatable with __type marker
  let mt = lua.create_table()?;
  mt.set("__type", BUILD_REF_TYPE)?;
  table.set_metatable(Some(mt))?;

  Ok(LuaValue::Table(table))
}

/// Register the `sys.build` function on the sys table.
///
/// The `sys.build{}` function:
/// 1. Parses a BuildSpec from the Lua table (id, inputs, create)
/// 2. Resolves inputs (calls function if dynamic, uses table directly if static)
/// 3. Creates a BuildCtx and calls the create function
/// 4. Captures the returned outputs (must be non-empty)
/// 5. Creates a BuildDef, computes its hash, and adds it to the manifest
/// 6. Returns a BuildRef as a Lua table with metatable marker
pub fn register_sys_build(lua: &Lua, sys_table: &LuaTable, manifest: Rc<RefCell<Manifest>>) -> LuaResult<()> {
  let build_fn = lua.create_function(move |lua, spec_table: LuaTable| {
    let build_spec: BuildSpec = lua.unpack(LuaValue::Table(spec_table))?;
    let id = build_spec.id.clone();
    let replace = build_spec.replace;

    let build_def = BuildDef::from_spec(
      lua,
      &manifest,
      build_spec,
      lua_value_to_build_inputs_ref,
      build_inputs_def_to_lua,
      parse_outputs,
    )?;

    let build_ref = BuildRef::from_def(&build_def)?;

    {
      let mut manifest = manifest.borrow_mut();

      // Hash dedup (existing behavior): identical content = same hash
      if manifest.builds.contains_key(&build_ref.hash) {
        tracing::warn!(
          hash = %build_ref.hash.0,
          id = ?id,
          "duplicate build detected, skipping insertion"
        );
        return lua.pack(build_ref);
      }

      // ID dedup with explicit replace flag
      if let Some(ref build_id) = id {
        let existing = manifest
          .builds
          .iter()
          .find(|(_, def)| def.id.as_ref() == Some(build_id))
          .map(|(h, _)| h.clone());

        if let Some(old_hash) = existing {
          if !replace {
            return Err(LuaError::external(format!(
              "build with id '{}' already exists (hash: {}). Use `replace = true` to override, \
               or use a different id. This error prevents accidental collisions.",
              build_id, old_hash.0
            )));
          }
          manifest.builds.remove(&old_hash);
        }
      }

      manifest.builds.insert(build_ref.hash.clone(), build_def);
    }

    lua.pack(build_ref)
  })?;

  sys_table.set("build", build_fn)?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::lua::globals::register_globals;

  fn create_test_lua_with_manifest() -> LuaResult<(Lua, Rc<RefCell<Manifest>>)> {
    let lua = Lua::new();
    let manifest = Rc::new(RefCell::new(Manifest::default()));

    // register_globals sets up sys table including sys.build
    register_globals(&lua, manifest.clone())?;

    Ok((lua, manifest))
  }

  mod sys_build {
    use crate::{action::Action, consts::OBJ_HASH_PREFIX_LEN};

    use super::*;

    #[test]
    fn simple_build_returns_build_ref() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      let result: LuaTable = lua
        .load(
          r#"
                return sys.build({
                    id = "test-pkg",
                    create = function(inputs, ctx)
                        ctx:exec("make install")
                        return { out = "/path/to/output" }
                    end,
                })
            "#,
        )
        .eval()?;

      // Check returned BuildRef
      let id: String = result.get("id")?;
      assert_eq!(id, "test-pkg");

      let hash: String = result.get("hash")?;
      assert!(!hash.is_empty(), "hash should not be empty");

      let outputs: LuaTable = result.get("outputs")?;
      let out: String = outputs.get("out")?;
      // Output should be a placeholder with the hash
      let hash: String = result.get("hash")?;
      assert_eq!(out, format!("$${{{{build:{}:out}}}}", hash));

      // Check metatable
      let mt = result.metatable().expect("should have metatable");
      let type_name: String = mt.get("__type")?;
      assert_eq!(type_name, "BuildRef");

      // Check manifest was updated
      let manifest = manifest.borrow();
      assert_eq!(manifest.builds.len(), 1);
      let (_, build_def) = manifest.builds.iter().next().unwrap();
      assert_eq!(build_def.id, Some("test-pkg".to_string()));

      Ok(())
    }

    #[test]
    fn build_with_static_inputs() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      lua
        .load(
          r#"
                return sys.build({
                    id = "with-inputs",
                    inputs = { url = "https://example.com/src.tar.gz", sha256 = "abc123" },
                    create = function(inputs, ctx)
                        local archive = ctx:fetch_url(inputs.url, inputs.sha256)
                        ctx:exec("tar -xzf " .. archive)
                        return { out = "/build/output" }
                    end,
                })
            "#,
        )
        .eval::<LuaTable>()?;

      let manifest = manifest.borrow();
      assert_eq!(manifest.builds.len(), 1);

      // Check inputs were captured
      let (_, build_def) = manifest.builds.iter().next().unwrap();
      let inputs = build_def.inputs.as_ref().expect("should have inputs");
      match inputs {
        BuildInputs::Table(map) => {
          assert!(map.contains_key("url"));
          assert!(map.contains_key("sha256"));
        }
        _ => panic!("expected Table inputs"),
      }

      // Check actions were recorded
      assert_eq!(build_def.create_actions.len(), 2); // fetch_url + cmd

      Ok(())
    }

    #[test]
    fn build_with_dynamic_inputs() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      lua
        .load(
          r#"
                return sys.build({
                    id = "dynamic-inputs",
                    inputs = function()
                        return { computed = "value" }
                    end,
                    create = function(inputs, ctx)
                        ctx:exec("echo " .. inputs.computed)
                        return { out = "/output" }
                    end,
                })
            "#,
        )
        .eval::<LuaTable>()?;

      let manifest = manifest.borrow();
      let (_, build_def) = manifest.builds.iter().next().unwrap();
      let inputs = build_def.inputs.as_ref().expect("should have inputs");
      match inputs {
        BuildInputs::Table(map) => {
          assert_eq!(map.get("computed"), Some(&BuildInputs::String("value".to_string())));
        }
        _ => panic!("expected Table inputs"),
      }

      Ok(())
    }

    #[test]
    fn build_ref_can_be_used_as_input() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      lua
        .load(
          r#"
                local dep = sys.build({
                    id = "dependency",
                    create = function(inputs, ctx)
                        ctx:exec("make dep")
                        return { out = "/dep/output" }
                    end,
                })

                return sys.build({
                    id = "consumer",
                    inputs = { dep = dep },
                    create = function(inputs, ctx)
                        ctx:exec("make -I " .. inputs.dep.outputs.out)
                        return { out = "/consumer/output" }
                    end,
                })
            "#,
        )
        .eval::<LuaTable>()?;

      let manifest = manifest.borrow();
      assert_eq!(manifest.builds.len(), 2);

      // Check the consumer's inputs contain the ObjectHash
      // The consumer is the one with id = "consumer"
      let consumer = manifest
        .builds
        .values()
        .find(|b| b.id == Some("consumer".to_string()))
        .unwrap();
      let inputs = consumer.inputs.as_ref().expect("should have inputs");
      match inputs {
        BuildInputs::Table(map) => {
          let dep = map.get("dep").expect("should have dep key");
          match dep {
            BuildInputs::Build(build_hash) => {
              // Verify it's a truncated hash (HASH_PREFIX_LEN hex chars)
              assert_eq!(build_hash.0.len(), OBJ_HASH_PREFIX_LEN);
              // Verify the referenced build exists
              assert!(manifest.builds.contains_key(build_hash));
            }
            _ => panic!("expected Build ref"),
          }
        }
        _ => panic!("expected Table inputs"),
      }

      Ok(())
    }

    #[test]
    fn build_without_create_fails() -> LuaResult<()> {
      let (lua, _) = create_test_lua_with_manifest()?;

      let result = lua
        .load(
          r#"
                return sys.build({
                    id = "no-create",
                })
            "#,
        )
        .eval::<LuaTable>();

      assert!(result.is_err());
      let err = result.unwrap_err().to_string();
      assert!(err.contains("create"), "error should mention 'create': {}", err);

      Ok(())
    }

    #[test]
    fn build_with_empty_outputs_fails() -> LuaResult<()> {
      let (lua, _) = create_test_lua_with_manifest()?;

      let result = lua
        .load(
          r#"
                return sys.build({
                    id = "empty-outputs",
                    create = function(inputs, ctx)
                        return {}
                    end,
                })
            "#,
        )
        .eval::<LuaTable>();

      assert!(result.is_err());
      let err = result.unwrap_err().to_string();
      assert!(err.contains("non-empty"), "error should mention 'non-empty': {}", err);

      Ok(())
    }

    #[test]
    fn build_with_nil_outputs_fails() -> LuaResult<()> {
      let (lua, _) = create_test_lua_with_manifest()?;

      let result = lua
        .load(
          r#"
                return sys.build({
                    id = "nil-outputs",
                    create = function(inputs, ctx)
                        return nil
                    end,
                })
            "#,
        )
        .eval::<LuaTable>();

      assert!(result.is_err());
      let err = result.unwrap_err().to_string();
      assert!(
        err.contains("non-empty") || err.contains("nil"),
        "error should mention outputs issue: {}",
        err
      );

      Ok(())
    }

    #[test]
    fn multiple_builds_added_to_manifest() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      lua
        .load(
          r#"
                sys.build({ id = "pkg1", create = function(i, c) c:exec("a"); return { out = "x" } end })
                sys.build({ id = "pkg2", create = function(i, c) c:exec("b"); return { out = "y" } end })
                sys.build({ id = "pkg3", create = function(i, c) c:exec("c"); return { out = "z" } end })
            "#,
        )
        .exec()?;

      let manifest = manifest.borrow();
      assert_eq!(manifest.builds.len(), 3);

      // Check all names are present (order in BTreeMap is by hash, not insertion order)
      let ids: Vec<_> = manifest.builds.values().map(|b| b.id.as_deref()).collect();
      assert!(ids.contains(&Some("pkg1")));
      assert!(ids.contains(&Some("pkg2")));
      assert!(ids.contains(&Some("pkg3")));

      Ok(())
    }

    #[test]
    fn build_hash_is_deterministic() -> LuaResult<()> {
      // Create two separate Lua runtimes with the same build
      let (lua1, _) = create_test_lua_with_manifest()?;
      let (lua2, _) = create_test_lua_with_manifest()?;

      let code = r#"
                return sys.build({
                    id = "deterministic-1.0.0",
                    inputs = { key = "value" },
                    create = function(inputs, ctx)
                        ctx:exec("make")
                        return { out = "/output" }
                    end,
                })
            "#;

      let ref1: LuaTable = lua1.load(code).eval()?;
      let ref2: LuaTable = lua2.load(code).eval()?;

      let hash1: String = ref1.get("hash")?;
      let hash2: String = ref2.get("hash")?;

      assert_eq!(hash1, hash2, "same build should produce same hash");

      Ok(())
    }

    #[test]
    fn duplicate_build_is_deduplicated() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      lua
        .load(
          r#"
                sys.build({
                    id = "same-pkg",
                    create = function(inputs, ctx)
                        ctx:exec("make")
                        return { out = "/output" }
                    end,
                })
                sys.build({
                    id = "same-pkg",
                    create = function(inputs, ctx)
                        ctx:exec("make")
                        return { out = "/output" }
                    end,
                })
            "#,
        )
        .exec()?;

      let manifest = manifest.borrow();
      assert_eq!(manifest.builds.len(), 1);

      Ok(())
    }

    #[test]
    fn duplicate_build_id_without_replace_fails() -> LuaResult<()> {
      let (lua, _) = create_test_lua_with_manifest()?;

      let result = lua
        .load(
          r#"
                sys.build({
                    id = "my-build",
                    create = function(inputs, ctx)
                        ctx:exec("echo first")
                        return { out = "/first" }
                    end,
                })
                sys.build({
                    id = "my-build",
                    create = function(inputs, ctx)
                        ctx:exec("echo second")
                        return { out = "/second" }
                    end,
                })
            "#,
        )
        .exec();

      assert!(result.is_err());
      let err = result.unwrap_err().to_string();
      assert!(
        err.contains("already exists"),
        "error should mention 'already exists': {}",
        err
      );
      assert!(
        err.contains("replace = true"),
        "error should suggest replace flag: {}",
        err
      );

      Ok(())
    }

    #[test]
    fn duplicate_build_id_with_replace_succeeds() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      lua
        .load(
          r#"
                sys.build({
                    id = "my-build",
                    create = function(inputs, ctx)
                        ctx:exec("echo first")
                        return { out = "/first" }
                    end,
                })
                sys.build({
                    id = "my-build",
                    replace = true,
                    create = function(inputs, ctx)
                        ctx:exec("echo second")
                        return { out = "/second" }
                    end,
                })
            "#,
        )
        .exec()?;

      let manifest = manifest.borrow();
      assert_eq!(manifest.builds.len(), 1);

      let (_, build_def) = manifest.builds.iter().next().unwrap();
      match &build_def.create_actions[0] {
        Action::Exec(opts) => {
          assert_eq!(opts.bin, "echo second", "second build should replace first");
        }
        _ => panic!("expected Exec action"),
      }

      Ok(())
    }

    #[test]
    fn replace_true_on_first_build_succeeds() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      lua
        .load(
          r#"
                sys.build({
                    id = "my-build",
                    replace = true,
                    create = function(inputs, ctx)
                        ctx:exec("echo only")
                        return { out = "/only" }
                    end,
                })
            "#,
        )
        .exec()?;

      let manifest = manifest.borrow();
      assert_eq!(manifest.builds.len(), 1);

      Ok(())
    }

    #[test]
    fn different_build_ids_both_kept() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      lua
        .load(
          r#"
                sys.build({
                    id = "build-a",
                    create = function(inputs, ctx)
                        ctx:exec("echo a")
                        return { out = "/a" }
                    end,
                })
                sys.build({
                    id = "build-b",
                    create = function(inputs, ctx)
                        ctx:exec("echo b")
                        return { out = "/b" }
                    end,
                })
            "#,
        )
        .exec()?;

      let manifest = manifest.borrow();
      assert_eq!(manifest.builds.len(), 2);

      Ok(())
    }

    #[test]
    fn ctx_out_returns_placeholder() -> LuaResult<()> {
      let (lua, _) = create_test_lua_with_manifest()?;

      // Test that ctx.out returns the $${out} placeholder
      let result: LuaTable = lua
        .load(
          r#"
                return sys.build({
                    id = "test-out",
                    create = function(inputs, ctx)
                        -- ctx.out should return $${out} placeholder
                        ctx:exec("mkdir -p " .. ctx.out .. "/bin")
                        return { out = ctx.out }
                    end,
                })
            "#,
        )
        .eval()?;

      let outputs: LuaTable = result.get("outputs")?;
      let out: String = outputs.get("out")?;

      // The output value should contain the build placeholder (resolved from ctx.out)
      // Since ctx.out returns "$${{out}}" and that's returned as the output value,
      // the final placeholder wraps it as $${{build:HASH:out}}
      let hash: String = result.get("hash")?;
      assert_eq!(out, format!("$${{{{build:{}:out}}}}", hash));

      Ok(())
    }

    #[test]
    fn ctx_out_can_be_used_in_commands() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      lua
        .load(
          r#"
                sys.build({
                    id = "uses-out",
                    create = function(inputs, ctx)
                        ctx:exec("mkdir -p " .. ctx.out .. "/bin")
                        ctx:exec("cp binary " .. ctx.out .. "/bin/")
                        return { out = ctx.out }
                    end,
                })
            "#,
        )
        .exec()?;

      let manifest = manifest.borrow();
      let (_, build_def) = manifest.builds.iter().next().unwrap();

      // Check that the commands contain the $${{out}} placeholder
      assert_eq!(build_def.create_actions.len(), 2);

      match &build_def.create_actions[0] {
        Action::Exec(opts) => {
          assert!(
            opts.bin.contains("$${{out}}"),
            "cmd should contain $${{{{out}}}} placeholder: {}",
            opts.bin
          );
          assert_eq!(opts.bin, "mkdir -p $${{out}}/bin");
        }
        _ => panic!("expected Cmd action"),
      }

      match &build_def.create_actions[1] {
        Action::Exec(opts) => {
          assert!(
            opts.bin.contains("$${{out}}"),
            "cmd should contain $${{{{out}}}} placeholder: {}",
            opts.bin
          );
          assert_eq!(opts.bin, "cp binary $${{out}}/bin/");
        }
        _ => panic!("expected Cmd action"),
      }

      Ok(())
    }

    #[test]
    fn build_with_bind_input_fails() -> LuaResult<()> {
      let (lua, _) = create_test_lua_with_manifest()?;

      let result = lua
        .load(
          r#"
                local link = sys.bind({
                    id = "symlink-src-dest",
                    create = function(inputs, ctx)
                        ctx:exec("ln -sf /src /dest")
                    end,
                    destroy = function(inputs, ctx)
                        ctx:exec("rm /dest")
                    end,
                })

                return sys.build({
                    id = "invalid-build",
                    inputs = { bind_dep = link },
                    create = function(inputs, ctx)
                        ctx:exec("make")
                        return { out = "/output" }
                    end,
                })
            "#,
        )
        .eval::<LuaTable>();

      assert!(result.is_err());
      let err = result.unwrap_err().to_string();
      assert!(
        err.contains("cannot reference binds") || err.contains("side-effectful"),
        "error should explain bind ref constraint: {}",
        err
      );

      Ok(())
    }

    #[test]
    fn build_with_nested_bind_input_fails() -> LuaResult<()> {
      let (lua, _) = create_test_lua_with_manifest()?;

      // Test that nested bind references are also caught
      let result = lua
        .load(
          r#"
                local link = sys.bind({
                    id = "symlink-src-dest",
                    create = function(inputs, ctx)
                        ctx:exec("ln -sf /src /dest")
                    end,
                    destroy = function(inputs, ctx)
                        ctx:exec("rm /dest")
                    end,
                })

                return sys.build({
                    id = "invalid-build",
                    inputs = {
                        nested = {
                            deep = { bind_dep = link }
                        }
                    },
                    create = function(inputs, ctx)
                        ctx:exec("make")
                        return { out = "/output" }
                    end,
                })
            "#,
        )
        .eval::<LuaTable>();

      assert!(result.is_err());
      let err = result.unwrap_err().to_string();
      assert!(
        err.contains("cannot reference binds") || err.contains("side-effectful"),
        "error should explain bind ref constraint for nested refs: {}",
        err
      );

      Ok(())
    }
  }
}
