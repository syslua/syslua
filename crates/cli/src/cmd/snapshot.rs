use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Result, bail};
use clap::Subcommand;
use serde::Serialize;
use syslua_lib::{
  platform::paths::snapshots_dir,
  snapshot::SnapshotStore,
  store_lock::{LockMode, StoreLock},
};
use tracing::{debug, info};

use crate::output::{OutputFormat, print_error, print_info, print_json, print_success, print_warning};
use crate::prompts::confirm;

#[derive(Subcommand, Debug)]
pub enum SnapshotCommand {
  /// List all snapshots
  List {
    /// Show additional details (config path, build/bind counts, tags)
    #[arg(short, long)]
    verbose: bool,

    /// Output format
    #[arg(short = 'o', long, value_enum, default_value = "text")]
    output: OutputFormat,
  },

  /// Show details of a specific snapshot
  Show {
    /// Snapshot ID to show
    id: String,

    /// Include list of builds and binds
    #[arg(short, long)]
    verbose: bool,

    /// Output format
    #[arg(short = 'o', long, value_enum, default_value = "text")]
    output: OutputFormat,
  },

  /// Delete snapshots
  Delete {
    /// Snapshot IDs to delete
    ids: Vec<String>,

    /// Delete snapshots older than this duration (e.g., "7d", "24h", "2w")
    #[arg(long, value_parser = humantime::parse_duration)]
    older_than: Option<Duration>,

    /// Preview what would be deleted without actually deleting
    #[arg(long)]
    dry_run: bool,

    /// Skip confirmation prompt
    #[arg(long)]
    force: bool,

    /// Output format
    #[arg(short = 'o', long, value_enum, default_value = "text")]
    output: OutputFormat,
  },

  /// Add a tag to a snapshot
  Tag {
    /// Snapshot ID to tag
    id: String,

    /// Tag name to apply
    name: String,
  },

  /// Remove tag(s) from a snapshot
  Untag {
    /// Snapshot ID to untag
    id: String,

    /// Specific tag to remove (removes all tags if not specified)
    name: Option<String>,
  },
}

#[derive(Debug, Serialize)]
struct DeleteResult {
  deleted: Vec<String>,
  failed: Vec<DeleteFailure>,
  skipped_current: Option<String>,
  dry_run: bool,
}

#[derive(Debug, Serialize)]
struct DeleteFailure {
  id: String,
  error: String,
}

pub fn cmd_snapshot(command: SnapshotCommand) -> Result<()> {
  match command {
    SnapshotCommand::List { verbose, output } => cmd_list(verbose, output),
    SnapshotCommand::Show { id, verbose, output } => cmd_show(&id, verbose, output),
    SnapshotCommand::Delete {
      ids,
      older_than,
      dry_run,
      force,
      output,
    } => cmd_delete(ids, older_than, dry_run, force, output),
    SnapshotCommand::Tag { id, name } => cmd_tag(&id, &name),
    SnapshotCommand::Untag { id, name } => cmd_untag(&id, name.as_deref()),
  }
}

fn cmd_list(verbose: bool, output: OutputFormat) -> Result<()> {
  let store = SnapshotStore::new(snapshots_dir());

  let mut snapshots = store.list()?;
  let current_id = store.current_id()?;

  snapshots.reverse();

  if output.is_json() {
    #[derive(Serialize)]
    struct ListOutput {
      snapshots: Vec<SnapshotListItem>,
      current: Option<String>,
    }

    #[derive(Serialize)]
    struct SnapshotListItem {
      id: String,
      created_at: u64,
      is_current: bool,
      #[serde(skip_serializing_if = "Option::is_none")]
      config_path: Option<String>,
      tags: Vec<String>,
      build_count: usize,
      bind_count: usize,
    }

    let items: Vec<SnapshotListItem> = snapshots
      .iter()
      .map(|s| SnapshotListItem {
        id: s.id.clone(),
        created_at: s.created_at,
        is_current: current_id.as_ref() == Some(&s.id),
        config_path: s.config_path.as_ref().map(|p| p.display().to_string()),
        tags: s.tags.clone(),
        build_count: s.build_count,
        bind_count: s.bind_count,
      })
      .collect();

    print_json(&ListOutput {
      snapshots: items,
      current: current_id,
    })?;
  } else {
    if snapshots.is_empty() {
      print_info("No snapshots found");
      return Ok(());
    }

    for snapshot in &snapshots {
      let is_current = current_id.as_ref() == Some(&snapshot.id);
      let current_marker = if is_current { " (current)" } else { "" };
      let timestamp = format_timestamp(snapshot.created_at);

      let tags_str = if snapshot.tags.is_empty() {
        String::new()
      } else {
        format!(" [{}]", snapshot.tags.join(", "))
      };

      if verbose {
        let config_str = snapshot
          .config_path
          .as_ref()
          .map(|p| format!(" config={}", p.display()))
          .unwrap_or_default();

        println!(
          "{}{}{} - {}{} (builds: {}, binds: {})",
          snapshot.id, current_marker, tags_str, timestamp, config_str, snapshot.build_count, snapshot.bind_count
        );
      } else {
        println!("{}{}{} - {}", snapshot.id, current_marker, tags_str, timestamp);
      }
    }

    print_info(&format!("{} snapshot(s) total", snapshots.len()));
  }

  Ok(())
}

fn cmd_show(id: &str, verbose: bool, output: OutputFormat) -> Result<()> {
  let store = SnapshotStore::new(snapshots_dir());

  let snapshot = store.load_snapshot(id)?;
  let current_id = store.current_id()?;
  let is_current = current_id.as_ref() == Some(&snapshot.id);

  let metadata = store.list()?.into_iter().find(|m| m.id == id);
  let tags = metadata.map(|m| m.tags).unwrap_or_default();

  if output.is_json() {
    #[derive(Serialize)]
    struct ShowOutput {
      id: String,
      created_at: u64,
      is_current: bool,
      config_path: Option<String>,
      tags: Vec<String>,
      builds: Vec<BuildInfo>,
      binds: Vec<BindInfo>,
    }

    #[derive(Serialize)]
    struct BuildInfo {
      id: String,
      hash: String,
    }

    #[derive(Serialize)]
    struct BindInfo {
      id: String,
      hash: String,
    }

    let builds: Vec<BuildInfo> = snapshot
      .manifest
      .builds
      .iter()
      .map(|(hash, build_def)| BuildInfo {
        id: build_def.id.clone().unwrap_or_else(|| "unnamed".to_string()),
        hash: hash.0.clone(),
      })
      .collect();

    let binds: Vec<BindInfo> = snapshot
      .manifest
      .bindings
      .iter()
      .map(|(hash, bind_def)| BindInfo {
        id: bind_def.id.clone().unwrap_or_else(|| "unnamed".to_string()),
        hash: hash.0.clone(),
      })
      .collect();

    print_json(&ShowOutput {
      id: snapshot.id.clone(),
      created_at: snapshot.created_at,
      is_current,
      config_path: snapshot.config_path.as_ref().map(|p| p.display().to_string()),
      tags,
      builds,
      binds,
    })?;
  } else {
    let current_marker = if is_current { " (current)" } else { "" };
    let timestamp = format_timestamp(snapshot.created_at);
    let tags_str = if tags.is_empty() {
      String::new()
    } else {
      format!(" [{}]", tags.join(", "))
    };

    println!("Snapshot: {}{}{}", snapshot.id, current_marker, tags_str);
    println!("Created:  {}", timestamp);
    if let Some(config) = &snapshot.config_path {
      println!("Config:   {}", config.display());
    }
    println!("Builds:   {}", snapshot.manifest.builds.len());
    println!("Binds:    {}", snapshot.manifest.bindings.len());

    if verbose {
      if !snapshot.manifest.builds.is_empty() {
        println!("\nBuilds:");
        for (hash, build_def) in &snapshot.manifest.builds {
          let id = build_def.id.as_deref().unwrap_or("unnamed");
          println!("  {} ({})", id, &hash.0[..12]);
        }
      }

      if !snapshot.manifest.bindings.is_empty() {
        println!("\nBinds:");
        for (hash, bind_def) in &snapshot.manifest.bindings {
          let id = bind_def.id.as_deref().unwrap_or("unnamed");
          println!("  {} ({})", id, &hash.0[..12]);
        }
      }
    }
  }

  Ok(())
}

fn cmd_delete(
  ids: Vec<String>,
  older_than: Option<Duration>,
  dry_run: bool,
  force: bool,
  output: OutputFormat,
) -> Result<()> {
  let store = SnapshotStore::new(snapshots_dir());

  let mut candidates: Vec<String> = ids;
  let current_id = store.current_id()?;

  if let Some(duration) = older_than {
    let now = SystemTime::now()
      .duration_since(UNIX_EPOCH)
      .map(|d| d.as_secs())
      .unwrap_or(0);
    let cutoff = now.saturating_sub(duration.as_secs());

    let snapshots = store.list()?;
    for snapshot in snapshots {
      if snapshot.created_at < cutoff && !candidates.contains(&snapshot.id) {
        candidates.push(snapshot.id);
      }
    }
  }

  if candidates.is_empty() {
    if output.is_json() {
      print_json(&DeleteResult {
        deleted: vec![],
        failed: vec![],
        skipped_current: None,
        dry_run,
      })?;
    } else {
      print_info("No snapshots to delete");
    }
    return Ok(());
  }

  let mut skipped_current: Option<String> = None;
  if let Some(ref current) = current_id
    && candidates.contains(current)
  {
    skipped_current = Some(current.clone());
    candidates.retain(|id| id != current);
  }

  if candidates.is_empty() {
    if output.is_json() {
      print_json(&DeleteResult {
        deleted: vec![],
        failed: vec![],
        skipped_current,
        dry_run,
      })?;
    } else {
      print_warning("Cannot delete the current snapshot. Use 'sys destroy' first.");
    }
    return Ok(());
  }

  if !output.is_json() {
    if dry_run {
      print_info("Dry run - the following snapshots would be deleted:");
    } else {
      println!("The following snapshots will be deleted:");
    }
    for id in &candidates {
      println!("  {}", id);
    }
    if let Some(ref current) = skipped_current {
      print_warning(&format!(
        "Skipping current snapshot: {} (use 'sys destroy' first)",
        current
      ));
    }
  }

  if !dry_run && !confirm(&format!("Delete {} snapshot(s)?", candidates.len()), force)? {
    if output.is_json() {
      print_json(&DeleteResult {
        deleted: vec![],
        failed: vec![],
        skipped_current,
        dry_run,
      })?;
    } else {
      print_info("Cancelled");
    }
    return Ok(());
  }

  if dry_run {
    if output.is_json() {
      print_json(&DeleteResult {
        deleted: candidates,
        failed: vec![],
        skipped_current,
        dry_run: true,
      })?;
    } else {
      print_info("Dry run - no changes made");
    }
    return Ok(());
  }

  let _lock = StoreLock::acquire(LockMode::Exclusive, "snapshot delete")?;

  let mut deleted = Vec::new();
  let mut failed = Vec::new();

  for id in candidates {
    debug!(snapshot_id = %id, "deleting snapshot");
    match store.delete_snapshot(&id) {
      Ok(()) => {
        info!(snapshot_id = %id, "deleted snapshot");
        deleted.push(id);
      }
      Err(e) => {
        debug!(snapshot_id = %id, error = %e, "failed to delete snapshot");
        failed.push(DeleteFailure {
          id,
          error: e.to_string(),
        });
      }
    }
  }

  if output.is_json() {
    print_json(&DeleteResult {
      deleted,
      failed,
      skipped_current,
      dry_run: false,
    })?;
  } else {
    if !deleted.is_empty() {
      print_success(&format!("Deleted {} snapshot(s)", deleted.len()));
    }
    for f in &failed {
      print_error(&format!("Failed to delete {}: {}", f.id, f.error));
    }
  }

  Ok(())
}

fn cmd_tag(id: &str, name: &str) -> Result<()> {
  let store = SnapshotStore::new(snapshots_dir());

  let _ = store.load_snapshot(id)?;

  let metadata = store.list()?.into_iter().find(|m| m.id == id);
  let mut tags = metadata.map(|m| m.tags).unwrap_or_default();

  if tags.contains(&name.to_string()) {
    bail!("Snapshot {} already has tag '{}'", id, name);
  }

  tags.push(name.to_string());

  let _lock = StoreLock::acquire(LockMode::Exclusive, "snapshot tag")?;
  store.set_snapshot_tags(id, tags)?;

  info!(snapshot_id = %id, tag = %name, "tagged snapshot");
  print_success(&format!("Tagged snapshot {} as '{}'", id, name));

  Ok(())
}

fn cmd_untag(id: &str, name: Option<&str>) -> Result<()> {
  let store = SnapshotStore::new(snapshots_dir());

  let _ = store.load_snapshot(id)?;

  let metadata = store.list()?.into_iter().find(|m| m.id == id);
  let mut tags = metadata.map(|m| m.tags).unwrap_or_default();

  let _lock = StoreLock::acquire(LockMode::Exclusive, "snapshot untag")?;

  match name {
    Some(tag_name) => {
      if !tags.contains(&tag_name.to_string()) {
        bail!("Snapshot {} does not have tag '{}'", id, tag_name);
      }
      tags.retain(|t| t != tag_name);
      store.set_snapshot_tags(id, tags)?;
      info!(snapshot_id = %id, tag = %tag_name, "removed tag from snapshot");
      print_success(&format!("Removed tag '{}' from snapshot {}", tag_name, id));
    }
    None => {
      if tags.is_empty() {
        print_info(&format!("Snapshot {} has no tags", id));
        return Ok(());
      }
      store.set_snapshot_tags(id, vec![])?;
      info!(snapshot_id = %id, "removed all tags from snapshot");
      print_success(&format!("Removed all tags from snapshot {}", id));
    }
  }

  Ok(())
}

fn format_timestamp(timestamp: u64) -> String {
  let datetime = UNIX_EPOCH + Duration::from_secs(timestamp);
  if let Ok(duration) = SystemTime::now().duration_since(datetime) {
    let secs = duration.as_secs();
    if secs < 60 {
      format!("{} seconds ago", secs)
    } else if secs < 3600 {
      format!("{} minutes ago", secs / 60)
    } else if secs < 86400 {
      format!("{} hours ago", secs / 3600)
    } else {
      format!("{} days ago", secs / 86400)
    }
  } else {
    format!("timestamp: {}", timestamp)
  }
}
