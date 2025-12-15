//! Lua bindings for `sys.build{}`.
//!
//! This module provides:
//! - `BuildCtx` as LuaUserData with methods like `fetch_url` and `cmd`
//! - `register_sys_build()` to register the `sys.build` function
//! - Helper functions for converting between Lua values and Rust types

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use mlua::prelude::*;

use crate::build::{BUILD_REF_TYPE, BuildCmdOptions, BuildCtx, BuildDef};
use crate::consts::HASH_PREFIX_LEN;
use crate::inputs::InputsRef;
use crate::lua::inputs::{contains_bind_ref, inputs_ref_to_lua, lua_value_to_inputs_ref};
use crate::lua::outputs::parse_outputs;
use crate::manifest::Manifest;

impl LuaUserData for BuildCtx {
  fn add_fields<F: LuaUserDataFields<Self>>(fields: &mut F) {
    fields.add_field_method_get("out", |_, this| Ok(this.out().to_string()));
  }

  fn add_methods<M: LuaUserDataMethods<Self>>(methods: &mut M) {
    methods.add_method_mut("fetch_url", |_, this, (url, sha256): (String, String)| {
      Ok(this.fetch_url(&url, &sha256))
    });

    methods.add_method_mut("cmd", |_, this, opts: LuaValue| {
      let cmd_opts = parse_cmd_opts(opts)?;
      Ok(this.cmd(cmd_opts))
    });
  }
}

fn parse_cmd_opts(opts: LuaValue) -> LuaResult<BuildCmdOptions> {
  match opts {
    LuaValue::String(s) => {
      let cmd = s.to_str()?.to_string();
      Ok(BuildCmdOptions::new(&cmd))
    }
    LuaValue::Table(table) => {
      let cmd: String = table.get("cmd")?;
      let cwd: Option<String> = table.get("cwd")?;
      let env: Option<LuaTable> = table.get("env")?;

      let mut opts = BuildCmdOptions::new(&cmd);
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

/// Register the `sys.build` function on the sys table.
///
/// The `sys.build{}` function:
/// 1. Parses a BuildSpec from the Lua table (name, version, inputs, apply)
/// 2. Resolves inputs (calls function if dynamic, uses table directly if static)
/// 3. Creates a BuildCtx and calls the apply function
/// 4. Captures the returned outputs (must be non-empty)
/// 5. Creates a BuildDef, computes its hash, and adds it to the manifest
/// 6. Returns a BuildRef as a Lua table with metatable marker
pub fn register_sys_build(lua: &Lua, sys_table: &LuaTable, manifest: Rc<RefCell<Manifest>>) -> LuaResult<()> {
  let build_fn = lua.create_function(move |lua, spec_table: LuaTable| {
    // 1. Parse the BuildSpec from the Lua table
    let name: String = spec_table
      .get("name")
      .map_err(|_| LuaError::external("build spec requires 'name' field"))?;

    let version: Option<String> = spec_table.get("version")?;
    let apply_fn: LuaFunction = spec_table
      .get("apply")
      .map_err(|_| LuaError::external("build spec requires 'apply' function"))?;

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

    // 2.5 Validate no bind references in inputs (builds cannot depend on binds)
    if let Some(ref inputs) = resolved_inputs
      && contains_bind_ref(inputs)
    {
      return Err(LuaError::external(
        "build inputs cannot reference binds: binds are side-effectful and cannot be inputs to immutable builds",
      ));
    }

    // 3. Create BuildCtx and call the apply function
    let ctx = BuildCtx::new();
    let ctx_userdata = lua.create_userdata(ctx)?;

    // Prepare inputs argument for apply function
    let inputs_arg: LuaValue = match &resolved_inputs {
      Some(inputs) => inputs_ref_to_lua(lua, inputs, &manifest.borrow())?,
      None => LuaValue::Table(lua.create_table()?), // Empty table if no inputs
    };

    // Call: apply(inputs, ctx) -> outputs
    let result: LuaValue = apply_fn.call((inputs_arg, &ctx_userdata))?;

    // 4. Extract outputs from return value (must be non-empty table)
    let outputs: BTreeMap<String, String> = match result {
      LuaValue::Table(t) => {
        let parsed = parse_outputs(t)?;
        if parsed.is_empty() {
          return Err(LuaError::external("build apply must return a non-empty outputs table"));
        }
        parsed
      }
      LuaValue::Nil => {
        return Err(LuaError::external(
          "build apply must return a non-empty outputs table, got nil",
        ));
      }
      _ => {
        return Err(LuaError::external("build apply must return a table of outputs"));
      }
    };

    // 5. Extract actions from BuildCtx
    let ctx: BuildCtx = ctx_userdata.take()?;
    let apply_actions = ctx.into_actions();

    // 6. Create BuildDef
    let build_def = BuildDef {
      name: name.clone(),
      version: version.clone(),
      inputs: resolved_inputs.clone(),
      apply_actions,
      outputs: Some(outputs.clone()),
    };

    // 7. Compute hash
    let hash = build_def
      .compute_hash()
      .map_err(|e| LuaError::external(format!("failed to compute build hash: {}", e)))?;

    // 8. Add to manifest (deduplicate by hash)
    {
      let mut manifest = manifest.borrow_mut();
      if manifest.builds.contains_key(&hash) {
        tracing::warn!(
          hash = %hash.0,
          name = %name,
          "duplicate build detected, skipping insertion"
        );
      } else {
        manifest.builds.insert(hash.clone(), build_def);
      }
    }

    // 9. Create and return BuildRef as Lua table
    let ref_table = lua.create_table()?;
    ref_table.set("name", name.as_str())?;
    if let Some(v) = &version {
      ref_table.set("version", v.as_str())?;
    }
    ref_table.set("hash", hash.0.as_str())?;

    // Add inputs to ref (nil if not specified)
    if let Some(inputs) = &resolved_inputs {
      ref_table.set("inputs", inputs_ref_to_lua(lua, inputs, &manifest.borrow())?)?;
    }

    // Convert outputs to Lua table with placeholders for runtime resolution
    let outputs_table = lua.create_table()?;
    let short_hash = &hash.0[..HASH_PREFIX_LEN.min(hash.0.len())];
    for k in outputs.keys() {
      let placeholder = format!("$${{build:{}:{}}}", short_hash, k);
      outputs_table.set(k.as_str(), placeholder.as_str())?;
    }
    ref_table.set("outputs", outputs_table)?;

    // Set metatable with __type marker
    let mt = lua.create_table()?;
    mt.set("__type", BUILD_REF_TYPE)?;
    ref_table.set_metatable(Some(mt))?;

    Ok(ref_table)
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
    use super::*;

    #[test]
    fn simple_build_returns_build_ref() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      let result: LuaTable = lua
        .load(
          r#"
                return sys.build({
                    name = "test-pkg",
                    version = "1.0.0",
                    apply = function(inputs, ctx)
                        ctx:cmd("make install")
                        return { out = "/path/to/output" }
                    end,
                })
            "#,
        )
        .eval()?;

      // Check returned BuildRef
      let name: String = result.get("name")?;
      assert_eq!(name, "test-pkg");

      let version: String = result.get("version")?;
      assert_eq!(version, "1.0.0");

      let hash: String = result.get("hash")?;
      assert!(!hash.is_empty(), "hash should not be empty");

      let outputs: LuaTable = result.get("outputs")?;
      let out: String = outputs.get("out")?;
      // Output should be a placeholder with the hash
      let hash: String = result.get("hash")?;
      assert_eq!(out, format!("$${{build:{}:out}}", hash));

      // Check metatable
      let mt = result.metatable().expect("should have metatable");
      let type_name: String = mt.get("__type")?;
      assert_eq!(type_name, "BuildRef");

      // Check manifest was updated
      let manifest = manifest.borrow();
      assert_eq!(manifest.builds.len(), 1);
      let (_, build_def) = manifest.builds.iter().next().unwrap();
      assert_eq!(build_def.name, "test-pkg");

      Ok(())
    }

    #[test]
    fn build_with_static_inputs() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      lua
        .load(
          r#"
                return sys.build({
                    name = "with-inputs",
                    inputs = { url = "https://example.com/src.tar.gz", sha256 = "abc123" },
                    apply = function(inputs, ctx)
                        local archive = ctx:fetch_url(inputs.url, inputs.sha256)
                        ctx:cmd("tar -xzf " .. archive)
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
        InputsRef::Table(map) => {
          assert!(map.contains_key("url"));
          assert!(map.contains_key("sha256"));
        }
        _ => panic!("expected Table inputs"),
      }

      // Check actions were recorded
      assert_eq!(build_def.apply_actions.len(), 2); // fetch_url + cmd

      Ok(())
    }

    #[test]
    fn build_with_dynamic_inputs() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      lua
        .load(
          r#"
                return sys.build({
                    name = "dynamic-inputs",
                    inputs = function()
                        return { computed = "value" }
                    end,
                    apply = function(inputs, ctx)
                        ctx:cmd("echo " .. inputs.computed)
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
        InputsRef::Table(map) => {
          assert_eq!(map.get("computed"), Some(&InputsRef::String("value".to_string())));
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
                    name = "dependency",
                    apply = function(inputs, ctx)
                        ctx:cmd("make dep")
                        return { out = "/dep/output" }
                    end,
                })

                return sys.build({
                    name = "consumer",
                    inputs = { dep = dep },
                    apply = function(inputs, ctx)
                        ctx:cmd("make -I " .. inputs.dep.outputs.out)
                        return { out = "/consumer/output" }
                    end,
                })
            "#,
        )
        .eval::<LuaTable>()?;

      let manifest = manifest.borrow();
      assert_eq!(manifest.builds.len(), 2);

      // Check the consumer's inputs contain the BuildHash
      // The consumer is the one with name = "consumer"
      let consumer = manifest.builds.values().find(|b| b.name == "consumer").unwrap();
      let inputs = consumer.inputs.as_ref().expect("should have inputs");
      match inputs {
        InputsRef::Table(map) => {
          let dep = map.get("dep").expect("should have dep key");
          match dep {
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
    fn build_without_name_fails() -> LuaResult<()> {
      let (lua, _) = create_test_lua_with_manifest()?;

      let result = lua
        .load(
          r#"
                return sys.build({
                    apply = function(inputs, ctx)
                        return { out = "/output" }
                    end,
                })
            "#,
        )
        .eval::<LuaTable>();

      assert!(result.is_err());
      let err = result.unwrap_err().to_string();
      assert!(err.contains("name"), "error should mention 'name': {}", err);

      Ok(())
    }

    #[test]
    fn build_without_apply_fails() -> LuaResult<()> {
      let (lua, _) = create_test_lua_with_manifest()?;

      let result = lua
        .load(
          r#"
                return sys.build({
                    name = "no-apply",
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
    fn build_with_empty_outputs_fails() -> LuaResult<()> {
      let (lua, _) = create_test_lua_with_manifest()?;

      let result = lua
        .load(
          r#"
                return sys.build({
                    name = "empty-outputs",
                    apply = function(inputs, ctx)
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
                    name = "nil-outputs",
                    apply = function(inputs, ctx)
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
                sys.build({ name = "pkg1", apply = function(i, c) c:cmd("a"); return { out = "x" } end })
                sys.build({ name = "pkg2", apply = function(i, c) c:cmd("b"); return { out = "y" } end })
                sys.build({ name = "pkg3", apply = function(i, c) c:cmd("c"); return { out = "z" } end })
            "#,
        )
        .exec()?;

      let manifest = manifest.borrow();
      assert_eq!(manifest.builds.len(), 3);

      // Check all names are present (order in BTreeMap is by hash, not insertion order)
      let names: Vec<_> = manifest.builds.values().map(|b| b.name.as_str()).collect();
      assert!(names.contains(&"pkg1"));
      assert!(names.contains(&"pkg2"));
      assert!(names.contains(&"pkg3"));

      Ok(())
    }

    #[test]
    fn build_hash_is_deterministic() -> LuaResult<()> {
      // Create two separate Lua runtimes with the same build
      let (lua1, _) = create_test_lua_with_manifest()?;
      let (lua2, _) = create_test_lua_with_manifest()?;

      let code = r#"
                return sys.build({
                    name = "deterministic",
                    version = "1.0.0",
                    inputs = { key = "value" },
                    apply = function(inputs, ctx)
                        ctx:cmd("make")
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

      // Create the same build twice
      lua
        .load(
          r#"
                sys.build({
                    name = "same-pkg",
                    version = "1.0.0",
                    apply = function(inputs, ctx)
                        ctx:cmd("make")
                        return { out = "/output" }
                    end,
                })
                sys.build({
                    name = "same-pkg",
                    version = "1.0.0",
                    apply = function(inputs, ctx)
                        ctx:cmd("make")
                        return { out = "/output" }
                    end,
                })
            "#,
        )
        .exec()?;

      let manifest = manifest.borrow();
      // Should only have 1 build, not 2
      assert_eq!(manifest.builds.len(), 1);

      Ok(())
    }

    #[test]
    fn build_ref_includes_inputs() -> LuaResult<()> {
      let (lua, _) = create_test_lua_with_manifest()?;

      let result: LuaTable = lua
        .load(
          r#"
                return sys.build({
                    name = "with-inputs",
                    inputs = { url = "https://example.com/src.tar.gz", sha256 = "abc123" },
                    apply = function(inputs, ctx)
                        ctx:fetch_url(inputs.url, inputs.sha256)
                        return { out = "/output" }
                    end,
                })
            "#,
        )
        .eval()?;

      // Check that inputs are available on the BuildRef
      let inputs: LuaTable = result.get("inputs")?;
      let url: String = inputs.get("url")?;
      let sha256: String = inputs.get("sha256")?;

      assert_eq!(url, "https://example.com/src.tar.gz");
      assert_eq!(sha256, "abc123");

      Ok(())
    }

    #[test]
    fn build_ref_inputs_is_nil_when_not_specified() -> LuaResult<()> {
      let (lua, _) = create_test_lua_with_manifest()?;

      let result: LuaTable = lua
        .load(
          r#"
                return sys.build({
                    name = "no-inputs",
                    apply = function(inputs, ctx)
                        ctx:cmd("make")
                        return { out = "/output" }
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
                return sys.build({
                    name = "test-out",
                    apply = function(inputs, ctx)
                        -- ctx.out should return $${out} placeholder
                        ctx:cmd("mkdir -p " .. ctx.out .. "/bin")
                        return { out = ctx.out }
                    end,
                })
            "#,
        )
        .eval()?;

      let outputs: LuaTable = result.get("outputs")?;
      let out: String = outputs.get("out")?;

      // The output value should contain the build placeholder (resolved from ctx.out)
      // Since ctx.out returns "$${out}" and that's returned as the output value,
      // the final placeholder wraps it as $${build:HASH:out}
      let hash: String = result.get("hash")?;
      assert_eq!(out, format!("$${{build:{}:out}}", hash));

      Ok(())
    }

    #[test]
    fn ctx_out_can_be_used_in_commands() -> LuaResult<()> {
      let (lua, manifest) = create_test_lua_with_manifest()?;

      lua
        .load(
          r#"
                sys.build({
                    name = "uses-out",
                    apply = function(inputs, ctx)
                        ctx:cmd("mkdir -p " .. ctx.out .. "/bin")
                        ctx:cmd("cp binary " .. ctx.out .. "/bin/")
                        return { out = ctx.out }
                    end,
                })
            "#,
        )
        .exec()?;

      let manifest = manifest.borrow();
      let (_, build_def) = manifest.builds.iter().next().unwrap();

      // Check that the commands contain the $${out} placeholder
      assert_eq!(build_def.apply_actions.len(), 2);

      use crate::build::BuildAction;
      match &build_def.apply_actions[0] {
        BuildAction::Cmd { cmd, .. } => {
          assert!(
            cmd.contains("$${out}"),
            "cmd should contain ${{out}} placeholder: {}",
            cmd
          );
          assert_eq!(cmd, "mkdir -p $${out}/bin");
        }
        _ => panic!("expected Cmd action"),
      }

      match &build_def.apply_actions[1] {
        BuildAction::Cmd { cmd, .. } => {
          assert!(
            cmd.contains("$${out}"),
            "cmd should contain ${{out}} placeholder: {}",
            cmd
          );
          assert_eq!(cmd, "cp binary $${out}/bin/");
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
                    apply = function(inputs, ctx)
                        ctx:cmd("ln -sf /src /dest")
                    end,
                })

                return sys.build({
                    name = "invalid-build",
                    inputs = { bind_dep = link },
                    apply = function(inputs, ctx)
                        ctx:cmd("make")
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
                    apply = function(inputs, ctx)
                        ctx:cmd("ln -sf /src /dest")
                    end,
                })

                return sys.build({
                    name = "invalid-build",
                    inputs = {
                        nested = {
                            deep = { bind_dep = link }
                        }
                    },
                    apply = function(inputs, ctx)
                        ctx:cmd("make")
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
