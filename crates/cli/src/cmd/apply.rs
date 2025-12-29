//! Implementation of the `sys apply` command.
//!
//! This command evaluates a Lua configuration file and applies changes to the system,
//! tracking state via snapshots.

use std::path::Path;

use anyhow::{Context, Result};
use tracing::info;

use syslua_lib::execute::{ApplyOptions, ExecuteConfig, apply};
use syslua_lib::platform::paths;

/// Execute the apply command.
///
/// Evaluates the given Lua configuration file and applies the resulting manifest:
/// - Loads current state from snapshots
/// - Computes diff between desired and current state
/// - Destroys removed binds
/// - Realizes new builds
/// - Applies new binds
/// - Saves new snapshot
///
/// Prints a summary including counts of builds realized, binds applied/destroyed, and the snapshot ID.
pub fn cmd_apply(file: &str, repair: bool) -> Result<()> {
  let path = Path::new(file);

  let options = ApplyOptions {
    execute: ExecuteConfig::default(),
    dry_run: false,
    repair,
  };

  // Run async apply
  let rt = tokio::runtime::Runtime::new().context("Failed to create async runtime")?;
  let result = rt.block_on(apply(path, &options)).context("Apply failed")?;

  // Print summary
  println!();
  println!("Apply complete!");
  println!("  Snapshot: {}", result.snapshot.id);
  println!("  Builds realized: {}", result.execution.realized.len());
  println!("  Builds cached: {}", result.diff.builds_cached.len());
  println!("  Binds applied: {}", result.execution.applied.len());
  println!("  Binds updated: {}", result.binds_updated);
  println!("  Binds destroyed: {}", result.binds_destroyed);
  println!("  Binds unchanged: {}", result.diff.binds_unchanged.len());

  let drifted_count = result.drift_results.iter().filter(|r| r.result.drifted).count();
  if drifted_count > 0 {
    println!();
    println!("  Drift detected: {} bind(s)", drifted_count);
    for drift in result.drift_results.iter().filter(|r| r.result.drifted) {
      let id = drift.id.as_deref().unwrap_or(&drift.hash.0);
      if let Some(ref msg) = drift.result.message {
        println!("    - {}: {}", id, msg);
      } else {
        println!("    - {}", id);
      }
    }
    if repair {
      println!("  Binds repaired: {}", drifted_count);
    } else {
      println!("  Run with --repair to fix drifted binds");
    }
  }

  if !result.execution.is_success() {
    if let Some((hash, ref err)) = result.execution.build_failed {
      eprintln!("  Build failed: {} - {}", hash.0, err);
    }
    if let Some((hash, ref err)) = result.execution.bind_failed {
      eprintln!("  Bind failed: {} - {}", hash.0, err);
    }
  }

  // Print plan directory
  let snapshot_path = paths::snapshots_dir().join(format!("{}.json", result.snapshot.id));
  info!(path = %snapshot_path.display(), "snapshot saved");

  Ok(())
}
