use mlua::Lua;
use mlua::prelude::*;

/// Create the `sys.path` table with path manipulation utilities.
pub fn create_path_helpers(lua: &Lua) -> LuaResult<LuaTable> {
  let path = lua.create_table()?;

  // sys.path.join(...) - Join multiple path segments
  path.set(
    "join",
    lua.create_function(|_, segments: LuaMultiValue| {
      let mut result = std::path::PathBuf::new();
      for segment in segments {
        if let LuaValue::String(s) = segment {
          result.push(s.to_str()?.as_ref());
        }
      }
      Ok(result.to_string_lossy().into_owned())
    })?,
  )?;

  // sys.path.dirname(path) - Get parent directory
  path.set(
    "dirname",
    lua.create_function(|_, path_str: String| {
      let path = std::path::Path::new(&path_str);
      Ok(
        path
          .parent()
          .map(|p| p.to_string_lossy().into_owned())
          .unwrap_or_default(),
      )
    })?,
  )?;

  // sys.path.basename(path) - Get file name
  path.set(
    "basename",
    lua.create_function(|_, path_str: String| {
      let path = std::path::Path::new(&path_str);
      Ok(
        path
          .file_name()
          .map(|n| n.to_string_lossy().into_owned())
          .unwrap_or_default(),
      )
    })?,
  )?;

  // sys.path.extname(path) - Get file extension (including the dot)
  path.set(
    "extname",
    lua.create_function(|_, path_str: String| {
      let path = std::path::Path::new(&path_str);
      Ok(
        path
          .extension()
          .map(|e| format!(".{}", e.to_string_lossy()))
          .unwrap_or_default(),
      )
    })?,
  )?;

  // sys.path.is_absolute(path) - Check if path is absolute
  path.set(
    "is_absolute",
    lua.create_function(|_, path_str: String| {
      let path = std::path::Path::new(&path_str);
      Ok(path.is_absolute())
    })?,
  )?;

  // sys.path.normalize(path) - Normalize path (resolve . and ..)
  path.set(
    "normalize",
    lua.create_function(|_, path_str: String| {
      let path = std::path::Path::new(&path_str);
      // Use components to normalize without touching the filesystem
      let mut normalized = std::path::PathBuf::new();
      for component in path.components() {
        match component {
          std::path::Component::ParentDir => {
            normalized.pop();
          }
          std::path::Component::CurDir => {}
          _ => normalized.push(component),
        }
      }
      Ok(normalized.to_string_lossy().into_owned())
    })?,
  )?;

  // sys.path.resolve(...) - Resolve to absolute path
  path.set(
    "resolve",
    lua.create_function(|_, segments: LuaMultiValue| {
      let mut result = std::env::current_dir().unwrap_or_default();
      for segment in segments {
        if let LuaValue::String(s) = segment {
          let seg = s.to_str()?;
          let seg_path = std::path::Path::new(seg.as_ref());
          if seg_path.is_absolute() {
            result = seg_path.to_path_buf();
          } else {
            result.push(seg.as_ref());
          }
        }
      }
      // Normalize the result
      let mut normalized = std::path::PathBuf::new();
      for component in result.components() {
        match component {
          std::path::Component::ParentDir => {
            normalized.pop();
          }
          std::path::Component::CurDir => {}
          _ => normalized.push(component),
        }
      }
      Ok(normalized.to_string_lossy().into_owned())
    })?,
  )?;

  // sys.path.relative(from, to) - Get relative path from one path to another
  path.set(
    "relative",
    lua.create_function(|_, (from, to): (String, String)| {
      let from_path = std::path::Path::new(&from);
      let to_path = std::path::Path::new(&to);

      // Simple implementation: find common prefix and build relative path
      let from_components: Vec<_> = from_path.components().collect();
      let to_components: Vec<_> = to_path.components().collect();

      // Find common prefix length
      let common_len = from_components
        .iter()
        .zip(to_components.iter())
        .take_while(|(a, b)| a == b)
        .count();

      // Build relative path
      let mut relative = std::path::PathBuf::new();

      // Add .. for each remaining component in 'from'
      for _ in common_len..from_components.len() {
        relative.push("..");
      }

      // Add remaining components from 'to'
      for component in to_components.iter().skip(common_len) {
        relative.push(component);
      }

      if relative.as_os_str().is_empty() {
        Ok(".".to_string())
      } else {
        Ok(relative.to_string_lossy().into_owned())
      }
    })?,
  )?;

  // sys.path.split(path) - Split path into components
  path.set(
    "split",
    lua.create_function(|lua, path_str: String| {
      let path = std::path::Path::new(&path_str);
      let components: Vec<String> = path
        .components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect();

      let table = lua.create_table()?;
      for (i, component) in components.into_iter().enumerate() {
        table.set(i + 1, component)?;
      }
      Ok(table)
    })?,
  )?;

  Ok(path)
}
