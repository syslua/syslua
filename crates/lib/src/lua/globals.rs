//! Global Lua values and the `sys` table.
//!
//! This module registers the `sys` global table which provides:
//! - `sys.platform` - Platform triple (e.g., "aarch64-darwin")
//! - `sys.os` - Operating system name (e.g., "darwin", "linux", "windows")
//! - `sys.arch` - CPU architecture (e.g., "x86_64", "aarch64")
//! - `sys.version` - syslua version string
//! - `sys.path` - Path manipulation utilities

use mlua::prelude::*;

use super::helpers;
use crate::platform::Platform;

/// Register the `sys` global table in the Lua runtime.
///
/// This function creates the `sys` table with platform information and utilities,
/// making it available as a global in Lua scripts.
pub fn register_globals(lua: &Lua) -> LuaResult<()> {
  let sys = lua.create_table()?;

  // Platform information
  let platform = Platform::current().ok_or_else(|| LuaError::external("unsupported platform"))?;

  sys.set("platform", platform.triple())?;
  sys.set("os", platform.os.as_str())?;
  sys.set("arch", platform.arch.as_str())?;

  // Path utilities
  let path = helpers::path::create_path_helpers(lua)?;
  sys.set("path", path)?;

  // Register as global
  lua.globals().set("sys", sys)?;

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn create_test_lua() -> LuaResult<Lua> {
    let lua = Lua::new();
    register_globals(&lua)?;
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
      assert!(sys.contains_key("version")?);
      assert!(sys.contains_key("path")?);
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

    #[test]
    fn version_is_semver() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let version: String = lua.load("return sys.version").eval()?;

      // Should match semver pattern (at least x.y.z)
      let parts: Vec<&str> = version.split('.').collect();
      assert!(
        parts.len() >= 2,
        "Version should have at least major.minor: {}",
        version
      );

      // Each part should be numeric (at least the first two)
      for (i, part) in parts.iter().take(2).enumerate() {
        assert!(
          part.parse::<u32>().is_ok(),
          "Version part {} should be numeric: {}",
          i,
          part
        );
      }

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
