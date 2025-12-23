# Snapshots

> Part of the [SysLua Architecture](./00-overview.md) documentation.

This document covers the snapshot design, rollback algorithm, and garbage collection.

## Core Principle

**Builds are immutable; binds are what change between snapshots.**

Snapshots capture system state using the **builds + binds** model. A snapshot contains a manifest with build definitions and bind definitions (called "activations"). This unified model eliminates the need for separate snapshot types for files, packages, and environment variables.

When you rollback, the builds (content in the store) don't change - they're already there, cached by their content hash. What changes is which binds are active: which symlinks exist, which directories are in PATH, which services are enabled.

## Snapshot Structure

```rust
/// A snapshot captures system state as a manifest of builds and binds.
pub struct Snapshot {
    /// Unique identifier (timestamp-based)
    pub id: String,

    /// Unix timestamp when the snapshot was created
    pub created_at: u64,

    /// Path to the configuration file that produced this state
    pub config_path: Option<PathBuf>,

    /// The manifest containing builds and binds (activations)
    pub manifest: Manifest,
}

/// The manifest contains evaluated build and bind definitions.
/// Keys are 20-char truncated hashes for deduplication.
pub struct Manifest {
    /// All build definitions, keyed by BuildHash
    pub builds: BTreeMap<BuildHash, BuildDef>,

    /// All bind definitions, keyed by BindHash
    pub bindings: BTreeMap<BindHash, BindDef>,
}

/// An evaluated bind definition (serializable).
pub struct BindDef {
    pub inputs: Option<InputsRef>,
    pub apply_actions: Vec<BindAction>,
    pub outputs: Option<BTreeMap<String, String>>,
    pub destroy_actions: Option<Vec<BindAction>>,
}

/// Actions that can be performed during a bind.
pub enum BindAction {
    Cmd {
        cmd: String,
        env: Option<BTreeMap<String, String>>,
        cwd: Option<String>,
    },
}
```

## Storage Layout

```
~/.local/share/syslua/
├── snapshots/
│   ├── metadata.json           # Index of all snapshots
│   ├── <snapshot_id>.json      # Individual snapshot data
│   └── ...
└── store/
    └── obj/                    # Build outputs (immutable, content-addressed)
        ├── ripgrep-15.1.0-abc123def456789012/  # 20-char hash
        ├── file-gitconfig-def456abc123789012/
        └── env-editor-ghi789abc123456012/
```

### Metadata Index

```json
{
  "version": 1,
  "snapshots": [
    {
      "id": "1765208363188",
      "created_at": 1733667300,
      "build_count": 5,
      "activation_count": 8
    }
  ],
  "current": "1765208363188"
}
```

### Individual Snapshot

```json
{
  "id": "1765208363188",
  "created_at": 1733667300,
  "config_path": "/home/ian/.config/syslua/init.lua",

  "manifest": {
    "builds": [
      {
        "name": "ripgrep",
        "version": "15.1.0",
        "inputs": { ... },
        "apply_actions": [ ... ],
        "outputs": { "out": "/store/obj/ripgrep-abc123" }
      }
    ],
    "activations": [
      {
        "inputs": { "build": { "hash": "abc123", "outputs": { "out": "..." } } },
        "apply_actions": [
          { "Cmd": { "cmd": "ln -sf /store/path/bin/rg /usr/local/bin/rg" } }
        ],
        "destroy_actions": [
          { "Cmd": { "cmd": "rm /usr/local/bin/rg" } }
        ]
      }
    ]
  }
}
```

## Why This Model is Better

| Aspect                    | Old Model (separate types)      | New Model (builds + binds)                  |
| ------------------------- | ------------------------------- | ------------------------------------------- |
| **Type proliferation**    | SnapshotFile, SnapshotEnv, etc. | Just BindDef with apply/destroy actions     |
| **Adding new features**   | New struct for each feature     | Use cmd action with appropriate destroy     |
| **Diff clarity**          | Compare heterogeneous lists     | Compare manifests                           |
| **GC integration**        | Must track refs from each type  | Build hashes are the refs                   |
| **Rollback logic**        | Different logic per type        | Uniform: execute destroy_actions            |
| **Content deduplication** | Per-type deduplication          | Single build store                          |

## What Gets Captured

Everything is captured through builds and binds:

| User Action                                 | Build                   | Bind                                              |
| ------------------------------------------- | ----------------------- | ------------------------------------------------- |
| `require("pkgs.cli.ripgrep").setup()`       | Package fetch/extract   | `apply` creates symlink, `destroy` removes it     |
| `lib.file.setup({ path, src })`             | Content copy to store   | `apply` symlinks, `destroy` removes               |
| `lib.file.setup({ mutable = true })`        | Metadata (link info)    | `apply` symlinks, `destroy` removes               |
| `lib.env.setup({ EDITOR = "nvim" })`        | Shell fragments         | Shell integration sources the files               |
| `require("modules.services.nginx").setup()` | Service unit build      | `apply` installs/enables, `destroy` stops/removes |

## Rollback

Rollback is straightforward with the builds + binds model:

```bash
$ sys rollback                    # Rollback to previous snapshot
$ sys rollback <snapshot_id>      # Rollback to specific snapshot
$ sys rollback --dry-run          # Preview what would change
```

**Key insight**: Builds don't need to be "rolled back" - they're immutable in the store. Only binds change, and each bind has `destroy_actions` that reverse its effect.

## Rollback Algorithm

```
ROLLBACK_TO_SNAPSHOT(target_snapshot_id, dry_run=false):
    target = LOAD_SNAPSHOT(target_snapshot_id)
    IF target IS NULL:
        ERROR "Snapshot '{target_snapshot_id}' not found"

    current = GET_CURRENT_SNAPSHOT()

    // Phase 1: Compute activation diff
    activations_to_remove = current.manifest.activations - target.manifest.activations
    activations_to_add = target.manifest.activations - current.manifest.activations

    // Phase 2: Display changes
    PRINT_ROLLBACK_PLAN(activations_to_remove, activations_to_add)

    IF dry_run:
        RETURN

    IF NOT CONFIRM("Proceed with rollback?"):
        RETURN

    // Phase 3: Create pre-rollback snapshot
    pre_rollback = CREATE_SNAPSHOT("Before rollback to " + target_snapshot_id)

    // Phase 4: Execute rollback (atomic)
    TRY:
        // Execute destroy_actions for activations not in target
        FOR EACH activation IN activations_to_remove:
            IF activation.destroy_actions IS NOT NULL:
                FOR EACH action IN activation.destroy_actions:
                    EXECUTE(action.cmd)

        // Execute apply_actions for activations in target
        FOR EACH activation IN activations_to_add:
            FOR EACH action IN activation.apply_actions:
                EXECUTE(action.cmd)

        // Update current pointer
        SET_CURRENT_SNAPSHOT(target_snapshot_id)
        PRINT "Rollback successful"

    CATCH error:
        ERROR "Rollback failed: {error}"
        PRINT "Restoring pre-rollback state..."
        ROLLBACK_TO_SNAPSHOT(pre_rollback.id, dry_run=false)
        ERROR "Rollback aborted. System restored to pre-rollback state."
```

### Undo Logic

```
UNDO_ACTIVATION(activation):
    IF activation.destroy_actions IS NOT NULL:
        FOR EACH action IN activation.destroy_actions:
            EXECUTE(action.cmd)
```

### Apply Logic

```
APPLY_ACTIVATION(activation):
    FOR EACH action IN activation.apply_actions:
        EXECUTE(action.cmd)
```

## Garbage Collection

The GC algorithm is simplified with the builds + binds model:

```
GARBAGE_COLLECT():
    // Collect all build hashes referenced by any snapshot
    referenced_hashes = SET()
    FOR EACH snapshot IN ALL_SNAPSHOTS():
        FOR EACH build IN snapshot.manifest.builds:
            referenced_hashes.add(COMPUTE_HASH(build))

    // Remove unreferenced objects from store
    FOR EACH obj_dir IN store/obj/*:
        hash = EXTRACT_HASH(obj_dir)
        IF hash NOT IN referenced_hashes:
            REMOVE_IMMUTABILITY(obj_dir)
            DELETE(obj_dir)
```

### GC with Locking

To prevent race conditions, GC uses a global lock:

```
GC_COLLECT():
    lock = ACQUIRE_STORE_LOCK(exclusive=true, timeout=30s)
    IF lock IS NULL:
        ERROR "Could not acquire store lock. Another SysLua operation may be running."

    TRY:
        // Phase 1: Find all roots
        roots = SET()

        // Add all package symlinks
        FOR EACH symlink IN GLOB("store/pkg/**/*"):
            IF IS_SYMLINK(symlink):
                target = READ_LINK(symlink)
                hash = EXTRACT_HASH_FROM_PATH(target)
                roots.add(hash)

        // Add all snapshots
        FOR EACH snapshot IN LOAD_ALL_SNAPSHOTS():
            FOR EACH build IN snapshot.manifest.builds:
                roots.add(COMPUTE_HASH(build))

        // Phase 2: Find unreferenced objects
        unreferenced = []
        FOR EACH obj_path IN GLOB("store/obj/*"):
            hash = EXTRACT_HASH(obj_path)
            IF hash NOT IN roots:
                unreferenced.append({ hash, path: obj_path })

        // Phase 3: Remove unreferenced objects
        total_size = 0
        FOR EACH { hash, path } IN unreferenced:
            size = GET_DIRECTORY_SIZE(path)
            total_size += size
            MAKE_MUTABLE(path)
            REMOVE_DIRECTORY(path)

        PRINT "Removed {unreferenced.length} objects, freed {total_size} bytes"

    FINALLY:
        RELEASE_STORE_LOCK(lock)
```

### Concurrent Operation Protection

| Operation    | Lock Type     | Blocks GC? | Blocked by GC? |
| ------------ | ------------- | ---------- | -------------- |
| `sys apply`  | Exclusive     | Yes        | Yes            |
| `sys gc`     | Exclusive     | N/A        | Yes (by apply) |
| `sys plan`   | Shared (read) | No         | No             |
| `sys status` | Shared (read) | No         | No             |
| `sys shell`  | Shared (read) | No         | No             |

### GC and Snapshots

Snapshots protect their referenced objects from GC:

```bash
$ sys apply init.lua           # Installs ripgrep@15.1.0 (creates snapshot 1)
$ # Edit SysLua to remove ripgrep
$ sys apply init.lua           # Removes ripgrep symlink (creates snapshot 2)
$ sys gc                       # Does NOT delete ripgrep object (snapshot 1 references it)
$ sys rollback <snapshot 1>    # Can still rollback (object exists)
$ sys gc --delete-old-snapshots --keep 5  # Delete old snapshots
$ sys gc                       # NOW ripgrep object can be deleted
```

## Comparing Snapshots

With builds + binds, comparing snapshots is clear:

```bash
$ sys diff <snapshot_a> <snapshot_b>

Build changes:
  + ripgrep-16.0.0-newhhash  (new version)
  - ripgrep-15.1.0-oldhash   (removed)
  = neovim-0.10.0-abc123     (unchanged)

Activation changes:
  ~ Symlink ~/.gitconfig     (different build: def456 → ghi789)
  + Service postgresql       (added)
  - Symlink /old/tool        (removed)
```

This clear separation makes it easy to understand what changed between configurations.

## See Also

- [Store](./03-store.md) - Where build outputs live
- [Builds](./01-builds.md) - How builds work
- [Binds](./02-binds.md) - How binds work
- [Apply Flow](./08-apply-flow.md) - How snapshots are created during apply
