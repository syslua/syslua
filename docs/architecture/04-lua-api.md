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
  local lib = syslua.lib

  -- Configure packages and modules
  require('syslua.pkgs.cli.ripgrep').setup()
  private.setup_dotfiles()

  lib.user.setup({
    name = 'alice',
    setup = function()
      -- inputs accessible via closure
      syslua.pkgs.editors.neovim.setup({ colorscheme = 'gruvbox' })
      lib.file.setup({ path = '~/.gitconfig', source = inputs.private.path .. '/gitconfig' })
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
    ...,
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
│  Contexts: BuildCtx, BindCtx                │  ← Passed to config functions
├─────────────────────────────────────────────┤
│  syslua.lib: toJSON, mkDefault, mkForce     │  ← Utility functions
└─────────────────────────────────────────────┘
```

## Global Functions

### Core Primitives (Rust-backed)

| Function     | Purpose                             | See Also                     |
| ------------ | ----------------------------------- | ---------------------------- |
| `sys.build()` | Create a build (build recipe)      | [Builds](./01-builds.md)     |
| `sys.bind()`  | Create a bind (side effects)       | [Binds](./02-binds.md)       |
| `input()`    | Declare an input source             | [Inputs](./06-inputs.md)     |

### Convenience Helpers (Lua, provided by syslua input source)

| Function              | Purpose                               |
| --------------------- | ------------------------------------- |
| `lib.file.setup()`    | Declare a managed file                |
| `lib.env.setup()`     | Declare environment variables         |
| `lib.user.setup()`    | Declare per-user scoped configuration |
| `lib.project.setup()` | Declare project-scoped environment    |

Note: There is no `service()` or `pkg()` global. Packages and services are plain Lua modules:

```lua
require('pkgs.cli.ripgrep').setup()
require('pkgs.cli.ripgrep').setup({ version = '14.0.0' })
require('modules.services.nginx').setup({ port = 8080 })
```

### System Information

The global `syslua` table provides system information:

```lua
syslua.platform   -- "aarch64-darwin", "x86_64-linux", etc.
syslua.os         -- "darwin", "linux", "windows"
syslua.arch       -- "aarch64", "x86_64", "arm"
syslua.hostname   -- Machine hostname
syslua.username   -- Current user
syslua.version    -- sys.lua version string
```

## Library Functions (`syslua.lib`)

```lua
local lib = require('syslua.lib')

-- JSON conversion
lib.toJSON(table) -- Convert Lua table to JSON string

-- Priority functions for conflict resolution
lib.mkDefault(value) -- Priority 1000 (can be overridden)
lib.mkForce(value) -- Priority 50 (forces value)
lib.mkBefore(value) -- Priority 500 (prepend to mergeable)
lib.mkAfter(value) -- Priority 1500 (append to mergeable)
lib.mkOverride(priority, value) -- Explicit priority
lib.mkOrder(priority, value) -- Alias for mkOverride

-- Environment variable definitions
lib.env.defineMergeable(var_name) -- PATH-like variables
lib.env.defineSingular(var_name) -- Single-value variables
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

sys.lua ships with comprehensive type definitions in `lib/types/`:

```
syslua/
├── lib/
│   ├── types/
│   │   ├── syslua.d.lua         # Global syslua table
│   │   ├── syslua.lib.d.lua     # syslua.lib module
│   │   ├── globals.d.lua        # derive(), activate(), input()
│   │   ├── modules.d.lua        # Module system types
│   └── init.lua                 # Actual runtime implementations
```

### Example Type Definitions

**`lib/types/syslua.d.lua`:**

```lua
---@meta

---@class Sys
---@field platform string Platform identifier (e.g., "x86_64-linux", "aarch64-darwin")
---@field os "linux"|"darwin"|"windows" Operating system
---@field arch "x86_64"|"aarch64"|"arm" CPU architecture
---@field hostname string Machine hostname
---@field username string Current user
---@field version string sys.lua version (e.g., "0.1.0")
---@field path PathHelpers Path utilities
---@field build fun(spec: BuildSpec): BuildRef Create a build
---@field bind fun(spec: BindSpec): BindRef Create a bind

---Global sys system information
---@type Sys
sys = {}
```

**`lib/types/globals.d.lua`:**

```lua
---@meta

---@class BuildRef
---@field name string Build name
---@field version? string Version string
---@field hash string Content-addressed hash
---@field outputs table<string, string> All output paths

---@class BuildSpec
---@field name string Required: build name
---@field version? string Optional: version string
---@field outputs? table<string,string> Optional: output names (default: {out="out"})
---@field inputs? table|fun(): table Optional: input data
---@field config fun(inputs: table, ctx: BuildCtx): nil Required: build logic

---@class BuildCtx
---@field outputs table<string, string> Output paths
---@field fetch_url fun(self: BuildCtx, url: string, sha256: string): string
---@field cmd fun(self: BuildCtx, opts: BuildCmdOptions|string): nil

---@class BuildCmdOptions
---@field cmd string Command to execute
---@field env? table<string,string> Environment variables
---@field cwd? string Working directory

---@class BindSpec
---@field inputs? table|fun(): table Optional: input data
---@field apply fun(inputs: table, ctx: BindCtx): table? Required: apply logic, can return outputs
---@field destroy? fun(inputs: table, ctx: BindCtx): nil Optional: destroy logic for rollback

---@class BindCtx
---@field cmd fun(self: BindCtx, opts: BindCmdOptions|string): string Returns "${action:N}" placeholder

---@class BindCmdOptions
---@field cmd string Command to execute
---@field env? table<string,string> Environment variables
---@field cwd? string Working directory
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
    "globals": ["sys", "input"]
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
-- Invalid config examples
local lib = require('syslua.lib')

lib.env.setup({
  EDITOR = { 'nvim' }, -- ✗ Runtime error: singular env var must be string
})

lib.file.setup({
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

## The `config` Property Pattern

### In `sys.build {}` and `sys.bind {}`

The `config` function receives resolved options and a context object:

```lua
sys.build({
  name = 'ripgrep',
  inputs = function()
    return { url = '...', sha256 = '...' }
  end,
  apply = function(inputs, ctx)
    -- ctx provides build operations
    local archive = ctx:fetch_url(inputs.url, inputs.sha256)
    ctx:cmd({ cmd = 'tar -xzf ' .. archive .. ' -C ' .. ctx.outputs.out })
  end,
})
```

## See Also

- [Builds](./01-builds.md) - How `sys.build {}` works
- [Binds](./02-binds.md) - How `sys.bind {}` works
- [Modules](./07-modules.md) - Module system and composition
