//! Build and bind execution module.
//!
//! This module provides the main entry points for executing builds and binds from a manifest.
//! It handles:
//! - DAG-based dependency ordering
//! - Parallel execution of independent nodes
//! - Failure propagation and skip tracking
//! - Atomic rollback of binds on failure

pub mod apply;
pub mod dag;
pub mod resolver;
pub mod types;

use std::collections::{HashMap, HashSet};

use tokio::sync::Semaphore;
use tracing::{debug, error, info, warn};

use crate::{
  bind::execute::{apply_bind, destroy_bind},
  manifest::Manifest,
  util::hash::ObjectHash,
};

use dag::DagNode;
use resolver::ExecutionResolver;

pub use apply::{ApplyError, ApplyOptions, ApplyResult, DestroyOptions, DestroyResult, apply, destroy};
pub use dag::ExecutionDag;
pub use types::{BindResult, BuildResult, DagResult, ExecuteConfig, ExecuteError, FailedDependency};

/// Type alias for build task JoinSet to reduce complexity.
type BuildJoinSet = tokio::task::JoinSet<Result<(ObjectHash, Result<BuildResult, ExecuteError>), ExecuteError>>;

/// Type alias for bind task JoinSet to reduce complexity.
type BindJoinSet = tokio::task::JoinSet<Result<(ObjectHash, Result<BindResult, ExecuteError>), ExecuteError>>;

/// Execute all builds in a manifest.
///
/// This is the main entry point for build execution. It:
/// 1. Constructs a DAG from the manifest
/// 2. Computes parallel execution waves
/// 3. Executes builds wave by wave, with parallelism within each wave
/// 4. Tracks failures and skips dependent builds
///
/// # Arguments
///
/// * `manifest` - The manifest containing builds to execute
/// * `config` - Execution configuration
///
/// # Returns
///
/// A `DagResult` containing realized builds, failures, and skipped builds.
pub async fn execute_builds(manifest: &Manifest, config: &ExecuteConfig) -> Result<DagResult, ExecuteError> {
  info!(build_count = manifest.builds.len(), "starting build execution");

  // Build the execution DAG
  let dag = ExecutionDag::from_manifest(manifest)?;

  // Get execution waves
  let waves = dag.build_waves()?;

  info!(wave_count = waves.len(), "computed execution waves");

  // Track results
  let mut result = DagResult::default();
  let mut failed_builds: HashSet<ObjectHash> = HashSet::new();

  // Create semaphore for parallelism control
  let semaphore = std::sync::Arc::new(Semaphore::new(config.parallelism));

  // Execute waves in order
  for (wave_idx, wave) in waves.iter().enumerate() {
    debug!(wave = wave_idx, builds = wave.len(), "executing wave");

    // Partition wave into ready and skipped
    let mut ready_builds = Vec::new();
    let mut wave_skipped = Vec::new();

    for hash in wave {
      // Check if any dependency failed
      let deps = dag.build_dependencies(hash);
      let failed_dep = deps.iter().find(|dep| failed_builds.contains(dep));

      if let Some(failed_dep) = failed_dep {
        wave_skipped.push((hash.clone(), FailedDependency::Build(failed_dep.clone())));
      } else {
        ready_builds.push(hash.clone());
      }
    }

    // Record skipped builds
    for (hash, failed_dep) in wave_skipped {
      warn!(
        build = %hash.0,
        failed_dep = %failed_dep,
        "skipping build due to failed dependency"
      );
      result.build_skipped.insert(hash.clone(), failed_dep);
      failed_builds.insert(hash);
    }

    // Execute ready builds in parallel
    if !ready_builds.is_empty() {
      let wave_results = execute_wave(&ready_builds, manifest, config, &result.realized, semaphore.clone()).await;

      // Process results
      for (hash, build_result) in wave_results {
        match build_result {
          Ok(br) => {
            info!(build = %hash.0, "build succeeded");
            result.realized.insert(hash, br);
          }
          Err(e) => {
            error!(build = %hash.0, error = %e, "build failed");
            failed_builds.insert(hash.clone());
            result.build_failed = Some((hash, e));
          }
        }
      }
    }
  }

  info!(
    realized = result.realized.len(),
    failed = result.build_failed.is_some(),
    skipped = result.build_skipped.len(),
    "build execution complete"
  );

  Ok(result)
}

/// Execute all builds and binds in a manifest.
///
/// This is the main entry point for unified execution. It:
/// 1. Constructs a DAG from the manifest
/// 2. Computes unified execution waves (interleaved builds and binds)
/// 3. Executes nodes wave by wave, with parallelism within each wave
/// 4. Tracks failures and skips dependent nodes
/// 5. On any failure, rolls back all successfully applied binds
///
/// # Arguments
///
/// * `manifest` - The manifest containing builds and binds to execute
/// * `config` - Execution configuration
///
/// # Returns
///
/// A `DagResult` containing realized builds, applied binds, failures, and skipped nodes.
///
/// # Rollback Behavior
///
/// If any build or bind fails:
/// - All already-completed builds remain (they're immutable in the store)
/// - All already-applied binds are destroyed in reverse order
/// - The failed node is recorded in `build_failed` or `bind_failed`
/// - Dependent nodes are recorded in `build_skipped` or `bind_skipped`
pub async fn execute_manifest(manifest: &Manifest, config: &ExecuteConfig) -> Result<DagResult, ExecuteError> {
  info!(
    build_count = manifest.builds.len(),
    bind_count = manifest.bindings.len(),
    "starting manifest execution"
  );

  // Build the execution DAG
  let dag = ExecutionDag::from_manifest(manifest)?;

  // Get unified execution waves
  let waves = dag.execution_waves()?;

  info!(wave_count = waves.len(), "computed execution waves");

  // Track results
  let mut result = DagResult::default();
  let mut failed_nodes: HashSet<DagNode> = HashSet::new();

  // Track applied binds in order for rollback
  let mut applied_binds_order: Vec<ObjectHash> = Vec::new();

  // Create semaphore for parallelism control
  let semaphore = std::sync::Arc::new(Semaphore::new(config.parallelism));

  // Execute waves in order
  'waves: for (wave_idx, wave) in waves.iter().enumerate() {
    debug!(wave = wave_idx, nodes = wave.len(), "executing wave");

    // Separate builds and binds in this wave
    let mut ready_builds = Vec::new();
    let mut ready_binds = Vec::new();
    let mut skipped_builds = Vec::new();
    let mut skipped_binds = Vec::new();

    for node in wave {
      // Check if any dependency failed
      let failed_dep = find_failed_dependency(node, &dag, &failed_nodes);

      if let Some(dep) = failed_dep {
        match node {
          DagNode::Build(hash) => skipped_builds.push((hash.clone(), dep)),
          DagNode::Bind(hash) => skipped_binds.push((hash.clone(), dep)),
        }
      } else {
        match node {
          DagNode::Build(hash) => ready_builds.push(hash.clone()),
          DagNode::Bind(hash) => ready_binds.push(hash.clone()),
        }
      }
    }

    // Record skipped nodes
    for (hash, failed_dep) in skipped_builds {
      warn!(
        build = %hash.0,
        failed_dep = %failed_dep,
        "skipping build due to failed dependency"
      );
      failed_nodes.insert(DagNode::Build(hash.clone()));
      result.build_skipped.insert(hash, failed_dep);
    }

    for (hash, failed_dep) in skipped_binds {
      warn!(
        bind = %hash.0,
        failed_dep = %failed_dep,
        "skipping bind due to failed dependency"
      );
      failed_nodes.insert(DagNode::Bind(hash.clone()));
      result.bind_skipped.insert(hash, failed_dep);
    }

    // Execute ready builds in parallel
    if !ready_builds.is_empty() {
      let build_results = execute_build_wave(
        &ready_builds,
        manifest,
        config,
        &result.realized,
        &result.applied,
        semaphore.clone(),
      )
      .await;

      // Process build results
      for (hash, build_result) in build_results {
        match build_result {
          Ok(br) => {
            info!(build = %hash.0, "build succeeded");
            result.realized.insert(hash, br);
          }
          Err(e) => {
            error!(build = %hash.0, error = %e, "build failed");
            failed_nodes.insert(DagNode::Build(hash.clone()));
            result.build_failed = Some((hash, e));

            // Trigger rollback and stop
            rollback_binds(&applied_binds_order, &result.applied, manifest, config).await;
            break 'waves;
          }
        }
      }
    }

    // Execute ready binds in parallel
    if !ready_binds.is_empty() {
      let bind_results = execute_bind_wave(
        &ready_binds,
        manifest,
        config,
        &result.realized,
        &result.applied,
        semaphore.clone(),
      )
      .await;

      // Process bind results
      for (hash, bind_result) in bind_results {
        match bind_result {
          Ok(br) => {
            info!(bind = %hash.0, "bind succeeded");
            applied_binds_order.push(hash.clone());
            result.applied.insert(hash, br);
          }
          Err(e) => {
            error!(bind = %hash.0, error = %e, "bind failed");
            failed_nodes.insert(DagNode::Bind(hash.clone()));
            result.bind_failed = Some((hash, e));

            // Trigger rollback and stop
            rollback_binds(&applied_binds_order, &result.applied, manifest, config).await;
            break 'waves;
          }
        }
      }
    }
  }

  info!(
    realized = result.realized.len(),
    applied = result.applied.len(),
    build_failed = result.build_failed.is_some(),
    bind_failed = result.bind_failed.is_some(),
    build_skipped = result.build_skipped.len(),
    bind_skipped = result.bind_skipped.len(),
    "manifest execution complete"
  );

  Ok(result)
}

/// Find a failed dependency for a node.
fn find_failed_dependency(
  node: &DagNode,
  dag: &ExecutionDag,
  failed_nodes: &HashSet<DagNode>,
) -> Option<FailedDependency> {
  match node {
    DagNode::Build(hash) => {
      // Builds can only depend on other builds (not binds)
      for dep in dag.build_dependencies(hash) {
        if failed_nodes.contains(&DagNode::Build(dep.clone())) {
          return Some(FailedDependency::Build(dep));
        }
      }
      None
    }
    DagNode::Bind(hash) => {
      // Binds can depend on builds and other binds
      for dep in dag.bind_build_dependencies(hash) {
        if failed_nodes.contains(&DagNode::Build(dep.clone())) {
          return Some(FailedDependency::Build(dep));
        }
      }
      for dep in dag.bind_bind_dependencies(hash) {
        if failed_nodes.contains(&DagNode::Bind(dep.clone())) {
          return Some(FailedDependency::Bind(dep));
        }
      }
      None
    }
  }
}

/// Execute a wave of builds in parallel (unified execution version).
async fn execute_build_wave(
  builds: &[ObjectHash],
  manifest: &Manifest,
  config: &ExecuteConfig,
  completed_builds: &HashMap<ObjectHash, BuildResult>,
  completed_binds: &HashMap<ObjectHash, BindResult>,
  semaphore: std::sync::Arc<Semaphore>,
) -> Vec<(ObjectHash, Result<BuildResult, ExecuteError>)> {
  use tokio::task::JoinSet;

  let mut join_set = JoinSet::new();

  for hash in builds {
    let hash = hash.clone();
    let manifest = manifest.clone();
    let config = config.clone();
    let completed_builds = completed_builds.clone();
    let completed_binds = completed_binds.clone();
    let semaphore = semaphore.clone();

    join_set.spawn(async move {
      let _permit = semaphore.acquire().await.unwrap();

      let build_def = manifest
        .builds
        .get(&hash)
        .ok_or_else(|| ExecuteError::BuildNotFound(hash.clone()))?;

      // Use ExecutionResolver which supports both build and bind resolution
      let result = crate::build::execute::realize_build_with_resolver(
        &hash,
        build_def,
        &completed_builds,
        &completed_binds,
        &manifest,
        &config,
      )
      .await;

      Ok::<_, ExecuteError>((hash, result))
    });
  }

  collect_join_results(join_set).await
}

/// Execute a wave of binds in parallel.
async fn execute_bind_wave(
  binds: &[ObjectHash],
  manifest: &Manifest,
  config: &ExecuteConfig,
  completed_builds: &HashMap<ObjectHash, BuildResult>,
  completed_binds: &HashMap<ObjectHash, BindResult>,
  semaphore: std::sync::Arc<Semaphore>,
) -> Vec<(ObjectHash, Result<BindResult, ExecuteError>)> {
  use tokio::task::JoinSet;

  let mut join_set = JoinSet::new();

  for hash in binds {
    let hash = hash.clone();
    let manifest = manifest.clone();
    let config = config.clone();
    let completed_builds = completed_builds.clone();
    let completed_binds = completed_binds.clone();
    let semaphore = semaphore.clone();

    join_set.spawn(async move {
      let _permit = semaphore.acquire().await.unwrap();

      let bind_def = manifest
        .bindings
        .get(&hash)
        .ok_or_else(|| ExecuteError::BindNotFound(hash.clone()))?;

      // Create resolver with completed builds and binds
      let resolver = ExecutionResolver::new(
        &completed_builds,
        &completed_binds,
        &manifest,
        "/tmp".to_string(), // Temporary; apply_bind creates its own working dir
        config.system,
      );

      let result = apply_bind(&hash, bind_def, &resolver).await;

      Ok::<_, ExecuteError>((hash, result))
    });
  }

  collect_bind_join_results(join_set).await
}

/// Collect results from a JoinSet of build tasks.
async fn collect_join_results(mut join_set: BuildJoinSet) -> Vec<(ObjectHash, Result<BuildResult, ExecuteError>)> {
  let mut results = Vec::new();

  while let Some(join_result) = join_set.join_next().await {
    match join_result {
      Ok(Ok((hash, build_result))) => {
        results.push((hash, build_result));
      }
      Ok(Err(e)) => {
        error!(error = %e, "unexpected error in build task");
      }
      Err(e) => {
        error!(error = %e, "build task panicked");
      }
    }
  }

  results
}

/// Collect results from a JoinSet of bind tasks.
async fn collect_bind_join_results(mut join_set: BindJoinSet) -> Vec<(ObjectHash, Result<BindResult, ExecuteError>)> {
  let mut results = Vec::new();

  while let Some(join_result) = join_set.join_next().await {
    match join_result {
      Ok(Ok((hash, bind_result))) => {
        results.push((hash, bind_result));
      }
      Ok(Err(e)) => {
        error!(error = %e, "unexpected error in bind task");
      }
      Err(e) => {
        error!(error = %e, "bind task panicked");
      }
    }
  }

  results
}

/// Rollback applied binds in reverse order.
///
/// This is called when a build or bind fails to undo all side effects
/// from previously applied binds.
async fn rollback_binds(
  applied_order: &[ObjectHash],
  applied_results: &HashMap<ObjectHash, BindResult>,
  manifest: &Manifest,
  config: &ExecuteConfig,
) {
  if applied_order.is_empty() {
    return;
  }

  info!(count = applied_order.len(), "rolling back applied binds");

  // Create an empty resolver for destroy operations
  // (destroy actions typically don't need to reference other completed nodes)
  let empty_builds = HashMap::new();
  let empty_binds = HashMap::new();
  let resolver = ExecutionResolver::new(&empty_builds, &empty_binds, manifest, "/tmp".to_string(), config.system);

  // Rollback in reverse order
  for hash in applied_order.iter().rev() {
    if let Some(bind_def) = manifest.bindings.get(hash)
      && let Some(bind_result) = applied_results.get(hash)
    {
      info!(bind = %hash.0, "destroying bind");
      if let Err(e) = destroy_bind(hash, bind_def, bind_result, &resolver).await {
        // Log but continue - we want to try to rollback as much as possible
        error!(bind = %hash.0, error = %e, "failed to destroy bind during rollback");
      }
    }
  }

  info!("rollback complete");
}

/// Execute a wave of builds in parallel.
async fn execute_wave(
  builds: &[ObjectHash],
  manifest: &Manifest,
  config: &ExecuteConfig,
  completed: &HashMap<ObjectHash, BuildResult>,
  semaphore: std::sync::Arc<Semaphore>,
) -> Vec<(ObjectHash, Result<BuildResult, ExecuteError>)> {
  use tokio::task::JoinSet;

  let mut join_set = JoinSet::new();

  for hash in builds {
    let hash = hash.clone();
    let manifest = manifest.clone();
    let config = config.clone();
    let completed = completed.clone();
    let semaphore = semaphore.clone();

    join_set.spawn(async move {
      // Acquire semaphore permit inside the task
      let _permit = semaphore.acquire().await.unwrap();

      let build_def = manifest
        .builds
        .get(&hash)
        .ok_or_else(|| ExecuteError::BuildNotFound(hash.clone()))?;

      let result = crate::build::execute::realize_build(&hash, build_def, &completed, &manifest, &config).await;

      Ok::<_, ExecuteError>((hash, result))
    });
  }

  let mut results = Vec::new();

  while let Some(join_result) = join_set.join_next().await {
    match join_result {
      Ok(Ok((hash, build_result))) => {
        results.push((hash, build_result));
      }
      Ok(Err(e)) => {
        // This shouldn't happen as we handle errors in the task
        error!(error = %e, "unexpected error in build task");
      }
      Err(e) => {
        // Task panicked
        error!(error = %e, "build task panicked");
      }
    }
  }

  results
}

/// Execute a single build by hash.
///
/// This is a convenience function for executing a single build without
/// computing the full DAG. Dependencies must already be built.
pub async fn execute_single_build(
  hash: &ObjectHash,
  manifest: &Manifest,
  config: &ExecuteConfig,
  completed: &HashMap<ObjectHash, BuildResult>,
) -> Result<BuildResult, ExecuteError> {
  let build_def = manifest
    .builds
    .get(hash)
    .ok_or_else(|| ExecuteError::BuildNotFound(hash.clone()))?;

  crate::build::execute::realize_build(hash, build_def, completed, manifest, config).await
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{
    action::{Action, actions::exec::ExecOpts},
    bind::BindInputs,
    build::{BuildDef, BuildInputs},
    util::{
      hash::Hashable,
      testutil::{echo_msg, shell_cmd},
    },
  };
  use std::collections::BTreeMap;
  use tempfile::TempDir;

  fn make_build(id: &str, inputs: Option<BuildInputs>) -> BuildDef {
    let (cmd, args) = echo_msg(id);
    BuildDef {
      id: None,
      inputs,
      create_actions: vec![Action::Exec(ExecOpts {
        bin: cmd.to_string(),
        args: Some(args),
        env: None,
        cwd: None,
      })],
      outputs: None,
    }
  }

  fn test_config() -> ExecuteConfig {
    ExecuteConfig {
      parallelism: 4,
      system: false,
    }
  }

  /// Helper to set up a temp store and run a test.
  fn with_temp_store<F, Fut, T>(f: F) -> T
  where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
  {
    let temp_dir = TempDir::new().unwrap();
    let store_path = temp_dir.path().join("store");

    temp_env::with_var("SYSLUA_USER_STORE", Some(store_path.to_str().unwrap()), || {
      tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(f())
    })
  }

  /// Returns a command and args to create an empty file at the given path.
  /// Unix: /usr/bin/touch {path}
  /// Windows: powershell New-Item
  #[cfg(unix)]
  fn touch_cmd(path: &std::path::Path) -> (String, Vec<String>) {
    ("/usr/bin/touch".to_string(), vec![path.display().to_string()])
  }

  #[cfg(windows)]
  fn touch_cmd(path: &std::path::Path) -> (String, Vec<String>) {
    // Use PowerShell to create an empty file - more reliable than cmd.exe approaches
    (
      "powershell.exe".to_string(),
      vec![
        "-NoProfile".to_string(),
        "-Command".to_string(),
        format!("New-Item -ItemType File -Path '{}' -Force | Out-Null", path.display()),
      ],
    )
  }

  /// Returns a command and args to remove a file at the given path.
  /// Unix: /bin/rm -f {path}
  /// Windows: powershell Remove-Item
  #[cfg(unix)]
  fn rm_cmd(path: &std::path::Path) -> (String, Vec<String>) {
    (
      "/bin/rm".to_string(),
      vec!["-f".to_string(), path.display().to_string()],
    )
  }

  #[cfg(windows)]
  fn rm_cmd(path: &std::path::Path) -> (String, Vec<String>) {
    // Use PowerShell to remove file - more reliable than cmd.exe approaches
    (
      "powershell.exe".to_string(),
      vec![
        "-NoProfile".to_string(),
        "-Command".to_string(),
        format!(
          "Remove-Item -Path '{}' -Force -ErrorAction SilentlyContinue",
          path.display()
        ),
      ],
    )
  }

  #[test]
  fn execute_empty_manifest() {
    with_temp_store(|| async {
      let manifest = Manifest::default();
      let config = test_config();

      let result = execute_builds(&manifest, &config).await.unwrap();

      assert!(result.is_success());
      assert_eq!(result.total(), 0);
    });
  }

  #[test]
  fn execute_single_independent_build() {
    with_temp_store(|| async {
      let build = make_build("test", None);
      let hash = build.compute_hash().unwrap();

      let mut manifest = Manifest::default();
      manifest.builds.insert(hash.clone(), build);

      let config = test_config();
      let result = execute_builds(&manifest, &config).await.unwrap();

      assert!(result.is_success());
      assert_eq!(result.realized.len(), 1);
      assert!(result.realized.contains_key(&hash));
    });
  }

  #[test]
  fn execute_parallel_independent_builds() {
    with_temp_store(|| async {
      let build_a = make_build("a", None);
      let hash_a = build_a.compute_hash().unwrap();

      let build_b = make_build("b", None);
      let hash_b = build_b.compute_hash().unwrap();

      let build_c = make_build("c", None);
      let hash_c = build_c.compute_hash().unwrap();

      let mut manifest = Manifest::default();
      manifest.builds.insert(hash_a.clone(), build_a);
      manifest.builds.insert(hash_b.clone(), build_b);
      manifest.builds.insert(hash_c.clone(), build_c);

      let config = test_config();
      let result = execute_builds(&manifest, &config).await.unwrap();

      assert!(result.is_success());
      assert_eq!(result.realized.len(), 3);
    });
  }

  #[test]
  fn execute_dependent_builds() {
    with_temp_store(|| async {
      let build_a = make_build("a", None);
      let hash_a = build_a.compute_hash().unwrap();

      let build_b = make_build("b", Some(BuildInputs::Build(hash_a.clone())));
      let hash_b = build_b.compute_hash().unwrap();

      let mut manifest = Manifest::default();
      manifest.builds.insert(hash_a.clone(), build_a);
      manifest.builds.insert(hash_b.clone(), build_b);

      let config = test_config();
      let result = execute_builds(&manifest, &config).await.unwrap();

      assert!(result.is_success());
      assert_eq!(result.realized.len(), 2);

      // Verify both builds completed
      assert!(result.realized.contains_key(&hash_a));
      assert!(result.realized.contains_key(&hash_b));
    });
  }

  #[test]
  fn execute_failing_build() {
    with_temp_store(|| async {
      let (cmd, args) = shell_cmd("exit 1");
      let build = BuildDef {
        id: None,
        inputs: None,
        create_actions: vec![Action::Exec(ExecOpts {
          bin: cmd.to_string(),
          args: Some(args),
          env: None,
          cwd: None,
        })],
        outputs: None,
      };
      let hash = build.compute_hash().unwrap();

      let mut manifest = Manifest::default();
      manifest.builds.insert(hash.clone(), build);

      let config = test_config();
      let result = execute_builds(&manifest, &config).await.unwrap();

      assert!(!result.is_success());
      assert!(result.build_failed.is_some());
      let (failed_hash, _) = result.build_failed.as_ref().unwrap();
      assert_eq!(failed_hash, &hash);
    });
  }

  #[test]
  fn execute_skip_dependent_on_failure() {
    with_temp_store(|| async {
      // A fails, B depends on A -> B should be skipped
      let (cmd, args) = shell_cmd("exit 1");
      let build_a = BuildDef {
        id: None,
        inputs: None,
        create_actions: vec![Action::Exec(ExecOpts {
          bin: cmd.to_string(),
          args: Some(args),
          env: None,
          cwd: None,
        })],
        outputs: None,
      };
      let hash_a = build_a.compute_hash().unwrap();

      let build_b = make_build("b", Some(BuildInputs::Build(hash_a.clone())));
      let hash_b = build_b.compute_hash().unwrap();

      let mut manifest = Manifest::default();
      manifest.builds.insert(hash_a.clone(), build_a);
      manifest.builds.insert(hash_b.clone(), build_b);

      let config = test_config();
      let result = execute_builds(&manifest, &config).await.unwrap();

      assert!(!result.is_success());
      assert!(result.build_failed.is_some());
      assert_eq!(result.build_skipped.len(), 1);

      let (failed_hash, _) = result.build_failed.as_ref().unwrap();
      assert_eq!(failed_hash, &hash_a);
      assert!(result.build_skipped.contains_key(&hash_b));
      assert_eq!(result.build_skipped[&hash_b], FailedDependency::Build(hash_a));
    });
  }

  #[test]
  fn execute_diamond_dependency() {
    with_temp_store(|| async {
      //     A
      //    / \
      //   B   C
      //    \ /
      //     D
      let build_a = make_build("a", None);
      let hash_a = build_a.compute_hash().unwrap();

      let build_b = make_build("b", Some(BuildInputs::Build(hash_a.clone())));
      let hash_b = build_b.compute_hash().unwrap();

      let build_c = make_build("c", Some(BuildInputs::Build(hash_a.clone())));
      let hash_c = build_c.compute_hash().unwrap();

      let mut d_inputs = BTreeMap::new();
      d_inputs.insert("b".to_string(), BuildInputs::Build(hash_b.clone()));
      d_inputs.insert("c".to_string(), BuildInputs::Build(hash_c.clone()));
      let build_d = make_build("d", Some(BuildInputs::Table(d_inputs)));
      let hash_d = build_d.compute_hash().unwrap();

      let mut manifest = Manifest::default();
      manifest.builds.insert(hash_a.clone(), build_a);
      manifest.builds.insert(hash_b.clone(), build_b);
      manifest.builds.insert(hash_c.clone(), build_c);
      manifest.builds.insert(hash_d.clone(), build_d);

      let config = test_config();
      let result = execute_builds(&manifest, &config).await.unwrap();

      assert!(result.is_success());
      assert_eq!(result.realized.len(), 4);
    });
  }

  // ============================================================
  // execute_manifest() tests - unified build + bind execution
  // ============================================================

  use crate::bind::BindDef;

  fn make_bind(id: &str, script: &str, inputs: Option<BindInputs>) -> BindDef {
    let (cmd, args) = shell_cmd(script);
    BindDef {
      id: Some(id.to_string()),
      inputs,
      outputs: None,
      create_actions: vec![Action::Exec(ExecOpts {
        bin: cmd.to_string(),
        args: Some(args),
        env: None,
        cwd: None,
      })],
      update_actions: None,
      destroy_actions: vec![],
    }
  }

  #[test]
  fn manifest_bind_after_build() {
    // Bind depends on build -> bind executes after build
    with_temp_store(|| async {
      let build = make_build("app", None);
      let build_hash = build.compute_hash().unwrap();

      let bind = make_bind("bind1", "echo linking", Some(BindInputs::Build(build_hash.clone())));
      let bind_hash = bind.compute_hash().unwrap();

      let mut manifest = Manifest::default();
      manifest.builds.insert(build_hash.clone(), build);
      manifest.bindings.insert(bind_hash.clone(), bind);

      let config = test_config();
      let result = execute_manifest(&manifest, &config).await.unwrap();

      assert!(result.is_success());
      assert_eq!(result.realized.len(), 1);
      assert_eq!(result.applied.len(), 1);
      assert!(result.realized.contains_key(&build_hash));
      assert!(result.applied.contains_key(&bind_hash));
    });
  }

  #[test]
  fn manifest_bind_chain() {
    // Bind A -> Bind B -> Bind C (linear chain)
    with_temp_store(|| async {
      let bind_a = make_bind("bind1", "echo step_a", None);
      let hash_a = bind_a.compute_hash().unwrap();

      let bind_b = make_bind("bind2", "echo step_b", Some(BindInputs::Bind(hash_a.clone())));
      let hash_b = bind_b.compute_hash().unwrap();

      let bind_c = make_bind("bind3", "echo step_c", Some(BindInputs::Bind(hash_b.clone())));
      let hash_c = bind_c.compute_hash().unwrap();

      let mut manifest = Manifest::default();
      manifest.bindings.insert(hash_a.clone(), bind_a);
      manifest.bindings.insert(hash_b.clone(), bind_b);
      manifest.bindings.insert(hash_c.clone(), bind_c);

      let config = test_config();
      let result = execute_manifest(&manifest, &config).await.unwrap();

      assert!(result.is_success());
      assert_eq!(result.applied.len(), 3);
      assert!(result.applied.contains_key(&hash_a));
      assert!(result.applied.contains_key(&hash_b));
      assert!(result.applied.contains_key(&hash_c));
    });
  }

  #[test]
  fn manifest_bind_placeholder_resolution() {
    // Bind uses $${build:hash:out} placeholder that should resolve to build output
    with_temp_store(|| async {
      // Build that produces an output
      let (echo_cmd, echo_args) = shell_cmd("echo built");
      let build = BuildDef {
        id: None,
        inputs: None,
        create_actions: vec![Action::Exec(ExecOpts {
          bin: echo_cmd.to_string(),
          args: Some(echo_args),
          env: None,
          cwd: None,
        })],
        outputs: Some([("bin".to_string(), "$${out}/bin".to_string())].into_iter().collect()),
      };
      let build_hash = build.compute_hash().unwrap();

      // Bind that references the build output via placeholder
      // Using the full hash in the command to test placeholder resolution
      let (bind_cmd, bind_args) = shell_cmd(&format!("echo using $$${{build:{}:bin}}", build_hash.0));
      let bind = BindDef {
        id: None,
        inputs: Some(BindInputs::Build(build_hash.clone())),
        outputs: None,
        create_actions: vec![Action::Exec(ExecOpts {
          bin: bind_cmd.to_string(),
          args: Some(bind_args),
          env: None,
          cwd: None,
        })],
        update_actions: None,
        destroy_actions: vec![],
      };
      let bind_hash = bind.compute_hash().unwrap();

      let mut manifest = Manifest::default();
      manifest.builds.insert(build_hash.clone(), build);
      manifest.bindings.insert(bind_hash.clone(), bind);

      let config = test_config();
      let result = execute_manifest(&manifest, &config).await.unwrap();

      assert!(result.is_success());
      assert_eq!(result.realized.len(), 1);
      assert_eq!(result.applied.len(), 1);

      // Verify the build output was resolved
      let build_result = &result.realized[&build_hash];
      assert!(build_result.outputs.contains_key("bin"));
      assert!(build_result.outputs["bin"].ends_with("/bin"));
    });
  }

  #[test]
  fn manifest_bind_failure_rollback() {
    // Bind A succeeds, Bind B fails -> Bind A should be rolled back (destroyed)
    with_temp_store(|| async {
      // Create a temp file to track rollback
      let temp_dir = TempDir::new().unwrap();
      let marker_file = temp_dir.path().join("bind_a_applied");

      // Debug logging for Windows path issues
      let (touch_cmd_str, touch_args) = touch_cmd(&marker_file);
      let (rm_cmd_str, rm_args) = rm_cmd(&marker_file);
      eprintln!("=== DEBUG: manifest_bind_failure_rollback ===");
      eprintln!("temp_dir: {:?}", temp_dir.path());
      eprintln!("temp_dir length: {}", temp_dir.path().display().to_string().len());
      eprintln!("marker_file: {:?}", marker_file);
      eprintln!("marker_file length: {}", marker_file.display().to_string().len());
      eprintln!("touch_cmd: {} {:?}", touch_cmd_str, touch_args);
      eprintln!("rm_cmd: {} {:?}", rm_cmd_str, rm_args);
      eprintln!("==============================================");

      // Use platform-specific commands since PATH is isolated
      let bind_a = BindDef {
        id: None,
        inputs: None,
        outputs: None,
        create_actions: vec![Action::Exec(ExecOpts {
          bin: touch_cmd_str,
          args: Some(touch_args),
          env: None,
          cwd: None,
        })],
        update_actions: None,
        destroy_actions: vec![Action::Exec(ExecOpts {
          bin: rm_cmd_str,
          args: Some(rm_args),
          env: None,
          cwd: None,
        })],
      };
      let hash_a = bind_a.compute_hash().unwrap();

      // Bind B depends on A and fails
      let (exit_cmd, exit_args) = shell_cmd("exit 1");
      let bind_b = BindDef {
        id: None,
        inputs: Some(BindInputs::Bind(hash_a.clone())),
        outputs: None,
        create_actions: vec![Action::Exec(ExecOpts {
          bin: exit_cmd.to_string(),
          args: Some(exit_args),
          env: None,
          cwd: None,
        })],
        update_actions: None,
        destroy_actions: vec![],
      };
      let hash_b = bind_b.compute_hash().unwrap();

      let mut manifest = Manifest::default();
      manifest.bindings.insert(hash_a.clone(), bind_a);
      manifest.bindings.insert(hash_b.clone(), bind_b);

      let config = test_config();
      let result = execute_manifest(&manifest, &config).await.unwrap();

      // Debug: print full result details
      eprintln!("=== DEBUG: Result details ===");
      eprintln!("is_success: {}", result.is_success());
      eprintln!("bind_failed: {:?}", result.bind_failed);
      eprintln!("applied keys: {:?}", result.applied.keys().collect::<Vec<_>>());
      eprintln!("hash_a: {:?}", hash_a);
      eprintln!("hash_b: {:?}", hash_b);
      eprintln!("=============================");

      assert!(!result.is_success());
      assert!(result.bind_failed.is_some());

      // The failing bind should be hash_b (which depends on hash_a)
      let (failed_hash, failed_err) = result.bind_failed.as_ref().unwrap();
      eprintln!("=== DEBUG: Failed bind details ===");
      eprintln!("failed_hash: {:?}", failed_hash);
      eprintln!("failed_err: {:?}", failed_err);
      eprintln!("==================================");
      assert_eq!(failed_hash, &hash_b, "Bind B should have failed, not Bind A");

      // Bind A should have been applied (before failure)
      assert!(
        result.applied.contains_key(&hash_a),
        "Bind A should have been applied before rollback"
      );

      // The marker file should have been removed by rollback
      assert!(!marker_file.exists(), "Marker file should be removed after rollback");
    });
  }

  #[test]
  fn manifest_build_failure_skips_binds() {
    // Build fails -> dependent bind should be skipped (not applied)
    with_temp_store(|| async {
      let build = BuildDef {
        id: None,
        inputs: None,
        create_actions: vec![Action::Exec(ExecOpts {
          bin: "exit 1".to_string(),
          args: None,
          env: None,
          cwd: None,
        })],
        outputs: None,
      };
      let build_hash = build.compute_hash().unwrap();

      let bind = make_bind(
        "bind",
        "echo should-not-run",
        Some(BindInputs::Build(build_hash.clone())),
      );
      let bind_hash = bind.compute_hash().unwrap();

      let mut manifest = Manifest::default();
      manifest.builds.insert(build_hash.clone(), build);
      manifest.bindings.insert(bind_hash.clone(), bind);

      let config = test_config();
      let result = execute_manifest(&manifest, &config).await.unwrap();

      assert!(!result.is_success());
      assert!(result.build_failed.is_some());
      let (failed_hash, _) = result.build_failed.as_ref().unwrap();
      assert_eq!(failed_hash, &build_hash);

      // No binds should have been applied (we break out of execution on failure)
      assert!(result.applied.is_empty(), "No binds should have been applied");
    });
  }

  #[test]
  fn manifest_mixed_wave_execution() {
    // Independent builds and binds should run in parallel within a wave
    // Build A (no deps) and Bind X (no deps) can run together
    // Build B depends on A, Bind Y depends on X
    with_temp_store(|| async {
      let build_a = make_build("a", None);
      let hash_a = build_a.compute_hash().unwrap();

      let build_b = make_build("b", Some(BuildInputs::Build(hash_a.clone())));
      let hash_b = build_b.compute_hash().unwrap();

      let bind_x = make_bind("bind-x", "echo x", None);
      let hash_x = bind_x.compute_hash().unwrap();

      let bind_y = make_bind("bind-y", "echo y", Some(BindInputs::Bind(hash_x.clone())));
      let hash_y = bind_y.compute_hash().unwrap();

      let mut manifest = Manifest::default();
      manifest.builds.insert(hash_a.clone(), build_a);
      manifest.builds.insert(hash_b.clone(), build_b);
      manifest.bindings.insert(hash_x.clone(), bind_x);
      manifest.bindings.insert(hash_y.clone(), bind_y);

      let config = test_config();
      let result = execute_manifest(&manifest, &config).await.unwrap();

      assert!(result.is_success());
      assert_eq!(result.realized.len(), 2);
      assert_eq!(result.applied.len(), 2);

      // All nodes should be completed
      assert!(result.realized.contains_key(&hash_a));
      assert!(result.realized.contains_key(&hash_b));
      assert!(result.applied.contains_key(&hash_x));
      assert!(result.applied.contains_key(&hash_y));
    });
  }

  #[test]
  fn manifest_empty() {
    // Empty manifest should succeed with no nodes
    with_temp_store(|| async {
      let manifest = Manifest::default();
      let config = test_config();

      let result = execute_manifest(&manifest, &config).await.unwrap();

      assert!(result.is_success());
      assert_eq!(result.build_total(), 0);
      assert_eq!(result.bind_total(), 0);
      assert_eq!(result.total(), 0);
    });
  }

  #[test]
  fn manifest_only_binds() {
    // Manifest with only binds (no builds)
    with_temp_store(|| async {
      let bind_a = make_bind("bind-a", "echo a", None);
      let hash_a = bind_a.compute_hash().unwrap();

      let bind_b = make_bind("bind-b", "echo b", None);
      let hash_b = bind_b.compute_hash().unwrap();

      let mut manifest = Manifest::default();
      manifest.bindings.insert(hash_a.clone(), bind_a);
      manifest.bindings.insert(hash_b.clone(), bind_b);

      let config = test_config();
      let result = execute_manifest(&manifest, &config).await.unwrap();

      assert!(result.is_success());
      assert_eq!(result.realized.len(), 0);
      assert_eq!(result.applied.len(), 2);
    });
  }
}
