# Plan: Complete Type Definitions for LuaLS

## Goal

Provide comprehensive type definitions for LuaLS (Lua Language Server) to enable IDE autocompletion, type checking, and documentation.

## Problem

Only `globals.d.lua` exists. Users don't get full IDE support for the syslua API.

## Architecture Reference

- [04-lua-api.md](../architecture/04-lua-api.md):214-322 - Type definitions and LuaLS integration

## Approach

Create a complete set of type definition files covering all API surfaces:

1. `syslua.d.lua` - Global `sys` table
2. `syslua.lib.d.lua` - Library functions (env, file, user, priorities)
3. `contexts.d.lua` - BuildCtx and BindCtx
4. `modules.d.lua` - Module pattern types

## Files to Create

| Path | Purpose |
|------|---------|
| `lua/syslua/types/syslua.d.lua` | sys table types |
| `lua/syslua/types/syslua.lib.d.lua` | lib module types |
| `lua/syslua/types/contexts.d.lua` | BuildCtx/BindCtx types |
| `lua/syslua/types/modules.d.lua` | Module pattern types |

## Example: `syslua.d.lua`

```lua
---@meta

---@class Sys
---@field platform string Platform identifier (e.g., "x86_64-linux", "aarch64-darwin")
---@field os "linux"|"darwin"|"windows" Operating system
---@field arch "x86_64"|"aarch64"|"arm" CPU architecture
---@field hostname string Machine hostname
---@field username string Current user
---@field version string syslua version (e.g., "0.1.0")
---@field path PathHelpers Path utilities
---@field build fun(spec: BuildSpec): BuildRef Create a build
---@field bind fun(spec: BindSpec): BindRef Create a bind

---Global sys system information
---@type Sys
sys = {}

---@class PathHelpers
---@field join fun(...: string): string Join path segments
---@field dirname fun(path: string): string Get directory name
---@field basename fun(path: string): string Get base name
---@field extname fun(path: string): string Get extension
---@field is_absolute fun(path: string): boolean Check if path is absolute
---@field normalize fun(path: string): string Normalize path
---@field relative fun(from: string, to: string): string Get relative path
```

## Files to Modify

| Path | Changes |
|------|---------|
| `lua/syslua/globals.d.lua` | May merge into types/ or keep as entry point |
| `crates/lib/src/init/templates.rs` | Include new type files in init |

## Success Criteria

1. Full autocompletion for `sys.*` in editors
2. Type checking catches invalid API usage
3. Hover documentation shows parameter types and descriptions
4. Go-to-definition works for syslua modules
5. `.luarc.json` properly includes type definition paths

## Open Questions

- [ ] Should type files be generated from Rust types?
- [ ] How to keep Lua types in sync with Rust implementation?
- [ ] Should we use a single large file or split by module?
