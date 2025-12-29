# Plan: Store Improvements

## Goal

Implement additional store features described in the architecture: drv files, hardlink deduplication, and binary cache support.

## Problem

The current store implementation works but lacks some features for debugging, deduplication, and remote caching.

## Architecture Reference

- [03-store.md](../architecture/03-store.md) - Full store layout

## Features

### 1. Derivation Files (`drv/`)

Store serialized build descriptions for debugging and reproducibility:

```
store/
├── drv/<build-hash>.drv    # JSON serialized BuildDef
└── drv-out/<build-hash>    # Maps build hash -> output hash
```

Currently we use `.syslua-complete` marker. drv/drv-out provides:
- Debug inspection of what was requested
- Cache index by build hash
- Reproducibility verification

### 2. Hardlink Deduplication

When user store and system store are on same filesystem:
- User packages can hardlink to system store objects
- Zero additional disk space for shared packages

```rust
fn link_or_copy(src: &Path, dst: &Path) -> Result<()> {
    if same_filesystem(src, dst) {
        std::fs::hard_link(src, dst)?;
    } else {
        copy_dir_all(src, dst)?;
    }
    Ok(())
}
```

### 3. Binary Cache (Future)

Query remote cache by output hash before building:

```
Cache lookup order:
1. Local store - check if output hash exists
2. Binary cache - query by output hash
3. Build from source
```

## Files to Modify

| Path | Changes |
|------|---------|
| `crates/lib/src/store/mod.rs` | Add drv file writing |
| `crates/lib/src/store/paths.rs` | Add drv/drv-out paths |
| `crates/lib/src/build/execute.rs` | Write drv files during build |

## Files to Create (Future)

| Path | Purpose |
|------|---------|
| `crates/lib/src/cache/mod.rs` | Binary cache client |

## Success Criteria

1. drv files written for each build
2. drv-out maps build hash to output hash
3. Hardlinking works when filesystems match
4. Debug command can inspect drv files

## Open Questions

- [ ] Binary cache format and protocol?
- [ ] How to verify binary cache downloads?
- [ ] Should hardlinking be automatic or opt-in?
- [ ] What about drv files for binds?
