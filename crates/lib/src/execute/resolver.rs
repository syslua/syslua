//! Placeholder resolver for build and bind execution.
//!
//! This module provides two resolver implementations:
//! - `BuildCtxResolver` for build execution (builds can only reference other builds)
//! - `BindCtxResolver` for bind execution (binds can reference builds and other binds)

use std::collections::HashMap;

use crate::build::store::build_dir_path;
use crate::manifest::Manifest;
use crate::placeholder::{PlaceholderError, Resolver};
use crate::util::hash::ObjectHash;

use super::types::{BindResult, BuildResult};

/// Resolver for placeholders during build execution.
///
/// Builds can only reference other builds, not binds. This resolver supports:
/// - `${action:N}` - stdout of action at index N
/// - `${build:HASH:OUTPUT}` - output from a completed build
/// - `${out}` - the current build's output directory
/// - `${env:NAME}` - environment variable
///
/// Note: `${bind:...}` placeholders will always error since builds cannot
/// depend on binds.
pub struct BuildCtxResolver<'a> {
  action_results: Vec<String>,
  completed_builds: &'a HashMap<ObjectHash, BuildResult>,
  manifest: &'a Manifest,
  out_dir: String,
}

impl<'a> BuildCtxResolver<'a> {
  pub fn new(completed_builds: &'a HashMap<ObjectHash, BuildResult>, manifest: &'a Manifest, out_dir: String) -> Self {
    Self {
      action_results: Vec::new(),
      completed_builds,
      manifest,
      out_dir,
    }
  }

  pub fn push_action_result(&mut self, result: String) {
    self.action_results.push(result);
  }

  pub fn action_count(&self) -> usize {
    self.action_results.len()
  }
}

impl Resolver for BuildCtxResolver<'_> {
  fn resolve_action(&self, index: usize) -> Result<&str, PlaceholderError> {
    self
      .action_results
      .get(index)
      .map(|s| s.as_str())
      .ok_or(PlaceholderError::UnresolvedAction(index))
  }

  fn resolve_build(&self, hash: &str, output: &str) -> Result<&str, PlaceholderError> {
    resolve_build_output(hash, output, self.completed_builds, self.manifest)
  }

  fn resolve_bind(&self, hash: &str, output: &str) -> Result<&str, PlaceholderError> {
    // Builds cannot reference binds
    Err(PlaceholderError::UnresolvedBind {
      hash: hash.to_string(),
      output: output.to_string(),
    })
  }

  fn resolve_out(&self) -> Result<&str, PlaceholderError> {
    Ok(&self.out_dir)
  }

  fn resolve_env(&self, name: &str) -> Result<String, PlaceholderError> {
    resolve_env_var(name)
  }
}

/// Resolver for placeholders during bind execution.
///
/// Binds can reference both builds and other binds. This resolver supports:
/// - `${action:N}` - stdout of action at index N
/// - `${build:HASH:OUTPUT}` - output from a completed build
/// - `${bind:HASH:OUTPUT}` - output from a completed bind
/// - `${out}` - the current bind's output directory
/// - `${env:NAME}` - environment variable
///
/// Use `with_out_dir()` to create child resolvers for bind actions that need
/// a different output directory (e.g., a temporary working directory).
pub struct BindCtxResolver<'a> {
  action_results: Vec<String>,
  completed_builds: &'a HashMap<ObjectHash, BuildResult>,
  completed_binds: &'a HashMap<ObjectHash, BindResult>,
  manifest: &'a Manifest,
  out_dir: String,
}

impl<'a> BindCtxResolver<'a> {
  pub fn new(
    completed_builds: &'a HashMap<ObjectHash, BuildResult>,
    completed_binds: &'a HashMap<ObjectHash, BindResult>,
    manifest: &'a Manifest,
    out_dir: String,
  ) -> Self {
    Self {
      action_results: Vec::new(),
      completed_builds,
      completed_binds,
      manifest,
      out_dir,
    }
  }

  pub fn push_action_result(&mut self, result: String) {
    self.action_results.push(result);
  }

  #[allow(dead_code)]
  pub fn action_count(&self) -> usize {
    self.action_results.len()
  }

  /// Create a child resolver with a new output directory.
  ///
  /// This is used for bind actions (apply, destroy, update, check) that need
  /// their own temporary working directory. The child resolver shares access
  /// to completed builds/binds but has fresh action results.
  pub fn with_out_dir(&self, out_dir: String) -> BindCtxResolver<'a> {
    BindCtxResolver {
      action_results: Vec::new(),
      completed_builds: self.completed_builds,
      completed_binds: self.completed_binds,
      manifest: self.manifest,
      out_dir,
    }
  }
}

impl Resolver for BindCtxResolver<'_> {
  fn resolve_action(&self, index: usize) -> Result<&str, PlaceholderError> {
    self
      .action_results
      .get(index)
      .map(|s| s.as_str())
      .ok_or(PlaceholderError::UnresolvedAction(index))
  }

  fn resolve_build(&self, hash: &str, output: &str) -> Result<&str, PlaceholderError> {
    resolve_build_output(hash, output, self.completed_builds, self.manifest)
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

  fn resolve_env(&self, name: &str) -> Result<String, PlaceholderError> {
    resolve_env_var(name)
  }
}

/// Shared logic for resolving environment variables.
fn resolve_env_var(name: &str) -> Result<String, PlaceholderError> {
  std::env::var(name).map_err(|_| PlaceholderError::UnresolvedEnv(name.to_string()))
}

/// Shared logic for resolving build outputs.
fn resolve_build_output<'a>(
  hash: &str,
  output: &str,
  completed_builds: &'a HashMap<ObjectHash, BuildResult>,
  manifest: &Manifest,
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

    if let Some(full_hash) = full_hash {
      let store_path = build_dir_path(&full_hash);
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

  // Tests for BuildCtxResolver

  #[test]
  fn build_ctx_resolve_action_success() {
    let completed = HashMap::new();
    let manifest = empty_manifest();
    let mut resolver = BuildCtxResolver::new(&completed, &manifest, "/out".to_string());

    resolver.push_action_result("/tmp/downloaded.tar.gz".to_string());
    resolver.push_action_result("/build/output".to_string());

    assert_eq!(resolver.resolve_action(0).unwrap(), "/tmp/downloaded.tar.gz");
    assert_eq!(resolver.resolve_action(1).unwrap(), "/build/output");
  }

  #[test]
  fn build_ctx_resolve_action_out_of_bounds() {
    let completed = HashMap::new();
    let manifest = empty_manifest();
    let resolver = BuildCtxResolver::new(&completed, &manifest, "/out".to_string());

    let result = resolver.resolve_action(0);
    assert!(matches!(result, Err(PlaceholderError::UnresolvedAction(0))));
  }

  #[test]
  fn build_ctx_resolve_out_success() {
    let completed = HashMap::new();
    let manifest = empty_manifest();
    let resolver = BuildCtxResolver::new(&completed, &manifest, "/store/build/myapp-1.0-abc123".to_string());

    assert_eq!(resolver.resolve_out().unwrap(), "/store/build/myapp-1.0-abc123");
  }

  #[test]
  fn build_ctx_resolve_build_from_completed() {
    let hash = ObjectHash("abc123def456".to_string());
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
    let resolver = BuildCtxResolver::new(&completed, &manifest, "/out".to_string());

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
  fn build_ctx_resolve_build_not_found() {
    let completed = HashMap::new();
    let manifest = empty_manifest();
    let resolver = BuildCtxResolver::new(&completed, &manifest, "/out".to_string());

    let result = resolver.resolve_build("nonexistent", "out");
    assert!(matches!(result, Err(PlaceholderError::UnresolvedBuild { .. })));
  }

  #[test]
  fn build_ctx_resolve_bind_not_supported() {
    let completed = HashMap::new();
    let manifest = empty_manifest();
    let resolver = BuildCtxResolver::new(&completed, &manifest, "/out".to_string());

    let result = resolver.resolve_bind("somebind", "path");
    assert!(matches!(result, Err(PlaceholderError::UnresolvedBind { .. })));
  }

  #[test]
  fn build_ctx_action_count_tracks_results() {
    let completed = HashMap::new();
    let manifest = empty_manifest();
    let mut resolver = BuildCtxResolver::new(&completed, &manifest, "/out".to_string());

    assert_eq!(resolver.action_count(), 0);

    resolver.push_action_result("one".to_string());
    assert_eq!(resolver.action_count(), 1);

    resolver.push_action_result("two".to_string());
    assert_eq!(resolver.action_count(), 2);
  }

  // Tests for BindCtxResolver

  #[test]
  fn bind_ctx_resolve_build() {
    let build_hash = ObjectHash("build123".to_string());
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

    let resolver = BindCtxResolver::new(&completed_builds, &completed_binds, &manifest, "/out".to_string());

    assert_eq!(resolver.resolve_build("build123", "bin").unwrap(), "/store/obj/app/bin");
    assert_eq!(resolver.resolve_build("build123", "out").unwrap(), "/store/obj/app");
  }

  #[test]
  fn bind_ctx_resolve_bind() {
    let bind_hash = ObjectHash("bind456".to_string());
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

    let resolver = BindCtxResolver::new(&completed_builds, &completed_binds, &manifest, "/out".to_string());

    assert_eq!(
      resolver.resolve_bind("bind456", "link").unwrap(),
      "/home/user/.config/app"
    );
  }

  #[test]
  fn bind_ctx_resolve_bind_by_prefix() {
    let bind_hash = ObjectHash("bind456def789".to_string());
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

    let resolver = BindCtxResolver::new(&completed_builds, &completed_binds, &manifest, "/out".to_string());

    // Should resolve by prefix
    assert_eq!(resolver.resolve_bind("bind456", "path").unwrap(), "/some/path");
  }

  #[test]
  fn bind_ctx_resolve_bind_not_found() {
    let completed_builds = HashMap::new();
    let completed_binds = HashMap::new();
    let manifest = empty_manifest();

    let resolver = BindCtxResolver::new(&completed_builds, &completed_binds, &manifest, "/out".to_string());

    let result = resolver.resolve_bind("nonexistent", "output");
    assert!(matches!(result, Err(PlaceholderError::UnresolvedBind { .. })));
  }

  #[test]
  fn bind_ctx_resolve_bind_output_not_found() {
    let bind_hash = ObjectHash("bind456".to_string());
    let bind_result = BindResult {
      outputs: HashMap::new(), // No outputs
      action_results: vec![],
    };

    let completed_builds = HashMap::new();
    let mut completed_binds = HashMap::new();
    completed_binds.insert(bind_hash.clone(), bind_result);

    let manifest = empty_manifest();

    let resolver = BindCtxResolver::new(&completed_builds, &completed_binds, &manifest, "/out".to_string());

    // Bind exists but output doesn't
    let result = resolver.resolve_bind("bind456", "nonexistent_output");
    assert!(matches!(result, Err(PlaceholderError::UnresolvedBind { .. })));
  }

  #[test]
  fn bind_ctx_action_tracking() {
    let completed_builds = HashMap::new();
    let completed_binds = HashMap::new();
    let manifest = empty_manifest();

    let mut resolver = BindCtxResolver::new(&completed_builds, &completed_binds, &manifest, "/out".to_string());

    assert_eq!(resolver.action_count(), 0);

    resolver.push_action_result("result1".to_string());
    assert_eq!(resolver.action_count(), 1);
    assert_eq!(resolver.resolve_action(0).unwrap(), "result1");

    resolver.push_action_result("result2".to_string());
    assert_eq!(resolver.action_count(), 2);
    assert_eq!(resolver.resolve_action(1).unwrap(), "result2");
  }

  #[test]
  fn bind_ctx_out_dir() {
    let completed_builds = HashMap::new();
    let completed_binds = HashMap::new();
    let manifest = empty_manifest();

    let resolver = BindCtxResolver::new(
      &completed_builds,
      &completed_binds,
      &manifest,
      "/my/output/dir".to_string(),
    );

    assert_eq!(resolver.resolve_out().unwrap(), "/my/output/dir");
  }

  #[test]
  fn bind_ctx_with_out_dir() {
    let completed_builds = HashMap::new();
    let completed_binds = HashMap::new();
    let manifest = empty_manifest();

    let parent = BindCtxResolver::new(
      &completed_builds,
      &completed_binds,
      &manifest,
      "/parent/out".to_string(),
    );

    // Create child with different out_dir
    let mut child = parent.with_out_dir("/child/out".to_string());

    // Child should have different out_dir
    assert_eq!(child.resolve_out().unwrap(), "/child/out");

    // Child should have fresh action results
    assert_eq!(child.action_count(), 0);
    child.push_action_result("child_action".to_string());
    assert_eq!(child.action_count(), 1);
  }
}
