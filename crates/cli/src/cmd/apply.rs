//! Implementation of the `sys apply` command.
//!
//! This command evaluates a Lua configuration file and applies changes to the system,
//! tracking state via snapshots.

use std::path::Path;
use std::time::Instant;

use anyhow::{Context, Result};
use owo_colors::{OwoColorize, Stream};
use tracing::info;

use syslua_lib::execute::{ApplyOptions, ExecuteConfig, apply};

use crate::output::{
  OutputFormat, format_duration, print_error, print_info, print_json, print_stat, print_success, print_warning,
  symbols, truncate_hash,
};
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
pub fn cmd_apply(file: &str, repair: bool, output: OutputFormat) -> Result<()> {
  let start = Instant::now();
  let path = Path::new(file);

  let options = ApplyOptions {
    execute: ExecuteConfig::default(),
    dry_run: false,
    repair,
  };

  // Run async apply
  let rt = tokio::runtime::Runtime::new().context("Failed to create async runtime")?;
  let result = rt.block_on(apply(path, &options)).context("Apply failed")?;

  if output.is_json() {
    print_json(&result)?;
  } else {
    println!();
    print_success("Apply complete!");
    print_stat("Snapshot", truncate_hash(&result.snapshot.id));
    print_stat("Builds realized", &result.execution.realized.len().to_string());
    print_stat("Builds cached", &result.diff.builds_cached.len().to_string());
    print_stat("Binds applied", &result.execution.applied.len().to_string());
    print_stat("Binds updated", &result.binds_updated.to_string());
    print_stat("Binds destroyed", &result.binds_destroyed.to_string());
    print_stat("Binds unchanged", &result.diff.binds_unchanged.len().to_string());
    print_stat("Duration", &format_duration(start.elapsed()));

    let drifted_count = result.drift_results.iter().filter(|r| r.result.drifted).count();
    if drifted_count > 0 {
      eprintln!();
      print_warning(&format!("Drift detected: {} bind(s)", drifted_count));
      for drift in result.drift_results.iter().filter(|r| r.result.drifted) {
        let id = drift.id.as_deref().unwrap_or(&drift.hash.0);
        if let Some(ref msg) = drift.result.message {
          eprintln!(
            "    {} {}: {}",
            symbols::MINUS.if_supports_color(Stream::Stderr, |s| s.yellow()),
            id,
            msg
          );
        } else {
          eprintln!(
            "    {} {}",
            symbols::MINUS.if_supports_color(Stream::Stderr, |s| s.yellow()),
            id
          );
        }
      }
      if repair {
        print_info(&format!("Binds repaired: {}", drifted_count));
      } else {
        print_info("Run with --repair to fix drifted binds");
      }
    }

    if !result.execution.is_success() {
      if let Some((hash, ref err)) = result.execution.build_failed {
        print_error(&format!("Build failed: {} - {}", truncate_hash(&hash.0), err));
      }
      if let Some((hash, ref err)) = result.execution.bind_failed {
        print_error(&format!("Bind failed: {} - {}", truncate_hash(&hash.0), err));
      }
    }
  }

  // Print plan directory
  let snapshot_path = paths::snapshots_dir().join(format!("{}.json", result.snapshot.id));
  info!(path = %snapshot_path.display(), "snapshot saved");

  Ok(())
}
