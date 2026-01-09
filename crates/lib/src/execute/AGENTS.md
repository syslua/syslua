# Execute Module

**Generated:** 2026-01-04 | **Commit:** c3a22f5

**OVERVIEW:** Execution engine orchestrating parallel realize/apply waves via petgraph-backed DAG.

## FILES

- `apply.rs`: Top-level orchestration (evaluate -> diff -> exec -> snapshot).
- `dag.rs`: Dependency graph construction and wave calculation using `petgraph`.
- `resolver.rs`: Just-in-time placeholder resolution ($${{build:...}}, $${{bind:...}}).
- `types.rs`: Core error types (`ApplyError`, `ExecuteError`) and result structures.
- `mod.rs`: Public API entry point for manifest execution.

## EXECUTION MODEL

- **Petgraph DAG**: Nodes are `DagNode::Build(hash)` or `DagNode::Bind(hash)`.
- **Direction**: Directed edges from dependency (provider) to dependent (consumer).
- **Wave Parallelism**: Independent nodes at the same topological depth execute in parallel using `tokio::task::JoinSet`.
- **Atomicity**: Binds are journaled and rolled back on failure; realized builds persist in the immutable store.

## PLACEHOLDER RESOLUTION

- **Resolvers**: `BuildCtxResolver` (for builds, cannot reference binds) and `BindCtxResolver` (for binds, can reference builds and binds) map `ObjectHash` to its realized/applied outputs.
- **Substitution**: Placeholders like `$${{build:HASH:output}}` or `$${{bind:HASH:output}}` are resolved immediately before node execution.
- **Context**: Resolution uses the actual results from previously completed waves.

## ERROR HANDLING

- **Granular Errors**: `ExecuteError` covers command failures, IO, and hash mismatches.
- **Propagation**: If a dependency fails, all downstream nodes are marked `DependencyFailed` and skipped.
- **Rollback**: `ApplyError` manages global state recovery when the orchestration flow is interrupted.
