//! Implementation of the `sys plan` command.
//!
//! This command evaluates a Lua configuration file and writes the resulting
//! manifest to a plan directory for later application.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use syslua_lib::eval::evaluate_config;
use syslua_lib::execute::{ExecuteConfig, check_unchanged_binds};
use syslua_lib::platform::paths::{plans_dir, store_dir};
use syslua_lib::snapshot::{SnapshotStore, compute_diff};
use syslua_lib::util::hash::Hashable;

pub fn cmd_plan(file: &str) -> Result<()> {
  let path = Path::new(file);

  let manifest = evaluate_config(path).with_context(|| format!("Failed to evaluate config: {}", file))?;

  let hash = manifest.compute_hash().context("Failed to compute manifest hash")?;

  let plan_dir = plans_dir().join(&hash.0);
  fs::create_dir_all(&plan_dir).with_context(|| format!("Failed to create plan directory: {}", plan_dir.display()))?;

  let manifest_path = plan_dir.join("manifest.json");
  let manifest_json = serde_json::to_string_pretty(&manifest).context("Failed to serialize manifest")?;
  fs::write(&manifest_path, &manifest_json)
    .with_context(|| format!("Failed to write manifest: {}", manifest_path.display()))?;

  let snapshot_store = SnapshotStore::default_store();
  let current_snapshot = snapshot_store
    .load_current()
    .context("Failed to load current snapshot")?;
  let current_manifest = current_snapshot.as_ref().map(|s| &s.manifest);

  let store_path = store_dir();
  let diff = compute_diff(&manifest, current_manifest, &store_path);

  println!("Plan: {}", &hash);
  println!("Builds: {}", manifest.builds.len());
  println!("  To realize: {}", diff.builds_to_realize.len());
  println!("  Cached: {}", diff.builds_cached.len());
  println!("Binds: {}", manifest.bindings.len());
  println!("  To apply: {}", diff.binds_to_apply.len());
  println!("  To update: {}", diff.binds_to_update.len());
  println!("  To destroy: {}", diff.binds_to_destroy.len());
  println!("  Unchanged: {}", diff.binds_unchanged.len());
  println!("Path: {}", manifest_path.display());

  if !diff.binds_unchanged.is_empty() {
    let rt = tokio::runtime::Runtime::new().context("Failed to create async runtime")?;
    let config = ExecuteConfig::default();

    let drift_results = rt
      .block_on(check_unchanged_binds(&diff.binds_unchanged, &manifest, &config))
      .context("Failed to check for drift")?;

    let drifted_count = drift_results.iter().filter(|r| r.result.drifted).count();
    if drifted_count > 0 {
      println!();
      println!("Drift detected: {} bind(s)", drifted_count);
      for drift in drift_results.iter().filter(|r| r.result.drifted) {
        let id = drift.id.as_deref().unwrap_or(&drift.hash.0);
        if let Some(ref msg) = drift.result.message {
          println!("  - {}: {}", id, msg);
        } else {
          println!("  - {}", id);
        }
      }
    }
  }

  Ok(())
}
