//! Build and bind action execution.
//!
//! This module provides the action execution system for builds and binds.
//! Actions are the primitive operations that can be performed during build
//! or bind execution, such as running shell commands (`exec`) or downloading
//! files (`fetch_url`).
//!
//! # Action Types
//!
//! - [`Action::Exec`] - Execute a shell command with optional args, env, and cwd
//! - [`Action::FetchUrl`] - Download a file from a URL with SHA256 verification
//!
//! # Placeholder Resolution
//!
//! Actions support placeholder syntax for dynamic values:
//! - `${{out}}` - The build/bind output directory
//! - `${{action:N}}` - Output from action at index N
//! - `${{build:HASH:output}}` - Output from a dependency build
//! - `${{bind:HASH:output}}` - Output from a dependency bind
//!
//! See [`crate::placeholder`] for the full placeholder system.

pub mod actions;
mod types;

pub use types::*;

use std::collections::BTreeMap;
use std::path::Path;

use crate::execute::types::{ActionResult, ExecuteError};
use crate::placeholder::{self, Resolver};
use actions::exec::ExecOpts;
use actions::exec::execute_cmd;
use actions::fetch_url::execute_fetch_url;

/// Names of built-in methods on BuildCtx that cannot be overwritten.
pub const BUILTIN_BUILD_CTX_METHODS: &[&str] = &["exec", "fetch_url", "out"];

/// Names of built-in methods on BindCtx that cannot be overwritten.
pub const BUILTIN_BIND_CTX_METHODS: &[&str] = &["exec", "out"];

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
///
/// # Returns
///
/// The result of the action execution.
pub async fn execute_action(
  action: &Action,
  resolver: &impl Resolver,
  out_dir: &Path,
) -> Result<ActionResult, ExecuteError> {
  match action {
    Action::FetchUrl { url, sha256 } => {
      // Resolve placeholders in URL (unusual but possible)
      let resolved_url = placeholder::substitute(url, resolver)?;
      let resolved_sha256 = placeholder::substitute(sha256, resolver)?;

      let path = execute_fetch_url(&resolved_url, &resolved_sha256, out_dir).await?;

      Ok(ActionResult {
        output: path.to_string_lossy().to_string(),
      })
    }

    Action::Exec(opts) => {
      let ExecOpts {
        bin: cmd,
        args,
        env,
        cwd,
      } = opts;
      // Resolve placeholders in command, env, and cwd
      let resolved_cmd = placeholder::substitute(cmd, resolver)?;

      let resolved_args = if let Some(args) = args {
        let mut resolved = Vec::new();
        for arg in args {
          resolved.push(placeholder::substitute(arg, resolver)?);
        }
        Some(resolved)
      } else {
        None
      };

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
        resolved_args.as_ref(),
        resolved_env.as_ref(),
        resolved_cwd.as_deref(),
        out_dir,
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
  use crate::util::testutil::{echo_msg, shell_echo_env};
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

    fn resolve_env(&self, name: &str) -> Result<String, PlaceholderError> {
      std::env::var(name).map_err(|_| PlaceholderError::UnresolvedEnv(name.to_string()))
    }
  }

  #[tokio::test]
  async fn execute_cmd_action() {
    let temp_dir = TempDir::new().unwrap();
    let out_dir = temp_dir.path();
    let resolver = TestResolver::new(out_dir.to_str().unwrap());

    let (cmd, args) = echo_msg("hello");
    let action = Action::Exec(ExecOpts {
      bin: cmd.to_string(),
      args: Some(args),
      env: None,
      cwd: None,
    });

    let result = execute_action(&action, &resolver, out_dir).await.unwrap();

    assert_eq!(result.output, "hello");
  }

  #[tokio::test]
  async fn execute_cmd_with_out_placeholder() {
    let temp_dir = TempDir::new().unwrap();
    let out_dir = temp_dir.path();
    let resolver = TestResolver::new(out_dir.to_str().unwrap());

    let (cmd, args) = echo_msg("$${{out}}");
    let action = Action::Exec(ExecOpts {
      bin: cmd.to_string(),
      args: Some(args),
      env: None,
      cwd: None,
    });

    let result = execute_action(&action, &resolver, out_dir).await.unwrap();

    assert_eq!(result.output, out_dir.to_string_lossy());
  }

  #[tokio::test]
  async fn execute_cmd_with_action_placeholder() {
    let temp_dir = TempDir::new().unwrap();
    let out_dir = temp_dir.path();
    let resolver = TestResolver::new(out_dir.to_str().unwrap()).with_action("/path/to/file.tar.gz");

    let (cmd, args) = echo_msg("$${{action:0}}");
    let action = Action::Exec(ExecOpts {
      bin: cmd.to_string(),
      args: Some(args),
      env: None,
      cwd: None,
    });

    let result = execute_action(&action, &resolver, out_dir).await.unwrap();

    assert_eq!(result.output, "/path/to/file.tar.gz");
  }

  #[tokio::test]
  async fn execute_cmd_with_env_placeholders() {
    let temp_dir = TempDir::new().unwrap();
    let out_dir = temp_dir.path();
    let resolver = TestResolver::new(out_dir.to_str().unwrap());

    let mut env = BTreeMap::new();
    env.insert("OUT_DIR".to_string(), "$${{out}}".to_string());

    let (cmd, args) = shell_echo_env("OUT_DIR");
    let action = Action::Exec(ExecOpts {
      bin: cmd.to_string(),
      args: Some(args),
      env: Some(env),
      cwd: None,
    });

    let result = execute_action(&action, &resolver, out_dir).await.unwrap();

    assert_eq!(result.output, out_dir.to_string_lossy());
  }
}
