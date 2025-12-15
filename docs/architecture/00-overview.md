# sys.lua Architecture Overview

> **Note:** This is a design document describing the target architecture for sys.lua.

sys.lua is a cross-platform declarative system/environment manager inspired by Nix.

## Core Values

1. **Standard Lua Idioms**: Plain tables, functions, `require()`. No magic, no DSL, no hidden behavior.
2. **Reproducibility**: Same config + same inputs = same environment, regardless of platform
3. **Builds & Binds**: The two atomic building blocks upon which all user-facing APIs are built
4. **Immutability**: Store objects are immutable and content-addressed
5. **Declarative**: The Lua config file is the single source of truth
6. **Simplicity**: Prebuilt binaries when available, human-readable store layout
7. **Cross-platform**: First-class support for Linux, macOS, and Windows
8. **Small backend surface area**: Rust only handles Lua parsing, builds, binds, the store, and snapshots
9. **Composability**: Builds can be built from other builds; binds can reference multiple builds

## Standard Lua Idioms

This is a core value that permeates the entire design:

```lua
-- sys.lua modules are plain Lua modules
local nginx = require("syslua.modules.services.nginx")
nginx.setup({ port = 8080 })

-- No magic. Just:
-- 1. require() returns a table
-- 2. setup() is a function call
-- 3. Options are plain tables
```

What this means in practice:

| Do                          | Don't                 |
| --------------------------- | --------------------- |
| `require()` + `setup()`     | Auto-evaluation magic |
| Plain tables for options    | Special DSL syntax    |
| Explicit function calls     | Implicit behavior     |
| Standard `for`/`if`/`while` | Custom control flow   |

If you know Lua, you know how to use sys.lua.

## The Two Primitives

Everything in sys.lua builds on two fundamental concepts:

```
Build (sys.build {})              Bind (sys.bind {})
━━━━━━━━━━━━━━━━━━━━━━━━          ━━━━━━━━━━━━━━━━━━━━━━━━━━
Describes HOW to produce          Describes WHAT TO DO with
content for the store.            build output.

- Fetch from URL                  - Run shell commands
- Execute shell commands          - Create symlinks
- Build from source               - Modify system state
- Generate config file            - Enable services

Output: immutable store object    Output: system side effects
                                  which are journaled and can
                                  be rolled back.
```

All user-facing APIs (`lib.file.setup()`, `lib.env.setup()`, `lib.user.setup()`, `package.setup()`) internally create builds and/or binds.

## The `cmd` Action

Both builds and binds use a flexible `cmd` action for executing platform-specific operations:

```lua
-- Build context: execute commands during build
sys.build({
  name = "my-tool",
  apply = function(inputs, ctx)
    ctx:cmd({ cmd = "make", cwd = "/build" })
    ctx:cmd({ cmd = "make install", env = { PREFIX = ctx.outputs.out } })
  end,
})

-- Bind context: apply and destroy are separate functions
sys.bind({
  apply = function(inputs, ctx)
    ctx:cmd('ln -s "/src" "/dest"')
  end,
  destroy = function(inputs, ctx)  -- Optional: for rollback support
    ctx:cmd('rm "/dest"')
  end,
})
```

This design provides maximum flexibility - the Lua configuration decides what commands to run for each platform, rather than relying on preset Rust-backed actions.

## Rust Surface Area

The Rust implementation is intentionally minimal, covering:

| Component     | Purpose                                 |
| ------------- | --------------------------------------- |
| **Builds**    | Hashing, realization, build context     |
| **Binds**     | Execution of system side effects        |
| **Store**     | Content-addressed storage, immutability |
| **Lua parsing** | Config evaluation via mlua            |
| **Snapshots** | History and rollback                    |

## Terminology

| Term           | Definition                                                       |
| -------------- | ---------------------------------------------------------------- |
| **Build**      | Immutable description of how to produce store content            |
| **Bind**       | Description of what to do with build output                      |
| **Store**      | Global, immutable location for package content (`/syslua/store`) |
| **Store Object** | Content-addressed directory in `store/obj/<hash>/`             |
| **Manifest**   | Intermediate representation from evaluating Lua config           |
| **Snapshot**   | Point-in-time capture of builds + binds                          |
| **Input**      | Declared source of packages (GitHub repo, local path, Git URL)   |

## High-Level Architecture

```
┌─────────────────────────────────────────────────────────┐
│                  User Config (init.lua)                  │
│  - Declares packages, files, env vars, services         │
│  - Uses Lua for logic and composition                   │
└───────────────────────────┬─────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────┐
│                Evaluation & Resolution                   │
│  - Parse Lua → Manifest                                 │
│  - Resolve inputs from lock file                        │
│  - Priority-based conflict resolution                   │
└───────────────────────────┬─────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────┐
│                  DAG Construction                        │
│  - Build execution graph from manifest                  │
│  - Topological sort, cycle detection                    │
└───────────────────────────┬─────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────┐
│                 Parallel Execution                       │
│  - Realize builds → store objects                       │
│  - Execute binds → system side effects                  │
│  - Atomic: all-or-nothing with rollback                 │
└───────────────────────────┬─────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────┐
│                   Immutable Store                        │
│  obj/<name>-<hash>/   Content-addressed objects         │
│  pkg/<name>/<ver>/    Human-readable symlinks           │
└─────────────────────────────────────────────────────────┘
```

## Document Index

This architecture is documented across focused files:

| Document                           | Content                                       |
| ---------------------------------- | --------------------------------------------- |
| [01-builds.md](./01-builds.md)     | Build system, context API, hashing            |
| [02-binds.md](./02-binds.md)       | Bind types, execution, examples               |
| [03-store.md](./03-store.md)       | Store layout, realization, immutability       |
| [04-lua-api.md](./04-lua-api.md)   | Lua API layers, globals, type definitions     |
| [05-snapshots.md](./05-snapshots.md) | Snapshot design, rollback, garbage collection |
| [06-inputs.md](./06-inputs.md)     | Input sources, registry, lock files           |
| [07-modules.md](./07-modules.md)   | Module system, auto-evaluation                |
| [08-apply-flow.md](./08-apply-flow.md) | Apply flow, DAG execution, atomicity      |
| [09-platform.md](./09-platform.md) | Platform-specific: services, env, paths       |

## Key Design Decisions

### Why Builds + Binds?

Separating build from deployment (bind) provides:

- **Better caching**: Same content with different targets = one build, multiple binds
- **Cleaner rollback**: Builds are immutable; only binds change
- **Composability**: Multiple binds can reference the same build
- **Clear semantics**: Build logic stays pure; side effects are explicit

### Why Content-Addressed Storage?

- **Deduplication**: Same content = same hash = stored once
- **Reproducibility**: Hash guarantees identical content
- **Safe rollback**: Old versions remain in store until GC
- **Parallel safety**: No conflicts from concurrent operations

### Why Lua?

- **Familiar syntax**: Easy to read and write
- **Powerful**: First-class functions, tables for configuration
- **Safe**: No arbitrary system access from config
- **Embeddable**: mlua provides excellent Rust integration

### Why `cmd` Instead of Preset Actions?

The `cmd` action provides maximum flexibility:

- **Platform-specific**: Lua config decides what commands to run per platform
- **No Rust changes**: Adding new operations doesn't require Rust code changes
- **Transparent**: Users can see exactly what commands will be executed
- **Composable**: Complex operations built from simple shell commands
