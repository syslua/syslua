//! Global Lua values and the `sys` table.
//!
//! This module registers the `sys` global table which provides:
//! - `sys.platform` - Platform triple (e.g., "aarch64-darwin")
//! - `sys.os` - Operating system name (e.g., "darwin", "linux", "windows")
//! - `sys.arch` - CPU architecture (e.g., "x86_64", "aarch64")
//! - `sys.path` - Path manipulation utilities
//! - `sys.build{}` - Define a build
//! - `sys.bind{}` - Define a bind
//! - `sys.register_ctx_method()` - Register a custom ActionCtx method
//! - `sys.unregister_ctx_method()` - Remove a registered ActionCtx method

use std::cell::RefCell;
use std::rc::Rc;

use mlua::prelude::*;

use super::helpers;
use crate::action::{BUILTIN_CTX_METHODS, CTX_METHODS_REGISTRY_KEY};
use crate::bind::lua::register_sys_bind;
use crate::build::lua::register_sys_build;
use crate::manifest::Manifest;
use crate::platform::Platform;

/// Register the `sys` global table in the Lua runtime.
///
/// This function creates the `sys` table with platform information, utilities,
/// and the `sys.build{}` and `sys.bind{}` functions, making it available as a global in Lua scripts.
pub fn register_globals(lua: &Lua, manifest: Rc<RefCell<Manifest>>) -> LuaResult<()> {
  let sys = lua.create_table()?;

  // Platform information
  let platform = Platform::current().ok_or_else(|| LuaError::external("unsupported platform"))?;

  sys.set("platform", platform.triple())?;
  sys.set("os", platform.os.as_str())?;
  sys.set("arch", platform.arch.as_str())?;

  // Path utilities
  let path = helpers::path::create_path_helpers(lua)?;
  sys.set("path", path)?;

  // Register sys.build{}
  register_sys_build(lua, &sys, manifest.clone())?;

  // Register sys.bind{}
  register_sys_bind(lua, &sys, manifest)?;

  // Initialize the ctx method registry (empty table)
  lua.set_named_registry_value(CTX_METHODS_REGISTRY_KEY, lua.create_table()?)?;

  // Register sys.register_ctx_method(name, fn)
  let register_ctx_method = lua.create_function(|lua, (name, func): (String, LuaFunction)| {
    // Prevent overwriting built-in methods
    if BUILTIN_CTX_METHODS.contains(&name.as_str()) {
      return Err(LuaError::external(format!(
        "cannot override built-in ctx method '{}'",
        name
      )));
    }

    let registry: LuaTable = lua.named_registry_value(CTX_METHODS_REGISTRY_KEY)?;
    registry.set(name, func)?;
    Ok(())
  })?;
  sys.set("register_ctx_method", register_ctx_method)?;

  // Register sys.unregister_ctx_method(name)
  let unregister_ctx_method = lua.create_function(|lua, name: String| {
    let registry: LuaTable = lua.named_registry_value(CTX_METHODS_REGISTRY_KEY)?;
    registry.set(name, LuaValue::Nil)?;
    Ok(())
  })?;
  sys.set("unregister_ctx_method", unregister_ctx_method)?;

  // Register as global
  lua.globals().set("sys", sys)?;

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn create_test_lua() -> LuaResult<Lua> {
    let lua = Lua::new();
    let manifest = Rc::new(RefCell::new(Manifest::default()));
    register_globals(&lua, manifest)?;
    Ok(lua)
  }

  mod sys_table {
    use super::*;

    #[test]
    fn sys_global_exists() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let sys: LuaTable = lua.globals().get("sys")?;
      assert!(sys.contains_key("platform")?);
      assert!(sys.contains_key("os")?);
      assert!(sys.contains_key("arch")?);
      assert!(sys.contains_key("path")?);
      assert!(sys.contains_key("build")?);
      assert!(sys.contains_key("bind")?);
      Ok(())
    }

    #[test]
    fn platform_is_valid_triple() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let platform: String = lua.load("return sys.platform").eval()?;

      // Should be in format "arch-os"
      assert!(platform.contains('-'), "Platform should contain a hyphen: {}", platform);

      let parts: Vec<&str> = platform.split('-').collect();
      assert_eq!(parts.len(), 2, "Platform should have exactly two parts");

      // Verify arch is valid
      let valid_archs = ["x86_64", "aarch64"];
      assert!(valid_archs.contains(&parts[0]), "Invalid arch: {}", parts[0]);

      // Verify os is valid
      let valid_os = ["darwin", "linux", "windows"];
      assert!(valid_os.contains(&parts[1]), "Invalid os: {}", parts[1]);

      Ok(())
    }

    #[test]
    fn os_matches_platform() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let os: String = lua.load("return sys.os").eval()?;
      let platform: String = lua.load("return sys.platform").eval()?;

      assert!(
        platform.ends_with(&os),
        "Platform '{}' should end with os '{}'",
        platform,
        os
      );
      Ok(())
    }

    #[test]
    fn arch_matches_platform() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let arch: String = lua.load("return sys.arch").eval()?;
      let platform: String = lua.load("return sys.platform").eval()?;

      assert!(
        platform.starts_with(&arch),
        "Platform '{}' should start with arch '{}'",
        platform,
        arch
      );
      Ok(())
    }
  }

  mod path_helpers {
    use super::*;

    #[test]
    fn join_single_segment() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let result: String = lua.load(r#"return sys.path.join("foo")"#).eval()?;
      assert_eq!(result, "foo");
      Ok(())
    }

    #[test]
    fn join_multiple_segments() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let result: String = lua.load(r#"return sys.path.join("foo", "bar", "baz")"#).eval()?;
      assert!(
        result == "foo/bar/baz" || result == r"foo\bar\baz",
        "Unexpected path: {}",
        result
      );
      Ok(())
    }

    #[test]
    fn dirname_returns_parent() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let result: String = lua.load(r#"return sys.path.dirname("/foo/bar/baz.txt")"#).eval()?;
      assert_eq!(result, "/foo/bar");
      Ok(())
    }

    #[test]
    fn basename_returns_filename() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let result: String = lua.load(r#"return sys.path.basename("/foo/bar/baz.txt")"#).eval()?;
      assert_eq!(result, "baz.txt");
      Ok(())
    }

    #[test]
    fn extname_returns_extension_with_dot() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let result: String = lua.load(r#"return sys.path.extname("/foo/bar/baz.txt")"#).eval()?;
      assert_eq!(result, ".txt");
      Ok(())
    }

    #[test]
    fn extname_returns_empty_for_no_extension() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let result: String = lua.load(r#"return sys.path.extname("/foo/bar/baz")"#).eval()?;
      assert_eq!(result, "");
      Ok(())
    }

    #[test]
    fn is_absolute_detects_absolute_paths() -> LuaResult<()> {
      let lua = create_test_lua()?;

      // Test platform-appropriate absolute paths
      #[cfg(unix)]
      {
        let abs: bool = lua.load(r#"return sys.path.is_absolute("/foo/bar")"#).eval()?;
        assert!(abs, "Unix absolute path should be detected");
      }

      #[cfg(windows)]
      {
        let abs: bool = lua.load(r#"return sys.path.is_absolute("C:\\foo\\bar")"#).eval()?;
        assert!(abs, "Windows absolute path should be detected");
      }

      let rel: bool = lua.load(r#"return sys.path.is_absolute("foo/bar")"#).eval()?;
      assert!(!rel, "Relative path should not be absolute");

      Ok(())
    }

    #[test]
    fn normalize_resolves_dots() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let result: String = lua
        .load(r#"return sys.path.normalize("/foo/bar/../baz/./qux")"#)
        .eval()?;
      // Path normalization uses platform separators
      #[cfg(unix)]
      assert_eq!(result, "/foo/baz/qux");
      #[cfg(windows)]
      assert_eq!(result, "\\foo\\baz\\qux");
      Ok(())
    }

    #[test]
    fn relative_computes_relative_path() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let result: String = lua
        .load(r#"return sys.path.relative("/foo/bar", "/foo/baz/qux")"#)
        .eval()?;
      // Relative paths use platform separators
      #[cfg(unix)]
      assert_eq!(result, "../baz/qux");
      #[cfg(windows)]
      assert_eq!(result, "..\\baz\\qux");
      Ok(())
    }

    #[test]
    fn relative_same_path_returns_dot() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let result: String = lua.load(r#"return sys.path.relative("/foo/bar", "/foo/bar")"#).eval()?;
      assert_eq!(result, ".");
      Ok(())
    }

    #[test]
    fn split_returns_components() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let result: LuaTable = lua.load(r#"return sys.path.split("/foo/bar/baz")"#).eval()?;

      // Root component differs by platform
      let first: String = result.get(1)?;
      #[cfg(unix)]
      assert_eq!(first, "/");
      #[cfg(windows)]
      assert_eq!(first, "\\");

      let second: String = result.get(2)?;
      assert_eq!(second, "foo");

      let third: String = result.get(3)?;
      assert_eq!(third, "bar");

      let fourth: String = result.get(4)?;
      assert_eq!(fourth, "baz");

      Ok(())
    }
  }

  mod ctx_method_registration {
    use super::*;

    #[test]
    fn register_ctx_method_exists() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let sys: LuaTable = lua.globals().get("sys")?;
      assert!(sys.contains_key("register_ctx_method")?);
      assert!(sys.contains_key("unregister_ctx_method")?);
      Ok(())
    }

    #[test]
    fn register_ctx_method_adds_to_registry() -> LuaResult<()> {
      let lua = create_test_lua()?;
      lua
        .load(
          r#"
        sys.register_ctx_method("my_custom_method", function(ctx, arg)
          return "called with: " .. arg
        end)
        "#,
        )
        .exec()?;

      // Verify the method was registered
      let registry: LuaTable = lua.named_registry_value(crate::action::CTX_METHODS_REGISTRY_KEY)?;
      assert!(registry.contains_key("my_custom_method")?);
      Ok(())
    }

    #[test]
    fn unregister_ctx_method_removes_from_registry() -> LuaResult<()> {
      let lua = create_test_lua()?;
      lua
        .load(
          r#"
        sys.register_ctx_method("temp_method", function(ctx) return "temp" end)
        sys.unregister_ctx_method("temp_method")
        "#,
        )
        .exec()?;

      // Verify the method was removed
      let registry: LuaTable = lua.named_registry_value(crate::action::CTX_METHODS_REGISTRY_KEY)?;
      let value: LuaValue = registry.get("temp_method")?;
      assert!(value.is_nil());
      Ok(())
    }

    #[test]
    fn cannot_override_builtin_exec() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let result = lua
        .load(
          r#"
        sys.register_ctx_method("exec", function(ctx) return "hacked" end)
        "#,
        )
        .exec();

      assert!(result.is_err());
      let err = result.unwrap_err().to_string();
      assert!(err.contains("cannot override built-in ctx method 'exec'"));
      Ok(())
    }

    #[test]
    fn cannot_override_builtin_fetch_url() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let result = lua
        .load(
          r#"
        sys.register_ctx_method("fetch_url", function(ctx) return "hacked" end)
        "#,
        )
        .exec();

      assert!(result.is_err());
      let err = result.unwrap_err().to_string();
      assert!(err.contains("cannot override built-in ctx method 'fetch_url'"));
      Ok(())
    }

    #[test]
    fn cannot_override_builtin_write_file() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let result = lua
        .load(
          r#"
        sys.register_ctx_method("write_file", function(ctx) return "hacked" end)
        "#,
        )
        .exec();

      assert!(result.is_err());
      let err = result.unwrap_err().to_string();
      assert!(err.contains("cannot override built-in ctx method 'write_file'"));
      Ok(())
    }

    #[test]
    fn cannot_override_builtin_out() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let result = lua
        .load(
          r#"
        sys.register_ctx_method("out", function(ctx) return "hacked" end)
        "#,
        )
        .exec();

      assert!(result.is_err());
      let err = result.unwrap_err().to_string();
      assert!(err.contains("cannot override built-in ctx method 'out'"));
      Ok(())
    }
  }
}
