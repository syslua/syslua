# Plan: `lib.user.setup()` Per-User Configuration

## Goal

Implement per-user scoped configuration with `lib.user.setup()`.

## Problem

Currently all configuration applies system-wide. Users need a way to scope packages, files, and environment to specific users.

## Architecture Reference

- [04-lua-api.md](../architecture/04-lua-api.md):165-168 - lib.user.setup()
- [07-modules.md](../architecture/07-modules.md):369-402 - User scoping
- [09-platform.md](../architecture/09-platform.md):74-98 - Per-user profiles

## Approach

1. Create `lua/syslua/lib/user.lua` module
2. Implement scoping via a global context variable
3. Generate per-user environment scripts
4. Handle user home directory resolution

## Lua API

```lua
local lib = require("syslua.lib")

lib.user.setup({
    name = "alice",
    setup = function()
        require("syslua.pkgs.cli.ripgrep").setup()
        lib.file.setup({ path = "~/.gitconfig", source = "./gitconfig" })
        lib.env.setup({ EDITOR = "nvim" })
    end,
})
```

## Per-User Script Generation

```
~/.local/share/syslua/
├── env.sh              # System-level env
└── users/
    ├── alice/
    │   ├── env.sh      # alice's packages + env vars
    │   └── env.fish
    └── bob/
        ├── env.sh      # bob's packages + env vars
        └── env.fish
```

## Files to Create

| Path | Purpose |
|------|---------|
| `lua/syslua/lib/user.lua` | User scoping module |

## Files to Modify

| Path | Changes |
|------|---------|
| `lua/syslua/lib/init.lua` | Export user module |
| `crates/lib/src/execute/apply.rs` | Generate per-user scripts |

## Success Criteria

1. `lib.user.setup()` scopes configuration to a user
2. User's home directory (`~`) expands correctly
3. Per-user environment scripts are generated
4. Nested user scopes produce error
5. Works with `lib.file.setup()` and `lib.env.setup()`

## Open Questions

- [ ] How to handle root/admin vs normal user?
- [ ] Should user scope affect store location?
- [ ] How to detect which user is currently logged in?
- [ ] What about multi-user services?
