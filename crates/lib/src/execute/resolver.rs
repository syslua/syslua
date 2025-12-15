//! Placeholder resolver for build and bind execution.
//!
//! This module provides resolver implementations that can resolve placeholders
//! during execution, including action outputs, build outputs, and bind outputs.

use std::collections::HashMap;
use std::path::Path;

use crate::bind::BindHash;
use crate::build::BuildHash;
use crate::manifest::Manifest;
use crate::placeholder::{PlaceholderError, Resolver};
use crate::store;

use super::types::{BindResult, BuildResult};

/// Resolver for placeholders during build execution.
///
/// This resolver knows about:
/// - Action results from the current build (indexed by action number)
/// - Completed builds from earlier in the execution
/// - The output directory for the current build
///
/// Note: This resolver does NOT support bind resolution. Use `ExecutionResolver`
/// for unified build+bind execution.
pub struct BuildResolver<'a> {
  /// Results of actions executed so far in this build.
  action_results: Vec<String>,

  /// Results of previously completed builds.
  completed_builds: &'a HashMap<BuildHash, BuildResult>,

  /// The manifest (for looking up build definitions).
  manifest: &'a Manifest,

  /// Output directory for the current build.
  out_dir: String,

  /// Whether to use system store paths.
  system: bool,
}

impl<'a> BuildResolver<'a> {
  /// Create a new resolver for a build.
  pub fn new(
    completed_builds: &'a HashMap<BuildHash, BuildResult>,
    manifest: &'a Manifest,
    out_dir: impl AsRef<Path>,
    system: bool,
  ) -> Self {
    Self {
      action_results: Vec::new(),
      completed_builds,
      manifest,
      out_dir: out_dir.as_ref().to_string_lossy().to_string(),
      system,
    }
  }

  /// Record an action result.
  pub fn push_action_result(&mut self, result: String) {
    self.action_results.push(result);
  }

  /// Get the number of recorded action results.
  pub fn action_count(&self) -> usize {
    self.action_results.len()
  }
}

impl Resolver for BuildResolver<'_> {
  fn resolve_action(&self, index: usize) -> Result<&str, PlaceholderError> {
    self
      .action_results
      .get(index)
      .map(|s| s.as_str())
      .ok_or(PlaceholderError::UnresolvedAction(index))
  }

  fn resolve_build(&self, hash: &str, output: &str) -> Result<&str, PlaceholderError> {
    resolve_build_output(hash, output, self.completed_builds, self.manifest, self.system)
  }

  fn resolve_bind(&self, hash: &str, output: &str) -> Result<&str, PlaceholderError> {
    // BuildResolver does not support bind resolution
    Err(PlaceholderError::UnresolvedBind {
      hash: hash.to_string(),
      output: output.to_string(),
    })
  }

  fn resolve_out(&self) -> Result<&str, PlaceholderError> {
    Ok(&self.out_dir)
  }
}

/// Resolver for placeholders during unified build+bind execution.
///
/// This resolver knows about:
/// - Action results from the current node (indexed by action number)
/// - Completed builds from earlier in the execution
/// - Completed binds from earlier in the execution
/// - The output directory for the current node
///
/// Use this resolver when executing in `execute_manifest()` where both
/// builds and binds are interleaved based on the DAG.
pub struct ExecutionResolver<'a> {
  /// Results of actions executed so far in this node.
  action_results: Vec<String>,

  /// Results of previously completed builds.
  completed_builds: &'a HashMap<BuildHash, BuildResult>,

  /// Results of previously completed binds.
  completed_binds: &'a HashMap<BindHash, BindResult>,

  /// The manifest (for looking up definitions).
  manifest: &'a Manifest,

  /// Output directory for the current node.
  out_dir: String,

  /// Whether to use system store paths.
  system: bool,
}

impl<'a> ExecutionResolver<'a> {
  /// Create a new resolver for unified execution.
  pub fn new(
    completed_builds: &'a HashMap<BuildHash, BuildResult>,
    completed_binds: &'a HashMap<BindHash, BindResult>,
    manifest: &'a Manifest,
    out_dir: impl AsRef<Path>,
    system: bool,
  ) -> Self {
    Self {
      action_results: Vec::new(),
      completed_builds,
      completed_binds,
      manifest,
      out_dir: out_dir.as_ref().to_string_lossy().to_string(),
      system,
    }
  }

  /// Record an action result.
  pub fn push_action_result(&mut self, result: String) {
    self.action_results.push(result);
  }

  /// Get the number of recorded action results.
  #[allow(dead_code)]
  pub fn action_count(&self) -> usize {
    self.action_results.len()
  }
}

impl Resolver for ExecutionResolver<'_> {
  fn resolve_action(&self, index: usize) -> Result<&str, PlaceholderError> {
    self
      .action_results
      .get(index)
      .map(|s| s.as_str())
      .ok_or(PlaceholderError::UnresolvedAction(index))
  }

  fn resolve_build(&self, hash: &str, output: &str) -> Result<&str, PlaceholderError> {
    resolve_build_output(hash, output, self.completed_builds, self.manifest, self.system)
  }

  fn resolve_bind(&self, hash: &str, output: &str) -> Result<&str, PlaceholderError> {
    // Find a completed bind with matching hash prefix
    let matching_hash = self.completed_binds.keys().find(|h| h.0.starts_with(hash)).cloned();

    if let Some(full_hash) = matching_hash
      && let Some(result) = self.completed_binds.get(&full_hash)
    {
      // Look up the output in the resolved outputs
      if let Some(value) = result.outputs.get(output) {
        return Ok(value);
      }
    }

    Err(PlaceholderError::UnresolvedBind {
      hash: hash.to_string(),
      output: output.to_string(),
    })
  }

  fn resolve_out(&self) -> Result<&str, PlaceholderError> {
    Ok(&self.out_dir)
  }
}

/// Shared logic for resolving build outputs.
fn resolve_build_output<'a>(
  hash: &str,
  output: &str,
  completed_builds: &'a HashMap<BuildHash, BuildResult>,
  manifest: &Manifest,
  system: bool,
) -> Result<&'a str, PlaceholderError> {
  // First, try to find a completed build with matching hash prefix
  let matching_hash = completed_builds.keys().find(|h| h.0.starts_with(hash)).cloned();

  if let Some(full_hash) = matching_hash
    && let Some(result) = completed_builds.get(&full_hash)
  {
    // Look up the output in the resolved outputs
    if let Some(value) = result.outputs.get(output) {
      return Ok(value);
    }

    // Special case: "out" refers to the store path
    if output == "out" {
      return result
        .store_path
        .to_str()
        .ok_or_else(|| PlaceholderError::UnresolvedBuild {
          hash: hash.to_string(),
          output: output.to_string(),
        });
    }
  }

  // If not in completed builds, check if it's in the manifest
  // and compute its store path (for "out" output)
  if output == "out" {
    let full_hash = manifest.builds.keys().find(|h| h.0.starts_with(hash)).cloned();

    if let Some(full_hash) = full_hash
      && let Some(build_def) = manifest.builds.get(&full_hash)
    {
      let store_path = store::build_path(&build_def.name, build_def.version.as_deref(), &full_hash, system);
      // This is a bit awkward - we need to return a reference but we're computing a value.
      // For now, return an error indicating the build hasn't been realized yet.
      return Err(PlaceholderError::UnresolvedBuild {
        hash: format!("{} (not yet realized, path would be {:?})", hash, store_path),
        output: output.to_string(),
      });
    }
  }

  Err(PlaceholderError::UnresolvedBuild {
    hash: hash.to_string(),
    output: output.to_string(),
  })
}

#[cfg(test)]
mod tests {
  use std::path::PathBuf;

  use super::*;

  fn empty_manifest() -> Manifest {
    Manifest::default()
  }

  #[test]
  fn resolve_action_success() {
    let completed = HashMap::new();
    let manifest = empty_manifest();
    let mut resolver = BuildResolver::new(&completed, &manifest, "/out", false);

    resolver.push_action_result("/tmp/downloaded.tar.gz".to_string());
    resolver.push_action_result("/build/output".to_string());

    assert_eq!(resolver.resolve_action(0).unwrap(), "/tmp/downloaded.tar.gz");
    assert_eq!(resolver.resolve_action(1).unwrap(), "/build/output");
  }

  #[test]
  fn resolve_action_out_of_bounds() {
    let completed = HashMap::new();
    let manifest = empty_manifest();
    let resolver = BuildResolver::new(&completed, &manifest, "/out", false);

    let result = resolver.resolve_action(0);
    assert!(matches!(result, Err(PlaceholderError::UnresolvedAction(0))));
  }

  #[test]
  fn resolve_out_success() {
    let completed = HashMap::new();
    let manifest = empty_manifest();
    let resolver = BuildResolver::new(&completed, &manifest, "/store/obj/myapp-1.0-abc123", false);

    assert_eq!(resolver.resolve_out().unwrap(), "/store/obj/myapp-1.0-abc123");
  }

  #[test]
  fn resolve_build_from_completed() {
    let hash = BuildHash("abc123def456".to_string());
    let mut outputs = HashMap::new();
    outputs.insert("bin".to_string(), "/store/obj/test/bin".to_string());

    let result = BuildResult {
      store_path: PathBuf::from("/store/obj/test"),
      outputs,
      action_results: vec![],
    };

    let mut completed = HashMap::new();
    completed.insert(hash.clone(), result);

    let manifest = empty_manifest();
    let resolver = BuildResolver::new(&completed, &manifest, "/out", false);

    // Resolve by full hash
    assert_eq!(
      resolver.resolve_build("abc123def456", "bin").unwrap(),
      "/store/obj/test/bin"
    );

    // Resolve by hash prefix
    assert_eq!(resolver.resolve_build("abc123", "bin").unwrap(), "/store/obj/test/bin");

    // Resolve "out" output
    assert_eq!(resolver.resolve_build("abc123", "out").unwrap(), "/store/obj/test");
  }

  #[test]
  fn resolve_build_not_found() {
    let completed = HashMap::new();
    let manifest = empty_manifest();
    let resolver = BuildResolver::new(&completed, &manifest, "/out", false);

    let result = resolver.resolve_build("nonexistent", "out");
    assert!(matches!(result, Err(PlaceholderError::UnresolvedBuild { .. })));
  }

  #[test]
  fn resolve_bind_not_supported() {
    let completed = HashMap::new();
    let manifest = empty_manifest();
    let resolver = BuildResolver::new(&completed, &manifest, "/out", false);

    let result = resolver.resolve_bind("somebind", "path");
    assert!(matches!(result, Err(PlaceholderError::UnresolvedBind { .. })));
  }

  #[test]
  fn action_count_tracks_results() {
    let completed = HashMap::new();
    let manifest = empty_manifest();
    let mut resolver = BuildResolver::new(&completed, &manifest, "/out", false);

    assert_eq!(resolver.action_count(), 0);

    resolver.push_action_result("one".to_string());
    assert_eq!(resolver.action_count(), 1);

    resolver.push_action_result("two".to_string());
    assert_eq!(resolver.action_count(), 2);
  }

  // Tests for ExecutionResolver

  #[test]
  fn execution_resolver_resolve_build() {
    let build_hash = BuildHash("build123".to_string());
    let mut build_outputs = HashMap::new();
    build_outputs.insert("bin".to_string(), "/store/obj/app/bin".to_string());

    let build_result = BuildResult {
      store_path: PathBuf::from("/store/obj/app"),
      outputs: build_outputs,
      action_results: vec![],
    };

    let mut completed_builds = HashMap::new();
    completed_builds.insert(build_hash.clone(), build_result);

    let completed_binds = HashMap::new();
    let manifest = empty_manifest();

    let resolver = ExecutionResolver::new(&completed_builds, &completed_binds, &manifest, "/out", false);

    assert_eq!(resolver.resolve_build("build123", "bin").unwrap(), "/store/obj/app/bin");
    assert_eq!(resolver.resolve_build("build123", "out").unwrap(), "/store/obj/app");
  }

  #[test]
  fn execution_resolver_resolve_bind() {
    let bind_hash = BindHash("bind456".to_string());
    let mut bind_outputs = HashMap::new();
    bind_outputs.insert("link".to_string(), "/home/user/.config/app".to_string());

    let bind_result = BindResult {
      outputs: bind_outputs,
      action_results: vec![],
    };

    let completed_builds = HashMap::new();
    let mut completed_binds = HashMap::new();
    completed_binds.insert(bind_hash.clone(), bind_result);

    let manifest = empty_manifest();

    let resolver = ExecutionResolver::new(&completed_builds, &completed_binds, &manifest, "/out", false);

    assert_eq!(
      resolver.resolve_bind("bind456", "link").unwrap(),
      "/home/user/.config/app"
    );
  }

  #[test]
  fn execution_resolver_resolve_bind_by_prefix() {
    let bind_hash = BindHash("bind456def789".to_string());
    let mut bind_outputs = HashMap::new();
    bind_outputs.insert("path".to_string(), "/some/path".to_string());

    let bind_result = BindResult {
      outputs: bind_outputs,
      action_results: vec![],
    };

    let completed_builds = HashMap::new();
    let mut completed_binds = HashMap::new();
    completed_binds.insert(bind_hash.clone(), bind_result);

    let manifest = empty_manifest();

    let resolver = ExecutionResolver::new(&completed_builds, &completed_binds, &manifest, "/out", false);

    // Should resolve by prefix
    assert_eq!(resolver.resolve_bind("bind456", "path").unwrap(), "/some/path");
  }

  #[test]
  fn execution_resolver_resolve_bind_not_found() {
    let completed_builds = HashMap::new();
    let completed_binds = HashMap::new();
    let manifest = empty_manifest();

    let resolver = ExecutionResolver::new(&completed_builds, &completed_binds, &manifest, "/out", false);

    let result = resolver.resolve_bind("nonexistent", "output");
    assert!(matches!(result, Err(PlaceholderError::UnresolvedBind { .. })));
  }

  #[test]
  fn execution_resolver_resolve_bind_output_not_found() {
    let bind_hash = BindHash("bind456".to_string());
    let bind_result = BindResult {
      outputs: HashMap::new(), // No outputs
      action_results: vec![],
    };

    let completed_builds = HashMap::new();
    let mut completed_binds = HashMap::new();
    completed_binds.insert(bind_hash.clone(), bind_result);

    let manifest = empty_manifest();

    let resolver = ExecutionResolver::new(&completed_builds, &completed_binds, &manifest, "/out", false);

    // Bind exists but output doesn't
    let result = resolver.resolve_bind("bind456", "nonexistent_output");
    assert!(matches!(result, Err(PlaceholderError::UnresolvedBind { .. })));
  }

  #[test]
  fn execution_resolver_action_tracking() {
    let completed_builds = HashMap::new();
    let completed_binds = HashMap::new();
    let manifest = empty_manifest();

    let mut resolver = ExecutionResolver::new(&completed_builds, &completed_binds, &manifest, "/out", false);

    assert_eq!(resolver.action_count(), 0);

    resolver.push_action_result("result1".to_string());
    assert_eq!(resolver.action_count(), 1);
    assert_eq!(resolver.resolve_action(0).unwrap(), "result1");

    resolver.push_action_result("result2".to_string());
    assert_eq!(resolver.action_count(), 2);
    assert_eq!(resolver.resolve_action(1).unwrap(), "result2");
  }

  #[test]
  fn execution_resolver_out_dir() {
    let completed_builds = HashMap::new();
    let completed_binds = HashMap::new();
    let manifest = empty_manifest();

    let resolver = ExecutionResolver::new(&completed_builds, &completed_binds, &manifest, "/my/output/dir", false);

    assert_eq!(resolver.resolve_out().unwrap(), "/my/output/dir");
  }
}
