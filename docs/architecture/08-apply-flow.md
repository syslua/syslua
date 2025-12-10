# Apply Flow

> Part of the [sys.lua Architecture](./00-overview.md) documentation.

This document covers the apply command flow, DAG construction, parallel execution, and atomicity.

## Overview

The apply command is fully declarative - it makes the current state match the config exactly by both installing new packages and removing packages not in the config.

**Key Design Principle:** Lua configuration is evaluated into a manifest first, conflicts are resolved using priorities, then a DAG-based system applies changes. This ensures:

- Order of declarations in Lua does not affect the final result
- Conflicts are detected and resolved deterministically
- The system determines optimal execution order, not the user
- Dependencies are resolved before dependents
- Parallel execution where possible

## Apply Flow Diagram

```
sys apply init.lua
    │
    ├─► PHASE 1: INPUT RESOLUTION
    │   ├─► Load init.lua, extract M.inputs table
    │   ├─► For each input in M.inputs:
    │   │   ├─► Check syslua.lock for pinned revision
    │   │   ├─► If locked: use pinned rev
    │   │   └─► If not locked: resolve latest, update lock
    │   ├─► Fetch/clone all inputs to cache
    │   └─► Configure require("inputs.*") paths
    │
    ├─► PHASE 2: CONFIGURATION EVALUATION
    │   ├─► Call M.setup(inputs) with resolved inputs
    │   ├─► Execute all require().setup(), file{}, env{}, user{} declarations
    │   ├─► Collect all declarations with their priorities
    │   └─► Resolve fetch helpers (fetchUrl, fetchGit, etc.)
    │
    ├─► PHASE 3: MERGE & CONFLICT RESOLUTION
    │   ├─► Group declarations by key (package name, file path, env var)
    │   ├─► For each group:
    │   │   ├─► Singular values: lowest priority wins
    │   │   ├─► Mergeable values: combine and sort by priority
    │   │   └─► Same priority + different values: ERROR
    │   └─► Produce resolved Manifest
    │
    ├─► PHASE 4: PLANNING
    │   ├─► Get current installed state from store
    │   ├─► Compute diff: desired (manifest) vs current
    │   │   ├─► to_realize = derivations not in store
    │   │   └─► to_deactivate = current activations not in manifest
    │   ├─► Build execution DAG from manifest
    │   │   ├─► Nodes: derivations and activations
    │   │   └─► Edges: derivation dependencies (from opts)
    │   └─► Topologically sort DAG for execution order
    │
    ├─► PHASE 5: EXECUTION
    │   ├─► Display plan (always shown)
    │   ├─► If no changes: exit early
    │   ├─► Create pre-apply snapshot (with config content)
    │   ├─► Execute DAG in topological order:
    │   │   ├─► Parallel execution for independent nodes
    │   │   ├─► Realize derivations (download/build to store)
    │   │   ├─► Execute activations (symlinks, PATH, services)
    │   │   └─► Deactivate removed activations
    │   ├─► On failure: rollback completed nodes, abort
    │   ├─► Create post-apply snapshot (with config content)
    │   └─► Generate env scripts (env.sh, env.fish, env.ps1)
    │
    └─► Print summary and shell setup instructions
```

## Two-Phase Evaluation

sys.lua uses a two-phase evaluation model that separates input resolution from configuration:

### Phase 1: Input Resolution

Before any configuration runs, syslua:

1. Loads `init.lua` and reads the `M.inputs` table
2. Resolves each input (checking lock file, fetching from git, etc.)
3. Configures the Lua `require` path so `require("inputs.<name>")` works

```lua
-- init.lua
local M = {}

-- Phase 1 reads this table BEFORE calling setup
M.inputs = {
    pkgs = "git:https://github.com/syslua/pkgs.git",
    private = "git:git@github.com:myorg/dotfiles.git",
}

-- Phase 2 calls this AFTER inputs are resolved
function M.setup(inputs)
    local pkgs = require("inputs.pkgs")
    pkgs.cli.ripgrep.setup()
end

return M
```

### Why Two Phases?

- **Deterministic resolution**: All inputs resolved before config runs—no ordering issues
- **Lock file accuracy**: syslua knows all inputs upfront to check/update the lock
- **Clear errors**: Input fetch failures happen before any configuration side effects
- **Parallel fetching**: All inputs can be fetched concurrently

### Phase 2: Configuration Evaluation

Once inputs are resolved, syslua calls `M.setup(inputs)`:

1. The `inputs` parameter contains metadata about resolved inputs
2. `require("inputs.<name>")` loads modules from the resolved input
3. All declarations (`file{}`, `env{}`, `user{}`, `setup()` calls) are collected
4. Priorities are tracked for conflict resolution

## Manifest Structure

The manifest is the intermediate representation between Lua config and system state. It contains only the two core primitives:

```rust
/// The complete manifest produced by evaluating a Lua configuration
pub struct Manifest {
    /// All derivations (build recipes producing store content)
    pub derivations: Vec<Derivation>,
    /// All activations (making derivations visible to the system)
    pub activations: Vec<Activation>,
}
```

**Key insight:** There are no separate types for packages, files, or environment variables. The Lua helpers `file{}`, `env{}`, `user{}`, and package `setup()` all create derivations and activations internally:

| Lua Declaration | Creates |
|-----------------|---------|
| `require("pkgs.cli.ripgrep").setup()` | Derivation (fetch/build) + Activation (add to PATH) |
| `file { path, source }` | Derivation (copy to store) + Activation (symlink) |
| `env { EDITOR = "nvim" }` | Derivation (generate shell scripts) + Activation (source in shell) |
| `user { name, setup }` | Scoping only (derivations/activations created inside `setup`) |

This unified model provides:

- **Simpler implementation**: Only two types to handle, not N
- **Consistent caching**: All content goes through the derivation system
- **Clean rollback**: Snapshots store derivation hashes + activations
- **Composability**: Everything uses the same primitives

See [Derivations](./01-derivations.md) and [Activations](./02-activations.md) for the full type definitions.

## Execution DAG

The DAG ensures correct ordering regardless of config declaration order. Nodes are derivations and activations; edges represent dependencies.

```
Example: User declares in any order:
  require("pkgs.cli.neovim").setup()
  require("pkgs.cli.ripgrep").setup()
  file { path = "~/.config/nvim/init.lua", source = "./nvim-config" }

Internally creates:
  - ripgrep derivation + activation
  - neovim derivation + activation
  - file derivation (copy content) + activation (symlink)

DAG constructed (derivations must complete before their activations):
  ┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
  │ ripgrep (deriv) │     │ neovim (deriv)  │     │ nvim-cfg (deriv)│
  └────────┬────────┘     └────────┬────────┘     └────────┬────────┘
           │                       │                       │
           ▼                       ▼                       ▼
  ┌─────────────────┐     ┌─────────────────┐     ┌─────────────────┐
  │ ripgrep (activ) │     │ neovim (activ)  │     │ nvim-cfg (activ)│
  └─────────────────┘     └─────────────────┘     └─────────────────┘

Execution order (determined by system, not user):
  Wave 1: ripgrep, neovim, nvim-cfg derivations (parallel - independent)
  Wave 2: ripgrep, neovim, nvim-cfg activations (parallel - derivations done)
```

### DAG Execution Example

```
$ sys plan init.lua

Derivations to realize:
  + ripgrep-15.1.0-abc123 (not in store)
  + neovim-0.10.0-def456 (not in store)
  = postgresql-16.1.0-ghi789 (cached)

Activations to apply:
  + AddToPath: ripgrep/bin
  + AddToPath: neovim/bin
  + AddToPath: postgresql/bin
  + Symlink: ~/.config/nvim/init.lua

Execution order:
  [Wave 1] Realize: ripgrep, neovim (parallel)
  [Wave 2] Activate: all activations (parallel, derivations done)
```

## Atomic Apply (All-or-Nothing)

**sys.lua uses atomic semantics for the apply operation.** Either all changes succeed or the system remains in its previous state - there is no partial application.

### Why Atomic?

Partial application creates broken states that are difficult to debug and recover from:

- A file might reference a package that failed to install
- Environment variables might point to missing paths
- Services might fail because their dependencies aren't available
- Users would need to manually figure out what succeeded vs failed

### How It Works

```
Apply begins
    │
    ├─► Create pre-apply snapshot
    │
    ├─► Execute DAG nodes...
    │       │
    │       ├─► Node 1: Success ✓ (tracked)
    │       ├─► Node 2: Success ✓ (tracked)
    │       ├─► Node 3: FAILURE ✗
    │       │
    │       └─► Rollback triggered
    │               │
    │               ├─► Undo Node 2
    │               ├─► Undo Node 1
    │               └─► Restore pre-apply snapshot
    │
    └─► Exit with error (system unchanged)
```

### Rollback Behavior

When any node in the DAG fails:

1. **Stop execution** - No further nodes are attempted
2. **Undo completed nodes** - In reverse order of completion
3. **Restore snapshot** - Revert to the pre-apply snapshot
4. **Report failure** - Show which node failed and why

```bash
$ sudo sys apply sys.lua
Evaluating sys.lua...
Building execution plan...

Executing:
  [1/4] ✓ ripgrep@15.1.0
  [2/4] ✓ fd@9.0.0
  [3/4] ✗ custom-tool@1.0.0
        Error: Build failed: missing dependency 'libfoo'

Rolling back...
  - Removing fd@9.0.0 from profile
  - Removing ripgrep@15.1.0 from profile
  - Restoring pre-apply state

Apply failed. System unchanged.
Run 'sys plan' to review the execution plan.
```

### What Gets Rolled Back

| Component        | Rollback Action                                         |
| ---------------- | ------------------------------------------------------- |
| **Derivations**  | Objects remain in store (immutable, may be GC'd later)  |
| **Activations**  | Deactivate: remove symlinks, PATH entries, stop services|
| **Symlinks**     | Restore original target or remove                       |
| **Environment**  | Regenerate env scripts from previous snapshot           |
| **Services**     | Stop newly started services, restart stopped services   |

### Edge Cases

**Already-installed packages**: If a package already exists in the store from a previous apply, it's not re-downloaded. Rollback simply removes the symlink - the cached object remains for future use.

**External changes during apply**: If the system is modified externally during apply (rare), rollback restores to the snapshot which reflects state at apply-start, not the external changes.

**Idempotent re-apply**: After a failed apply and rollback, running `sys apply` again will attempt the same changes. Fix the underlying issue (e.g., the missing `libfoo` dependency) before re-running.

## Plan Command

Preview changes without applying (evaluates config to manifest, builds DAG, but doesn't execute):

```bash
$ sys plan sys.lua
Evaluating sys.lua...
Building execution plan...

Derivations:
  + fd-9.0.0-abc123 (to realize)
  + bat-0.24.0-def456 (to realize)
  = jq-1.7.1-ghi789 (cached)
  - ripgrep-14.1.1-old123 (unreferenced, will be GC'd)

Activations:
  + AddToPath: fd/bin
  + AddToPath: bat/bin
  = AddToPath: jq/bin (unchanged)
  - AddToPath: ripgrep/bin (to remove)

Execution order:
  1. [realize] fd, bat (parallel)
  2. [activate] fd, bat activations (parallel)
  3. [deactivate] ripgrep activation
```

## Priority-Based Conflict Resolution

When multiple declarations affect the same key, priorities determine the outcome:

### Priority Values

| Function        | Priority | Use Case                    |
| --------------- | -------- | --------------------------- |
| `lib.mkForce`   | 50       | Force a value (highest)     |
| `lib.mkBefore`  | 500      | Prepend to mergeable values |
| (default)       | 1000     | Normal declarations         |
| `lib.mkDefault` | 1000     | Provide a default           |
| `lib.mkAfter`   | 1500     | Append to mergeable values  |

### Singular Values

For values that can only have one result (e.g., `EDITOR`), lowest priority wins:

```lua
env { EDITOR = "vim" }                    -- priority 1000
env { EDITOR = lib.mkDefault("nano") }    -- priority 1000 (same)
env { EDITOR = lib.mkForce("nvim") }      -- priority 50 (wins)
```

### Mergeable Values

For values that combine (e.g., `PATH`), all declarations are merged and sorted by priority:

```lua
env { PATH = lib.mkBefore("/custom/bin") }  -- priority 500 (first)
env { PATH = "/home/user/bin" }              -- priority 1000 (middle)
env { PATH = lib.mkAfter("/opt/bin") }       -- priority 1500 (last)
-- Result: PATH="/custom/bin:/home/user/bin:/opt/bin:$PATH"
```

### Conflict Errors

Same priority + different values = error:

```lua
env { EDITOR = "vim" }   -- priority 1000
env { EDITOR = "emacs" } -- priority 1000 (ERROR!)
```

```
Error: Conflicting values for env.EDITOR at priority 1000:
  - "vim" (declared at sys.lua:10)
  - "emacs" (declared at sys.lua:15)

Use lib.mkForce() to override, or lib.mkDefault() to provide a fallback.
```

## See Also

- [Lua API](./04-lua-api.md) - Entry point pattern (`M.inputs`/`M.setup`)
- [Inputs](./06-inputs.md) - Input sources and authentication
- [Derivations](./01-derivations.md) - Build recipes
- [Activations](./02-activations.md) - Making derivations visible
- [Snapshots](./05-snapshots.md) - State capture and rollback
- [Store](./03-store.md) - Where objects live
