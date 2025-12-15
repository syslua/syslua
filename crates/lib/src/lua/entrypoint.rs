use std::collections::HashMap;
use std::path::Path;

use mlua::Table;

use crate::lua::{loader, runtime};

pub fn extract_inputs(entrypoint_path: &str) -> Result<HashMap<String, String>, mlua::Error> {
  let lua = runtime::create_runtime()?;

  let path = Path::new(entrypoint_path);
  let result = loader::load_file_with_dir(&lua, path)?;
  let result_table = result
    .as_table()
    .ok_or_else(|| mlua::Error::external("entrypoint must return a table"))?;

  let inputs_table: Table = result_table.get("inputs")?;

  let mut inputs = HashMap::new();
  for pair in inputs_table.pairs::<String, String>() {
    let (key, value) = pair?;
    inputs.insert(key, value);
  }

  Ok(inputs)
}
