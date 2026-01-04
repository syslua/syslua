# Inputs Internals

**OVERVIEW:** Declarative input resolution, dependency graphing, and lockfile management.

## FILES
- `mod.rs`: Module entry and orchestration logic.
- `source.rs`: URL parsing for `git:`, `path:`, and shorthand sources.
- `resolve.rs`: Transitive resolution engine; handles `follows` and overrides.
- `graph.rs`: Dependency DAG management using `petgraph`.
- `lock.rs`: `LockFile` persistence and reconciliation.
- `fetch.rs`: Git/HTTP retrieval and local path resolution.
- `store.rs`: Cache-backed storage for resolved inputs.
- `types.rs`: Core types (`InputDecl`, `ResolvedInput`, `InputOverride`).

## KEY TYPES
- `InputSource`: Parsed request (URL/Path/Revision).
- `ResolvedInput`: Fetched input with pinned hash and transitive deps.
- `InputGraph`: DAG representing the full dependency tree.
- `LockFile`: Deterministic mapping of pinned revisions and hashes.
- `InputOverride`: Specification for transitive dependency redirection (`follows`).

## FLOW
1. **Load**: Load `LockFile` and parse `InputDecl` from Lua config.
2. **Fetch**: `fetch.rs` retrieves content; `store.rs` caches the results.
3. **Recurse**: `resolve.rs` inspects `init.lua` of fetched inputs for sub-deps.
4. **Override**: Apply `follows` overrides to transitive dependencies.
5. **Verify**: Detect namespace conflicts (duplicate providers for Lua paths).
6. **Assemble**: Build `InputGraph` (DAG) and topological sort.
7. **Pin**: Generate updated `LockFile` for reproducibility.

## GOTCHAS
- **Transitive Complexity**: `resolve.rs` is the most complex part (1.8k lines).
- **Namespace Conflicts**: Occur when multiple inputs provide the same top-level Lua module.
- **Lock Reconciliation**: Updates only occur on explicit `sys update` or URL changes.
- **Determinism**: Uses `BTreeMap` throughout to ensure stable lockfile serialization.
- **Cycles**: `petgraph` detects cycles during graph construction.
