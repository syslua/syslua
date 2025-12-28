//! Lua bindings for `sys.bind{}`.
//!
//! This module provides:
//! - `BindCtx` as LuaUserData with methods like `exec`
//! - `register_sys_bind()` to register the `sys.bind` function

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use mlua::prelude::*;

use crate::action::BIND_CTX_METHODS_REGISTRY_KEY;
use crate::action::actions::exec::parse_exec_opts;
use crate::bind::{BindInputsDef, BindRef, BindSpec};
use crate::build::BUILD_REF_TYPE;
use crate::build::lua::build_hash_to_lua;
use crate::manifest::Manifest;
use crate::util::hash::ObjectHash;

use super::{BIND_REF_TYPE, BindCtx, BindDef};

impl LuaUserData for BindCtx {
  fn add_fields<F: LuaUserDataFields<Self>>(fields: &mut F) {
    fields.add_field_method_get("out", |_, this| Ok(this.out().to_string()));
  }

  fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
    // NO fetch_url here - binds should only use build outputs

    methods.add_method_mut("exec", |_, this, (opts, args): (LuaValue, Option<LuaValue>)| {
      let cmd_opts = parse_exec_opts(opts, args)?;
      Ok(this.exec(cmd_opts))
    });

    // Fallback for custom registered methods (bind-specific registry)
    methods.add_meta_method(mlua::MetaMethod::Index, |lua, _this, key: String| {
      let registry: LuaTable = lua.named_registry_value(BIND_CTX_METHODS_REGISTRY_KEY)?;
      let func: LuaValue = registry.get(key.as_str())?;

      match func {
        LuaValue::Function(_) => Ok(func),
        LuaValue::Nil => Err(LuaError::external(format!(
          "unknown bind ctx method '{}'. Use sys.register_bind_ctx_method to add custom methods.",
          key
        ))),
        _ => Err(LuaError::external(format!(
          "bind ctx method '{}' is not a function",
          key
        ))),
      }
    });
  }
}

/// Convert a Lua value to BindInputsRef (for resolved/static inputs).
///
/// Handles primitives, arrays, tables, and specially-marked BuildRef/BindRef tables
/// (detected via metatable `__type` field).
///
/// Validates that any referenced builds/binds exist in the manifest.
pub fn lua_value_to_bind_inputs_def(value: LuaValue, manifest: &Manifest) -> LuaResult<BindInputsDef> {
  match value {
    LuaValue::String(s) => Ok(BindInputsDef::String(s.to_str()?.to_string())),
    LuaValue::Number(n) => Ok(BindInputsDef::Number(n)),
    LuaValue::Integer(i) => Ok(BindInputsDef::Number(i as f64)),
    LuaValue::Boolean(b) => Ok(BindInputsDef::Boolean(b)),
    LuaValue::Table(t) => {
      // Check metatable for type marker (BuildRef or BindRef)
      if let Some(mt) = t.metatable()
        && let Ok(type_name) = mt.get::<String>("__type")
      {
        match type_name.as_str() {
          BUILD_REF_TYPE => return parse_build_ref_table(&t, manifest),
          BIND_REF_TYPE => return parse_bind_ref_table(&t, manifest),
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
          arr.push(lua_value_to_bind_inputs_def(val, manifest)?);
        }
        Ok(BindInputsDef::Array(arr))
      } else {
        // Treat as table/map
        let mut map = BTreeMap::new();
        for pair in t.pairs::<String, LuaValue>() {
          let (k, v) = pair?;
          map.insert(k, lua_value_to_bind_inputs_def(v, manifest)?);
        }
        Ok(BindInputsDef::Table(map))
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
fn parse_build_ref_table(t: &LuaTable, manifest: &Manifest) -> LuaResult<BindInputsDef> {
  let hash: String = t.get("hash")?;
  let build_hash = ObjectHash(hash);

  // Validate build exists in manifest
  if !manifest.builds.contains_key(&build_hash) {
    return Err(LuaError::external(format!(
      "referenced build not found in manifest: {}",
      build_hash.0
    )));
  }

  Ok(BindInputsDef::Build(build_hash))
}

/// Parse a Lua table marked as BindRef into InputsRef::Bind.
///
/// Validates that the referenced bind exists in the manifest.
fn parse_bind_ref_table(t: &LuaTable, manifest: &Manifest) -> LuaResult<BindInputsDef> {
  let hash: String = t.get("hash")?;
  let bind_hash = ObjectHash(hash);

  // Validate bind exists in manifest
  if !manifest.bindings.contains_key(&bind_hash) {
    return Err(LuaError::external(format!(
      "referenced bind not found in manifest: {}",
      bind_hash.0
    )));
  }

  Ok(BindInputsDef::Bind(bind_hash))
}

/// Convert BindInputsRef to a Lua value for passing to the create function.
///
/// For Build/Bind references, looks up the definition in the manifest to
/// reconstruct the Lua table with placeholder outputs.
pub fn bind_inputs_ref_to_lua(lua: &Lua, inputs: &BindInputsDef, manifest: &Manifest) -> LuaResult<LuaValue> {
  match inputs {
    BindInputsDef::String(s) => Ok(LuaValue::String(lua.create_string(s)?)),
    BindInputsDef::Number(n) => Ok(LuaValue::Number(*n)),
    BindInputsDef::Boolean(b) => Ok(LuaValue::Boolean(*b)),
    BindInputsDef::Array(arr) => {
      let table = lua.create_table()?;
      for (i, val) in arr.iter().enumerate() {
        table.set(i + 1, bind_inputs_ref_to_lua(lua, val, manifest)?)?;
      }
      Ok(LuaValue::Table(table))
    }
    BindInputsDef::Table(map) => {
      let table = lua.create_table()?;
      for (k, v) in map {
        table.set(k.as_str(), bind_inputs_ref_to_lua(lua, v, manifest)?)?;
      }
      Ok(LuaValue::Table(table))
    }
    BindInputsDef::Build(hash) => build_hash_to_lua(lua, hash, manifest),
    BindInputsDef::Bind(hash) => bind_hash_to_lua(lua, hash, manifest),
  }
}

/// Convert a BindHash to a Lua table by looking up the BindDef in the manifest.
///
/// Generates placeholder outputs from the BindDef's output keys (if present).
pub fn bind_hash_to_lua(lua: &Lua, hash: &ObjectHash, manifest: &Manifest) -> LuaResult<LuaValue> {
  let bind_def = manifest
    .bindings
    .get(hash)
    .ok_or_else(|| LuaError::external(format!("bind not found in manifest: {}", hash.0)))?;

  let table = lua.create_table()?;
  table.set("hash", hash.0.as_str())?;

  // Generate placeholder outputs from BindDef (if present)
  if let Some(def_outputs) = &bind_def.outputs {
    let outputs = lua.create_table()?;
    let hash = &hash.0;
    for key in def_outputs.keys() {
      let placeholder = format!("$${{bind:{}:{}}}", hash, key);
      outputs.set(key.as_str(), placeholder.as_str())?;
    }
    table.set("outputs", outputs)?;
  }

  // Set metatable with __type marker
  let mt = lua.create_table()?;
  mt.set("__type", BIND_REF_TYPE)?;
  table.set_metatable(Some(mt))?;

  Ok(LuaValue::Table(table))
}

/// Register the `sys.bind` function on the sys table.
///
/// The `sys.bind{}` function:
/// 1. Parses a BindSpec from the Lua table (inputs, create, update, destroy)
/// 2. Resolves inputs (calls function if dynamic, uses table directly if static)
/// 3. Creates a ActionCtx and calls the create function
/// 4. Optionally calls the destroy function with a fresh ActionCtx
/// 5. Creates a BindDef, computes its hash, and adds it to the manifest
/// 6. Returns a BindRef as a Lua table with metatable marker
pub fn register_sys_bind(lua: &Lua, sys_table: &LuaTable, manifest: Rc<RefCell<Manifest>>) -> LuaResult<()> {
  let bind_fn = lua.create_function(move |lua, spec_table: LuaTable| {
    let bind_spec: BindSpec = lua.unpack(LuaValue::Table(spec_table))?;
    let bind_def = BindDef::from_spec(lua, &manifest, bind_spec)?;
    let bind_ref = BindRef::from_def(&bind_def)?;

    // Check for duplicate bind IDs (only for binds with IDs)
    if let Some(ref id) = bind_def.id {
      let manifest_ref = manifest.borrow();
      for (existing_hash, existing_def) in manifest_ref.bindings.iter() {
        if let Some(ref existing_id) = existing_def.id
          && existing_id == id
          && *existing_hash != bind_ref.hash
        {
          return Err(LuaError::external(format!(
            "duplicate bind id '{}': a bind with this id already exists (hash: {})",
            id, existing_hash.0
          )));
        }
      }
    }

    // Add to manifest (deduplicate by hash)
    {
      let mut manifest = manifest.borrow_mut();
      if manifest.bindings.contains_key(&bind_ref.hash) {
        tracing::warn!(
          hash = %bind_ref.hash.0,
          "duplicate bind detected, skipping insertion"
        );
      } else {
        manifest.bindings.insert(bind_ref.hash.clone(), bind_def.clone());
      }
    }

    lua.pack(bind_ref)
  })?;

  sys_table.set("bind", bind_fn)?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::lua::globals::register_globals;

  fn create_test_lua_with_manifest() -> LuaResult<(Lua, Rc<RefCell<Manifest>>)> {
    let lua = Lua::new();
    let manifest = Rc::new(RefCell::new(Manifest::default()));

    // register_globals sets up sys table including sys.build and sys.bind
    register_globals(&lua, manifest.clone())?;

    Ok((lua, manifest))
  }

  mod sys_bind {
    use crate::{action::Action, consts::OBJ_HASH_PREFIX_LEN};

    use super::*;

    #[test]
    fn simple_bind_returns_bind_ref() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      let result: LuaTable = lua
        .load(
          r#"
                return sys.bind({
                    id = "simple-bind",
                    create = function(inputs, ctx)
                        ctx:exec("ln -sf /src /dest")
                    end,
                    destroy = function(outputs, ctx)
                        ctx:exec("rm /dest")
                    end,
                })
            "#,
        )
        .eval()?;

      // Check returned BindRef
      let hash: String = result.get("hash")?;
      assert!(!hash.is_empty(), "hash should not be empty");

      // Check metatable
      let mt = result.metatable().expect("should have metatable");
      let type_name: String = mt.get("__type")?;
      assert_eq!(type_name, "BindRef");

      // Check manifest was updated
      let manifest = manifest.borrow();
      assert_eq!(manifest.bindings.len(), 1);
      let (_, bind_def) = manifest.bindings.iter().next().unwrap();
      assert_eq!(bind_def.create_actions.len(), 1);

      Ok(())
    }

    #[test]
    fn bind_with_outputs() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      let result: LuaTable = lua
        .load(
          r#"
                return sys.bind({
                    id = "bind-with-outputs",
                    create = function(inputs, ctx)
                        ctx:exec("ln -sf /src /dest")
                        return { link = "/dest" }
                    end,
                    destroy = function(outputs, ctx)
                        ctx:exec("rm /dest")
                    end,
                })
            "#,
        )
        .eval()?;

      // Check outputs are present as placeholders
      let outputs: LuaTable = result.get("outputs")?;
      let link: String = outputs.get("link")?;
      // Output should be a placeholder with the hash
      let hash: String = result.get("hash")?;
      assert_eq!(link, format!("$${{bind:{}:link}}", hash));

      // Check manifest
      let manifest = manifest.borrow();
      let (_, bind_def) = manifest.bindings.iter().next().unwrap();
      assert!(bind_def.outputs.is_some());

      Ok(())
    }

    #[test]
    fn bind_with_destroy() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      lua
        .load(
          r#"
                return sys.bind({
                    id = "bind-with-destroy",
                    create = function(inputs, ctx)
                        ctx:exec("ln -sf /src /dest")
                    end,
                    destroy = function(inputs, ctx)
                        ctx:exec("rm /dest")
                    end,
                })
            "#,
        )
        .eval::<LuaTable>()?;

      let manifest = manifest.borrow();
      assert_eq!(manifest.bindings.len(), 1);
      let (_, bind_def) = manifest.bindings.iter().next().unwrap();
      assert_eq!(bind_def.destroy_actions.len(), 1);

      Ok(())
    }

    #[test]
    fn bind_with_inputs_from_build() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      lua
        .load(
          r#"
                local pkg = sys.build({
                    id = "my-pkg",
                    create = function(inputs, ctx)
                        ctx:exec("make install")
                        return { out = "/store/my-pkg" }
                    end,
                })

                return sys.bind({
                    id = "bind-with-build-input",
                    inputs = { pkg = pkg },
                    create = function(inputs, ctx)
                        ctx:exec("ln -sf " .. inputs.pkg.outputs.out .. "/bin/app /usr/local/bin/app")
                    end,
                    destroy = function(inputs, ctx)
                        ctx:exec("rm /usr/local/bin/app")
                    end,
                })
            "#,
        )
        .eval::<LuaTable>()?;

      let manifest = manifest.borrow();
      assert_eq!(manifest.builds.len(), 1);
      assert_eq!(manifest.bindings.len(), 1);

      // Check the bind's inputs contain the BuildHash
      let (_, bind) = manifest.bindings.iter().next().unwrap();
      let inputs = bind.inputs.as_ref().expect("should have inputs");
      match inputs {
        BindInputsDef::Table(map) => {
          let pkg = map.get("pkg").expect("should have pkg key");
          match pkg {
            BindInputsDef::Build(build_hash) => {
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
    fn bind_with_static_inputs() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      lua
        .load(
          r#"
                return sys.bind({
                    id = "bind-with-static-inputs",
                    inputs = { src = "/path/to/source", dest = "/path/to/dest" },
                    create = function(inputs, ctx)
                        ctx:exec("ln -sf " .. inputs.src .. " " .. inputs.dest)
                        return { dest = inputs.dest }
                    end,
                    destroy = function(outputs, ctx)
                        ctx:exec("rm " .. outputs.dest)
                    end,
                })
            "#,
        )
        .eval::<LuaTable>()?;

      let manifest = manifest.borrow();
      let (_, bind_def) = manifest.bindings.iter().next().unwrap();
      let inputs = bind_def.inputs.as_ref().expect("should have inputs");
      match inputs {
        BindInputsDef::Table(map) => {
          assert_eq!(
            map.get("src"),
            Some(&BindInputsDef::String("/path/to/source".to_string()))
          );
          assert_eq!(
            map.get("dest"),
            Some(&BindInputsDef::String("/path/to/dest".to_string()))
          );
        }
        _ => panic!("expected Table inputs"),
      }

      Ok(())
    }

    #[test]
    fn bind_with_dynamic_inputs() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      lua
        .load(
          r#"
                return sys.bind({
                    id = "bind-with-dynamic-inputs",
                    inputs = function()
                        return { computed = "dynamic-value" }
                    end,
                    create = function(inputs, ctx)
                        ctx:exec("echo " .. inputs.computed)
                        return { result = inputs.computed }
                    end,
                    destroy = function(outputs, ctx)
                        ctx:exec("echo cleanup " .. outputs.result)
                    end,
                })
            "#,
        )
        .eval::<LuaTable>()?;

      let manifest = manifest.borrow();
      let (_, bind_def) = manifest.bindings.iter().next().unwrap();
      let inputs = bind_def.inputs.as_ref().expect("should have inputs");
      match inputs {
        BindInputsDef::Table(map) => {
          assert_eq!(
            map.get("computed"),
            Some(&BindInputsDef::String("dynamic-value".to_string()))
          );
        }
        _ => panic!("expected Table inputs"),
      }

      Ok(())
    }

    #[test]
    fn bind_without_create_fails() -> LuaResult<()> {
      let (lua, _) = create_test_lua_with_manifest()?;

      let result = lua
        .load(
          r#"
                return sys.bind({
                    id = "bind-without-create",
                    destroy = function(inputs, ctx)
                        ctx:exec("rm /dest")
                    end,
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
    fn multiple_binds_added_to_manifest() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      lua
        .load(
          r#"
                sys.bind({
                    id = "bind-a",
                    create = function(i, c) c:exec("a") end,
                    destroy = function(i, c) c:exec("rm a") end
                })
                sys.bind({
                    id = "bind-b",
                    create = function(i, c) c:exec("b") end,
                    destroy = function(i, c) c:exec("rm b") end
                })
                sys.bind({
                    id = "bind-c",
                    create = function(i, c) c:exec("c") end,
                    destroy = function(i, c) c:exec("rm c") end
                })
            "#,
        )
        .exec()?;

      let manifest = manifest.borrow();
      assert_eq!(manifest.bindings.len(), 3);

      Ok(())
    }

    #[test]
    fn bind_hash_is_deterministic() -> LuaResult<()> {
      let (lua1, _) = create_test_lua_with_manifest()?;
      let (lua2, _) = create_test_lua_with_manifest()?;

      let code = r#"
                return sys.bind({
                    id = "my-bind",
                    inputs = { key = "value" },
                    create = function(inputs, ctx)
                        ctx:exec("ln -sf /src /dest")
                    end,
                    destroy = function(inputs, ctx)
                        ctx:exec("rm /dest")
                    end,
                })
            "#;

      let ref1: LuaTable = lua1.load(code).eval()?;
      let ref2: LuaTable = lua2.load(code).eval()?;

      let hash1: String = ref1.get("hash")?;
      let hash2: String = ref2.get("hash")?;

      assert_eq!(hash1, hash2, "same bind should produce same hash");

      Ok(())
    }

    #[test]
    fn bind_hash_changes_with_update() -> LuaResult<()> {
      let (lua1, _) = create_test_lua_with_manifest()?;
      let (lua2, _) = create_test_lua_with_manifest()?;

      let code_without_update = r#"
                return sys.bind({
                    id = "my-bind",
                    create = function(inputs, ctx)
                        ctx:exec("ln -sf /src /dest")
                    end,
                    destroy = function(inputs, ctx)
                        ctx:exec("rm /dest")
                    end,
                })
            "#;

      let code_with_update = r#"
                return sys.bind({
                    id = "my-bind",
                    create = function(inputs, ctx)
                        ctx:exec("ln -sf /src /dest")
                    end,
                    update = function(outputs, inputs, ctx)
                        ctx:exec("echo updating...")
                    end,
                    destroy = function(inputs, ctx)
                        ctx:exec("rm /dest")
                    end,
                })
            "#;

      let ref1: LuaTable = lua1.load(code_without_update).eval()?;
      let ref2: LuaTable = lua2.load(code_with_update).eval()?;

      let hash1: String = ref1.get("hash")?;
      let hash2: String = ref2.get("hash")?;

      assert_ne!(hash1, hash2, "adding destroy should change hash");

      Ok(())
    }

    #[test]
    fn duplicate_bind_is_deduplicated() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      // Create the same bind twice
      lua
        .load(
          r#"
                sys.bind({
                    id = "my-bind",
                    inputs = { src = "/src", dest = "/dest" },
                    create = function(inputs, ctx)
                        ctx:exec("ln -sf " .. inputs.src .. " " .. inputs.dest)
                        return { dest = inputs.dest }
                    end,
                    destroy = function(outputs, ctx)
                        ctx:exec("rm " .. outputs.dest)
                    end,
                })
                sys.bind({
                    id = "my-bind",
                    inputs = { src = "/src", dest = "/dest" },
                    create = function(inputs, ctx)
                        ctx:exec("ln -sf " .. inputs.src .. " " .. inputs.dest)
                        return { dest = inputs.dest }
                    end,
                    destroy = function(outputs, ctx)
                        ctx:exec("rm " .. outputs.dest)
                    end,
                })
            "#,
        )
        .exec()?;

      let manifest = manifest.borrow();
      // Should only have 1 bind, not 2
      assert_eq!(manifest.bindings.len(), 1);

      Ok(())
    }

    #[test]
    fn ctx_out_returns_placeholder() -> LuaResult<()> {
      let (lua, _) = create_test_lua_with_manifest()?;

      // Test that ctx.out returns the $${out} placeholder
      let result: LuaTable = lua
        .load(
          r#"
                return sys.bind({
                    id = "bind-with-out",
                    create = function(inputs, ctx)
                        -- ctx.out should return $${out} placeholder
                        ctx:exec("mkdir -p " .. ctx.out)
                        return { out = ctx.out }
                    end,
                    destroy = function(outputs, ctx)
                        ctx:exec("rm -rf " .. ctx.out)
                    end,
                })
            "#,
        )
        .eval()?;

      let outputs: LuaTable = result.get("outputs")?;
      let out: String = outputs.get("out")?;

      // The output value should contain the bind placeholder (resolved from ctx.out)
      // Since ctx.out returns "$${out}" and that's returned as the output value,
      // the final placeholder wraps it as $${bind:HASH:out}
      let hash: String = result.get("hash")?;
      assert_eq!(out, format!("$${{bind:{}:out}}", hash));

      Ok(())
    }

    #[test]
    fn ctx_out_can_be_used_in_commands() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      lua
        .load(
          r#"
                sys.bind({
                    id = "bind-with-ctx-out",
                    create = function(inputs, ctx)
                        ctx:exec("mkdir -p " .. ctx.out)
                        ctx:exec("ln -sf /src " .. ctx.out .. "/link")
                    end,
                    destroy = function(outputs, ctx)
                        ctx:exec("rm -rf " .. ctx.out)
                    end,
                })
            "#,
        )
        .exec()?;

      let manifest = manifest.borrow();
      let (_, bind_def) = manifest.bindings.iter().next().unwrap();

      // Check that the commands contain the $${out} placeholder
      assert_eq!(bind_def.create_actions.len(), 2);

      match &bind_def.create_actions[0] {
        Action::Exec(opts) => {
          assert!(
            opts.bin.contains("$${out}"),
            "cmd should contain ${{out}} placeholder: {}",
            opts.bin
          );
          assert_eq!(opts.bin, "mkdir -p $${out}");
        }
        _ => {
          panic!("expected Cmd action");
        }
      }

      match &bind_def.create_actions[1] {
        Action::Exec(opts) => {
          assert!(
            opts.bin.contains("$${out}"),
            "cmd should contain ${{out}} placeholder: {}",
            opts.bin
          );
          assert_eq!(opts.bin, "ln -sf /src $${out}/link");
        }
        _ => {
          panic!("expected Cmd action");
        }
      }

      Ok(())
    }

    #[test]
    fn ctx_out_can_be_used_in_destroy() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      lua
        .load(
          r#"
                sys.bind({
                    id = "bind-with-ctx-out-destroy",
                    create = function(inputs, ctx)
                        ctx:exec("mkdir -p " .. ctx.out)
                    end,
                    destroy = function(inputs, ctx)
                        ctx:exec("rm -rf " .. ctx.out)
                    end,
                })
            "#,
        )
        .exec()?;

      let manifest = manifest.borrow();
      let (_, bind_def) = manifest.bindings.iter().next().unwrap();

      // Check that destroy commands also contain the $${out} placeholder
      assert_eq!(bind_def.destroy_actions.len(), 1);

      match &bind_def.destroy_actions[0] {
        Action::Exec(opts) => {
          assert!(
            opts.bin.contains("$${out}"),
            "destroy cmd should contain ${{out}} placeholder: {}",
            opts.bin
          );
          assert_eq!(opts.bin, "rm -rf $${out}");
        }
        _ => {
          panic!("expected Cmd action");
        }
      }

      Ok(())
    }

    #[test]
    fn duplicate_bind_id_with_different_content_fails() -> LuaResult<()> {
      let (lua, _) = create_test_lua_with_manifest()?;

      let result = lua
        .load(
          r#"
                -- First bind with id "my-bind"
                sys.bind({
                    id = "my-bind",
                    create = function(inputs, ctx)
                        ctx:exec("echo first")
                    end,
                    destroy = function(outputs, ctx)
                        ctx:exec("echo destroy first")
                    end,
                })
                -- Second bind with same id but different content
                sys.bind({
                    id = "my-bind",
                    create = function(inputs, ctx)
                        ctx:exec("echo second")
                    end,
                    destroy = function(outputs, ctx)
                        ctx:exec("echo destroy second")
                    end,
                })
            "#,
        )
        .exec();

      assert!(result.is_err());
      let err = result.unwrap_err().to_string();
      assert!(
        err.contains("duplicate bind id"),
        "error should mention 'duplicate bind id': {}",
        err
      );
      assert!(err.contains("my-bind"), "error should mention the id: {}", err);

      Ok(())
    }

    #[test]
    fn binds_without_id_can_have_different_content() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      // Two binds without IDs with different content should both be added
      lua
        .load(
          r#"
                sys.bind({
                    create = function(inputs, ctx)
                        ctx:exec("echo first")
                    end,
                    destroy = function(outputs, ctx)
                        ctx:exec("echo destroy first")
                    end,
                })
                sys.bind({
                    create = function(inputs, ctx)
                        ctx:exec("echo second")
                    end,
                    destroy = function(outputs, ctx)
                        ctx:exec("echo destroy second")
                    end,
                })
            "#,
        )
        .exec()?;

      let manifest = manifest.borrow();
      // Both binds should be added since they have no ID (no conflict possible)
      assert_eq!(manifest.bindings.len(), 2);

      Ok(())
    }

    #[test]
    fn update_with_different_output_keys_fails() -> LuaResult<()> {
      let (lua, _) = create_test_lua_with_manifest()?;

      let result = lua
        .load(
          r#"
                return sys.bind({
                    id = "my-bind",
                    create = function(inputs, ctx)
                        ctx:exec("echo create")
                        return { path = "/some/path" }
                    end,
                    update = function(outputs, inputs, ctx)
                        ctx:exec("echo update")
                        return { different_key = "/other/path" }
                    end,
                    destroy = function(outputs, ctx)
                        ctx:exec("echo destroy")
                    end,
                })
            "#,
        )
        .eval::<LuaTable>();

      assert!(result.is_err());
      let err = result.unwrap_err().to_string();
      assert!(
        err.contains("update must return same output keys as create"),
        "error should mention output key mismatch: {}",
        err
      );

      Ok(())
    }

    #[test]
    fn update_with_same_output_keys_succeeds() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      lua
        .load(
          r#"
                return sys.bind({
                    id = "my-bind",
                    create = function(inputs, ctx)
                        ctx:exec("echo create")
                        return { path = "/some/path" }
                    end,
                    update = function(outputs, inputs, ctx)
                        ctx:exec("echo update")
                        return { path = "/updated/path" }
                    end,
                    destroy = function(outputs, ctx)
                        ctx:exec("echo destroy")
                    end,
                })
            "#,
        )
        .eval::<LuaTable>()?;

      let manifest = manifest.borrow();
      assert_eq!(manifest.bindings.len(), 1);

      Ok(())
    }

    #[test]
    fn update_with_nil_return_when_create_has_no_outputs() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      lua
        .load(
          r#"
                return sys.bind({
                    id = "my-bind",
                    create = function(inputs, ctx)
                        ctx:exec("echo create")
                        -- no return (nil)
                    end,
                    update = function(outputs, inputs, ctx)
                        ctx:exec("echo update")
                        -- no return (nil)
                    end,
                    destroy = function(outputs, ctx)
                        ctx:exec("echo destroy")
                    end,
                })
            "#,
        )
        .eval::<LuaTable>()?;

      let manifest = manifest.borrow();
      assert_eq!(manifest.bindings.len(), 1);

      Ok(())
    }

    #[test]
    fn update_returns_outputs_but_create_does_not_fails() -> LuaResult<()> {
      let (lua, _) = create_test_lua_with_manifest()?;

      let result = lua
        .load(
          r#"
                return sys.bind({
                    id = "my-bind",
                    create = function(inputs, ctx)
                        ctx:exec("echo create")
                        -- no return
                    end,
                    update = function(outputs, inputs, ctx)
                        ctx:exec("echo update")
                        return { path = "/some/path" }
                    end,
                    destroy = function(outputs, ctx)
                        ctx:exec("echo destroy")
                    end,
                })
            "#,
        )
        .eval::<LuaTable>();

      assert!(result.is_err());
      let err = result.unwrap_err().to_string();
      assert!(
        err.contains("update returned outputs but create did not"),
        "error should mention create has no outputs: {}",
        err
      );

      Ok(())
    }

    #[test]
    fn update_returns_nil_but_create_has_outputs_fails() -> LuaResult<()> {
      let (lua, _) = create_test_lua_with_manifest()?;

      let result = lua
        .load(
          r#"
                return sys.bind({
                    id = "my-bind",
                    create = function(inputs, ctx)
                        ctx:exec("echo create")
                        return { path = "/some/path" }
                    end,
                    update = function(outputs, inputs, ctx)
                        ctx:exec("echo update")
                        -- no return (nil)
                    end,
                    destroy = function(outputs, ctx)
                        ctx:exec("echo destroy")
                    end,
                })
            "#,
        )
        .eval::<LuaTable>();

      assert!(result.is_err());
      let err = result.unwrap_err().to_string();
      assert!(
        err.contains("update returned nil but create returned outputs"),
        "error should mention create has outputs: {}",
        err
      );

      Ok(())
    }

    #[test]
    fn bind_ctx_does_not_have_fetch_url() -> LuaResult<()> {
      let (lua, _) = create_test_lua_with_manifest()?;

      let result = lua
        .load(
          r#"
                return sys.bind({
                    id = "fetch-url-test",
                    create = function(inputs, ctx)
                        ctx:fetch_url("http://example.com", "abc123")
                    end,
                    destroy = function(outputs, ctx)
                    end,
                })
            "#,
        )
        .eval::<LuaTable>();

      assert!(result.is_err());
      let err = result.unwrap_err().to_string();
      assert!(
        err.contains("unknown bind ctx method 'fetch_url'"),
        "error should mention fetch_url is not available: {}",
        err
      );

      Ok(())
    }
  }
}
