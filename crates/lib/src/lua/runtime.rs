use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use mlua::prelude::*;

use crate::lua::globals;
use crate::manifest::Manifest;

/// Create a new Lua runtime environment with standard settings.
/// Registers global tables and functions as needed.
/// Returns the initialized Lua instance.
pub fn create_runtime(manifest: Rc<RefCell<Manifest>>) -> LuaResult<Lua> {
  let lua = Lua::new();
  let package_path = lua.globals().get::<LuaTable>("package")?.get::<String>("path")?;
  let new_package_path = format!("./lua/?.lua;./lua/?/init.lua;{}", package_path);
  lua
    .globals()
    .get::<LuaTable>("package")?
    .set("path", new_package_path)?;

  // Register global tables (sys.platform, sys.os, sys.arch, sys.build, etc.)
  globals::register_globals(&lua, manifest)?;

  Ok(lua)
}

/// Load and execute a Lua file at the given path.
/// Sets the `sys.dir` global to the directory of the loaded file.
/// Returns the result of the file execution.
pub fn load_file(lua: &Lua, path: &Path) -> LuaResult<LuaValue> {
  let canonical_path = path
    .canonicalize()
    .map_err(|e| LuaError::external(format!("cannot canonicalize '{}': {}", path.display(), e)))?;
  let content = std::fs::read_to_string(&canonical_path)
    .map_err(|e| LuaError::external(format!("cannot read '{}': {}", canonical_path.display(), e)))?;

  let sys_globals = lua.globals().get::<LuaTable>("sys")?;

  // Set sys.dir global
  sys_globals.set(
    "dir",
    canonical_path
      .parent()
      .unwrap_or(Path::new(""))
      .to_string_lossy()
      .to_string(),
  )?;

  let result = lua
    .load(&content)
    .set_name(format!("@{}", canonical_path.display()))
    .eval::<LuaValue>()?;
  Ok(result)
}
