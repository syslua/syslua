---
beads_id: syslua-oph
type: feature
priority: 2
created: 2026-01-07
status: implemented
keywords:
  - programs
  - modules
  - pkgs
  - ripgrep
  - shell completions
  - man pages
  - PATH
  - env integration
  - aliases
  - priority system
patterns:
  - M.setup module idiom
  - prio.merge for options
  - prio.mergeable for accumulating values
  - sys.build and sys.bind
  - lazy-loading __index metatable
---

# Implement programs module for system-binding packages

## Description

Create a new top-level `syslua.programs` namespace that binds packages (from `syslua.pkgs`) to the target system, plus a new `modules.alias` module for shell aliases. This is analogous to NixOS's `programs.*` configuration, where users can enable programs with configurable options.

The programs module bridges the gap between:

- **pkgs**: Define how to fetch/build software (returns `BuildRef` with `outputs.bin`)
- **programs**: Bind that software to the system (PATH, completions, man pages, config files)
- **modules.alias**: Manage shell aliases across shells (new)

## Context

Currently, users must manually:

1. Call `pkgs.cli.ripgrep.setup()` to get the build
2. Manually integrate with `modules.env` for PATH
3. Handle shell completions themselves
4. Manage config files separately
5. No way to declaratively manage shell aliases

The programs module provides a unified interface that handles all of this declaratively.

**Business impact**: Makes syslua's UX match NixOS's ergonomics for program configuration, which is a key selling point for the project.

## Requirements

### Functional

#### Priority System (CRITICAL - applies to ALL modules)

All modules MUST use the priority system to support multiple `M.setup` calls that accumulate inputs:

```lua
-- Pattern from modules/env.lua - MUST follow this exactly
local prio = require('syslua.priority')

local M = {}

local default_opts = {
  -- Use prio.mergeable() for fields that accumulate across setup calls
  some_list = prio.mergeable({ separator = ':' }),
}

M.opts = default_opts

M.setup = function(provided_opts)
  -- prio.merge accumulates mergeable fields, resolves conflicts for others
  local new_opts, err = prio.merge(M.opts, provided_opts)
  if not new_opts then
    error(string.format('Failed to merge options: %s', err or 'unknown error'))
  end
  M.opts = new_opts
  
  -- Create builds/binds using M.opts
end
```

#### Build ID + Replace Pattern (CRITICAL for accumulation)

All `sys.build()` calls MUST use a unique `id` with `replace = true` to ensure multiple `M.setup` calls replace rather than duplicate:

```lua
-- From env.lua - this pattern is REQUIRED
return sys.build({
  id = '__syslua_env',      -- Unique ID for this module's build
  replace = true,            -- Replace previous build with same ID
  inputs = { ... },
  create = function(inputs, ctx) ... end,
})
```

**Why this matters:**

- Without `id` + `replace`: Each `M.setup` call creates a NEW build (duplicates)
- With `id` + `replace`: Each `M.setup` call REPLACES the previous build (accumulation works)

**Naming convention for IDs:**

- `__syslua_env` - env module
- `__syslua_alias` - alias module  
- `__syslua_programs_ripgrep` - ripgrep program
- `__syslua_programs_fd` - fd program

This enables:

```lua
-- Multiple calls accumulate rather than overwrite
programs.ripgrep.setup({ zsh_integration = true })
programs.ripgrep.setup({ config = { path = '~/.ripgreprc', content = '...' } })
-- Both zsh_integration AND config are applied
```

#### Core Structure - Programs

- [ ] Create `lua/syslua/programs/` directory structure
- [ ] Create `lua/syslua/programs/init.lua` with lazy-loading `__index` metatable (same pattern as `pkgs/init.lua` and `modules/init.lua`)
- [ ] Each program gets its own file: `lua/syslua/programs/ripgrep.lua`

#### Program Module Pattern (using ripgrep as reference implementation)

- [ ] Follow `M.setup(opts)` idiom from modules
- [ ] Use `prio.merge` for option merging (supports multiple setup calls)
- [ ] Use `prio.mergeable()` for fields that should accumulate
- [ ] Internally call the corresponding pkg's `setup()` to get the `BuildRef`

#### Bindings (each as separate sys.bind)

- [ ] **PATH binding**: Register all binaries with `modules.env` for PATH integration
- [ ] **Shell completions**: Enabled via `<shellname>_integration` option (e.g., `bash_integration`, `zsh_integration`, `fish_integration`, `powershell_integration`)
- [ ] **Man pages**: Bind man pages if present in tarball, skip gracefully if missing
- [ ] **Config files**: Accept `config` option as `FileOpts` to pass to `modules.file.setup()` internally

#### Options Schema (ripgrep example)

```lua
---@class RipgrepOptions
---@field version? string Version to install (default: 'stable')
---@field bash_integration? boolean Enable bash completions
---@field zsh_integration? boolean Enable zsh completions  
---@field fish_integration? boolean Enable fish completions
---@field powershell_integration? boolean Enable PowerShell completions
---@field config? FileOpts Config file options (passed to file.setup)

programs.ripgrep.setup({
  version = 'stable',
  zsh_integration = true,
  fish_integration = true,
  config = {
    path = '~/.ripgreprc',
    content = '--smart-case\n--hidden\n--glob=!.git',
  },
})
```

#### Multiple Binaries

- [ ] Expose all binaries from a package (not just primary)
- [ ] Register all binaries with PATH

#### Alias Module (modules.alias)

Create `lua/syslua/modules/alias.lua` - heavily inspired by `modules/env.lua`:

- [ ] Same structure as env.lua (M.opts, M.setup with prio.merge)
- [ ] Use `prio.mergeable()` for alias accumulation across multiple setup calls
- [ ] Generate shell-specific alias files (bash/zsh, fish, powershell)
- [ ] Bind to shell rc files with BEGIN/END markers (same pattern as env.lua)
- [ ] Support per-shell alias syntax differences

```lua
---@class AliasOptions
---@field [string] string | priority.PriorityValue<string> Alias name -> command

local modules = require('syslua.modules')

-- Multiple calls accumulate
modules.alias.setup({
  ll = 'ls -la',
  gs = 'git status',
})

modules.alias.setup({
  gd = 'git diff',
  -- Can use priority to override
  ll = prio.force('exa -la'),  -- Override previous ll
})
```

Shell output examples:

```bash
# bash/zsh (syslua-alias.sh)
alias ll='ls -la'
alias gs='git status'

# fish (syslua-alias.fish)
alias ll 'ls -la'
alias gs 'git status'

# powershell (syslua-alias.ps1)
Set-Alias -Name ll -Value 'ls -la'
function gs { git status $args }
```

### Non-Functional

- [ ] Cross-platform: Windows, macOS, Linux
- [ ] Follow existing code patterns strictly (disciplined codebase)
- [ ] LuaCATS type annotations for all public APIs
- [ ] No breaking changes to existing pkgs or modules

## Current State

- `syslua.pkgs.cli.ripgrep` exists and returns `BuildRef` with `outputs.bin` and `outputs.out`
- `syslua.modules.env` exists and manages PATH via shell rc file injection
- `syslua.modules.file` exists for managing config files
- `syslua.priority` exists with full merge/mergeable support
- No unified way to "enable a program" with all its integrations
- No way to declaratively manage shell aliases

## Desired State

```lua
-- User's init.lua
local programs = require('syslua.programs')
local modules = require('syslua.modules')
local prio = require('syslua.priority')

-- Programs with shell integration
programs.ripgrep.setup({
  zsh_integration = true,
  config = {
    path = '~/.ripgreprc',
    content = '--smart-case\n--hidden',
  },
})

programs.fd.setup({
  fish_integration = true,
})

programs.jq.setup({})  -- Just install and add to PATH

-- Shell aliases (accumulates across calls)
modules.alias.setup({
  ll = 'ls -la',
  gs = 'git status',
  rg = 'rg --smart-case',  -- Alias for installed program
})

-- Can call again from different file, values accumulate
modules.alias.setup({
  gd = 'git diff',
  ll = prio.force('exa -la'),  -- Override with priority
})
```

This:

1. Fetches/builds packages via `pkgs.cli.*`
2. Registers binaries with `modules.env` for PATH
3. Installs shell completions to appropriate locations
4. Creates config files via `modules.file`
5. Manages shell aliases via `modules.alias`

## Research Context

### Keywords to Search

- `programs` - NixOS programs module pattern
- `M.setup` - existing module setup pattern
- `prio.merge` - option merging
- `prio.mergeable` - accumulating values
- `sys.bind` - system binding creation
- `modules.env` - PATH integration and alias module template
- `modules.file` - config file management
- `shell completions` - bash/zsh/fish/powershell completion installation
- `shell aliases` - per-shell alias syntax

### Patterns to Investigate

- How `modules.env` registers PATH entries (for integration)
- How `modules.env` uses prio.mergeable for PATH accumulation
- How `modules.file` handles FileOpts (for config passthrough)
- Shell completion file locations per shell and privilege level
- Man page installation paths per platform
- Fish alias syntax (`alias name 'command'` vs bash `alias name='command'`)
- PowerShell alias limitations (Set-Alias vs functions for complex commands)

### Key Decisions Made

- **Top-level namespace**: `syslua.programs` (parallel to `pkgs`, `modules`)
- **No enable flag**: Calling `setup()` = enabled
- **Shell integration opt-in**: Via `<shell>_integration` boolean options
- **Config via file module**: Pass `FileOpts` to `modules.file.setup()` internally
- **Alias module included**: `modules.alias` follows env.lua pattern exactly
- **Priority system required**: All modules use `prio.merge`/`prio.mergeable` for accumulation
- **Man pages best-effort**: Skip if not in tarball (pkgs will be updated later)

## Implementation Strategy

### Phase 1: Alias Module

1. Create `lua/syslua/modules/alias.lua` following env.lua pattern exactly
2. Implement shell-specific alias generation (bash/zsh, fish, powershell)
3. Implement binds to shell rc files with markers
4. Test accumulation across multiple setup calls
5. Test priority conflict resolution

### Phase 2: Ripgrep Reference Implementation

1. Create `lua/syslua/programs/init.lua` with lazy-loading
2. Create `lua/syslua/programs/ripgrep.lua` with full implementation
3. Implement PATH binding via env module integration
4. Implement shell completion bindings (all 4 shells)
5. Implement config file passthrough to file module
6. Ensure multiple setup calls accumulate correctly
7. Test thoroughly on macOS and Linux

### Phase 3: Generalize Pattern

1. Extract common helpers (completion paths, man paths)
2. Implement `programs.fd`
3. Implement `programs.jq`
4. Document the pattern for adding new programs

### Phase 4: Remaining CLI Tools

1. Port remaining `pkgs.cli.*` to programs

## Success Criteria

### Automated

- [ ] `cargo test` passes
- [ ] `cargo clippy` clean
- [ ] Lua type annotations valid (no LuaLS errors)

### Manual

- [ ] `modules.alias.setup({ ll = 'ls -la' })` creates aliases in all shells
- [ ] Multiple `modules.alias.setup` calls accumulate aliases
- [ ] `prio.force()` correctly overrides conflicting aliases
- [ ] `programs.ripgrep.setup({ zsh_integration = true })` installs rg to PATH and zsh completions
- [ ] Multiple `programs.*.setup` calls accumulate options
- [ ] Completions work in target shell after reload
- [ ] Config file created when `config` option provided
- [ ] `sys destroy` cleanly removes all bindings
- [ ] Works on macOS, Linux, and Windows

## Out of Scope

- GUI applications
- Services/daemons (separate design)
- System-wide installation (use elevated env module)
- Updating pkgs to include missing completions/man pages (separate task)

## Related Files

- `lua/syslua/pkgs/cli/ripgrep.lua` - Package definition
- `lua/syslua/pkgs/cli/fd.lua` - Package definition
- `lua/syslua/pkgs/cli/jq.lua` - Package definition
- `lua/syslua/modules/env.lua` - PATH management reference AND alias module template
- `lua/syslua/modules/file.lua` - Config file management reference
- `lua/syslua/modules/init.lua` - Lazy-loading pattern reference
- `lua/syslua/pkgs/init.lua` - Lazy-loading pattern reference
- `lua/syslua/priority.lua` - Priority system (prio.merge, prio.mergeable)
