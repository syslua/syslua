# Programs Module Implementation Plan

## Overview

Implement a new `syslua.programs` namespace that binds packages to the target system (PATH, shell completions, man pages, config files), plus a new `modules.alias` module for declarative shell alias management.

## Beads Reference

- Issue: `syslua-oph`
- Ticket: `thoughts/tickets/feature_programs-module.md`

## Research Findings

### Existing Patterns

- **env.lua**: Uses `prio.mergeable()` for PATH accumulation, `id='__syslua_env'` + `replace=true` for build deduplication, shell-specific file generation, bind to rc files with BEGIN/END markers
- **file.lua**: `FileOptions = {target, source?, content?, mutable?}`. Each file is independent (no id+replace needed)
- **modules/init.lua**: Simple lazy-loading with `__index` metatable, `pcall(require)`, `rawset` for caching
- **ripgrep.lua**: Returns `{outputs: {bin, out}}` where `out` is extracted tarball directory

### Tarball Structures (Verified)

| Package | Binary | Completions | Man Page |
|---------|--------|-------------|----------|
| ripgrep | `rg` | `complete/{_rg, rg.bash, rg.fish, _rg.ps1}` | `doc/rg.1` |
| fd | `fd` | `autocomplete/{_fd, fd.bash, fd.fish, fd.ps1}` | `fd.1` (root) |
| jq | `jq` | None | None |

### Shell Completion Paths

| Shell | User Path | System Path | Setup Required |
|-------|-----------|-------------|----------------|
| bash | `~/.local/share/bash-completion/completions/` | `/usr/share/bash-completion/completions/` | None (auto-loaded) |
| zsh | `~/.zsh/completions/` | `/usr/local/share/zsh/site-functions/` | Add to fpath in .zshrc |
| fish | `~/.config/fish/completions/` | `/usr/share/fish/vendor_completions.d/` | None (auto-loaded) |
| powershell | Profile script | Profile script | Source in profile |

### Man Page Paths

| Platform | User Path | System Path |
|----------|-----------|-------------|
| macOS | `~/.local/share/man/man1/` | `/usr/local/share/man/man1/` |
| Linux | `~/.local/share/man/man1/` | `/usr/share/man/man1/` |
| Windows | N/A | N/A |

## Current State

- `syslua.pkgs.cli.*` exists for ripgrep, fd, jq
- `syslua.modules.env` manages PATH via shell rc injection
- `syslua.modules.file` manages config files
- `syslua.priority` provides merge/mergeable for accumulation
- No programs module
- No alias module

## Desired End State

```lua
local programs = require('syslua.programs')
local modules = require('syslua.modules')
local prio = require('syslua.priority')

-- Programs with shell integration
programs.ripgrep.setup({
  zsh_integration = true,
  fish_integration = true,
  config = {
    path = '~/.ripgreprc',
    content = '--smart-case\n--hidden',
  },
})

programs.fd.setup({ bash_integration = true })
programs.jq.setup({})  -- PATH only (no completions available)

-- Shell aliases
modules.alias.setup({
  ll = 'ls -la',
  gs = 'git status',
})
```

## What We're NOT Doing

- GUI applications
- Services/daemons (separate design)
- Updating pkgs to expose completion metadata (future enhancement)
- System-wide installation (use elevated mode)

---

## Phase 1: Alias Module

Create `lua/syslua/modules/alias.lua` following env.lua pattern exactly.

### Changes Required

**File**: `lua/syslua/modules/alias.lua` (new)

```lua
-- Structure mirrors env.lua exactly
local prio = require('syslua.priority')

local M = {}

-- Default opts - aliases accumulate via priority system
local default_opts = {}

M.opts = default_opts

-- Helper: generate shell-specific alias syntax
-- bash/zsh: alias name='command'
-- fish: alias name 'command'  
-- powershell: function name { command $args }

-- Build: generates syslua-alias.{sh,fish,ps1}
-- Uses id='__syslua_alias', replace=true

-- Binds: inject source line into shell rc files
-- Same BEGIN/END marker pattern as env.lua

M.setup = function(provided_opts)
  local new_opts, err = prio.merge(M.opts, provided_opts)
  if not new_opts then
    error(string.format('Failed to merge alias options: %s', err or 'unknown error'))
  end
  M.opts = new_opts
  
  local build = create_alias_build(M.opts)
  create_alias_binds(build)
end

return M
```

**Key implementation details**:

1. Each alias key-value pair in opts is a simple string (command)
2. `prio.merge` handles conflicts (same alias defined twice)
3. Build generates three files: `syslua-alias.sh`, `syslua-alias.fish`, `syslua-alias.ps1`
4. Binds source these files from shell rc (same pattern as env.lua)
5. Use `id='__syslua_alias'` + `replace=true` for accumulation

**Shell syntax differences**:

```bash
# bash/zsh (syslua-alias.sh)
alias ll='ls -la'
alias gs='git status'

# fish (syslua-alias.fish)
alias ll 'ls -la'
alias gs 'git status'

# powershell (syslua-alias.ps1)
function ll { ls -la $args }
function gs { git status $args }
```

Note: PowerShell uses functions instead of Set-Alias because Set-Alias only works for simple command mappings, not commands with arguments.

### Success Criteria

#### Automated

- [x] `cargo test` passes
- [x] `cargo clippy` clean

#### Manual

- [ ] `modules.alias.setup({ ll = 'ls -la' })` creates alias files
- [ ] Sourcing generated files works in each shell
- [ ] Multiple `setup()` calls accumulate aliases
- [ ] `prio.force()` overrides conflicting aliases
- [ ] `sys destroy` removes alias bindings

---

## Phase 2: Programs Infrastructure

Create the programs namespace with lazy-loading and shared helpers.

### Changes Required

**File**: `lua/syslua/programs/init.lua` (new)

```lua
-- Lazy-loading pattern (same as modules/init.lua)
local M = {}

setmetatable(M, {
  __index = function(t, k)
    local cached = rawget(t, k)
    if cached then return cached end
    
    local ok, mod = pcall(require, 'syslua.programs.' .. k)
    if ok then
      rawset(t, k, mod)
      return mod
    else
      error(string.format('Program not found: %s', k))
    end
  end,
})

return M
```

**File**: `lua/syslua/programs/helpers.lua` (new)

Shared utilities for all program modules:

```lua
local M = {}

-- Shell completion installation paths
function M.get_completion_paths()
  local home = sys.getenv('HOME')
  
  if sys.is_elevated then
    return {
      bash = sys.os == 'linux' and '/usr/share/bash-completion/completions/' 
             or '/usr/local/share/bash-completion/completions/',
      zsh = '/usr/local/share/zsh/site-functions/',
      fish = sys.os == 'linux' and '/usr/share/fish/vendor_completions.d/'
             or '/usr/local/share/fish/vendor_completions.d/',
    }
  else
    return {
      bash = home .. '/.local/share/bash-completion/completions/',
      zsh = home .. '/.zsh/completions/',
      fish = home .. '/.config/fish/completions/',
    }
  end
end

-- Man page installation paths
function M.get_man_paths()
  local home = sys.getenv('HOME')
  
  if sys.is_elevated then
    return {
      man1 = sys.os == 'linux' and '/usr/share/man/man1/' 
             or '/usr/local/share/man/man1/',
    }
  else
    return {
      man1 = home .. '/.local/share/man/man1/',
    }
  end
end

-- Create completion binds for a program
-- @param pkg_build BuildRef from pkg.setup()
-- @param name string Program name (e.g., 'rg', 'fd')
-- @param completions table {bash?: string, zsh?: string, fish?: string, ps1?: string}
-- @param opts table {bash_integration?, zsh_integration?, fish_integration?, powershell_integration?}
function M.create_completion_binds(pkg_build, name, completions, opts)
  -- Implementation: create sys.bind for each enabled shell
  -- Symlink from pkg output to completion install path
end

-- Create man page bind
-- @param pkg_build BuildRef
-- @param man_source string Path within pkg output (e.g., 'doc/rg.1')
-- @param man_name string Target filename (e.g., 'rg.1')
function M.create_man_bind(pkg_build, man_source, man_name)
  -- Implementation: symlink man page to man path
end

return M
```

### Success Criteria

#### Automated

- [x] `require('syslua.programs')` works
- [x] `require('syslua.programs.helpers')` works
- [x] Lazy-loading triggers on first access

#### Manual

- [ ] `programs.nonexistent` errors with "Program not found"

---

## Phase 3: Ripgrep Program

Full reference implementation with all features.

### Changes Required

**File**: `lua/syslua/programs/ripgrep.lua` (new)

```lua
local prio = require('syslua.priority')
local pkgs = require('syslua.pkgs')
local modules = require('syslua.modules')
local helpers = require('syslua.programs.helpers')

---@class syslua.programs.ripgrep
local M = {}

---@class RipgrepOptions
---@field version? string
---@field bash_integration? boolean
---@field zsh_integration? boolean
---@field fish_integration? boolean
---@field powershell_integration? boolean
---@field config? FileOpts

local default_opts = {
  version = prio.default('stable'),
  bash_integration = prio.default(false),
  zsh_integration = prio.default(false),
  fish_integration = prio.default(false),
  powershell_integration = prio.default(false),
}

M.opts = default_opts

-- Ripgrep-specific paths within extracted tarball
local COMPLETIONS = {
  bash = 'complete/rg.bash',
  zsh = 'complete/_rg',
  fish = 'complete/rg.fish',
  ps1 = 'complete/_rg.ps1',
}
local MAN_PAGE = 'doc/rg.1'

M.setup = function(provided_opts)
  local new_opts, err = prio.merge(M.opts, provided_opts)
  if not new_opts then
    error(string.format('Failed to merge ripgrep options: %s', err or 'unknown error'))
  end
  M.opts = new_opts
  
  -- 1. Get the package build
  local pkg_build = pkgs.cli.ripgrep.setup({ version = M.opts.version })
  
  -- 2. Register binary with PATH via env module
  modules.env.setup({
    PATH = prio.before(pkg_build.outputs.out),
  })
  
  -- 3. Shell completions (if enabled)
  helpers.create_completion_binds(pkg_build, 'rg', COMPLETIONS, M.opts)
  
  -- 4. Man page
  helpers.create_man_bind(pkg_build, MAN_PAGE, 'rg.1')
  
  -- 5. Config file (if provided)
  if M.opts.config then
    modules.file.setup(M.opts.config)
  end
end

return M
```

**Key implementation details**:

1. Uses `id='__syslua_programs_ripgrep'` + `replace=true` pattern in any internal builds
2. Calls `pkgs.cli.ripgrep.setup()` to get the package
3. Integrates with `modules.env` for PATH (using `prio.before()` to prepend)
4. Creates separate binds for each enabled shell completion
5. Creates bind for man page
6. Passes config to `modules.file.setup()` if provided

**Completion bind pattern**:

```lua
sys.bind({
  id = '__syslua_programs_ripgrep_zsh_completion',
  replace = true,
  inputs = {
    pkg_build = pkg_build,
    source = pkg_build.outputs.out .. '/complete/_rg',
    target = completion_paths.zsh .. '_rg',
  },
  create = function(inputs, ctx)
    -- mkdir -p target dir
    -- symlink source -> target
    return { link = inputs.target }
  end,
  destroy = function(outputs, ctx)
    -- rm symlink
  end,
})
```

### Success Criteria

#### Automated

- [x] `cargo test` passes
- [x] `cargo clippy` clean
- [x] No LuaLS type errors

#### Manual

- [ ] `programs.ripgrep.setup({})` installs rg to PATH
- [ ] `which rg` returns syslua store path after shell reload
- [ ] `programs.ripgrep.setup({ zsh_integration = true })` installs zsh completions
- [ ] `rg <TAB>` shows completions in zsh after reload
- [ ] `man rg` works after setup
- [ ] `programs.ripgrep.setup({ config = { path = '~/.ripgreprc', content = '...' } })` creates config
- [ ] Multiple `setup()` calls accumulate options
- [ ] `sys destroy` removes all bindings

---

## Phase 4: fd and jq Programs

Apply the pattern with per-package structural differences.

### Changes Required

**File**: `lua/syslua/programs/fd.lua` (new)

```lua
-- Same pattern as ripgrep.lua, but with fd-specific paths:
local COMPLETIONS = {
  bash = 'autocomplete/fd.bash',  -- Note: 'autocomplete' not 'complete'
  zsh = 'autocomplete/_fd',
  fish = 'autocomplete/fd.fish',
  ps1 = 'autocomplete/fd.ps1',
}
local MAN_PAGE = 'fd.1'  -- Note: root level, not doc/
```

**File**: `lua/syslua/programs/jq.lua` (new)

```lua
-- Simpler: no completions, no man page in release
-- Just PATH integration

M.setup = function(provided_opts)
  local new_opts, err = prio.merge(M.opts, provided_opts)
  if not new_opts then
    error(string.format('Failed to merge jq options: %s', err or 'unknown error'))
  end
  M.opts = new_opts
  
  local pkg_build = pkgs.cli.jq.setup({ version = M.opts.version })
  
  modules.env.setup({
    PATH = prio.before(pkg_build.outputs.out),
  })
  
  -- No completions or man page available in jq releases
  
  if M.opts.config then
    modules.file.setup(M.opts.config)
  end
end
```

### Success Criteria

#### Automated

- [x] `cargo test` passes
- [x] `cargo clippy` clean

#### Manual

- [ ] `programs.fd.setup({ fish_integration = true })` works
- [ ] `fd <TAB>` shows completions in fish
- [ ] `man fd` works
- [ ] `programs.jq.setup({})` installs jq to PATH
- [ ] `which jq` returns syslua store path

---

## Testing Strategy

### Unit Tests

- Priority merge behavior for program options
- Shell-specific alias syntax generation
- Completion path resolution per platform/privilege

### Integration Tests

- Full `programs.ripgrep.setup()` cycle
- Multiple `setup()` calls accumulate correctly
- `sys destroy` cleans up all bindings

### Manual Verification

1. Fresh shell after `sys apply`
2. Verify `which <tool>` returns store path
3. Verify `<tool> <TAB>` shows completions
4. Verify `man <tool>` works
5. Verify config files created
6. Run `sys destroy` and verify cleanup

---

## File Summary

| File | Action | Description |
|------|--------|-------------|
| `lua/syslua/modules/alias.lua` | Create | Alias module following env.lua pattern |
| `lua/syslua/programs/init.lua` | Create | Lazy-loading namespace |
| `lua/syslua/programs/helpers.lua` | Create | Shared completion/man utilities |
| `lua/syslua/programs/ripgrep.lua` | Create | Reference implementation |
| `lua/syslua/programs/fd.lua` | Create | fd program |
| `lua/syslua/programs/jq.lua` | Create | jq program (PATH only) |

## References

- Ticket: `thoughts/tickets/feature_programs-module.md`
- env.lua pattern: `lua/syslua/modules/env.lua`
- Priority system: `lua/syslua/priority.lua`
- Lazy-loading: `lua/syslua/modules/init.lua`
