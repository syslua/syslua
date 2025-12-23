# Lua API

> Part of the [SysLua Architecture](./00-overview.md) documentation.

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
    id = 'my-tool',
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

| Function      | Purpose                       | See Also                 |
| ------------- | ----------------------------- | ------------------------ |
| `sys.build()` | Create a build (build recipe) | [Builds](./01-builds.md) |
| `sys.bind()`  | Create a bind (side effects)  | [Binds](./02-binds.md)   |

### Custom Context Methods

`sys.register_build_ctx_method()` `sys.register_bind_ctx_method()` allows Lua libraries to extend `BuildCtx` and `BindCtx` with custom methods that compose existing primitives. This enables higher-level abstractions while keeping actions properly recorded.

```lua
-- Register a cross-platform mkdir helper
sys.register_build_ctx_method('mkdir', function(ctx, path)
  if sys.os == 'windows' then
    return ctx:exec({ bin = 'cmd.exe', args = { '/c', 'mkdir', path } })
  else
    return ctx:exec({ bin = '/bin/mkdir', args = { '-p', path } })
  end
end)

-- Now available on any BuildCtx:
sys.build({
  id = 'my-tool',
  create = function(inputs, ctx)
    ctx:mkdir(ctx.out .. '/bin') -- Uses the registered method
    return { out = ctx.out }
  end,
})
```

| Function                                  | Purpose                                |
| ----------------------------------------- | -------------------------------------- |
| `sys.register_build_ctx_method(name, fn)` | Register a custom method on `BuildCtx` |
| `sys.register_bind_ctx_method(name, fn)`  | Register a custom method on `BindCtx`  |

**Rules:**

- Built-in methods (`exec`, `fetch_url`, `out`) cannot be overridden
- Registered methods receive `(ctx, ...)` when called with `:` syntax
- Actions called within registered methods are recorded normally
- Registration is global—methods are available to all subsequent builds/binds
- Unknown method calls produce helpful error messages suggesting `sys.register_ctx_method`

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
sys.path.resolve(...) -- Resolve to absolute path
sys.path.join(...) -- Join path segments
sys.path.dirname(path) -- Get directory name
sys.path.basename(path) -- Get file name
sys.path.extname(path) -- Get file extension
sys.path.is_absolute(path) -- Check if path is absolute
sys.path.normalize(path) -- Normalize path (resolve . and ..)
sys.path.relative(from, to) -- Get relative path
sys.path.split(path) -- Split into components
```

## Lua Language Server (LuaLS) Integration

SysLua provides excellent IDE/editor support through type definition files and automatic workspace configuration.

### Goals

- Autocomplete for global functions
- Type checking for all API parameters
- Hover documentation for functions and options
- Go-to-definition for modules and package references
- Diagnostics for invalid configurations
- Zero configuration required - works out of the box

### Type Definition Files

SysLua ships with comprehensive type definitions:

```
syslua/
├── lua/
│   └── syslua/
│       └── globals.d.lua     # All type definitions
```

### Workspace Configuration

SysLua automatically generates a `.luarc.json` file when you run `sys apply`. This tells LuaLS where to find type definitions:

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

# SysLua automatically generates .luarc.json on first apply
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
Generated SysLua template

# Force regenerate .luarc.json
$ sys init --force
Regenerated .luarc.json
```

## Runtime Type Checking

In addition to LSP-based type checking, SysLua performs runtime validation during config evaluation:

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
Error evaluating SysLua:
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

### BuildCtx Methods

| Method                       | Description                                                 | Returns                 |
| ---------------------------- | ----------------------------------------------------------- | ----------------------- |
| `ctx.out`                    | Property returning the build's output directory placeholder | string                  |
| `ctx:fetch_url(url, sha256)` | Download file with hash verification                        | opaque path reference   |
| `ctx:exec(opts)`             | Execute a command                                           | opaque stdout reference |

### BindCtx Methods

| Method           | Description                                                 | Returns                 |
| ---------------- | ----------------------------------------------------------- | ----------------------- |
| `ctx.out`        | Property returning the binds's output directory placeholder | string                  |
| `ctx:exec(opts)` | Execute a command                                           | opaque stdout reference |

## See Also

- [Builds](./01-builds.md) - How `sys.build {}` works
- [Binds](./02-binds.md) - How `sys.bind {}` works
- [Modules](./07-modules.md) - Module system and composition
