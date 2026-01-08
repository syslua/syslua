//! Tests for syslua.pkgs.* packages.

use mlua::prelude::*;

use super::common::create_test_runtime;

mod extract {
  use super::*;

  #[test]
  fn function_loads() -> LuaResult<()> {
    let (lua, _) = create_test_runtime()?;

    lua
      .load(
        r#"
            local lib = require('syslua.lib')
            assert(type(lib.extract) == 'function', "lib.extract should be a function")
        "#,
      )
      .exec()?;

    Ok(())
  }
}

mod cli_category {
  use super::*;

  #[test]
  fn loads_without_error() -> LuaResult<()> {
    let (lua, _) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            local cli = syslua.pkgs.cli
            assert(cli, "syslua.pkgs.cli should exist")
        "#,
      )
      .exec()?;

    Ok(())
  }

  #[test]
  fn nonexistent_package_throws_clear_error() -> LuaResult<()> {
    let (lua, _) = create_test_runtime()?;

    let result = lua
      .load(
        r#"
            local syslua = require('syslua')
            local _ = syslua.pkgs.cli.nonexistent_package_xyz
        "#,
      )
      .exec();

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
      err_msg.contains("not found"),
      "Expected 'not found' error, got: {}",
      err_msg
    );
    Ok(())
  }
}

mod ripgrep {
  use super::*;

  #[test]
  fn module_loads() -> LuaResult<()> {
    let (lua, _) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            local rg = syslua.pkgs.cli.ripgrep
            assert(rg, "ripgrep module should load")
        "#,
      )
      .exec()?;

    Ok(())
  }

  #[test]
  fn exports_releases() -> LuaResult<()> {
    let (lua, _) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            local rg = syslua.pkgs.cli.ripgrep

            assert(rg.releases, "releases should exist")
            assert(rg.releases['15.1.0'], "version 15.1.0 should exist")
            assert(rg.releases['15.1.0']['aarch64-darwin'], "aarch64-darwin should exist")
            assert(rg.releases['15.1.0']['aarch64-darwin'].url, "url should exist")
            assert(rg.releases['15.1.0']['aarch64-darwin'].sha256, "sha256 should exist")
        "#,
      )
      .exec()?;

    Ok(())
  }

  #[test]
  fn exports_meta() -> LuaResult<()> {
    let (lua, _) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            local rg = syslua.pkgs.cli.ripgrep

            assert(rg.meta, "meta should exist")
            assert(rg.meta.name == 'ripgrep', "name should be 'ripgrep'")
            assert(rg.meta.homepage, "homepage should exist")
            assert(rg.meta.description, "description should exist")
            assert(rg.meta.license, "license should exist")
            assert(rg.meta.versions, "versions should exist")
            assert(rg.meta.versions.stable == '15.1.0', "stable version should be 15.1.0")
        "#,
      )
      .exec()?;

    Ok(())
  }

  #[test]
  fn exports_opts() -> LuaResult<()> {
    let (lua, _) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            local rg = syslua.pkgs.cli.ripgrep

            assert(rg.opts, "opts should exist")
            assert(rg.opts.version, "version option should exist")
        "#,
      )
      .exec()?;

    Ok(())
  }

  #[test]
  fn setup_creates_build() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            local ref = syslua.pkgs.cli.ripgrep.setup()
            assert(ref, "setup should return a BuildRef")
            assert(ref.outputs, "BuildRef should have outputs")
            assert(ref.outputs.bin, "outputs should have bin")
            assert(ref.outputs.out, "outputs should have out")
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    assert!(
      !m.builds.is_empty(),
      "setup() should create at least one build (via lib.extract)"
    );

    Ok(())
  }

  #[test]
  fn invalid_version_shows_available() -> LuaResult<()> {
    let (lua, _) = create_test_runtime()?;

    let result = lua
      .load(
        r#"
            local syslua = require('syslua')
            syslua.pkgs.cli.ripgrep.setup({ version = 'nonexistent-version' })
        "#,
      )
      .exec();

    assert!(result.is_err());
    let err_msg = result.unwrap_err().to_string();
    assert!(
      err_msg.contains("not found") && err_msg.contains("Available"),
      "Error should mention 'not found' and 'Available', got: {}",
      err_msg
    );
    Ok(())
  }
}

mod fd {
  use super::*;

  #[test]
  fn module_loads() -> LuaResult<()> {
    let (lua, _) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            local fd = syslua.pkgs.cli.fd
            assert(fd, "fd module should load")
        "#,
      )
      .exec()?;

    Ok(())
  }

  #[test]
  fn exports_required_fields() -> LuaResult<()> {
    let (lua, _) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            local fd = syslua.pkgs.cli.fd

            assert(fd.releases, "releases should exist")
            assert(fd.meta, "meta should exist")
            assert(fd.meta.name == 'fd', "name should be 'fd'")
            assert(fd.opts, "opts should exist")
            assert(fd.setup, "setup should exist")
        "#,
      )
      .exec()?;

    Ok(())
  }

  #[test]
  fn setup_creates_build() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            local ref = syslua.pkgs.cli.fd.setup()
            assert(ref, "setup should return a BuildRef")
            assert(ref.outputs.bin, "outputs should have bin")
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    assert!(
      !m.builds.is_empty(),
      "setup() should create at least one build (via lib.extract)"
    );

    Ok(())
  }
}

mod jq {
  use super::*;

  #[test]
  fn module_loads() -> LuaResult<()> {
    let (lua, _) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            local jq = syslua.pkgs.cli.jq
            assert(jq, "jq module should load")
        "#,
      )
      .exec()?;

    Ok(())
  }

  #[test]
  fn exports_required_fields() -> LuaResult<()> {
    let (lua, _) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            local jq = syslua.pkgs.cli.jq

            assert(jq.releases, "releases should exist")
            assert(jq.meta, "meta should exist")
            assert(jq.meta.name == 'jq', "name should be 'jq'")
            assert(jq.opts, "opts should exist")
            assert(jq.setup, "setup should exist")
        "#,
      )
      .exec()?;

    Ok(())
  }

  #[test]
  fn setup_creates_build() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            local ref = syslua.pkgs.cli.jq.setup()
            assert(ref, "setup should return a BuildRef")
            assert(ref.outputs.bin, "outputs should have bin")
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    assert!(!m.builds.is_empty(), "should have at least one build");

    Ok(())
  }

  #[test]
  fn standalone_binary_no_extract() -> LuaResult<()> {
    let (lua, manifest) = create_test_runtime()?;

    lua
      .load(
        r#"
            local syslua = require('syslua')
            syslua.pkgs.cli.jq.setup()
        "#,
      )
      .exec()?;

    let m = manifest.borrow();
    assert!(!m.builds.is_empty(), "jq build should exist");

    Ok(())
  }
}
