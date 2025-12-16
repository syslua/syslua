//! Action execution module.
//!
//! This module provides the dispatch logic for executing build actions.

pub mod cmd;
pub mod fetch;

use std::collections::BTreeMap;
use std::path::Path;

use crate::build::BuildAction;
use crate::execute::types::{ActionResult, ExecuteError};
use crate::placeholder::{self, Resolver};

pub use cmd::execute_cmd;
pub use fetch::execute_fetch;

/// Execute a single build action.
///
/// This dispatches to the appropriate action handler based on the action type.
/// Placeholders in the action are resolved before execution.
///
/// # Arguments
///
/// * `action` - The action to execute
/// * `resolver` - The placeholder resolver for this build
/// * `out_dir` - The build's output directory
/// * `shell` - Optional shell override for Cmd actions
///
/// # Returns
///
/// The result of the action execution.
pub async fn execute_action(
  action: &BuildAction,
  resolver: &impl Resolver,
  out_dir: &Path,
  shell: Option<&str>,
) -> Result<ActionResult, ExecuteError> {
  match action {
    BuildAction::FetchUrl { url, sha256 } => {
      // Resolve placeholders in URL (unusual but possible)
      let resolved_url = placeholder::substitute(url, resolver)?;
      let resolved_sha256 = placeholder::substitute(sha256, resolver)?;

      let path = execute_fetch(&resolved_url, &resolved_sha256, out_dir).await?;

      Ok(ActionResult {
        output: path.to_string_lossy().to_string(),
      })
    }

    BuildAction::Cmd { cmd, env, cwd } => {
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

#[cfg(test)]
mod tests {
  use super::*;
  use crate::placeholder::PlaceholderError;
  use tempfile::TempDir;

  /// Simple test resolver that returns fixed values.
  struct TestResolver {
    actions: Vec<String>,
    out_dir: String,
  }

  impl TestResolver {
    fn new(out_dir: &str) -> Self {
      Self {
        actions: Vec::new(),
        out_dir: out_dir.to_string(),
      }
    }

    fn with_action(mut self, output: &str) -> Self {
      self.actions.push(output.to_string());
      self
    }
  }

  impl Resolver for TestResolver {
    fn resolve_action(&self, index: usize) -> Result<&str, PlaceholderError> {
      self
        .actions
        .get(index)
        .map(|s| s.as_str())
        .ok_or(PlaceholderError::UnresolvedAction(index))
    }

    fn resolve_build(&self, hash: &str, output: &str) -> Result<&str, PlaceholderError> {
      Err(PlaceholderError::UnresolvedBuild {
        hash: hash.to_string(),
        output: output.to_string(),
      })
    }

    fn resolve_bind(&self, hash: &str, output: &str) -> Result<&str, PlaceholderError> {
      Err(PlaceholderError::UnresolvedBind {
        hash: hash.to_string(),
        output: output.to_string(),
      })
    }

    fn resolve_out(&self) -> Result<&str, PlaceholderError> {
      Ok(&self.out_dir)
    }
  }

  /// Get an echo command that prints an environment variable.
  /// Unix: echo $VAR
  /// Windows: echo %VAR%
  #[cfg(unix)]
  fn echo_env(var: &str) -> String {
    format!("echo ${}", var)
  }

  #[cfg(windows)]
  fn echo_env(var: &str) -> String {
    format!("echo %{}%", var)
  }

  #[tokio::test]
  async fn execute_cmd_action() {
    let temp_dir = TempDir::new().unwrap();
    let out_dir = temp_dir.path();
    let resolver = TestResolver::new(out_dir.to_str().unwrap());

    let action = BuildAction::Cmd {
      cmd: "echo hello".to_string(),
      env: None,
      cwd: None,
    };

    let result = execute_action(&action, &resolver, out_dir, None).await.unwrap();

    assert_eq!(result.output, "hello");
  }

  #[tokio::test]
  async fn execute_cmd_with_out_placeholder() {
    let temp_dir = TempDir::new().unwrap();
    let out_dir = temp_dir.path();
    let resolver = TestResolver::new(out_dir.to_str().unwrap());

    let action = BuildAction::Cmd {
      cmd: "echo $${out}".to_string(),
      env: None,
      cwd: None,
    };

    let result = execute_action(&action, &resolver, out_dir, None).await.unwrap();

    assert_eq!(result.output, out_dir.to_string_lossy());
  }

  #[tokio::test]
  async fn execute_cmd_with_action_placeholder() {
    let temp_dir = TempDir::new().unwrap();
    let out_dir = temp_dir.path();
    let resolver = TestResolver::new(out_dir.to_str().unwrap()).with_action("/path/to/file.tar.gz");

    let action = BuildAction::Cmd {
      cmd: "echo $${action:0}".to_string(),
      env: None,
      cwd: None,
    };

    let result = execute_action(&action, &resolver, out_dir, None).await.unwrap();

    assert_eq!(result.output, "/path/to/file.tar.gz");
  }

  #[tokio::test]
  async fn execute_cmd_with_env_placeholders() {
    let temp_dir = TempDir::new().unwrap();
    let out_dir = temp_dir.path();
    let resolver = TestResolver::new(out_dir.to_str().unwrap());

    let mut env = BTreeMap::new();
    env.insert("OUT_DIR".to_string(), "$${out}".to_string());

    let action = BuildAction::Cmd {
      cmd: echo_env("OUT_DIR"),
      env: Some(env),
      cwd: None,
    };

    let result = execute_action(&action, &resolver, out_dir, None).await.unwrap();

    assert_eq!(result.output, out_dir.to_string_lossy());
  }
}
