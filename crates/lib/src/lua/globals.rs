//! Global Lua values and the `sys` table.
//!
//! This module registers the `sys` global table which provides:
//! - `sys.platform` - Platform triple (e.g., "aarch64-darwin")
//! - `sys.os` - Operating system name (e.g., "darwin", "linux", "windows")
//! - `sys.arch` - CPU architecture (e.g., "x86_64", "aarch64")
//! - `sys.path` - Path manipulation utilities
//! - `sys.build{}` - Define a build
//! - `sys.bind{}` - Define a bind
//! - `sys.register_build_ctx_method()` - Register a custom BuildCtx method
//! - `sys.register_bind_ctx_method()` - Register a custom BindCtx method

use std::cell::RefCell;
use std::rc::Rc;

use mlua::prelude::*;

use super::helpers;
use crate::action::{
  BIND_CTX_METHODS_REGISTRY_KEY, BUILD_CTX_METHODS_REGISTRY_KEY, BUILTIN_BIND_CTX_METHODS, BUILTIN_BUILD_CTX_METHODS,
};
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

  // Initialize the build and bind ctx method registries (empty tables)
  lua.set_named_registry_value(BUILD_CTX_METHODS_REGISTRY_KEY, lua.create_table()?)?;
  lua.set_named_registry_value(BIND_CTX_METHODS_REGISTRY_KEY, lua.create_table()?)?;

  // Register sys.register_build_ctx_method(name, fn)
  let register_build_ctx_method = lua.create_function(|lua, (name, func): (String, LuaFunction)| {
    // Prevent overwriting built-in methods
    if BUILTIN_BUILD_CTX_METHODS.contains(&name.as_str()) {
      return Err(LuaError::external(format!(
        "cannot override built-in build ctx method '{}'",
        name
      )));
    }

    let registry: LuaTable = lua.named_registry_value(BUILD_CTX_METHODS_REGISTRY_KEY)?;

    // Warn if overwriting an existing custom method
    let existing: LuaValue = registry.get(name.as_str())?;
    if !existing.is_nil() {
      tracing::warn!(method = %name, "overwriting existing build ctx method");
    }

    registry.set(name, func)?;
    Ok(())
  })?;
  sys.set("register_build_ctx_method", register_build_ctx_method)?;

  // Register sys.register_bind_ctx_method(name, fn)
  let register_bind_ctx_method = lua.create_function(|lua, (name, func): (String, LuaFunction)| {
    // Prevent overwriting built-in methods
    if BUILTIN_BIND_CTX_METHODS.contains(&name.as_str()) {
      return Err(LuaError::external(format!(
        "cannot override built-in bind ctx method '{}'",
        name
      )));
    }

    let registry: LuaTable = lua.named_registry_value(BIND_CTX_METHODS_REGISTRY_KEY)?;

    // Warn if overwriting an existing custom method
    let existing: LuaValue = registry.get(name.as_str())?;
    if !existing.is_nil() {
      tracing::warn!(method = %name, "overwriting existing bind ctx method");
    }

    registry.set(name, func)?;
    Ok(())
  })?;
  sys.set("register_bind_ctx_method", register_bind_ctx_method)?;

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

    #[test]
    fn canonicalize_resolves_existing_path() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let temp_dir = std::env::temp_dir();
      let temp_path = temp_dir.to_string_lossy();
      let code = format!(r#"return sys.path.canonicalize("{}")"#, temp_path.replace('\\', "\\\\"));
      let result: String = lua.load(&code).eval()?;
      let expected = dunce::canonicalize(&temp_dir).unwrap();
      assert_eq!(result, expected.to_string_lossy());
      Ok(())
    }

    #[test]
    fn canonicalize_throws_on_nonexistent_path() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let result: Result<String, _> = lua
        .load(r#"return sys.path.canonicalize("/this/path/definitely/does/not/exist/12345")"#)
        .eval();
      assert!(result.is_err());
      let err = result.unwrap_err().to_string();
      assert!(err.contains("failed to canonicalize path"));
      Ok(())
    }
  }

  mod ctx_method_registration {
    use super::*;

    #[test]
    fn register_build_ctx_method_exists() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let sys: LuaTable = lua.globals().get("sys")?;
      assert!(sys.contains_key("register_build_ctx_method")?);
      assert!(sys.contains_key("register_bind_ctx_method")?);
      Ok(())
    }

    #[test]
    fn register_build_ctx_method_adds_to_registry() -> LuaResult<()> {
      let lua = create_test_lua()?;
      lua
        .load(
          r#"
        sys.register_build_ctx_method("my_custom_method", function(ctx, arg)
          return "called with: " .. arg
        end)
        "#,
        )
        .exec()?;

      // Verify the method was registered in build registry
      let registry: LuaTable = lua.named_registry_value(BUILD_CTX_METHODS_REGISTRY_KEY)?;
      assert!(registry.contains_key("my_custom_method")?);
      Ok(())
    }

    #[test]
    fn register_bind_ctx_method_adds_to_registry() -> LuaResult<()> {
      let lua = create_test_lua()?;
      lua
        .load(
          r#"
        sys.register_bind_ctx_method("my_bind_method", function(ctx, arg)
          return "bind called with: " .. arg
        end)
        "#,
        )
        .exec()?;

      // Verify the method was registered in bind registry
      let registry: LuaTable = lua.named_registry_value(BIND_CTX_METHODS_REGISTRY_KEY)?;
      assert!(registry.contains_key("my_bind_method")?);
      Ok(())
    }

    #[test]
    fn build_and_bind_registries_are_separate() -> LuaResult<()> {
      let lua = create_test_lua()?;
      lua
        .load(
          r#"
        sys.register_build_ctx_method("build_only", function(ctx) return "build" end)
        sys.register_bind_ctx_method("bind_only", function(ctx) return "bind" end)
        "#,
        )
        .exec()?;

      // Verify registries are separate
      let build_registry: LuaTable = lua.named_registry_value(BUILD_CTX_METHODS_REGISTRY_KEY)?;
      let bind_registry: LuaTable = lua.named_registry_value(BIND_CTX_METHODS_REGISTRY_KEY)?;

      assert!(build_registry.contains_key("build_only")?);
      assert!(!build_registry.contains_key("bind_only")?);

      assert!(bind_registry.contains_key("bind_only")?);
      assert!(!bind_registry.contains_key("build_only")?);
      Ok(())
    }

    #[test]
    fn cannot_override_builtin_exec_in_build() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let result = lua
        .load(
          r#"
        sys.register_build_ctx_method("exec", function(ctx) return "hacked" end)
        "#,
        )
        .exec();

      assert!(result.is_err());
      let err = result.unwrap_err().to_string();
      assert!(err.contains("cannot override built-in build ctx method 'exec'"));
      Ok(())
    }

    #[test]
    fn cannot_override_builtin_exec_in_bind() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let result = lua
        .load(
          r#"
        sys.register_bind_ctx_method("exec", function(ctx) return "hacked" end)
        "#,
        )
        .exec();

      assert!(result.is_err());
      let err = result.unwrap_err().to_string();
      assert!(err.contains("cannot override built-in bind ctx method 'exec'"));
      Ok(())
    }

    #[test]
    fn cannot_override_builtin_fetch_url_in_build() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let result = lua
        .load(
          r#"
        sys.register_build_ctx_method("fetch_url", function(ctx) return "hacked" end)
        "#,
        )
        .exec();

      assert!(result.is_err());
      let err = result.unwrap_err().to_string();
      assert!(err.contains("cannot override built-in build ctx method 'fetch_url'"));
      Ok(())
    }

    #[test]
    fn can_register_fetch_url_in_bind() -> LuaResult<()> {
      // fetch_url is NOT a builtin for BindCtx, so this should succeed
      let lua = create_test_lua()?;
      lua
        .load(
          r#"
        sys.register_bind_ctx_method("fetch_url", function(ctx) return "custom fetch" end)
        "#,
        )
        .exec()?;

      let registry: LuaTable = lua.named_registry_value(BIND_CTX_METHODS_REGISTRY_KEY)?;
      assert!(registry.contains_key("fetch_url")?);
      Ok(())
    }

    #[test]
    fn cannot_override_builtin_out_in_build() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let result = lua
        .load(
          r#"
        sys.register_build_ctx_method("out", function(ctx) return "hacked" end)
        "#,
        )
        .exec();

      assert!(result.is_err());
      let err = result.unwrap_err().to_string();
      assert!(err.contains("cannot override built-in build ctx method 'out'"));
      Ok(())
    }

    #[test]
    fn cannot_override_builtin_out_in_bind() -> LuaResult<()> {
      let lua = create_test_lua()?;
      let result = lua
        .load(
          r#"
        sys.register_bind_ctx_method("out", function(ctx) return "hacked" end)
        "#,
        )
        .exec();

      assert!(result.is_err());
      let err = result.unwrap_err().to_string();
      assert!(err.contains("cannot override built-in bind ctx method 'out'"));
      Ok(())
    }
  }
}
