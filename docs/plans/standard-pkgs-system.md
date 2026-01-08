# Standard Pkgs System Implementation Plan

## Overview

Implement a core set of standard packages (pkgs) for syslua - pure builds without binds that produce artifacts in the content-addressed store. This enables reproducible, cross-platform package management similar to nixpkgs.

## Beads Reference

- Issue: `syslua-13q`

## Research Findings

- `sys.build({id?, inputs?, create, replace?})` returns `BuildRef` with `.outputs` table (`globals.d.lua:18-22`)
- `ctx.out` = output directory placeholder, `ctx:fetch_url(url, sha256)` returns downloaded path, `ctx:exec({bin, args?, env?, cwd?})` records command (`globals.d.lua:9-12`)
- `sys.platform` provides values like `"aarch64-darwin"`, `"x86_64-linux"`, `"x86_64-windows"` (`globals.d.lua:59`)
- `sys.os` provides `"darwin"`, `"linux"`, `"windows"` - used for conditional logic (`modules/file.lua`, `modules/env.lua`)
- Lazy-loading pattern: `setmetatable(M, {__index=...})` with `pcall(require, prefix..k)` and `rawset` caching (`pkgs/init.lua`, `modules/init.lua`, `lib/init.lua`)
- `prio.merge(defaults, provided)` for option merging with priority support (`priority.lua`)
- `syslua.lib.fetch_url({url, sha256})` exists as a helper that wraps `sys.build` + `ctx:fetch_url` (`lib/init.lua:27-47`)
- Archive extraction: `tar -xzf` for `.tar.gz`, `unzip -q` for `.zip`, PowerShell `Expand-Archive` on Windows

### Package-Specific Notes

- **ripgrep** (15.1.0): Archives with individual `.sha256` sidecar files
- **fd** (v10.3.0): Archives but NO checksums provided - must generate manually
- **jq** (jq-1.8.1): Standalone binaries (not archives), combined `sha256sum.txt`, no musl builds for Linux

## Current State

```
lua/syslua/pkgs/
└── init.lua  # Lazy-loading wrapper only, no packages
```

## Desired End State

```
lua/syslua/lib/
├── init.lua                    # Existing (has fetch_url, add extract)
└── extract.lua                 # NEW: Archive extraction helper

lua/syslua/pkgs/
├── init.lua                    # Existing lazy-loader (unchanged)
├── cli/
│   ├── init.lua                # Category lazy-loader
│   ├── ripgrep.lua             # Reference package (15.1.0)
│   ├── fd.lua                  # Second package (v10.3.0)
│   └── jq.lua                  # Third package (jq-1.8.1, standalone binary)
```

**Verification**: User can run `sys apply` with config using `syslua.pkgs.cli.ripgrep.setup()` and get a working binary in the store.

## What We're NOT Doing

- Programs layer (convenience bindings) - future ticket
- Runtime config generation for packages - future ticket
- Automated version/hash updates - manual curation for now
- Library packages (openssl, sqlite) - future ticket
- Source builds - prebuilt binaries only

---

## Phase 1: Library Helpers

Add archive extraction helper to `syslua.lib` (alongside existing `fetch_url`).

### Changes Required

**File**: `lua/syslua/lib/extract.lua`
**Changes**: Create archive extraction helper that handles tar.gz, zip cross-platform

```lua
---@class syslua.lib.extract
local M = {}

---Extract an archive to a destination directory
---@param ctx BuildCtx Build context
---@param archive string Path to archive file
---@param dest string Destination directory
---@param opts? {strip_components?: number} Options
function M.archive(ctx, archive, dest, opts)
  opts = opts or {}
  local strip = opts.strip_components or 0

  if archive:match('%.zip$') then
    if sys.os == 'windows' then
      ctx:exec({
        bin = 'powershell.exe',
        args = {
          '-NoProfile',
          '-Command',
          string.format('Expand-Archive -Path "%s" -DestinationPath "%s" -Force', archive, dest),
        },
      })
    else
      ctx:exec({ bin = 'unzip', args = { '-q', archive, '-d', dest } })
    end
  elseif archive:match('%.tar%.gz$') or archive:match('%.tgz$') then
    local args = { '-xzf', archive, '-C', dest }
    if strip > 0 then
      table.insert(args, '--strip-components=' .. strip)
    end
    ctx:exec({ bin = 'tar', args = args })
  elseif archive:match('%.tar%.xz$') then
    local args = { '-xJf', archive, '-C', dest }
    if strip > 0 then
      table.insert(args, '--strip-components=' .. strip)
    end
    ctx:exec({ bin = 'tar', args = args })
  else
    error('Unsupported archive format: ' .. archive)
  end
end

return M
```

### Success Criteria

#### Automated

- [x] `cargo build -p syslua-cli` - CLI builds (no Rust changes needed)

#### Manual

- [x] `require('syslua.lib.extract')` loads without error
- [x] `syslua.lib.extract.archive` function exists and has correct signature

---

## Phase 2: CLI Category Infrastructure

Create the cli category with lazy-loading.

### Changes Required

**File**: `lua/syslua/pkgs/cli/init.lua`
**Changes**: Create category lazy-loader following established pattern

```lua
---@class syslua.pkgs.cli
local M = {}

setmetatable(M, {
  __index = function(t, k)
    local cached = rawget(t, k)
    if cached ~= nil then
      return cached
    end
    local ok, mod = pcall(require, 'syslua.pkgs.cli.' .. k)
    if ok then
      rawset(t, k, mod)
      return mod
    else
      error("Module 'syslua.pkgs.cli." .. k .. "' not found")
    end
  end,
})

return M
```

### Success Criteria

#### Manual

- [x] `require('syslua.pkgs.cli')` loads without error
- [x] `syslua.pkgs.cli.nonexistent` throws clear error message

---

## Phase 3: First Package (ripgrep)

Implement ripgrep as the reference package with full module structure.

### Changes Required

**File**: `lua/syslua/pkgs/cli/ripgrep.lua`
**Changes**: Create full package implementation with M.releases, M.meta, M.opts, M.setup()

```lua
local prio = require('syslua.priority')
local extract = require('syslua.lib.extract')

---@class syslua.pkgs.cli.ripgrep
local M = {}

-- ============================================================================
-- Metadata (exported for tooling/automation)
-- ============================================================================

---@class RipgrepRelease
---@field url string
---@field sha256 string

---@type table<string, table<string, RipgrepRelease>>
M.releases = {
  ['15.1.0'] = {
    ['aarch64-darwin'] = {
      url = 'https://github.com/BurntSushi/ripgrep/releases/download/15.1.0/ripgrep-15.1.0-aarch64-apple-darwin.tar.gz',
      sha256 = '<obtain-during-implementation>',
    },
    ['x86_64-darwin'] = {
      url = 'https://github.com/BurntSushi/ripgrep/releases/download/15.1.0/ripgrep-15.1.0-x86_64-apple-darwin.tar.gz',
      sha256 = '<obtain-during-implementation>',
    },
    ['x86_64-linux'] = {
      url = 'https://github.com/BurntSushi/ripgrep/releases/download/15.1.0/ripgrep-15.1.0-x86_64-unknown-linux-musl.tar.gz',
      sha256 = '<obtain-during-implementation>',
    },
    ['x86_64-windows'] = {
      url = 'https://github.com/BurntSushi/ripgrep/releases/download/15.1.0/ripgrep-15.1.0-x86_64-pc-windows-msvc.zip',
      sha256 = '<obtain-during-implementation>',
    },
  },
}

---@class RipgrepMeta
M.meta = {
  name = 'ripgrep',
  homepage = 'https://github.com/BurntSushi/ripgrep',
  description = 'ripgrep recursively searches directories for a regex pattern',
  license = 'MIT',
  versions = {
    stable = '15.1.0',
    latest = '15.1.0',
  },
}

-- ============================================================================
-- Options
-- ============================================================================

---@class RipgrepOptions
---@field version? string Version to install (default: stable)

local default_opts = {
  version = prio.default(M.meta.versions.stable),
}

---@type RipgrepOptions
M.opts = default_opts

-- ============================================================================
-- Setup
-- ============================================================================

---Build ripgrep package
---@param provided_opts? RipgrepOptions
---@return BuildRef
function M.setup(provided_opts)
  local new_opts = prio.merge(M.opts, provided_opts or {})
  if not new_opts then
    error('Failed to merge ripgrep options')
  end
  M.opts = new_opts

  -- Resolve version alias
  local version = M.meta.versions[M.opts.version] or M.opts.version

  local release = M.releases[version]
  if not release then
    local available = {}
    for v in pairs(M.releases) do
      table.insert(available, v)
    end
    table.sort(available)
    error(string.format("ripgrep version '%s' not found. Available: %s", version, table.concat(available, ', ')))
  end

  local platform_release = release[sys.platform]
  if not platform_release then
    local available = {}
    for p in pairs(release) do
      table.insert(available, p)
    end
    table.sort(available)
    error(
      string.format(
        'ripgrep %s not available for %s. Available: %s',
        version,
        sys.platform,
        table.concat(available, ', ')
      )
    )
  end

  return sys.build({
    id = 'ripgrep-' .. version,
    inputs = {
      url = platform_release.url,
      sha256 = platform_release.sha256,
      version = version,
    },
    create = function(inputs, ctx)
      local archive = ctx:fetch_url(inputs.url, inputs.sha256)
      extract.archive(ctx, archive, ctx.out, { strip_components = 1 })

      local bin_name = 'rg' .. (sys.os == 'windows' and '.exe' or '')
      return {
        bin = ctx.out .. '/' .. bin_name,
        out = ctx.out,
      }
    end,
  })
end

return M
```

### Success Criteria

#### Automated

- [x] `cargo test -p syslua-lib` - Library tests pass

#### Manual

- [x] `ripgrep.meta.versions.stable` returns `"15.1.0"`
- [x] `ripgrep.releases['15.1.0']['aarch64-darwin']` returns table with url/sha256
- [x] `ripgrep.setup()` returns BuildRef without error
- [x] `ripgrep.setup({ version = 'invalid' })` shows available versions in error
- [ ] `sys apply` with ripgrep usage produces binary in store (after obtaining real hashes)

---

## Phase 4: Additional Packages (fd, jq)

Implement fd and jq following the ripgrep pattern.

### Changes Required

**File**: `lua/syslua/pkgs/cli/fd.lua`
**Changes**: Implement fd package following ripgrep pattern

- Same module structure: `M.releases`, `M.meta`, `M.opts`, `M.setup()`
- fd releases from: `https://github.com/sharkdp/fd/releases`
- Version: `v10.3.0`
- Binary name: `fd` (or `fd.exe` on Windows)
- Archive format: `.tar.gz` on Unix, `.zip` on Windows
- **Note**: fd does NOT provide checksums - must download and generate SHA256 manually

**File**: `lua/syslua/pkgs/cli/jq.lua`
**Changes**: Implement jq package (different pattern - standalone binary)

- Same module structure: `M.releases`, `M.meta`, `M.opts`, `M.setup()`
- jq releases from: `https://github.com/jqlang/jq/releases`
- Version: `jq-1.8.1`
- Binary name: `jq` (or `jq.exe` on Windows)
- **IMPORTANT**: jq releases are standalone binaries, NOT archives
  - No extraction needed - just `ctx:fetch_url()` + chmod
  - Use `jq-macos-arm64`, `jq-macos-amd64`, `jq-linux-amd64`, `jq-windows-amd64.exe`
  - Linux build is glibc-based (no musl variant available)
  - Checksums available in combined `sha256sum.txt` file

### Success Criteria

#### Manual

- [x] `syslua.pkgs.cli.fd.setup()` returns BuildRef
- [x] `syslua.pkgs.cli.jq.setup()` returns BuildRef
- [x] Both packages have complete `M.releases`, `M.meta` exports
- [x] Error messages list available versions/platforms
- [x] jq package correctly handles standalone binary (no extraction)

---

## Phase 5: Integration Verification

Verify the complete system works end-to-end.

### Changes Required

**File**: `crates/cli/tests/fixtures/pkg_usage.lua` (new test fixture)
**Changes**: Create integration test fixture

```lua
local syslua = require('syslua')
local prio = require('syslua.priority')

-- Test: Package builds work
local rg = syslua.pkgs.cli.ripgrep.setup()

-- Test: Can use package output in bind
syslua.modules.env.setup({
  PATH = prio.before(rg.outputs.out),
})

-- Test: Metadata is accessible
assert(syslua.pkgs.cli.ripgrep.meta.name == 'ripgrep')
assert(syslua.pkgs.cli.ripgrep.meta.versions.stable == '15.1.0')
```

### Success Criteria

#### Automated

- [x] `cargo test -p syslua-cli` - All tests pass including new fixture

#### Manual

- [ ] `sys plan` with fixture shows ripgrep build in plan
- [ ] `sys apply` with fixture creates store entry and symlink (with real hashes)

---

## Testing Strategy

### Unit Tests

- No new Rust unit tests needed - this is pure Lua implementation

### Integration Tests

- New fixture `pkg_usage.lua` exercises the full flow
- Existing test infrastructure validates build/bind mechanics

### Manual Verification

1. Obtain real SHA256 hashes for all package releases
2. Run `sys apply` with test config using each package
3. Verify binaries exist in store and are executable
4. Verify error messages are helpful for missing versions/platforms

---

## Implementation Notes

### Obtaining SHA256 Hashes

During implementation, obtain hashes using these methods:

**ripgrep (15.1.0)**: Has individual `.sha256` sidecar files

```bash
# Download the checksum file directly
curl -L https://github.com/BurntSushi/ripgrep/releases/download/15.1.0/ripgrep-15.1.0-aarch64-apple-darwin.tar.gz.sha256
```

**fd (v10.3.0)**: No checksums provided - must generate manually

```bash
# Download and hash yourself
curl -L https://github.com/sharkdp/fd/releases/download/v10.3.0/fd-v10.3.0-aarch64-apple-darwin.tar.gz | sha256sum
```

**jq (jq-1.8.1)**: Combined `sha256sum.txt` file

```bash
# Download the combined checksums file
curl -L https://github.com/jqlang/jq/releases/download/jq-1.8.1/sha256sum.txt
```

### Platform Coverage

MVP covers these platforms:

- `aarch64-darwin` (Apple Silicon Mac)
- `x86_64-darwin` (Intel Mac)
- `x86_64-linux` (Linux x64)
- `x86_64-windows` (Windows x64)

Future: `aarch64-linux` support as packages provide ARM Linux builds.

### Version Strategy

- `M.meta.versions.stable` = current recommended version
- `M.meta.versions.latest` = newest available (may equal stable)
- Version aliases resolved in `setup()` before lookup

---

## Deviations from Plan

| Package | Plan Version | Actual Version | Reason |
|---------|--------------|----------------|--------|
| fd | v10.3.0 | v10.2.0 | v10.3.0 not available at implementation time |
| jq | jq-1.8.1 | 1.7.1 | jq-1.8.1 does not exist; 1.7.1 is latest stable |

**Impact**: None - these are simply the latest available versions at implementation time. Future updates can bump versions as needed.

---

## References

- Ticket: `thoughts/tickets/feature_standard-pkgs-system.md`
- Architecture: `docs/architecture/01-builds.md`
- Reference patterns: `lua/syslua/modules/file.lua`, `lua/syslua/modules/env.lua`
- Library helpers: `lua/syslua/lib/init.lua` (existing `fetch_url`, new `extract`)
- Type definitions: `lua/syslua/globals.d.lua`
