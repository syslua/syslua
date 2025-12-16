//! Diff computation between manifests.
//!
//! This module computes the difference between a desired manifest and the
//! current state, determining what builds need to be realized and what
//! binds need to be applied or destroyed.

use std::collections::HashSet;
use std::path::Path;

use crate::build::store::build_dir_name;
use crate::manifest::Manifest;
use crate::util::hash::ObjectHash;

/// Diff between desired and current state.
///
/// This struct describes what changes need to be made to transform
/// the current state into the desired state.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct StateDiff {
  /// Builds that need to be realized (not in store).
  pub builds_to_realize: Vec<ObjectHash>,

  /// Builds that are already cached (in store).
  pub builds_cached: Vec<ObjectHash>,

  /// Binds to apply (in desired, not in current).
  pub binds_to_apply: Vec<ObjectHash>,

  /// Binds to destroy (in current, not in desired).
  pub binds_to_destroy: Vec<ObjectHash>,

  /// Binds unchanged (same hash in both).
  pub binds_unchanged: Vec<ObjectHash>,
}

impl StateDiff {
  /// Returns true if there are no changes to make.
  pub fn is_empty(&self) -> bool {
    self.builds_to_realize.is_empty() && self.binds_to_apply.is_empty() && self.binds_to_destroy.is_empty()
  }

  /// Returns the total number of builds in the desired manifest.
  pub fn total_builds(&self) -> usize {
    self.builds_to_realize.len() + self.builds_cached.len()
  }

  /// Returns the total number of binds in the desired manifest.
  pub fn total_binds(&self) -> usize {
    self.binds_to_apply.len() + self.binds_unchanged.len()
  }
}

/// Compute diff between desired manifest and current state.
///
/// # Arguments
///
/// * `desired` - The manifest from evaluating the config (target state)
/// * `current` - The manifest from the current snapshot (None if first apply)
/// * `store_path` - Path to the store to check for cached builds
///
/// # Returns
///
/// A [`StateDiff`] describing what changes need to be made.
///
/// # Build Diff Logic
///
/// For each build in the desired manifest:
/// - If the build output directory exists in the store → `builds_cached`
/// - Otherwise → `builds_to_realize`
///
/// # Bind Diff Logic
///
/// - Binds in desired but not in current → `binds_to_apply`
/// - Binds in current but not in desired → `binds_to_destroy`
/// - Binds in both (same hash) → `binds_unchanged`
///
/// Note: If a bind's hash changes (modified definition), the old bind is destroyed
/// and the new one is applied. This is handled by the set difference logic.
pub fn compute_diff(desired: &Manifest, current: Option<&Manifest>, store_path: &Path) -> StateDiff {
  let mut diff = StateDiff::default();

  // Compute build diff
  for (hash, build_def) in &desired.builds {
    if build_exists_in_store(&build_def.name, build_def.version.as_deref(), hash, store_path) {
      diff.builds_cached.push(hash.clone());
    } else {
      diff.builds_to_realize.push(hash.clone());
    }
  }

  // Compute bind diff
  let desired_binds: HashSet<&ObjectHash> = desired.bindings.keys().collect();
  let current_binds: HashSet<&ObjectHash> = current.map(|m| m.bindings.keys().collect()).unwrap_or_default();

  // Binds to apply: in desired but not in current
  for hash in desired_binds.difference(&current_binds) {
    diff.binds_to_apply.push((*hash).clone());
  }

  // Binds to destroy: in current but not in desired
  for hash in current_binds.difference(&desired_binds) {
    diff.binds_to_destroy.push((*hash).clone());
  }

  // Binds unchanged: in both
  for hash in desired_binds.intersection(&current_binds) {
    diff.binds_unchanged.push((*hash).clone());
  }

  diff
}

/// Check if a build's output directory exists in the store.
fn build_exists_in_store(name: &str, version: Option<&str>, hash: &ObjectHash, store_path: &Path) -> bool {
  let dir_name = build_dir_name(name, version, hash);
  let build_path = store_path.join("obj").join(dir_name);
  build_path.exists()
}

#[cfg(test)]
mod tests {
  use super::*;
  use crate::bind::BindDef;
  use crate::build::BuildDef;
  use tempfile::TempDir;

  fn make_build_def(name: &str) -> BuildDef {
    BuildDef {
      name: name.to_string(),
      version: None,
      inputs: None,
      apply_actions: vec![],
      outputs: None,
    }
  }

  fn make_bind_def() -> BindDef {
    BindDef {
      inputs: None,
      apply_actions: vec![],
      outputs: None,
      destroy_actions: None,
    }
  }

  #[test]
  fn diff_empty_manifests() {
    let temp_dir = TempDir::new().unwrap();
    let desired = Manifest::default();
    let diff = compute_diff(&desired, None, temp_dir.path());

    assert!(diff.is_empty());
    assert_eq!(diff.total_builds(), 0);
    assert_eq!(diff.total_binds(), 0);
  }

  #[test]
  fn diff_first_apply() {
    let temp_dir = TempDir::new().unwrap();

    let mut desired = Manifest::default();
    desired
      .builds
      .insert(ObjectHash("build1".to_string()), make_build_def("pkg1"));
    desired
      .bindings
      .insert(ObjectHash("bind1".to_string()), make_bind_def());

    let diff = compute_diff(&desired, None, temp_dir.path());

    assert!(!diff.is_empty());
    assert_eq!(diff.builds_to_realize.len(), 1);
    assert_eq!(diff.builds_cached.len(), 0);
    assert_eq!(diff.binds_to_apply.len(), 1);
    assert_eq!(diff.binds_to_destroy.len(), 0);
    assert_eq!(diff.binds_unchanged.len(), 0);
  }

  #[test]
  fn diff_cached_build() {
    let temp_dir = TempDir::new().unwrap();

    // Create the build directory to simulate cached build
    let build_hash = ObjectHash("abc123def45678901234".to_string());
    let build_dir = temp_dir.path().join("obj").join("pkg1-abc123def45678901234");
    std::fs::create_dir_all(&build_dir).unwrap();

    let mut desired = Manifest::default();
    desired.builds.insert(build_hash.clone(), make_build_def("pkg1"));

    let diff = compute_diff(&desired, None, temp_dir.path());

    assert!(diff.is_empty()); // No builds to realize
    assert_eq!(diff.builds_cached.len(), 1);
    assert_eq!(diff.builds_to_realize.len(), 0);
  }

  #[test]
  fn diff_no_changes() {
    let temp_dir = TempDir::new().unwrap();

    // Create cached build
    let build_hash = ObjectHash("abc123def45678901234".to_string());
    let build_dir = temp_dir.path().join("obj").join("pkg1-abc123def45678901234");
    std::fs::create_dir_all(&build_dir).unwrap();

    let bind_hash = ObjectHash("bind1".to_string());

    let mut manifest = Manifest::default();
    manifest.builds.insert(build_hash, make_build_def("pkg1"));
    manifest.bindings.insert(bind_hash, make_bind_def());

    // Same manifest for desired and current
    let diff = compute_diff(&manifest, Some(&manifest), temp_dir.path());

    assert!(diff.is_empty());
    assert_eq!(diff.binds_unchanged.len(), 1);
  }

  #[test]
  fn diff_new_bind() {
    let temp_dir = TempDir::new().unwrap();

    let mut current = Manifest::default();
    current
      .bindings
      .insert(ObjectHash("existing".to_string()), make_bind_def());

    let mut desired = Manifest::default();
    desired
      .bindings
      .insert(ObjectHash("existing".to_string()), make_bind_def());
    desired.bindings.insert(ObjectHash("new".to_string()), make_bind_def());

    let diff = compute_diff(&desired, Some(&current), temp_dir.path());

    assert_eq!(diff.binds_to_apply.len(), 1);
    assert_eq!(diff.binds_unchanged.len(), 1);
    assert_eq!(diff.binds_to_destroy.len(), 0);
  }

  #[test]
  fn diff_removed_bind() {
    let temp_dir = TempDir::new().unwrap();

    let mut current = Manifest::default();
    current.bindings.insert(ObjectHash("keep".to_string()), make_bind_def());
    current
      .bindings
      .insert(ObjectHash("remove".to_string()), make_bind_def());

    let mut desired = Manifest::default();
    desired.bindings.insert(ObjectHash("keep".to_string()), make_bind_def());

    let diff = compute_diff(&desired, Some(&current), temp_dir.path());

    assert_eq!(diff.binds_to_apply.len(), 0);
    assert_eq!(diff.binds_unchanged.len(), 1);
    assert_eq!(diff.binds_to_destroy.len(), 1);
    assert!(diff.binds_to_destroy.contains(&ObjectHash("remove".to_string())));
  }

  #[test]
  fn diff_modified_bind() {
    let temp_dir = TempDir::new().unwrap();

    // "Modified" bind means the hash changed, so old one is destroyed, new one applied
    let mut current = Manifest::default();
    current
      .bindings
      .insert(ObjectHash("old_hash".to_string()), make_bind_def());

    let mut desired = Manifest::default();
    desired
      .bindings
      .insert(ObjectHash("new_hash".to_string()), make_bind_def());

    let diff = compute_diff(&desired, Some(&current), temp_dir.path());

    assert_eq!(diff.binds_to_apply.len(), 1);
    assert_eq!(diff.binds_to_destroy.len(), 1);
    assert_eq!(diff.binds_unchanged.len(), 0);
  }

  #[test]
  fn diff_mixed_scenario() {
    let temp_dir = TempDir::new().unwrap();

    // Create some cached builds
    std::fs::create_dir_all(temp_dir.path().join("obj/cached-abc123def45678901234")).unwrap();

    let mut current = Manifest::default();
    current
      .builds
      .insert(ObjectHash("abc123def45678901234".to_string()), make_build_def("cached"));
    current
      .bindings
      .insert(ObjectHash("unchanged_bind".to_string()), make_bind_def());
    current
      .bindings
      .insert(ObjectHash("removed_bind".to_string()), make_bind_def());

    let mut desired = Manifest::default();
    desired
      .builds
      .insert(ObjectHash("abc123def45678901234".to_string()), make_build_def("cached"));
    desired.builds.insert(
      ObjectHash("new_build_hash12345678".to_string()),
      make_build_def("new_pkg"),
    );
    desired
      .bindings
      .insert(ObjectHash("unchanged_bind".to_string()), make_bind_def());
    desired
      .bindings
      .insert(ObjectHash("new_bind".to_string()), make_bind_def());

    let diff = compute_diff(&desired, Some(&current), temp_dir.path());

    assert_eq!(diff.builds_cached.len(), 1);
    assert_eq!(diff.builds_to_realize.len(), 1);
    assert_eq!(diff.binds_unchanged.len(), 1);
    assert_eq!(diff.binds_to_apply.len(), 1);
    assert_eq!(diff.binds_to_destroy.len(), 1);
  }

  #[test]
  fn state_diff_is_empty() {
    let diff = StateDiff::default();
    assert!(diff.is_empty());

    let diff_with_cached = StateDiff {
      builds_cached: vec![ObjectHash("x".to_string())],
      ..Default::default()
    };
    assert!(diff_with_cached.is_empty()); // Cached builds don't count as changes

    let diff_with_unchanged = StateDiff {
      binds_unchanged: vec![ObjectHash("x".to_string())],
      ..Default::default()
    };
    assert!(diff_with_unchanged.is_empty()); // Unchanged binds don't count as changes

    let diff_with_realize = StateDiff {
      builds_to_realize: vec![ObjectHash("x".to_string())],
      ..Default::default()
    };
    assert!(!diff_with_realize.is_empty());

    let diff_with_apply = StateDiff {
      binds_to_apply: vec![ObjectHash("x".to_string())],
      ..Default::default()
    };
    assert!(!diff_with_apply.is_empty());

    let diff_with_destroy = StateDiff {
      binds_to_destroy: vec![ObjectHash("x".to_string())],
      ..Default::default()
    };
    assert!(!diff_with_destroy.is_empty());
  }
}
