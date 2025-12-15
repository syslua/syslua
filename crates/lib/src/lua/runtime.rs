use mlua::prelude::*;

use crate::lua::{globals, loader};

pub fn create_runtime() -> LuaResult<Lua> {
  let lua = Lua::new();
  lua
    .load(
      r#"
      package.path = package.path .. ";./?.lua;./?/init.lua;./lua/?.lua;./lua/?/init.lua"
    "#,
    )
    .exec()?;

  // Install custom module loaders that inject __dir into each loaded file
  loader::install_loaders(&lua)?;

  // Register global tables (sys.platform, sys.os, sys.arch, etc.)
  globals::register_globals(&lua)?;

  Ok(lua)
}
