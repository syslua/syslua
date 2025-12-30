//! Diff command implementation.
//!
//! Compares two snapshots and displays added/removed/updated builds and binds.

use anyhow::{Context, Result, bail};
use owo_colors::{OwoColorize, Stream};

use syslua_lib::action::Action;
use syslua_lib::action::actions::exec::ExecOpts;
use syslua_lib::bind::BindDef;
use syslua_lib::build::BuildDef;
use syslua_lib::platform::paths::{snapshots_dir, store_dir};
use syslua_lib::snapshot::{Snapshot, SnapshotStore, StateDiff, compute_diff};
use syslua_lib::util::hash::ObjectHash;

use crate::output::{print_json, symbols, truncate_hash};

pub fn cmd_diff(snapshot_a: Option<String>, snapshot_b: Option<String>, verbose: bool, json: bool) -> Result<()> {
  let store = SnapshotStore::new(snapshots_dir());

  let (snap_a, snap_b) = load_snapshots_to_compare(&store, snapshot_a, snapshot_b)?;

  let store_path = store_dir();
  let diff = compute_diff(&snap_b.manifest, Some(&snap_a.manifest), &store_path);

  if json {
    let diff_output = serde_json::json!({
      "snapshot_a": snap_a,
      "snapshot_b": snap_b,
      "diff": diff
    });
    print_json(&diff_output)?;
  } else {
    print_human_diff(&snap_a, &snap_b, &diff, verbose);
  }

  Ok(())
}

fn load_snapshots_to_compare(
  store: &SnapshotStore,
  snapshot_a: Option<String>,
  snapshot_b: Option<String>,
) -> Result<(Snapshot, Snapshot)> {
  match (snapshot_a, snapshot_b) {
    (Some(a), Some(b)) => {
      let snap_a = store
        .load_snapshot(&a)
        .with_context(|| format!("Failed to load snapshot: {}", a))?;
      let snap_b = store
        .load_snapshot(&b)
        .with_context(|| format!("Failed to load snapshot: {}", b))?;
      Ok((snap_a, snap_b))
    }
    (None, None) => {
      let index = store.load_index().context("Failed to load snapshot index")?;

      if index.snapshots.len() < 2 {
        bail!("Not enough snapshots to compare. Need at least 2 snapshots.");
      }

      let current = store
        .load_current()
        .context("Failed to load current snapshot")?
        .context("No current snapshot set")?;

      let current_idx = index
        .snapshots
        .iter()
        .position(|s| s.id == current.id)
        .context("Current snapshot not found in index")?;

      if current_idx == 0 {
        bail!("No previous snapshot to compare to. Current is the oldest snapshot.");
      }

      let prev_id = &index.snapshots[current_idx - 1].id;
      let prev = store
        .load_snapshot(prev_id)
        .with_context(|| format!("Failed to load previous snapshot: {}", prev_id))?;

      Ok((prev, current))
    }
    _ => {
      bail!("Must provide either no arguments (compare previous → current) or both snapshot IDs");
    }
  }
}

fn print_human_diff(snap_a: &Snapshot, snap_b: &Snapshot, diff: &StateDiff, verbose: bool) {
  println!("Comparing {} → {}", snap_a.id, snap_b.id);
  println!();

  if diff.is_empty()
    && diff.builds_cached.is_empty()
    && diff.binds_unchanged.is_empty()
    && diff.builds_orphaned.is_empty()
  {
    println!("No changes.");
    return;
  }

  if verbose {
    print_verbose_diff(snap_a, snap_b, diff);
  } else {
    print_summary_diff(diff);
  }
}

fn print_summary_diff(diff: &StateDiff) {
  let has_build_changes = !diff.builds_to_realize.is_empty() || !diff.builds_orphaned.is_empty();
  let has_bind_changes =
    !diff.binds_to_apply.is_empty() || !diff.binds_to_update.is_empty() || !diff.binds_to_destroy.is_empty();

  if has_build_changes || !diff.builds_cached.is_empty() {
    println!("Builds:");
    if !diff.builds_to_realize.is_empty() {
      println!(
        "  {} {} added",
        symbols::PLUS.if_supports_color(Stream::Stdout, |s| s.green()),
        diff.builds_to_realize.len()
      );
    }
    if !diff.builds_orphaned.is_empty() {
      println!(
        "  {} {} removed",
        symbols::MINUS.if_supports_color(Stream::Stdout, |s| s.red()),
        diff.builds_orphaned.len()
      );
    }
    println!();
  }

  if has_bind_changes || !diff.binds_unchanged.is_empty() {
    println!("Binds:");
    if !diff.binds_to_apply.is_empty() {
      println!(
        "  {} {} added",
        symbols::PLUS.if_supports_color(Stream::Stdout, |s| s.green()),
        diff.binds_to_apply.len()
      );
    }
    if !diff.binds_to_update.is_empty() {
      println!(
        "  {} {} updated",
        symbols::TILDE.if_supports_color(Stream::Stdout, |s| s.yellow()),
        diff.binds_to_update.len()
      );
    }
    if !diff.binds_to_destroy.is_empty() {
      println!(
        "  {} {} removed",
        symbols::MINUS.if_supports_color(Stream::Stdout, |s| s.red()),
        diff.binds_to_destroy.len()
      );
    }
    if !diff.binds_unchanged.is_empty() {
      println!("  = {} unchanged", diff.binds_unchanged.len());
    }
  }

  if !has_build_changes && !has_bind_changes {
    println!("No changes.");
  }
}

fn print_verbose_diff(snap_a: &Snapshot, snap_b: &Snapshot, diff: &StateDiff) {
  if !diff.builds_to_realize.is_empty() {
    println!("Builds added:");
    for hash in &diff.builds_to_realize {
      if let Some(build) = snap_b.manifest.builds.get(hash) {
        print_build(hash, build, "+");
      }
    }
    println!();
  }

  if !diff.builds_orphaned.is_empty() {
    println!("Builds removed:");
    for hash in &diff.builds_orphaned {
      if let Some(build) = snap_a.manifest.builds.get(hash) {
        print_build(hash, build, "-");
      }
    }
    println!();
  }

  if !diff.binds_to_apply.is_empty() {
    println!("Binds added:");
    for hash in &diff.binds_to_apply {
      if let Some(bind) = snap_b.manifest.bindings.get(hash) {
        print_bind_added(hash, bind);
      }
    }
    println!();
  }

  if !diff.binds_to_update.is_empty() {
    println!("Binds updated:");
    for (old_hash, new_hash) in &diff.binds_to_update {
      let old_bind = snap_a.manifest.bindings.get(old_hash);
      let new_bind = snap_b.manifest.bindings.get(new_hash);
      if let (Some(_old), Some(new)) = (old_bind, new_bind) {
        print_bind_updated(old_hash, new_hash, new);
      }
    }
    println!();
  }

  if !diff.binds_to_destroy.is_empty() {
    println!("Binds removed:");
    for hash in &diff.binds_to_destroy {
      if let Some(bind) = snap_a.manifest.bindings.get(hash) {
        print_bind_removed(hash, bind);
      }
    }
    println!();
  }

  if !diff.binds_unchanged.is_empty() {
    println!("Binds unchanged: {}", diff.binds_unchanged.len());
  }
}

fn print_build(hash: &ObjectHash, build: &BuildDef, prefix: &str) {
  let name = build.id.as_deref().unwrap_or("(unnamed)");
  let short_hash = truncate_hash(&hash.0);
  let colored_prefix = if prefix == "+" {
    prefix.if_supports_color(Stream::Stdout, |s| s.green()).to_string()
  } else {
    prefix.if_supports_color(Stream::Stdout, |s| s.red()).to_string()
  };
  println!("  {} {} ({})", colored_prefix, name, short_hash);
}

fn print_bind_added(hash: &ObjectHash, bind: &BindDef) {
  let name = bind.id.as_deref().unwrap_or("(unnamed)");
  let short_hash = truncate_hash(&hash.0);
  println!(
    "  {} {} ({})",
    symbols::PLUS.if_supports_color(Stream::Stdout, |s| s.green()),
    name,
    short_hash
  );
  print_actions("create", &bind.create_actions);
}

fn print_bind_updated(old_hash: &ObjectHash, new_hash: &ObjectHash, bind: &BindDef) {
  let name = bind.id.as_deref().unwrap_or("(unnamed)");
  let old_short = truncate_hash(&old_hash.0);
  let new_short = truncate_hash(&new_hash.0);
  println!(
    "  {} {} ({} {} {})",
    symbols::TILDE.if_supports_color(Stream::Stdout, |s| s.yellow()),
    name,
    old_short,
    symbols::ARROW,
    new_short
  );
  if let Some(ref actions) = bind.update_actions {
    print_actions("update", actions);
  } else {
    println!("      (no update actions defined)");
  }
}

fn print_bind_removed(hash: &ObjectHash, bind: &BindDef) {
  let name = bind.id.as_deref().unwrap_or("(unnamed)");
  let short_hash = truncate_hash(&hash.0);
  println!(
    "  {} {} ({})",
    symbols::MINUS.if_supports_color(Stream::Stdout, |s| s.red()),
    name,
    short_hash
  );
  print_actions("destroy", &bind.destroy_actions);
}

fn print_actions(label: &str, actions: &[Action]) {
  if actions.is_empty() {
    println!("      {}: (none)", label);
    return;
  }

  if actions.len() == 1 {
    println!("      {}: {}", label, format_action(&actions[0]));
  } else {
    println!("      {}:", label);
    for (i, action) in actions.iter().enumerate() {
      println!("        {}. {}", i + 1, format_action(action));
    }
  }
}

fn format_action(action: &Action) -> String {
  match action {
    Action::Exec(opts) => format_exec(opts),
    Action::FetchUrl { url, sha256 } => {
      let short_sha = truncate_hash(sha256);
      format!("fetch_url: {} (sha256: {}...)", url, short_sha)
    }
  }
}

fn format_exec(opts: &ExecOpts) -> String {
  let mut cmd = opts.bin.clone();
  if let Some(ref args) = opts.args {
    for arg in args {
      if arg.contains(' ') {
        cmd.push_str(&format!(" \"{}\"", arg));
      } else {
        cmd.push_str(&format!(" {}", arg));
      }
    }
  }
  if let Some(ref cwd) = opts.cwd {
    cmd.push_str(&format!(" (cwd: {})", cwd));
  }

  let formatted = format!("exec: {}", cmd);
  if formatted.len() > 100 {
    format!("{}...", &formatted[..97])
  } else {
    formatted
  }
}
