# SysLua

**Declarative cross-platform system management with the simplicity of Lua and the power of Nix.**

> **Note:** SysLua is currently in the design phase. This README describes the target system. We're looking for contributors to help bring this vision to life.

## What is SysLua?

SysLua is a cross-platform declarative system/environment manager inspired by Nix. It combines:

1. **Standard Lua idioms** - Plain tables, functions, `require()`. No magic, no DSL, no hidden behavior
2. **Reproducibility** - Same config + same inputs = same environment, regardless of platform
3. **Builds & Binds** - Two atomic building blocks upon which all user-facing APIs are built

```lua
-- SysLua modules are plain Lua modules
local nginx = require('syslua.modules.services.nginx')
nginx.setup({ port = 8080 })

-- No magic. Just:
-- 1. require() returns a table
-- 2. setup() is a function call
-- 3. Options are plain tables
```

If you know Lua, you know how to use SysLua.

**Apply it:**

```bash
$ sys apply
```

Your system now matches your declaration.

## Why SysLua?

### The Problem with Current Tools

**Traditional package managers** (apt, brew, pacman) are imperative and stateful. You run commands that mutate system state, making it impossible to reproduce or rollback.

**Nix** solves this brilliantly but has a steep learning curve: custom language, complex module system, and impenetrable error messages scare away newcomers.

**SysLua bridges this gap.**

### What Makes SysLua Different

- **Standard Lua idioms** - `require()` + `setup()`, plain tables for options, explicit function calls
- **Prebuilt binaries first** - Install instantly without compilation
- **Reproducible by default** - Lock files pin exact versions
- **True cross-platform** - Linux, macOS, and Windows as first-class citizens
- **Composable** - Builds can be built from other builds; binds can reference multiple builds
- **Content-addressed store** - Immutable, deduplicated, human-readable layout
- **Atomic operations** - Apply succeeds completely or rolls back entirely
- **Small backend surface area** - Rust only handles Lua parsing, builds, binds, the store, and snapshots

## The Two Primitives

Everything in SysLua builds on two fundamental concepts:

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

All modules are built using these two primitives. For example, a service module may define a build that compiles the service binary, and a bind that sets up the service to run on system startup.

### The `exec` Action

Both builds and binds use a flexible `exec` action for executing platform-specific operations:

```lua
-- Build context: execute commands during build
local my_tool = sys.build({
  id = 'my-tool',
  inputs = {
    make = require('syslua.modules.build_tools.make').setup(),
  },
  create = function(inputs, ctx)
    local build_dir = ctx:exec({ bin = inputs.make.outputs.bin, cwd = '/build' })
    ctx:exec({
      bin = inputs.make.outputs.bin,
      args = { 'install' },
      env = { PREFIX = build_dir },
    })
  end,
})

-- Bind context: create and destroy are separate functions
sys.bind({
  inputs = {
    tool = my_tool,
  },
  create = function(inputs, ctx)
    local dest = '/usr/local/sbin/my-tool'
    ctx:exec({
      bin = '/bin/ln',
      args = { '-s', inputs.tool.outputs.bin, dest },
    })
    return { dest = dest }
  end,
  destroy = function(outputs, ctx)
    ctx:exec({
      bin = '/bin/rm',
      args = { outputs.dest },
    })
  end,
})
```

This design provides maximum flexibility - the Lua configuration decides what commands to run for each platform, rather than relying on preset Rust-backed actions.

### Why Builds + Binds?

Separating build from deployment (bind) provides:

- **Better caching** - Same content with different targets = one build, multiple binds
- **Cleaner rollback** - Builds are immutable; only binds change
- **Composability** - Multiple binds can reference the same build
- **Clear semantics** - Build logic stays pure; side effects are explicit

## Key Features

### Reproducible Environments

Lock files ensure your team uses identical package versions:

```bash
# Developer A: Add packages and commit lock file
$ sys apply # Creates syslua.lock
$ git add . && git commit

# Developer B: Get exact same versions
$ git pull
$ sys apply # Uses pinned versions from syslua.lock
```

### Atomic Rollbacks

Every `sys apply` creates a snapshot. Rollback instantly if something breaks:

```bash
$ sys history
Snapshots:
#5  2024-12-08 14:23  Added Docker and PostgreSQL
#4  2024-12-07 09:15  Updated neovim to 0.10.0
#3  2024-12-06 18:42  Initial system config

$ sys rollback 4 # Instant rollback to snapshot #4
```

### Cross-Platform

First-class support for Linux, macOS, and Windows. Platform-specific logic lives in Lua:

```lua
-- Platform-specific behavior in the exec action
sys.bind({
  inputs = { tool = my_tool },
  create = function(inputs, ctx)
    if sys.os == 'linux' then
      ctx:exec({
        bin = '/bin/systemctl',
        args = { 'enable', 'my-tool' },
      })
    elseif sys.os == 'darwin' then
      ctx:exec({
        bin = '/bin/launchctl',
        args = { 'load', '/Library/LaunchDaemons/my-tool.plist' },
      })
    end
  end,
})
```

## Architecture

### High-Level Flow

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
│  - Resolve inputs from lock file                        │
│  - Parse Lua → Manifest                                 │
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
│  build/<hash>/      Content-addressed objects           │
│  bind/<hash>/       Bind state tracking                 │
└─────────────────────────────────────────────────────────┘
```

### Content-Addressed Store

Packages are stored immutably with content-addressed hashing (20-char truncated hash):

```
/syslua/store/
├── build/<hash>/     # Immutable content (e.g., build/abc123def456789012ab/)
│   └── bin/rg
├── bind/<hash>/      # Bind state tracking
└── ...
```

### Rust Surface Area

The Rust implementation is intentionally minimal:

| Component       | Purpose                                 |
| --------------- | --------------------------------------- |
| **Builds**      | Hashing, realization, build context     |
| **Binds**       | Execution of system side effects        |
| **Store**       | Content-addressed storage, immutability |
| **Lua parsing** | Config evaluation via mlua              |
| **Snapshots**   | History and rollback                    |

### Why Content-Addressed Storage?

- **Deduplication** - Same content = same hash = stored once
- **Reproducibility** - Hash guarantees identical content
- **Safe rollback** - Old versions remain in store until GC
- **Parallel safety** - No conflicts from concurrent operations

### Why Lua?

- **Familiar syntax** - Easy to read and write
- **Powerful** - First-class functions, tables for configuration
- **Safe** - No arbitrary system access from config
- **Embeddable** - mlua provides excellent Rust integration

### Why `exec` Instead of Preset Actions?

The `exec` action provides maximum flexibility:

- **Platform-specific** - Lua config decides what commands to run per platform
- **No Rust changes** - Adding new operations doesn't require Rust code changes
- **Transparent** - Users can see exactly what commands will be executed
- **Composable** - Complex operations built from simple shell commands

## Terminology

| Term            | Definition                                                       |
| --------------- | ---------------------------------------------------------------- |
| **Build**       | Immutable description of how to produce store content            |
| **Bind**        | Description of what to do with build output                      |
| **Store**       | Global, immutable location for package content (`/syslua/store`) |
| **Store Build** | Content-addressed directory in `store/build/<hash>/`             |
| **Manifest**    | Intermediate representation from evaluating Lua config           |
| **Snapshot**    | Point-in-time capture of builds + binds                          |
| **Input**       | Declared source of packages (GitHub repo, local path, Git URL)   |

## Project Status

**SysLua is currently in the design and architecture phase.** We have:

- Comprehensive architecture documentation (see [docs/architecture/](./docs/architecture/))
- Split architecture docs covering [builds](./docs/architecture/01-builds.md), [binds](./docs/architecture/02-binds.md), [store](./docs/architecture/03-store.md), [Lua API](./docs/architecture/04-lua-api.md), and more
- Crate structure defined (`syslua-cli`, `syslua-lib`)
- Clear design philosophy and feature roadmap
- Implementation in progress

## Getting Involved

We're actively seeking contributors to help build SysLua. Here's how you can help:

### For Developers

- **Rust developers**: Core functionality needs implementation
- **Lua experts**: Help design the Lua API and standard library
- **Platform specialists**: Windows/macOS/Linux-specific features
- **Package maintainers**: Build the official package registry

### For Early Adopters

- **Feedback**: Review the [architecture docs](./docs/architecture/) and share thoughts
- **Use cases**: Tell us about your workflow and how SysLua could help
- **Testing**: Try early releases and report issues

### Getting Started

```bash
# Clone the repository
$ git clone https://github.com/syslua/syslua.git
$ cd syslua

# Build the project
$ cargo build --release

# Read the architecture
$ cat docs/architecture/00-overview.md

# Check contributor guidelines
$ cat AGENTS.md
```

## Design Principles

SysLua is guided by these core principles:

1. **Standard Lua Idioms** - Plain tables, functions, `require()`. No magic, no DSL
2. **Reproducibility** - Same config + same inputs = same environment
3. **Builds & Binds** - Two atomic building blocks for all operations
4. **Immutability** - Store objects are immutable and content-addressed
5. **Declarative** - The Lua config file is the single source of truth
6. **Simplicity** - Prebuilt binaries when available, human-readable store layout
7. **Cross-platform** - Linux, macOS, Windows as first-class citizens
8. **Small backend surface area** - Rust handles only the essentials
9. **Composability** - Builds from builds; binds reference multiple builds

## Inspiration

SysLua stands on the shoulders of giants:

- **Nix/NixOS** - Reproducibility, immutability, declarative config
- **Home Manager** - User-level configuration management
- **Ansible** - Simple, readable syntax
- **Lua** - Approachable, powerful language

We're taking the best ideas from these projects and making them accessible to everyone.

## Comparison

| Feature           | SysLua        | Nix       | Ansible    | Homebrew   |
| ----------------- | ------------- | --------- | ---------- | ---------- |
| Declarative       | Yes           | Yes       | Yes        | No         |
| Reproducible      | Yes           | Yes       | Partial    | No         |
| Rollback          | Yes           | Yes       | No         | No         |
| Cross-platform    | Yes           | Partial   | Yes        | macOS only |
| Easy to learn     | Yes           | No        | Yes        | Yes        |
| Prebuilt binaries | Yes (default) | Sometimes | N/A        | Yes        |
| Immutable store   | Yes           | Yes       | No         | No         |
| Standard language | Yes (Lua)     | No (Nix)  | Yes (YAML) | N/A        |

## License

MIT License - see [LICENSE](./LICENSE)

## Community

- **GitHub Issues**: [Report bugs and request features](https://github.com/syslua/syslua/issues)

## Documentation

See the [architecture documentation](./docs/architecture/) for detailed design:

| Document                                                 | Content                                       |
| -------------------------------------------------------- | --------------------------------------------- |
| [00-overview.md](./docs/architecture/00-overview.md)     | Architecture overview, core values            |
| [01-builds.md](./docs/architecture/01-builds.md)         | Build system, context API, hashing            |
| [02-binds.md](./docs/architecture/02-binds.md)           | Bind types, execution, examples               |
| [03-store.md](./docs/architecture/03-store.md)           | Store layout, realization, immutability       |
| [04-lua-api.md](./docs/architecture/04-lua-api.md)       | Lua API layers, globals, type definitions     |
| [05-snapshots.md](./docs/architecture/05-snapshots.md)   | Snapshot design, rollback, garbage collection |
| [06-inputs.md](./docs/architecture/06-inputs.md)         | Input sources, registry, lock files           |
| [07-modules.md](./docs/architecture/07-modules.md)       | Module system                                 |
| [08-apply-flow.md](./docs/architecture/08-apply-flow.md) | Apply flow, DAG execution, atomicity          |
| [09-platform.md](./docs/architecture/09-platform.md)     | Platform-specific: services, env, paths       |

---

**SysLua** - System management that makes sense.

_Declarative. Reproducible. Simple._
