# Plan: `sys rollback` Command

## Goal

Implement the `sys rollback` command to restore the system to a previous snapshot state.

## Problem

While snapshots are created during `sys apply`, there's no way to actually restore to a previous state. Users cannot recover from bad configurations.

## Architecture Reference

- [05-snapshots.md](../architecture/05-snapshots.md):149-226 - Rollback algorithm
- [08-apply-flow.md](../architecture/08-apply-flow.md):203-285 - Atomic rollback during apply

## Approach

1. Add `rollback` subcommand to CLI
2. Load target snapshot (previous by default, or by ID)
3. Compute diff between current and target snapshots
4. Execute destroy_actions for binds not in target
5. Execute apply_actions for binds in target but not current
6. Update current snapshot pointer

## CLI Interface

```bash
sys rollback                    # Rollback to previous snapshot
sys rollback <snapshot_id>      # Rollback to specific snapshot
sys rollback --list             # List available snapshots
sys rollback --dry-run          # Preview what would change
```

## Key Considerations

- Create pre-rollback snapshot for recovery?
- Handle case where target snapshot references builds no longer in store
- Parallel execution of independent rollback operations
- Transaction semantics if rollback partially fails

## Files to Create

| Path | Purpose |
|------|---------|
| `crates/cli/src/cmd/rollback.rs` | CLI command implementation |

## Files to Modify

| Path | Changes |
|------|---------|
| `crates/cli/src/cmd/mod.rs` | Add rollback module |
| `crates/cli/src/main.rs` | Add rollback subcommand |
| `crates/lib/src/snapshot/mod.rs` | Add `list_snapshots()`, `get_previous()` |
| `crates/lib/src/execute/mod.rs` | Add `rollback_to()` function |

## Success Criteria

1. `sys rollback` restores to the previous snapshot
2. `sys rollback <id>` restores to a specific snapshot
3. Dry-run mode shows what would change
4. Failed rollback attempts don't leave system in broken state
5. Integration tests verify rollback behavior

## Open Questions

- [ ] How many snapshots to retain by default?
- [ ] Should rollback be atomic (all-or-nothing)?
- [ ] How to handle rollback when builds have been GC'd?
