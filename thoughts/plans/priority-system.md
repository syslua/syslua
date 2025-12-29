# Plan: Priority System (`mkDefault`, `mkForce`, etc.)

## Goal

Implement the priority-based conflict resolution system for configuration values.

## Problem

When multiple declarations affect the same key (e.g., two modules set `EDITOR`), there's no way to resolve conflicts. The architecture describes a priority system that isn't implemented.

## Architecture Reference

- [04-lua-api.md](../architecture/04-lua-api.md):191-210 - lib priority functions
- [08-apply-flow.md](../architecture/08-apply-flow.md):313-363 - Priority-based conflict resolution

## Approach

### Priority Values

| Function | Priority | Use Case |
|----------|----------|----------|
| `lib.mkForce` | 50 | Force a value (highest priority) |
| `lib.mkBefore` | 500 | Prepend to mergeable values |
| (default) | 1000 | Normal declarations |
| `lib.mkDefault` | 1000 | Provide a default |
| `lib.mkAfter` | 1500 | Append to mergeable values |

### Implementation

1. Create priority wrapper type in Lua (table with metatable)
2. Track priorities during manifest building
3. Resolve conflicts: lowest priority wins for singular values
4. Merge and sort for mergeable values (PATH, etc.)
5. Error on same priority + different values

## Lua API

```lua
local lib = require("syslua.lib")

lib.env.setup({
    EDITOR = lib.mkDefault("nano"),      -- Can be overridden
})

lib.env.setup({
    EDITOR = lib.mkForce("nvim"),        -- Forces this value
})

lib.env.setup({
    PATH = lib.mkBefore("/my/bin"),      -- Prepend to PATH
    PATH = lib.mkAfter("/opt/bin"),      -- Append to PATH
})
```

## Files to Create

| Path | Purpose |
|------|---------|
| `lua/syslua/lib/priority.lua` | Priority wrapper functions |

## Files to Modify

| Path | Changes |
|------|---------|
| `lua/syslua/lib/init.lua` | Export mkDefault, mkForce, etc. |
| `crates/lib/src/manifest/types.rs` | Add priority field to manifest items |
| `crates/lib/src/eval.rs` | Handle priority resolution during eval |

## Success Criteria

1. `lib.mkDefault()` and `lib.mkForce()` work in configs
2. Conflicts are resolved by priority
3. Same priority + different values produces clear error
4. Mergeable values combine correctly with ordering
5. Priority info preserved through evaluation

## Open Questions

- [ ] Should priority be stored in manifest or resolved at eval time?
- [ ] How to handle priorities across modules?
- [ ] What's the error message format for conflicts?
