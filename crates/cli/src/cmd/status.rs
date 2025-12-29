use std::path::Path;
use std::process::ExitCode;

use syslua_lib::bind::store::bind_dir_path;
use syslua_lib::build::store::build_dir_path;
use syslua_lib::platform::paths::snapshots_dir;
use syslua_lib::snapshot::SnapshotStore;

pub fn cmd_status(verbose: bool) -> ExitCode {
  let store = SnapshotStore::new(snapshots_dir());

  let snapshot = match store.load_current() {
    Ok(Some(snap)) => snap,
    Ok(None) => {
      println!("No snapshot found. Run 'sys apply' to create one.");
      return ExitCode::SUCCESS;
    }
    Err(e) => {
      eprintln!("Error loading snapshot: {}", e);
      return ExitCode::FAILURE;
    }
  };

  println!("Current snapshot: {}", snapshot.id);
  println!("Created: {}", snapshot.created_at);
  println!();
  println!("Builds: {}", snapshot.manifest.builds.len());
  println!("Binds: {}", snapshot.manifest.bindings.len());

  if verbose {
    if !snapshot.manifest.builds.is_empty() {
      println!();
      for (hash, build) in &snapshot.manifest.builds {
        match &build.id {
          Some(id) => println!("  {}-{}", id, hash.0),
          None => println!("  {}", hash.0),
        }
      }
    }

    if !snapshot.manifest.bindings.is_empty() {
      println!();
      for (hash, bind) in &snapshot.manifest.bindings {
        match &bind.id {
          Some(id) => println!("  {}-{}", id, hash.0),
          None => println!("  {}", hash.0),
        }
      }
    }
  }

  let usage = calculate_store_usage(&snapshot.manifest);
  println!();
  println!("Store usage: {}", format_bytes(usage));

  ExitCode::SUCCESS
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

fn format_bytes(bytes: u64) -> String {
  const KB: u64 = 1024;
  const MB: u64 = KB * 1024;
  const GB: u64 = MB * 1024;

  if bytes >= GB {
    format!("{:.1} GB", bytes as f64 / GB as f64)
  } else if bytes >= MB {
    format!("{:.1} MB", bytes as f64 / MB as f64)
  } else if bytes >= KB {
    format!("{:.1} KB", bytes as f64 / KB as f64)
  } else {
    format!("{} bytes", bytes)
  }
}
