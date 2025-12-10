# Implementation Gap Resolution Plan

This document tracks the work required to bring the sys.lua implementation in line with the architecture documentation. The architecture docs are the source of truth, with one exception: the manifest should only track derivations and activations (not the packages/files/env/users structure shown in `08-apply-flow.md`).

## Status Legend

- [ ] Not started
- [x] Complete
- [~] Partial (see notes)

---

## Phase 0: Documentation Corrections (FIRST PRIORITY)

The manifest structure in `08-apply-flow.md` is incorrect and must be fixed before other work proceeds. This ensures documentation and implementation stay aligned.

### 0.1 Fix Manifest Structure in `08-apply-flow.md`

**Status:** [x] Complete

The document showed:
```rust
pub struct Manifest {
    pub packages: Vec<PackageSpec>,
    pub files: Vec<FileSpec>,
    pub env: EnvConfig,
    pub users: Vec<UserConfig>,
}
```

**Correct structure** (per `01-derivations.md` and `02-activations.md`):
```rust
pub struct Manifest {
    pub derivations: Vec<Derivation>,
    pub activations: Vec<Activation>,
}
```

Packages, files, env, and users are all expressed as derivations + activations. The Lua globals `file{}`, `env{}`, `user{}` are convenience helpers that create derivations and activations - they don't have separate manifest entries.

**Implementation Status:** The Rust code in `crates/lua/src/manifest.rs` is already correct. Only the documentation needs updating.

---

## Phase 1: Core Primitives

These are the foundational building blocks that everything else depends on.

### 1.1 Derivation Context (`DerivationCtx`)

**Status:** [~] Partial

**Architecture Reference:** `01-derivations.md:80-120`

| Method | Status | Notes |
|--------|--------|-------|
| `ctx.sys.*` | [x] | Platform info available |
| `ctx.out` | [x] | Output path works |
| `ctx.outputs.out` | [x] | Same as ctx.out |
| `ctx.fetch_url()` | [x] | Implemented in `crates/core/src/fetch.rs` |
| `ctx.fetch_git()` | [ ] | Not implemented - TODO comment only |
| `ctx.unpack()` | [x] | tar.gz extraction works |
| `ctx.copy()` | [x] | Implemented |
| `ctx.move()` | [ ] | Not implemented |
| `ctx.mkdir()` | [x] | Implemented |
| `ctx.write()` | [x] | Implemented |
| `ctx.chmod()` | [x] | Implemented |
| `ctx.symlink()` | [x] | Implemented |
| `ctx.run()` | [ ] | Not implemented |
| `ctx.env` | [ ] | Mutable env table not implemented |

**Work Required:**
- [ ] Implement `ctx.fetch_git()` with revision checkout and SHA256 verification
- [ ] Implement `ctx.move()` for file/directory moves
- [ ] Implement `ctx.run()` for shell command execution
- [ ] Implement `ctx.env` as mutable table seeded with basic PATH

### 1.2 Activation Context (`ActivationCtx`)

**Status:** [~] Partial

**Architecture Reference:** `02-activations.md:110-129`

| Method | Status | Notes |
|--------|--------|-------|
| `ctx.sys.*` | [x] | Platform info available |
| `ctx.add_to_path()` | [x] | Implemented |
| `ctx.symlink()` | [x] | Implemented |
| `ctx.source_in_shell()` | [ ] | Not implemented |
| `ctx.run()` | [ ] | Not implemented (escape hatch) |
| `ctx.enable_service()` | [ ] | Not implemented |

**Work Required:**
- [ ] Implement `ctx.source_in_shell()` for shell script sourcing
- [ ] Implement `ctx.run()` escape hatch with warning during plan phase
- [ ] Implement `ctx.enable_service()` for service management

### 1.3 Derivation Hashing

**Status:** [~] Partial

**Architecture Reference:** `01-derivations.md:142-156`

**Current:** Hash computed from name + version + opts. Uses 12-char truncation.

**Required:** Hash should include:
- name
- version (if present)
- opts (evaluated result)
- config function source code hash
- outputs list
- sys (platform, os, arch)

**Work Required:**
- [ ] Include config function source in derivation hash
- [ ] Change hash truncation from 12 to 9 characters (per `03-store.md:56`)

---

## Phase 2: Lua API Globals

### 2.1 Core Globals (Rust-backed)

**Status:** [x] Complete

| Global | Status |
|--------|--------|
| `derive {}` | [x] |
| `activate {}` | [x] |

### 2.2 Convenience Helpers (Lua)

**Status:** [ ] Not implemented

**Architecture Reference:** `04-lua-api.md:131-138`

| Global | Status | Notes |
|--------|--------|-------|
| `file {}` | [ ] | Should create derivation + activation internally |
| `env {}` | [ ] | Should create derivation + activation internally |
| `user {}` | [ ] | Scoping helper for per-user config |
| `project {}` | [ ] | Scoping helper for project environments |
| `input ""` | [ ] | Declared in M.inputs, not a global function |

**Work Required:**
- [ ] Implement `file {}` global that creates file derivation + symlink activation
- [ ] Implement `env {}` global that creates env derivation + source_in_shell activation
- [ ] Implement `user {}` scoping helper
- [ ] Implement `project {}` scoping helper

### 2.3 System Information (`syslua` table)

**Status:** [~] Partial

**Architecture Reference:** `04-lua-api.md:150-161`

| Field | Status |
|-------|--------|
| `syslua.platform` | [x] |
| `syslua.os` | [x] |
| `syslua.arch` | [x] |
| `syslua.hostname` | [x] |
| `syslua.username` | [x] |
| `syslua.is_linux` | [x] |
| `syslua.is_darwin` | [x] |
| `syslua.is_windows` | [x] |
| `syslua.version` | [ ] |

**Work Required:**
- [ ] Add `syslua.version` field with sys.lua version string

### 2.4 Library Functions (`syslua.lib`)

**Status:** [ ] Not implemented

**Architecture Reference:** `04-lua-api.md:164-183`

| Function | Status |
|----------|--------|
| `lib.toJSON()` | [ ] |
| `lib.mkDefault()` | [ ] |
| `lib.mkForce()` | [ ] |
| `lib.mkBefore()` | [ ] |
| `lib.mkAfter()` | [ ] |
| `lib.mkOverride()` | [ ] |
| `lib.mkOrder()` | [ ] |
| `lib.env.defineMergeable()` | [ ] |
| `lib.env.defineSingular()` | [ ] |

**Work Required:**
- [ ] Implement `syslua.lib` module with all priority functions
- [ ] Implement JSON conversion utility
- [ ] Implement env variable definition helpers

---

## Phase 3: Input Resolution

**Status:** [ ] Not started

**Architecture Reference:** `06-inputs.md`

### 3.1 Input URL Parsing

| Format | Status |
|--------|--------|
| `git:git@github.com:org/repo.git` (SSH) | [ ] |
| `git:https://github.com/org/repo.git` (HTTPS) | [ ] |
| `path:~/local/path` | [ ] |
| `path:./relative/path` | [ ] |

### 3.2 Lock File Management

- [ ] Generate `syslua.lock` from resolved inputs
- [ ] Read and validate existing lock file
- [ ] Update lock file on `sys update`

### 3.3 Git Operations

- [ ] Clone git repositories (SSH auth via ~/.ssh/)
- [ ] Checkout specific revisions
- [ ] Compute SHA256 of repository content
- [ ] Strip .git directory after clone

**Work Required:**
- [ ] Implement input URL parsing in `crates/core/`
- [ ] Implement lock file read/write
- [ ] Implement git clone with revision checkout (use `gix` crate)
- [ ] Implement SSH key authentication
- [ ] Implement `sys update` command

---

## Phase 4: Store Enhancements

**Status:** [~] Partial

**Architecture Reference:** `03-store.md`

### 4.1 Store Layout

| Directory | Status | Notes |
|-----------|--------|-------|
| `obj/<name>-<hash>/` | [x] | Working |
| `pkg/<name>/<ver>/<plat>` | [~] | Symlinks created but platform not in path |
| `drv/<hash>.drv` | [ ] | Derivation files not written |
| `drv-out/<hash>` | [ ] | Cache index not implemented |
| `metadata/manifest.json` | [ ] | Not implemented |
| `metadata/snapshots.json` | [ ] | Not implemented |
| `metadata/gc-roots/` | [ ] | Not implemented |

### 4.2 Immutability

| Platform | Status |
|----------|--------|
| Linux (`chattr +i`) | [ ] |
| macOS (`chflags uchg`) | [ ] |
| Windows (ACLs) | [ ] |

**Work Required:**
- [ ] Add platform to pkg symlink path
- [ ] Write .drv files for debugging/rebuilds
- [ ] Implement drv-out cache index
- [ ] Create metadata directory structure
- [ ] Implement immutability flags per platform

---

## Phase 5: Snapshot System

**Status:** [ ] Not started

**Architecture Reference:** `05-snapshots.md`

### 5.1 Snapshot Structure

```rust
pub struct Snapshot {
    pub id: String,
    pub created_at: u64,
    pub description: String,
    pub config_path: Option<PathBuf>,
    pub derivations: Vec<String>,      // Just hashes
    pub activations: Vec<Activation>,  // What to do with outputs
}
```

### 5.2 Snapshot Operations

- [ ] Create snapshot on successful apply
- [ ] Create pre-apply snapshot for rollback
- [ ] List snapshots (`sys history`)
- [ ] Compare snapshots (`sys diff`)
- [ ] Rollback to snapshot (`sys rollback`)

### 5.3 Garbage Collection

- [ ] Find all derivation hashes referenced by snapshots
- [ ] Remove unreferenced objects from store
- [ ] Clear immutability before deletion
- [ ] Implement `sys gc` command

**Work Required:**
- [ ] Implement Snapshot struct and serialization
- [ ] Implement snapshot storage (metadata/snapshots/)
- [ ] Implement snapshot creation during apply
- [ ] Implement rollback algorithm
- [ ] Implement garbage collection

---

## Phase 6: CLI Commands

**Status:** [~] Partial

**Architecture Reference:** `10-crates.md:28-44`

| Command | Status | Notes |
|---------|--------|-------|
| `sys apply` | [x] | Basic implementation works |
| `sys plan` | [x] | Basic implementation works |
| `sys info` | [x] | Shows system info |
| `sys status` | [ ] | Not implemented |
| `sys list` | [ ] | Not implemented |
| `sys history` | [ ] | Not implemented |
| `sys rollback` | [ ] | Not implemented |
| `sys gc` | [ ] | Not implemented |
| `sys update` | [ ] | Not implemented |
| `sys shell` | [ ] | Not implemented |
| `sys env` | [ ] | Not implemented |
| `sys secrets rotate` | [ ] | Not implemented |
| `sys secrets set` | [ ] | Not implemented |
| `sys completions` | [ ] | Not implemented |
| `sys init` | [ ] | Not implemented |

**Work Required:**
- [ ] Implement `sys status` - show current environment status
- [ ] Implement `sys list` - list installed packages
- [ ] Implement `sys history` - show snapshot history
- [ ] Implement `sys rollback` - rollback to snapshot
- [ ] Implement `sys gc` - garbage collect store
- [ ] Implement `sys update` - update lock file
- [ ] Implement `sys shell` - enter project/ephemeral shell
- [ ] Implement `sys env` - print activation script
- [ ] Implement `sys secrets *` - SOPS secret management
- [ ] Implement `sys completions` - shell completions
- [ ] Implement `sys init` - generate .luarc.json and template

---

## Phase 7: DAG Execution

**Status:** [ ] Not started

**Architecture Reference:** `08-apply-flow.md:149-187`

**Current:** Sequential loop over derivations/activations.

**Required:**
- DAG construction from manifest
- Topological sorting
- Parallel execution of independent nodes
- Atomic rollback on failure

**Work Required:**
- [ ] Build execution DAG from derivations (petgraph)
- [ ] Implement topological sort
- [ ] Implement parallel execution (rayon)
- [ ] Implement rollback on node failure

---

## Phase 8: Environment Script Generation

**Status:** [~] Partial

**Architecture Reference:** `09-platform.md:36-71`

| Script | Status |
|--------|--------|
| `env.sh` (bash/zsh) | [x] |
| `env.fish` | [ ] |
| `env.ps1` (PowerShell) | [ ] |
| `env.cmd` (cmd.exe) | [ ] |

**Work Required:**
- [ ] Generate fish shell environment script
- [ ] Generate PowerShell environment script
- [ ] Generate cmd.exe environment script
- [ ] Implement per-user profile directories

---

## Phase 9: Service Management

**Status:** [ ] Not started

**Architecture Reference:** `09-platform.md:162-259`

| Platform | Init System | Status |
|----------|-------------|--------|
| Linux | systemd | [ ] |
| macOS | launchd | [ ] |
| Windows | Windows Services | [ ] |

**Work Required:**
- [ ] Implement systemd service management
- [ ] Implement launchd service management
- [ ] Implement Windows service management
- [ ] Add `ctx.enable_service()` to activation context

---

## Phase 10: SOPS Integration

**Status:** [ ] Not started (crate is stub)

**Architecture Reference:** `10-crates.md:168-183`

**Current:** `crates/sops/` contains default cargo template, not actual implementation.

**Work Required:**
- [ ] Implement SOPS file format parsing
- [ ] Implement Age encryption/decryption
- [ ] Implement `sops.load()` Lua function
- [ ] Fix `Cargo.toml` edition (currently "2024" which doesn't exist)

---

## Phase 11: LuaLS Integration

**Status:** [ ] Not started

**Architecture Reference:** `04-lua-api.md:186-460`

**Work Required:**
- [ ] Create `lib/types/` directory with type definition files
- [ ] Implement `.luarc.json` generation in `sys init`
- [ ] Add type annotations to all Lua modules

---

## Phase 12: Documentation Corrections

These items in the architecture docs need updating (not implementation gaps):

### 12.1 `08-apply-flow.md` Manifest Structure

The manifest example shows:
```rust
pub struct Manifest {
    pub packages: Vec<PackageSpec>,
    pub files: Vec<FileSpec>,
    pub env: EnvConfig,
    pub users: Vec<UserConfig>,
}
```

**Correction:** Manifest should only contain:
```rust
pub struct Manifest {
    pub derivations: Vec<Derivation>,
    pub activations: Vec<Activation>,
}
```

Packages, files, env, and users are all expressed as derivations + activations. This is the correct model per `01-derivations.md` and `02-activations.md`.

---

## Priority Order

Recommended implementation order based on dependencies:

1. ~~**Phase 0** - Documentation corrections~~ (COMPLETE)
2. **Phase 1** - Core primitives (everything depends on these)
3. **Phase 2** - Lua globals (needed for user configs)
4. **Phase 4** - Store enhancements (needed before snapshots)
5. **Phase 5** - Snapshot system (needed for rollback)
6. **Phase 7** - DAG execution (needed for proper apply)
7. **Phase 3** - Input resolution (enables external packages)
8. **Phase 6** - CLI commands (user-facing features)
9. **Phase 8** - Environment scripts (multi-shell support)
10. **Phase 9** - Service management (advanced feature)
11. **Phase 10** - SOPS integration (secrets management)
12. **Phase 11** - LuaLS integration (developer experience)

---

## Related Files

- Implementation: `crates/*/src/`
- Architecture docs: `docs/architecture/`
- Examples: `examples/`
- Package definitions: `pkgs/`
