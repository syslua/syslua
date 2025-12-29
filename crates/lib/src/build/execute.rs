//! Single build realization.
//!
//! This module handles executing all actions for a single build and
//! producing the final BuildResult.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tokio::fs;
use tracing::{debug, info, warn};

use crate::build::BuildDef;
use crate::build::store::build_dir_path;
use crate::manifest::Manifest;
use crate::placeholder;

use crate::action::execute_action;
use crate::execute::resolver::{BuildResolver, ExecutionResolver};
use crate::execute::types::{ActionResult, BindResult, BuildResult, ExecuteConfig, ExecuteError};
use crate::util::hash::{ObjectHash, hash_directory};

/// Marker file name indicating a build completed successfully.
pub const BUILD_COMPLETE_MARKER: &str = ".syslua-complete";

/// Files/directories excluded when hashing build outputs.
/// - BUILD_COMPLETE_MARKER: The marker itself (written after hash)
/// - "tmp": Build temp directory (may have leftovers)
const BUILD_HASH_EXCLUSIONS: &[&str] = &[".syslua-complete", "tmp"];

/// Marker file content structure.
#[derive(Debug, Serialize, Deserialize)]
pub struct BuildMarker {
  /// Marker format version.
  pub version: u32,
  /// Build status (always "complete" for successful builds).
  pub status: String,
  /// Full 64-character SHA256 hash of build outputs.
  #[serde(skip_serializing_if = "Option::is_none")]
  pub output_hash: Option<String>,
}

/// Write the build completion marker with output hash.
/// Called after build succeeds, before returning BuildResult.
async fn write_build_complete_marker(store_path: &Path) -> Result<(), ExecuteError> {
  // Compute hash of build outputs (excluding marker and tmp)
  let output_hash = hash_directory(store_path, BUILD_HASH_EXCLUSIONS)?;

  let marker = BuildMarker {
    version: 1,
    status: "complete".to_string(),
    output_hash: Some(output_hash.0),
  };
  let content = serde_json::to_string(&marker).expect("failed to serialize marker");
  fs::write(store_path.join(BUILD_COMPLETE_MARKER), format!("{}\n", content))
    .await
    .map_err(|e| ExecuteError::WriteMarker { message: e.to_string() })
}

/// Read the build completion marker.
///
/// Returns `None` if the marker doesn't exist.
/// Returns `Some(marker)` if it exists and can be parsed.
pub fn read_build_marker(store_path: &Path) -> Result<Option<BuildMarker>, ExecuteError> {
  let marker_path = store_path.join(BUILD_COMPLETE_MARKER);

  if !marker_path.exists() {
    return Ok(None);
  }

  let content = std::fs::read_to_string(&marker_path).map_err(|e| ExecuteError::ReadMarker { message: e.to_string() })?;
  let marker: BuildMarker = serde_json::from_str(&content).map_err(|e| ExecuteError::ParseMarker { message: e.to_string() })?;
  Ok(Some(marker))
}

/// Check if a build store path has a completion marker.
pub fn is_build_complete(store_path: &Path) -> bool {
  read_build_marker(store_path).map(|m| m.is_some()).unwrap_or(false)
}

/// Verify a cached build's output hash matches the marker.
///
/// Returns `true` if valid (should use cache), `false` if should rebuild.
/// Legacy markers without `output_hash` are trusted.
fn verify_build_hash(store_path: &Path, marker: &BuildMarker) -> bool {
  let Some(stored_hash) = &marker.output_hash else {
    // Legacy marker without hash - trust it
    debug!(path = ?store_path, "legacy marker without hash, trusting cache");
    return true;
  };

  match hash_directory(store_path, BUILD_HASH_EXCLUSIONS) {
    Ok(current_hash) => {
      if current_hash.0 == *stored_hash {
        true
      } else {
        warn!(
          path = ?store_path,
          expected = %stored_hash,
          actual = %current_hash.0,
          "build output corrupted, will rebuild"
        );
        false
      }
    }
    Err(e) => {
      warn!(
        path = ?store_path,
        error = %e,
        "failed to hash build output, will rebuild"
      );
      false
    }
  }
}

/// Realize a single build.
///
/// This executes all actions in the build definition and produces the
/// final BuildResult with resolved outputs.
///
/// # Arguments
///
/// * `hash` - The build hash
/// * `build_def` - The build definition
/// * `completed_builds` - Results of already-completed builds (for dependency resolution)
/// * `manifest` - The full manifest (for looking up definitions)
/// * `config` - Execution configuration
///
/// # Returns
///
/// The result of realizing the build.
pub async fn realize_build(
  hash: &ObjectHash,
  build_def: &BuildDef,
  completed_builds: &HashMap<ObjectHash, BuildResult>,
  manifest: &Manifest,
  config: &ExecuteConfig,
) -> Result<BuildResult, ExecuteError> {
  info!(
    id = ?build_def.id,
    hash = %hash.0,
    "realizing build"
  );

  // Compute the store path for this build
  let store_path = build_dir_path(hash);

  // Check if already built (cache hit)
  if store_path.exists() {
    match read_build_marker(&store_path) {
      Ok(Some(marker)) => {
        if verify_build_hash(&store_path, &marker) {
          debug!(path = ?store_path, "build already exists in store (cache hit)");
          let outputs = resolve_outputs(build_def, &store_path, &[], completed_builds, manifest, config)?;
          return Ok(BuildResult {
            store_path,
            outputs,
            action_results: vec![],
          });
        }
        // Hash mismatch - remove and rebuild
        debug!(path = ?store_path, "removing corrupted build");
        fs::remove_dir_all(&store_path).await?;
      }
      Ok(None) => {
        // No marker - incomplete build
        debug!(path = ?store_path, "incomplete build found, removing");
        fs::remove_dir_all(&store_path).await?;
      }
      Err(e) => {
        // Invalid marker - treat as incomplete
        debug!(path = ?store_path, error = %e, "invalid marker, removing");
        fs::remove_dir_all(&store_path).await?;
      }
    }
  }

  // Create the output directory
  fs::create_dir_all(&store_path).await?;

  // Create resolver for this build
  let mut resolver = BuildResolver::new(completed_builds, manifest, store_path.to_string_lossy().to_string());

  // Execute actions in order
  let mut action_results = Vec::new();

  for (idx, action) in build_def.create_actions.iter().enumerate() {
    debug!(action_idx = idx, "executing action");

    let result = execute_action(action, &resolver, &store_path).await?;

    // Record the result for subsequent actions
    resolver.push_action_result(result.output.clone());
    action_results.push(result);
  }

  // Resolve outputs
  let outputs = resolve_outputs(
    build_def,
    &store_path,
    &action_results,
    completed_builds,
    manifest,
    config,
  )?;

  // Write completion marker
  write_build_complete_marker(&store_path).await?;

  info!(
    id = ?build_def.id,
    path = ?store_path,
    "build complete"
  );

  Ok(BuildResult {
    store_path,
    outputs,
    action_results,
  })
}

/// Realize a single build with unified resolver support.
///
/// This is similar to `realize_build()` but uses `ExecutionResolver` which supports
/// both build and bind placeholder resolution. Use this function when executing
/// via `execute_manifest()` where builds may depend on binds.
///
/// # Arguments
///
/// * `hash` - The build hash
/// * `build_def` - The build definition
/// * `completed_builds` - Results of already-completed builds (for dependency resolution)
/// * `completed_binds` - Results of already-completed binds (for dependency resolution)
/// * `manifest` - The full manifest (for looking up definitions)
/// * `config` - Execution configuration
///
/// # Returns
///
/// The result of realizing the build.
pub async fn realize_build_with_resolver(
  hash: &ObjectHash,
  build_def: &BuildDef,
  completed_builds: &HashMap<ObjectHash, BuildResult>,
  completed_binds: &HashMap<ObjectHash, BindResult>,
  manifest: &Manifest,
  config: &ExecuteConfig,
) -> Result<BuildResult, ExecuteError> {
  info!(
    id = ?build_def.id,
    hash = %hash.0,
    "realizing build (with unified resolver)"
  );

  // Compute the store path for this build
  let store_path = build_dir_path(hash);

  // Check if already built (cache hit)
  if store_path.exists() {
    match read_build_marker(&store_path) {
      Ok(Some(marker)) => {
        if verify_build_hash(&store_path, &marker) {
          debug!(path = ?store_path, "build already exists in store (cache hit)");
          let outputs = resolve_outputs_with_resolver(
            build_def,
            &store_path,
            &[],
            completed_builds,
            completed_binds,
            manifest,
            config,
          )?;
          return Ok(BuildResult {
            store_path,
            outputs,
            action_results: vec![],
          });
        }
        // Hash mismatch - remove and rebuild
        debug!(path = ?store_path, "removing corrupted build");
        fs::remove_dir_all(&store_path).await?;
      }
      Ok(None) => {
        // No marker - incomplete build
        debug!(path = ?store_path, "incomplete build found, removing");
        fs::remove_dir_all(&store_path).await?;
      }
      Err(e) => {
        // Invalid marker - treat as incomplete
        debug!(path = ?store_path, error = %e, "invalid marker, removing");
        fs::remove_dir_all(&store_path).await?;
      }
    }
  }

  // Create the output directory
  fs::create_dir_all(&store_path).await?;

  // Create unified resolver for this build
  let mut resolver = ExecutionResolver::new(
    completed_builds,
    completed_binds,
    manifest,
    store_path.to_string_lossy().to_string(),
  );

  // Execute actions in order
  let mut action_results = Vec::new();

  for (idx, action) in build_def.create_actions.iter().enumerate() {
    debug!(action_idx = idx, "executing action");

    let result = execute_action(action, &resolver, &store_path).await?;

    // Record the result for subsequent actions
    resolver.push_action_result(result.output.clone());
    action_results.push(result);
  }

  // Resolve outputs
  let outputs = resolve_outputs_with_resolver(
    build_def,
    &store_path,
    &action_results,
    completed_builds,
    completed_binds,
    manifest,
    config,
  )?;

  // Write completion marker
  write_build_complete_marker(&store_path).await?;

  info!(
    id = ?build_def.id,
    path = ?store_path,
    "build complete"
  );

  Ok(BuildResult {
    store_path,
    outputs,
    action_results,
  })
}

/// Resolve the outputs from a build definition.
///
/// This substitutes placeholders in the output values with actual paths.
fn resolve_outputs(
  build_def: &BuildDef,
  store_path: &Path,
  action_results: &[ActionResult],
  completed_builds: &HashMap<ObjectHash, BuildResult>,
  manifest: &Manifest,
  _config: &ExecuteConfig,
) -> Result<HashMap<String, String>, ExecuteError> {
  let mut outputs = HashMap::new();

  // Always include "out" pointing to the store path
  outputs.insert("out".to_string(), store_path.to_string_lossy().to_string());

  // Resolve user-defined outputs
  if let Some(def_outputs) = &build_def.outputs {
    // Create a resolver with the action results
    let mut resolver = BuildResolver::new(completed_builds, manifest, store_path.to_string_lossy().to_string());
    for result in action_results {
      resolver.push_action_result(result.output.clone());
    }

    for (name, value) in def_outputs {
      let resolved = placeholder::substitute(value, &resolver)?;
      outputs.insert(name.clone(), resolved);
    }
  }

  Ok(outputs)
}

/// Resolve the outputs from a build definition using the unified resolver.
///
/// This is similar to `resolve_outputs()` but uses `ExecutionResolver` which
/// supports both build and bind placeholder resolution.
fn resolve_outputs_with_resolver(
  build_def: &BuildDef,
  store_path: &Path,
  action_results: &[ActionResult],
  completed_builds: &HashMap<ObjectHash, BuildResult>,
  completed_binds: &HashMap<ObjectHash, BindResult>,
  manifest: &Manifest,
  _config: &ExecuteConfig,
) -> Result<HashMap<String, String>, ExecuteError> {
  let mut outputs = HashMap::new();

  // Always include "out" pointing to the store path
  outputs.insert("out".to_string(), store_path.to_string_lossy().to_string());

  // Resolve user-defined outputs
  if let Some(def_outputs) = &build_def.outputs {
    // Create a unified resolver with the action results
    let mut resolver = ExecutionResolver::new(
      completed_builds,
      completed_binds,
      manifest,
      store_path.to_string_lossy().to_string(),
    );
    for result in action_results {
      resolver.push_action_result(result.output.clone());
    }

    for (name, value) in def_outputs {
      let resolved = placeholder::substitute(value, &resolver)?;
      outputs.insert(name.clone(), resolved);
    }
  }

  Ok(outputs)
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::util::testutil::{echo_msg, shell_cmd};
  use crate::{
    action::{Action, actions::exec::ExecOpts},
    util::hash::Hashable,
  };
  use tempfile::TempDir;

  fn make_simple_build() -> BuildDef {
    let (cmd, args) = echo_msg("hello");
    BuildDef {
      id: None,
      inputs: None,
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
    ExecuteConfig { parallelism: 1 }
  }

  /// Helper to set up a temp store and run a test.
  /// Returns the result of running the async test function.
  fn with_temp_store<F, Fut, T>(f: F) -> T
  where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = T>,
  {
    let temp_dir = TempDir::new().unwrap();
    let store_path = temp_dir.path().join("store");

    temp_env::with_var("SYSLUA_STORE", Some(store_path.to_str().unwrap()), || {
      tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(f())
    })
  }

  #[test]
  fn realize_simple_build() {
    with_temp_store(|| async {
      let build_def = make_simple_build();
      let hash = build_def.compute_hash().unwrap();

      let manifest = Manifest {
        builds: [(hash.clone(), build_def.clone())].into_iter().collect(),
        bindings: Default::default(),
      };

      let config = test_config();
      let completed = HashMap::new();

      let result = realize_build(&hash, &build_def, &completed, &manifest, &config)
        .await
        .unwrap();

      // Check that output directory was created
      assert!(result.store_path.exists());

      // Check that "out" output is set
      assert!(result.outputs.contains_key("out"));
      assert_eq!(result.outputs["out"], result.store_path.to_string_lossy());

      // Check action was executed
      assert_eq!(result.action_results.len(), 1);
      assert_eq!(result.action_results[0].output, "hello");
    });
  }

  #[test]
  fn realize_build_with_custom_outputs() {
    with_temp_store(|| async {
      let (cmd, args) = echo_msg("/path/to/binary");
      let build_def = BuildDef {
        id: None,
        inputs: None,
        create_actions: vec![Action::Exec(ExecOpts {
          bin: cmd.to_string(),
          args: Some(args),
          env: None,
          cwd: None,
        })],
        outputs: Some(
          [
            ("bin".to_string(), "$${action:0}".to_string()),
            ("lib".to_string(), "$${out}/lib".to_string()),
          ]
          .into_iter()
          .collect(),
        ),
      };
      let hash = build_def.compute_hash().unwrap();

      let manifest = Manifest {
        builds: [(hash.clone(), build_def.clone())].into_iter().collect(),
        bindings: Default::default(),
      };

      let config = test_config();
      let completed = HashMap::new();

      let result = realize_build(&hash, &build_def, &completed, &manifest, &config)
        .await
        .unwrap();

      // Check custom outputs
      assert_eq!(result.outputs["bin"], "/path/to/binary");
      assert!(result.outputs["lib"].ends_with("/lib"));
    });
  }

  #[test]
  fn realize_build_with_multiple_actions() {
    with_temp_store(|| async {
      let (cmd1, args1) = echo_msg("step1");
      let (cmd2, args2) = echo_msg("step2");
      let (cmd3, args3) = echo_msg("$${action:0} $${action:1}");
      let build_def = BuildDef {
        id: None,
        inputs: None,
        create_actions: vec![
          Action::Exec(ExecOpts {
            bin: cmd1.to_string(),
            args: Some(args1),
            env: None,
            cwd: None,
          }),
          Action::Exec(ExecOpts {
            bin: cmd2.to_string(),
            args: Some(args2),
            env: None,
            cwd: None,
          }),
          Action::Exec(ExecOpts {
            // Reference previous action output
            bin: cmd3.to_string(),
            args: Some(args3),
            env: None,
            cwd: None,
          }),
        ],
        outputs: Some(
          [("combined".to_string(), "$${action:2}".to_string())]
            .into_iter()
            .collect(),
        ),
      };
      let hash = build_def.compute_hash().unwrap();

      let manifest = Manifest {
        builds: [(hash.clone(), build_def.clone())].into_iter().collect(),
        bindings: Default::default(),
      };

      let config = test_config();
      let completed = HashMap::new();

      let result = realize_build(&hash, &build_def, &completed, &manifest, &config)
        .await
        .unwrap();

      assert_eq!(result.action_results.len(), 3);
      assert_eq!(result.action_results[0].output, "step1");
      assert_eq!(result.action_results[1].output, "step2");
      assert_eq!(result.action_results[2].output, "step1 step2");
      assert_eq!(result.outputs["combined"], "step1 step2");
    });
  }

  #[test]
  fn realize_build_action_failure() {
    with_temp_store(|| async {
      let (cmd, args) = shell_cmd("exit 1");
      let build_def = BuildDef {
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
      let hash = build_def.compute_hash().unwrap();

      let manifest = Manifest {
        builds: [(hash.clone(), build_def.clone())].into_iter().collect(),
        bindings: Default::default(),
      };

      let config = test_config();
      let completed = HashMap::new();

      let result = realize_build(&hash, &build_def, &completed, &manifest, &config).await;

      assert!(matches!(result, Err(ExecuteError::CmdFailed { .. })));
    });
  }

  #[test]
  fn is_build_complete_without_marker() {
    let temp = TempDir::new().unwrap();
    let store_path = temp.path().join("test-build");
    std::fs::create_dir(&store_path).unwrap();

    assert!(!is_build_complete(&store_path));
  }

  #[test]
  fn is_build_complete_with_marker() {
    let temp = TempDir::new().unwrap();
    let store_path = temp.path().join("test-build");
    std::fs::create_dir(&store_path).unwrap();
    std::fs::write(
      store_path.join(BUILD_COMPLETE_MARKER),
      r#"{"version":1,"status":"complete"}"#,
    )
    .unwrap();

    assert!(is_build_complete(&store_path));
  }

  #[test]

  fn successful_build_has_completion_marker() {
    with_temp_store(|| async {
      let build_def = make_simple_build();
      let hash = build_def.compute_hash().unwrap();
      let manifest = Manifest {
        builds: [(hash.clone(), build_def.clone())].into_iter().collect(),
        bindings: Default::default(),
      };
      let config = test_config();

      let result = realize_build(&hash, &build_def, &HashMap::new(), &manifest, &config)
        .await
        .unwrap();

      // Verify marker exists
      assert!(is_build_complete(&result.store_path));

      // Verify marker content using read_build_marker
      let marker = read_build_marker(&result.store_path).unwrap().unwrap();
      assert_eq!(marker.version, 1);
      assert_eq!(marker.status, "complete");
      assert!(marker.output_hash.is_some());
      assert_eq!(marker.output_hash.unwrap().len(), 64);
    });
  }

  #[test]
  fn incomplete_build_triggers_rebuild() {
    with_temp_store(|| async {
      let build_def = make_simple_build();
      let hash = build_def.compute_hash().unwrap();
      let manifest = Manifest {
        builds: [(hash.clone(), build_def.clone())].into_iter().collect(),
        bindings: Default::default(),
      };
      let config = test_config();

      // Pre-create the store path WITHOUT a marker (simulating interrupted build)
      let store_path = build_dir_path(&hash);
      tokio::fs::create_dir_all(&store_path).await.unwrap();
      tokio::fs::write(store_path.join("partial-file"), "incomplete")
        .await
        .unwrap();

      // Run build - should detect incomplete and rebuild
      let result = realize_build(&hash, &build_def, &HashMap::new(), &manifest, &config)
        .await
        .unwrap();

      // Verify marker now exists
      assert!(is_build_complete(&result.store_path));

      // Verify the partial file was removed (directory was cleaned)
      assert!(!result.store_path.join("partial-file").exists());
    });
  }

  #[test]
  fn read_build_marker_missing() {
    let temp = TempDir::new().unwrap();
    let marker = read_build_marker(temp.path()).unwrap();
    assert!(marker.is_none());
  }

  #[test]
  fn read_build_marker_valid() {
    let temp = TempDir::new().unwrap();
    let hash = "a".repeat(64);
    std::fs::write(
      temp.path().join(BUILD_COMPLETE_MARKER),
      format!(r#"{{"version":1,"status":"complete","output_hash":"{}"}}"#, hash),
    )
    .unwrap();

    let marker = read_build_marker(temp.path()).unwrap().unwrap();
    assert_eq!(marker.version, 1);
    assert_eq!(marker.status, "complete");
    assert!(marker.output_hash.is_some());
  }

  #[test]
  fn read_build_marker_without_hash() {
    // Markers from before Chunk 7 don't have output_hash
    let temp = TempDir::new().unwrap();
    std::fs::write(
      temp.path().join(BUILD_COMPLETE_MARKER),
      r#"{"version":1,"status":"complete"}"#,
    )
    .unwrap();

    let marker = read_build_marker(temp.path()).unwrap().unwrap();
    assert_eq!(marker.version, 1);
    assert!(marker.output_hash.is_none());
  }

  #[test]
  fn marker_hash_matches_directory_hash() {
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("file.txt"), "content").unwrap();

    // Compute expected hash (with same exclusions)
    let expected_hash = hash_directory(temp.path(), BUILD_HASH_EXCLUSIONS).unwrap();

    // Write marker using our function
    tokio::runtime::Runtime::new().unwrap().block_on(async {
      write_build_complete_marker(temp.path()).await.unwrap();
    });

    // Read and verify hash matches
    let marker = read_build_marker(temp.path()).unwrap().unwrap();
    assert_eq!(marker.output_hash.unwrap(), expected_hash.0);
  }

  #[test]
  fn verify_valid_build_cache_hit() {
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("file.txt"), "content").unwrap();

    // Write marker with hash
    tokio::runtime::Runtime::new().unwrap().block_on(async {
      write_build_complete_marker(temp.path()).await.unwrap();
    });

    let marker = read_build_marker(temp.path()).unwrap().unwrap();
    assert!(verify_build_hash(temp.path(), &marker));
  }

  #[test]
  fn verify_corrupted_build_triggers_rebuild() {
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("file.txt"), "original").unwrap();

    // Write marker with hash for "original" content
    tokio::runtime::Runtime::new().unwrap().block_on(async {
      write_build_complete_marker(temp.path()).await.unwrap();
    });

    // Corrupt the file
    std::fs::write(temp.path().join("file.txt"), "corrupted").unwrap();

    let marker = read_build_marker(temp.path()).unwrap().unwrap();
    assert!(!verify_build_hash(temp.path(), &marker));
  }

  #[test]
  fn verify_legacy_marker_without_hash_uses_cache() {
    let temp = TempDir::new().unwrap();
    std::fs::write(temp.path().join("file.txt"), "content").unwrap();

    // Write legacy marker without output_hash
    std::fs::write(
      temp.path().join(BUILD_COMPLETE_MARKER),
      r#"{"version":1,"status":"complete"}"#,
    )
    .unwrap();

    let marker = read_build_marker(temp.path()).unwrap().unwrap();
    assert!(verify_build_hash(temp.path(), &marker));
  }

  #[test]
  fn corrupted_build_triggers_full_rebuild() {
    with_temp_store(|| async {
      let build_def = make_simple_build();
      let hash = build_def.compute_hash().unwrap();
      let manifest = Manifest {
        builds: [(hash.clone(), build_def.clone())].into_iter().collect(),
        bindings: Default::default(),
      };
      let config = test_config();

      // First build - creates valid cached build
      let result1 = realize_build(&hash, &build_def, &HashMap::new(), &manifest, &config)
        .await
        .unwrap();

      // Corrupt the build by adding a file
      std::fs::write(result1.store_path.join("corrupt.txt"), "bad data").unwrap();

      // Second build - should detect corruption and rebuild
      let result2 = realize_build(&hash, &build_def, &HashMap::new(), &manifest, &config)
        .await
        .unwrap();

      // Verify rebuild happened (corruption file removed)
      assert!(!result2.store_path.join("corrupt.txt").exists());
      assert!(is_build_complete(&result2.store_path));
    });
  }
}
