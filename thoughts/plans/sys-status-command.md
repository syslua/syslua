# `sys status` Command Implementation Plan

## Overview

Implement the `sys status` command to display the current system state managed by syslua, including snapshot information, managed builds/binds counts, and store disk usage.

## Current State Analysis

### What Exists:
- Snapshot infrastructure in `crates/lib/src/snapshot/`:
  - `SnapshotStore::load_current()` returns `Option<Snapshot>` (storage.rs)
  - `Snapshot` struct with `id`, `created_at`, `config_path`, `manifest` (types.rs)
  - `SnapshotMetadata` with `build_count`, `bind_count` for quick summaries (types.rs)
- CLI pattern established in `crates/cli/src/`:
  - Commands enum in `main.rs` using clap derive macros
  - Each command in `cmd/{name}.rs` with `pub fn cmd_{name}()` function
  - `info.rs` is simplest pattern (just prints, no async)
  - `apply.rs` shows summary output format with counts
- Store paths via `platform::paths::store_dir()` and build/bind store modules
- Test infrastructure in `crates/cli/tests/` with `TestEnv` helper

### Key Discoveries:
- `Manifest` contains `builds: BTreeMap<ObjectHash, BuildDef>` and `bindings: BTreeMap<ObjectHash, BindDef>` (manifest/types.rs)
- Store paths: `store_dir()/build/<hash>/` and `store_dir()/bind/<hash>/` 
- `OBJ_HASH_PREFIX_LEN = 20` chars for hash display (consts.rs)
- Test pattern uses `assert_cmd` and `predicates` crates (cli_smoke.rs)

## Desired End State

Running `sys status` displays:
```
Current snapshot: abc123def456...
Created: 2024-12-29 10:30:00

Builds: 5
Binds: 8

Store usage: 156 MB
```

With `--verbose`:
```
Current snapshot: abc123def456...
Created: 2024-12-29 10:30:00

Builds: 5
  ripgrep-abc123def456...
  fd-def456ghi789...
  ...

Binds: 8
  /usr/local/bin/rg -> store/build/abc123.../bin/rg
  ~/.gitconfig -> store/bind/xyz789.../content
  ...

Store usage: 156 MB
```

When no snapshot exists:
```
No snapshot found. Run 'sys apply' to create one.
```

### Verification:
- `sys status` returns exit code 0 and prints snapshot summary
- `sys status --verbose` lists all builds and binds
- `sys status` with no snapshot prints helpful message and exits 0
- Store usage calculated from manifest entries (not full directory walk)

## What We're NOT Doing

- JSON output (deferred to future)
- Bind verification (checking symlinks still exist)
- Colored output (deferred to future styling pass)
- Integration with `sys info` (keeping commands separate)
- Full store directory walk for size (using manifest-based calculation)

## Implementation Approach

Follow existing CLI patterns. Create `status.rs` mirroring `info.rs` simplicity but with snapshot loading like `apply.rs`. Calculate store size by iterating manifest entries and summing filesystem metadata.

---

## Phase 1: Core Status Command

### Overview
Create the basic `sys status` command that loads and displays the current snapshot summary.

### Changes Required:

#### 1. Create Status Command Module
**File**: `crates/cli/src/cmd/status.rs` (new)
**Changes**: Create new command module

```rust
use std::process::ExitCode;
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

    ExitCode::SUCCESS
}
```

#### 2. Export Status Module
**File**: `crates/cli/src/cmd/mod.rs`
**Changes**: Add status module export

```rust
// Add to existing exports:
mod status;
pub use status::cmd_status;
```

#### 3. Wire Up CLI Command
**File**: `crates/cli/src/main.rs`
**Changes**: Add Status variant to Commands enum and match arm

```rust
// In Commands enum, add:
/// Show current system state
Status {
    /// Show all builds and binds
    #[arg(short, long)]
    verbose: bool,
},

// In main() match, add:
Commands::Status { verbose } => cmd_status(verbose),
```

### Success Criteria:

#### Automated Verification:
- [x] `cargo build -p syslua-cli` compiles without errors
- [x] `cargo clippy -p syslua-cli --all-targets` passes
- [x] `cargo fmt --check` passes
- [x] `sys status --help` shows command help

#### Manual Verification:
- [x] `sys status` with no snapshot prints "No snapshot found" message
- [x] After `sys apply`, `sys status` shows snapshot id and counts

---

## Phase 2: Verbose Mode

### Overview
Add `--verbose` flag to list all managed builds and binds with their details.

### Changes Required:

#### 1. Extend Status Command
**File**: `crates/cli/src/cmd/status.rs`
**Changes**: Add verbose output logic after the counts

```rust
// After printing counts, add:
if verbose {
    println!();
    println!("Builds:");
    for (hash, build) in &snapshot.manifest.builds {
        // Format: name-hash (truncated)
        let short_hash = &hash.as_str()[..20.min(hash.as_str().len())];
        println!("  {}-{}", build.name, short_hash);
    }
    
    println!();
    println!("Binds:");
    for (_hash, bind) in &snapshot.manifest.bindings {
        // Show target path and what it points to
        println!("  {} -> {}", bind.target.display(), bind.source.display());
    }
}
```

Note: Exact field names (`build.name`, `bind.target`, `bind.source`) need verification from `BuildDef` and `BindDef` types.

### Success Criteria:

#### Automated Verification:
- [x] `cargo build -p syslua-cli` compiles without errors
- [x] `cargo clippy -p syslua-cli --all-targets` passes

#### Manual Verification:
- [x] `sys status --verbose` lists all builds with name-hash format
- [x] `sys status --verbose` lists all binds with target -> source format
- [x] `sys status` (without verbose) still shows only counts

---

## Phase 3: Size Calculation

### Overview
Calculate and display store disk usage by summing sizes of managed build/bind directories from the manifest.

### Changes Required:

#### 1. Add Size Calculation Function
**File**: `crates/cli/src/cmd/status.rs`
**Changes**: Add helper function and call it

```rust
use std::path::Path;
use syslua_lib::platform::paths::store_dir;
use syslua_lib::build::store::build_dir_path;
use syslua_lib::bind::store::bind_dir_path;

/// Calculate total size of a directory recursively
fn dir_size(path: &Path) -> u64 {
    if !path.exists() {
        return 0;
    }
    
    let mut size = 0;
    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                size += entry.metadata().map(|m| m.len()).unwrap_or(0);
            } else if path.is_dir() {
                size += dir_size(&path);
            }
        }
    }
    size
}

/// Calculate store usage from manifest entries
fn calculate_store_usage(manifest: &Manifest, store_path: &Path) -> u64 {
    let mut total = 0;
    
    for hash in manifest.builds.keys() {
        let build_path = build_dir_path(hash, store_path);
        total += dir_size(&build_path);
    }
    
    for hash in manifest.bindings.keys() {
        let bind_path = bind_dir_path(hash, store_path);
        total += dir_size(&bind_path);
    }
    
    total
}

/// Format bytes as human-readable string
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
```

Then in `cmd_status()`, after verbose output:

```rust
println!();
let usage = calculate_store_usage(&snapshot.manifest, &store_dir());
println!("Store usage: {}", format_bytes(usage));
```

### Success Criteria:

#### Automated Verification:
- [x] `cargo build -p syslua-cli` compiles without errors
- [x] `cargo clippy -p syslua-cli --all-targets` passes
- [x] `cargo test -p syslua-cli` passes

#### Manual Verification:
- [x] `sys status` shows "Store usage: X MB" line
- [x] Size is reasonable (matches rough `du -sh` on store directory)
- [x] Size is 0 bytes when no builds/binds exist

---

## Phase 4: Edge Cases & Tests

### Overview
Handle edge cases and add comprehensive tests.

### Changes Required:

#### 1. Add Smoke Test
**File**: `crates/cli/tests/cli_smoke.rs`
**Changes**: Add status command tests

```rust
#[test]
fn status_no_snapshot() {
    let env = TestEnv::empty();
    env.cmd()
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("No snapshot found"));
}

#[test]
fn status_after_apply() {
    let env = TestEnv::with_config(r#"
        sys.build {
            name = "test-build",
            build = function(ctx) end
        }
    "#);
    
    env.cmd().arg("apply").assert().success();
    
    env.cmd()
        .arg("status")
        .assert()
        .success()
        .stdout(predicate::str::contains("Current snapshot:"))
        .stdout(predicate::str::contains("Builds: 1"))
        .stdout(predicate::str::contains("Binds: 0"));
}

#[test]
fn status_verbose() {
    let env = TestEnv::with_config(r#"
        sys.build {
            name = "test-build",
            build = function(ctx) end
        }
    "#);
    
    env.cmd().arg("apply").assert().success();
    
    env.cmd()
        .arg("status")
        .arg("--verbose")
        .assert()
        .success()
        .stdout(predicate::str::contains("test-build-"));
}

#[test]
fn status_help() {
    TestEnv::empty()
        .cmd()
        .arg("status")
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("Show current system state"));
}
```

#### 2. Handle Corrupted Snapshot Gracefully
**File**: `crates/cli/src/cmd/status.rs`
**Changes**: Already handled in Phase 1 with error case returning FAILURE

### Success Criteria:

#### Automated Verification:
- [x] `cargo test -p syslua-cli status` - all status tests pass
- [x] `cargo test -p syslua-cli` - full test suite passes
- [x] `cargo clippy --all-targets --all-features` passes

#### Manual Verification:
- [x] Command handles missing snapshot gracefully
- [x] Command handles corrupted snapshot with clear error message

---

## Testing Strategy

### Unit Tests:
- `format_bytes()` with various sizes (0, KB, MB, GB boundaries)
- `dir_size()` with empty dir, nested files, missing path

### Integration Tests:
- Status with no snapshot
- Status after apply with builds
- Status after apply with binds
- Status verbose mode
- Status help text

### Manual Testing Steps:
1. Fresh install: `sys status` shows "No snapshot found"
2. Run `sys apply` with a simple config
3. `sys status` shows correct snapshot id and counts
4. `sys status --verbose` lists all items
5. Verify store usage is reasonable

## Performance Considerations

- Size calculation is O(n * m) where n = manifest entries and m = average files per entry
- For typical configs (10-50 items), this is sub-second
- Could add `--no-size` flag in future if users have very large stores

## Migration Notes

None - this is a new command with no existing state to migrate.

## References

- Original plan: `docs/plans/status-command.md`
- Architecture: `docs/architecture/05-snapshots.md`, `docs/architecture/03-store.md`
- Similar command: `crates/cli/src/cmd/info.rs` (simplest pattern)
- Output pattern: `crates/cli/src/cmd/apply.rs` (summary format)
- Test pattern: `crates/cli/tests/cli_smoke.rs`
