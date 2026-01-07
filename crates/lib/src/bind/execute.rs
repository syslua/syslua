//! Single bind application and destruction.
//!
//! This module handles executing actions for a single bind and
//! producing the final BindResult.

use std::collections::HashMap;
use std::path::Path;

use tempfile::TempDir;
use tracing::debug;

use crate::action::{Action, execute_action};
use crate::bind::BindDef;
use crate::execute::resolver::BindCtxResolver;
use crate::execute::types::{ActionResult, BindResult, ExecuteError};
use crate::placeholder;
use crate::util::hash::ObjectHash;

/// Apply a single bind.
///
/// This executes all apply_actions in the bind definition and produces the
/// final BindResult with resolved outputs.
///
/// # Arguments
///
/// * `hash` - The bind hash
/// * `bind_def` - The bind definition
/// * `resolver` - A resolver that can resolve placeholders (including completed builds/binds)
///
/// # Returns
///
/// The result of applying the bind.
pub async fn apply_bind(
  hash: &ObjectHash,
  bind_def: &BindDef,
  resolver: &BindCtxResolver<'_>,
) -> Result<BindResult, ExecuteError> {
  debug!(hash = %hash.0, "applying bind");

  // Create a temporary working directory for the bind's $${out}
  let temp_dir = TempDir::new()?;
  let out_dir = temp_dir.path();

  // Create a child resolver with its own out_dir and action_results
  let mut bind_resolver = resolver.with_out_dir(out_dir.to_string_lossy().to_string());

  // Execute actions in order
  let (action_results, outputs) =
    execute_bind_actions(&bind_def.create_actions, &mut bind_resolver, bind_def, out_dir).await?;

  debug!(hash = %hash.0, "bind applied");

  // Keep the temp dir alive until the result is processed
  // In a real implementation, we might want to persist state somewhere
  std::mem::forget(temp_dir);

  Ok(BindResult {
    outputs,
    action_results,
  })
}

/// Destroy a previously applied bind.
///
/// This executes the destroy_actions for a bind, typically used during rollback.
///
/// # Arguments
///
/// * `hash` - The bind hash
/// * `bind_def` - The bind definition
/// * `bind_result` - The result from when the bind was applied (provides outputs)
/// * `resolver` - A resolver for placeholder resolution
///
/// # Returns
///
/// Ok(()) on success, or an error if destruction failed.
pub async fn destroy_bind(
  hash: &ObjectHash,
  bind_def: &BindDef,
  bind_result: &BindResult,
  resolver: &BindCtxResolver<'_>,
) -> Result<(), ExecuteError> {
  let destroy_actions = &bind_def.destroy_actions;
  let _ = bind_result; // TODO: May be used in future for referencing applied outputs

  debug!(hash = %hash.0, "destroying bind");

  // Create a temporary directory for destroy actions
  let temp_dir = TempDir::new()?;
  let out_dir = temp_dir.path();

  // Create a child resolver with its own out_dir and action_results
  let mut bind_resolver = resolver.with_out_dir(out_dir.to_string_lossy().to_string());

  // Execute destroy actions
  let _ = execute_bind_actions_raw(destroy_actions, &mut bind_resolver, out_dir).await?;

  debug!(hash = %hash.0, "bind destroyed");

  Ok(())
}

/// Update a previously applied bind with a new definition.
///
/// This executes the update_actions from the new bind definition, passing
/// the old outputs and new inputs to the update function.
///
/// # Arguments
///
/// * `old_hash` - Hash of the currently applied bind
/// * `new_hash` - Hash of the new bind definition
/// * `new_bind_def` - The new bind definition (must have update_actions)
/// * `old_bind_result` - The result from when the bind was originally applied
/// * `resolver` - A resolver for placeholder resolution
///
/// # Returns
///
/// A new `BindResult` with updated outputs.
pub async fn update_bind(
  old_hash: &ObjectHash,
  new_hash: &ObjectHash,
  new_bind_def: &BindDef,
  old_bind_result: &BindResult,
  resolver: &BindCtxResolver<'_>,
) -> Result<BindResult, ExecuteError> {
  let _ = old_bind_result; // TODO: May be used in future for referencing old outputs
  debug!(old_hash = %old_hash.0, new_hash = %new_hash.0, "updating bind");

  // Create a temporary working directory for the bind's $${out}
  let temp_dir = TempDir::new()?;
  let out_dir = temp_dir.path();

  // Get update_actions (caller should ensure this exists)
  let update_actions = new_bind_def
    .update_actions
    .as_ref()
    .ok_or_else(|| ExecuteError::CmdFailed {
      cmd: "update_bind called without update_actions".to_string(),
      code: None,
    })?;

  // Create a child resolver with its own out_dir and action_results
  let mut bind_resolver = resolver.with_out_dir(out_dir.to_string_lossy().to_string());

  // Execute update actions
  let (action_results, outputs) =
    execute_bind_actions(&update_actions, &mut bind_resolver, new_bind_def, out_dir).await?;

  debug!(old_hash = %old_hash.0, new_hash = %new_hash.0, "bind updated");

  // Keep the temp dir alive
  std::mem::forget(temp_dir);

  Ok(BindResult {
    outputs,
    action_results,
  })
}

/// Check if a bind has drifted from its expected state.
///
/// Executes the bind's check_actions and interprets the result.
/// Returns `None` if the bind has no check callback defined.
pub async fn check_bind(
  hash: &ObjectHash,
  bind_def: &BindDef,
  bind_result: &BindResult,
  resolver: &BindCtxResolver<'_>,
) -> Result<Option<crate::bind::BindCheckResult>, ExecuteError> {
  let _ = bind_result; // TODO: May be used in future for referencing applied outputs
  let Some(ref check_actions) = bind_def.check_actions else {
    return Ok(None);
  };
  let Some(ref check_outputs) = bind_def.check_outputs else {
    return Ok(None);
  };

  debug!(hash = %hash.0, "checking bind for drift");

  let temp_dir = TempDir::new()?;
  let out_dir = temp_dir.path();

  // Create a child resolver with its own out_dir and action_results
  let mut check_resolver = resolver.with_out_dir(out_dir.to_string_lossy().to_string());

  // Execute check actions (this populates action_results in check_resolver)
  execute_bind_check_actions(check_actions, &mut check_resolver, out_dir).await?;

  // Resolve check outputs using the resolver (now has action results)
  let drifted_str = placeholder::substitute(&check_outputs.drifted, &check_resolver)?;
  let drifted = drifted_str == "true";

  let message = match &check_outputs.message {
    Some(msg_pattern) => Some(placeholder::substitute(msg_pattern, &check_resolver)?),
    None => None,
  };

  debug!(hash = %hash.0, drifted = drifted, "check complete");

  Ok(Some(crate::bind::BindCheckResult { drifted, message }))
}

async fn execute_bind_check_actions(
  actions: &[Action],
  resolver: &mut BindCtxResolver<'_>,
  out_dir: &Path,
) -> Result<Vec<ActionResult>, ExecuteError> {
  let mut action_results = Vec::new();

  for (idx, action) in actions.iter().enumerate() {
    debug!(action_idx = idx, "executing check action");

    let result = execute_action(action, resolver, out_dir).await?;

    resolver.push_action_result(result.output.clone());
    action_results.push(result);
  }

  Ok(action_results)
}

/// Execute bind actions and resolve outputs.
async fn execute_bind_actions(
  actions: &[Action],
  resolver: &mut BindCtxResolver<'_>,
  bind_def: &BindDef,
  out_dir: &Path,
) -> Result<(Vec<ActionResult>, HashMap<String, String>), ExecuteError> {
  let mut action_results = Vec::new();

  for (idx, action) in actions.iter().enumerate() {
    debug!(action_idx = idx, "executing bind action");

    let result = execute_action(action, resolver, out_dir).await?;

    // Record the result for subsequent actions
    resolver.push_action_result(result.output.clone());
    action_results.push(result);
  }

  // Resolve outputs
  let outputs = resolve_bind_outputs(bind_def, resolver)?;

  Ok((action_results, outputs))
}

/// Execute bind actions without output resolution (used for destroy).
async fn execute_bind_actions_raw(
  actions: &[Action],
  resolver: &mut BindCtxResolver<'_>,
  out_dir: &Path,
) -> Result<Vec<ActionResult>, ExecuteError> {
  let mut action_results = Vec::new();

  for (idx, action) in actions.iter().enumerate() {
    debug!(action_idx = idx, "executing destroy action");

    let result = execute_action(action, resolver, out_dir).await?;

    resolver.push_action_result(result.output.clone());
    action_results.push(result);
  }

  Ok(action_results)
}

/// Resolve the outputs from a bind definition.
fn resolve_bind_outputs(
  bind_def: &BindDef,
  resolver: &BindCtxResolver<'_>,
) -> Result<HashMap<String, String>, ExecuteError> {
  let mut outputs = HashMap::new();

  if let Some(def_outputs) = &bind_def.outputs {
    for (name, value) in def_outputs {
      let resolved = placeholder::substitute(value, resolver)?;
      outputs.insert(name.clone(), resolved);
    }
  }

  Ok(outputs)
}

#[cfg(test)]
mod tests {
  use std::vec;

  use super::*;
  use crate::execute::types::BuildResult;
  use crate::manifest::Manifest;
  use crate::util::testutil::{echo_msg, shell_cmd};
  use crate::{action::actions::exec::ExecOpts, util::hash::Hashable};

  /// Create a test resolver with empty collections.
  fn test_resolver() -> (
    HashMap<ObjectHash, BuildResult>,
    HashMap<ObjectHash, BindResult>,
    Manifest,
  ) {
    (HashMap::new(), HashMap::new(), Manifest::default())
  }

  fn make_simple_bind() -> BindDef {
    let (cmd, args) = echo_msg("applied");
    BindDef {
      id: None,
      inputs: None,
      outputs: None,
      create_actions: vec![Action::Exec(ExecOpts {
        bin: cmd.to_string(),
        args: Some(args),
        env: None,
        cwd: None,
      })],
      update_actions: None,
      destroy_actions: vec![],
      check_actions: None,
      check_outputs: None,
    }
  }

  #[tokio::test]
  async fn apply_simple_bind() {
    let bind_def = make_simple_bind();
    let hash = bind_def.compute_hash().unwrap();
    let (builds, binds, manifest) = test_resolver();
    let resolver = BindCtxResolver::new(&builds, &binds, &manifest, "/tmp".to_string());

    let result = apply_bind(&hash, &bind_def, &resolver).await.unwrap();

    assert_eq!(result.action_results.len(), 1);
    assert_eq!(result.action_results[0].output, "applied");
  }

  #[tokio::test]
  async fn apply_bind_with_outputs() {
    let (cmd, args) = echo_msg("/path/to/link");
    let bind_def = BindDef {
      id: None,
      inputs: None,
      outputs: Some([("link".to_string(), "$${action:0}".to_string())].into_iter().collect()),
      create_actions: vec![Action::Exec(ExecOpts {
        bin: cmd.to_string(),
        args: Some(args),
        env: None,
        cwd: None,
      })],
      update_actions: None,
      destroy_actions: vec![],
      check_actions: None,
      check_outputs: None,
    };
    let hash = bind_def.compute_hash().unwrap();
    let (builds, binds, manifest) = test_resolver();
    let resolver = BindCtxResolver::new(&builds, &binds, &manifest, "/tmp".to_string());

    let result = apply_bind(&hash, &bind_def, &resolver).await.unwrap();

    assert_eq!(result.outputs["link"], "/path/to/link");
  }

  #[tokio::test]
  async fn apply_bind_with_out_placeholder() {
    let (cmd, args) = echo_msg("$${out}");
    let bind_def = BindDef {
      id: None,
      inputs: None,
      outputs: Some([("dir".to_string(), "$${out}".to_string())].into_iter().collect()),
      create_actions: vec![Action::Exec(ExecOpts {
        bin: cmd.to_string(),
        args: Some(args),
        env: None,
        cwd: None,
      })],
      update_actions: None,
      destroy_actions: vec![],
      check_actions: None,
      check_outputs: None,
    };
    let hash = bind_def.compute_hash().unwrap();
    let (builds, binds, manifest) = test_resolver();
    let resolver = BindCtxResolver::new(&builds, &binds, &manifest, "/tmp".to_string());

    let result = apply_bind(&hash, &bind_def, &resolver).await.unwrap();

    // The output should be a temp directory path
    assert!(!result.outputs["dir"].is_empty());
    // Check path looks reasonable: starts with / (Unix) or has drive letter (Windows)
    let output = &result.action_results[0].output;
    assert!(
      output.starts_with('/') || output.starts_with('\\') || output.chars().nth(1) == Some(':'),
      "Output path should be absolute: {}",
      output
    );
  }

  #[tokio::test]
  async fn apply_bind_with_build_dependency() {
    use std::path::PathBuf;

    let (cmd, args) = echo_msg("$${build:abc123:bin}");
    let bind_def = BindDef {
      id: None,
      inputs: None,
      outputs: None,
      create_actions: vec![Action::Exec(ExecOpts {
        bin: cmd.to_string(),
        args: Some(args),
        env: None,
        cwd: None,
      })],
      update_actions: None,
      destroy_actions: vec![],
      check_actions: None,
      check_outputs: None,
    };
    let hash = bind_def.compute_hash().unwrap();

    let mut build_outputs = HashMap::new();
    build_outputs.insert("bin".to_string(), "/store/obj/myapp/bin".to_string());
    let build_result = BuildResult {
      store_path: PathBuf::from("/store/obj/myapp"),
      outputs: build_outputs,
      action_results: vec![],
    };
    let mut builds = HashMap::new();
    builds.insert(ObjectHash("abc123def456".to_string()), build_result);
    let binds = HashMap::new();
    let manifest = Manifest::default();
    let resolver = BindCtxResolver::new(&builds, &binds, &manifest, "/tmp".to_string());

    let result = apply_bind(&hash, &bind_def, &resolver).await.unwrap();

    assert_eq!(result.action_results[0].output, "/store/obj/myapp/bin");
  }

  #[tokio::test]
  async fn destroy_bind_with_actions() {
    let (apply_cmd, apply_args) = echo_msg("applied");
    let (destroy_cmd, destroy_args) = echo_msg("destroyed");
    let bind_def = BindDef {
      id: None,
      inputs: None,
      outputs: Some(
        [("path".to_string(), "/created/path".to_string())]
          .into_iter()
          .collect(),
      ),
      create_actions: vec![Action::Exec(ExecOpts {
        bin: apply_cmd.to_string(),
        args: Some(apply_args),
        env: None,
        cwd: None,
      })],
      update_actions: None,
      destroy_actions: vec![Action::Exec(ExecOpts {
        bin: destroy_cmd.to_string(),
        args: Some(destroy_args),
        env: None,
        cwd: None,
      })],
      check_actions: None,
      check_outputs: None,
    };
    let hash = bind_def.compute_hash().unwrap();
    let (builds, binds, manifest) = test_resolver();
    let resolver = BindCtxResolver::new(&builds, &binds, &manifest, "/tmp".to_string());

    // First apply
    let bind_result = apply_bind(&hash, &bind_def, &resolver).await.unwrap();

    // Then destroy
    let destroy_result = destroy_bind(&hash, &bind_def, &bind_result, &resolver).await;

    assert!(destroy_result.is_ok());
  }

  #[tokio::test]
  async fn destroy_bind_without_actions() {
    let bind_def = make_simple_bind();
    let hash = bind_def.compute_hash().unwrap();
    let (builds, binds, manifest) = test_resolver();
    let resolver = BindCtxResolver::new(&builds, &binds, &manifest, "/tmp".to_string());

    let bind_result = BindResult {
      outputs: HashMap::new(),
      action_results: vec![],
    };

    // Destroy should succeed even with no destroy_actions
    let result = destroy_bind(&hash, &bind_def, &bind_result, &resolver).await;
    assert!(result.is_ok());
  }

  #[tokio::test]
  async fn apply_bind_action_failure() {
    let (cmd, args) = shell_cmd("exit 1");
    let bind_def = BindDef {
      id: None,
      inputs: None,
      outputs: None,
      create_actions: vec![Action::Exec(ExecOpts {
        bin: cmd.to_string(),
        args: Some(args),
        env: None,
        cwd: None,
      })],
      update_actions: None,
      destroy_actions: vec![],
      check_actions: None,
      check_outputs: None,
    };
    let hash = bind_def.compute_hash().unwrap();
    let (builds, binds, manifest) = test_resolver();
    let resolver = BindCtxResolver::new(&builds, &binds, &manifest, "/tmp".to_string());

    let result = apply_bind(&hash, &bind_def, &resolver).await;

    assert!(matches!(result, Err(ExecuteError::CmdFailed { .. })));
  }

  #[tokio::test]
  async fn apply_bind_multiple_actions() {
    let (cmd1, args1) = echo_msg("step1");
    let (cmd2, args2) = echo_msg("step2");
    let (cmd3, args3) = echo_msg("$${action:0} $${action:1}");
    let bind_def = BindDef {
      id: None,
      inputs: None,
      outputs: Some(
        [("combined".to_string(), "$${action:2}".to_string())]
          .into_iter()
          .collect(),
      ),
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
          bin: cmd3.to_string(),
          args: Some(args3),
          env: None,
          cwd: None,
        }),
      ],
      update_actions: None,
      destroy_actions: vec![],
      check_actions: None,
      check_outputs: None,
    };
    let hash = bind_def.compute_hash().unwrap();
    let (builds, binds, manifest) = test_resolver();
    let resolver = BindCtxResolver::new(&builds, &binds, &manifest, "/tmp".to_string());

    let result = apply_bind(&hash, &bind_def, &resolver).await.unwrap();

    assert_eq!(result.action_results.len(), 3);
    assert_eq!(result.action_results[0].output, "step1");
    assert_eq!(result.action_results[1].output, "step2");
    assert_eq!(result.action_results[2].output, "step1 step2");
    assert_eq!(result.outputs["combined"], "step1 step2");
  }

  #[tokio::test]
  async fn update_bind_executes_update_actions() {
    let (create_cmd, create_args) = echo_msg("created");
    let (update_cmd, update_args) = echo_msg("updated");
    let bind_def = BindDef {
      id: Some("test-bind".to_string()),
      inputs: None,
      outputs: Some(
        [("status".to_string(), "$${action:0}".to_string())]
          .into_iter()
          .collect(),
      ),
      create_actions: vec![Action::Exec(ExecOpts {
        bin: create_cmd.to_string(),
        args: Some(create_args),
        env: None,
        cwd: None,
      })],
      update_actions: Some(vec![Action::Exec(ExecOpts {
        bin: update_cmd.to_string(),
        args: Some(update_args),
        env: None,
        cwd: None,
      })]),
      destroy_actions: vec![],
      check_actions: None,
      check_outputs: None,
    };
    let old_hash = ObjectHash("old_hash".to_string());
    let new_hash = bind_def.compute_hash().unwrap();
    let (builds, binds, manifest) = test_resolver();
    let resolver = BindCtxResolver::new(&builds, &binds, &manifest, "/tmp".to_string());

    // Simulate previous apply result
    let old_bind_result = BindResult {
      outputs: [("status".to_string(), "created".to_string())].into_iter().collect(),
      action_results: vec![],
    };

    let result = update_bind(&old_hash, &new_hash, &bind_def, &old_bind_result, &resolver)
      .await
      .unwrap();

    // Should have executed the update action
    assert_eq!(result.action_results.len(), 1);
    assert_eq!(result.action_results[0].output, "updated");
    assert_eq!(result.outputs["status"], "updated");
  }

  #[tokio::test]
  async fn update_bind_returns_new_outputs() {
    let (create_cmd, create_args) = echo_msg("/old/path");
    let (update_cmd, update_args) = echo_msg("/new/path");
    let bind_def = BindDef {
      id: Some("path-bind".to_string()),
      inputs: None,
      outputs: Some([("path".to_string(), "$${action:0}".to_string())].into_iter().collect()),
      create_actions: vec![Action::Exec(ExecOpts {
        bin: create_cmd.to_string(),
        args: Some(create_args),
        env: None,
        cwd: None,
      })],
      update_actions: Some(vec![Action::Exec(ExecOpts {
        bin: update_cmd.to_string(),
        args: Some(update_args),
        env: None,
        cwd: None,
      })]),
      destroy_actions: vec![],
      check_actions: None,
      check_outputs: None,
    };
    let old_hash = ObjectHash("old".to_string());
    let new_hash = bind_def.compute_hash().unwrap();
    let (builds, binds, manifest) = test_resolver();
    let resolver = BindCtxResolver::new(&builds, &binds, &manifest, "/tmp".to_string());

    let old_bind_result = BindResult {
      outputs: [("path".to_string(), "/old/path".to_string())].into_iter().collect(),
      action_results: vec![],
    };

    let result = update_bind(&old_hash, &new_hash, &bind_def, &old_bind_result, &resolver)
      .await
      .unwrap();

    // New outputs should reflect the update action
    assert_eq!(result.outputs["path"], "/new/path");
  }

  #[tokio::test]
  async fn update_bind_fails_without_update_actions() {
    let (cmd, args) = echo_msg("created");
    let bind_def = BindDef {
      id: Some("no-update-bind".to_string()),
      inputs: None,
      outputs: None,
      create_actions: vec![Action::Exec(ExecOpts {
        bin: cmd.to_string(),
        args: Some(args),
        env: None,
        cwd: None,
      })],
      update_actions: None, // No update actions!
      destroy_actions: vec![],
      check_actions: None,
      check_outputs: None,
    };
    let old_hash = ObjectHash("old".to_string());
    let new_hash = bind_def.compute_hash().unwrap();
    let (builds, binds, manifest) = test_resolver();
    let resolver = BindCtxResolver::new(&builds, &binds, &manifest, "/tmp".to_string());

    let old_bind_result = BindResult {
      outputs: HashMap::new(),
      action_results: vec![],
    };

    let result = update_bind(&old_hash, &new_hash, &bind_def, &old_bind_result, &resolver).await;

    assert!(matches!(result, Err(ExecuteError::CmdFailed { .. })));
  }

  #[tokio::test]
  async fn update_bind_with_multiple_actions() {
    let (cmd1, args1) = echo_msg("step1");
    let (cmd2, args2) = echo_msg("step2");
    let (cmd3, args3) = echo_msg("$${action:0}-$${action:1}");
    let bind_def = BindDef {
      id: Some("multi-step-update".to_string()),
      inputs: None,
      outputs: Some(
        [("result".to_string(), "$${action:2}".to_string())]
          .into_iter()
          .collect(),
      ),
      create_actions: vec![Action::Exec(ExecOpts {
        bin: cmd1.to_string(),
        args: Some(args1.clone()),
        env: None,
        cwd: None,
      })],
      update_actions: Some(vec![
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
          bin: cmd3.to_string(),
          args: Some(args3),
          env: None,
          cwd: None,
        }),
      ]),
      destroy_actions: vec![],
      check_actions: None,
      check_outputs: None,
    };
    let old_hash = ObjectHash("old".to_string());
    let new_hash = bind_def.compute_hash().unwrap();
    let (builds, binds, manifest) = test_resolver();
    let resolver = BindCtxResolver::new(&builds, &binds, &manifest, "/tmp".to_string());

    let old_bind_result = BindResult {
      outputs: [("result".to_string(), "old-result".to_string())].into_iter().collect(),
      action_results: vec![],
    };

    let result = update_bind(&old_hash, &new_hash, &bind_def, &old_bind_result, &resolver)
      .await
      .unwrap();

    assert_eq!(result.action_results.len(), 3);
    assert_eq!(result.outputs["result"], "step1-step2");
  }

  // ============ check_bind tests ============

  #[tokio::test]
  async fn check_bind_returns_none_without_check_actions() {
    // A bind with no check_actions should return None
    let bind_def = make_simple_bind();
    let hash = bind_def.compute_hash().unwrap();
    let (builds, binds, manifest) = test_resolver();
    let resolver = BindCtxResolver::new(&builds, &binds, &manifest, "/tmp".to_string());
    let bind_result = BindResult {
      outputs: HashMap::new(),
      action_results: vec![],
    };

    let result = check_bind(&hash, &bind_def, &bind_result, &resolver).await.unwrap();

    assert!(result.is_none());
  }

  #[tokio::test]
  async fn check_bind_parses_drifted_true() {
    use crate::bind::BindCheckOutputs;

    // Create a bind with check that returns drifted=true
    let (cmd, args) = echo_msg("true");
    let bind_def = BindDef {
      id: Some("check-test".to_string()),
      inputs: None,
      outputs: None,
      create_actions: vec![],
      update_actions: None,
      destroy_actions: vec![],
      check_actions: Some(vec![Action::Exec(ExecOpts {
        bin: cmd.to_string(),
        args: Some(args),
        env: None,
        cwd: None,
      })]),
      check_outputs: Some(BindCheckOutputs {
        drifted: "$${action:0}".to_string(),
        message: Some("file missing".to_string()),
      }),
    };
    let hash = bind_def.compute_hash().unwrap();
    let (builds, binds, manifest) = test_resolver();
    let resolver = BindCtxResolver::new(&builds, &binds, &manifest, "/tmp".to_string());
    let bind_result = BindResult {
      outputs: HashMap::new(),
      action_results: vec![],
    };

    let result = check_bind(&hash, &bind_def, &bind_result, &resolver).await.unwrap();

    assert!(result.is_some());
    let check_result = result.unwrap();
    assert!(check_result.drifted);
    assert_eq!(check_result.message, Some("file missing".to_string()));
  }

  #[tokio::test]
  async fn check_bind_parses_drifted_false() {
    use crate::bind::BindCheckOutputs;

    // Create a bind with check that returns drifted=false
    let (cmd, args) = echo_msg("false");
    let bind_def = BindDef {
      id: Some("check-test".to_string()),
      inputs: None,
      outputs: None,
      create_actions: vec![],
      update_actions: None,
      destroy_actions: vec![],
      check_actions: Some(vec![Action::Exec(ExecOpts {
        bin: cmd.to_string(),
        args: Some(args),
        env: None,
        cwd: None,
      })]),
      check_outputs: Some(BindCheckOutputs {
        drifted: "$${action:0}".to_string(),
        message: None,
      }),
    };
    let hash = bind_def.compute_hash().unwrap();
    let (builds, binds, manifest) = test_resolver();
    let resolver = BindCtxResolver::new(&builds, &binds, &manifest, "/tmp".to_string());
    let bind_result = BindResult {
      outputs: HashMap::new(),
      action_results: vec![],
    };

    let result = check_bind(&hash, &bind_def, &bind_result, &resolver).await.unwrap();

    assert!(result.is_some());
    let check_result = result.unwrap();
    assert!(!check_result.drifted);
    assert!(check_result.message.is_none());
  }

  #[tokio::test]
  async fn check_bind_executes_actions_and_resolves_placeholders() {
    use crate::bind::BindCheckOutputs;

    // Create a bind that executes multiple check actions and uses placeholders
    let (cmd1, args1) = echo_msg("check1");
    let (cmd2, args2) = echo_msg("$${action:0}-check2");
    let bind_def = BindDef {
      id: Some("multi-check".to_string()),
      inputs: None,
      outputs: None,
      create_actions: vec![],
      update_actions: None,
      destroy_actions: vec![],
      check_actions: Some(vec![
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
      ]),
      check_outputs: Some(BindCheckOutputs {
        drifted: "true".to_string(),
        message: Some("$${action:1}".to_string()),
      }),
    };
    let hash = bind_def.compute_hash().unwrap();
    let (builds, binds, manifest) = test_resolver();
    let resolver = BindCtxResolver::new(&builds, &binds, &manifest, "/tmp".to_string());
    let bind_result = BindResult {
      outputs: HashMap::new(),
      action_results: vec![],
    };

    let result = check_bind(&hash, &bind_def, &bind_result, &resolver).await.unwrap();

    assert!(result.is_some());
    let check_result = result.unwrap();
    assert!(check_result.drifted);
    // The second action should have received the resolved first action output
    assert_eq!(check_result.message, Some("check1-check2".to_string()));
  }
}
