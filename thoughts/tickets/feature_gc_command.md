---
type: feature
priority: high
created: 2025-12-31T10:24:00Z
status: implemented
tags: [cli, gc, garbage-collection, storage, locking]
keywords:
  [gc, garbage collection, orphaned builds, destroyed binds, store cleanup, locking, .syslua-complete, inputs cache]
patterns: [cmd pattern, store iteration, snapshot traversal, file locking, build marker detection]
---

# FEATURE: Implement `sys gc` Command for Garbage Collection

## Description

Implement a new `sys gc` CLI command that performs garbage collection of orphaned builds, destroyed binds, incomplete builds, and orphaned inputs. This command will also introduce a file-based locking mechanism to prevent concurrent operations from corrupting state.

## Context

Currently, when `sys destroy` runs, it cleans up bind state directories but leaves build artifacts in place (reported as `builds_orphaned` count). Over time, these orphaned builds accumulate and consume disk space. There is no mechanism to reclaim this space.

Additionally, there is no locking mechanism to prevent concurrent `sys apply`, `sys destroy`, or `sys gc` operations from corrupting shared state. The architecture docs (`docs/architecture/05-snapshots.md` lines 251-306) define the locking requirements but implementation doesn't exist.

## Requirements

### Functional Requirements

#### GC Command

- New `sys gc` CLI command following existing cmd pattern (clap, async, `--json` support)
- Execute garbage collection by default (no confirmation prompt)
- `--dry-run` flag to preview what would be deleted without executing
- `--json` flag for machine-readable output (consistent with other commands)
- Works on both user store (`~/.local/share/syslua/store`) and root store (`/syslua/store`)
- Informational message if store doesn't exist yet (not an error)

#### Items to Clean

1. **Orphaned builds**: Builds in `<store>/build/<hash>/` not referenced by ANY snapshot
2. **Incomplete builds**: Build directories without `.syslua-complete` marker file
3. **Leftover bind states**: Orphaned `<store>/bind/<hash>/` directories not in current snapshot
4. **Orphaned inputs**: Cached inputs in `~/.cache/syslua/inputs/` not referenced in `syslua.lock` graph

#### Corrupted Items Handling

- Corrupted `.syslua-complete` files (invalid JSON, hash mismatch): Report for manual intervention, do NOT auto-delete
- Malformed `state.json` in bind directories: Report for manual intervention, do NOT auto-delete

#### Output Requirements

- Summary statistics: count of items deleted per category
- Disk space reclaimed (formatted human-readable, e.g., "15.2 MB")
- Per-item listing of what was deleted
- Report on snapshots consuming space with message: "N snapshots using X MB - use `sys snapshot gc` to clean (future feature)"
- All output respects `--json` flag for machine parsing

#### Locking Mechanism

- File-based lock in store directory (e.g., `<store>/.lock`)
- **Exclusive lock** required for: `sys apply`, `sys destroy`, `sys gc`
- **Shared lock** required for: `sys plan`, `sys status`
- If lock cannot be acquired: fail immediately with helpful error message
- Error message should mention stale lock possibility and how to manually remove lock file
- Lock should include metadata (PID, timestamp, command) for debugging stale locks

### Non-Functional Requirements

- Cross-platform: Works on macOS, Linux, and Windows
- Performance: Should handle stores with thousands of builds efficiently
- Safety: Never delete items that might be in use (locking protects this)
- Idempotent: Running gc multiple times should be safe

## Current State

- `sys destroy` leaves orphaned builds in store (by design, for GC to handle)
- No `sys gc` command exists
- No inter-process locking mechanism exists
- Orphaned builds accumulate indefinitely
- `remove_orphaned_nodes()` in `lock.rs` cleans lock file graph but NOT disk

## Desired State

- `sys gc` command cleans all orphaned/incomplete items
- Locking mechanism prevents concurrent state corruption
- Users can reclaim disk space on demand
- Clear reporting of what was cleaned and space reclaimed

## Research Context

### Keywords to Search

- `gc` - potential existing gc code or references
- `.syslua-complete` - build marker file for detecting incomplete builds
- `syslua.lock` - inputs lock file for orphan detection
- `store` - store directory handling patterns
- `snapshot` - snapshot traversal for build reference counting
- `build_hash` - how builds are identified
- `remove_orphaned_nodes` - existing orphan detection in inputs

### Patterns to Investigate

- `crates/cli/src/cmd/*.rs` - existing command structure to follow
- `crates/lib/src/build/store.rs` - build store operations
- `crates/lib/src/bind/store.rs` - bind store operations
- `crates/lib/src/snapshot/storage.rs` - snapshot listing and traversal
- `crates/lib/src/inputs/lock.rs` - orphan node detection for inputs
- `crates/lib/src/inputs/store.rs` - inputs cache storage
- `docs/architecture/05-snapshots.md` - locking design spec (lines 251-306)
- `crates/lib/src/build/execute.rs` - build marker file handling

### Key Decisions Made

- Execute by default, `--dry-run` for preview (not dry-run by default)
- No confirmation prompt (not a destructive operation on active data)
- Fail immediately if locked (don't wait/retry)
- Report corrupted items for manual intervention (don't auto-delete)
- Locking implemented alongside GC (not separate ticket)
- No filtering options for v1 (`--older-than`, `--builds-only`, etc.)
- No hooks (pre-gc, post-gc lua callbacks)
- Clean inputs cache (`~/.cache/syslua/inputs/`) as part of gc

## Success Criteria

### Automated Verification

- [x] `cargo test -p syslua-cli` passes with new gc tests
- [x] `cargo test -p syslua-lib` passes with new locking tests
- [x] `cargo clippy --all-targets --all-features` has no warnings
- [x] `cargo fmt --check` passes
- [x] Integration tests cover: basic gc, dry-run, empty store (concurrent lock detection deferred)

### Manual Verification

- [ ] `sys gc` cleans orphaned builds after `sys destroy`
- [ ] `sys gc --dry-run` shows what would be deleted without deleting
- [ ] `sys gc --json` produces valid JSON output
- [ ] Running `sys gc` while `sys apply` is running fails with lock error
- [ ] Corrupted items are reported but not deleted
- [ ] Disk space reclaimed is accurate
- [ ] Works on both user and root store paths
- [ ] Informational message shown when store doesn't exist

## Out of Scope

- Snapshot cleanup (future `sys snapshot gc` command)
- Filtering options (`--older-than`, `--builds-only`, `--inputs-only`)
- Pre/post gc hooks in lua config
- `--all` aggressive cleanup mode
- Confirmation prompts
- Auto-deletion of corrupted items
- Waiting/retrying on locked store

## Related Information

- Architecture docs: `docs/architecture/05-snapshots.md` (locking spec)
- Architecture docs: `docs/architecture/03-store.md` (store structure)
- Build marker: `.syslua-complete` JSON file with `{version, status, output_hash}`
- Inputs lock: `syslua.lock` with dependency graph

## Notes

- The locking mechanism introduced here should be used by `sys apply` and `sys destroy` as well - this is part of the implementation scope
- Consider adding lock info to `sys status` output (who holds lock, when acquired)
- Future enhancement: `sys gc --unlock` to force-remove stale lock after confirmation
