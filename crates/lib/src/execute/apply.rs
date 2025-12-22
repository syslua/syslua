//! Apply orchestration for syslua.
//!
//! This module provides the high-level `apply` function that orchestrates
//! the full apply flow:
//!
//! 1. Load current state
//! 2. Evaluate config to produce desired manifest
//! 3. Compute diff between desired and current
//! 4. Destroy removed binds
//! 5. Update modified binds (same ID, different content)
//! 6. Realize new builds
//! 7. Apply new binds
//! 8. Save new snapshot
//!
//! On failure, rolls back any applied binds from this run (except updates).

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use thiserror::Error;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tracing::{debug, error, info, warn};

use crate::bind::execute::{apply_bind, destroy_bind, update_bind};
use crate::bind::state::{BindState, BindStateError, load_bind_state, remove_bind_state, save_bind_state};
use crate::build::store::build_dir_path;
use crate::eval::{EvalError, evaluate_config};
use crate::execute::execute_manifest;
use crate::manifest::Manifest;
use crate::snapshot::{Snapshot, SnapshotError, SnapshotStore, StateDiff, compute_diff, generate_snapshot_id};
use crate::store::paths::StorePaths;
use crate::util::hash::ObjectHash;

use super::dag::{DagNode, ExecutionDag};
use super::resolver::ExecutionResolver;
use super::types::{BindResult, BuildResult, DagResult, ExecuteConfig, ExecuteError};

/// Result of an apply operation.
#[derive(Debug)]
pub struct ApplyResult {
  /// The snapshot that was created.
  pub snapshot: Snapshot,

  /// Diff that was applied.
  pub diff: StateDiff,

  /// Execution result details.
  pub execution: DagResult,

  /// Number of binds that were destroyed (removed from previous state).
  pub binds_destroyed: usize,

  /// Number of binds that were updated (same ID, different content).
  pub binds_updated: usize,
}

/// Errors that can occur during apply.
#[derive(Debug, Error)]
pub enum ApplyError {
  /// Config evaluation failed.
  #[error("evaluation error: {0}")]
  Eval(#[from] EvalError),

  /// Snapshot storage failed.
  #[error("snapshot error: {0}")]
  Snapshot(#[from] SnapshotError),

  /// Execution failed.
  #[error("execution error: {0}")]
  Execute(#[from] ExecuteError),

  /// Bind state persistence failed.
  #[error("bind state error: {0}")]
  BindState(#[from] BindStateError),

  /// Config file not found.
  #[error("config file not found: {0}")]
  ConfigNotFound(PathBuf),

  /// Destroy phase failed.
  #[error("failed to destroy bind {hash}: {source}")]
  DestroyFailed {
    hash: ObjectHash,
    #[source]
    source: ExecuteError,
  },

  /// Restore phase failed during rollback.
  #[error("failed to restore bind {hash} during rollback: {source}")]
  RestoreFailed {
    hash: ObjectHash,
    #[source]
    source: Box<dyn std::error::Error + Send + Sync>,
  },

  /// Update phase failed.
  #[error("failed to update bind {old_hash} -> {new_hash}: {source}")]
  UpdateFailed {
    old_hash: ObjectHash,
    new_hash: ObjectHash,
    #[source]
    source: ExecuteError,
  },
}

/// Error during the destroy phase, tracking partial progress for rollback.
///
/// This is used internally to track which binds were successfully destroyed
/// before a failure occurred, enabling restoration of those binds.
#[derive(Debug)]
struct DestroyPhaseError {
  /// Bind hashes that were successfully destroyed before the failure.
  destroyed: Vec<ObjectHash>,
  /// The bind hash that failed to destroy.
  failed_hash: ObjectHash,
  /// The underlying error.
  source: ExecuteError,
}

/// Options for the apply operation.
#[derive(Debug, Clone, Default)]
pub struct ApplyOptions {
  /// Execution configuration (parallelism, shell, etc.)
  pub execute: ExecuteConfig,

  /// Whether to use the system store (vs user store).
  pub system: bool,

  /// Dry run mode - compute diff but don't apply.
  pub dry_run: bool,
}

/// Options for the destroy operation.
#[derive(Debug, Clone, Default)]
pub struct DestroyOptions {
  /// Execution configuration (parallelism, etc.)
  pub execute: ExecuteConfig,

  /// Whether to use the system store (vs user store).
  pub system: bool,

  /// Dry run mode - show what would be destroyed without making changes.
  pub dry_run: bool,
}

/// Result of a destroy operation.
#[derive(Debug)]
pub struct DestroyResult {
  /// Number of binds that were destroyed.
  pub binds_destroyed: usize,

  /// Number of builds now orphaned (left for future GC).
  pub builds_orphaned: usize,
}

/// Apply a configuration file.
///
/// This is the main entry point for `sys apply`. It:
/// 1. Loads current state (if any)
/// 2. Evaluates the config to produce desired manifest
/// 3. Computes diff between desired and current
/// 4. Destroys removed binds
/// 5. Realizes new builds
/// 6. Applies new binds
/// 7. Saves new snapshot
///
/// On failure, rolls back any applied binds from this run.
///
/// # Arguments
///
/// * `config_path` - Path to the Lua configuration file
/// * `options` - Apply options
///
/// # Returns
///
/// An [`ApplyResult`] containing the new snapshot and execution details.
pub async fn apply(config_path: &Path, options: &ApplyOptions) -> Result<ApplyResult, ApplyError> {
  info!(config = %config_path.display(), "starting apply");

  // Validate config exists
  if !config_path.exists() {
    return Err(ApplyError::ConfigNotFound(config_path.to_path_buf()));
  }

  // 1. Load current state
  let snapshot_store = SnapshotStore::default_store(&options.system);
  let current_snapshot = snapshot_store.load_current()?;
  let current_manifest = current_snapshot.as_ref().map(|s| &s.manifest);

  // Capture previous snapshot ID for potential rollback
  let previous_snapshot_id = snapshot_store.current_id()?;

  info!(has_current = current_snapshot.is_some(), "loaded current state");

  // 2. Evaluate config to produce desired manifest
  info!("evaluating config");
  let desired_manifest = evaluate_config(config_path)?;

  info!(
    builds = desired_manifest.builds.len(),
    binds = desired_manifest.bindings.len(),
    "config evaluated"
  );

  // 3. Compute diff
  let store_path = if options.system {
    StorePaths::system_store_path()
  } else {
    StorePaths::user_store_path()
  };
  let diff = compute_diff(&desired_manifest, current_manifest, &store_path);

  info!(
    builds_to_realize = diff.builds_to_realize.len(),
    builds_cached = diff.builds_cached.len(),
    binds_to_apply = diff.binds_to_apply.len(),
    binds_to_update = diff.binds_to_update.len(),
    binds_to_destroy = diff.binds_to_destroy.len(),
    binds_unchanged = diff.binds_unchanged.len(),
    "diff computed"
  );

  // Early exit if no changes
  if diff.is_empty() {
    info!("no changes to apply");

    // Still create a snapshot to record the state
    let snapshot = Snapshot::new(
      generate_snapshot_id(),
      Some(config_path.to_path_buf()),
      desired_manifest,
    );

    // Save snapshot and set as current
    snapshot_store.save_and_set_current(&snapshot)?;

    return Ok(ApplyResult {
      snapshot,
      diff,
      execution: DagResult::default(),
      binds_destroyed: 0,
      binds_updated: 0,
    });
  }

  // Dry run - return without making changes
  if options.dry_run {
    info!("dry run - not applying changes");
    return Ok(ApplyResult {
      snapshot: Snapshot::new("dry-run".to_string(), Some(config_path.to_path_buf()), desired_manifest),
      diff,
      execution: DagResult::default(),
      binds_destroyed: 0,
      binds_updated: 0,
    });
  }

  // 4. Destroy removed binds (state file cleanup is deferred until success)
  let destroyed_hashes = match destroy_removed_binds(&diff.binds_to_destroy, current_manifest, &options.execute).await {
    Ok(hashes) => hashes,
    Err(destroy_err) => {
      // Partial destroy failure - restore what we destroyed
      if !destroy_err.destroyed.is_empty()
        && let Some(ref current_snapshot) = current_snapshot
      {
        let _ = restore_destroyed_binds(
          &destroy_err.destroyed,
          &current_snapshot.manifest,
          &options.execute,
          options.system,
        )
        .await;
      }
      return Err(ApplyError::DestroyFailed {
        hash: destroy_err.failed_hash,
        source: destroy_err.source,
      });
    }
  };

  // 5. Update modified binds (no rollback on failure - just fail with error)
  let updated_hashes = update_modified_binds(
    &diff.binds_to_update,
    current_manifest,
    &desired_manifest,
    &options.execute,
  )
  .await?;

  // 6 & 7. Build execution manifest and execute (realize builds, apply new binds)
  // Filter to only include builds that need realization and binds that need applying
  let execution_manifest = build_execution_manifest(&desired_manifest, &diff);

  info!(
    builds = execution_manifest.builds.len(),
    binds = execution_manifest.bindings.len(),
    "executing manifest"
  );

  let dag_result = execute_manifest(&execution_manifest, &options.execute).await?;

  // Check for failures
  if !dag_result.is_success() {
    // Log the failure details
    error!("execution failed");

    if let Some((hash, ref err)) = dag_result.build_failed {
      error!(build = %hash.0, error = %err, "build failed");
    }
    if let Some((hash, ref err)) = dag_result.bind_failed {
      error!(bind = %hash.0, error = %err, "bind failed");
    }

    // Execution failed - restore destroyed binds
    if !destroyed_hashes.is_empty()
      && let Some(ref current_snapshot) = current_snapshot
    {
      match restore_destroyed_binds(
        &destroyed_hashes,
        &current_snapshot.manifest,
        &options.execute,
        options.system,
      )
      .await
      {
        Ok(_) => {
          // Restore succeeded - point snapshot back to previous
          if let Some(ref prev_id) = previous_snapshot_id {
            let _ = snapshot_store.set_current(prev_id);
            info!(snapshot_id = %prev_id, "restored previous snapshot");
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

    // Return the execution error
    return Err(ApplyError::Execute(ExecuteError::CmdFailed {
      cmd: "apply".to_string(),
      code: Some(1),
    }));
  }

  // Save bind state for newly applied binds
  for (hash, result) in &dag_result.applied {
    let bind_state = BindState::new(result.outputs.clone());
    save_bind_state(hash, &bind_state, options.system)?;
    debug!(bind = %hash.0, "saved bind state");
  }

  // Clean up state files for destroyed binds (only after full success)
  cleanup_destroyed_bind_states(&destroyed_hashes, options.system)?;

  // 7. Create and save snapshot
  let snapshot = Snapshot::new(
    generate_snapshot_id(),
    Some(config_path.to_path_buf()),
    desired_manifest,
  );

  snapshot_store.save_and_set_current(&snapshot)?;
  info!(snapshot_id = %snapshot.id, "snapshot saved");

  Ok(ApplyResult {
    snapshot,
    diff,
    execution: dag_result,
    binds_destroyed: destroyed_hashes.len(),
    binds_updated: updated_hashes.len(),
  })
}

/// Destroy all binds from the current snapshot.
///
/// This is the main entry point for `sys destroy`. It:
/// 1. Loads current snapshot (if any)
/// 2. Returns early with success if no current snapshot exists (idempotent)
/// 3. Destroys all binds from the snapshot in reverse dependency order
/// 4. Cleans up bind state files
/// 5. Clears the current snapshot pointer
///
/// # Arguments
///
/// * `options` - Destroy options
///
/// # Returns
///
/// A [`DestroyResult`] containing counts of destroyed binds and orphaned builds.
pub async fn destroy(options: &DestroyOptions) -> Result<DestroyResult, ApplyError> {
  info!(system = options.system, dry_run = options.dry_run, "starting destroy");

  // 1. Load current state
  let snapshot_store = SnapshotStore::default_store(&options.system);
  debug!(snapshot_store_path = ?snapshot_store.base_path(), "using snapshot store");
  let current_snapshot = snapshot_store.load_current()?;

  // 2. Early exit if no current snapshot (idempotent)
  let snapshot = match current_snapshot {
    Some(s) => s,
    None => {
      info!("no current snapshot, nothing to destroy");
      return Ok(DestroyResult {
        binds_destroyed: 0,
        builds_orphaned: 0,
      });
    }
  };

  let manifest = &snapshot.manifest;
  let bind_count = manifest.bindings.len();
  let build_count = manifest.builds.len();

  info!(
    binds = bind_count,
    builds = build_count,
    snapshot_id = %snapshot.id,
    "loaded current snapshot"
  );

  // Early exit if no binds to destroy
  if bind_count == 0 {
    info!("no binds to destroy");
    snapshot_store.clear_current()?;
    return Ok(DestroyResult {
      binds_destroyed: 0,
      builds_orphaned: build_count,
    });
  }

  // Dry run - return without making changes
  if options.dry_run {
    info!("dry run - not destroying");
    return Ok(DestroyResult {
      binds_destroyed: bind_count,
      builds_orphaned: build_count,
    });
  }

  // 3. Get all bind hashes from the manifest
  let bind_hashes: Vec<ObjectHash> = manifest.bindings.keys().cloned().collect();

  // 4. Destroy all binds
  // We use destroy_removed_binds which handles:
  // - Loading bind state for each bind
  // - Creating the resolver for destroy actions
  // - Executing destroy_actions with proper error handling
  // - Returning which binds were destroyed
  let destroyed_hashes = match destroy_removed_binds(&bind_hashes, Some(manifest), &options.execute).await {
    Ok(hashes) => hashes,
    Err(destroy_err) => {
      // Partial failure - some binds destroyed, one failed
      // We don't restore here (unlike apply) - user can retry destroy
      error!(
        failed_hash = %destroy_err.failed_hash.0,
        destroyed_count = destroy_err.destroyed.len(),
        error = %destroy_err.source,
        "destroy failed partway through"
      );

      // Clean up state files for binds that were successfully destroyed
      if let Err(e) = cleanup_destroyed_bind_states(&destroy_err.destroyed, options.system) {
        warn!(error = %e, "failed to clean up some bind state files");
      }

      return Err(ApplyError::DestroyFailed {
        hash: destroy_err.failed_hash,
        source: destroy_err.source,
      });
    }
  };

  // 5. Clean up bind state files
  cleanup_destroyed_bind_states(&destroyed_hashes, options.system)?;

  // 6. Clear the current snapshot pointer
  snapshot_store.clear_current()?;
  info!(binds_destroyed = destroyed_hashes.len(), "destroy complete");

  Ok(DestroyResult {
    binds_destroyed: destroyed_hashes.len(),
    builds_orphaned: build_count,
  })
}

/// Build an execution manifest containing only items that need work.
///
/// Filters the desired manifest to include:
/// - Only builds that need to be realized (not cached)
/// - Only binds that need to be applied (new in desired)
fn build_execution_manifest(desired: &Manifest, diff: &StateDiff) -> Manifest {
  let mut manifest = Manifest::default();

  // Include builds that need realization
  for hash in &diff.builds_to_realize {
    if let Some(build_def) = desired.builds.get(hash) {
      manifest.builds.insert(hash.clone(), build_def.clone());
    }
  }

  // Also include cached builds (needed for bind placeholder resolution)
  for hash in &diff.builds_cached {
    if let Some(build_def) = desired.builds.get(hash) {
      manifest.builds.insert(hash.clone(), build_def.clone());
    }
  }

  // Include binds that need applying
  for hash in &diff.binds_to_apply {
    if let Some(bind_def) = desired.bindings.get(hash) {
      manifest.bindings.insert(hash.clone(), bind_def.clone());
    }
  }

  manifest
}

/// Destroy removed binds.
///
/// Executes destroy_actions for binds that are in the current state
/// but not in the desired state.
///
/// # Returns
///
/// List of bind hashes that were successfully destroyed.
/// Does NOT remove bind state files - caller must do this after successful apply.
async fn destroy_removed_binds(
  hashes: &[ObjectHash],
  current_manifest: Option<&Manifest>,
  config: &ExecuteConfig,
) -> Result<Vec<ObjectHash>, DestroyPhaseError> {
  if hashes.is_empty() {
    return Ok(Vec::new());
  }

  info!(count = hashes.len(), system = config.system, "destroying removed binds");
  debug!(bind_hashes = ?hashes.iter().map(|h| &h.0).collect::<Vec<_>>(), "binds to destroy");

  let mut destroyed = Vec::new();

  // Create an empty resolver for destroy operations
  // (destroy actions typically only need outputs from the bind itself)
  let empty_builds: HashMap<ObjectHash, BuildResult> = HashMap::new();
  let empty_binds: HashMap<ObjectHash, BindResult> = HashMap::new();
  let empty_manifest = Manifest::default();
  let resolver = ExecutionResolver::new(
    &empty_builds,
    &empty_binds,
    &empty_manifest,
    "/tmp".to_string(),
    config.system,
  );

  // Log the bind state directory for debugging
  let bind_store_path = crate::store::paths::StorePaths::user_store_path().join("bind");
  debug!(bind_store_path = ?bind_store_path, "checking bind state directory");

  for hash in hashes {
    // Log the expected bind state path
    let bind_state_path = crate::bind::store::bind_dir_path(hash, config.system);
    debug!(bind = %hash.0, bind_state_path = ?bind_state_path, "looking for bind state");

    // Load bind state (outputs from when it was applied)
    let bind_state = match load_bind_state(hash, config.system) {
      Ok(Some(state)) => {
        debug!(bind = %hash.0, outputs = ?state.outputs, "loaded bind state");
        state
      }
      Ok(None) => {
        warn!(bind = %hash.0, bind_state_path = ?bind_state_path, "no bind state found, skipping destroy");
        continue;
      }
      Err(e) => {
        error!(bind = %hash.0, error = %e, "failed to load bind state");
        return Err(DestroyPhaseError {
          destroyed,
          failed_hash: hash.clone(),
          source: ExecuteError::CmdFailed {
            cmd: format!("load bind state for {}", hash.0),
            code: None,
          },
        });
      }
    };

    // Get bind definition from current manifest
    let bind_def = match current_manifest.and_then(|m| m.bindings.get(hash)) {
      Some(def) => {
        debug!(
          bind = %hash.0,
          destroy_actions_count = def.destroy_actions.len(),
          "found bind definition"
        );
        def
      }
      None => {
        warn!(bind = %hash.0, "bind definition not found in current manifest, skipping");
        continue;
      }
    };

    // Create a bind result from the saved state
    let bind_result = BindResult {
      outputs: bind_state.outputs.clone(),
      action_results: vec![],
    };

    // Execute destroy
    info!(bind = %hash.0, destroy_actions = bind_def.destroy_actions.len(), "destroying bind");
    if let Err(e) = destroy_bind(hash, bind_def, &bind_result, &resolver).await {
      error!(bind = %hash.0, error = %e, "failed to destroy bind");
      return Err(DestroyPhaseError {
        destroyed,
        failed_hash: hash.clone(),
        source: e,
      });
    }

    // Track successful destruction (state file cleanup is deferred)
    destroyed.push(hash.clone());
    info!(bind = %hash.0, "bind destroyed successfully");
  }

  info!(count = destroyed.len(), "destroy phase complete");
  Ok(destroyed)
}

/// Remove bind state files for successfully destroyed binds.
///
/// This is called only after apply fully succeeds, to clean up state files
/// for binds that were destroyed and whose state is no longer needed.
fn cleanup_destroyed_bind_states(destroyed_hashes: &[ObjectHash], system: bool) -> Result<(), BindStateError> {
  for hash in destroyed_hashes {
    remove_bind_state(hash, system)?;
  }
  Ok(())
}

/// Update modified binds.
///
/// For each (old_hash, new_hash) pair in binds_to_update:
/// 1. Load old bind state (outputs from previous apply)
/// 2. Get new bind definition from desired manifest
/// 3. Call update_bind()
/// 4. On success: save new bind state, remove old state if hash changed
/// 5. On failure: log error and return error (no rollback)
///
/// # Arguments
///
/// * `updates` - List of (old_hash, new_hash) pairs to update
/// * `current` - Current manifest (to get old bind definitions if needed)
/// * `desired` - Desired manifest (to get new bind definitions)
/// * `config` - Execution configuration
///
/// # Returns
///
/// List of new hashes that were successfully updated.
async fn update_modified_binds(
  updates: &[(ObjectHash, ObjectHash)],
  _current: Option<&Manifest>,
  desired: &Manifest,
  config: &ExecuteConfig,
) -> Result<Vec<ObjectHash>, ApplyError> {
  if updates.is_empty() {
    return Ok(Vec::new());
  }

  info!(count = updates.len(), "updating modified binds");

  let mut updated = Vec::new();

  // Build resolver data for placeholder resolution during update
  // We need access to builds and existing binds for placeholder resolution
  let (completed_builds, completed_binds) = build_restore_resolver_data(desired, config.system)?;

  for (old_hash, new_hash) in updates {
    // Load old bind state (outputs from when it was originally applied)
    let old_bind_state = match load_bind_state(old_hash, config.system) {
      Ok(Some(state)) => state,
      Ok(None) => {
        error!(old_hash = %old_hash.0, "no bind state found for update, cannot proceed");
        return Err(ApplyError::UpdateFailed {
          old_hash: old_hash.clone(),
          new_hash: new_hash.clone(),
          source: ExecuteError::CmdFailed {
            cmd: format!("load bind state for {}", old_hash.0),
            code: None,
          },
        });
      }
      Err(e) => {
        error!(old_hash = %old_hash.0, error = %e, "failed to load bind state for update");
        return Err(ApplyError::UpdateFailed {
          old_hash: old_hash.clone(),
          new_hash: new_hash.clone(),
          source: ExecuteError::CmdFailed {
            cmd: format!("load bind state for {}", old_hash.0),
            code: None,
          },
        });
      }
    };

    // Get new bind definition from desired manifest
    let new_bind_def = match desired.bindings.get(new_hash) {
      Some(def) => def,
      None => {
        error!(new_hash = %new_hash.0, "bind definition not found in desired manifest");
        return Err(ApplyError::UpdateFailed {
          old_hash: old_hash.clone(),
          new_hash: new_hash.clone(),
          source: ExecuteError::CmdFailed {
            cmd: format!("find bind definition for {}", new_hash.0),
            code: None,
          },
        });
      }
    };

    // Create resolver for update
    let resolver = ExecutionResolver::new(
      &completed_builds,
      &completed_binds,
      desired,
      "/tmp".to_string(),
      config.system,
    );

    // Create old bind result from saved state
    let old_bind_result = BindResult {
      outputs: old_bind_state.outputs.clone(),
      action_results: vec![],
    };

    // Execute update
    info!(old_hash = %old_hash.0, new_hash = %new_hash.0, "updating bind");
    let update_result = match update_bind(old_hash, new_hash, new_bind_def, &old_bind_result, &resolver).await {
      Ok(result) => result,
      Err(e) => {
        error!(old_hash = %old_hash.0, new_hash = %new_hash.0, error = %e, "failed to update bind");
        return Err(ApplyError::UpdateFailed {
          old_hash: old_hash.clone(),
          new_hash: new_hash.clone(),
          source: e,
        });
      }
    };

    // Save new bind state
    let new_bind_state = BindState::new(update_result.outputs.clone());
    save_bind_state(new_hash, &new_bind_state, config.system)?;

    // Remove old bind state if hash changed
    if old_hash != new_hash {
      remove_bind_state(old_hash, config.system)?;
    }

    updated.push(new_hash.clone());
    debug!(old_hash = %old_hash.0, new_hash = %new_hash.0, "bind updated");
  }

  info!(count = updated.len(), "update phase complete");
  Ok(updated)
}

/// Build resolver data for restore operations.
///
/// Loads bind state for all binds in the manifest (destroyed + unchanged)
/// and computes build store paths from the manifest. This allows placeholder
/// resolution during restore (e.g., `$${build:hash:out}`, `$${bind:hash:output}`).
fn build_restore_resolver_data(
  manifest: &Manifest,
  system: bool,
) -> Result<(HashMap<ObjectHash, BuildResult>, HashMap<ObjectHash, BindResult>), ApplyError> {
  let mut builds = HashMap::new();
  let mut binds = HashMap::new();

  // Compute BuildResult for each build (just need store_path and outputs)
  for (hash, build_def) in &manifest.builds {
    let store_path = build_dir_path(hash, system);

    // Resolve outputs - for now use the definition's output patterns
    // In practice, builds in the store should have their outputs already resolved
    let mut outputs = HashMap::new();
    if let Some(def_outputs) = &build_def.outputs {
      for (name, pattern) in def_outputs {
        // Simple substitution of $${out} with store_path
        let resolved = pattern.replace("$${out}", store_path.to_string_lossy().as_ref());
        outputs.insert(name.clone(), resolved);
      }
    }
    // Always add "out" pointing to store path
    outputs.insert("out".to_string(), store_path.to_string_lossy().to_string());

    builds.insert(
      hash.clone(),
      BuildResult {
        store_path,
        outputs,
        action_results: vec![],
      },
    );
  }

  // Load BindState for each bind and convert to BindResult
  for hash in manifest.bindings.keys() {
    if let Some(state) = load_bind_state(hash, system)? {
      binds.insert(
        hash.clone(),
        BindResult {
          outputs: state.outputs,
          action_results: vec![],
        },
      );
    }
  }

  Ok((builds, binds))
}

/// Restore previously destroyed binds using DAG ordering from the manifest.
///
/// Uses parallel wave execution matching the normal apply flow.
/// This is a best-effort operation used during rollback.
///
/// # Arguments
///
/// * `destroyed_hashes` - Hashes of binds that were destroyed and need restoration
/// * `manifest` - The previous manifest (from the snapshot before apply started)
/// * `config` - Execution configuration
/// * `system` - Whether to use system store
///
/// # Returns
///
/// Ok(()) on success, or an error if any bind fails to restore.
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

  // Build resolver data with all binds from previous manifest
  let (completed_builds, mut completed_binds) = build_restore_resolver_data(manifest, system)?;

  // Build DAG from the full manifest to get correct dependency ordering
  let dag = ExecutionDag::from_manifest(manifest)?;
  let waves = dag.execution_waves()?;

  // Create semaphore for parallelism control
  let semaphore = Arc::new(Semaphore::new(config.parallelism));

  for (wave_idx, wave) in waves.iter().enumerate() {
    // Filter wave to only include destroyed binds
    let binds_to_restore: Vec<_> = wave
      .iter()
      .filter_map(|node| match node {
        DagNode::Bind(hash) if destroyed_set.contains(hash) => {
          manifest.bindings.get(hash).map(|def| (hash.clone(), def.clone()))
        }
        _ => None,
      })
      .collect();

    if binds_to_restore.is_empty() {
      continue;
    }

    debug!(wave = wave_idx, count = binds_to_restore.len(), "restoring wave");

    // Execute in parallel within wave using JoinSet
    let mut join_set: JoinSet<Result<(ObjectHash, BindResult), ApplyError>> = JoinSet::new();

    for (hash, bind_def) in binds_to_restore {
      let hash = hash.clone();
      let bind_def = bind_def.clone();
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
          "/tmp".to_string(),
          system,
        );

        let result = apply_bind(&hash, &bind_def, &resolver)
          .await
          .map_err(|e| ApplyError::RestoreFailed {
            hash: hash.clone(),
            source: Box::new(e),
          })?;

        // Save bind state
        let bind_state = BindState::new(result.outputs.clone());
        save_bind_state(&hash, &bind_state, system).map_err(|e| ApplyError::RestoreFailed {
          hash: hash.clone(),
          source: Box::new(e),
        })?;

        Ok((hash, result))
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
          return Err(ApplyError::RestoreFailed {
            hash: ObjectHash("unknown".to_string()),
            source: Box::new(std::io::Error::other(e.to_string())),
          });
        }
      }
    }
  }

  info!("restore complete");
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;
  use serial_test::serial;
  use tempfile::TempDir;

  fn test_options() -> ApplyOptions {
    ApplyOptions {
      execute: ExecuteConfig {
        parallelism: 1,
        system: false,
      },
      system: false,
      dry_run: false,
    }
  }

  /// Helper to set up a temp store environment for tests.
  fn with_temp_env<F, R>(f: F) -> R
  where
    F: FnOnce(&TempDir) -> R,
  {
    let temp_dir = TempDir::new().unwrap();
    temp_env::with_vars(
      [
        (
          "SYSLUA_USER_STORE",
          Some(temp_dir.path().join("store").to_str().unwrap()),
        ),
        ("XDG_DATA_HOME", Some(temp_dir.path().join("data").to_str().unwrap())),
      ],
      || f(&temp_dir),
    )
  }

  #[test]
  fn build_execution_manifest_filters_correctly() {
    use crate::bind::BindDef;
    use crate::build::BuildDef;

    let mut desired = Manifest::default();

    // Add builds
    desired.builds.insert(
      ObjectHash("cached".to_string()),
      BuildDef {
        id: None,
        inputs: None,
        create_actions: vec![],
        outputs: None,
      },
    );
    desired.builds.insert(
      ObjectHash("new".to_string()),
      BuildDef {
        id: None,
        inputs: None,
        create_actions: vec![],
        outputs: None,
      },
    );

    // Add binds
    desired.bindings.insert(
      ObjectHash("new_bind".to_string()),
      BindDef {
        id: None,
        inputs: None,
        outputs: None,
        create_actions: vec![],
        update_actions: None,
        destroy_actions: vec![],
      },
    );
    desired.bindings.insert(
      ObjectHash("unchanged_bind".to_string()),
      BindDef {
        id: None,
        inputs: None,
        outputs: None,
        create_actions: vec![],
        update_actions: None,
        destroy_actions: vec![],
      },
    );

    let diff = StateDiff {
      builds_to_realize: vec![ObjectHash("new".to_string())],
      builds_cached: vec![ObjectHash("cached".to_string())],
      binds_to_apply: vec![ObjectHash("new_bind".to_string())],
      binds_to_destroy: vec![],
      binds_unchanged: vec![ObjectHash("unchanged_bind".to_string())],
      binds_to_update: vec![],
    };

    let exec_manifest = build_execution_manifest(&desired, &diff);

    // Should include both builds (cached needed for resolution)
    assert_eq!(exec_manifest.builds.len(), 2);
    assert!(exec_manifest.builds.contains_key(&ObjectHash("new".to_string())));
    assert!(exec_manifest.builds.contains_key(&ObjectHash("cached".to_string())));

    // Should only include new binds
    assert_eq!(exec_manifest.bindings.len(), 1);
    assert!(exec_manifest.bindings.contains_key(&ObjectHash("new_bind".to_string())));
    assert!(
      !exec_manifest
        .bindings
        .contains_key(&ObjectHash("unchanged_bind".to_string()))
    );
  }

  #[tokio::test]
  async fn apply_config_not_found() {
    let result = apply(Path::new("/nonexistent/config.lua"), &test_options()).await;
    assert!(matches!(result, Err(ApplyError::ConfigNotFound(_))));
  }

  #[test]
  fn apply_dry_run() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("init.lua");

    // Write a minimal config
    std::fs::write(
      &config_path,
      r#"
      local M = {}
      function M.setup() end
      return M
      "#,
    )
    .unwrap();

    let mut options = test_options();
    options.dry_run = true;

    // Set env vars for test isolation
    temp_env::with_vars(
      [
        (
          "SYSLUA_USER_STORE",
          Some(temp_dir.path().join("store").to_str().unwrap()),
        ),
        ("XDG_DATA_HOME", Some(temp_dir.path().join("data").to_str().unwrap())),
      ],
      || {
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(apply(&config_path, &options));

        assert!(result.is_ok());
        let result = result.unwrap();
        // Empty config results in empty diff, so it returns early with a real snapshot ID
        // (not "dry-run") because there's nothing to do
        assert!(result.diff.is_empty());
        assert_eq!(result.execution.realized.len(), 0);
        assert_eq!(result.execution.applied.len(), 0);
      },
    );
  }

  #[test]
  #[serial]
  fn cleanup_destroyed_bind_states_removes_state_files() {
    with_temp_env(|_temp_dir| {
      let hash1 = ObjectHash("destroyed_bind_1".to_string());
      let hash2 = ObjectHash("destroyed_bind_2".to_string());

      // Create bind state files
      let mut outputs = HashMap::new();
      outputs.insert("link".to_string(), "/test/path".to_string());
      let state = BindState::new(outputs);

      save_bind_state(&hash1, &state, false).unwrap();
      save_bind_state(&hash2, &state, false).unwrap();

      // Verify they exist
      assert!(load_bind_state(&hash1, false).unwrap().is_some());
      assert!(load_bind_state(&hash2, false).unwrap().is_some());

      // Clean up
      cleanup_destroyed_bind_states(&[hash1.clone(), hash2.clone()], false).unwrap();

      // Verify they're gone
      assert!(load_bind_state(&hash1, false).unwrap().is_none());
      assert!(load_bind_state(&hash2, false).unwrap().is_none());
    });
  }

  #[test]
  #[serial]
  fn cleanup_destroyed_bind_states_handles_empty_list() {
    with_temp_env(|_temp_dir| {
      // Should succeed with empty list
      cleanup_destroyed_bind_states(&[], false).unwrap();
    });
  }

  #[test]
  #[serial]
  fn build_restore_resolver_data_computes_build_paths() {
    use crate::build::BuildDef;

    with_temp_env(|_temp_dir| {
      let mut manifest = Manifest::default();

      // Add a build
      manifest.builds.insert(
        ObjectHash("build123".to_string()),
        BuildDef {
          id: None,
          inputs: None,
          create_actions: vec![],
          outputs: None,
        },
      );

      let (builds, binds) = build_restore_resolver_data(&manifest, false).unwrap();

      // Should have one build result
      assert_eq!(builds.len(), 1);
      assert!(builds.contains_key(&ObjectHash("build123".to_string())));

      let build_result = builds.get(&ObjectHash("build123".to_string())).unwrap();
      // Should have "out" output
      assert!(build_result.outputs.contains_key("out"));

      // No binds in manifest, so no bind results
      assert!(binds.is_empty());
    });
  }

  #[test]
  #[serial]
  fn build_restore_resolver_data_loads_bind_states() {
    use crate::bind::BindDef;

    with_temp_env(|_temp_dir| {
      let hash = ObjectHash("bind123".to_string());

      // Create a bind state file
      let mut outputs = HashMap::new();
      outputs.insert("link".to_string(), "/home/user/.config/test".to_string());
      let state = BindState::new(outputs.clone());
      save_bind_state(&hash, &state, false).unwrap();

      // Create manifest with the bind
      let mut manifest = Manifest::default();
      manifest.bindings.insert(
        hash.clone(),
        BindDef {
          id: None,
          inputs: None,
          outputs: None,
          create_actions: vec![],
          update_actions: None,
          destroy_actions: vec![],
        },
      );

      let (builds, binds) = build_restore_resolver_data(&manifest, false).unwrap();

      // No builds
      assert!(builds.is_empty());

      // Should have loaded the bind state
      assert_eq!(binds.len(), 1);
      let bind_result = binds.get(&hash).unwrap();
      assert_eq!(bind_result.outputs.get("link").unwrap(), "/home/user/.config/test");
    });
  }

  #[test]
  #[serial]
  fn build_restore_resolver_data_skips_missing_bind_states() {
    use crate::bind::BindDef;

    with_temp_env(|_temp_dir| {
      let hash = ObjectHash("bind_without_state".to_string());

      // Create manifest with a bind but don't create state file
      let mut manifest = Manifest::default();
      manifest.bindings.insert(
        hash.clone(),
        BindDef {
          id: None,
          inputs: None,
          outputs: None,
          create_actions: vec![],
          update_actions: None,
          destroy_actions: vec![],
        },
      );

      let (builds, binds) = build_restore_resolver_data(&manifest, false).unwrap();

      // No builds
      assert!(builds.is_empty());
      // Bind without state should be skipped
      assert!(binds.is_empty());
    });
  }

  #[test]
  #[serial]
  fn destroy_removed_binds_returns_empty_vec_for_empty_input() {
    with_temp_env(|_temp_dir| {
      let rt = tokio::runtime::Runtime::new().unwrap();
      let result = rt.block_on(destroy_removed_binds(&[], None, &ExecuteConfig::default()));

      assert!(result.is_ok());
      assert!(result.unwrap().is_empty());
    });
  }

  #[test]
  #[serial]
  fn destroy_removed_binds_skips_binds_without_state() {
    use crate::bind::BindDef;

    with_temp_env(|_temp_dir| {
      let hash = ObjectHash("bind_no_state".to_string());

      // Create manifest with bind definition but no state file
      let mut manifest = Manifest::default();
      manifest.bindings.insert(
        hash.clone(),
        BindDef {
          id: None,
          inputs: None,
          outputs: None,
          create_actions: vec![],
          update_actions: None,
          destroy_actions: vec![],
        },
      );

      let rt = tokio::runtime::Runtime::new().unwrap();
      let result = rt.block_on(destroy_removed_binds(
        &[hash],
        Some(&manifest),
        &ExecuteConfig::default(),
      ));

      // Should succeed but return empty (skipped due to no state)
      assert!(result.is_ok());
      assert!(result.unwrap().is_empty());
    });
  }

  #[test]
  #[serial]
  fn destroy_removed_binds_skips_binds_without_definition() {
    with_temp_env(|_temp_dir| {
      let hash = ObjectHash("bind_no_def".to_string());

      // Create state file but no manifest entry
      let state = BindState::new(HashMap::new());
      save_bind_state(&hash, &state, false).unwrap();

      let manifest = Manifest::default(); // No bindings

      let rt = tokio::runtime::Runtime::new().unwrap();
      let result = rt.block_on(destroy_removed_binds(
        std::slice::from_ref(&hash),
        Some(&manifest),
        &ExecuteConfig::default(),
      ));

      // Should succeed but return empty (skipped due to no definition)
      assert!(result.is_ok());
      assert!(result.unwrap().is_empty());

      // State file should still exist (not cleaned up on skip)
      assert!(load_bind_state(&hash, false).unwrap().is_some());
    });
  }

  #[test]
  #[serial]
  fn restore_destroyed_binds_handles_empty_list() {
    with_temp_env(|_temp_dir| {
      let manifest = Manifest::default();
      let config = ExecuteConfig::default();

      let rt = tokio::runtime::Runtime::new().unwrap();
      let result = rt.block_on(restore_destroyed_binds(&[], &manifest, &config, false));

      assert!(result.is_ok());
    });
  }

  #[test]
  #[serial]
  fn update_modified_binds_returns_empty_for_empty_input() {
    with_temp_env(|_temp_dir| {
      let manifest = Manifest::default();
      let config = ExecuteConfig::default();

      let rt = tokio::runtime::Runtime::new().unwrap();
      let result = rt.block_on(update_modified_binds(&[], None, &manifest, &config));

      assert!(result.is_ok());
      assert!(result.unwrap().is_empty());
    });
  }

  #[test]
  #[serial]
  fn update_modified_binds_fails_without_old_state() {
    use crate::bind::BindDef;

    with_temp_env(|_temp_dir| {
      let old_hash = ObjectHash("old_bind".to_string());
      let new_hash = ObjectHash("new_bind".to_string());

      // Create manifest with new bind but no old state
      let mut manifest = Manifest::default();
      manifest.bindings.insert(
        new_hash.clone(),
        BindDef {
          id: Some("test-bind".to_string()),
          inputs: None,
          outputs: None,
          create_actions: vec![],
          update_actions: Some(vec![]),
          destroy_actions: vec![],
        },
      );

      let config = ExecuteConfig::default();
      let rt = tokio::runtime::Runtime::new().unwrap();
      let result = rt.block_on(update_modified_binds(
        &[(old_hash.clone(), new_hash.clone())],
        None,
        &manifest,
        &config,
      ));

      // Should fail because old bind state doesn't exist
      assert!(matches!(result, Err(ApplyError::UpdateFailed { .. })));
    });
  }

  #[test]
  #[serial]
  fn update_modified_binds_fails_without_new_bind_def() {
    with_temp_env(|_temp_dir| {
      let old_hash = ObjectHash("old_bind".to_string());
      let new_hash = ObjectHash("new_bind".to_string());

      // Create old state but no new bind in manifest
      let state = BindState::new([("path".to_string(), "/old/path".to_string())].into_iter().collect());
      save_bind_state(&old_hash, &state, false).unwrap();

      let manifest = Manifest::default(); // No bindings!
      let config = ExecuteConfig::default();

      let rt = tokio::runtime::Runtime::new().unwrap();
      let result = rt.block_on(update_modified_binds(
        &[(old_hash.clone(), new_hash.clone())],
        None,
        &manifest,
        &config,
      ));

      // Should fail because new bind definition doesn't exist
      assert!(matches!(result, Err(ApplyError::UpdateFailed { .. })));
    });
  }

  #[test]
  fn apply_result_includes_updated_count() {
    // Verify that ApplyResult has binds_updated field
    let result = ApplyResult {
      snapshot: Snapshot::new("test".to_string(), None, Manifest::default()),
      diff: StateDiff::default(),
      execution: DagResult::default(),
      binds_destroyed: 3,
      binds_updated: 5,
    };

    assert_eq!(result.binds_destroyed, 3);
    assert_eq!(result.binds_updated, 5);
  }
}
