use std::cell::RefCell;
use std::rc::Rc;

use mlua::prelude::*;

use crate::lua::{globals, loaders};
use crate::manifest::Manifest;

pub fn create_runtime(manifest: Rc<RefCell<Manifest>>) -> LuaResult<Lua> {
  let lua = Lua::new();
  lua
    .load(
      r#"
      package.path = package.path .. ";./?.lua;./?/init.lua;./lua/?.lua;./lua/?/init.lua"
    "#,
    )
    .exec()?;

  // Install custom module loaders that inject sys.dir into each loaded file
  loaders::install_loaders(&lua)?;

  // Register global tables (sys.platform, sys.os, sys.arch, sys.build, etc.)
  globals::register_globals(&lua, manifest)?;

  Ok(lua)
}
