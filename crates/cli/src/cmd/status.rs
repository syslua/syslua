//! Status command implementation.
//!
//! Displays current snapshot state including build/bind counts and store usage.

use anyhow::Result;
use std::path::Path;

use syslua_lib::bind::store::bind_dir_path;
use syslua_lib::build::store::build_dir_path;
use syslua_lib::platform::paths::snapshots_dir;
use syslua_lib::snapshot::SnapshotStore;

use crate::output::{
  self, format_bytes, print_error, print_info, print_json, print_stat, print_success, truncate_hash,
};

pub fn cmd_status(verbose: bool, json: bool) -> Result<()> {
  let store = SnapshotStore::new(snapshots_dir());

  let snapshot = match store.load_current() {
    Ok(Some(snap)) => snap,
    Ok(None) => {
      print_info("No snapshot found. Run 'sys apply' to create one.");
      return Ok(());
    }
    Err(e) => {
      print_error(&format!("Error loading snapshot: {}", e));
      return Err(e.into());
    }
  };

  let usage = calculate_store_usage(&snapshot.manifest);

  if json {
    let build_list: Vec<_> = snapshot
      .manifest
      .builds
      .iter()
      .map(|(hash, build)| serde_json::json!({ "id": build.id, "hash": hash.0 }))
      .collect();
    let bind_list: Vec<_> = snapshot
      .manifest
      .bindings
      .iter()
      .map(|(hash, bind)| serde_json::json!({ "id": bind.id, "hash": hash.0 }))
      .collect();
    let json_output = serde_json::json!({ "snapshot_id": snapshot.id, "created_at": snapshot.created_at, "builds": { "count": snapshot.manifest.builds.len(), "items": build_list }, "binds": { "count": snapshot.manifest.bindings.len(), "items": bind_list }, "store_usage_bytes": usage });
    print_json(&json_output)?;
  } else {
    print_success(&format!("Current snapshot: {}", snapshot.id));
    print_stat("Created", &snapshot.created_at.to_string());
    println!();
    print_stat("Builds", &snapshot.manifest.builds.len().to_string());
    print_stat("Binds", &snapshot.manifest.bindings.len().to_string());

    if verbose {
      if !snapshot.manifest.builds.is_empty() {
        println!();
        println!("Builds:");
        for (hash, build) in &snapshot.manifest.builds {
          match &build.id {
            Some(id) => println!("  {} {}-{}", output::symbols::INFO, id, truncate_hash(&hash.0)),
            None => println!("  {} {}", output::symbols::INFO, truncate_hash(&hash.0)),
          }
        }
      }

      if !snapshot.manifest.bindings.is_empty() {
        println!();
        println!("Binds:");
        for (hash, bind) in &snapshot.manifest.bindings {
          match &bind.id {
            Some(id) => println!("  {} {}-{}", output::symbols::INFO, id, truncate_hash(&hash.0)),
            None => println!("  {} {}", output::symbols::INFO, truncate_hash(&hash.0)),
          }
        }
      }
    }

    println!();
    print_stat("Store usage", &format_bytes(usage));
  }

  Ok(())
}

fn dir_size(path: &Path) -> u64 {
  if !path.exists() {
    return 0;
  }

  let mut size = 0;
  if let Ok(entries) = std::fs::read_dir(path) {
    for entry in entries.flatten() {
      let entry_path = entry.path();
      if entry_path.is_file() {
        size += entry.metadata().map(|m| m.len()).unwrap_or(0);
      } else if entry_path.is_dir() {
        size += dir_size(&entry_path);
      }
    }
  }
  size
}

fn calculate_store_usage(manifest: &syslua_lib::manifest::Manifest) -> u64 {
  let mut total = 0;

  for hash in manifest.builds.keys() {
    let build_path = build_dir_path(hash);
    total += dir_size(&build_path);
  }

  for hash in manifest.bindings.keys() {
    let bind_path = bind_dir_path(hash);
    total += dir_size(&bind_path);
  }

  total
}
