# Implementation Plan: syslua.environment.packages

**Spec:** [2026-01-08-environment-packages-design.md](../specs/2026-01-08-environment-packages-design.md)  
**Created:** 2026-01-08  
**Status:** Draft

## Summary

Implement `syslua.environment.packages`, a module that takes a list of packages and creates a unified bin directory at `~/.syslua/bin` with symlinks to all package binaries, plus optional shell integration.

## Context Analysis

### Existing Patterns to Follow

| Pattern           | Source                                 | Application                                                                |
| ----------------- | -------------------------------------- | -------------------------------------------------------------------------- |
| Module structure  | `lua/syslua/environment/variables.lua` | Lazy-loaded module with `setup()` API                                      |
| Priority system   | `lua/syslua/priority.lua`              | Wrap packages with `prio.before()`/`prio.after()` for collision resolution |
| Shell integration | `environment/variables.lua:291-420`    | Markers pattern (`BEGIN/END SYSLUA`) for shell configs                     |
| Package outputs   | `pkgs/cli/ripgrep.lua`                 | `BuildRef` with `outputs.bin`, `outputs.man`, `outputs.completions`        |
| Symlink binds     | `lib/programs.lua`                     | `sys.bind()` with `ln -sf` via `ctx:exec()`                                |

### Key Constraints

1. **No dedicated symlink abstraction** - Use shell commands via `ctx:exec()`
2. **Windows requires privileges** - Symlinks need Developer Mode or Admin; spec says fall back to junctions/hardlinks/copy
3. **Hermetic builds** - `ctx:exec()` runs with `PATH=/path-not-set`, must use full paths
4. **Cross-platform paths** - Use `sys.path.join()` everywhere

## Implementation Phases

The spec recommends: Phases 1-2 first (user-facing), then 3-5 (build infrastructure).

---

## Phase 1: Core Symlink Forest

**Goal:** Create `~/.syslua/bin/` with symlinks to package binaries.

### 1.1 Create Module Structure

**File:** `lua/syslua/environment/packages.lua`

```
lua/syslua/environment/
├── init.lua          # Add @field packages
├── packages.lua      # NEW - Main implementation
└── variables.lua     # Existing
```

**Changes to `init.lua`:**

- Add `---@field packages syslua.environment.packages` to class definition

### 1.2 Define API Types

```lua
---@class syslua.environment.packages
local M = {}

---@class syslua.environment.packages.Options
---@field use BuildRef[] List of packages to include
---@field link? syslua.environment.packages.LinkOptions
---@field shell_integration? boolean

---@class syslua.environment.packages.LinkOptions
---@field bin? boolean Default true
---@field man? boolean Default true
---@field completions? boolean|string[] Default true, or list of shells

local default_opts = {
  use = {},
  link = {
    bin = true,
    man = true,
    completions = true,
  },
  shell_integration = true,
}
```

### 1.3 Implement Package Resolution

**Input:** List of `BuildRef` (possibly wrapped with priority)

**Steps:**

1. Unwrap priority values to get raw `BuildRef`
2. Extract priority level for each package
3. For each package, read its outputs (`bin`, `man`, `completions`)
4. Build collision map: `{ binary_name -> [{pkg, priority, path}, ...] }`
5. Resolve collisions using priority (lower wins)
6. Error on same-priority conflicts with helpful message

**Collision Error Format (from spec):**

```
Priority conflict in 'ls'

  Conflicting packages at same priority level (default: 1000):

  Package: eza (from syslua.pkgs.cli.eza)
    Provides: ls

  Package: gnu-coreutils (from syslua.pkgs.cli.gnu_coreutils)
    Provides: ls

  Resolution options:
  1. Use prio.before(pkg) to make one package win
  2. Use prio.after(pkg) to make one package lose
  3. Remove one of the conflicting packages
```

### 1.4 Implement Symlink Forest Build

**Build Phase:** Create environment directory in store

```lua
local env_build = sys.build({
  id = '__syslua_env_packages',
  replace = true,
  inputs = {
    packages = resolved_packages,  -- [{name, bin_path, priority}, ...]
    link_opts = opts.link,
  },
  create = function(inputs, ctx)
    -- Create directory structure
    -- ~/.syslua/store/env/<hash>/bin/
    -- ~/.syslua/store/env/<hash>/share/man/man1/
    -- etc.

    -- For each binary, create symlink
    for _, pkg in ipairs(inputs.packages) do
      if pkg.bin then
        -- Symlink: env/bin/<name> -> /store/build/<hash>/bin/<name>
      end
    end

    return {
      bin = ctx.out .. '/bin',
      man = ctx.out .. '/share/man',
      completions = ctx.out .. '/share/completions',
    }
  end,
})
```

### 1.5 Implement Atomic Swap Bind

**Bind Phase:** Atomically swap `~/.syslua/bin` to point to new env

```lua
sys.bind({
  id = '__syslua_env_packages_link',
  replace = true,
  inputs = {
    env_build = env_build,
    target_bin = home .. '/.syslua/bin',
  },
  create = function(inputs, ctx)
    -- First run: create symlink directly
    -- Subsequent: atomic swap via temp symlink + rename
    if sys.os == 'windows' then
      -- Junction or directory symlink
    else
      -- ln -sfn for atomic directory symlink update
    end
    return { link = inputs.target_bin }
  end,
  destroy = function(outputs, ctx)
    -- Remove symlink
  end,
})
```

### 1.6 Platform-Specific Symlink Handling

| Platform    | Directory Link         | File Link               | Fallback    |
| ----------- | ---------------------- | ----------------------- | ----------- |
| Linux/macOS | `ln -sfn`              | `ln -sf`                | None needed |
| Windows     | `mklink /D` (junction) | `mklink /H` (hard link) | `copy`      |

**Windows Detection:**

```lua
local function create_link(source, target, is_dir)
  if sys.os == 'windows' then
    if is_dir then
      -- Try junction first (no admin needed for own dirs)
      ctx:exec({ bin = 'cmd.exe', args = { '/c', 'mklink /J "' .. target .. '" "' .. source .. '"' } })
    else
      -- Hard link for files
      ctx:exec({ bin = 'cmd.exe', args = { '/c', 'mklink /H "' .. target .. '" "' .. source .. '"' } })
    end
  else
    local flag = is_dir and '-sfn' or '-sf'
    ctx:exec({ bin = '/bin/ln', args = { flag, source, target } })
  end
end
```

### 1.7 Binary Discovery

**Challenge:** Package's `outputs.bin` can be a file OR directory

**Algorithm:**

```lua
local function discover_binaries(pkg)
  local bin_output = pkg.outputs.bin
  if not bin_output then return {} end

  -- Check if it's a file or directory
  -- If file: single binary with basename
  -- If directory: list all executables inside

  -- This needs to happen at build time, so use ctx:exec to list
end
```

**Implementation:**

- In the build's `create` function, use `ls` or `dir` to discover binaries
- Store the list in build outputs for the bind phase

---

## Phase 2: Shell Integration

**Goal:** Auto-add `~/.syslua/bin` to PATH in shell configs.

### 2.1 Reuse Existing Pattern

Follow `environment/variables.lua` markers pattern exactly:

```lua
local BEGIN_MARKER = '# BEGIN SYSLUA PACKAGES'
local END_MARKER = '# END SYSLUA PACKAGES'
```

### 2.2 Shell-Specific PATH Addition

| Shell      | Config File                  | Content                                     |
| ---------- | ---------------------------- | ------------------------------------------- |
| Bash       | `~/.bashrc`                  | `export PATH="$HOME/.syslua/bin:$PATH"`     |
| Zsh        | `~/.zshenv`                  | `export PATH="$HOME/.syslua/bin:$PATH"`     |
| Fish       | `~/.config/fish/config.fish` | `fish_add_path ~/.syslua/bin`               |
| PowerShell | `$PROFILE`                   | `$env:PATH = "$HOME/.syslua/bin;$env:PATH"` |

### 2.3 Opt-Out Handling

When `shell_integration = false`:

```lua
if not opts.shell_integration then
  -- Print instructions to user (via sys.log or similar)
  print([[
Shell integration disabled. Add to your shell config:

  Bash/Zsh: export PATH="$HOME/.syslua/bin:$PATH"
  Fish:     fish_add_path ~/.syslua/bin
  PowerShell: $env:PATH = "$HOME/.syslua/bin;$env:PATH"
]])
end
```

### 2.4 Completions and Man Pages

**Completions linking:**

- If `link.completions = true`, link all shells
- If `link.completions = {'zsh', 'bash'}`, link only those
- Use extension detection from spec (`.bash`, `.zsh`/`_*`, `.fish`, `.ps1`)

**Man pages linking:**

- Detect section from filename (`rg.1` → `man1/`)
- Link to `~/.syslua/share/man/manN/`

---

## Phase 3: `ctx:wrap_binary()` (Build Infrastructure)

**Goal:** Create wrapper scripts for binary isolation (primary mechanism on Windows).

### 3.1 Register Build Context Method

**File:** `lua/syslua/init.lua` (or separate `lib/isolation.lua`)

```lua
sys.register_build_ctx_method('wrap_binary', function(ctx, opts)
  -- opts.binary: path to real binary
  -- opts.env: environment variables to set

  local wrapper_content
  if sys.os == 'windows' then
    wrapper_content = generate_cmd_wrapper(opts)
  else
    wrapper_content = generate_sh_wrapper(opts)
  end

  -- Rename original to .real
  -- Write wrapper in its place
end)
```

### 3.2 Wrapper Templates

**Unix (.sh):**

```bash
#!/bin/sh
export PATH="/store/build/<hash>/bin:$PATH"
export LD_LIBRARY_PATH="/store/build/<hash>/lib"
exec "/store/build/<hash>/bin/mytool.real" "$@"
```

**Windows (.cmd):**

```batch
@echo off
set PATH=%~dp0..\deps\openssl\bin;%PATH%
"%~dp0mytool.real.exe" %*
```

---

## Phase 4: `ctx:patch_rpath()` (Build Infrastructure)

**Goal:** Patch ELF/Mach-O binaries to find libraries at store paths.

### 4.1 Platform Detection

```lua
sys.register_build_ctx_method('patch_rpath', function(ctx, deps)
  if sys.os == 'windows' then
    return -- No-op, use wrap_binary instead
  end

  -- Find all binaries in ctx.out
  -- For each binary:
  if sys.os == 'linux' then
    -- Use patchelf
    ctx:exec({ bin = 'patchelf', args = { '--set-rpath', rpath, binary } })
  elseif sys.os == 'darwin' then
    -- Use install_name_tool
    for _, dep in pairs(deps) do
      ctx:exec({
        bin = '/usr/bin/install_name_tool',
        args = { '-add_rpath', dep.outputs.lib, binary }
      })
    end
  end
end)
```

### 4.2 Dependency: patchelf Package

For Linux, `patchelf` must be available. Options:

1. Bootstrap `patchelf` as a core package
2. Require user to have it installed (temporary)
3. Bundle a static binary

---

## Phase 5: `ctx:patch_shebang()` (Build Infrastructure)

**Goal:** Rewrite script shebangs to use store paths.

### 5.1 Implementation

```lua
sys.register_build_ctx_method('patch_shebang', function(ctx, interpreter)
  -- Find all scripts in ctx.out (files starting with #!)
  -- Replace shebang line with store path

  ctx:exec({
    bin = '/bin/sh',
    args = {
      '-c',
      string.format([[
find "%s" -type f -exec grep -l '^#!' {} \; | while read f; do
  sed -i.bak '1s|^#!.*|#!%s|' "$f" && rm -f "$f.bak"
done
]], ctx.out, interpreter)
    },
  })
end)
```

---

## File Changes Summary

### New Files

| File                                  | Purpose                    |
| ------------------------------------- | -------------------------- |
| `lua/syslua/environment/packages.lua` | Main module implementation |

### Modified Files

| File                              | Changes                                                            |
| --------------------------------- | ------------------------------------------------------------------ |
| `lua/syslua/environment/init.lua` | Add `@field packages`                                              |
| `lua/syslua/init.lua`             | Register `ctx:wrap_binary`, `ctx:patch_rpath`, `ctx:patch_shebang` |

### Test Files

| File                                              | Purpose           |
| ------------------------------------------------- | ----------------- |
| `tests/integration/environment_packages_test.lua` | Integration tests |
| `tests/fixtures/env_packages_*.lua`               | Test fixtures     |

---

## Implementation Order

```
Phase 1.1 ──► Phase 1.2 ──► Phase 1.3 ──► Phase 1.4 ──► Phase 1.5 ──► Phase 1.6 ──► Phase 1.7
    │                           │
    │                           └──────────────────────────────────────────────────────────────┐
    ▼                                                                                          ▼
Phase 2.1 ──► Phase 2.2 ──► Phase 2.3 ──► Phase 2.4                                      [MVP Complete]
                                                                                               │
                                                                                               ▼
Phase 3.1 ──► Phase 3.2                                                              [Windows Support]
    │
    ▼
Phase 4.1 ──► Phase 4.2                                                              [Source Builds]
    │
    ▼
Phase 5.1                                                                            [Script Isolation]
```

**MVP (Phases 1-2):** ~400-600 lines of Lua
**Full Implementation (Phases 1-5):** ~800-1000 lines of Lua

---

## Testing Strategy

### Unit Tests

1. **Priority resolution** - Collision detection, priority ordering
2. **Binary discovery** - File vs directory outputs
3. **Platform detection** - Correct commands per OS

### Integration Tests

1. **Basic usage** - Single package, verify symlink created
2. **Multiple packages** - Verify all binaries linked
3. **Collision handling** - Priority resolution works
4. **Shell integration** - Markers added to shell configs
5. **Atomic updates** - Swap works without breaking PATH

### Platform-Specific Tests

1. **Linux** - Symlinks work
2. **macOS** - Symlinks work
3. **Windows** - Junctions/hardlinks/copy fallback

---

## Open Questions from Spec

| Question                | Recommendation                       |
| ----------------------- | ------------------------------------ |
| GC for old generations  | Count-based: keep last 3 generations |
| System-wide support     | Defer to future work                 |
| Profile switching       | Defer to future work                 |
| Transitive dependencies | Don't link transitives by default    |

---

## Risk Assessment

| Risk                        | Mitigation                                                |
| --------------------------- | --------------------------------------------------------- |
| Windows symlink privileges  | Implement junction/hardlink/copy fallback                 |
| Binary discovery complexity | Keep it simple: file = single binary, dir = list contents |
| Shell config corruption     | Use markers pattern (proven in `variables.lua`)           |
| Atomic swap failure         | Use temp symlink + rename (atomic on POSIX)               |

---

## Success Criteria

1. User can call `environment.packages.setup({ use = { pkgs.cli.ripgrep } })`
2. `~/.syslua/bin/rg` exists and is executable
3. `~/.syslua/bin` is in PATH after shell restart
4. Priority conflicts produce clear error messages
5. Works on Linux, macOS, and Windows
