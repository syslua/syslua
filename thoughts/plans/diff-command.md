# `sys diff` Command Implementation Plan

## Overview

Implement the `sys diff` command to compare two snapshots and show what changed between them. Default output shows a summary of counts; `--verbose` shows granular details including the actual exec actions that would run.

## Current State Analysis

### What Exists

- `StateDiff` struct in `crates/lib/src/snapshot/diff.rs` with fields:
  - `builds_to_realize`, `builds_cached`
  - `binds_to_apply`, `binds_to_destroy`, `binds_unchanged`, `binds_to_update`
- `compute_diff(desired: &Manifest, current: Option<&Manifest>, store_path: &Path) -> StateDiff`
- `SnapshotStore` with `load_current()`, `load_snapshot(id)`, `list()` methods
- `SnapshotIndex` tracks all snapshots with chronological ordering
- CLI command pattern: clap enums with inline struct variants, `cmd_<name>` functions

### What's Missing

- `builds_orphaned` field in `StateDiff` to track builds removed between snapshots
- `crates/cli/src/cmd/diff.rs` - the command itself
- Human-readable diff formatting with action details
- Snapshot-to-snapshot comparison (current API compares manifest-to-manifest)

### Key Insight

The existing `compute_diff()` function takes `desired` and `current` manifests. For snapshot comparison A → B:

- Pass snapshot A's manifest as `current`
- Pass snapshot B's manifest as `desired`
- This gives us "what changed going from A to B"

## Desired End State

After implementation:

1. `sys diff` compares previous snapshot to current snapshot (what the last apply changed)
2. `sys diff <a> <b>` compares snapshot A to snapshot B chronologically
3. Default output shows summary counts
4. `--verbose` shows granular build/bind details with all exec actions

### Verification

```bash
# After at least 2 applies, these should work:
sys diff                    # Shows summary of last apply's changes
sys diff --verbose          # Shows detailed changes with actions
sys diff <id_a> <id_b>      # Compares two specific snapshots
```

## What We're NOT Doing

- Color output (future enhancement)
- Field-level diff within a bind (only showing add/remove/update at bind level)
- Interactive mode
- Diff against unapplied config (that's what `sys plan` does)

## Implementation Approach

1. Add `builds_orphaned` field to `StateDiff` in the library
2. Create the CLI command structure following existing patterns
3. Implement snapshot loading and comparison logic
4. Build formatting utilities for human-readable output
5. Wire up to main.rs and mod.rs

---

## Phase 1: Add `builds_orphaned` to StateDiff

### Overview

Add a new field to `StateDiff` to track builds that exist in the current manifest but not in the desired manifest. This enables the diff command to show removed builds.

### Changes Required

#### 1. Update `crates/lib/src/snapshot/diff.rs`

**File**: `crates/lib/src/snapshot/diff.rs`

Add field to `StateDiff` struct (after `builds_cached`):

```rust
/// Builds that are orphaned (in current, not in desired).
/// These builds are no longer referenced and can be garbage collected.
pub builds_orphaned: Vec<ObjectHash>,
```

Update `compute_diff()` function to populate this field. After the existing build diff logic (around line 101), add:

```rust
// Compute orphaned builds (in current but not in desired)
if let Some(current_manifest) = current {
    for hash in current_manifest.builds.keys() {
        if !desired.builds.contains_key(hash) {
            diff.builds_orphaned.push(hash.clone());
        }
    }
}
```

#### 2. Update tests in `crates/lib/src/snapshot/diff.rs`

Add test for orphaned builds:

```rust
#[test]
fn diff_orphaned_builds() {
    let temp_dir = TempDir::new().unwrap();

    // Current has two builds
    let mut current = Manifest::default();
    current
        .builds
        .insert(ObjectHash("keep_build".to_string()), make_build_def("pkg1"));
    current
        .builds
        .insert(ObjectHash("orphan_build".to_string()), make_build_def("pkg2"));

    // Desired only has one
    let mut desired = Manifest::default();
    desired
        .builds
        .insert(ObjectHash("keep_build".to_string()), make_build_def("pkg1"));

    let diff = compute_diff(&desired, Some(&current), temp_dir.path());

    assert_eq!(diff.builds_orphaned.len(), 1);
    assert!(diff.builds_orphaned.contains(&ObjectHash("orphan_build".to_string())));
}
```

### Success Criteria

#### Automated Verification

- [x] `cargo build -p syslua-lib` compiles without errors
- [x] `cargo test -p syslua-lib snapshot` passes
- [x] `cargo clippy -p syslua-lib --all-targets` passes

#### Manual Verification

- [x] N/A for this phase (library change only)

---

## Phase 2: Create Diff Command Module

### Overview

Create `crates/cli/src/cmd/diff.rs` with the core command implementation.

### Changes Required

#### 1. Create `crates/cli/src/cmd/diff.rs`

**File**: `crates/cli/src/cmd/diff.rs` (new file)

```rust
//! Implementation of the `sys diff` command.
//!
//! Compares two snapshots and shows what changed between them.

use std::collections::BTreeMap;

use anyhow::{Context, Result, bail};

use syslua_lib::action::types::{Action, ExecOpts};
use syslua_lib::bind::types::BindDef;
use syslua_lib::build::types::BuildDef;
use syslua_lib::manifest::types::Manifest;
use syslua_lib::platform::paths::{snapshots_dir, store_dir};
use syslua_lib::snapshot::{Snapshot, SnapshotStore, compute_diff, StateDiff};
use syslua_lib::util::hash::ObjectHash;

/// Execute the diff command.
pub fn cmd_diff(
    snapshot_a: Option<String>,
    snapshot_b: Option<String>,
    verbose: bool,
) -> Result<()> {
    let store = SnapshotStore::new(snapshots_dir());

    let (snap_a, snap_b) = load_snapshots_to_compare(&store, snapshot_a, snapshot_b)?;

    let store_path = store_dir();
    // compute_diff(desired, current, ...) - we want A → B, so A is "current", B is "desired"
    let diff = compute_diff(&snap_b.manifest, Some(&snap_a.manifest), &store_path);

    print_human_diff(&snap_a, &snap_b, &diff, verbose);

    Ok(())
}

/// Load the two snapshots to compare based on CLI arguments.
fn load_snapshots_to_compare(
    store: &SnapshotStore,
    snapshot_a: Option<String>,
    snapshot_b: Option<String>,
) -> Result<(Snapshot, Snapshot)> {
    match (snapshot_a, snapshot_b) {
        // Both specified: compare A → B
        (Some(a), Some(b)) => {
            let snap_a = store
                .load_snapshot(&a)
                .with_context(|| format!("Failed to load snapshot: {}", a))?;
            let snap_b = store
                .load_snapshot(&b)
                .with_context(|| format!("Failed to load snapshot: {}", b))?;
            Ok((snap_a, snap_b))
        }
        // Neither specified: compare previous → current
        (None, None) => {
            let index = store.load_index().context("Failed to load snapshot index")?;

            if index.snapshots.len() < 2 {
                bail!("Not enough snapshots to compare. Need at least 2 snapshots.");
            }

            let current = store
                .load_current()
                .context("Failed to load current snapshot")?
                .context("No current snapshot set")?;

            // Find previous snapshot (the one before current in chronological order)
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
        // Only one specified: invalid
        _ => {
            bail!("Must provide either no arguments (compare previous → current) or both snapshot IDs");
        }
    }
}

/// Print diff in human-readable format.
fn print_human_diff(snap_a: &Snapshot, snap_b: &Snapshot, diff: &StateDiff, verbose: bool) {
    println!("Comparing {} → {}", snap_a.id, snap_b.id);
    println!();

    if diff.is_empty() && diff.builds_cached.is_empty() && diff.binds_unchanged.is_empty() {
        println!("No changes.");
        return;
    }

    if verbose {
        print_verbose_diff(snap_a, snap_b, diff);
    } else {
        print_summary_diff(diff);
    }
}

/// Print summary counts only.
fn print_summary_diff(diff: &StateDiff) {
    let has_build_changes = !diff.builds_to_realize.is_empty() || !diff.builds_orphaned.is_empty();
    let has_bind_changes = !diff.binds_to_apply.is_empty()
        || !diff.binds_to_update.is_empty()
        || !diff.binds_to_destroy.is_empty();

    if has_build_changes || !diff.builds_cached.is_empty() {
        println!("Builds:");
        if !diff.builds_to_realize.is_empty() {
            println!("  + {} added", diff.builds_to_realize.len());
        }
        if !diff.builds_orphaned.is_empty() {
            println!("  - {} removed", diff.builds_orphaned.len());
        }
        println!();
    }

    if has_bind_changes
        || !diff.binds_unchanged.is_empty()
    {
        println!("Binds:");
        if !diff.binds_to_apply.is_empty() {
            println!("  + {} added", diff.binds_to_apply.len());
        }
        if !diff.binds_to_update.is_empty() {
            println!("  ~ {} updated", diff.binds_to_update.len());
        }
        if !diff.binds_to_destroy.is_empty() {
            println!("  - {} removed", diff.binds_to_destroy.len());
        }
        if !diff.binds_unchanged.is_empty() {
            println!("  = {} unchanged", diff.binds_unchanged.len());
        }
    }

    if !has_build_changes && !has_bind_changes {
        println!("No changes.");
    }
}

/// Print verbose diff with build/bind details and actions.
fn print_verbose_diff(snap_a: &Snapshot, snap_b: &Snapshot, diff: &StateDiff) {
    // Builds added (in B but not realized from A's perspective)
    if !diff.builds_to_realize.is_empty() {
        println!("Builds added:");
        for hash in &diff.builds_to_realize {
            if let Some(build) = snap_b.manifest.builds.get(hash) {
                print_build(hash, build, "+");
            }
        }
        println!();
    }

    // Builds removed (orphaned - in A but not in B)
    if !diff.builds_orphaned.is_empty() {
        println!("Builds removed:");
        for hash in &diff.builds_orphaned {
            if let Some(build) = snap_a.manifest.builds.get(hash) {
                print_build(hash, build, "-");
            }
        }
        println!();
    }

    // Binds added
    if !diff.binds_to_apply.is_empty() {
        println!("Binds added:");
        for hash in &diff.binds_to_apply {
            if let Some(bind) = snap_b.manifest.bindings.get(hash) {
                print_bind_added(hash, bind);
            }
        }
        println!();
    }

    // Binds updated
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

    // Binds removed
    if !diff.binds_to_destroy.is_empty() {
        println!("Binds removed:");
        for hash in &diff.binds_to_destroy {
            if let Some(bind) = snap_a.manifest.bindings.get(hash) {
                print_bind_removed(hash, bind);
            }
        }
        println!();
    }

    // Binds unchanged (just count, don't list all)
    if !diff.binds_unchanged.is_empty() {
        println!("Binds unchanged: {}", diff.binds_unchanged.len());
    }
}

/// Format a build for display.
fn print_build(hash: &ObjectHash, build: &BuildDef, prefix: &str) {
    let name = build.id.as_deref().unwrap_or("(unnamed)");
    println!("  {} {} ({})", prefix, name, &hash.0[..12]);
}

/// Format an added bind with create actions.
fn print_bind_added(hash: &ObjectHash, bind: &BindDef) {
    let name = bind.id.as_deref().unwrap_or("(unnamed)");
    println!("  + {} ({})", name, &hash.0[..12]);
    print_actions("create", &bind.create_actions);
}

/// Format an updated bind with update actions.
fn print_bind_updated(old_hash: &ObjectHash, new_hash: &ObjectHash, bind: &BindDef) {
    let name = bind.id.as_deref().unwrap_or("(unnamed)");
    println!(
        "  ~ {} ({} → {})",
        name,
        &old_hash.0[..12],
        &new_hash.0[..12]
    );
    if let Some(ref actions) = bind.update_actions {
        print_actions("update", actions);
    } else {
        println!("      (no update actions defined)");
    }
}

/// Format a removed bind with destroy actions.
fn print_bind_removed(hash: &ObjectHash, bind: &BindDef) {
    let name = bind.id.as_deref().unwrap_or("(unnamed)");
    println!("  - {} ({})", name, &hash.0[..12]);
    print_actions("destroy", &bind.destroy_actions);
}

/// Print a list of actions with proper formatting.
fn print_actions(label: &str, actions: &[Action]) {
    if actions.is_empty() {
        println!("      {}: (none)", label);
        return;
    }

    if actions.len() == 1 {
        // Single action: inline format
        println!("      {}: {}", label, format_action(&actions[0]));
    } else {
        // Multiple actions: numbered list
        println!("      {}:", label);
        for (i, action) in actions.iter().enumerate() {
            println!("        {}. {}", i + 1, format_action(action));
        }
    }
}

/// Format a single action for display.
fn format_action(action: &Action) -> String {
    match action {
        Action::Exec(opts) => format_exec(opts),
        Action::FetchUrl { url, sha256 } => {
            format!("fetch_url: {} (sha256: {}...)", url, &sha256[..12])
        }
    }
}

/// Format an exec action showing the command.
fn format_exec(opts: &ExecOpts) -> String {
    let mut cmd = opts.bin.clone();
    if let Some(ref args) = opts.args {
        for arg in args {
            // Quote args with spaces
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
    format!("exec: {}", cmd)
}
```

### Success Criteria

#### Automated Verification

- [x] `cargo build -p syslua-cli` compiles without errors
- [x] `cargo clippy -p syslua-cli --all-targets` passes

#### Manual Verification

- [x] N/A for this phase (command not wired up yet)

---

## Phase 3: Wire Up CLI

### Overview

Add the `Diff` command variant to `main.rs` and export from `mod.rs`.

### Changes Required

#### 1. Update `crates/cli/src/cmd/mod.rs`

**File**: `crates/cli/src/cmd/mod.rs`

Add diff module and export:

```rust
mod apply;
mod destroy;
mod diff;  // ADD THIS
mod info;
mod init;
mod plan;
mod status;
mod update;

pub use apply::cmd_apply;
pub use destroy::cmd_destroy;
pub use diff::cmd_diff;  // ADD THIS
pub use info::cmd_info;
pub use init::cmd_init;
pub use plan::cmd_plan;
pub use status::cmd_status;
pub use update::cmd_update;
```

#### 2. Update `crates/cli/src/main.rs`

**File**: `crates/cli/src/main.rs`

Add to the `Commands` enum (after `Destroy`):

```rust
/// Compare two snapshots and show differences
Diff {
    /// First snapshot ID (defaults to previous if not specified)
    #[arg(value_name = "SNAPSHOT_A")]
    snapshot_a: Option<String>,

    /// Second snapshot ID (defaults to current if not specified)
    #[arg(value_name = "SNAPSHOT_B")]
    snapshot_b: Option<String>,

    /// Show detailed changes with actions
    #[arg(short, long)]
    verbose: bool,
},
```

Add to the match in `main()`:

```rust
Commands::Diff {
    snapshot_a,
    snapshot_b,
    verbose,
} => cmd_diff(snapshot_a, snapshot_b, verbose),
```

Add import at top:

```rust
use cmd::{cmd_apply, cmd_destroy, cmd_diff, cmd_info, cmd_init, cmd_plan, cmd_status, cmd_update};
```

### Success Criteria

#### Automated Verification

- [x] `cargo build -p syslua-cli` compiles
- [x] `cargo clippy -p syslua-cli --all-targets` passes
- [x] `cargo test -p syslua-cli` passes

#### Manual Verification

- [x] `sys diff --help` shows the command with proper documentation
- [ ] `sys diff` with < 2 snapshots shows helpful error message
- [ ] `sys diff` with 2+ snapshots shows summary
- [ ] `sys diff --verbose` shows detailed output with actions
- [ ] `sys diff <id_a> <id_b>` compares specific snapshots

---

## Phase 4: Handle Edge Cases

### Overview

Ensure graceful handling of edge cases and improve error messages.

### Changes Required

#### 1. Add snapshot ID validation

In `load_snapshots_to_compare`, the current implementation will return appropriate errors from `load_snapshot` if IDs don't exist. No additional changes needed.

#### 2. Handle empty manifests

The current `print_human_diff` already checks for empty diff. Verify it handles:

- Both snapshots with empty manifests
- One empty, one populated

#### 3. Truncate long action commands

In `format_exec`, if the command line is very long (>100 chars), truncate with ellipsis:

```rust
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

    // Truncate very long commands
    let formatted = format!("exec: {}", cmd);
    if formatted.len() > 100 {
        format!("{}...", &formatted[..97])
    } else {
        formatted
    }
}
```

### Success Criteria

#### Automated Verification

- [x] `cargo test -p syslua-cli` passes
- [x] `cargo clippy` passes

#### Manual Verification

- [ ] Error message for non-existent snapshot ID is clear
- [ ] Empty snapshot comparison shows "No changes."
- [x] Long commands are truncated cleanly

---

## Testing Strategy

### Unit Tests

Add to `crates/cli/tests/` or within `diff.rs`:

1. `test_format_action_exec` - verify exec formatting
2. `test_format_action_fetch_url` - verify fetch_url formatting
3. `test_format_exec_with_spaces` - verify quoting args with spaces
4. `test_format_exec_truncation` - verify long command truncation

### Integration Tests

Add to `crates/cli/tests/integration/`:

1. `diff_tests.rs`:
   - `test_diff_no_snapshots` - error when < 2 snapshots
   - `test_diff_two_snapshots` - basic comparison works
   - `test_diff_specific_ids` - comparing specific snapshot IDs

### Manual Testing Steps

1. Run `sys apply` twice with different configs
2. Run `sys diff` and verify previous → current comparison
3. Run `sys diff --verbose` and verify actions are shown
4. Get snapshot IDs from `sys status` or files, run `sys diff <a> <b>`
5. Try invalid snapshot ID, verify error message

---

## Performance Considerations

- Snapshot loading is I/O bound (reading JSON files) - acceptable for CLI
- No need for async since operations are simple file reads
- Manifest comparison is O(n) where n is number of builds + binds

---

## References

- Architecture: `docs/architecture/05-snapshots.md`
- Existing snapshot code: `crates/lib/src/snapshot/`
- CLI patterns: `crates/cli/src/cmd/plan.rs`, `crates/cli/src/cmd/status.rs`
- Action types: `crates/lib/src/action/types.rs`
