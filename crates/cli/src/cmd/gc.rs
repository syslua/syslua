use std::time::Instant;

use anyhow::{Context, Result};

use syslua_lib::gc::collect_garbage;
use syslua_lib::store_lock::{LockMode, StoreLock};

use crate::output::{OutputFormat, format_bytes, format_duration, print_info, print_json, print_stat, print_success};

pub fn cmd_gc(dry_run: bool, output: OutputFormat) -> Result<()> {
  let start = Instant::now();

  let _lock = StoreLock::acquire(LockMode::Exclusive, "gc").context("Failed to acquire store lock")?;

  let result = collect_garbage(dry_run)?;

  if output.is_json() {
    print_json(&result)?;
  } else {
    println!();
    if dry_run {
      print_info("Dry run - no changes made");
    } else {
      print_success("Garbage collection complete!");
    }
    print_stat("Builds removed", &result.stats.builds_deleted.to_string());
    print_stat("Inputs removed", &result.stats.inputs_deleted.to_string());
    print_stat("Space freed", &format_bytes(result.stats.total_bytes_freed()));
    print_stat("Duration", &format_duration(start.elapsed()));
  }

  Ok(())
}
