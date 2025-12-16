//! Single build realization.
//!
//! This module handles executing all actions for a single build and
//! producing the final BuildResult.

use std::collections::HashMap;
use std::path::PathBuf;

use tokio::fs;
use tracing::{debug, info};

use crate::build::BuildDef;
use crate::build::store::build_path;
use crate::manifest::Manifest;
use crate::placeholder;

use crate::execute::actions::execute_action;
use crate::execute::resolver::{BuildResolver, ExecutionResolver};
use crate::execute::types::{ActionResult, BindResult, BuildResult, ExecuteConfig, ExecuteError};
use crate::util::hash::ObjectHash;

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
    name = %build_def.name,
    version = ?build_def.version,
    hash = %hash.0,
    "realizing build"
  );

  // Compute the store path for this build
  let store_path = build_path(&build_def.name, build_def.version.as_deref(), hash, config.system);

  // Check if already built (cache hit)
  if store_path.exists() {
    debug!(path = ?store_path, "build already exists in store");
    // TODO: Verify the build is complete (e.g., check for marker file)
    // For now, assume existence means complete

    // We need to recover the outputs - for cached builds, read from a metadata file
    // or re-resolve from the definition
    let outputs = resolve_outputs(build_def, &store_path, &[], completed_builds, manifest, config)?;

    return Ok(BuildResult {
      store_path,
      outputs,
      action_results: vec![],
    });
  }

  // Create the output directory
  fs::create_dir_all(&store_path).await?;

  // Create resolver for this build
  let mut resolver = BuildResolver::new(completed_builds, manifest, &store_path, config.system);

  // Execute actions in order
  let mut action_results = Vec::new();

  for (idx, action) in build_def.apply_actions.iter().enumerate() {
    debug!(action_idx = idx, "executing action");

    let result = execute_action(action, &resolver, &store_path, config.shell.as_deref()).await?;

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

  info!(
    name = %build_def.name,
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
    name = %build_def.name,
    version = ?build_def.version,
    hash = %hash.0,
    "realizing build (with unified resolver)"
  );

  // Compute the store path for this build
  let store_path = build_path(&build_def.name, build_def.version.as_deref(), hash, config.system);

  // Check if already built (cache hit)
  if store_path.exists() {
    debug!(path = ?store_path, "build already exists in store");

    // Recover outputs for cached builds
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

  // Create the output directory
  fs::create_dir_all(&store_path).await?;

  // Create unified resolver for this build
  let mut resolver = ExecutionResolver::new(completed_builds, completed_binds, manifest, &store_path, config.system);

  // Execute actions in order
  let mut action_results = Vec::new();

  for (idx, action) in build_def.apply_actions.iter().enumerate() {
    debug!(action_idx = idx, "executing action");

    let result = execute_action(action, &resolver, &store_path, config.shell.as_deref()).await?;

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

  info!(
    name = %build_def.name,
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
  store_path: &PathBuf,
  action_results: &[ActionResult],
  completed_builds: &HashMap<ObjectHash, BuildResult>,
  manifest: &Manifest,
  config: &ExecuteConfig,
) -> Result<HashMap<String, String>, ExecuteError> {
  let mut outputs = HashMap::new();

  // Always include "out" pointing to the store path
  outputs.insert("out".to_string(), store_path.to_string_lossy().to_string());

  // Resolve user-defined outputs
  if let Some(def_outputs) = &build_def.outputs {
    // Create a resolver with the action results
    let mut resolver = BuildResolver::new(completed_builds, manifest, store_path, config.system);
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
  store_path: &PathBuf,
  action_results: &[ActionResult],
  completed_builds: &HashMap<ObjectHash, BuildResult>,
  completed_binds: &HashMap<ObjectHash, BindResult>,
  manifest: &Manifest,
  config: &ExecuteConfig,
) -> Result<HashMap<String, String>, ExecuteError> {
  let mut outputs = HashMap::new();

  // Always include "out" pointing to the store path
  outputs.insert("out".to_string(), store_path.to_string_lossy().to_string());

  // Resolve user-defined outputs
  if let Some(def_outputs) = &build_def.outputs {
    // Create a unified resolver with the action results
    let mut resolver = ExecutionResolver::new(completed_builds, completed_binds, manifest, store_path, config.system);
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
  use crate::{build::BuildAction, util::hash::Hashable};
  use serial_test::serial;
  use tempfile::TempDir;

  fn make_simple_build() -> BuildDef {
    BuildDef {
      name: "test-build".to_string(),
      version: Some("1.0.0".to_string()),
      inputs: None,
      apply_actions: vec![BuildAction::Cmd {
        cmd: "echo hello".to_string(),
        env: None,
        cwd: None,
      }],
      outputs: None,
    }
  }

  fn test_config() -> ExecuteConfig {
    ExecuteConfig {
      parallelism: 1,
      system: false,
      shell: None,
    }
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

    temp_env::with_var("SYSLUA_USER_STORE", Some(store_path.to_str().unwrap()), || {
      tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(f())
    })
  }

  #[test]
  #[serial]
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
  #[serial]
  fn realize_build_with_custom_outputs() {
    with_temp_store(|| async {
      let build_def = BuildDef {
        name: "test-build".to_string(),
        version: Some("1.0.0".to_string()),
        inputs: None,
        apply_actions: vec![BuildAction::Cmd {
          cmd: "echo /path/to/binary".to_string(),
          env: None,
          cwd: None,
        }],
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
  #[serial]
  fn realize_build_with_multiple_actions() {
    with_temp_store(|| async {
      let build_def = BuildDef {
        name: "multi-action".to_string(),
        version: None,
        inputs: None,
        apply_actions: vec![
          BuildAction::Cmd {
            cmd: "echo step1".to_string(),
            env: None,
            cwd: None,
          },
          BuildAction::Cmd {
            cmd: "echo step2".to_string(),
            env: None,
            cwd: None,
          },
          BuildAction::Cmd {
            // Reference previous action output
            cmd: "echo $${action:0} $${action:1}".to_string(),
            env: None,
            cwd: None,
          },
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
  #[serial]
  fn realize_build_action_failure() {
    with_temp_store(|| async {
      let build_def = BuildDef {
        name: "failing-build".to_string(),
        version: None,
        inputs: None,
        apply_actions: vec![BuildAction::Cmd {
          cmd: "exit 1".to_string(),
          env: None,
          cwd: None,
        }],
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
}
