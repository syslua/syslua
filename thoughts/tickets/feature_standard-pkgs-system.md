---
beads_id: syslua-13q
type: feature
priority: 2
created: 2026-01-07
status: reviewed
keywords: pkgs, packages, builds, nixpkgs, prebuilt, binaries
patterns: sys.build, M.setup, M.releases, M.meta
---

# Implement Standard Pkgs System (Builds Without Binds)

## Description

Implement a core set of standard packages (pkgs) for syslua, similar in concept to nixpkgs. Unlike modules (which use `sys.build` + `sys.bind`), pkgs are **pure builds only** - they produce artifacts in the content-addressed store but don't touch the system directly.

Packages serve as reusable building blocks that can be:

1. Used as dependencies for other packages (via inputs)
2. Composed with modules to apply to the system (e.g., via `env.setup()` to add to PATH)
3. Consumed by a future "programs" convenience layer

## Context

Currently, syslua has:

- `syslua.modules.*` - System configuration (env, file) using build + bind pattern
- `syslua.lib.*` - Helper utilities (fetch_url)
- `syslua.pkgs.*` - Empty lazy-loading wrapper (exists but no packages)

The pkgs system is foundational infrastructure that enables users to declare their development environment reproducibly across platforms.

## Requirements

### Functional

1. **Pkg module structure** - Each package exports:
   - `M.releases` - Version/platform/hash table for automation
   - `M.meta` - Package metadata (name, homepage, description, license, version aliases)
   - `M.opts` - Default options with priority support
   - `M.setup(opts)` - Returns `BuildRef` only (no side effects)

2. **Category organization** - Packages organized by category:
   - `cli/` - Command-line tools (ripgrep, fd, fzf, jq, bat)
   - `dev/` - Development tools (git, gh, delta)
   - `lang/` - Language runtimes (node, python, rust, go)
   - `editors/` - Text editors (neovim, helix)
   - `net/` - Network utilities (curl, wget)
   - `lib/` - Libraries/dependencies (openssl, sqlite)

3. **Cross-platform support** - Each package defines platform matrix:
   - `aarch64-darwin`, `x86_64-darwin`
   - `aarch64-linux`, `x86_64-linux`
   - `x86_64-windows`

4. **Dependency handling** - Packages can depend on other packages via inputs:
   - Library dependencies (e.g., curl depends on openssl)
   - Build-time dependencies (e.g., from-source builds need toolchain)

5. **Internal helpers** - `_internal/` module with shared utilities:
   - Archive extraction (tar, zip, 7z)
   - Platform detection helpers

### Non-Functional

1. **Naming convention** - Use full names (e.g., `ripgrep` not `rg`)
2. **Version management** - Manual curation, but structure supports future automation
3. **Error handling** - Clear errors for missing versions/platforms with available options listed
4. **Config handling** - Out of scope for pkgs; leave to programs layer

## Current State

```
lua/syslua/pkgs/
└── init.lua  # Lazy-loading wrapper only, no packages
```

The `init.lua` uses metatable `__index` to lazy-load submodules.

## Desired State

```
lua/syslua/pkgs/
├── init.lua                    # Lazy-loading entry (exists)
├── _internal/                  # Shared utilities
│   ├── init.lua
│   ├── extract.lua             # Archive extraction
│   └── platform.lua            # Platform helpers
│
├── cli/
│   ├── init.lua                # Category lazy-loader
│   ├── ripgrep.lua
│   ├── fd.lua
│   └── jq.lua
│
└── lib/
    ├── init.lua
    └── ... (future: openssl, etc.)
```

Usage:

```lua
local syslua = require('syslua')

-- Get package build
local rg = syslua.pkgs.cli.ripgrep.setup()

-- Use outputs
print(rg.outputs.bin)  -- "/syslua/store/build/abc123.../rg"

-- Access metadata (for tooling)
print(syslua.pkgs.cli.ripgrep.meta.versions.stable)  -- "14.1.1"
```

## Research Context

### Keywords to Search

- `sys.build` - Core build primitive
- `M.setup` - Module setup pattern
- `prio.merge` - Priority-based option merging
- `ctx:fetch_url` - URL fetching in builds
- `ctx:exec` - Command execution in builds

### Patterns to Investigate

- `lua/syslua/modules/file.lua` - Reference for build + bind pattern
- `lua/syslua/modules/env.lua` - Reference for complex module structure
- `lua/syslua/lib/init.lua` - Reference for lazy-loading pattern
- `docs/architecture/01-builds.md` - Build system design

### Key Decisions Made

1. **Pkgs = builds only** - No binds; pure store artifacts
2. **Programs layer later** - Config/binding convenience is out of scope
3. **Dependencies as inputs** - Library deps passed as build inputs
4. **Full names** - Use `ripgrep` not `rg` for discoverability
5. **Exported metadata** - `M.releases` and `M.meta` for future automation
6. **Manual curation** - Version updates manual for now, structure supports automation

## Success Criteria

### Automated

- [ ] `cargo test -p syslua-lib` - Library tests pass
- [ ] `cargo build -p syslua-cli` - CLI builds successfully
- [ ] `sys apply` with pkg usage - Packages build correctly

### Manual

- [ ] `ripgrep.setup()` returns BuildRef with valid outputs
- [ ] `ripgrep.meta.versions.stable` returns version string
- [ ] `ripgrep.releases['14.1.1']['aarch64-darwin']` returns url/sha256
- [ ] Error message for missing platform lists available platforms
- [ ] Error message for missing version lists available versions

## Starter Package Set (MVP)

Phase 1 scope - implement these packages:

1. `cli/ripgrep` - Fast grep (good first package, simple binary)
2. `cli/fd` - Fast find (similar pattern to ripgrep)
3. `cli/jq` - JSON processor (tests different archive format)

Future phases (out of scope for this ticket):

- Additional CLI tools (fzf, bat, eza)
- Developer tools (git, gh, delta)
- Language runtimes (node, python, rust)
- Library dependencies (openssl, sqlite)
