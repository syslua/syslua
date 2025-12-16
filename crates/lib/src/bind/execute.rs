//! Single bind application and destruction.
//!
//! This module handles executing actions for a single bind and
//! producing the final BindResult.

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use tempfile::TempDir;
use tracing::{debug, info};

use crate::bind::{BindAction, BindDef};
use crate::execute::actions::execute_cmd;
use crate::execute::types::{ActionResult, BindResult, ExecuteConfig, ExecuteError};
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
  config: &ExecuteConfig,
) -> Result<BindResult, ExecuteError> {
  info!(hash = %hash.0, "applying bind");

  // Create a temporary working directory for the bind's $${out}
  let temp_dir = TempDir::new()?;
  let out_dir = temp_dir.path();

  // Create a resolver that wraps the provided one but overrides $${out}
  let bind_resolver = BindApplyResolver {
    inner: resolver,
    out_dir: out_dir.to_string_lossy().to_string(),
    action_results: Vec::new(),
  };

  // Execute actions in order
  let (action_results, outputs) =
    execute_bind_actions(&bind_def.apply_actions, bind_resolver, bind_def, out_dir, config).await?;

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
  config: &ExecuteConfig,
) -> Result<(), ExecuteError> {
  let Some(destroy_actions) = &bind_def.destroy_actions else {
    debug!(hash = %hash.0, "bind has no destroy_actions, skipping");
    return Ok(());
  };

  info!(hash = %hash.0, "destroying bind");

  // Create a temporary directory for destroy actions
  let temp_dir = TempDir::new()?;
  let out_dir = temp_dir.path();

  // For destroy, we use the outputs from when the bind was applied
  // so that destroy commands can reference them
  let bind_resolver = BindDestroyResolver {
    inner: resolver,
    out_dir: out_dir.to_string_lossy().to_string(),
    applied_outputs: &bind_result.outputs,
    action_results: Vec::new(),
  };

  // Execute destroy actions
  let _ = execute_bind_actions_raw(destroy_actions, bind_resolver, out_dir, config).await?;

  info!(hash = %hash.0, "bind destroyed");

  Ok(())
}

/// Execute bind actions and resolve outputs.
async fn execute_bind_actions<R: Resolver>(
  actions: &[BindAction],
  mut resolver: BindApplyResolver<'_, R>,
  bind_def: &BindDef,
  out_dir: &Path,
  config: &ExecuteConfig,
) -> Result<(Vec<ActionResult>, HashMap<String, String>), ExecuteError> {
  let mut action_results = Vec::new();

  for (idx, action) in actions.iter().enumerate() {
    debug!(action_idx = idx, "executing bind action");

    let result = execute_bind_action(action, &resolver, out_dir, config.shell.as_deref()).await?;

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
  actions: &[BindAction],
  mut resolver: BindDestroyResolver<'_, R>,
  out_dir: &Path,
  config: &ExecuteConfig,
) -> Result<Vec<ActionResult>, ExecuteError> {
  let mut action_results = Vec::new();

  for (idx, action) in actions.iter().enumerate() {
    debug!(action_idx = idx, "executing destroy action");

    let result = execute_bind_action(action, &resolver, out_dir, config.shell.as_deref()).await?;

    resolver.action_results.push(result.output.clone());
    action_results.push(result);
  }

  Ok(action_results)
}

/// Execute a single bind action.
async fn execute_bind_action(
  action: &BindAction,
  resolver: &impl Resolver,
  out_dir: &Path,
  shell: Option<&str>,
) -> Result<ActionResult, ExecuteError> {
  match action {
    BindAction::Cmd { cmd, env, cwd } => {
      // Resolve placeholders in command, env, and cwd
      let resolved_cmd = placeholder::substitute(cmd, resolver)?;

      let resolved_env = if let Some(env) = env {
        let mut resolved = BTreeMap::new();
        for (key, value) in env {
          resolved.insert(key.clone(), placeholder::substitute(value, resolver)?);
        }
        Some(resolved)
      } else {
        None
      };

      let resolved_cwd = if let Some(cwd) = cwd {
        Some(placeholder::substitute(cwd, resolver)?)
      } else {
        None
      };

      let output = execute_cmd(
        &resolved_cmd,
        resolved_env.as_ref(),
        resolved_cwd.as_deref(),
        out_dir,
        shell,
      )
      .await?;

      Ok(ActionResult { output })
    }
  }
}

/// Resolve the outputs from a bind definition.
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

#[cfg(test)]
mod tests {
  use super::*;
  use crate::{placeholder::PlaceholderError, util::hash::Hashable};
  use serial_test::serial;

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
    BindDef {
      inputs: None,
      apply_actions: vec![BindAction::Cmd {
        cmd: "echo applied".to_string(),
        env: None,
        cwd: None,
      }],
      outputs: None,
      destroy_actions: None,
    }
  }

  fn test_config() -> ExecuteConfig {
    ExecuteConfig {
      parallelism: 1,
      system: false,
      shell: None,
    }
  }

  #[tokio::test]
  #[serial]
  async fn apply_simple_bind() {
    let bind_def = make_simple_bind();
    let hash = bind_def.compute_hash().unwrap();
    let resolver = TestResolver::new();
    let config = test_config();

    let result = apply_bind(&hash, &bind_def, &resolver, &config).await.unwrap();

    assert_eq!(result.action_results.len(), 1);
    assert_eq!(result.action_results[0].output, "applied");
  }

  #[tokio::test]
  #[serial]
  async fn apply_bind_with_outputs() {
    let bind_def = BindDef {
      inputs: None,
      apply_actions: vec![BindAction::Cmd {
        cmd: "echo /path/to/link".to_string(),
        env: None,
        cwd: None,
      }],
      outputs: Some([("link".to_string(), "$${action:0}".to_string())].into_iter().collect()),
      destroy_actions: None,
    };
    let hash = bind_def.compute_hash().unwrap();
    let resolver = TestResolver::new();
    let config = test_config();

    let result = apply_bind(&hash, &bind_def, &resolver, &config).await.unwrap();

    assert_eq!(result.outputs["link"], "/path/to/link");
  }

  #[tokio::test]
  #[serial]
  async fn apply_bind_with_out_placeholder() {
    let bind_def = BindDef {
      inputs: None,
      apply_actions: vec![BindAction::Cmd {
        cmd: "echo $${out}".to_string(),
        env: None,
        cwd: None,
      }],
      outputs: Some([("dir".to_string(), "$${out}".to_string())].into_iter().collect()),
      destroy_actions: None,
    };
    let hash = bind_def.compute_hash().unwrap();
    let resolver = TestResolver::new();
    let config = test_config();

    let result = apply_bind(&hash, &bind_def, &resolver, &config).await.unwrap();

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
  #[serial]
  async fn apply_bind_with_build_dependency() {
    let bind_def = BindDef {
      inputs: None,
      apply_actions: vec![BindAction::Cmd {
        cmd: "echo $${build:abc123:bin}".to_string(),
        env: None,
        cwd: None,
      }],
      outputs: None,
      destroy_actions: None,
    };
    let hash = bind_def.compute_hash().unwrap();

    let mut build_outputs = HashMap::new();
    build_outputs.insert("bin".to_string(), "/store/obj/myapp/bin".to_string());
    let resolver = TestResolver::new().with_build("abc123def456", build_outputs);
    let config = test_config();

    let result = apply_bind(&hash, &bind_def, &resolver, &config).await.unwrap();

    assert_eq!(result.action_results[0].output, "/store/obj/myapp/bin");
  }

  #[tokio::test]
  #[serial]
  async fn destroy_bind_with_actions() {
    let bind_def = BindDef {
      inputs: None,
      apply_actions: vec![BindAction::Cmd {
        cmd: "echo applied".to_string(),
        env: None,
        cwd: None,
      }],
      outputs: Some(
        [("path".to_string(), "/created/path".to_string())]
          .into_iter()
          .collect(),
      ),
      destroy_actions: Some(vec![BindAction::Cmd {
        cmd: "echo destroyed".to_string(),
        env: None,
        cwd: None,
      }]),
    };
    let hash = bind_def.compute_hash().unwrap();
    let resolver = TestResolver::new();
    let config = test_config();

    // First apply
    let bind_result = apply_bind(&hash, &bind_def, &resolver, &config).await.unwrap();

    // Then destroy
    let destroy_result = destroy_bind(&hash, &bind_def, &bind_result, &resolver, &config).await;

    assert!(destroy_result.is_ok());
  }

  #[tokio::test]
  #[serial]
  async fn destroy_bind_without_actions() {
    let bind_def = make_simple_bind();
    let hash = bind_def.compute_hash().unwrap();
    let resolver = TestResolver::new();
    let config = test_config();

    let bind_result = BindResult {
      outputs: HashMap::new(),
      action_results: vec![],
    };

    // Destroy should succeed even with no destroy_actions
    let result = destroy_bind(&hash, &bind_def, &bind_result, &resolver, &config).await;
    assert!(result.is_ok());
  }

  #[tokio::test]
  #[serial]
  async fn apply_bind_action_failure() {
    let bind_def = BindDef {
      inputs: None,
      apply_actions: vec![BindAction::Cmd {
        cmd: "exit 1".to_string(),
        env: None,
        cwd: None,
      }],
      outputs: None,
      destroy_actions: None,
    };
    let hash = bind_def.compute_hash().unwrap();
    let resolver = TestResolver::new();
    let config = test_config();

    let result = apply_bind(&hash, &bind_def, &resolver, &config).await;

    assert!(matches!(result, Err(ExecuteError::CmdFailed { .. })));
  }

  #[tokio::test]
  #[serial]
  async fn apply_bind_multiple_actions() {
    let bind_def = BindDef {
      inputs: None,
      apply_actions: vec![
        BindAction::Cmd {
          cmd: "echo step1".to_string(),
          env: None,
          cwd: None,
        },
        BindAction::Cmd {
          cmd: "echo step2".to_string(),
          env: None,
          cwd: None,
        },
        BindAction::Cmd {
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
      destroy_actions: None,
    };
    let hash = bind_def.compute_hash().unwrap();
    let resolver = TestResolver::new();
    let config = test_config();

    let result = apply_bind(&hash, &bind_def, &resolver, &config).await.unwrap();

    assert_eq!(result.action_results.len(), 3);
    assert_eq!(result.action_results[0].output, "step1");
    assert_eq!(result.action_results[1].output, "step2");
    assert_eq!(result.action_results[2].output, "step1 step2");
    assert_eq!(result.outputs["combined"], "step1 step2");
  }
}
