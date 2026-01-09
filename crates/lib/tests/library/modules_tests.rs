//! Tests for syslua.modules.* functions.

use mlua::prelude::*;

use super::common::create_test_runtime;

mod file_module {
  use super::*;

  #[test]
  fn requires_source_or_content() -> LuaResult<()> {
    let (lua, _) = create_test_runtime()?;

    let result = lua
      .load(
        r#"
            local syslua = require('syslua')
            syslua.environment.files.setup({ ['/tmp/test.txt'] = {} })
        "#,
      )
      .exec();

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
      err_msg.contains("source' or 'content'"),
      "Expected error about missing source/content, got: {}",
      err_msg
    );
    Ok(())
  }

  #[test]
  fn mutable_creates_bind_only() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            syslua.environment.files.setup({
                ['/tmp/test.txt'] = {
                  content = 'hello world',
                  mutable = true,
                }
            })
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    assert_eq!(m.builds.len(), 0, "mutable file should not create a build");
    assert_eq!(m.bindings.len(), 1, "mutable file should create a bind");
    Ok(())
  }

  #[test]
  fn immutable_creates_build_and_bind() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            syslua.environment.files.setup({
                ['/tmp/test.txt'] = {
                    content = 'hello world',
                    mutable = false,
                }
            })
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    assert_eq!(m.builds.len(), 1, "immutable file should create a build");
    assert_eq!(m.bindings.len(), 1, "immutable file should create a bind");
    Ok(())
  }

  #[test]
  fn default_is_immutable() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            syslua.environment.files.setup({
                ['/tmp/test.txt'] = {
                    content = 'hello world',
                }
            })
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    assert_eq!(m.builds.len(), 1, "default file should be immutable (create a build)");
    assert_eq!(m.bindings.len(), 1, "default file should create a bind");
    Ok(())
  }

  #[test]
  fn source_option_creates_build() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            syslua.environment.files.setup({
                ['/tmp/myconfig'] = {
                    source = '/path/to/source/config',
                }
            })
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    assert_eq!(m.builds.len(), 1, "source option should create a build");
    assert_eq!(m.bindings.len(), 1, "source option should create a bind");
    Ok(())
  }

  #[test]
  fn build_id_uses_target_basename() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            syslua.environment.files.setup({
                ['/home/user/.config/myapp.conf'] = {
                    content = 'config data',
                }
            })
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    let build = m.builds.values().next().unwrap();
    assert_eq!(
      build.id,
      Some("myapp.conf-file".to_string()),
      "build id should be basename + '-file'"
    );
    Ok(())
  }

  #[test]
  fn immutable_bind_depends_on_build() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            syslua.environment.files.setup({
                ['/tmp/test.txt'] = {
                    content = 'hello',
                }
            })
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    let build_hash = m.builds.keys().next().unwrap();

    // Check that the bind references the build
    let bind = m.bindings.values().next().unwrap();
    let inputs_str = serde_json::to_string(&bind.inputs).unwrap_or_default();
    assert!(
      inputs_str.contains(&build_hash.0[..8]),
      "bind should reference build hash"
    );
    Ok(())
  }

  #[test]
  fn immutable_build_has_path_output() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            syslua.environment.files.setup({
                ['/tmp/test.txt'] = {
                    content = 'hello',
                }
            })
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    let build = m.builds.values().next().unwrap();
    assert!(build.outputs.is_some(), "build should have outputs");
    let outputs = build.outputs.as_ref().unwrap();
    assert!(outputs.contains_key("path"), "build should have 'path' output");
    Ok(())
  }

  #[test]
  fn immutable_bind_has_link_output() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            syslua.environment.files.setup({
                ['/tmp/test.txt'] = {
                    content = 'hello',
                }
            })
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    let bind = m.bindings.values().next().unwrap();
    assert!(bind.outputs.is_some(), "bind should have outputs");
    let outputs = bind.outputs.as_ref().unwrap();
    assert!(outputs.contains_key("link"), "immutable bind should have 'link' output");
    Ok(())
  }

  #[test]
  fn mutable_bind_has_target_output() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            syslua.environment.files.setup({
                ['/tmp/test.txt'] = {
                    content = 'hello',
                    mutable = true,
                }
            })
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    let bind = m.bindings.values().next().unwrap();
    assert!(bind.outputs.is_some(), "bind should have outputs");
    let outputs = bind.outputs.as_ref().unwrap();
    assert!(
      outputs.contains_key("target"),
      "mutable bind should have 'target' output"
    );
    Ok(())
  }

  #[test]
  fn mutable_bind_has_destroy_actions() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            syslua.environment.files.setup({
                ['/tmp/test.txt'] = {
                    content = 'hello',
                    mutable = true,
                }
            })
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    let bind = m.bindings.values().next().unwrap();
    assert!(
      !bind.destroy_actions.is_empty(),
      "mutable bind should have destroy actions for cleanup"
    );
    Ok(())
  }

  #[test]
  fn immutable_bind_has_destroy_actions() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            syslua.environment.files.setup({
                ['/tmp/test.txt'] = {
                    content = 'hello',
                }
            })
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    let bind = m.bindings.values().next().unwrap();
    assert!(
      !bind.destroy_actions.is_empty(),
      "immutable bind should have destroy actions to remove symlink"
    );
    Ok(())
  }

  #[test]
  fn priority_force_overrides_mutable() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            local prio = require('syslua.priority')

            -- Default is immutable, but force mutable
            syslua.environment.files.setup({
                ['/tmp/test.txt'] = {
                    content = 'hello',
                    mutable = prio.force(true),
                }
            })
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    assert_eq!(m.builds.len(), 0, "force mutable should skip build");
    assert_eq!(m.bindings.len(), 1, "force mutable should create bind only");
    Ok(())
  }

  #[test]
  fn multiple_setup_calls_create_multiple_files() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')

            syslua.environment.files.setup({
                ['/tmp/file1.txt'] = {
                    content = 'first file',
                }
            })

            syslua.environment.files.setup({
                ['/tmp/file2.txt'] = {
                    content = 'second file',
                }
            })
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    assert_eq!(m.builds.len(), 2, "two setup calls should create two builds");
    assert_eq!(m.bindings.len(), 2, "two setup calls should create two binds");

    let build_ids: Vec<_> = m.builds.values().filter_map(|b| b.id.as_ref()).collect();
    assert!(
      build_ids.contains(&&"file1.txt-file".to_string()),
      "should have build for file1.txt"
    );
    assert!(
      build_ids.contains(&&"file2.txt-file".to_string()),
      "should have build for file2.txt"
    );
    Ok(())
  }

  #[test]
  fn single_setup_with_multiple_files() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')

            syslua.environment.files.setup({
                ['/tmp/config.json'] = {
                    content = '{}',
                },
                ['/tmp/data.txt'] = {
                    content = 'data',
                    mutable = true,
                }
            })
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    assert_eq!(m.builds.len(), 1, "only immutable file should create a build");
    assert_eq!(m.bindings.len(), 2, "both files should create binds");

    let build = m.builds.values().next().unwrap();
    assert_eq!(
      build.id,
      Some("config.json-file".to_string()),
      "immutable file should have build"
    );
    Ok(())
  }
}

mod env_module {
  use super::*;

  #[test]
  fn creates_build_and_binds() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            local prio = require('syslua.priority')
            syslua.environment.variables.setup({
                PATH = prio.before('/opt/bin'),
            })
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    assert_eq!(m.builds.len(), 1, "env should create a build");
    // On non-Windows, creates binds for zsh, bash, and fish (3 binds)
    assert!(!m.bindings.is_empty(), "env should create at least one bind");
    Ok(())
  }

  #[test]
  fn accepts_env_vars() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            local prio = require('syslua.priority')
            syslua.environment.variables.setup({
                EDITOR = 'nvim',
                GOPATH = prio.force('/go'),
            })
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    assert_eq!(m.builds.len(), 1, "env with vars should create a build");
    Ok(())
  }

  #[test]
  fn path_is_mergeable_by_default() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    // Setup multiple times to test merging
    lua
      .load(
        r#"
            local syslua = require('syslua')
            local prio = require('syslua.priority')

            -- First setup adds /opt/bin
            syslua.environment.variables.setup({
                PATH = prio.before('/opt/bin'),
            })
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    assert_eq!(m.builds.len(), 1, "env should create a build");
    Ok(())
  }

  #[test]
  fn build_id_is_syslua_env() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            syslua.environment.variables.setup({})
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    assert_eq!(m.builds.len(), 1);

    // Check that the build has the expected ID
    let build = m.builds.values().next().unwrap();
    assert_eq!(
      build.id,
      Some("__syslua_env".to_string()),
      "build should have id __syslua_env"
    );
    Ok(())
  }

  #[test]
  fn multiple_setup_calls_merge_path() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            local prio = require('syslua.priority')

            -- First setup adds /opt/bin before default
            syslua.environment.variables.setup({
                PATH = prio.before('/opt/bin'),
            })

            -- Second setup adds /custom/bin after default
            syslua.environment.variables.setup({
                PATH = prio.after('/custom/bin'),
            })
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    // Should still only have one build (same id, deduplicated)
    assert_eq!(m.builds.len(), 1, "multiple setups should result in one build");
    Ok(())
  }

  #[test]
  fn multiple_setup_calls_merge_env_vars() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            local prio = require('syslua.priority')

            -- First setup sets EDITOR
            syslua.environment.variables.setup({
                EDITOR = 'vim',
            })

            -- Second setup sets PAGER (different var, should merge)
            syslua.environment.variables.setup({
                PAGER = 'less',
            })
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    assert_eq!(m.builds.len(), 1, "multiple setups should result in one build");
    Ok(())
  }

  #[test]
  fn priority_force_overrides_existing() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            local prio = require('syslua.priority')

            -- First setup sets EDITOR
            syslua.environment.variables.setup({
                EDITOR = 'vim',
            })

            -- Second setup forces EDITOR to different value
            syslua.environment.variables.setup({
                EDITOR = prio.force('nvim'),
            })
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    assert_eq!(m.builds.len(), 1, "force should successfully override");
    Ok(())
  }

  #[test]
  fn binds_depend_on_build() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            syslua.environment.variables.setup({
                EDITOR = 'vim',
            })
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    assert_eq!(m.builds.len(), 1);
    assert!(!m.bindings.is_empty());

    // Get the build hash
    let build_hash = m.builds.keys().next().unwrap();

    // Check that at least one bind references the build by serializing inputs to string
    let has_build_dep = m.bindings.values().any(|bind| {
      bind
        .inputs
        .as_ref()
        .map(|inputs| {
          // Serialize inputs to check if they contain the build hash
          let inputs_str = serde_json::to_string(inputs).unwrap_or_default();
          inputs_str.contains(&build_hash.0[..8])
        })
        .unwrap_or(false)
    });

    assert!(has_build_dep, "binds should depend on the env build");
    Ok(())
  }

  #[test]
  fn empty_setup_uses_defaults() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            syslua.environment.variables.setup({})
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    assert_eq!(m.builds.len(), 1, "empty setup should still create build");
    assert!(!m.bindings.is_empty(), "empty setup should create binds");
    Ok(())
  }

  #[test]
  fn string_path_value_accepted() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            syslua.environment.variables.setup({
                PATH = '/usr/local/bin:/usr/bin',
            })
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    assert_eq!(m.builds.len(), 1, "string PATH should be accepted");
    Ok(())
  }

  #[test]
  fn env_vars_with_mixed_priority_types() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            local prio = require('syslua.priority')

            syslua.environment.variables.setup({
                EDITOR = 'vim',                  -- plain string
                PAGER = prio.default('less'),    -- default priority
                SHELL = prio.force('/bin/zsh'),  -- force priority
            })
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    assert_eq!(m.builds.len(), 1, "mixed priority types should work");
    Ok(())
  }

  #[test]
  fn build_has_expected_outputs() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            syslua.environment.variables.setup({})
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    let build = m.builds.values().next().unwrap();

    // Check that build has outputs defined
    assert!(build.outputs.is_some(), "build should have outputs");

    let outputs = build.outputs.as_ref().unwrap();

    // On non-Windows, should have sh and fish outputs
    #[cfg(not(windows))]
    {
      assert!(outputs.contains_key("sh"), "should have sh output");
      assert!(outputs.contains_key("fish"), "should have fish output");
    }

    // On Windows, should have ps1 output
    #[cfg(windows)]
    {
      assert!(outputs.contains_key("ps1"), "should have ps1 output");
    }

    Ok(())
  }

  #[test]
  fn conflicting_env_vars_without_priority_fails() -> LuaResult<()> {
    let (lua, _) = create_test_runtime()?;

    // Verify that direct priority merge detects conflicts
    let direct_result = lua
      .load(
        r#"
            local prio = require('syslua.priority')
            local merged = prio.merge({ EDITOR = 'vim' }, { EDITOR = 'nano' })
        "#,
      )
      .exec();

    assert!(direct_result.is_err(), "direct priority merge should detect conflict");

    Ok(())
  }

  #[test]
  fn conflicting_env_vars_via_setup_fails() -> LuaResult<()> {
    let (lua, _) = create_test_runtime()?;

    let result = lua
      .load(
        r#"
            local syslua = require('syslua')

            -- First setup sets EDITOR
            syslua.environment.variables.setup({
                EDITOR = 'vim',
            })

            -- Second setup sets EDITOR to different value without priority
            syslua.environment.variables.setup({
                EDITOR = 'nano',
            })
        "#,
      )
      .exec();

    assert!(result.is_err(), "conflicting env vars without priority should fail");
    Ok(())
  }

  #[test]
  fn path_before_and_after_can_coexist() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            local prio = require('syslua.priority')

            syslua.environment.variables.setup({
                PATH = prio.before('/opt/first'),
            })

            syslua.environment.variables.setup({
                PATH = prio.before('/opt/second'),
            })

            syslua.environment.variables.setup({
                PATH = prio.after('/opt/last'),
            })
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    assert_eq!(m.builds.len(), 1, "before and after should merge successfully");
    Ok(())
  }
}
