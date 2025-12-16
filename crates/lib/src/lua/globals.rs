//! Global Lua values and the `sys` table.
//!
//! This module registers the `sys` global table which provides:
//! - `sys.platform` - Platform triple (e.g., "aarch64-darwin")
//! - `sys.os` - Operating system name (e.g., "darwin", "linux", "windows")
//! - `sys.arch` - CPU architecture (e.g., "x86_64", "aarch64")
//! - `sys.path` - Path manipulation utilities
//! - `sys.build{}` - Define a build
//! - `sys.bind{}` - Define a bind

use std::cell::RefCell;
use std::rc::Rc;

use mlua::prelude::*;

use super::helpers;
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

      let abs: bool = lua.load(r#"return sys.path.is_absolute("/foo/bar")"#).eval()?;
      assert!(abs, "Unix absolute path should be detected");

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
      assert_eq!(result, "/foo/baz/qux");
      Ok(())
    }

    #[test]
    fn relative_computes_relative_path() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let result: String = lua
        .load(r#"return sys.path.relative("/foo/bar", "/foo/baz/qux")"#)
        .eval()?;
      assert_eq!(result, "../baz/qux");
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

      let first: String = result.get(1)?;
      assert_eq!(first, "/");

      let second: String = result.get(2)?;
      assert_eq!(second, "foo");

      let third: String = result.get(3)?;
      assert_eq!(third, "bar");

      let fourth: String = result.get(4)?;
      assert_eq!(fourth, "baz");

      Ok(())
    }
  }
}
