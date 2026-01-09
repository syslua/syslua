# Agent Guidelines for syslua

**Generated:** 2026-01-04 | **Commit:** c3a22f5 | **Branch:** main

## OVERVIEW

Declarative cross-platform system manager. Rust workspace with Lua config evaluation (mlua), content-addressed store, DAG-based parallel execution.

## STRUCTURE

```
syslua/
├── crates/
│   ├── cli/           # Binary 'sys' - see cli/AGENTS.md
│   └── lib/           # Core library - see lib/AGENTS.md
├── docs/architecture/ # Design docs (00-09)
├── lua/               # Lua stdlib (init.lua entry point)
└── tests/             # Integration tests + fixtures
```

## WHERE TO LOOK

| Task              | Location                           | Notes                                                    |
| ----------------- | ---------------------------------- | -------------------------------------------------------- |
| Add CLI command   | `crates/cli/src/cmd/`              | One file per command, register in `mod.rs`               |
| Modify apply flow | `crates/lib/src/execute/`          | `apply.rs` orchestrates, `dag.rs` schedules              |
| Change Lua API    | `crates/lib/src/lua/`              | `globals.rs` for sys.\*, `helpers/` for types            |
| Add input source  | `crates/lib/src/inputs/`           | `source.rs` for types, `fetch.rs` for retrieval          |
| Platform behavior | `crates/lib/src/platform/`         | `os.rs`, `arch.rs`, `paths.rs`, `immutable.rs`           |
| Build/bind types  | `crates/lib/src/build/` or `bind/` | `types.rs` for structs, `lua.rs` for conversion          |
| Snapshot/rollback | `crates/lib/src/snapshot/`         | `diff.rs` for state comparison, `history.rs` for journal |

## CODE MAP

| Symbol                 | Type   | Location           | Role                                          |
| ---------------------- | ------ | ------------------ | --------------------------------------------- |
| `ObjectHash`           | struct | `util/hash.rs`     | 20-char truncated SHA256 for store addressing |
| `Hashable`             | trait  | `util/hash.rs`     | Content hashing for builds/binds              |
| `Resolver`             | trait  | `placeholder.rs`   | Placeholder substitution ($${...})            |
| `BuildSpec`/`BuildDef` | struct | `build/types.rs`   | Spec has Lua closures, Def is serializable    |
| `BindSpec`/`BindDef`   | struct | `bind/types.rs`    | Same pattern as build                         |
| `ActionCtx`            | struct | `action/types.rs`  | Base context for build/bind execution         |
| `Action`               | enum   | `action/mod.rs`    | Serializable command or fetch operation       |
| `ExecutionDag`         | struct | `execute/dag.rs`   | Dependency graph for builds and binds         |
| `ApplyError`           | enum   | `execute/types.rs` | Top-level execution errors                    |

## CONVENTIONS

- **Spec/Def duality**: `*Spec` contains `LuaFunction` closures (runtime), `*Def` is serializable (storage)
- **Three-stage pipeline**: Input Resolution → Lua Config Eval → DAG Construction → Parallel Execution
- **Placeholders**: `$${action:N}`, `$${build:HASH:output}`, `$${bind:HASH:output}`, `$${out}`
- **BTreeMap everywhere**: Deterministic serialization for reproducible hashes
- **Platform module**: Use `syslua_lib::platform` for OS-specific code, not direct APIs
- **Module hierarchy**: execute → manifest → {build,bind} → action → util/hash (one-way deps)

## ANTI-PATTERNS

| Forbidden                                 | Reason                                     |
| ----------------------------------------- | ------------------------------------------ |
| `.unwrap()` / `.expect()` in library code | Use `?` with proper error types            |
| `as any` / type suppression               | Explicit types at public boundaries        |
| Direct OS APIs                            | Use `platform/` abstractions               |
| Glob imports `use foo::*`                 | Explicit imports preferred                 |
| One-letter variable names                 | Except short loop indices                  |
| Builds referencing binds                  | One-way dependency: binds depend on builds |
| `nil` values in Lua inputs                | Explicit defaults required                 |

## COMMANDS

```bash
# Build
cargo build                          # Workspace
cargo build -p syslua-cli            # CLI only

# Test
cargo test                           # All tests
cargo test -p syslua-lib <filter>    # Specific test
cargo test -- --nocapture            # With output

# Lint (run before commits)
cargo fmt && cargo clippy --all-targets --all-features
```

## LOGGING

Use `tracing` macros with structured fields:

| Level    | Use For                                        |
| -------- | ---------------------------------------------- |
| `error!` | Unrecoverable failures                         |
| `warn!`  | Recoverable issues, degraded behavior          |
| `info!`  | User-facing milestones (command start/end)     |
| `debug!` | Internal ops, per-item progress, state changes |
| `trace!` | High-volume internals (DAG traversal, hashing) |

```rust
debug!(hash = %hash.0, "applying bind");
```

## NOTES

- **Windows first-class**: All features must work cross-platform
- **Edition 2024**: Use Rust 2024 idioms
- **Tests**: Unit tests inline `#[cfg(test)]`, integration in `tests/integration/`
- **Fixtures**: `tests/fixtures/*.lua` for test Lua configs
- **Unsafe blocks**: 3 exist (macOS chflags, Windows token/OVERLAPPED) - all documented
- **Complexity hotspots**: `inputs/resolve.rs` (1.8k lines), `execute/apply.rs` (1.5k lines)

## SEE ALSO

- [Architecture Docs](./docs/architecture/) - Design principles (builds, binds, store, Lua API, snapshots)
- [crates/cli/AGENTS.md](./crates/cli/AGENTS.md) - CLI binary
- [crates/lib/AGENTS.md](./crates/lib/AGENTS.md) - Library internals

## Landing the Plane (Session Completion)

**When ending a work session**, you MUST complete ALL steps below. Work is NOT complete until `git push` succeeds.

**MANDATORY WORKFLOW:**

1. **File issues for remaining work** - Create issues for anything that needs follow-up
2. **Run quality gates** (if code changed) - Tests, linters, builds
3. **Update issue status** - Close finished work, update in-progress items
4. **PUSH TO REMOTE** - This is MANDATORY:

   ```bash
   git pull --rebase
   bd sync
   git push
   git status  # MUST show "up to date with origin"
   ```

5. **Clean up** - Clear stashes, prune remote branches
6. **Verify** - All changes committed AND pushed
7. **Hand off** - Provide context for next session

**CRITICAL RULES:**

- Work is NOT complete until `git push` succeeds
- NEVER stop before pushing - that leaves work stranded locally
- NEVER say "ready to push when you are" - YOU must push
- If push fails, resolve and retry until it succeeds

Use 'bd' for task tracking
