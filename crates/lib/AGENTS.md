# Agent Guidelines for syslua-lib

**Generated:** 2026-01-04 | **Commit:** bc66463 | **Branch:** main

## OVERVIEW

Core library for syslua. Implements content-addressed store, Lua configuration evaluation, 
atomic binds with rollback, and parallel DAG execution.

## STRUCTURE

- `action/`: Atomic execution units (Exec, FetchUrl) shared by builds/binds
- `bind/`: Mutable system state management (create/update/destroy/check)
- `build/`: Immutable content production for store
- `execute/`: DAG scheduling, parallel waves, and atomic apply orchestration
- `inputs/`: Transitive dependency resolution, lock files, namespace discovery
- `lua/`: mlua integration, global `sys` API, type conversion
- `manifest/`: Evaluated configuration IR (BTreeMap of BuildDef/BindDef)
- `snapshot/`: History tracking, diffing, and rollback journal

## WHERE TO LOOK

| Task | Location | Notes |
|------|----------|-------|
| Parallel Execution | `execute/mod.rs` | Wave-based scheduler using JoinSet |
| Apply/Rollback Flow | `execute/apply.rs` | High-level orchestration of diff/apply/rollback |
| Build Hashing | `build/types.rs` | Serializable BuildDef determines ObjectHash |
| Bind Logic | `bind/execute.rs` | Platform-specific side effect application |
| Placeholder Eval | `execute/resolver.rs` | Resolves $${...} during execution |
| Transitive Deps | `inputs/resolve.rs` | Recursive input fetching and lock management |

## CODE MAP

| Symbol | Type | Location | Role |
|--------|------|----------|------|
| `ExecutionDag` | struct | `execute/dag.rs` | Dependency graph for builds and binds |
| `ExecutionResolver` | struct | `execute/resolver.rs` | Resolves placeholders against completed nodes |
| `Action` | enum | `action/mod.rs` | Serializable command or fetch operation |
| `BindState` | struct | `bind/state.rs` | Persisted outputs for drift check/destroy |
| `StateDiff` | struct | `snapshot/diff.rs` | Comparison between current and desired state |
| `LuaNamespace` | struct | `inputs/types.rs` | Discovered Lua module paths from inputs |

## CONVENTIONS

- **Error Policy**: 18+ module-specific error enums using `thiserror`. All errors must be serializable.
- **Placeholder Resolution**: Resolved ONLY during execution via `ExecutionResolver`. Never store resolved values in `Def`.
- **Deterministic IR**: Use `BTreeMap` for all serializable maps to ensure stable hashes.
- **Bind ID**: IDs required for `update()` support; anonymous binds only support create/destroy.
- **Out Directory**: Builds must use `ctx:out()` placeholder for all filesystem output.
- **Store Layout**: `build/<hash>/` for immutable content, `bind/<hash>/` for state tracking.

## ANTI-PATTERNS

- **Mutable Global State**: Use `ActionCtx` and `ExecutionResolver` to pass state.
- **Direct FS in Build**: Builds MUST remain pure; use `Exec` actions for all changes.
- **HashMap in Defs**: Breaking deterministic hashing/serialization.
- **Unresolved Placeholders**: Accessing $${...} strings without passing through a resolver.
