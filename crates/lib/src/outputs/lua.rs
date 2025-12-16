use std::collections::BTreeMap;

use mlua::prelude::*;

/// Convert a Lua table of outputs to a BTreeMap.
pub fn parse_outputs(table: LuaTable) -> LuaResult<BTreeMap<String, String>> {
  let mut outputs = BTreeMap::new();
  for pair in table.pairs::<String, String>() {
    let (k, v) = pair?;
    outputs.insert(k, v);
  }
  Ok(outputs)
}

pub fn outputs_to_lua_table(lua: &Lua, outputs: &BTreeMap<String, String>) -> LuaResult<LuaTable> {
  let table = lua.create_table()?;
  for (k, v) in outputs {
    table.set(k.as_str(), v.as_str())?;
  }
  Ok(table)
}
