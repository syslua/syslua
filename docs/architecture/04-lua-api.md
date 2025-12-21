# Lua API

> Part of the [sys.lua Architecture](./00-overview.md) documentation.

This document covers the Lua API layers, globals, type definitions, and IDE integration.

## Core Value: Standard Lua Idioms

The API follows standard Lua patterns:

- `require()` returns a table
- `setup(opts)` is a function call that does the work
- No auto-evaluation, no hidden behavior
- Plain tables for configuration

## Entry Point Pattern

The entry point file (`init.lua`) follows a two-phase evaluation pattern using `M.inputs` and `M.setup(inputs)`:

```lua
-- ~/.config/syslua/init.lua
local M = {}

-- Phase 1: Declare external inputs (optional)
-- syslua reads this table first and resolves all sources
M.inputs = {
  -- Public registries via HTTPS
  syslua = 'git:https://github.com/spirit-led-software/syslua',

  -- Git repositories (SSH recommended for private repos)
  private = 'git:git@github.com:myorg/my-dotfiles.git',

  -- Local paths for development
  local_pkgs = 'path:~/code/my-packages',
}

-- Phase 2: Configure the system
-- Called after inputs are resolved; inputs table provides require paths
function M.setup(inputs)
  -- Access resolved inputs via require
  local private = require('private') -- reads init.lua at root of private repo
  local syslua = require('syslua')
  local modules = require('syslua.modules')

  -- Configure packages and modules
  require('syslua.pkgs.cli.ripgrep').setup()
  private.setup_dotfiles()

  modules.user.setup({
    name = 'alice',
    setup = function()
      -- inputs accessible via closure
      require('syslua.pkgs.editors.neovim').setup({ colorscheme = 'gruvbox' })
      modules.file.setup({ path = '~/.gitconfig', source = inputs.private.path .. '/gitconfig' })
    end,
  })
end

return M
```

### Two-Phase Evaluation

1. **Phase 1 (Input Resolution)**: syslua loads `init.lua`, reads `M.inputs`, and resolves all external sources (cloning git repos, validating paths). This happens before any configuration runs.

2. **Phase 2 (Configuration)**: syslua calls `M.setup(inputs)` with the resolved inputs table. The `inputs` parameter provides metadata, but you access input modules via `require("inputs.<name>")`.

### Contract

- Entry point **must** return a table with a `setup` function
- Entry point **may** include an `inputs` table (optional if no external dependencies)
- `setup` receives the resolved inputs metadata table
- syslua errors if `init.lua` doesn't return a valid table with `setup`

### Minimal Entry Point (No Inputs)

If you don't need external inputs, the entry point is simpler:

```lua
local M = {}

-- M.inputs omitted - no external dependencies

function M.setup()
  sys.build({
    id = "my-tool",
    create = function(inputs, ctx)
      return { out = ctx.out }
    end,
  })
end

return M
```

### Why This Pattern?

- **Explicit dependencies**: All external sources declared upfront in `M.inputs`
- **Deterministic resolution**: Inputs resolved before config evaluation prevents ordering issues
- **SSH-first auth**: Git SSH URLs use existing `~/.ssh/` keys—no token management
- **Standard Lua**: Follows the same `local M = {} ... return M` pattern as modules
- **IDE friendly**: LuaLS can analyze the structure and provide completions

## API Layers

```
┌─────────────────────────────────────────────┐
│  Modules: require("...").setup({})          │  ← Packages, services, programs
├─────────────────────────────────────────────┤
│  Core primitives: sys.build {}, sys.bind {} │  ← Everything builds on this
├─────────────────────────────────────────────┤
│  Contexts: ActionCtx                        │  ← Passed to create/update/destroy
├─────────────────────────────────────────────┤
│  Helpers: sys.path, toJSON                  │  ← Utility functions
└─────────────────────────────────────────────┘
```

## Global Functions

### Core Primitives (Rust-backed)

| Function     | Purpose                             | See Also                     |
| ------------ | ----------------------------------- | ---------------------------- |
| `sys.build()` | Create a build (build recipe)      | [Builds](./01-builds.md)     |
| `sys.bind()`  | Create a bind (side effects)       | [Binds](./02-binds.md)       |
| `input()`    | Declare an input source             | [Inputs](./06-inputs.md)     |

### Custom ActionCtx Methods

`sys.register_ctx_method()` allows Lua libraries to extend `ActionCtx` with custom methods that compose existing primitives. This enables higher-level abstractions while keeping actions properly recorded.

```lua
-- Register a cross-platform mkdir helper
sys.register_ctx_method("mkdir", function(ctx, path)
  if sys.os == "windows" then
    return ctx:exec({ bin = "cmd.exe", args = { "/c", "mkdir", path } })
  else
    return ctx:exec({ bin = "/bin/mkdir", args = { "-p", path } })
  end
end)

-- Now available on any ActionCtx:
sys.build({
  id = "my-tool",
  create = function(inputs, ctx)
    ctx:mkdir(ctx.out .. "/bin")  -- Uses the registered method
    return { out = ctx.out }
  end,
})
```

| Function | Purpose |
| -------- | ------- |
| `sys.register_ctx_method(name, fn)` | Register a custom method on `ActionCtx` |
| `sys.unregister_ctx_method(name)` | Remove a previously registered method |

**Rules:**
- Built-in methods (`exec`, `fetch_url`, `write_file`, `out`) cannot be overridden
- Registered methods receive `(ctx, ...)` when called with `:` syntax
- Actions called within registered methods are recorded normally
- Registration is global—methods are available to all subsequent builds/binds
- Unknown method calls produce helpful error messages suggesting `sys.register_ctx_method`

### Convenience Helpers (Lua, via `require('syslua.modules')`)

| Function              | Purpose                               |
| --------------------- | ------------------------------------- |
| `modules.file.setup()`    | Declare a managed file                |
| `modules.env.setup()`     | Declare environment variables         |
| `modules.user.setup()`    | Declare per-user scoped configuration |
| `modules.project.setup()` | Declare project-scoped environment    |

> **Note:** These helpers are accessed via `local modules = require('syslua.modules')`, not as globals.

Note: There is no `service()` or `pkg()` global. Packages and services are plain Lua modules:

```lua
require('syslua.pkgs.cli.ripgrep').setup()
require('syslua.pkgs.cli.ripgrep').setup({ version = '14.0.0' })
require('syslua.modules.services.nginx').setup({ port = 8080 })
```

### System Information

The global `sys` table provides system information:

```lua
sys.platform   -- "aarch64-darwin", "x86_64-linux", etc.
sys.os         -- "darwin", "linux", "windows"
sys.arch       -- "aarch64", "x86_64", "i386"
```

### Path Utilities

The `sys.path` table provides cross-platform path helpers:

```lua
sys.path.resolve(...)        -- Resolve to absolute path
sys.path.join(...)           -- Join path segments
sys.path.dirname(path)       -- Get directory name
sys.path.basename(path)      -- Get file name
sys.path.extname(path)       -- Get file extension
sys.path.is_absolute(path)   -- Check if path is absolute
sys.path.normalize(path)     -- Normalize path (resolve . and ..)
sys.path.relative(from, to)  -- Get relative path
sys.path.split(path)         -- Split into components
```

## Library Functions

> **Future Feature:** The priority system (`mkDefault`, `mkForce`, etc.) is documented but not yet implemented.

```lua
local lib = require('syslua.lib')

-- JSON conversion
lib.toJSON(table) -- Convert Lua table to JSON string

-- FUTURE: Priority functions for conflict resolution (not yet implemented)
-- lib.mkDefault(value) -- Priority 1000 (can be overridden)
-- lib.mkForce(value) -- Priority 50 (forces value)
-- lib.mkBefore(value) -- Priority 500 (prepend to mergeable)
-- lib.mkAfter(value) -- Priority 1500 (append to mergeable)
-- lib.mkOverride(priority, value) -- Explicit priority
-- lib.mkOrder(priority, value) -- Alias for mkOverride

-- FUTURE: Environment variable definitions (not yet implemented)
-- lib.env.defineMergeable(var_name) -- PATH-like variables
-- lib.env.defineSingular(var_name) -- Single-value variables
```

## Lua Language Server (LuaLS) Integration

sys.lua provides excellent IDE/editor support through type definition files and automatic workspace configuration.

### Goals

- Autocomplete for global functions
- Type checking for all API parameters
- Hover documentation for functions and options
- Go-to-definition for modules and package references
- Diagnostics for invalid configurations
- Zero configuration required - works out of the box

### Type Definition Files

sys.lua ships with comprehensive type definitions:

```
syslua/
├── lua/
│   └── syslua/
│       └── globals.d.lua     # All type definitions
```

### Example Type Definitions

**`globals.d.lua`:**

```lua
---@meta

---@class ExecOpts
---@field bin string Path to binary/executable to run
---@field args? string[] Optional: arguments to pass to the binary
---@field env? table<string,string> Optional: environment variables
---@field cwd? string Optional: working directory

---@class ActionCtx
---@field out string Returns the store path placeholder
---@field fetch_url fun(self: ActionCtx, url: string, sha256: string): string Fetches a URL and returns store path
---@field write_file fun(self: ActionCtx, path: string, content: string): string Writes content to a file, returns path
---@field exec fun(self: ActionCtx, opts: string | ExecOpts, args?: string[]): string Executes a command, returns stdout

---@class BuildRef
---@field id? string Build id
---@field inputs? table All inputs to the build
---@field outputs table All outputs from the build
---@field hash string Content-addressed hash

---@class BuildSpec
---@field id? string Optional: build id for debugging/logging
---@field inputs? table|fun(): table Optional: input data
---@field create fun(inputs: table, ctx: ActionCtx): table Required: build logic, returns outputs

---@class BindRef
---@field id? string Binding id
---@field inputs? table All inputs to the binding
---@field outputs? table All outputs from the binding
---@field hash string Hash for deduplication

---@class BindSpec
---@field id? string Binding id. Required when providing update method
---@field inputs? table|fun(): table Optional: input data
---@field create fun(inputs: table, ctx: ActionCtx): table|nil Required: binding logic, optionally returns outputs
---@field update? fun(outputs: table, inputs: table, ctx: ActionCtx): table|nil Optional: update logic
---@field destroy? fun(outputs: table, ctx: ActionCtx): nil Optional: cleanup logic, receives outputs

---@class Sys
---@field platform Platform Active platform
---@field os Os Operating system name
---@field arch Arch System architecture
---@field path PathHelpers File path utilities
---@field build fun(spec: BuildSpec): BuildRef Creates a build within the store
---@field bind fun(spec: BindSpec): BindRef Creates a binding to the active system
---@field register_ctx_method fun(name: string, fn: fun(ctx: ActionCtx, ...: any): any)
---@field unregister_ctx_method fun(name: string)

---@type Sys
sys = {}
```

### Workspace Configuration

sys.lua automatically generates a `.luarc.json` file when you run `sys apply`. This tells LuaLS where to find type definitions:

```json
{
  "runtime": {
    "version": "Lua 5.4"
  },
  "workspace": {
    "library": ["/syslua/types", "~/.local/share/syslua/types"],
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
```

### Editor Setup

**VS Code:**

```bash
# Install Lua Language Server extension
code --install-extension sumneko.lua

# sys.lua automatically generates .luarc.json on first apply
sys apply ~/.config/syslua/
```

**Neovim (with nvim-lspconfig):**

```lua
-- ~/.config/nvim/init.lua
require('lspconfig').lua_ls.setup({
  -- LuaLS will automatically read .luarc.json
  on_attach = function(client, bufnr)
    -- Your LSP keymaps here
  end,
})
```

**Emacs (with lsp-mode):**

```elisp
;; LuaLS will automatically read .luarc.json
(use-package lsp-mode
  :hook ((lua-mode . lsp)))
```

**CLI Command:**

```bash
# Generate .luarc.json without applying config
$ sys init
Generated .luarc.json for Lua Language Server integration
Generated sys.lua template

# Force regenerate .luarc.json
$ sys init --force
Regenerated .luarc.json
```

## Runtime Type Checking

In addition to LSP-based type checking, sys.lua performs runtime validation during config evaluation:

```lua
local modules = require('syslua.modules')

-- Invalid config examples
modules.env.setup({
  EDITOR = { 'nvim' }, -- ✗ Runtime error: singular env var must be string
})

modules.file.setup({
  path = '~/.gitconfig',
  content = '...',
  source = '...', -- ✗ Runtime error: cannot specify both content and source
})
```

**Error messages include:**

- Field name and type mismatch
- Expected vs actual type
- Line number in config file (when available)
- Suggestion for fixing the error

**Example error output:**

```
Error evaluating sys.lua:
  Line 15: Invalid value for package version
    Expected: string
    Got: number (123)

  Suggestion: setup({ version = "0.10.0" })
```

## The `create` Function Pattern

### In `sys.build {}` and `sys.bind {}`

The `create` function receives resolved inputs and an ActionCtx:

```lua
sys.build({
  id = 'ripgrep',
  inputs = function()
    return { url = '...', sha256 = '...' }
  end,
  create = function(inputs, ctx)
    -- ctx provides build operations
    local archive = ctx:fetch_url(inputs.url, inputs.sha256)
    ctx:exec({ bin = 'tar', args = { '-xzf', archive, '-C', ctx.out } })
    return { out = ctx.out }
  end,
})
```

### ActionCtx Methods

| Method | Description | Returns |
|--------|-------------|---------|
| `ctx.out` | Property returning the build's output directory placeholder | string |
| `ctx:fetch_url(url, sha256)` | Download file with hash verification | opaque path reference |
| `ctx:write_file(path, contents)` | Write contents to a file | opaque path reference |
| `ctx:exec(opts)` | Execute a command | opaque stdout reference |

## See Also

- [Builds](./01-builds.md) - How `sys.build {}` works
- [Binds](./02-binds.md) - How `sys.bind {}` works
- [Modules](./07-modules.md) - Module system and composition
