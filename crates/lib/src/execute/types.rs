//! Types for build and bind execution.
//!
//! This module defines the error types, result types, and configuration
//! for executing builds and binds from a manifest.

use std::collections::HashMap;
use std::path::PathBuf;

use thiserror::Error;

use crate::bind::BindHash;
use crate::build::BuildHash;
use crate::placeholder::PlaceholderError;

/// Identifies what caused a build or bind to be skipped.
///
/// When a node in the execution DAG fails, all dependent nodes are skipped.
/// This enum tracks which dependency caused the skip, enabling better error
/// reporting and debugging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailedDependency {
  /// A build dependency failed.
  Build(BuildHash),
  /// A bind dependency failed.
  Bind(BindHash),
}

impl std::fmt::Display for FailedDependency {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      FailedDependency::Build(hash) => write!(f, "build:{}", hash.0),
      FailedDependency::Bind(hash) => write!(f, "bind:{}", hash.0),
    }
  }
}

/// Errors that can occur during build execution.
#[derive(Debug, Error)]
pub enum ExecuteError {
  /// A placeholder could not be resolved.
  #[error("placeholder error: {0}")]
  Placeholder(#[from] PlaceholderError),

  /// HTTP request failed during FetchUrl action.
  #[error("fetch failed for {url}: {message}")]
  FetchFailed { url: String, message: String },

  /// SHA256 hash mismatch after download.
  #[error("hash mismatch for {url}: expected {expected}, got {actual}")]
  HashMismatch {
    url: String,
    expected: String,
    actual: String,
  },

  /// Command execution failed.
  #[error("command failed with exit code {code:?}: {cmd}")]
  CmdFailed { cmd: String, code: Option<i32> },

  /// Command produced output on stderr.
  #[error("command error: {message}")]
  CmdError { message: String },

  /// I/O error during execution.
  #[error("io error: {0}")]
  Io(#[from] std::io::Error),

  /// Build dependency failed, so this build was skipped.
  #[error("dependency failed: {0}")]
  DependencyFailed(BuildHash),

  /// Cycle detected in the dependency graph.
  #[error("dependency cycle detected")]
  CycleDetected,

  /// Build not found in manifest.
  #[error("build not found: {0}")]
  BuildNotFound(BuildHash),

  /// Bind not found in manifest.
  #[error("bind not found: {0}")]
  BindNotFound(BindHash),

  /// Action index out of bounds.
  #[error("action index {index} out of bounds (max {max})")]
  ActionIndexOutOfBounds { index: usize, max: usize },

  /// Manifest validation failed (e.g., build depends on bind).
  #[error("invalid manifest: {0}")]
  InvalidManifest(String),
}

/// Result of executing a single action.
#[derive(Debug, Clone)]
pub struct ActionResult {
  /// The output of the action (file path for FetchUrl, stdout for Cmd).
  pub output: String,
}

/// Result of realizing a single build.
#[derive(Debug, Clone)]
pub struct BuildResult {
  /// The store path where the build outputs were written.
  pub store_path: PathBuf,

  /// Resolved outputs from the build (output name -> resolved value).
  /// These are the values from BuildDef.outputs with placeholders resolved.
  pub outputs: HashMap<String, String>,

  /// Results of individual actions (for debugging/logging).
  pub action_results: Vec<ActionResult>,
}

/// Result of applying a single bind.
#[derive(Debug, Clone)]
pub struct BindResult {
  /// Resolved outputs from the bind (output name -> resolved value).
  /// These are the values from BindDef.outputs with placeholders resolved.
  pub outputs: HashMap<String, String>,

  /// Results of individual actions (for debugging/logging).
  pub action_results: Vec<ActionResult>,
}

/// Result of executing the entire DAG.
#[derive(Debug, Default)]
pub struct DagResult {
  // === Builds ===
  /// Successfully realized builds.
  pub realized: HashMap<BuildHash, BuildResult>,

  /// Build that failed during execution (at most one, stops execution).
  pub build_failed: Option<(BuildHash, ExecuteError)>,

  /// Builds that were skipped because a dependency failed.
  /// Maps skipped build hash -> the failed dependency.
  pub build_skipped: HashMap<BuildHash, FailedDependency>,

  // === Binds ===
  /// Successfully applied binds.
  pub applied: HashMap<BindHash, BindResult>,

  /// Bind that failed during execution (at most one, triggers rollback).
  pub bind_failed: Option<(BindHash, ExecuteError)>,

  /// Binds that were skipped because a dependency failed.
  /// Maps skipped bind hash -> the failed dependency.
  pub bind_skipped: HashMap<BindHash, FailedDependency>,
}

impl DagResult {
  /// Returns true if all builds and binds succeeded.
  pub fn is_success(&self) -> bool {
    self.build_failed.is_none()
      && self.build_skipped.is_empty()
      && self.bind_failed.is_none()
      && self.bind_skipped.is_empty()
  }

  /// Returns the total number of builds processed.
  pub fn build_total(&self) -> usize {
    self.realized.len() + self.build_failed.iter().count() + self.build_skipped.len()
  }

  /// Returns the total number of binds processed.
  pub fn bind_total(&self) -> usize {
    self.applied.len() + self.bind_failed.iter().count() + self.bind_skipped.len()
  }

  /// Returns the total number of nodes (builds + binds) processed.
  pub fn total(&self) -> usize {
    self.build_total() + self.bind_total()
  }
}

/// Configuration for build execution.
#[derive(Debug, Clone)]
pub struct ExecuteConfig {
  /// Maximum number of builds to execute in parallel.
  pub parallelism: usize,

  /// Whether to use system store paths (vs user store).
  pub system: bool,

  /// Shell to use for command execution.
  /// If None, uses SHELL env var or falls back to /bin/sh (Unix) or cmd.exe (Windows).
  pub shell: Option<String>,
}

impl Default for ExecuteConfig {
  fn default() -> Self {
    Self {
      parallelism: num_cpus(),
      system: false,
      shell: None,
    }
  }
}

/// Get the number of CPUs for default parallelism.
fn num_cpus() -> usize {
  std::thread::available_parallelism().map(|p| p.get()).unwrap_or(4)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn dag_result_success_when_empty() {
    let result = DagResult::default();
    assert!(result.is_success());
    assert_eq!(result.total(), 0);
    assert_eq!(result.build_total(), 0);
    assert_eq!(result.bind_total(), 0);
  }

  #[test]
  fn dag_result_success_with_realized_build() {
    let mut result = DagResult::default();
    result.realized.insert(
      BuildHash("abc123".to_string()),
      BuildResult {
        store_path: PathBuf::from("/store/obj/test"),
        outputs: HashMap::new(),
        action_results: vec![],
      },
    );
    assert!(result.is_success());
    assert_eq!(result.build_total(), 1);
    assert_eq!(result.bind_total(), 0);
    assert_eq!(result.total(), 1);
  }

  #[test]
  fn dag_result_success_with_applied_bind() {
    let mut result = DagResult::default();
    result.applied.insert(
      BindHash("def456".to_string()),
      BindResult {
        outputs: HashMap::new(),
        action_results: vec![],
      },
    );
    assert!(result.is_success());
    assert_eq!(result.build_total(), 0);
    assert_eq!(result.bind_total(), 1);
    assert_eq!(result.total(), 1);
  }

  #[test]
  fn dag_result_failure_with_build_failed() {
    let mut result = DagResult::default();
    result.build_failed = Some((
      BuildHash("abc123".to_string()),
      ExecuteError::CmdFailed {
        cmd: "make".to_string(),
        code: Some(1),
      },
    ));
    assert!(!result.is_success());
    assert_eq!(result.build_total(), 1);
    assert_eq!(result.total(), 1);
  }

  #[test]
  fn dag_result_failure_with_bind_failed() {
    let mut result = DagResult::default();
    result.bind_failed = Some((
      BindHash("def456".to_string()),
      ExecuteError::CmdFailed {
        cmd: "ln -s".to_string(),
        code: Some(1),
      },
    ));
    assert!(!result.is_success());
    assert_eq!(result.bind_total(), 1);
    assert_eq!(result.total(), 1);
  }

  #[test]
  fn dag_result_failure_with_build_skipped() {
    let mut result = DagResult::default();
    result.build_skipped.insert(
      BuildHash("child".to_string()),
      FailedDependency::Build(BuildHash("parent".to_string())),
    );
    assert!(!result.is_success());
    assert_eq!(result.build_total(), 1);
    assert_eq!(result.total(), 1);
  }

  #[test]
  fn dag_result_failure_with_bind_skipped_due_to_build() {
    let mut result = DagResult::default();
    result.bind_skipped.insert(
      BindHash("mybind".to_string()),
      FailedDependency::Build(BuildHash("parent".to_string())),
    );
    assert!(!result.is_success());
    assert_eq!(result.bind_total(), 1);
    assert_eq!(result.total(), 1);
  }

  #[test]
  fn dag_result_failure_with_bind_skipped_due_to_bind() {
    let mut result = DagResult::default();
    result.bind_skipped.insert(
      BindHash("mybind".to_string()),
      FailedDependency::Bind(BindHash("parentbind".to_string())),
    );
    assert!(!result.is_success());
    assert_eq!(result.bind_total(), 1);
    assert_eq!(result.total(), 1);
  }

  #[test]
  fn failed_dependency_display() {
    let build_dep = FailedDependency::Build(BuildHash("abc123".to_string()));
    let bind_dep = FailedDependency::Bind(BindHash("def456".to_string()));

    assert_eq!(format!("{}", build_dep), "build:abc123");
    assert_eq!(format!("{}", bind_dep), "bind:def456");
  }

  #[test]
  fn execute_config_default_parallelism() {
    let config = ExecuteConfig::default();
    assert!(config.parallelism >= 1);
    assert!(!config.system);
    assert!(config.shell.is_none());
  }
}
