//! Implementation of the `sys destroy` command.
//!
//! This command destroys all binds from the current snapshot, effectively
//! removing everything syslua has applied.

use anyhow::{Context, Result};

use syslua_lib::execute::{DestroyOptions, ExecuteConfig, destroy};
use syslua_lib::platform;

/// Execute the destroy command.
///
/// Destroys all binds from the current snapshot:
/// - Loads current state from snapshots
/// - Executes destroy_actions for each bind in reverse dependency order
/// - Cleans up bind state files
/// - Clears the current snapshot pointer
///
/// Prints a summary including counts of binds destroyed and builds orphaned.
pub fn cmd_destroy(dry_run: bool) -> Result<()> {
  // Determine if running as elevated
  let system = platform::is_elevated();

  let options = DestroyOptions {
    execute: ExecuteConfig { parallelism: 4, system },
    system,
    dry_run,
  };

  // Run async destroy
  let rt = tokio::runtime::Runtime::new().context("Failed to create async runtime")?;
  let result = rt.block_on(destroy(&options)).context("Destroy failed")?;

  // Print summary
  println!();
  if dry_run {
    println!("Destroy dry run:");
    println!("  Would destroy {} bind(s)", result.binds_destroyed);
    println!("  Would orphan {} build(s) (for future GC)", result.builds_orphaned);
  } else if result.binds_destroyed == 0 {
    println!("Nothing to destroy.");
  } else {
    println!("Destroy complete!");
    println!("  Binds destroyed: {}", result.binds_destroyed);
    println!("  Builds orphaned: {} (for future GC)", result.builds_orphaned);
  }

  Ok(())
}
