# Atomic Apply with Full Rollback

## Problem Statement

When `apply()` destroys removed binds and then execution fails, those destroyed binds are gone (their state files deleted immediately), leaving the system in an inconsistent state. The user's system ends up partially applied with no way to recover automatically.

## Goals

1. **Atomic apply**: Either all changes succeed, or the system returns to pre-apply state
2. **Graceful degradation**: If restore fails, enable self-healing on next apply
3. **Preserve existing parallelism**: Maintain wave-based parallel execution

## Current Architecture

### Key Files and Their Roles

| File | Role |
|------|------|
| `execute/apply.rs` | High-level `apply()` orchestration, `destroy_removed_binds()` |
| `execute/mod.rs` | `execute_manifest()`, wave execution, existing `rollback_binds()` for in-flight binds |
| `execute/dag.rs` | `ExecutionDag`, `execution_waves()` for dependency ordering |
| `execute/resolver.rs` | `ExecutionResolver` for placeholder resolution (`$${build:hash:out}`, `$${bind:hash:output}`) |
| `bind/execute.rs` | `apply_bind()`, `destroy_bind()` - async functions |
| `bind/state.rs` | `save_bind_state()`, `load_bind_state()`, `remove_bind_state()` |
| `snapshot/storage.rs` | `SnapshotStore` with `load_current()`, `set_current()`, etc. |

### Current Flow

1. `apply()` loads current snapshot and evaluates config to get desired manifest
2. `compute_diff()` determines what to destroy/create
3. `destroy_removed_binds()` destroys old binds AND removes their state files immediately
4. `execute_manifest()` realizes builds and applies binds (with existing in-flight rollback)
5. On success: save bind states, save snapshot

### Problem Point

Line 347 in `apply.rs` - `remove_bind_state()` is called immediately after each destroy. If execution fails later, these state files are gone and we can't restore.

## Design Decisions

### 1. Defer bind state file deletion

Currently, `destroy_removed_binds()` calls `remove_bind_state()` immediately after each successful destroy. Instead:
- Track destroyed bind hashes in memory
- Only delete state files after the entire apply succeeds
- On failure, state files remain available for restore

### 2. Restore using DAG ordering with parallel execution

Destroyed binds must be restored in dependency order (same as original apply). The approach:
- Use the full `current_manifest` from the previous snapshot
- Build a DAG from this manifest via `ExecutionDag::from_manifest()`
- Get waves via `dag.execution_waves()`
- Filter each wave to only include destroyed bind hashes
- Execute filtered waves in parallel using `tokio::task::JoinSet` (matching existing patterns)

### 3. Placeholder resolution during restore

Binds may have `apply_actions` that reference other builds/binds via placeholders like `$${build:hash:out}` or `$${bind:hash:output}`.

**For restore, we build a resolver with:**
- **Builds**: Compute store paths from previous manifest's build definitions (builds are immutable in store)
- **All binds from previous manifest**: Load `BindState` files for both destroyed AND unchanged binds, convert to `BindResult` format

This ensures placeholders resolve correctly even when a destroyed bind depends on:
- Another destroyed bind (restored in an earlier wave)
- An unchanged bind (still has its state file)
- A build from the previous manifest (immutable in store)

### 4. Restore failure handling

If restore fails after execution failure:
- Log the restore error
- Clear the snapshot pointer (`SnapshotStore::clear_current()`)
- Return the original execution error
- Next `sys apply` will see no current snapshot and do a full apply (self-healing)

## Implementation

### File: `crates/lib/src/snapshot/storage.rs`

Add method to clear the current snapshot pointer:

```rust
/// Clear the current snapshot pointer without removing any snapshots.
/// Used when rollback fails and the system is in an inconsistent state.
/// Next apply will see no current state and do a full fresh apply.
pub fn clear_current(&self) -> Result<(), SnapshotError> {
    let mut index = self.load_index()?;
    index.current = None;
    self.save_index(&index)
}
```

### File: `crates/lib/src/execute/apply.rs`

#### 1. Add `DestroyPhaseError` struct

```rust
/// Error during the destroy phase, tracking partial progress for rollback.
struct DestroyPhaseError {
    /// Bind hashes that were successfully destroyed before the failure
    destroyed: Vec<ObjectHash>,
    /// The bind hash that failed to destroy
    failed_hash: ObjectHash,
    /// The underlying error
    source: ExecuteError,
}
```

#### 2. Modify `destroy_removed_binds()`

Current signature:
```rust
async fn destroy_removed_binds(
    hashes: &[ObjectHash],
    current_manifest: Option<&Manifest>,
    config: &ExecuteConfig,
) -> Result<usize, ApplyError>
```

New signature and behavior:
```rust
/// Destroy binds that were removed from the manifest.
/// Returns the list of successfully destroyed bind hashes.
/// Does NOT remove bind state files - caller must do this after successful apply.
async fn destroy_removed_binds(
    hashes: &[ObjectHash],
    current_manifest: Option<&Manifest>,
    config: &ExecuteConfig,
) -> Result<Vec<ObjectHash>, DestroyPhaseError> {
    // ... existing logic but:
    // 1. Track destroyed hashes in Vec instead of counting
    // 2. Remove the remove_bind_state() call (line 347)
    // 3. On error, return DestroyPhaseError with partial progress
}
```

#### 3. Add `cleanup_destroyed_bind_states()`

```rust
/// Remove bind state files for successfully destroyed binds.
/// Called only after apply fully succeeds.
fn cleanup_destroyed_bind_states(
    destroyed_hashes: &[ObjectHash],
    system: bool,
) -> Result<(), BindStateError> {
    for hash in destroyed_hashes {
        remove_bind_state(hash, system)?;
    }
    Ok(())
}
```

#### 4. Add `build_restore_resolver()` helper

```rust
/// Build a resolver for restore operations.
/// 
/// Loads bind state for all binds in the manifest (destroyed + unchanged)
/// and computes build store paths from the manifest.
fn build_restore_resolver(
    manifest: &Manifest,
    system: bool,
) -> Result<(HashMap<ObjectHash, BuildResult>, HashMap<ObjectHash, BindResult>), ApplyError> {
    let mut builds = HashMap::new();
    let mut binds = HashMap::new();
    
    // Compute BuildResult for each build (just need store_path and outputs)
    for (hash, build_def) in &manifest.builds {
        let store_path = build_path(&build_def.name, build_def.version.as_deref(), hash, system);
        // Load outputs from build's manifest file if it exists, or use definition outputs
        let outputs = build_def.outputs.clone().unwrap_or_default();
        builds.insert(hash.clone(), BuildResult {
            store_path,
            outputs,
            action_results: vec![],
        });
    }
    
    // Load BindState for each bind and convert to BindResult
    for hash in manifest.bindings.keys() {
        if let Some(state) = load_bind_state(hash, system)? {
            binds.insert(hash.clone(), BindResult {
                outputs: state.outputs,
                action_results: vec![],
            });
        }
    }
    
    Ok((builds, binds))
}
```

#### 5. Add `restore_destroyed_binds()`

```rust
/// Restore previously destroyed binds using DAG ordering from the manifest.
/// Uses parallel wave execution matching the normal apply flow.
/// This is a best-effort operation used during rollback.
async fn restore_destroyed_binds(
    destroyed_hashes: &[ObjectHash],
    manifest: &Manifest,
    config: &ExecuteConfig,
    system: bool,
) -> Result<(), ApplyError> {
    if destroyed_hashes.is_empty() {
        return Ok(());
    }
    
    info!(count = destroyed_hashes.len(), "restoring destroyed binds");
    
    let destroyed_set: HashSet<_> = destroyed_hashes.iter().collect();
    
    // Build resolver with all binds from previous manifest
    let (completed_builds, mut completed_binds) = build_restore_resolver(manifest, system)?;
    
    // Build DAG from the full manifest to get correct dependency ordering
    let dag = ExecutionDag::from_manifest(manifest)?;
    let waves = dag.execution_waves()?;
    
    // Create semaphore for parallelism control
    let semaphore = Arc::new(Semaphore::new(config.parallelism));
    
    for wave in waves {
        // Filter wave to only include destroyed binds
        let binds_to_restore: Vec<_> = wave
            .into_iter()
            .filter_map(|node| match node {
                DagNode::Bind(hash) if destroyed_set.contains(&hash) => {
                    manifest.bindings.get(&hash).map(|def| (hash, def.clone()))
                }
                _ => None,
            })
            .collect();
        
        if binds_to_restore.is_empty() {
            continue;
        }
        
        // Execute in parallel within wave using JoinSet
        let mut join_set = JoinSet::new();
        
        for (hash, bind_def) in binds_to_restore {
            let hash = hash.clone();
            let bind_def = bind_def.clone();
            let config = config.clone();
            let completed_builds = completed_builds.clone();
            let completed_binds = completed_binds.clone();
            let semaphore = semaphore.clone();
            let manifest = manifest.clone();
            
            join_set.spawn(async move {
                let _permit = semaphore.acquire().await.unwrap();
                
                let resolver = ExecutionResolver::new(
                    &completed_builds,
                    &completed_binds,
                    &manifest,
                    "/tmp", // apply_bind creates its own working dir
                    system,
                );
                
                let result = apply_bind(&hash, &bind_def, &resolver, &config).await?;
                
                // Save bind state
                let bind_state = BindState::new(result.outputs.clone());
                save_bind_state(&hash, &bind_state, system)?;
                
                Ok::<_, ApplyError>((hash, result))
            });
        }
        
        // Collect results and update completed_binds for next wave
        while let Some(join_result) = join_set.join_next().await {
            match join_result {
                Ok(Ok((hash, result))) => {
                    info!(bind = %hash.0, "bind restored");
                    completed_binds.insert(hash, result);
                }
                Ok(Err(e)) => {
                    error!(error = %e, "failed to restore bind");
                    return Err(e);
                }
                Err(e) => {
                    error!(error = %e, "restore task panicked");
                    return Err(ApplyError::Execute(ExecuteError::CmdFailed {
                        cmd: "restore".to_string(),
                        code: None,
                    }));
                }
            }
        }
    }
    
    info!("restore complete");
    Ok(())
}
```

#### 6. Refactor `apply()` orchestration

Key changes to the existing `apply()` function:

```rust
pub async fn apply(config_path: &Path, options: &ApplyOptions) -> Result<ApplyResult, ApplyError> {
    // ... existing setup ...
    
    // Capture previous snapshot ID for potential rollback
    let previous_snapshot_id = snapshot_store.current_id()?;
    
    // ... existing diff computation ...
    
    // Phase 1: Destroy removed binds (deferred state file cleanup)
    let destroyed_hashes = match destroy_removed_binds(
        &diff.binds_to_destroy,
        current_manifest.as_ref(),
        &options.execute,
    ).await {
        Ok(hashes) => hashes,
        Err(destroy_err) => {
            // Partial destroy failure - restore what we destroyed
            if !destroy_err.destroyed.is_empty() {
                if let Some(ref prev_manifest) = current_manifest {
                    let _ = restore_destroyed_binds(
                        &destroy_err.destroyed,
                        prev_manifest,
                        &options.execute,
                        options.system,
                    ).await;
                }
            }
            return Err(ApplyError::DestroyFailed {
                hash: destroy_err.failed_hash,
                source: destroy_err.source,
            });
        }
    };
    
    // Phase 2: Execute new manifest (builds + binds)
    let dag_result = execute_manifest(&execution_manifest, &options.execute).await?;
    
    if !dag_result.is_success() {
        // Execution failed - restore destroyed binds
        if !destroyed_hashes.is_empty() {
            if let Some(ref prev_manifest) = current_manifest {
                match restore_destroyed_binds(
                    &destroyed_hashes,
                    prev_manifest,
                    &options.execute,
                    options.system,
                ).await {
                    Ok(_) => {
                        // Restore succeeded - point snapshot back to previous
                        if let Some(ref prev_id) = previous_snapshot_id {
                            let _ = snapshot_store.set_current(prev_id);
                        }
                    }
                    Err(restore_err) => {
                        // Restore failed - clear snapshot for self-healing
                        error!(
                            error = %restore_err,
                            "failed to restore destroyed binds, clearing snapshot pointer"
                        );
                        let _ = snapshot_store.clear_current();
                    }
                }
            }
        }
        
        // Return the execution error
        return Err(ApplyError::Execute(ExecuteError::CmdFailed {
            cmd: "apply".to_string(),
            code: Some(1),
        }));
    }
    
    // Phase 3: Success - cleanup destroyed bind states, save new states, save snapshot
    cleanup_destroyed_bind_states(&destroyed_hashes, options.system)?;
    
    // ... existing bind state saving and snapshot saving ...
}
```

### Error Types

Add new variant to the existing `ApplyError` enum:

```rust
#[derive(Debug, Error)]
pub enum ApplyError {
    // ... existing variants ...
    
    /// Failed to restore a bind during rollback.
    #[error("failed to restore bind {hash} during rollback: {source}")]
    RestoreFailed {
        hash: ObjectHash,
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },
}
```

## Test Cases

| Test | Scenario | Expected Outcome |
|------|----------|------------------|
| `apply_restores_destroyed_binds_on_execution_failure` | Destroy succeeds, execute fails | Destroyed binds restored, previous snapshot current |
| `apply_restores_on_partial_destroy_failure` | Destroy A succeeds, destroy B fails | A restored, error returned |
| `apply_restores_dependent_binds_in_order` | A→B destroyed, execute fails | A restored first, then B |
| `apply_success_removes_destroyed_bind_states` | Full success | Destroyed bind state files removed |
| `apply_clears_snapshot_on_restore_failure` | Execute fails, restore fails | Snapshot pointer cleared |
| `apply_preserves_builds_on_failure` | Build succeeds, bind fails | Build remains in store |
| `apply_restore_resolves_bind_placeholders` | Bind A uses `$${bind:B:output}` | Placeholder resolves from loaded state |

## Implementation Order

1. Add `clear_current()` to `SnapshotStore` (small, independent)
2. Add `RestoreFailed` variant to `ApplyError`
3. Add `DestroyPhaseError` struct
4. Modify `destroy_removed_binds()` to return `Vec<ObjectHash>` and not delete state files
5. Add `cleanup_destroyed_bind_states()`
6. Add `build_restore_resolver()` helper
7. Add `restore_destroyed_binds()`
8. Refactor `apply()` orchestration
9. Add tests

## Resolved Questions

1. **Error handling granularity**: Add `RestoreFailed` as a new variant to `ApplyError`.

2. **Snapshot storage API**: Confirmed - `SnapshotStore` has `load_index()`, `save_index()`, `set_current()`, `current_id()`.

3. **Build state preservation**: Builds don't need rollback - they're immutable in the store. A re-apply will rebuild them if they don't exist or were GC'ed.

4. **Testing infrastructure**: Use temp directories for real filesystem operations. Reference existing tests for patterns.

5. **Parallel restore**: Use same parallelism (wave-based parallel execution with `JoinSet`).

6. **Logging verbosity**: Log at info level for restore start/success, error level for failures.

7. **Metrics/telemetry**: Deferred - not in initial implementation.

8. **Async execution**: All functions are async. The implementation uses `tokio::task::JoinSet` for parallel execution.

9. **Placeholder resolution during restore**: Build a resolver that includes:
   - All builds from previous manifest (compute store paths)
   - All binds from previous manifest (load from BindState files)
   
   This ensures placeholders resolve correctly for both destroyed and unchanged dependencies.

10. **Reusing existing code**: Create new `restore_destroyed_binds` function (option A) rather than refactoring existing wave execution. Can refactor later if needed.

## Future Considerations

- **Dry-run rollback verification**: Before apply, verify that removed binds can be restored
- **Partial success mode**: Option to keep successful changes even if some fail
- **Rollback history**: Track rollback events for debugging
- **Refactor wave execution**: Extract common wave execution logic into reusable helper
