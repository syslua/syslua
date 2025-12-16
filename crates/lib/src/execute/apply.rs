//! Apply orchestration for syslua.
//!
//! This module provides the high-level `apply` function that orchestrates
//! the full apply flow:
//!
//! 1. Load current state
//! 2. Evaluate config to produce desired manifest
//! 3. Compute diff between desired and current
//! 4. Destroy removed binds
//! 5. Realize new builds
//! 6. Apply new binds
//! 7. Save new snapshot
//!
//! On failure, rolls back any applied binds from this run.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use thiserror::Error;
use tracing::{debug, error, info, warn};

use crate::bind::execute::destroy_bind;
use crate::bind::state::{BindState, BindStateError, load_bind_state, remove_bind_state, save_bind_state};
use crate::eval::{EvalError, evaluate_config};
use crate::execute::execute_manifest;
use crate::manifest::Manifest;
use crate::snapshot::{Snapshot, SnapshotError, SnapshotStore, StateDiff, compute_diff, generate_snapshot_id};
use crate::store::paths::StorePaths;
use crate::util::hash::ObjectHash;

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
  let snapshot_store = SnapshotStore::default_store();
  let current_snapshot = snapshot_store.load_current()?;
  let current_manifest = current_snapshot.as_ref().map(|s| &s.manifest);

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
    });
  }

  // 4. Destroy removed binds
  let binds_destroyed = destroy_removed_binds(&diff.binds_to_destroy, current_manifest, &options.execute).await?;

  // 5 & 6. Build execution manifest and execute (realize builds, apply binds)
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
    // Rollback is handled by execute_manifest internally for binds applied in this run.
    // We need to handle the case where we partially succeeded.
    error!("execution failed");

    if let Some((hash, ref err)) = dag_result.build_failed {
      error!(build = %hash.0, error = %err, "build failed");
    }
    if let Some((hash, ref err)) = dag_result.bind_failed {
      error!(bind = %hash.0, error = %err, "bind failed");
    }

    // Return error but include partial results
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
    binds_destroyed,
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
/// Number of binds successfully destroyed.
async fn destroy_removed_binds(
  hashes: &[ObjectHash],
  current_manifest: Option<&Manifest>,
  config: &ExecuteConfig,
) -> Result<usize, ApplyError> {
  if hashes.is_empty() {
    return Ok(0);
  }

  info!(count = hashes.len(), "destroying removed binds");

  let mut destroyed = 0;

  // Create an empty resolver for destroy operations
  // (destroy actions typically only need outputs from the bind itself)
  let empty_builds: HashMap<ObjectHash, BuildResult> = HashMap::new();
  let empty_binds: HashMap<ObjectHash, BindResult> = HashMap::new();
  let empty_manifest = Manifest::default();
  let resolver = ExecutionResolver::new(&empty_builds, &empty_binds, &empty_manifest, "/tmp", config.system);

  for hash in hashes {
    // Load bind state (outputs from when it was applied)
    let bind_state = match load_bind_state(hash, config.system)? {
      Some(state) => state,
      None => {
        warn!(bind = %hash.0, "no bind state found, skipping destroy");
        continue;
      }
    };

    // Get bind definition from current manifest
    let bind_def = match current_manifest.and_then(|m| m.bindings.get(hash)) {
      Some(def) => def,
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
    info!(bind = %hash.0, "destroying bind");
    if let Err(e) = destroy_bind(hash, bind_def, &bind_result, &resolver, config).await {
      error!(bind = %hash.0, error = %e, "failed to destroy bind");
      return Err(ApplyError::DestroyFailed {
        hash: hash.clone(),
        source: e,
      });
    }

    // Remove bind state file
    remove_bind_state(hash, config.system)?;
    destroyed += 1;
    debug!(bind = %hash.0, "bind destroyed");
  }

  info!(destroyed, "destroy phase complete");
  Ok(destroyed)
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
        shell: None,
      },
      system: false,
      dry_run: false,
    }
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
        name: "cached-pkg".to_string(),
        version: None,
        inputs: None,
        apply_actions: vec![],
        outputs: None,
      },
    );
    desired.builds.insert(
      ObjectHash("new".to_string()),
      BuildDef {
        name: "new-pkg".to_string(),
        version: None,
        inputs: None,
        apply_actions: vec![],
        outputs: None,
      },
    );

    // Add binds
    desired.bindings.insert(
      ObjectHash("new_bind".to_string()),
      BindDef {
        inputs: None,
        apply_actions: vec![],
        outputs: None,
        destroy_actions: None,
      },
    );
    desired.bindings.insert(
      ObjectHash("unchanged_bind".to_string()),
      BindDef {
        inputs: None,
        apply_actions: vec![],
        outputs: None,
        destroy_actions: None,
      },
    );

    let diff = StateDiff {
      builds_to_realize: vec![ObjectHash("new".to_string())],
      builds_cached: vec![ObjectHash("cached".to_string())],
      binds_to_apply: vec![ObjectHash("new_bind".to_string())],
      binds_to_destroy: vec![],
      binds_unchanged: vec![ObjectHash("unchanged_bind".to_string())],
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
  #[serial]
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
}
