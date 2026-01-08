//! Template content for sys init command.

/// Template for init.lua entry point
pub const INIT_LUA_TEMPLATE: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../lua/template.lua"));

/// Embedded globals.d.lua type definitions
pub const GLOBALS_D_LUA: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../lua/globals.d.lua"));

/// Template for .luarc.json (LuaLS configuration)
/// Contains {types_path} placeholder for substitution
pub const LUARC_JSON_TEMPLATE: &str = r#"{
  "$schema": "https://raw.githubusercontent.com/LuaLS/vscode-lua/master/setting/schema.json",
  "runtime": {
    "version": "Lua 5.4"
  },
  "workspace": {
    "library": [
      "{types_path}"
    ],
    "checkThirdParty": false
  },
  "diagnostics": {
    "globals": ["sys"]
  },
  "completion": {
    "callSnippet": "Both",
    "keywordSnippet": "Both"
  }
}
"#;
