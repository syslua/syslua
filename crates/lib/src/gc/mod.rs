use std::collections::HashSet;
use std::path::PathBuf;
use std::{fs, io};

use thiserror::Error;
use tracing::{debug, info, warn};
use walkdir::WalkDir;

use crate::build::execute::BUILD_COMPLETE_MARKER;
use crate::platform::paths::{cache_dir, store_dir};
use crate::snapshot::SnapshotStore;

#[derive(Debug, Error)]
pub enum GcError {
  #[error("failed to list snapshots: {0}")]
  ListSnapshots(String),

  #[error("failed to load snapshot {id}: {message}")]
  LoadSnapshot { id: String, message: String },

  #[error("failed to read store directory: {0}")]
  ReadStore(#[from] io::Error),

  #[error("failed to delete {path}: {message}")]
  Delete { path: PathBuf, message: String },
}

#[derive(Debug, Default, serde::Serialize)]
pub struct GcStats {
  pub builds_scanned: usize,
  pub builds_deleted: usize,
  pub builds_bytes_freed: u64,
  pub inputs_scanned: usize,
  pub inputs_deleted: usize,
  pub inputs_bytes_freed: u64,
}

impl GcStats {
  pub fn total_deleted(&self) -> usize {
    self.builds_deleted + self.inputs_deleted
  }

  pub fn total_bytes_freed(&self) -> u64 {
    self.builds_bytes_freed + self.inputs_bytes_freed
  }
}

#[derive(Debug, serde::Serialize)]
pub struct GcResult {
  pub stats: GcStats,
  pub deleted_paths: Vec<PathBuf>,
}

fn collect_live_hashes(snapshot_store: &SnapshotStore) -> Result<HashSet<String>, GcError> {
  let mut live = HashSet::new();

  let snapshots = snapshot_store
    .list()
    .map_err(|e| GcError::ListSnapshots(e.to_string()))?;

  for meta in snapshots {
    match snapshot_store.load_snapshot(&meta.id) {
      Ok(snapshot) => {
        for hash in snapshot.manifest.builds.keys() {
          live.insert(hash.0.clone());
        }

        for hash in snapshot.manifest.bindings.keys() {
          live.insert(hash.0.clone());
        }
      }
      Err(e) => {
        warn!(id = %meta.id, error = %e, "skipping snapshot with incompatible format");
      }
    }
  }

  debug!(count = live.len(), "collected live hashes from snapshots");
  Ok(live)
}

fn dir_size(path: &std::path::Path) -> u64 {
  WalkDir::new(path)
    .into_iter()
    .filter_map(|e| e.ok())
    .filter(|e| e.file_type().is_file())
    .filter_map(|e| e.metadata().ok())
    .map(|m| m.len())
    .sum()
}

fn is_complete_build(path: &std::path::Path) -> bool {
  path.join(BUILD_COMPLETE_MARKER).exists()
}

pub fn collect_garbage(dry_run: bool) -> Result<GcResult, GcError> {
  let snapshot_store = SnapshotStore::default_store();
  let live_hashes = collect_live_hashes(&snapshot_store)?;

  let mut stats = GcStats::default();
  let mut deleted_paths = Vec::new();

  let build_dir = store_dir().join("build");
  if build_dir.exists() {
    sweep_builds(&build_dir, &live_hashes, dry_run, &mut stats, &mut deleted_paths)?;
  }

  let inputs_cache = cache_dir().join("inputs").join("store");
  if inputs_cache.exists() {
    sweep_inputs_cache(&inputs_cache, &live_hashes, dry_run, &mut stats, &mut deleted_paths)?;
  }

  info!(
    builds_deleted = stats.builds_deleted,
    inputs_deleted = stats.inputs_deleted,
    bytes_freed = stats.total_bytes_freed(),
    dry_run,
    "garbage collection complete"
  );

  Ok(GcResult { stats, deleted_paths })
}

fn sweep_builds(
  build_dir: &std::path::Path,
  live_hashes: &HashSet<String>,
  dry_run: bool,
  stats: &mut GcStats,
  deleted_paths: &mut Vec<PathBuf>,
) -> Result<(), GcError> {
  let entries = fs::read_dir(build_dir)?;

  for entry in entries.flatten() {
    let path = entry.path();
    if !path.is_dir() {
      continue;
    }

    stats.builds_scanned += 1;

    let dir_name = match path.file_name().and_then(|n| n.to_str()) {
      Some(name) => name.to_string(),
      None => continue,
    };

    let is_live = live_hashes.contains(&dir_name);
    let is_complete = is_complete_build(&path);

    if is_live && is_complete {
      continue;
    }

    let size = dir_size(&path);

    if !is_complete {
      debug!(path = %path.display(), "removing incomplete build");
    } else {
      debug!(path = %path.display(), "removing unreferenced build");
    }

    if dry_run {
      stats.builds_deleted += 1;
      stats.builds_bytes_freed += size;
      deleted_paths.push(path);
    } else {
      match fs::remove_dir_all(&path) {
        Ok(()) => {
          stats.builds_deleted += 1;
          stats.builds_bytes_freed += size;
          deleted_paths.push(path);
        }
        Err(e) => {
          warn!(path = %path.display(), error = %e, "failed to delete build directory");
        }
      }
    }
  }

  Ok(())
}

fn sweep_inputs_cache(
  cache_dir: &std::path::Path,
  live_hashes: &HashSet<String>,
  dry_run: bool,
  stats: &mut GcStats,
  deleted_paths: &mut Vec<PathBuf>,
) -> Result<(), GcError> {
  let entries = fs::read_dir(cache_dir)?;

  for entry in entries.flatten() {
    let path = entry.path();
    if !path.is_dir() {
      continue;
    }

    stats.inputs_scanned += 1;

    let dir_name = match path.file_name().and_then(|n| n.to_str()) {
      Some(name) => name.to_string(),
      None => continue,
    };

    let hash_part = extract_hash_from_cache_name(&dir_name);

    if live_hashes.contains(&hash_part) {
      continue;
    }

    let size = dir_size(&path);
    debug!(path = %path.display(), "removing unreferenced input cache");

    if dry_run {
      stats.inputs_deleted += 1;
      stats.inputs_bytes_freed += size;
      deleted_paths.push(path);
    } else {
      match fs::remove_dir_all(&path) {
        Ok(()) => {
          stats.inputs_deleted += 1;
          stats.inputs_bytes_freed += size;
          deleted_paths.push(path);
        }
        Err(e) => {
          warn!(path = %path.display(), error = %e, "failed to delete input cache directory");
        }
      }
    }
  }

  Ok(())
}

fn extract_hash_from_cache_name(name: &str) -> String {
  if let Some(pos) = name.rfind('-') {
    name[pos + 1..].to_string()
  } else {
    name.to_string()
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_extract_hash_from_cache_name() {
    assert_eq!(extract_hash_from_cache_name("myinput-abc123"), "abc123");
    assert_eq!(
      extract_hash_from_cache_name("complex-name-with-dashes-xyz789"),
      "xyz789"
    );
    assert_eq!(extract_hash_from_cache_name("nohash"), "nohash");
  }

  #[test]
  fn test_gc_stats_totals() {
    let stats = GcStats {
      builds_scanned: 10,
      builds_deleted: 3,
      builds_bytes_freed: 1000,
      inputs_scanned: 5,
      inputs_deleted: 2,
      inputs_bytes_freed: 500,
    };

    assert_eq!(stats.total_deleted(), 5);
    assert_eq!(stats.total_bytes_freed(), 1500);
  }
}
