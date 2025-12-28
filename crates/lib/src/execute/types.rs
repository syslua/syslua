//! Types for build and bind execution.
//!
//! This module defines the error types, result types, and configuration
//! for executing builds and binds from a manifest.

use std::collections::HashMap;
use std::path::PathBuf;

use thiserror::Error;

use crate::placeholder::PlaceholderError;
use crate::util::hash::{DirHashError, ObjectHash};

/// Identifies what caused a build or bind to be skipped.
///
/// When a node in the execution DAG fails, all dependent nodes are skipped.
/// This enum tracks which dependency caused the skip, enabling better error
/// reporting and debugging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FailedDependency {
  /// A build dependency failed.
  Build(ObjectHash),
  /// A bind dependency failed.
  Bind(ObjectHash),
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
  DependencyFailed(ObjectHash),

  /// Cycle detected in the dependency graph.
  #[error("dependency cycle detected")]
  CycleDetected,

  /// Build not found in manifest.
  #[error("build not found: {0}")]
  BuildNotFound(ObjectHash),

  /// Bind not found in manifest.
  #[error("bind not found: {0}")]
  BindNotFound(ObjectHash),

  /// Action index out of bounds.
  #[error("action index {index} out of bounds (max {max})")]
  ActionIndexOutOfBounds { index: usize, max: usize },

  /// Manifest validation failed (e.g., build depends on bind).
  #[error("invalid manifest: {0}")]
  InvalidManifest(String),

  /// Failed to hash build output directory.
  #[error("failed to hash build output: {0}")]
  HashOutput(#[from] DirHashError),

  /// Failed to write build marker file.
  #[error("failed to write build marker: {0}")]
  WriteMarker(#[source] std::io::Error),

  /// Failed to read build marker file.
  #[error("failed to read build marker: {0}")]
  ReadMarker(#[source] std::io::Error),

  /// Failed to parse build marker JSON.
  #[error("failed to parse build marker: {0}")]
  ParseMarker(#[source] serde_json::Error),
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

/// Result of checking a bind for drift.
#[derive(Debug, Clone)]
pub struct DriftResult {
  /// The bind's hash.
  pub hash: ObjectHash,
  /// The bind's ID (if any).
  pub id: Option<String>,
  /// The check result.
  pub result: crate::bind::BindCheckResult,
}

/// Result of executing the entire DAG.
#[derive(Debug, Default)]
pub struct DagResult {
  // === Builds ===
  /// Successfully realized builds.
  pub realized: HashMap<ObjectHash, BuildResult>,

  /// Build that failed during execution (at most one, stops execution).
  pub build_failed: Option<(ObjectHash, ExecuteError)>,

  /// Builds that were skipped because a dependency failed.
  /// Maps skipped build hash -> the failed dependency.
  pub build_skipped: HashMap<ObjectHash, FailedDependency>,

  // === Binds ===
  /// Successfully applied binds.
  pub applied: HashMap<ObjectHash, BindResult>,

  /// Bind that failed during execution (at most one, triggers rollback).
  pub bind_failed: Option<(ObjectHash, ExecuteError)>,

  /// Binds that were skipped because a dependency failed.
  /// Maps skipped bind hash -> the failed dependency.
  pub bind_skipped: HashMap<ObjectHash, FailedDependency>,
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
}

impl Default for ExecuteConfig {
  fn default() -> Self {
    Self {
      parallelism: num_cpus(),
      system: false,
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
  fn dag_result_success_with_realized_build() {
    let mut result = DagResult::default();
    result.realized.insert(
      ObjectHash("abc123".to_string()),
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
      ObjectHash("def456".to_string()),
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
    let result = DagResult {
      build_failed: Some((
        ObjectHash("abc123".to_string()),
        ExecuteError::CmdFailed {
          cmd: "make".to_string(),
          code: Some(1),
        },
      )),
      ..Default::default()
    };
    assert!(!result.is_success());
    assert_eq!(result.build_total(), 1);
    assert_eq!(result.total(), 1);
  }

  #[test]
  fn dag_result_failure_with_bind_failed() {
    let result = DagResult {
      bind_failed: Some((
        ObjectHash("def456".to_string()),
        ExecuteError::CmdFailed {
          cmd: "ln -s".to_string(),
          code: Some(1),
        },
      )),
      ..Default::default()
    };
    assert!(!result.is_success());
    assert_eq!(result.bind_total(), 1);
    assert_eq!(result.total(), 1);
  }

  #[test]
  fn dag_result_failure_with_build_skipped() {
    let mut result = DagResult::default();
    result.build_skipped.insert(
      ObjectHash("child".to_string()),
      FailedDependency::Build(ObjectHash("parent".to_string())),
    );
    assert!(!result.is_success());
    assert_eq!(result.build_total(), 1);
    assert_eq!(result.total(), 1);
  }

  #[test]
  fn dag_result_failure_with_bind_skipped_due_to_build() {
    let mut result = DagResult::default();
    result.bind_skipped.insert(
      ObjectHash("mybind".to_string()),
      FailedDependency::Build(ObjectHash("parent".to_string())),
    );
    assert!(!result.is_success());
    assert_eq!(result.bind_total(), 1);
    assert_eq!(result.total(), 1);
  }

  #[test]
  fn dag_result_failure_with_bind_skipped_due_to_bind() {
    let mut result = DagResult::default();
    result.bind_skipped.insert(
      ObjectHash("mybind".to_string()),
      FailedDependency::Bind(ObjectHash("parentbind".to_string())),
    );
    assert!(!result.is_success());
    assert_eq!(result.bind_total(), 1);
    assert_eq!(result.total(), 1);
  }

  #[test]
  fn failed_dependency_display() {
    let build_dep = FailedDependency::Build(ObjectHash("abc123".to_string()));
    let bind_dep = FailedDependency::Bind(ObjectHash("def456".to_string()));

    assert_eq!(format!("{}", build_dep), "build:abc123");
    assert_eq!(format!("{}", bind_dep), "bind:def456");
  }

  #[test]
  fn execute_config_default_parallelism() {
    let config = ExecuteConfig::default();
    assert!(config.parallelism >= 1);
    assert!(!config.system);
  }
}
