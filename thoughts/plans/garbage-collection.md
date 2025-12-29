# Plan: Garbage Collection (`sys gc`)

## Goal

Implement garbage collection to remove unreferenced builds from the store, freeing disk space.

## Problem

Builds accumulate in the store over time as configurations change. Old versions are never cleaned up, wasting disk space.

## Architecture Reference

- [05-snapshots.md](../architecture/05-snapshots.md):228-317 - GC algorithm and locking
- [03-store.md](../architecture/03-store.md) - Store layout

## Approach

1. Collect all build hashes referenced by any snapshot
2. Scan `store/obj/` for all existing objects
3. Remove objects not in the referenced set
4. Handle immutability flags before deletion
5. Use exclusive locking to prevent concurrent operations

## CLI Interface

```bash
sys gc                              # Remove unreferenced objects
sys gc --dry-run                    # Show what would be removed
sys gc --delete-old-snapshots       # Also delete old snapshots
sys gc --keep <n>                   # Keep last N snapshots (default: 10)
```

## Key Considerations

- Store locking to prevent race conditions with concurrent apply
- Clear immutability flags before deletion
- Calculate and report freed disk space
- Handle partially-deleted objects gracefully
- Cross-platform immutability handling (chattr, chflags, ACLs)

## Files to Create

| Path | Purpose |
|------|---------|
| `crates/cli/src/cmd/gc.rs` | CLI command implementation |
| `crates/lib/src/gc/mod.rs` | GC logic and algorithms |

## Files to Modify

| Path | Changes |
|------|---------|
| `crates/cli/src/cmd/mod.rs` | Add gc module |
| `crates/cli/src/main.rs` | Add gc subcommand |
| `crates/lib/src/lib.rs` | Add gc module |

## Success Criteria

1. `sys gc` removes unreferenced objects
2. Objects referenced by any snapshot are preserved
3. Dry-run mode shows what would be removed without removing
4. Freed space is reported to user
5. Works correctly on all platforms (Linux, macOS, Windows)
6. Integration tests verify GC behavior

## Open Questions

- [ ] Default snapshot retention policy?
- [ ] Should GC run automatically after apply?
- [ ] How to handle in-progress builds during GC?
- [ ] Should there be a `--all` flag to remove everything?
