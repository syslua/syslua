//! Single bind application and destruction.
//!
//! This module handles executing actions for a single bind and
//! producing the final BindResult.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use tempfile::TempDir;
use tracing::{debug, info};

use crate::action::{Action, execute_action};
use crate::bind::BindDef;
use crate::execute::types::{ActionResult, BindResult, ExecuteError};
use crate::placeholder::{self, Resolver};
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
/// * `config` - Execution configuration
///
/// # Returns
///
/// The result of applying the bind.
pub async fn apply_bind<R: Resolver>(
  hash: &ObjectHash,
  bind_def: &BindDef,
  resolver: &R,
) -> Result<BindResult, ExecuteError> {
  info!(hash = %hash.0, "applying bind");

  // Create a temporary working directory for the bind's $${out}
  let temp_dir = TempDir::new()?;
  let out_dir = temp_dir.path();

  // Create a resolver that wraps the provided one but overrides $${out}
  let bind_resolver = BindApplyResolver {
    inner: resolver,
    out_dir: fs::canonicalize(out_dir)?.to_string_lossy().to_string(),
    action_results: Vec::new(),
  };

  // Execute actions in order
  let (action_results, outputs) =
    execute_bind_actions(&bind_def.create_actions, bind_resolver, bind_def, out_dir).await?;

  info!(hash = %hash.0, "bind applied");

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
/// * `config` - Execution configuration
///
/// # Returns
///
/// Ok(()) on success, or an error if destruction failed.
pub async fn destroy_bind<R: Resolver>(
  hash: &ObjectHash,
  bind_def: &BindDef,
  bind_result: &BindResult,
  resolver: &R,
) -> Result<(), ExecuteError> {
  let destroy_actions = &bind_def.destroy_actions;

  info!(hash = %hash.0, "destroying bind");

  // Create a temporary directory for destroy actions
  let temp_dir = TempDir::new()?;
  let out_dir = temp_dir.path();

  // For destroy, we use the outputs from when the bind was applied
  // so that destroy commands can reference them
  let bind_resolver = BindDestroyResolver {
    inner: resolver,
    out_dir: fs::canonicalize(out_dir)?.to_string_lossy().to_string(),
    applied_outputs: &bind_result.outputs,
    action_results: Vec::new(),
  };

  // Execute destroy actions
  let _ = execute_bind_actions_raw(destroy_actions, bind_resolver, out_dir).await?;

  info!(hash = %hash.0, "bind destroyed");

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
pub async fn update_bind<R: Resolver>(
  old_hash: &ObjectHash,
  new_hash: &ObjectHash,
  new_bind_def: &BindDef,
  old_bind_result: &BindResult,
  resolver: &R,
) -> Result<BindResult, ExecuteError> {
  info!(old_hash = %old_hash.0, new_hash = %new_hash.0, "updating bind");

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

  // Create a resolver that has access to old outputs for placeholder resolution
  let bind_resolver = BindUpdateResolver {
    inner: resolver,
    out_dir: fs::canonicalize(out_dir)?.to_string_lossy().to_string(),
    old_outputs: &old_bind_result.outputs,
    action_results: Vec::new(),
  };

  // Execute update actions
  let (action_results, outputs) =
    execute_bind_update_actions(update_actions, bind_resolver, new_bind_def, out_dir).await?;

  info!(old_hash = %old_hash.0, new_hash = %new_hash.0, "bind updated");

  // Keep the temp dir alive
  std::mem::forget(temp_dir);

  Ok(BindResult {
    outputs,
    action_results,
  })
}

/// Execute bind actions and resolve outputs.
async fn execute_bind_actions<R: Resolver>(
  actions: &[Action],
  mut resolver: BindApplyResolver<'_, R>,
  bind_def: &BindDef,
  out_dir: &Path,
) -> Result<(Vec<ActionResult>, HashMap<String, String>), ExecuteError> {
  let mut action_results = Vec::new();

  for (idx, action) in actions.iter().enumerate() {
    debug!(action_idx = idx, "executing bind action");

    let result = execute_action(action, &resolver, out_dir).await?;

    // Record the result for subsequent actions
    resolver.action_results.push(result.output.clone());
    action_results.push(result);
  }

  // Resolve outputs
  let outputs = resolve_bind_outputs(bind_def, &resolver)?;

  Ok((action_results, outputs))
}

/// Execute bind actions without output resolution (used for destroy).
async fn execute_bind_actions_raw<R: Resolver>(
  actions: &[Action],
  mut resolver: BindDestroyResolver<'_, R>,
  out_dir: &Path,
) -> Result<Vec<ActionResult>, ExecuteError> {
  let mut action_results = Vec::new();

  for (idx, action) in actions.iter().enumerate() {
    debug!(action_idx = idx, "executing destroy action");

    let result = execute_action(action, &resolver, out_dir).await?;

    resolver.action_results.push(result.output.clone());
    action_results.push(result);
  }

  Ok(action_results)
}

/// Execute bind update actions and resolve outputs.
async fn execute_bind_update_actions<R: Resolver>(
  actions: &[Action],
  mut resolver: BindUpdateResolver<'_, R>,
  bind_def: &BindDef,
  out_dir: &Path,
) -> Result<(Vec<ActionResult>, HashMap<String, String>), ExecuteError> {
  let mut action_results = Vec::new();

  for (idx, action) in actions.iter().enumerate() {
    debug!(action_idx = idx, "executing update action");

    let result = execute_action(action, &resolver, out_dir).await?;

    // Record the result for subsequent actions
    resolver.action_results.push(result.output.clone());
    action_results.push(result);
  }

  // Resolve outputs using the update resolver
  let outputs = resolve_bind_update_outputs(bind_def, &resolver)?;

  Ok((action_results, outputs))
}

/// Resolve the outputs from a bind definition (for apply).
fn resolve_bind_outputs<R: Resolver>(
  bind_def: &BindDef,
  resolver: &BindApplyResolver<'_, R>,
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

/// Resolve the outputs from a bind definition (for update).
fn resolve_bind_update_outputs<R: Resolver>(
  bind_def: &BindDef,
  resolver: &BindUpdateResolver<'_, R>,
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

/// Resolver for bind apply actions.
///
/// Wraps an inner resolver but overrides $${out} to point to the bind's
/// working directory.
struct BindApplyResolver<'a, R: Resolver> {
  inner: &'a R,
  out_dir: String,
  action_results: Vec<String>,
}

impl<R: Resolver> Resolver for BindApplyResolver<'_, R> {
  fn resolve_action(&self, index: usize) -> Result<&str, crate::placeholder::PlaceholderError> {
    self
      .action_results
      .get(index)
      .map(|s| s.as_str())
      .ok_or(crate::placeholder::PlaceholderError::UnresolvedAction(index))
  }

  fn resolve_build(&self, hash: &str, output: &str) -> Result<&str, crate::placeholder::PlaceholderError> {
    self.inner.resolve_build(hash, output)
  }

  fn resolve_bind(&self, hash: &str, output: &str) -> Result<&str, crate::placeholder::PlaceholderError> {
    self.inner.resolve_bind(hash, output)
  }

  fn resolve_out(&self) -> Result<&str, crate::placeholder::PlaceholderError> {
    Ok(&self.out_dir)
  }
}

/// Resolver for bind destroy actions.
///
/// Similar to BindApplyResolver but also has access to the outputs
/// from when the bind was originally applied.
struct BindDestroyResolver<'a, R: Resolver> {
  inner: &'a R,
  out_dir: String,
  #[allow(dead_code)]
  applied_outputs: &'a HashMap<String, String>,
  action_results: Vec<String>,
}

impl<R: Resolver> Resolver for BindDestroyResolver<'_, R> {
  fn resolve_action(&self, index: usize) -> Result<&str, crate::placeholder::PlaceholderError> {
    self
      .action_results
      .get(index)
      .map(|s| s.as_str())
      .ok_or(crate::placeholder::PlaceholderError::UnresolvedAction(index))
  }

  fn resolve_build(&self, hash: &str, output: &str) -> Result<&str, crate::placeholder::PlaceholderError> {
    self.inner.resolve_build(hash, output)
  }

  fn resolve_bind(&self, hash: &str, output: &str) -> Result<&str, crate::placeholder::PlaceholderError> {
    self.inner.resolve_bind(hash, output)
  }

  fn resolve_out(&self) -> Result<&str, crate::placeholder::PlaceholderError> {
    Ok(&self.out_dir)
  }
}

/// Resolver for bind update actions.
///
/// Similar to BindApplyResolver but also has access to the outputs
/// from when the bind was originally applied (for use in update logic).
struct BindUpdateResolver<'a, R: Resolver> {
  inner: &'a R,
  out_dir: String,
  /// Outputs from the previous create/update - can be used to resolve
  /// placeholders that reference the old state.
  #[allow(dead_code)]
  old_outputs: &'a HashMap<String, String>,
  action_results: Vec<String>,
}

impl<R: Resolver> Resolver for BindUpdateResolver<'_, R> {
  fn resolve_action(&self, index: usize) -> Result<&str, crate::placeholder::PlaceholderError> {
    self
      .action_results
      .get(index)
      .map(|s| s.as_str())
      .ok_or(crate::placeholder::PlaceholderError::UnresolvedAction(index))
  }

  fn resolve_build(&self, hash: &str, output: &str) -> Result<&str, crate::placeholder::PlaceholderError> {
    self.inner.resolve_build(hash, output)
  }

  fn resolve_bind(&self, hash: &str, output: &str) -> Result<&str, crate::placeholder::PlaceholderError> {
    self.inner.resolve_bind(hash, output)
  }

  fn resolve_out(&self) -> Result<&str, crate::placeholder::PlaceholderError> {
    Ok(&self.out_dir)
  }
}

#[cfg(test)]
mod tests {
  use std::vec;

  use super::*;
  use crate::util::testutil::{echo_msg, shell_cmd};
  use crate::{action::actions::exec::ExecOpts, placeholder::PlaceholderError, util::hash::Hashable};

  /// Simple test resolver that returns fixed values.
  struct TestResolver {
    builds: HashMap<String, HashMap<String, String>>,
    binds: HashMap<String, HashMap<String, String>>,
  }

  impl TestResolver {
    fn new() -> Self {
      Self {
        builds: HashMap::new(),
        binds: HashMap::new(),
      }
    }

    fn with_build(mut self, hash: &str, outputs: HashMap<String, String>) -> Self {
      self.builds.insert(hash.to_string(), outputs);
      self
    }

    #[allow(dead_code)]
    fn with_bind(mut self, hash: &str, outputs: HashMap<String, String>) -> Self {
      self.binds.insert(hash.to_string(), outputs);
      self
    }
  }

  impl Resolver for TestResolver {
    fn resolve_action(&self, index: usize) -> Result<&str, PlaceholderError> {
      Err(PlaceholderError::UnresolvedAction(index))
    }

    fn resolve_build(&self, hash: &str, output: &str) -> Result<&str, PlaceholderError> {
      self
        .builds
        .iter()
        .find(|(h, _)| h.starts_with(hash))
        .and_then(|(_, outputs)| outputs.get(output))
        .map(|s| s.as_str())
        .ok_or_else(|| PlaceholderError::UnresolvedBuild {
          hash: hash.to_string(),
          output: output.to_string(),
        })
    }

    fn resolve_bind(&self, hash: &str, output: &str) -> Result<&str, PlaceholderError> {
      self
        .binds
        .iter()
        .find(|(h, _)| h.starts_with(hash))
        .and_then(|(_, outputs)| outputs.get(output))
        .map(|s| s.as_str())
        .ok_or_else(|| PlaceholderError::UnresolvedBind {
          hash: hash.to_string(),
          output: output.to_string(),
        })
    }

    fn resolve_out(&self) -> Result<&str, PlaceholderError> {
      // Test resolver doesn't have a default out - the bind execution provides its own
      Err(PlaceholderError::Malformed("no out in test resolver".to_string()))
    }
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
    }
  }

  #[tokio::test]
  async fn apply_simple_bind() {
    let bind_def = make_simple_bind();
    let hash = bind_def.compute_hash().unwrap();
    let resolver = TestResolver::new();

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
    };
    let hash = bind_def.compute_hash().unwrap();
    let resolver = TestResolver::new();

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
    };
    let hash = bind_def.compute_hash().unwrap();
    let resolver = TestResolver::new();

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
    };
    let hash = bind_def.compute_hash().unwrap();

    let mut build_outputs = HashMap::new();
    build_outputs.insert("bin".to_string(), "/store/obj/myapp/bin".to_string());
    let resolver = TestResolver::new().with_build("abc123def456", build_outputs);

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
    };
    let hash = bind_def.compute_hash().unwrap();
    let resolver = TestResolver::new();

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
    let resolver = TestResolver::new();

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
    };
    let hash = bind_def.compute_hash().unwrap();
    let resolver = TestResolver::new();

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
    };
    let hash = bind_def.compute_hash().unwrap();
    let resolver = TestResolver::new();

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
    };
    let old_hash = ObjectHash("old_hash".to_string());
    let new_hash = bind_def.compute_hash().unwrap();
    let resolver = TestResolver::new();

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
    };
    let old_hash = ObjectHash("old".to_string());
    let new_hash = bind_def.compute_hash().unwrap();
    let resolver = TestResolver::new();

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
    };
    let old_hash = ObjectHash("old".to_string());
    let new_hash = bind_def.compute_hash().unwrap();
    let resolver = TestResolver::new();

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
    };
    let old_hash = ObjectHash("old".to_string());
    let new_hash = bind_def.compute_hash().unwrap();
    let resolver = TestResolver::new();

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
}
