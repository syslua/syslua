# syslua.environment.packages Design

**Date:** 2026-01-08  
**Status:** Draft  
**Authors:** Ian Pascoe, Claude

## Overview

This document describes the design for `syslua.environment.packages`, a module that takes a list of packages and automatically adds them to the user's PATH, similar to Nix's `environment.systemPackages`.

### Goals

1. **Reduce boilerplate** - One-liner instead of per-package PATH setup
2. **Declarative package lists** - Separate "these packages should be available" from program-specific config
3. **Unified bin directory** - Single `~/.syslua/bin` with symlinks to all package binaries
4. **Cross-platform** - Works on Linux, macOS, and Windows
5. **Atomic updates** - Symlink swap for safe updates with rollback support

### Non-Goals

- Package compilation (handled by `sys.build()`)
- Package discovery/registry (handled by `syslua.pkgs.*`)
- Per-project environments (future work)

## API

```lua
local environment = require('syslua.environment')
local pkgs = require('syslua.pkgs')
local prio = require('syslua.priority')

environment.packages.setup({
  -- Packages to include (required)
  use = {
    pkgs.cli.ripgrep,
    pkgs.cli.fd,
    pkgs.cli.jq,
    prio.before(pkgs.cli.eza), -- wins collisions against default priority
  },

  -- What to link (optional, all default to true)
  link = {
    bin = true,
    man = true,
    completions = true,
  },

  -- Shell integration (optional, default true)
  shell_integration = true,
})
```

### Parameters

| Parameter           | Type                | Default  | Description                                             |
| ------------------- | ------------------- | -------- | ------------------------------------------------------- |
| `use`               | `BuildRef[]`        | required | List of packages (BuildRefs) to include                 |
| `link.bin`          | `boolean`           | `true`   | Link binaries to `~/.syslua/bin/`                       |
| `link.man`          | `boolean`           | `true`   | Link man pages to `~/.syslua/share/man/`                |
| `link.completions`  | `boolean\|string[]` | `true`   | Link completions; can specify shells: `{'zsh', 'bash'}` |
| `shell_integration` | `boolean`           | `true`   | Auto-add PATH to shell configs                          |

## Package Output Convention

Packages declare their outputs with semantic keys. The key name determines where the output is linked, not the structure.

```lua
-- In package setup(), return BuildRef with outputs:
return sys.build({
  create = function(inputs, ctx)
    -- ... build logic ...
    return {
      out = ctx.out, -- always present
      bin = ctx.out .. '/rg', -- file OR directory
      man = ctx.out .. '/doc/rg.1', -- file OR directory
      completions = ctx.out .. '/complete/', -- file OR directory
      lib = ctx.out .. '/lib/', -- optional, for libraries
      include = ctx.out .. '/include/', -- optional, for headers
    }
  end,
})
```

### Output Key Semantics

| Key           | Links To                      | Notes                                                        |
| ------------- | ----------------------------- | ------------------------------------------------------------ |
| `bin`         | `~/.syslua/bin/`              | File → direct symlink; Directory → symlink contents          |
| `man`         | `~/.syslua/share/man/man{N}/` | Section auto-detected from filename (e.g., `rg.1` → `man1/`) |
| `completions` | Shell-specific dirs           | Extension-based detection (see below)                        |
| `lib`         | `~/.syslua/lib/`              | For source builds with shared libraries                      |
| `include`     | `~/.syslua/include/`          | For development headers                                      |
| `out`         | (not linked)                  | Root path, used for custom linking                           |

### Completions Structure

Completions should be in a flat directory with shell-specific extensions:

```
outputs.completions/
├── rg.bash      → ~/.local/share/bash-completion/completions/rg
├── _rg.zsh      → ~/.zsh/completions/_rg (or detected $fpath)
├── rg.fish      → ~/.config/fish/completions/rg.fish
└── _rg.ps1      → PowerShell module path
```

Extension mapping:

| Extension             | Shell      | Target Directory                              |
| --------------------- | ---------- | --------------------------------------------- |
| `.bash`               | Bash       | `~/.local/share/bash-completion/completions/` |
| `.zsh`, `_*` (no ext) | Zsh        | `~/.zsh/completions/`                         |
| `.fish`               | Fish       | `~/.config/fish/completions/`                 |
| `.ps1`                | PowerShell | PowerShell profile directory                  |

## Symlink Forest Structure

The unified profile is created at `~/.syslua/`:

```
~/.syslua/
├── bin/                    → store/env/<hash>/bin/
├── share/
│   ├── man/
│   │   ├── man1/
│   │   ├── man5/
│   │   └── ...
│   └── completions/        (intermediate, linked to shell-specific dirs)
├── lib/                    → store/env/<hash>/lib/
└── include/                → store/env/<hash>/include/
```

### Atomic Updates

1. Build new environment in store: `~/.syslua/store/env/<new-hash>/`
2. Create symlink forest inside the new env directory
3. Atomic swap: `rename(tmp_symlink, ~/.syslua/bin)`
4. Previous env remains in store (for rollback, until GC)

### First-Run Handling

If `~/.syslua/bin` doesn't exist, create it directly (no swap needed).

## Collision Resolution

When two packages provide the same binary name, the existing priority system resolves conflicts:

```lua
environment.packages.setup({
  use = {
    pkgs.cli.eza, -- default priority (1000)
    prio.after(pkgs.cli.gnu_coreutils), -- priority 1500, eza wins for 'ls'
  },
})
```

### Priority Levels

| Function         | Priority | Use Case                      |
| ---------------- | -------- | ----------------------------- |
| `prio.force()`   | 50       | Must win, override everything |
| `prio.before()`  | 500      | Should win most conflicts     |
| (plain value)    | 900      | Normal priority               |
| `prio.default()` | 1000     | Explicit default              |
| `prio.after()`   | 1500     | Should lose to others         |

### Conflict Behavior

- **Different priorities**: Lower number wins, no error
- **Same priority, same binary**: Error with clear message showing both sources
- **Resolution options shown**: Use `prio.before()`/`prio.after()` to resolve

Example error:

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

## Shell Integration

When `shell_integration = true` (default), automatically add `~/.syslua/bin` to PATH in shell configs:

| Shell      | Config File                  | Method                                      |
| ---------- | ---------------------------- | ------------------------------------------- |
| Bash       | `~/.bashrc`                  | `export PATH="$HOME/.syslua/bin:$PATH"`     |
| Zsh        | `~/.zshenv`                  | `export PATH="$HOME/.syslua/bin:$PATH"`     |
| Fish       | `~/.config/fish/config.fish` | `fish_add_path ~/.syslua/bin`               |
| PowerShell | `$PROFILE`                   | `$env:PATH = "$HOME/.syslua/bin;$env:PATH"` |

### Markers

Use `# BEGIN SYSLUA PACKAGES` / `# END SYSLUA PACKAGES` markers (like existing `modules.env`):

```bash
# BEGIN SYSLUA PACKAGES
export PATH="$HOME/.syslua/bin:$PATH"
# END SYSLUA PACKAGES
```

### Opt-Out

```lua
environment.packages.setup({
  use = { ... },
  shell_integration = false,
})
```

When opted out, print manual instructions:

```
Shell integration disabled. Add to your shell config:

  Bash/Zsh: export PATH="$HOME/.syslua/bin:$PATH"
  Fish:     fish_add_path ~/.syslua/bin
  PowerShell: $env:PATH = "$HOME/.syslua/bin;$env:PATH"
```

## Build Isolation Helpers

For packages built from source with dependencies, syslua provides platform-aware isolation helpers. These are registered via `sys.register_build_ctx_method()` in `init.lua`.

### Helpers

| Helper                           | Purpose                                           | Platforms                |
| -------------------------------- | ------------------------------------------------- | ------------------------ |
| `ctx:patch_rpath(deps)`          | Patch ELF/Mach-O to find libraries at store paths | Linux, macOS             |
| `ctx:patch_shebang(interpreter)` | Rewrite script shebangs to store paths            | All                      |
| `ctx:wrap_binary(opts)`          | Create wrapper script with environment variables  | All (primary on Windows) |

### Platform Mechanisms

| Platform | Binary Isolation               | Script Isolation |
| -------- | ------------------------------ | ---------------- |
| Linux    | `patchelf --set-rpath`         | Shebang patching |
| macOS    | `install_name_tool -add_rpath` | Shebang patching |
| Windows  | Wrapper scripts (sets PATH)    | Wrapper scripts  |

### Usage Example

```lua
sys.build({
  inputs = {
    openssl = pkgs.lib.openssl,
    bash = pkgs.cli.bash,
  },
  create = function(inputs, ctx)
    -- Build the binary
    ctx:exec({ bin = 'make', args = { 'install', 'PREFIX=' .. ctx.out } })

    -- Patch library paths (Linux/macOS: RPATH, Windows: no-op)
    ctx:patch_rpath({ openssl = inputs.openssl })

    -- Patch script shebangs
    ctx:patch_shebang(inputs.bash.outputs.bin .. '/bash')

    return { bin = ctx.out .. '/bin/', out = ctx.out }
  end,
})
```

### `ctx:patch_rpath(deps)`

Patches all ELF (Linux) or Mach-O (macOS) binaries in `ctx.out` to include library paths from dependencies.

```lua
ctx:patch_rpath({
  openssl = inputs.openssl, -- adds openssl's lib/ to RPATH
  zlib = inputs.zlib,
})
```

**Linux implementation**: `patchelf --set-rpath <paths> <binary>`

**macOS implementation**: `install_name_tool -add_rpath <path> <binary>`

**Windows**: No-op (use `ctx:wrap_binary()` instead)

### `ctx:patch_shebang(interpreter)`

Rewrites shebang lines in scripts to use specific interpreter from store.

```lua
ctx:patch_shebang(inputs.bash.outputs.bin .. '/bash')
-- Rewrites #!/bin/bash → #!/home/user/.syslua/store/build/<hash>/bin/bash
```

### `ctx:wrap_binary(opts)`

Creates a wrapper script that sets environment variables before executing the real binary. Primary isolation mechanism on Windows.

```lua
ctx:wrap_binary({
  binary = ctx.out .. '/bin/mytool',
  env = {
    PATH = inputs.openssl.outputs.bin,
    LD_LIBRARY_PATH = inputs.openssl.outputs.lib,
  },
})
```

**Output** (Unix):

```bash
#!/bin/sh
export PATH="/store/build/<hash>/bin:$PATH"
export LD_LIBRARY_PATH="/store/build/<hash>/lib"
exec "/store/build/<hash>/bin/mytool.real" "$@"
```

**Output** (Windows):

```batch
@echo off
set PATH=%~dp0..\deps\openssl\bin;%PATH%
"%~dp0mytool.real.exe" %*
```

## Platform-Specific Behavior

### Windows Considerations

1. **Symlinks require privileges**: Developer Mode or Admin. Fall back to:
   - Directory junctions (for directories)
   - Hard links (for files)
   - Copy as last resort

2. **PATH separator**: `;` not `:`

3. **Executable extensions**: `.exe`, `.cmd`, `.bat`, `.ps1`

4. **Wrapper scripts**: `.cmd` files, primary isolation mechanism

### macOS Considerations

1. **Hardened Runtime**: Some signed binaries can't have RPATH modified
2. **`@rpath` vs `@executable_path`**: Use `@rpath` for relocatable binaries
3. **Notarization**: Modifying signed binaries invalidates signature

## Implementation Phases

| Phase | Scope                                                 | Dependencies |
| ----- | ----------------------------------------------------- | ------------ |
| 1     | `environment.packages` core - symlink forest creation | None         |
| 2     | Shell integration - auto-modify shell configs         | Phase 1      |
| 3     | `ctx:wrap_binary()` - wrapper script generation       | None         |
| 4     | `ctx:patch_rpath()` - RPATH patching                  | None         |
| 5     | `ctx:patch_shebang()` - shebang rewriting             | None         |

**Recommended order**: Phases 1-2 first (user-facing value), then 3-5 (build infrastructure).

## Relationship to Existing Modules

### `syslua.environment.packages` vs `syslua.modules.env`

| Module                 | Purpose                      | When to Use                              |
| ---------------------- | ---------------------------- | ---------------------------------------- |
| `environment.packages` | Unified package management   | Adding packages to PATH                  |
| `modules.env`          | Custom environment variables | Non-package PATH entries, other env vars |

They can coexist:

```lua
-- Packages via environment.packages
environment.packages.setup({ use = { pkgs.cli.ripgrep } })

-- Custom paths via modules.env
modules.env.setup({
  PATH = prio.after('/usr/local/custom/bin'),
  EDITOR = 'nvim',
})
```

### `syslua.programs.*` Refactoring

Current pattern:

```lua
-- programs/ripgrep.lua
local pkg_build = pkgs.cli.ripgrep.setup()
modules.env.setup({ PATH = prio.before(pkg_build.outputs.out) })
-- Plus completions, man pages...
```

New pattern (programs delegate to `environment.packages`):

```lua
-- programs/ripgrep.lua
local pkg_build = pkgs.cli.ripgrep.setup()
environment.packages.setup({ use = { pkg_build } })
-- Completions and man pages handled automatically via link options
```

## Open Questions

1. **Garbage collection**: How long to keep old env generations? Count-based or time-based?

2. **Per-user vs system-wide**: Current design is per-user (`~/.syslua/`). System-wide (`/usr/local/syslua/`) as future extension?

3. **Profile switching**: Support for multiple named profiles (`sys profile switch dev`)? Future work.

4. **Transitive dependencies**: If package A depends on package B, should B's binaries be linked? Probably not by default.

## Future Work

- Per-project environments (direnv-style activation)
- Profile management (`sys profile list`, `sys profile switch`)
- Lazy wrapper generation (only for actually-used packages)
- Binary caching for faster rebuilds
- Remote binary cache (like Nix's binary cache)

## References

- [Nix environment.systemPackages](https://nixos.org/manual/nixos/stable/#sec-declarative-package-mgmt)
- [Nix buildEnv](https://nixos.org/manual/nixpkgs/stable/#sec-building-environment)
- [patchelf](https://github.com/NixOS/patchelf)
