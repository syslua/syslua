//! Implementation of the `sys update` command.
//!
//! This command re-resolves inputs (fetching latest revisions) and
//! updates the lock file and .luarc.json.

use std::time::Instant;

use anyhow::{Context, Result};
use owo_colors::OwoColorize;

use syslua_lib::platform;
use syslua_lib::update::{UpdateOptions, find_config_path, update_inputs};

use crate::output::{format_duration, symbols};

/// Execute the update command.
///
/// Re-resolves inputs by fetching the latest revisions, updates the lock file,
/// and updates .luarc.json for LuaLS IDE integration.
///
/// # Arguments
///
/// * `config` - Optional path to config file. If not provided, uses default resolution.
/// * `inputs` - Specific inputs to update. If empty, all inputs are updated.
/// * `dry_run` - If true, show what would change without making changes.
///
/// # Errors
///
/// Returns an error if the config cannot be found or input resolution fails.
pub fn cmd_update(config: Option<&str>, inputs: Vec<String>, dry_run: bool) -> Result<()> {
  let start = Instant::now();
  let config_path = find_config_path(config).context("Failed to find config file")?;
  let system = platform::is_elevated();

  let options = UpdateOptions {
    inputs,
    dry_run,
    system,
  };

  let result = update_inputs(&config_path, &options).context("Failed to update inputs")?;

  // Print results
  if dry_run {
    println!("{}", "Dry run - no changes written".yellow());
    println!();
  }

  // Print updated direct inputs
  for (name, (old_rev, new_rev)) in &result.updated {
    let prefix = if dry_run { "Would update" } else { "Updated" };
    let old_short = &old_rev[..old_rev.len().min(8)];
    let new_short = &new_rev[..new_rev.len().min(8)];
    println!(
      "  {} {}: {} {} {}",
      symbols::MODIFY.yellow(),
      prefix,
      name.cyan(),
      format!("{} ->", old_short).dimmed(),
      new_short.green()
    );

    // Print transitive updates for this input
    print_transitive_updates(name, &result.transitive_updated, &result.transitive_added, dry_run);
  }

  // Print added direct inputs
  for name in &result.added {
    let prefix = if dry_run { "Would add" } else { "Added" };
    if let Some(resolved) = result.resolved.get(name) {
      let rev_short = &resolved.rev[..resolved.rev.len().min(8)];
      println!(
        "  {} {}: {} ({})",
        symbols::ADD.green(),
        prefix,
        name.cyan(),
        rev_short.dimmed()
      );

      // Print transitive adds for this input
      print_transitive_updates(name, &result.transitive_updated, &result.transitive_added, dry_run);
    }
  }

  // Print unchanged inputs
  if !result.unchanged.is_empty() {
    let names = result.unchanged.join(", ");
    println!("  {} Unchanged: {}", symbols::INFO.dimmed(), names.dimmed());
  }

  // Summary
  let has_changes = !result.updated.is_empty()
    || !result.added.is_empty()
    || !result.transitive_updated.is_empty()
    || !result.transitive_added.is_empty();

  if !has_changes {
    println!("{} All inputs are up to date.", symbols::SUCCESS.green());
  } else if !dry_run {
    println!();
    println!(
      "{} Lock file updated: {}",
      symbols::SUCCESS.green(),
      config_path
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("syslua.lock")
        .display()
    );
    println!(
      "  {} Duration: {}",
      symbols::INFO.dimmed(),
      format_duration(start.elapsed()).dimmed()
    );
  }

  Ok(())
}

/// Print transitive updates/adds for a given parent input.
fn print_transitive_updates(
  parent: &str,
  transitive_updated: &std::collections::BTreeMap<String, (String, String)>,
  transitive_added: &[String],
  dry_run: bool,
) {
  // Print transitive updates that belong to this parent
  for (path, (old_rev, new_rev)) in transitive_updated {
    if path.starts_with(&format!("{}/", parent)) {
      let prefix = if dry_run { "Would update" } else { "Updated" };
      let old_short = &old_rev[..old_rev.len().min(8)];
      let new_short = &new_rev[..new_rev.len().min(8)];
      let rel_path = &path[parent.len() + 1..]; // Remove parent prefix
      println!(
        "    {} {}: {} {} {}",
        symbols::MODIFY.yellow(),
        prefix,
        rel_path.cyan(),
        format!("{} ->", old_short).dimmed(),
        new_short.green()
      );
    }
  }

  // Print transitive adds that belong to this parent
  for path in transitive_added {
    if path.starts_with(&format!("{}/", parent)) {
      let prefix = if dry_run { "Would add" } else { "Added" };
      let rel_path = &path[parent.len() + 1..]; // Remove parent prefix
      println!("    {} {}: {}", symbols::ADD.green(), prefix, rel_path.cyan());
    }
  }
}
