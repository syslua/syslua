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
    -- Git repositories (SSH recommended for private repos)
    private = "git:git@github.com:myorg/my-dotfiles.git",

    -- Public registries via HTTPS
    community = "git:https://github.com/syslua/community-pkgs.git",

    -- Local paths for development
    local_pkgs = "path:~/code/my-packages",
}

-- Phase 2: Configure the system
-- Called after inputs are resolved; inputs table provides require paths
function M.setup(inputs)
    -- Access resolved inputs via require
    local private = require("inputs.private")
    local community = require("inputs.community")

    -- Configure packages and modules
    require("pkgs.cli.ripgrep").setup()
    private.setup_dotfiles()

    user {
        name = "alice",
        setup = function()
            -- inputs accessible via closure
            community.neovim.setup({ colorscheme = "gruvbox" })
            file { path = "~/.gitconfig", source = private.path .. "/gitconfig" }
        end,
    }
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
    require("pkgs.cli.ripgrep").setup()

    user {
        name = "alice",
        setup = function()
            file { path = "~/.bashrc", content = "# managed by syslua" }
        end,
    }
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
│  Helpers: file{}, env{}, user{}, project{}  │  ← Convenience wrappers
├─────────────────────────────────────────────┤
│  Core primitives: derive {}, activate {}    │  ← Everything builds on this
├─────────────────────────────────────────────┤
│  Contexts: DerivationCtx, ActivationCtx     │  ← Passed to config functions
├─────────────────────────────────────────────┤
│  syslua.lib: toJSON, mkDefault, mkForce     │  ← Utility functions
└─────────────────────────────────────────────┘
```

## Global Functions

### Core Primitives (Rust-backed)

| Function      | Purpose                             | See Also                           |
| ------------- | ----------------------------------- | ---------------------------------- |
| `derive {}`   | Create a derivation (build recipe)  | [Derivations](./01-derivations.md) |
| `activate {}` | Create an activation (side effects) | [Activations](./02-activations.md) |

### Convenience Helpers (Lua)

| Function     | Purpose                                       |
| ------------ | --------------------------------------------- |
| `file {}`    | Declare a managed file                        |
| `env {}`     | Declare environment variables                 |
| `user {}`    | Declare per-user scoped configuration         |
| `project {}` | Declare project-scoped environment            |
| `input ""`   | Declare an input source (registry, git, path) |

Note: There is no `service {}` or `pkg()` global. Packages and services are plain Lua modules:

```lua
require("pkgs.cli.ripgrep").setup()
require("pkgs.cli.ripgrep").setup({ version = "14.0.0" })
require("modules.services.nginx").setup({ port = 8080 })
```

### System Information

The global `syslua` table provides system information:

```lua
syslua.platform   -- "aarch64-darwin", "x86_64-linux", etc.
syslua.os         -- "darwin", "linux", "windows"
syslua.arch       -- "aarch64", "x86_64", "arm"
syslua.hostname   -- Machine hostname
syslua.username   -- Current user
syslua.is_linux   -- boolean
syslua.is_darwin  -- boolean
syslua.is_windows -- boolean
syslua.version    -- sys.lua version string
```

## Library Functions (`syslua.lib`)

```lua
local lib = require("syslua.lib")

-- JSON conversion
lib.toJSON(table)           -- Convert Lua table to JSON string

-- Priority functions for conflict resolution
lib.mkDefault(value)        -- Priority 1000 (can be overridden)
lib.mkForce(value)          -- Priority 50 (forces value)
lib.mkBefore(value)         -- Priority 500 (prepend to mergeable)
lib.mkAfter(value)          -- Priority 1500 (append to mergeable)
lib.mkOverride(priority, value)  -- Explicit priority
lib.mkOrder(priority, value)     -- Alias for mkOverride

-- Environment variable definitions
lib.env.defineMergeable(var_name)  -- PATH-like variables
lib.env.defineSingular(var_name)   -- Single-value variables
```

## Lua Language Server (LuaLS) Integration

sys.lua provides excellent IDE/editor support through type definition files and automatic workspace configuration.

### Goals

- Autocomplete for global functions (`pkg`, `file`, `env`, `user`, `project`, `input`, etc.)
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
│   │   ├── globals.d.lua        # derive{}, activate{}, file{}, env{}, etc.
│   │   ├── inputs.d.lua         # input() function
│   │   ├── modules.d.lua        # Module system types
│   │   └── sops.d.lua           # SOPS integration
│   └── init.lua                 # Actual runtime implementations
```

### Example Type Definitions

**`lib/types/syslua.d.lua`:**

```lua
---@meta

---@class Syslua
---@field platform string Platform identifier (e.g., "x86_64-linux", "aarch64-darwin")
---@field os "linux"|"darwin"|"windows" Operating system
---@field arch "x86_64"|"aarch64"|"arm" CPU architecture
---@field hostname string Machine hostname
---@field username string Current user
---@field is_linux boolean True if running on Linux
---@field is_darwin boolean True if running on macOS
---@field is_windows boolean True if running on Windows
---@field version string sys.lua version (e.g., "0.1.0")

---Global syslua system information
---@type Syslua
syslua = {}
```

**`lib/types/globals.d.lua`:**

```lua
---@meta

---@class Derivation
---@field name string Derivation name
---@field version? string Version string
---@field hash string Content-addressed hash
---@field out string Store path (after realization)
---@field outputs table<string, string> All output paths

---@class DeriveSpec
---@field name string Required: derivation name
---@field version? string Optional: version string
---@field outputs? string[] Optional: output names (default: {"out"})
---@field opts? table|fun(sys: System): table Optional: input data
---@field config fun(opts: table, ctx: DerivationCtx): nil Required: build logic

---Create a derivation (returns Derivation and registers globally)
---@param spec DeriveSpec
---@return Derivation
function derive(spec) end

---@class ActivateSpec
---@field opts? table|fun(sys: System): table Optional: input data
---@field config fun(opts: table, ctx: ActivationCtx): nil Required: activation logic

---Create an activation (registers globally)
---@param spec ActivateSpec
function activate(spec) end

---@class FileSpec
---@field path string Target path (~ expanded)
---@field source? string Source file/directory
---@field content? string Inline content
---@field mutable? boolean Direct symlink (default: false)
---@field mode? integer Unix file permissions

---Declare a file to be managed by sys.lua
---@param spec FileSpec
function file(spec) end

---@alias EnvValue string|string[]|table

---Declare environment variables
---@param vars table<string, EnvValue>
function env(vars) end

---@class UserSpec
---@field name string Username
---@field setup fun() User setup function

---Declare per-user configuration
---@param spec UserSpec
function user(spec) end
```

**`lib/types/syslua.lib.d.lua`:**

```lua
---@meta

---@class SysluaLib
local lib = {}

---Convert a Lua table to JSON string
---@param value table
---@return string
function lib.toJSON(value) end

---Set default priority (1000) - can be overridden
---@generic T
---@param value T
---@return T
function lib.mkDefault(value) end

---Set highest priority (50) - forces value
---@generic T
---@param value T
---@return T
function lib.mkForce(value) end

---Prepend to mergeable values (priority 500)
---@generic T
---@param value T
---@return T
function lib.mkBefore(value) end

---Append to mergeable values (priority 1500)
---@generic T
---@param value T
---@return T
function lib.mkAfter(value) end

return lib
```

### Workspace Configuration

sys.lua automatically generates a `.luarc.json` file when you run `sys apply`. This tells LuaLS where to find type definitions:

```json
{
  "runtime": {
    "version": "Lua 5.4"
  },
  "workspace": {
    "library": [
      "/syslua/store/pkg/syslua/0.1.0/share/lua/types",
      "~/.local/share/syslua/types"
    ],
    "checkThirdParty": false
  },
  "diagnostics": {
    "globals": [
      "syslua",
      "derive",
      "activate",
      "file",
      "env",
      "user",
      "project",
      "input",
      "sops"
    ]
  },
  "completion": {
    "callSnippet": "Both",
    "keywordSnippet": "Both"
  }
}
```

### Package Annotations

Registry packages should include type annotations for their options:

```lua
-- pkgs/neovim/0.10.0.lua

---@class NeovimOpts
---@field url string Download URL for prebuilt binary
---@field sha256 string Content hash

local hashes = {
  ["aarch64-darwin"] = "abc...",
  ["x86_64-linux"] = "def...",
}

local M = {}

M.name = "neovim"
M.version = "0.10.0"

M.derivation = derive {
  name = M.name,
  version = M.version,

  ---@param sys System
  ---@return NeovimOpts
  opts = function(sys)
    return {
      url = "https://github.com/neovim/neovim/releases/download/v0.10.0/nvim-" .. sys.platform .. ".tar.gz",
      sha256 = hashes[sys.platform],
    }
  end,

  ---@param opts NeovimOpts
  ---@param ctx DerivationCtx
  config = function(opts, ctx)
    local archive = ctx.fetch_url(opts.url, opts.sha256)
    ctx.unpack(archive, ctx.out)
  end,
}

return M
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

env {
    EDITOR = { "nvim" },  -- ✗ Runtime error: singular env var must be string
}

file {
    path = "~/.gitconfig",
    content = "...",
    source = "...",  -- ✗ Runtime error: cannot specify both content and source
}
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

The `config` property appears in two contexts:

### In `derive {}` and `activate {}`

The `config` function receives resolved options and a context object:

```lua
derive {
    name = "ripgrep",
    opts = function(sys) return { url = "...", sha256 = "..." } end,
    config = function(opts, ctx)
        -- ctx provides build operations
        local archive = ctx.fetch_url(opts.url, opts.sha256)
        ctx.unpack(archive, ctx.out)
    end,
}
```

### In `user {}` and `project {}`

The `config` function provides scoping for declarations:

```lua
user {
    name = "alice",
    setup = function()
        require("pkgs.cli.neovim").setup()
        file { path = "~/.gitconfig", content = "..." }
        env { EDITOR = "nvim" }
    end,
}

project {
    name = "my-app",
    setup = function()
        require("pkgs.runtime.nodejs").setup({ version = "20" })
        env { NODE_ENV = "development" }
    end,
}
```

## See Also

- [Derivations](./01-derivations.md) - How `derive {}` works
- [Activations](./02-activations.md) - How `activate {}` works
- [Modules](./07-modules.md) - Module system and composition
