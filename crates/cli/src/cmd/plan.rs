//! Implementation of the `sys plan` command.
//!
//! This command evaluates a Lua configuration file and writes the resulting
//! manifest to a plan directory for later application.

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use syslua_lib::eval::evaluate_config;
use syslua_lib::platform::paths;
use syslua_lib::util::hash::Hashable;

/// Execute the plan command.
///
/// Evaluates the given Lua configuration file and writes the manifest to:
/// - `/syslua/plans/<hash>/manifest.json` if running as root/admin
/// - `~/.local/share/syslua/plans/<hash>/manifest.json` otherwise
///
/// Prints a summary including the plan hash, build/bind counts, and output path.
pub fn cmd_plan(file: &str) -> Result<()> {
  let path = Path::new(file);

  // Evaluate the Lua config
  let manifest = evaluate_config(path).with_context(|| format!("Failed to evaluate config: {}", file))?;

  // Compute manifest hash (truncated)
  let hash = manifest.compute_hash().context("Failed to compute manifest hash")?;

  // Determine base directory based on privileges
  let base_dir = if syslua_lib::platform::is_elevated() {
    paths::root_dir()
  } else {
    paths::data_dir()
  };

  // Create plan directory
  let plan_dir = base_dir.join("plans").join(&hash.0);
  fs::create_dir_all(&plan_dir).with_context(|| format!("Failed to create plan directory: {}", plan_dir.display()))?;

  // Write manifest as pretty-printed JSON
  let manifest_path = plan_dir.join("manifest.json");
  let manifest_json = serde_json::to_string_pretty(&manifest).context("Failed to serialize manifest")?;
  fs::write(&manifest_path, &manifest_json)
    .with_context(|| format!("Failed to write manifest: {}", manifest_path.display()))?;

  // Print summary
  println!("Plan: {}", &hash);
  println!("Builds: {}", manifest.builds.len());
  println!("Binds: {}", manifest.bindings.len());
  println!("Path: {}", manifest_path.display());

  Ok(())
}
