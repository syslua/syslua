# Plan: `lib.env.setup()` and Environment Script Generation

## Goal

Implement the `lib.env.setup()` Lua helper and generate shell environment scripts during apply.

## Problem

Users have no declarative way to manage environment variables. The architecture describes generating `env.sh`, `env.fish`, and `env.ps1` scripts, but this isn't implemented.

## Architecture Reference

- [04-lua-api.md](../architecture/04-lua-api.md):165-210 - lib.env functions
- [09-platform.md](../architecture/09-platform.md):35-115 - Environment scripts and persistent variables

## Approach

### Phase 1: Basic env.setup()

1. Create `lua/syslua/lib/env.lua` module
2. Implement `env.setup({ VAR = "value" })` that creates a build + bind
3. Build generates shell-specific fragments
4. Bind registers the environment for script generation

### Phase 2: Script Generation

1. During apply, collect all env binds
2. Generate `~/.local/share/syslua/env.sh` (bash/zsh)
3. Generate `~/.local/share/syslua/env.fish` (fish)
4. Generate `~/.local/share/syslua/env.ps1` (PowerShell)

### Phase 3: Mergeable Variables

1. Implement `lib.env.defineMergeable("PATH")` for PATH-like variables
2. Implement `lib.env.defineSingular("EDITOR")` for single-value variables
3. Handle priority-based merging

## Lua API

```lua
local lib = require("syslua.lib")

-- Simple usage
lib.env.setup({
    EDITOR = "nvim",
    PAGER = "less",
})

-- With PATH additions
lib.env.setup({
    PATH = lib.mkBefore("/custom/bin"),  -- Prepend to PATH
})
```

## Files to Create

| Path | Purpose |
|------|---------|
| `lua/syslua/lib/env.lua` | Environment module implementation |

## Files to Modify

| Path | Changes |
|------|---------|
| `lua/syslua/lib/init.lua` | Export env module |
| `crates/lib/src/execute/apply.rs` | Generate env scripts after apply |

## Success Criteria

1. `lib.env.setup()` works in Lua configs
2. Environment scripts are generated during apply
3. Scripts are shell-specific (sh, fish, ps1)
4. PATH and other mergeable variables combine correctly
5. User can source the script from their shell rc file

## Open Questions

- [ ] How to handle per-user environments?
- [ ] Should we automatically detect installed shells?
- [ ] How to handle persistent (system-level) environment variables?
- [ ] Integration with package PATH additions?
