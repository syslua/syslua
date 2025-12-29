# Plan: Package Modules

## Goal

Create a set of example package modules that users can install via `require("syslua.pkgs.*").setup()`.

## Problem

The `lua/syslua/pkgs/` namespace exists but contains no actual packages. Users have no examples of how to create package modules.

## Architecture Reference

- [07-modules.md](../architecture/07-modules.md):102-152 - Package module pattern
- [01-builds.md](../architecture/01-builds.md):271-370 - Build examples

## Approach

### Phase 1: CLI Tools (Prebuilt Binaries)

Start with popular CLI tools that provide prebuilt binaries:

1. `ripgrep` - Fast grep alternative
2. `fd` - Fast find alternative  
3. `bat` - Cat with syntax highlighting
4. `jq` - JSON processor
5. `fzf` - Fuzzy finder

### Phase 2: Editors

1. `neovim` - Modern vim
2. `helix` - Modal editor

### Phase 3: Runtimes

1. `nodejs` - Node.js runtime
2. `deno` - Deno runtime

## Package Module Structure

```lua
-- lua/syslua/pkgs/cli/ripgrep.lua
local M = {}

M.options = {
    version = "14.1.0",
}

local hashes = {
    ["aarch64-darwin"] = "sha256:...",
    ["x86_64-linux"] = "sha256:...",
    ["x86_64-windows"] = "sha256:...",
}

function M.setup(opts)
    opts = opts or {}
    local version = opts.version or M.options.version
    
    local build = sys.build({
        name = "ripgrep",
        version = version,
        inputs = function()
            return {
                url = "https://github.com/BurntSushi/ripgrep/releases/...",
                sha256 = hashes[sys.platform],
            }
        end,
        apply = function(inputs, ctx)
            local archive = ctx:fetch_url(inputs.url, inputs.sha256)
            ctx:exec("tar -xzf " .. archive .. " -C " .. ctx.out)
            return { out = ctx.out }
        end,
    })
    
    sys.bind({
        inputs = function() return { build = build } end,
        apply = function(inputs, ctx)
            -- Add to PATH via symlink or shell integration
        end,
        destroy = function(inputs, ctx)
            -- Remove from PATH
        end,
    })
    
    return M
end

return M
```

## Files to Create

| Path | Purpose |
|------|---------|
| `lua/syslua/pkgs/cli/ripgrep.lua` | ripgrep package |
| `lua/syslua/pkgs/cli/fd.lua` | fd package |
| `lua/syslua/pkgs/cli/bat.lua` | bat package |
| `lua/syslua/pkgs/cli/jq.lua` | jq package |
| `lua/syslua/pkgs/cli/fzf.lua` | fzf package |
| `lua/syslua/pkgs/cli/init.lua` | CLI namespace |

## Success Criteria

1. At least 3 CLI packages work on all platforms
2. Packages follow the module pattern from architecture
3. Version can be overridden via options
4. Packages properly integrate with PATH
5. Documentation/examples for creating new packages

## Open Questions

- [ ] Where to get SHA256 hashes for releases?
- [ ] How to handle packages without Windows builds?
- [ ] Should there be a package registry/index?
- [ ] How to handle package dependencies?
