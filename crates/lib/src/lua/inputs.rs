use std::collections::BTreeMap;

use mlua::prelude::*;

use crate::{
  bind::{BIND_REF_TYPE, BindHash},
  build::{BUILD_REF_TYPE, BuildHash},
  consts::HASH_PREFIX_LEN,
  inputs::InputsRef,
  manifest::Manifest,
};

/// Check if an InputsRef contains any Bind references.
///
/// Returns true if any Bind reference is found anywhere in the tree.
/// This is used to validate that builds don't depend on binds.
pub fn contains_bind_ref(inputs: &InputsRef) -> bool {
  match inputs {
    InputsRef::Bind(_) => true,
    InputsRef::Table(map) => map.values().any(contains_bind_ref),
    InputsRef::Array(arr) => arr.iter().any(contains_bind_ref),
    InputsRef::String(_) | InputsRef::Number(_) | InputsRef::Boolean(_) | InputsRef::Build(_) => false,
  }
}

/// Convert a Lua value to InputsRef (for resolved/static inputs).
///
/// Handles primitives, arrays, tables, and specially-marked BuildRef/BindRef tables
/// (detected via metatable `__type` field).
///
/// Validates that any referenced builds/binds exist in the manifest.
pub fn lua_value_to_inputs_ref(value: LuaValue, manifest: &Manifest) -> LuaResult<InputsRef> {
  match value {
    LuaValue::String(s) => Ok(InputsRef::String(s.to_str()?.to_string())),
    LuaValue::Number(n) => Ok(InputsRef::Number(n)),
    LuaValue::Integer(i) => Ok(InputsRef::Number(i as f64)),
    LuaValue::Boolean(b) => Ok(InputsRef::Boolean(b)),
    LuaValue::Table(t) => {
      // Check metatable for type marker (BuildRef or BindRef)
      if let Some(mt) = t.metatable()
        && let Ok(type_name) = mt.get::<String>("__type")
      {
        match type_name.as_str() {
          BUILD_REF_TYPE => return parse_build_ref_table(&t, manifest),
          BIND_REF_TYPE => return parse_bind_ref_table(&t, manifest),
          _ => {}
        }
      }

      // Check if it's an array (sequential integer keys starting at 1)
      let len = t.raw_len();
      let first_key: Result<LuaValue, _> = t.get(1i64);
      if len > 0 && first_key.is_ok() && first_key.unwrap() != LuaValue::Nil {
        // Treat as array
        let mut arr = Vec::with_capacity(len);
        for i in 1..=len {
          let val: LuaValue = t.get(i)?;
          arr.push(lua_value_to_inputs_ref(val, manifest)?);
        }
        Ok(InputsRef::Array(arr))
      } else {
        // Treat as table/map
        let mut map = BTreeMap::new();
        for pair in t.pairs::<String, LuaValue>() {
          let (k, v) = pair?;
          map.insert(k, lua_value_to_inputs_ref(v, manifest)?);
        }
        Ok(InputsRef::Table(map))
      }
    }
    LuaValue::Nil => Err(LuaError::external("nil values not allowed in inputs")),
    _ => Err(LuaError::external(format!(
      "unsupported input type: {:?}",
      value.type_name()
    ))),
  }
}

/// Parse a Lua table marked as BuildRef into InputsRef::Build.
///
/// Validates that the referenced build exists in the manifest.
fn parse_build_ref_table(t: &LuaTable, manifest: &Manifest) -> LuaResult<InputsRef> {
  let hash: String = t.get("hash")?;
  let build_hash = BuildHash(hash);

  // Validate build exists in manifest
  if !manifest.builds.contains_key(&build_hash) {
    return Err(LuaError::external(format!(
      "referenced build not found in manifest: {}",
      build_hash.0
    )));
  }

  Ok(InputsRef::Build(build_hash))
}

/// Parse a Lua table marked as BindRef into InputsRef::Bind.
///
/// Validates that the referenced bind exists in the manifest.
fn parse_bind_ref_table(t: &LuaTable, manifest: &Manifest) -> LuaResult<InputsRef> {
  let hash: String = t.get("hash")?;
  let bind_hash = BindHash(hash);

  // Validate bind exists in manifest
  if !manifest.bindings.contains_key(&bind_hash) {
    return Err(LuaError::external(format!(
      "referenced bind not found in manifest: {}",
      bind_hash.0
    )));
  }

  Ok(InputsRef::Bind(bind_hash))
}

/// Convert InputsRef to a Lua value for passing to the apply function.
///
/// For Build/Bind references, looks up the definition in the manifest to
/// reconstruct the Lua table with placeholder outputs.
pub fn inputs_ref_to_lua(lua: &Lua, inputs: &InputsRef, manifest: &Manifest) -> LuaResult<LuaValue> {
  match inputs {
    InputsRef::String(s) => Ok(LuaValue::String(lua.create_string(s)?)),
    InputsRef::Number(n) => Ok(LuaValue::Number(*n)),
    InputsRef::Boolean(b) => Ok(LuaValue::Boolean(*b)),
    InputsRef::Array(arr) => {
      let table = lua.create_table()?;
      for (i, val) in arr.iter().enumerate() {
        table.set(i + 1, inputs_ref_to_lua(lua, val, manifest)?)?;
      }
      Ok(LuaValue::Table(table))
    }
    InputsRef::Table(map) => {
      let table = lua.create_table()?;
      for (k, v) in map {
        table.set(k.as_str(), inputs_ref_to_lua(lua, v, manifest)?)?;
      }
      Ok(LuaValue::Table(table))
    }
    InputsRef::Build(hash) => build_hash_to_lua(lua, hash, manifest),
    InputsRef::Bind(hash) => bind_hash_to_lua(lua, hash, manifest),
  }
}

/// Convert a BuildHash to a Lua table by looking up the BuildDef in the manifest.
///
/// Generates placeholder outputs from the BuildDef's output keys.
fn build_hash_to_lua(lua: &Lua, hash: &BuildHash, manifest: &Manifest) -> LuaResult<LuaValue> {
  let build_def = manifest
    .builds
    .get(hash)
    .ok_or_else(|| LuaError::external(format!("build not found in manifest: {}", hash.0)))?;

  let table = lua.create_table()?;
  table.set("name", build_def.name.as_str())?;
  if let Some(v) = &build_def.version {
    table.set("version", v.as_str())?;
  }
  table.set("hash", hash.0.as_str())?;

  // Generate placeholder outputs from BuildDef
  let outputs = lua.create_table()?;
  let short_hash = &hash.0[..HASH_PREFIX_LEN.min(hash.0.len())];
  if let Some(def_outputs) = &build_def.outputs {
    for key in def_outputs.keys() {
      let placeholder = format!("$${{build:{}:{}}}", short_hash, key);
      outputs.set(key.as_str(), placeholder.as_str())?;
    }
  }
  table.set("outputs", outputs)?;

  // Set metatable with __type marker
  let mt = lua.create_table()?;
  mt.set("__type", BUILD_REF_TYPE)?;
  table.set_metatable(Some(mt))?;

  Ok(LuaValue::Table(table))
}

/// Convert a BindHash to a Lua table by looking up the BindDef in the manifest.
///
/// Generates placeholder outputs from the BindDef's output keys (if present).
fn bind_hash_to_lua(lua: &Lua, hash: &BindHash, manifest: &Manifest) -> LuaResult<LuaValue> {
  let bind_def = manifest
    .bindings
    .get(hash)
    .ok_or_else(|| LuaError::external(format!("bind not found in manifest: {}", hash.0)))?;

  let table = lua.create_table()?;
  table.set("hash", hash.0.as_str())?;

  // Generate placeholder outputs from BindDef (if present)
  if let Some(def_outputs) = &bind_def.outputs {
    let outputs = lua.create_table()?;
    let short_hash = &hash.0[..HASH_PREFIX_LEN.min(hash.0.len())];
    for key in def_outputs.keys() {
      let placeholder = format!("$${{bind:{}:{}}}", short_hash, key);
      outputs.set(key.as_str(), placeholder.as_str())?;
    }
    table.set("outputs", outputs)?;
  }

  // Set metatable with __type marker
  let mt = lua.create_table()?;
  mt.set("__type", BIND_REF_TYPE)?;
  table.set_metatable(Some(mt))?;

  Ok(LuaValue::Table(table))
}
