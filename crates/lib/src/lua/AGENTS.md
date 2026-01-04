# Agent Guidelines for syslua-lib/lua

**Generated:** 2026-01-04 | **Commit:** bc66463 | **Branch:** main

## OVERVIEW
Lua runtime and API for configuration evaluation using mlua (Lua 5.4 vendored).

## FILES
- `mod.rs`: Module entry point; exports runtime, globals, and entrypoint.
- `runtime.rs`: Lua VM lifecycle, `create_runtime`, `load_file`. Inits `package.path`.
- `entrypoint.rs`: Initial config loading; parses `inputs` table from `init.lua`.
- `globals.rs`: Registers `sys` global table (os, arch, build, bind, path).
- `helpers/`: Utility modules (e.g., `path.rs`) and type conversion logic.

## LUA API
- `sys.build{ id, inputs, create }`: Defines immutable content for the store.
- `sys.bind{ id, inputs, create, update, destroy }`: Defines system side effects.
- `sys.os`, `sys.arch`, `sys.platform`: Target platform metadata.
- `sys.path`: Cross-platform path utilities (join, dirname, canonicalize).
- `sys.register_{build,bind}_ctx_method()`: Extends `ctx` with custom methods.

## TYPE CONVERSION
- **FromLua/IntoLua**: Maps Lua tables to Rust structs (BuildDef, BindDef, InputDecl).
- **Determinism**: Tables converted to `BTreeMap` for stable manifest hashes.
- **Spec/Def duality**: `Spec` holds Lua closures; `Def` holds serializable data.
- **Context**: `BuildCtx`/`BindCtx` exposed as UserData to record actions.

## GOTCHAS
- `sys.dir`: Relative to the currently executing Lua file (set during `load_file`).
- **Sandbox**: No direct access to `io.*` or `os.*`; must use `ctx:exec()`.
- **Built-ins**: Built-in `ctx` methods (exec, out, fetch_url) cannot be overridden.
- **Search Path**: `package.path` includes `./lua/?.lua` for module resolution.
