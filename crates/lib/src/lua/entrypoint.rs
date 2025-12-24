use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;

use mlua::prelude::*;

use crate::inputs::{InputDecl, InputDecls, InputOverride};
use crate::lua::runtime;
use crate::manifest::Manifest;

/// Extract raw input declarations from an entrypoint file.
///
/// This function parses the `inputs` table from the entrypoint and supports
/// both simple URL strings and extended table syntax:
///
/// ```lua
/// return {
///   inputs = {
///     -- Simple: just a URL
///     utils = "git:https://github.com/org/utils.git",
///
///     -- Extended: URL with transitive overrides
///     pkgs = {
///       url = "git:https://github.com/org/pkgs.git",
///       inputs = {
///         utils = { follows = "utils" },
///       },
///     },
///   },
/// }
/// ```
pub fn extract_input_decls(entrypoint_path: &str) -> LuaResult<InputDecls> {
  let manifest = Rc::new(RefCell::new(Manifest::default()));
  let lua = runtime::create_runtime(manifest)?;

  let path = Path::new(entrypoint_path);
  let result = runtime::load_file(&lua, path)?;

  let result_table = result
    .as_table()
    .ok_or_else(|| LuaError::external("entrypoint must return a table"))?;

  let inputs_value: LuaValue = result_table.get("inputs")?;

  match inputs_value {
    LuaValue::Nil => Ok(BTreeMap::new()),
    LuaValue::Table(inputs_table) => parse_input_decls(&inputs_table),
    _ => Err(LuaError::external("inputs must be a table")),
  }
}

/// Parse an inputs table into InputDecls.
fn parse_input_decls(inputs_table: &LuaTable) -> LuaResult<InputDecls> {
  let mut decls = BTreeMap::new();

  for pair in inputs_table.pairs::<String, LuaValue>() {
    let (name, value) = pair?;
    let decl = parse_input_decl(&name, value)?;
    decls.insert(name, decl);
  }

  Ok(decls)
}

/// Parse a single input declaration.
fn parse_input_decl(name: &str, value: LuaValue) -> LuaResult<InputDecl> {
  match value {
    LuaValue::String(url) => {
      let url_str = url.to_str()?.to_string();
      Ok(InputDecl::Url(url_str))
    }
    LuaValue::Table(table) => {
      // Extended syntax: { url = "...", inputs = { ... } }
      let url: Option<String> = table.get("url")?;
      let inputs_value: LuaValue = table.get("inputs")?;

      let overrides = match inputs_value {
        LuaValue::Nil => BTreeMap::new(),
        LuaValue::Table(inputs_table) => parse_input_overrides(&inputs_table)?,
        _ => {
          return Err(LuaError::external(format!(
            "input '{}': inputs field must be a table",
            name
          )));
        }
      };

      Ok(InputDecl::Extended { url, inputs: overrides })
    }
    _ => Err(LuaError::external(format!(
      "input '{}' must be a string URL or a table",
      name
    ))),
  }
}

/// Parse input overrides from a table.
fn parse_input_overrides(table: &LuaTable) -> LuaResult<BTreeMap<String, InputOverride>> {
  let mut overrides = BTreeMap::new();

  for pair in table.pairs::<String, LuaValue>() {
    let (name, value) = pair?;
    let override_ = parse_input_override(&name, value)?;
    overrides.insert(name, override_);
  }

  Ok(overrides)
}

/// Parse a single input override.
fn parse_input_override(name: &str, value: LuaValue) -> LuaResult<InputOverride> {
  match value {
    LuaValue::String(url) => {
      // String is interpreted as a URL override
      let url_str = url.to_str()?.to_string();
      Ok(InputOverride::Url(url_str))
    }
    LuaValue::Table(table) => {
      // Check for follows
      let follows: Option<String> = table.get("follows")?;
      if let Some(follows_path) = follows {
        return Ok(InputOverride::Follows(follows_path));
      }

      // Check for url
      let url: Option<String> = table.get("url")?;
      if let Some(url_str) = url {
        return Ok(InputOverride::Url(url_str));
      }

      Err(LuaError::external(format!(
        "override '{}' must have either 'url' or 'follows' field",
        name
      )))
    }
    _ => Err(LuaError::external(format!(
      "override '{}' must be a string URL or a table with 'url' or 'follows'",
      name
    ))),
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::fs;
  use tempfile::TempDir;

  #[test]
  fn test_extract_simple_inputs() -> LuaResult<()> {
    let temp_dir = TempDir::new().unwrap();
    let entrypoint_path = temp_dir.path().join("init.lua");

    fs::write(
      &entrypoint_path,
      r#"
        return {
          inputs = {
            utils = "git:https://github.com/org/utils.git",
            other = "path:../local",
          },
        }
      "#,
    )
    .unwrap();

    let decls = extract_input_decls(entrypoint_path.to_str().unwrap())?;

    assert_eq!(decls.len(), 2);

    let utils = decls.get("utils").unwrap();
    assert_eq!(utils.url(), Some("git:https://github.com/org/utils.git"));
    assert!(!utils.has_overrides());

    let other = decls.get("other").unwrap();
    assert_eq!(other.url(), Some("path:../local"));

    Ok(())
  }

  #[test]
  fn test_extract_extended_inputs() -> LuaResult<()> {
    let temp_dir = TempDir::new().unwrap();
    let entrypoint_path = temp_dir.path().join("init.lua");

    fs::write(
      &entrypoint_path,
      r#"
        return {
          inputs = {
            utils = "git:https://github.com/org/utils.git",
            pkgs = {
              url = "git:https://github.com/org/pkgs.git",
              inputs = {
                utils = { follows = "utils" },
              },
            },
          },
        }
      "#,
    )
    .unwrap();

    let decls = extract_input_decls(entrypoint_path.to_str().unwrap())?;

    assert_eq!(decls.len(), 2);

    let pkgs = decls.get("pkgs").unwrap();
    assert_eq!(pkgs.url(), Some("git:https://github.com/org/pkgs.git"));
    assert!(pkgs.has_overrides());

    let overrides = pkgs.overrides().unwrap();
    assert_eq!(overrides.len(), 1);

    let utils_override = overrides.get("utils").unwrap();
    assert!(utils_override.is_follows());
    assert_eq!(utils_override.follows_path(), Some("utils"));

    Ok(())
  }

  #[test]
  fn test_extract_url_override() -> LuaResult<()> {
    let temp_dir = TempDir::new().unwrap();
    let entrypoint_path = temp_dir.path().join("init.lua");

    fs::write(
      &entrypoint_path,
      r#"
        return {
          inputs = {
            pkgs = {
              url = "git:https://github.com/org/pkgs.git",
              inputs = {
                utils = { url = "git:https://github.com/myorg/utils.git" },
              },
            },
          },
        }
      "#,
    )
    .unwrap();

    let decls = extract_input_decls(entrypoint_path.to_str().unwrap())?;

    let pkgs = decls.get("pkgs").unwrap();
    let overrides = pkgs.overrides().unwrap();
    let utils_override = overrides.get("utils").unwrap();

    assert!(!utils_override.is_follows());
    assert!(matches!(utils_override, InputOverride::Url(url) if url == "git:https://github.com/myorg/utils.git"));

    Ok(())
  }

  #[test]
  fn test_extract_string_url_override() -> LuaResult<()> {
    let temp_dir = TempDir::new().unwrap();
    let entrypoint_path = temp_dir.path().join("init.lua");

    // String overrides are interpreted as URLs
    fs::write(
      &entrypoint_path,
      r#"
        return {
          inputs = {
            pkgs = {
              url = "git:https://github.com/org/pkgs.git",
              inputs = {
                utils = "git:https://github.com/myorg/utils.git",
              },
            },
          },
        }
      "#,
    )
    .unwrap();

    let decls = extract_input_decls(entrypoint_path.to_str().unwrap())?;

    let pkgs = decls.get("pkgs").unwrap();
    let overrides = pkgs.overrides().unwrap();
    let utils_override = overrides.get("utils").unwrap();

    assert!(matches!(utils_override, InputOverride::Url(url) if url == "git:https://github.com/myorg/utils.git"));

    Ok(())
  }

  #[test]
  fn test_extract_empty_inputs() -> LuaResult<()> {
    let temp_dir = TempDir::new().unwrap();
    let entrypoint_path = temp_dir.path().join("init.lua");

    fs::write(
      &entrypoint_path,
      r#"
        return {
          setup = function() end,
        }
      "#,
    )
    .unwrap();

    let decls = extract_input_decls(entrypoint_path.to_str().unwrap())?;
    assert!(decls.is_empty());

    Ok(())
  }
}
