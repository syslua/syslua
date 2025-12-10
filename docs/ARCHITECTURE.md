# sys.lua Architecture

> **Note:** This is a design document describing the target architecture for sys.lua.

sys.lua is a cross-platform declarative system/environment manager inspired by Nix.

## Documentation

The architecture documentation has been split into focused documents for easier navigation:

| Document                                              | Content                                               |
| ----------------------------------------------------- | ----------------------------------------------------- |
| [00-overview.md](./architecture/00-overview.md)       | Core principles, terminology, high-level architecture |
| [01-derivations.md](./architecture/01-derivations.md) | Derivation system, context API, hashing               |
| [02-activations.md](./architecture/02-activations.md) | Activation types, execution, examples                 |
| [03-store.md](./architecture/03-store.md)             | Store layout, realization, immutability               |
| [04-lua-api.md](./architecture/04-lua-api.md)         | Lua API layers, globals, type definitions, LuaLS      |
| [05-snapshots.md](./architecture/05-snapshots.md)     | Snapshot design, rollback, garbage collection         |
| [06-inputs.md](./architecture/06-inputs.md)           | Input sources, registry, lock files, authentication   |
| [07-modules.md](./architecture/07-modules.md)         | Module system, auto-evaluation, composition           |
| [08-apply-flow.md](./architecture/08-apply-flow.md)   | Apply flow, DAG execution, atomicity                  |
| [09-platform.md](./architecture/09-platform.md)       | Platform-specific: services, env, paths               |
| [10-crates.md](./architecture/10-crates.md)           | Crate structure and Rust dependencies                 |

## Quick Reference

### Core Principles

1. **Reproducibility**: Same config + same inputs = same environment
2. **Derivations & Activations**: The two atomic building blocks
3. **Immutability**: Store objects are immutable and content-addressed
4. **Declarative**: Lua config is the single source of truth
5. **Cross-platform**: Linux, macOS, and Windows support

### The Two Primitives

```
Derivation (derive {})          Activation (activate {})
━━━━━━━━━━━━━━━━━━━━━━━━       ━━━━━━━━━━━━━━━━━━━━━━━━━━
Describes HOW to produce        Describes WHAT TO DO with
content for the store.          derivation output.

- Fetch from URL                - Add to PATH
- Clone git repo                - Create symlink
- Build from source             - Source in shell
- Generate config file          - Enable service

Output: immutable store object  Output: system side effects
```

### Rust Surface Area

The Rust implementation is intentionally minimal:

| Component   | Purpose                                 |
| ----------- | --------------------------------------- |
| Derivations | Hashing, realization, build context     |
| Activations | Execution of system side effects        |
| Store       | Content-addressed storage, immutability |
| Lua parsing | Config evaluation via mlua              |
| Snapshots   | History and rollback                    |

## Getting Started

Start with [00-overview.md](./architecture/00-overview.md) for the high-level architecture, then dive into specific topics as needed.
