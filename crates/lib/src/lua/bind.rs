//! Lua bindings for `sys.bind{}`.
//!
//! This module provides:
//! - `BindCtx` as LuaUserData with methods like `cmd`
//! - `register_sys_bind()` to register the `sys.bind` function

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use mlua::prelude::*;

use crate::bind::{BIND_REF_TYPE, BindCmdOptions, BindCtx, BindDef};
use crate::consts::HASH_PREFIX_LEN;
use crate::inputs::InputsRef;
use crate::lua::inputs::{inputs_ref_to_lua, lua_value_to_inputs_ref};
use crate::lua::outputs::parse_outputs;
use crate::manifest::Manifest;

impl LuaUserData for BindCtx {
  fn add_fields<F: LuaUserDataFields<Self>>(fields: &mut F) {
    fields.add_field_method_get("out", |_, this| Ok(this.out().to_string()));
  }

  fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
    methods.add_method_mut("cmd", |_, this, opts: LuaValue| {
      let cmd_opts = parse_cmd_opts(opts)?;
      Ok(this.cmd(cmd_opts))
    });
  }
}

fn parse_cmd_opts(opts: LuaValue) -> LuaResult<BindCmdOptions> {
  match opts {
    LuaValue::String(s) => {
      let cmd = s.to_str()?.to_string();
      Ok(BindCmdOptions::new(&cmd))
    }
    LuaValue::Table(table) => {
      let cmd: String = table.get("cmd")?;
      let cwd: Option<String> = table.get("cwd")?;
      let env: Option<LuaTable> = table.get("env")?;

      let mut opts = BindCmdOptions::new(&cmd);
      if let Some(cwd) = cwd {
        opts = opts.with_cwd(&cwd);
      }

      if let Some(env_table) = env {
        let mut env_map = BTreeMap::new();
        for pair in env_table.pairs::<String, String>() {
          let (key, value) = pair?;
          env_map.insert(key, value);
        }
        opts = opts.with_env(env_map);
      }
      Ok(opts)
    }
    _ => Err(LuaError::external("cmd() expects a string or table with 'cmd' field")),
  }
}

/// Register the `sys.bind` function on the sys table.
///
/// The `sys.bind{}` function:
/// 1. Parses a BindSpec from the Lua table (inputs, apply, destroy)
/// 2. Resolves inputs (calls function if dynamic, uses table directly if static)
/// 3. Creates a BindCtx and calls the apply function
/// 4. Optionally calls the destroy function with a fresh BindCtx
/// 5. Creates a BindDef, computes its hash, and adds it to the manifest
/// 6. Returns a BindRef as a Lua table with metatable marker
pub fn register_sys_bind(lua: &Lua, sys_table: &LuaTable, manifest: Rc<RefCell<Manifest>>) -> LuaResult<()> {
  let bind_fn = lua.create_function(move |lua, spec_table: LuaTable| {
    // 1. Parse the BindSpec from the Lua table
    let apply_fn: LuaFunction = spec_table
      .get("apply")
      .map_err(|_| LuaError::external("bind spec requires 'apply' function"))?;

    let destroy_fn: Option<LuaFunction> = spec_table.get("destroy")?;

    // 2. Resolve inputs (if provided)
    let inputs_value: Option<LuaValue> = spec_table.get("inputs")?;
    let resolved_inputs: Option<InputsRef> = match inputs_value {
      Some(LuaValue::Function(f)) => {
        // Dynamic inputs - call the function to get resolved value
        let result: LuaValue = f.call(())?;
        if result == LuaValue::Nil {
          None
        } else {
          Some(lua_value_to_inputs_ref(result, &manifest.borrow())?)
        }
      }
      Some(LuaValue::Nil) => None,
      Some(v) => Some(lua_value_to_inputs_ref(v, &manifest.borrow())?),
      None => None,
    };

    // 3. Create BindCtx and call the apply function
    let apply_ctx = BindCtx::new();
    let apply_ctx_userdata = lua.create_userdata(apply_ctx)?;

    // Prepare inputs argument for apply function
    let inputs_arg: LuaValue = match &resolved_inputs {
      Some(inputs) => inputs_ref_to_lua(lua, inputs, &manifest.borrow())?,
      None => LuaValue::Table(lua.create_table()?), // Empty table if no inputs
    };

    // Call: apply(inputs, ctx) -> outputs (optional)
    let apply_result: LuaValue = apply_fn.call((inputs_arg, &apply_ctx_userdata))?;

    // 4. Extract outputs from apply return value (optional for binds)
    let outputs: Option<BTreeMap<String, String>> = match apply_result {
      LuaValue::Table(t) => {
        let parsed = parse_outputs(t)?;
        if parsed.is_empty() { None } else { Some(parsed) }
      }
      LuaValue::Nil => None,
      _ => {
        return Err(LuaError::external("bind apply must return a table of outputs or nil"));
      }
    };

    // 5. Extract apply actions from BindCtx
    let apply_ctx: BindCtx = apply_ctx_userdata.take()?;
    let apply_actions = apply_ctx.into_actions();

    // 6. Optionally call destroy function
    let destroy_actions = if let Some(destroy_fn) = destroy_fn {
      let destroy_ctx = BindCtx::new();
      let destroy_ctx_userdata = lua.create_userdata(destroy_ctx)?;

      // Reconstruct inputs argument for destroy function
      let destroy_inputs_arg: LuaValue = match &resolved_inputs {
        Some(inputs) => inputs_ref_to_lua(lua, inputs, &manifest.borrow())?,
        None => LuaValue::Table(lua.create_table()?),
      };

      // Call: destroy(inputs, ctx) -> ignored
      let _: LuaValue = destroy_fn.call((destroy_inputs_arg, &destroy_ctx_userdata))?;

      let destroy_ctx: BindCtx = destroy_ctx_userdata.take()?;
      let actions = destroy_ctx.into_actions();
      if actions.is_empty() { None } else { Some(actions) }
    } else {
      None
    };

    // 7. Create BindDef
    let bind_def = BindDef {
      inputs: resolved_inputs.clone(),
      apply_actions,
      outputs: outputs.clone(),
      destroy_actions,
    };

    // 8. Compute hash
    let hash = bind_def
      .compute_hash()
      .map_err(|e| LuaError::external(format!("failed to compute bind hash: {}", e)))?;

    // 9. Add to manifest (deduplicate by hash)
    {
      let mut manifest = manifest.borrow_mut();
      if manifest.bindings.contains_key(&hash) {
        tracing::warn!(
          hash = %hash.0,
          "duplicate bind detected, skipping insertion"
        );
      } else {
        manifest.bindings.insert(hash.clone(), bind_def);
      }
    }

    // 10. Create and return BindRef as Lua table
    let ref_table = lua.create_table()?;
    ref_table.set("hash", hash.0.as_str())?;

    // Add inputs to ref (nil if not specified)
    if let Some(inputs) = &resolved_inputs {
      ref_table.set("inputs", inputs_ref_to_lua(lua, inputs, &manifest.borrow())?)?;
    }

    // Convert outputs to Lua table with placeholders for runtime resolution (if present)
    if let Some(ref outs) = outputs {
      let outputs_table = lua.create_table()?;
      let short_hash = &hash.0[..HASH_PREFIX_LEN.min(hash.0.len())];
      for k in outs.keys() {
        let placeholder = format!("$${{bind:{}:{}}}", short_hash, k);
        outputs_table.set(k.as_str(), placeholder.as_str())?;
      }
      ref_table.set("outputs", outputs_table)?;
    }

    // Set metatable with __type marker
    let mt = lua.create_table()?;
    mt.set("__type", BIND_REF_TYPE)?;
    ref_table.set_metatable(Some(mt))?;

    Ok(ref_table)
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
    use super::*;

    #[test]
    fn simple_bind_returns_bind_ref() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      let result: LuaTable = lua
        .load(
          r#"
                return sys.bind({
                    apply = function(inputs, ctx)
                        ctx:cmd("ln -sf /src /dest")
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
      assert_eq!(bind_def.apply_actions.len(), 1);

      Ok(())
    }

    #[test]
    fn bind_with_outputs() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      let result: LuaTable = lua
        .load(
          r#"
                return sys.bind({
                    apply = function(inputs, ctx)
                        ctx:cmd("ln -sf /src /dest")
                        return { link = "/dest" }
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
                    apply = function(inputs, ctx)
                        ctx:cmd("ln -sf /src /dest")
                    end,
                    destroy = function(inputs, ctx)
                        ctx:cmd("rm /dest")
                    end,
                })
            "#,
        )
        .eval::<LuaTable>()?;

      let manifest = manifest.borrow();
      assert_eq!(manifest.bindings.len(), 1);
      let (_, bind_def) = manifest.bindings.iter().next().unwrap();
      assert!(bind_def.destroy_actions.is_some());
      assert_eq!(bind_def.destroy_actions.as_ref().unwrap().len(), 1);

      Ok(())
    }

    #[test]
    fn bind_with_inputs_from_build() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      lua
        .load(
          r#"
                local pkg = sys.build({
                    name = "my-pkg",
                    apply = function(inputs, ctx)
                        ctx:cmd("make install")
                        return { out = "/store/my-pkg" }
                    end,
                })

                return sys.bind({
                    inputs = { pkg = pkg },
                    apply = function(inputs, ctx)
                        ctx:cmd("ln -sf " .. inputs.pkg.outputs.out .. "/bin/app /usr/local/bin/app")
                    end,
                    destroy = function(inputs, ctx)
                        ctx:cmd("rm /usr/local/bin/app")
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
        InputsRef::Table(map) => {
          let pkg = map.get("pkg").expect("should have pkg key");
          match pkg {
            InputsRef::Build(build_hash) => {
              // Verify it's a truncated hash (HASH_PREFIX_LEN hex chars)
              assert_eq!(build_hash.0.len(), HASH_PREFIX_LEN);
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
                    inputs = { src = "/path/to/source", dest = "/path/to/dest" },
                    apply = function(inputs, ctx)
                        ctx:cmd("ln -sf " .. inputs.src .. " " .. inputs.dest)
                    end,
                })
            "#,
        )
        .eval::<LuaTable>()?;

      let manifest = manifest.borrow();
      let (_, bind_def) = manifest.bindings.iter().next().unwrap();
      let inputs = bind_def.inputs.as_ref().expect("should have inputs");
      match inputs {
        InputsRef::Table(map) => {
          assert_eq!(map.get("src"), Some(&InputsRef::String("/path/to/source".to_string())));
          assert_eq!(map.get("dest"), Some(&InputsRef::String("/path/to/dest".to_string())));
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
                    inputs = function()
                        return { computed = "dynamic-value" }
                    end,
                    apply = function(inputs, ctx)
                        ctx:cmd("echo " .. inputs.computed)
                    end,
                })
            "#,
        )
        .eval::<LuaTable>()?;

      let manifest = manifest.borrow();
      let (_, bind_def) = manifest.bindings.iter().next().unwrap();
      let inputs = bind_def.inputs.as_ref().expect("should have inputs");
      match inputs {
        InputsRef::Table(map) => {
          assert_eq!(
            map.get("computed"),
            Some(&InputsRef::String("dynamic-value".to_string()))
          );
        }
        _ => panic!("expected Table inputs"),
      }

      Ok(())
    }

    #[test]
    fn bind_without_apply_fails() -> LuaResult<()> {
      let (lua, _) = create_test_lua_with_manifest()?;

      let result = lua
        .load(
          r#"
                return sys.bind({
                    destroy = function(inputs, ctx)
                        ctx:cmd("rm /dest")
                    end,
                })
            "#,
        )
        .eval::<LuaTable>();

      assert!(result.is_err());
      let err = result.unwrap_err().to_string();
      assert!(err.contains("apply"), "error should mention 'apply': {}", err);

      Ok(())
    }

    #[test]
    fn multiple_binds_added_to_manifest() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      lua
        .load(
          r#"
                sys.bind({ apply = function(i, c) c:cmd("a") end })
                sys.bind({ apply = function(i, c) c:cmd("b") end })
                sys.bind({ apply = function(i, c) c:cmd("c") end })
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
                    inputs = { key = "value" },
                    apply = function(inputs, ctx)
                        ctx:cmd("ln -sf /src /dest")
                    end,
                    destroy = function(inputs, ctx)
                        ctx:cmd("rm /dest")
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
    fn bind_hash_changes_with_destroy() -> LuaResult<()> {
      let (lua1, _) = create_test_lua_with_manifest()?;
      let (lua2, _) = create_test_lua_with_manifest()?;

      let code_without_destroy = r#"
                return sys.bind({
                    apply = function(inputs, ctx)
                        ctx:cmd("ln -sf /src /dest")
                    end,
                })
            "#;

      let code_with_destroy = r#"
                return sys.bind({
                    apply = function(inputs, ctx)
                        ctx:cmd("ln -sf /src /dest")
                    end,
                    destroy = function(inputs, ctx)
                        ctx:cmd("rm /dest")
                    end,
                })
            "#;

      let ref1: LuaTable = lua1.load(code_without_destroy).eval()?;
      let ref2: LuaTable = lua2.load(code_with_destroy).eval()?;

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
                    inputs = { src = "/src", dest = "/dest" },
                    apply = function(inputs, ctx)
                        ctx:cmd("ln -sf " .. inputs.src .. " " .. inputs.dest)
                    end,
                })
                sys.bind({
                    inputs = { src = "/src", dest = "/dest" },
                    apply = function(inputs, ctx)
                        ctx:cmd("ln -sf " .. inputs.src .. " " .. inputs.dest)
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
    fn bind_ref_includes_inputs() -> LuaResult<()> {
      let (lua, _) = create_test_lua_with_manifest()?;

      let result: LuaTable = lua
        .load(
          r#"
                return sys.bind({
                    inputs = { src = "/path/to/source", dest = "/path/to/dest" },
                    apply = function(inputs, ctx)
                        ctx:cmd("ln -sf " .. inputs.src .. " " .. inputs.dest)
                    end,
                })
            "#,
        )
        .eval()?;

      // Check that inputs are available on the BindRef
      let inputs: LuaTable = result.get("inputs")?;
      let src: String = inputs.get("src")?;
      let dest: String = inputs.get("dest")?;

      assert_eq!(src, "/path/to/source");
      assert_eq!(dest, "/path/to/dest");

      Ok(())
    }

    #[test]
    fn bind_ref_inputs_is_nil_when_not_specified() -> LuaResult<()> {
      let (lua, _) = create_test_lua_with_manifest()?;

      let result: LuaTable = lua
        .load(
          r#"
                return sys.bind({
                    apply = function(inputs, ctx)
                        ctx:cmd("ln -sf /src /dest")
                    end,
                })
            "#,
        )
        .eval()?;

      // inputs should be nil, not an empty table
      let inputs: LuaValue = result.get("inputs")?;
      assert_eq!(inputs, LuaValue::Nil);

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
                    apply = function(inputs, ctx)
                        -- ctx.out should return $${out} placeholder
                        ctx:cmd("mkdir -p " .. ctx.out)
                        return { out = ctx.out }
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
                    apply = function(inputs, ctx)
                        ctx:cmd("mkdir -p " .. ctx.out)
                        ctx:cmd("ln -sf /src " .. ctx.out .. "/link")
                    end,
                })
            "#,
        )
        .exec()?;

      let manifest = manifest.borrow();
      let (_, bind_def) = manifest.bindings.iter().next().unwrap();

      // Check that the commands contain the $${out} placeholder
      assert_eq!(bind_def.apply_actions.len(), 2);

      use crate::bind::BindAction;
      match &bind_def.apply_actions[0] {
        BindAction::Cmd { cmd, .. } => {
          assert!(
            cmd.contains("$${out}"),
            "cmd should contain ${{out}} placeholder: {}",
            cmd
          );
          assert_eq!(cmd, "mkdir -p $${out}");
        }
      }

      match &bind_def.apply_actions[1] {
        BindAction::Cmd { cmd, .. } => {
          assert!(
            cmd.contains("$${out}"),
            "cmd should contain ${{out}} placeholder: {}",
            cmd
          );
          assert_eq!(cmd, "ln -sf /src $${out}/link");
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
                    apply = function(inputs, ctx)
                        ctx:cmd("mkdir -p " .. ctx.out)
                    end,
                    destroy = function(inputs, ctx)
                        ctx:cmd("rm -rf " .. ctx.out)
                    end,
                })
            "#,
        )
        .exec()?;

      let manifest = manifest.borrow();
      let (_, bind_def) = manifest.bindings.iter().next().unwrap();

      // Check that destroy commands also contain the $${out} placeholder
      let destroy_actions = bind_def.destroy_actions.as_ref().expect("should have destroy actions");
      assert_eq!(destroy_actions.len(), 1);

      use crate::bind::BindAction;
      match &destroy_actions[0] {
        BindAction::Cmd { cmd, .. } => {
          assert!(
            cmd.contains("$${out}"),
            "destroy cmd should contain ${{out}} placeholder: {}",
            cmd
          );
          assert_eq!(cmd, "rm -rf $${out}");
        }
      }

      Ok(())
    }
  }
}
