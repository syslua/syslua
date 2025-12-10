# sys.lua Implementation Progress

This document tracks the vertical slice implementation plan for sys.lua, prioritizing early dogfooding value.

## Overview

The implementation follows vertical slices that deliver usable functionality at each stage:

| Slice | Goal | Status |
|-------|------|--------|
| 1 | Basic File Management (symlinks/dotfiles) | ✅ Complete |
| 2 | Environment Variables | ✅ Complete |
| 3 | Store + Single Package Install | ⏳ Pending |
| 4 | Inputs + Lock File | ⏳ Pending |
| 5 | Snapshots + Rollback | ⏳ Pending |

---

## Slice 1: Basic File Management

**Goal:** `sys apply` can manage dotfiles via symlinks

**Status:** ✅ Complete (2024-12-09)

**Minimal config:**
```lua
file {
    path = "~/.gitconfig",
    symlink = "./dotfiles/gitconfig",
}
```

### Components

#### sys-platform
- [x] `Platform` struct with OS/arch detection
- [x] `expand_path()` for `~` expansion
- [x] `expand_path_with_base()` for relative path resolution
- [x] Platform identifiers (`aarch64-darwin`, `x86_64-linux`, etc.)
- [x] Home directory resolution
- [x] Store path helpers

#### sys-lua
- [x] Initialize mlua runtime
- [x] Expose `syslua` global table (os, arch, platform, hostname, username, version)
- [x] Boolean helpers (is_linux, is_darwin, is_windows)
- [x] Implement `file{}` declaration function
- [x] Evaluate config file and collect declarations
- [x] Support symlink, content, and copy file types
- [ ] Error handling with line numbers (future enhancement)

#### sys-core
- [x] `FileDecl` struct (path, symlink, content, copy, mode)
- [x] `Manifest` struct containing file declarations
- [x] `Plan` struct showing what will change
- [x] `FileChange` with CreateSymlink, UpdateSymlink, CreateContent, UpdateContent, CopyFile, Unchanged kinds
- [x] `compute_plan()` function to diff manifest vs current state
- [x] `apply()` function to apply changes
- [x] Automatic parent directory creation
- [x] Idempotency - running apply twice produces no changes
- [ ] Backup existing files before overwriting (future enhancement)

#### sys-cli
- [x] `plan <config.lua>` command (dry-run)
- [x] `apply <config.lua>` command
- [x] `status` command showing platform info
- [x] Colored diff output (green +, yellow ~)
- [x] `--verbose` flag for detailed output

### Testing
- [x] Unit tests for path expansion (12 tests in sys-platform)
- [x] Unit tests for Lua evaluation (12 tests in sys-lua)
- [x] Unit tests for plan/apply (8 tests in sys-core)
- [x] Manual integration test: apply symlinks, content files, nested directories

### Usage

```bash
# Build
cargo build --release -p sys-cli

# Show platform info
./target/release/sys status

# Dry-run (show what would change)
./target/release/sys plan init.lua

# Apply configuration
./target/release/sys apply init.lua
```

---

## Slice 2: Environment Variables

**Goal:** `sys apply` generates shell environment scripts

**Status:** ✅ Complete (2024-12-09)

**Config:**
```lua
env {
    EDITOR = "nvim",
    PATH = { "~/.local/bin" },  -- prepend
}
```

### Components

#### sys-platform
- [x] Shell enum (Bash, Zsh, Fish, PowerShell, Sh)
- [x] Shell detection from $SHELL
- [x] Shell-specific export statements
- [x] Env script path helpers

#### sys-lua
- [x] `EnvDecl` type (name, values, merge strategy)
- [x] `EnvValue` type with Replace/Prepend/Append strategies
- [x] `env{}` declaration function
- [x] Support for simple values, arrays (prepend), and explicit append

#### sys-core
- [x] `EnvDecl` in Manifest
- [x] Env script generation for multiple shells
- [x] Source command generation
- [x] Write env scripts to `~/.config/syslua/env/`

#### sys-cli
- [x] `sys env` command to generate/write scripts
- [x] `--shell` flag to specify target shell
- [x] `--print` flag to output script to stdout
- [x] Setup instructions for shell config

### Testing
- [x] Unit tests for shell export generation (11 tests in sys-platform)
- [x] Unit tests for env{} parsing (6 tests in sys-lua)
- [x] Unit tests for env script generation (7 tests in sys-core)
- [x] Manual integration test: generate env scripts for bash/zsh/fish

### Usage

```bash
# Generate env scripts and show setup instructions
sys env init.lua

# Print script content for current shell
sys env --print init.lua

# Print script content for specific shell
sys env --print --shell fish init.lua

# One-liner to activate in current shell
eval "$(sys env --print)"
```

---

## Slice 3: Store + Single Package Install

**Goal:** Install a prebuilt binary from URL

**Config:**
```lua
local lib = require("syslua.lib")

pkg "ripgrep" {
    version = "15.1.0",
    src = lib.fetchUrl {
        url = "https://github.com/.../rg.tar.gz",
        sha256 = "...",
    },
    bin = { "rg" },
}
```

### Components

#### sys-core
- [ ] Store directory layout (`obj/`, `pkg/`, `drv/`)
- [ ] `FetchUrl` derivation type
- [ ] SHA256 verification
- [ ] Archive extraction (tar.gz, zip)
- [ ] Content-addressed storage
- [ ] Package symlinks

#### sys-lua
- [ ] `lib.fetchUrl{}` function returning derivation
- [ ] `pkg` declaration function

---

## Slice 4: Inputs + Lock File

**Goal:** Use packages from a registry

**Config:**
```lua
local inputs = {
    pkgs = input "github:sys-lua/pkgs"
}
pkg(inputs.pkgs.ripgrep)
```

### Components

- [ ] Input declaration parsing
- [ ] GitHub tarball fetching
- [ ] Lock file generation/reading
- [ ] `sys update` command

---

## Slice 5: Snapshots + Rollback

**Goal:** `sys rollback` to previous state

### Components

- [ ] Snapshot creation on apply
- [ ] `sys history` command
- [ ] `sys rollback` command

---

## Notes

- Each slice should be independently testable
- Prioritize macOS support for dogfooding, then Linux
- Windows support can follow once core is stable
- 62 total tests passing as of Slice 2 completion (was 35 after Slice 1)
