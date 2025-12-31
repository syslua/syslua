---
date: 2025-12-31T05:47:22-05:00
git_commit: aec790b57a3b6763613fcd37116279522a73f62b
branch: feat/gc-command
repository: syslua
topic: "sys gc Command Implementation Research"
tags: [research, codebase, gc, garbage-collection, locking, cli, store]
last_updated: 2025-12-31
---

## Ticket Synopsis

Implement a new `sys gc` CLI command that performs garbage collection of:

1. Orphaned builds (not referenced by any snapshot)
2. Incomplete builds (no `.syslua-complete` marker)
3. Leftover bind state directories
4. Orphaned inputs cache

Additionally, implement a file-based locking mechanism to prevent concurrent operations from corrupting state. The lock should be exclusive for `gc/apply/destroy` and shared for `plan/status`.

## Summary

Research confirms no existing GC or locking implementation exists. The codebase provides all necessary primitives:

- Snapshot traversal to identify referenced builds/binds
- Build marker detection (`.syslua-complete`)
- Inputs lock file graph for orphan detection
- Directory iteration and deletion patterns
- Immutability handling before deletion

**Recommended approach**: Mark-and-sweep GC with file-based advisory locking using `fs2` crate.

**Effort estimate**: Medium (1-2 days) for core implementation + tests.

## Detailed Findings

### CLI Command Structure

**Location**: `crates/cli/src/cmd/`

**Pattern to follow**:

- Commands enum in `main.rs` (lines 76-151)
- Command dispatch at lines 200-221
- Each command in `crates/cli/src/cmd/<name>.rs` with `pub fn cmd_<name>(...) -> Result<()>`
- Uses `tokio::runtime::Runtime::new()?.block_on(async_fn)`
- JSON output: `if json { print_json(&result) } else { human output }`

**Key output utilities** (`output.rs`):

- `format_bytes()` (lines 30-44) - B/KB/MB/GB with 1 decimal
- `print_success`, `print_error`, `print_warning`, `print_info`, `print_stat`
- `truncate_hash()` (line 25)

**Proposed CLI structure**:

```rust
/// Garbage collect unreferenced store objects
Gc {
    /// Show what would be deleted without making changes
    #[arg(long)]
    dry_run: bool,

    /// Output JSON instead of human-readable text
    #[arg(long)]
    json: bool,
}
```

### Build Store & Marker Files

**Locations**:

- `crates/lib/src/build/store.rs` - `build_dir_path()`, `build_exists_in_store()`
- `crates/lib/src/build/execute.rs` - marker file handling

**Build structure**: `<store>/build/<hash>/` where hash is 20-char truncated SHA-256

**Marker file** (`.syslua-complete`):

```json
{
  "version": 1,
  "status": "complete",
  "output_hash": "sha256..."
}
```

**Key functions**:

- `BUILD_COMPLETE_MARKER = ".syslua-complete"` (line 24)
- `BUILD_HASH_EXCLUSIONS = [".syslua-complete", "tmp"]` (lines 29-30)
- `write_build_complete_marker()` (lines 45-58)
- `read_build_marker()` (lines 64-76)
- `is_build_complete()` (lines 79-81)
- `verify_build_hash()` (lines 87-117)

**Incomplete build detection**: Directory exists but `.syslua-complete` is missing.

### Bind Store & State Files

**Locations**:

- `crates/lib/src/bind/store.rs` - `bind_dir_path()`
- `crates/lib/src/bind/state.rs` - state handling

**Bind structure**: `<store>/bind/<hash>/state.json`

**Key functions**:

- `save_bind_state()` (line 81)
- `load_bind_state()` (line 105)
- `remove_bind_state()` (line 147)
- `bind_state_exists()` (line 172)

### Snapshot System

**Locations**:

- `crates/lib/src/snapshot/storage.rs` - SnapshotStore
- `crates/lib/src/snapshot/types.rs` - Snapshot, SnapshotIndex

**Storage**: `{data_dir}/snapshots/` with `index.json` + `<id>.json` files

**Snapshot structure** (types.rs lines 16-66):

```rust
struct Snapshot {
    id: String,
    created_at: DateTime,
    config_path: PathBuf,
    manifest: Manifest,  // Contains builds + bindings BTreeMaps
}
```

**Key methods**:

- `load_index()` (lines 73-89)
- `load_snapshot()` (lines 124-138)
- `list()` (lines 212-218)
- `delete_snapshot()` (lines 220-240)

**Builds/binds references**: `manifest.builds: BTreeMap<ObjectHash, BuildDef>` and `manifest.bindings: BTreeMap<ObjectHash, BindDef>`

### Inputs Cache & Lock File

**Locations**:

- `crates/lib/src/inputs/store.rs` - InputStore
- `crates/lib/src/inputs/lock.rs` - lock file graph

**Cache structure**: `~/.cache/syslua/inputs/store/{name}-{hash[:8]}/`

**Lock file**: `config_dir()/syslua.lock` with dependency graph

**Key structures** (lock.rs):

- `LOCK_FILENAME = "syslua.lock"` (line 54)
- `LockFileV1` (lines 100-282) with nodes map and root node
- `remove_orphaned_nodes()` (lines 262-281) - DFS from root, removes unreachable

**Orphan detection algorithm**: Compare on-disk cache directories against nodes in lock graph.

### Platform & Paths

**Location**: `crates/lib/src/platform/`

**Key path functions** (paths.rs):

- `root_dir()` - `/syslua` (elevated) or `data_dir()` (non-elevated)
- `store_dir()` - `root_dir()/store` or `SYSLUA_STORE` env
- `cache_dir()` - `~/.cache/syslua` or `XDG_CACHE_HOME/syslua`
- `snapshots_dir()` - `root_dir()/snapshots`

**Immutability handling** (immutable.rs):

- `make_immutable()` / `make_mutable()` before deletion
- Unix: chmod 0444/0644
- macOS: chflags
- Windows: FILE_ATTRIBUTE_READONLY

**IMPORTANT**: Current paths are elevation-sensitive. GC needs explicit scope handling to work on both user and root stores.

### Cleanup/Deletion Patterns

**Pattern from destroy**:

1. `make_mutable()` before deletion
2. `fs::remove_dir_all()` with NotFound-as-success
3. Defer cleanup until after success confirmation

**Example** (bind/state.rs:147):

```rust
pub fn remove_bind_state(hash: &str) -> Result<()> {
    let path = bind_dir_path(hash);
    match fs::remove_dir_all(&path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
```

### Locking Design

**Current state**: NO locking exists. Gap confirmed.

**Architecture spec** (05-snapshots.md lines 251-306):

- `gc/apply/destroy` need exclusive lock
- `plan/status` need shared lock
- 3-phase algorithm: find roots, find unreferenced, remove

**Recommended implementation**:

1. Add `fs2` crate for cross-platform advisory locks
2. New module: `crates/lib/src/store_lock.rs`
3. Lock file: `<store>/.lock`

**Lock file format**:

```json
{
  "version": 1,
  "pid": 12345,
  "started_at_unix": 1735689600,
  "command": "sys gc",
  "store": "/path/to/store"
}
```

**Acquisition algorithm**:

1. Open `<store>/.lock` with create flag
2. `try_lock_exclusive()` for gc/apply/destroy, `try_lock_shared()` for plan/status
3. On success: truncate file, write metadata JSON
4. On WouldBlock: read existing metadata, return error with stale lock message

## Code References

### CLI Structure

- `crates/cli/src/main.rs:76-151` - Commands enum
- `crates/cli/src/main.rs:200-221` - Command dispatch
- `crates/cli/src/output.rs:30-44` - format_bytes()
- `crates/cli/src/cmd/destroy.rs` - Reference command pattern

### Build System

- `crates/lib/src/build/store.rs:14` - build_dir_path()
- `crates/lib/src/build/execute.rs:24` - BUILD_COMPLETE_MARKER
- `crates/lib/src/build/execute.rs:32-41` - BuildMarker struct
- `crates/lib/src/build/execute.rs:79-81` - is_build_complete()

### Bind System

- `crates/lib/src/bind/store.rs:14` - bind_dir_path()
- `crates/lib/src/bind/state.rs:147` - remove_bind_state()

### Snapshot System

- `crates/lib/src/snapshot/storage.rs:73-89` - load_index()
- `crates/lib/src/snapshot/storage.rs:212-218` - list()
- `crates/lib/src/snapshot/types.rs:16-66` - Snapshot struct

### Inputs System

- `crates/lib/src/inputs/lock.rs:262-281` - remove_orphaned_nodes()
- `crates/lib/src/inputs/store.rs:106-118` - compute_store_path()

### Platform

- `crates/lib/src/platform/paths.rs` - All path functions
- `crates/lib/src/platform/immutable.rs` - make_mutable()

### Status (disk usage reference)

- `crates/cli/src/cmd/status.rs:87-104` - dir_size()
- `crates/cli/src/cmd/status.rs:106-120` - calculate_store_usage()

## Architecture Insights

### GC Algorithm (Mark-and-Sweep)

**Phase 1: Mark (build live sets)**

1. Load snapshot index
2. For each snapshot, load and extract `manifest.builds` and `manifest.bindings` hashes
3. Build `LiveBuilds: HashSet<ObjectHash>` and `LiveBinds: HashSet<ObjectHash>`
4. Parse inputs lock file, extract all node paths into `LiveInputs`

**Phase 2: Sweep**

1. Enumerate `store/build/` directories
   - If no `.syslua-complete` → delete (incomplete)
   - If hash not in LiveBuilds → delete (orphaned)
2. Enumerate `store/bind/` directories
   - If hash not in LiveBinds → delete (orphaned)
3. Enumerate `~/.cache/syslua/inputs/store/` directories
   - If not in LiveInputs → delete (orphaned)

**Order of operations**:

1. Acquire exclusive lock
2. Delete incomplete builds first (always safe)
3. Delete orphaned builds
4. Delete orphaned binds
5. Delete orphaned inputs
6. Release lock

### Locking Architecture

**Lock modes**:

- `Exclusive`: gc, apply, destroy (blocks all other operations)
- `Shared`: plan, status (allows concurrent reads)

**Integration points**:

- `execute/apply.rs::apply()` → acquire exclusive before execution
- New gc entry point → acquire exclusive
- Plan/status → acquire shared

**Cross-platform**:

- `fs2::FileExt::try_lock_exclusive()` / `try_lock_shared()`
- Maps to `flock(LOCK_NB)` on Unix, `LockFileEx(FAIL_IMMEDIATELY)` on Windows

### Scope Handling

Current path functions are elevation-sensitive. For GC to work on both stores:

```rust
enum StoreScope {
    User,   // ~/.local/share/syslua/store
    Root,   // /syslua/store (elevated)
}
```

Iterate both scopes explicitly rather than relying on elevation detection.

## Historical Context (from thoughts/)

- `thoughts/tickets/feature_gc_command.md` - Original ticket with full requirements

## Related Research

- `docs/architecture/05-snapshots.md` - Locking specification (lines 251-306)
- `docs/architecture/03-store.md` - Store structure documentation

## Open Questions

1. **Snapshot scope**: Should GC respect per-scope snapshots, or assume snapshots are global? Current design assumes scope-specific snapshots.

2. **Lock file location with multiple scopes**: Should there be one lock per store scope, or a global lock? Recommendation: per-scope locks at `<store>/.lock`.

3. **Concurrent build protection**: Should incomplete builds younger than N minutes be skipped to avoid racing with in-progress builds? The locking mechanism should prevent this, but belt-and-suspenders approach might be warranted.

4. **fs2 dependency**: Need to add `fs2` crate. Alternative is implementing flock/LockFileEx manually (more code, more risk).

## Implementation Checklist

### New Files to Create

- [ ] `crates/lib/src/gc/mod.rs` - GC module with algorithm
- [ ] `crates/lib/src/store_lock.rs` - Locking mechanism
- [ ] `crates/cli/src/cmd/gc.rs` - CLI command

### Files to Modify

- [ ] `crates/lib/src/lib.rs` - Export gc and store_lock modules
- [ ] `crates/cli/src/main.rs` - Add Gc command variant
- [ ] `crates/cli/src/cmd/mod.rs` - Export gc module
- [ ] `crates/lib/Cargo.toml` - Add fs2 dependency
- [ ] `crates/lib/src/execute/apply.rs` - Integrate locking
- [ ] (Optional) `crates/lib/src/platform/paths.rs` - Add scope-explicit path functions

### Data Structures to Define

- [ ] `GcOptions` - dry_run flag
- [ ] `GcReport` - Results with counts, bytes, items
- [ ] `GcScopeReport` - Per-scope results
- [ ] `StoreLock` - RAII lock handle
- [ ] `LockMode` - Shared/Exclusive enum
- [ ] `StoreLockError` - Lock contention error with metadata
