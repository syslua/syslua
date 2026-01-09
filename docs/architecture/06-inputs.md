# Inputs

> Part of the [SysLua Architecture](./00-overview.md) documentation.

This document covers input sources, lock files, and how inputs are accessed in configuration.

## Overview

Inputs are external dependencies declared in your entry point's `M.inputs` table. An input can be any Git repository or local path. Inputs are resolved before `M.setup(inputs)` runs, ensuring all external content is available during configuration evaluation.

**Key concepts:**

- **Explicit namespacing** - Libraries provide their code via `lua/<namespace>/` directories
- **Flat package.path** - All `lua/` directories are added to `package.path`
- **Conflict detection** - Namespace conflicts are detected at resolution time, not runtime
- **Per-input lock files** - Each input can have its own `syslua.lock` for transitive deps
- **Content-addressed deduplication** - Same URL + same revision = no conflict (diamond deps "just work")

## Input Declaration

Inputs are declared in the `M.inputs` table of your entry point (`init.lua`):

```lua
-- ~/.config/syslua/init.lua
local M = {}

M.inputs = {
    -- Git repository with lua/ directory (can be require()'d)
    syslua = "git:https://github.com/spirit-led-software/syslua.git",

    -- Git repository without lua/ (accessed via inputs.dotfiles.path)
    dotfiles = "git:git@github.com:myuser/dotfiles.git",

    -- Local path
    local_config = "path:~/code/my-config",
}

function M.setup(inputs)
    -- Inputs with lua/ directories can be require()'d
    local syslua = require("syslua")
    local path, lib = syslua.path, syslua.lib

    -- Install a package from syslua
    require("syslua.pkgs.cli.ripgrep").setup()

    -- Inputs without lua/ are accessed via inputs table
    lib.file.setup({
        target = "~/.gitconfig",
        source = path.join(inputs.dotfiles.path, ".gitconfig"),
    })
end

return M
```

### Extended Input Syntax (Transitive Overrides)

For advanced use cases, you can use the extended table syntax to override how an input's transitive dependencies are resolved:

```lua
M.inputs = {
    -- Simple URL string (most common)
    syslua = "git:https://github.com/spirit-led-software/syslua.git",

    -- Extended syntax: override transitive dependencies
    my_lib = {
        url = "git:https://github.com/myorg/my-lib.git",
        inputs = {
            -- Make my_lib use our version of utils (instead of its own)
            utils = { follows = "my_utils" },

            -- Override with a specific URL
            logger = "git:https://github.com/myorg/better-logger.git",
        },
    },

    -- This is what my_lib's "utils" dependency will use
    my_utils = "git:https://github.com/myorg/utils.git",
}
```

See [Transitive Dependencies](#transitive-dependencies) for more details.

## Input URL Formats

| Format     | Example                               | Auth Method                 |
| ---------- | ------------------------------------- | --------------------------- |
| Git SSH    | `git:git@github.com:org/repo.git`     | SSH keys (~/.ssh/)          |
| Git HTTPS  | `git:https://github.com/org/repo.git` | None (public) or SOPS token |
| Local path | `path:~/code/my-packages`             | None                        |
| Local path | `path:./relative/path`                | None                        |

## Input Structure

### Library Input (with Lua code)

Libraries that want to be `require()`able must use the `lua/<namespace>/` structure:

```
my-library/
├── init.lua              # Declares inputs and setup (same shape as user config)
├── syslua.lock           # Optional: locks transitive deps
└── lua/
    └── my_lib/           # Library namespace (self-chosen, unique)
        ├── init.lua      # require("my_lib") loads this
        ├── utils.lua     # require("my_lib.utils") loads this
        └── sub/
            └── module.lua  # require("my_lib.sub.module") loads this
```

**Library init.lua structure:**

```lua
-- my-library/init.lua
return {
    inputs = {
        -- Transitive dependencies
        utils = "git:https://github.com/org/utils.git",
    },
    setup = function(inputs)
        -- Called automatically by resolver after deps are resolved
        -- Used for library initialization
    end,
}
```

### Config/Dotfiles Input (no code)

Inputs without a `lua/` directory are accessed via their path:

```
dotfiles/
├── .gitconfig
├── .zshrc
└── nvim/
    └── init.lua
```

Access via `inputs.dotfiles.path` in your `M.setup()`.

## Accessing Inputs

### Inputs with `lua/` Directory (Requireable)

If an input has a `lua/<namespace>/` directory, that namespace becomes available via `require()`:

```lua
M.inputs = {
    syslua = "git:https://github.com/spirit-led-software/syslua.git",
    my_helpers = "git:git@github.com:myorg/lua-helpers.git",
}

function M.setup(inputs)
    -- require() using the namespace from lua/<namespace>/
    local syslua = require("syslua")
    local helpers = require("my_helpers")

    -- Access submodules
    local ripgrep = require("syslua.pkgs.cli.ripgrep")
    ripgrep.setup()
end
```

**How it works:** During resolution, syslua scans all `lua/` directories and builds a flat `package.path`. This means `require()` works identically everywhere - in your config, in libraries, and in transitive deps.

### Inputs without `lua/` Directory (Path Access)

Inputs without a `lua/` directory are accessed via the `inputs` table:

```lua
M.inputs = {
    syslua = "git:https://github.com/spirit-led-software/syslua.git",
    dotfiles = "git:git@github.com:myuser/dotfiles.git",  -- no lua/
}

function M.setup(inputs)
    local syslua = require("syslua")
    local path, lib = syslua.path, syslua.lib

    -- Access dotfiles via inputs.dotfiles.path
    lib.file.setup({
        target = "~/.gitconfig",
        source = path.join(inputs.dotfiles.path, ".gitconfig"),
    })
end
```

### Input Table Structure

Each input in the `inputs` table passed to `M.setup()` has:

```lua
inputs.my_lib = {
    path = "/path/to/resolved/input",  -- Absolute path to input content
    rev = "abc123...",                  -- Git revision (or "local" for path inputs)
    inputs = {                          -- Transitive dependencies (if any)
        utils = {
            path = "/path/to/utils",
            rev = "def456...",
        },
    },
}
```

## Transitive Dependencies

Inputs can declare their own dependencies, which syslua resolves automatically. Each input can have its own `M.inputs` table, and those dependencies are resolved transitively.

### How Transitive Dependencies Work

1. **Automatic Resolution**: syslua parses each input's `init.lua` and resolves its declared dependencies
2. **Content-Addressed Cache**: Each unique input (by URL + revision) is stored once in the cache
3. **Setup Order**: Input `setup()` functions are called in dependency order (deps before dependents)
4. **Flat package.path**: All `lua/` directories are added to a single `package.path`

### Example: Library with Dependencies

Consider a library `my-lib` that depends on `utils`:

```lua
-- my-lib/init.lua
return {
    inputs = {
        utils = "git:https://github.com/someorg/utils.git",
    },
    setup = function(inputs)
        -- utils is already resolved and its setup() has been called
        local utils = require("utils")
    end,
}
```

When you use `my-lib` in your config, syslua:

1. Fetches `my-lib` and parses its `inputs` declaration
2. Fetches `utils` (the transitive dep)
3. Builds `package.path` from all `lua/` directories
4. Calls `utils.setup()` first
5. Calls `my-lib.setup()` second
6. Calls your config's `M.setup()` last

### The `follows` Mechanism

Use `follows` to override how an input's transitive dependencies are resolved:

**Use cases:**

- Use a newer version of a shared dependency
- Deduplicate the same library used by multiple inputs
- Use your own fork of a dependency

```lua
M.inputs = {
    -- Your version of utils
    my_utils = "git:https://github.com/myorg/utils-fork.git",

    -- Override my_lib's utils to use your version
    my_lib = {
        url = "git:https://github.com/myorg/my-lib.git",
        inputs = {
            utils = { follows = "my_utils" },
        },
    },
}
```

### Diamond Dependencies

When multiple inputs depend on the same library:

**Same version (automatic deduplication):**

```lua
M.inputs = {
    lib_a = "git:.../lib-a.git",  -- depends on utils@abc123
    lib_b = "git:.../lib-b.git",  -- also depends on utils@abc123
}
```

Both get the same cached copy. No conflict, no user action required.

**Different versions (requires follows):**

```lua
M.inputs = {
    lib_a = "git:.../lib-a.git",  -- depends on utils@v1.0.0
    lib_b = "git:.../lib-b.git",  -- depends on utils@v2.0.0
}
```

This produces a conflict error:

```
Namespace conflict: 'utils' provided by:
  - 'lib_a/utils' (git:.../utils.git@v1.0.0)
  - 'lib_b/utils' (git:.../utils.git@v2.0.0)
Add a follows override to resolve.
```

Resolve by adding `follows`:

```lua
M.inputs = {
    utils = "git:.../utils.git#v2.0.0",  -- pick v2
    lib_a = {
        url = "git:.../lib-a.git",
        inputs = { utils = { follows = "utils" } },
    },
    lib_b = {
        url = "git:.../lib-b.git",
        inputs = { utils = { follows = "utils" } },
    },
}
```

### Follows Chains

`follows` declarations can chain: if A's dep follows B, and B's dep follows C, then A gets C's version. The chain is limited to 10 hops to prevent infinite loops.

### Circular Dependencies

Circular dependencies between inputs are supported for runtime usage:

```lua
-- lib_a/init.lua
M.inputs = { lib_b = "path:../lib_b" }

-- lib_b/init.lua
M.inputs = { lib_a = "path:../lib_a" }
```

The resolution algorithm detects cycles by tracking URLs and avoids infinite loops.

## Lock File

syslua generates a `syslua.lock` file to ensure reproducible builds by pinning input revisions.

- **System configs**: `/etc/syslua/syslua.lock`
- **User configs**: `~/.config/syslua/syslua.lock`
- **Project configs**: `./syslua.lock` (committed to version control)

### Lock File Format

```json
{
  "version": 1,
  "root": "root",
  "nodes": {
    "root": {
      "inputs": {
        "syslua": "syslua-a1b2c3d4",
        "dotfiles": "dotfiles-f6e5d4c3"
      }
    },
    "syslua-a1b2c3d4": {
      "type": "git",
      "url": "git:https://github.com/spirit-led-software/syslua.git",
      "rev": "a1b2c3d4e5f6...",
      "lastModified": 1733667300,
      "inputs": {}
    },
    "dotfiles-f6e5d4c3": {
      "type": "git",
      "url": "git:git@github.com:myuser/dotfiles.git",
      "rev": "f6e5d4c3b2a1...",
      "lastModified": 1733667400,
      "inputs": {}
    }
  }
}
```

### Per-Input Lock Files

Inputs can have their own `syslua.lock` to pin their transitive dependencies:

```
my-library/
├── init.lua
├── syslua.lock     # Pins this library's transitive deps
└── lua/
    └── my_lib/
```

**Lock file precedence (highest to lowest):**

1. `follows` directive - explicit override from parent
2. Input's own `syslua.lock` - input controls its transitive deps
3. Input's `init.lua` declaration - floating (resolves to latest)

### Lock File Behavior

| Scenario              | Behavior                                 |
| --------------------- | ---------------------------------------- |
| `syslua.lock` exists  | Use pinned revisions from lock file      |
| `syslua.lock` missing | Resolve latest, create lock file         |
| `sys update`          | Re-resolve specified inputs, update lock |
| `sys update --commit` | Update lock and `git commit` it          |

### Commands

```bash
sys update                    # Update all inputs to latest
sys update syslua             # Update specific input
sys update --commit           # Update and commit lock file
sys update --dry-run          # Show what would change
```

## Namespace Conflicts

Conflicts are detected when two different inputs provide the same namespace in their `lua/` directories.

### Conflict Error Example

```
Namespace conflict: 'utils' provided by:
  - 'lib_a/utils' (git:https://github.com/org/utils.git@abc123)
  - 'lib_b/utils' (git:https://github.com/other/utils.git@def456)
Add a follows override to resolve, or rename one of the directories.
```

### Resolution Options

1. **Add a `follows` override** - Make one input use the other's version
2. **Rename the namespace** - Ask the library author to use a unique name
3. **Fork and modify** - Create your own version with a different namespace

### Config vs Input Conflicts

Your config's `lua/` directory is also checked for conflicts:

```
~/.config/syslua/
├── init.lua
└── lua/
    └── my_lib/    # Conflicts if an input also has lua/my_lib/
```

## Input Authentication

### SSH-First (Recommended)

For private repositories, **SSH URLs are recommended**:

```lua
M.inputs = {
    -- Public (HTTPS, no auth)
    syslua = "git:https://github.com/spirit-led-software/syslua.git",

    -- Private (SSH - uses ~/.ssh/id_ed25519, ~/.ssh/id_rsa, etc.)
    company = "git:git@github.com:mycompany/internal-pkgs.git",
}
```

**Why SSH-first?**

- No token management - uses existing SSH keys
- Works with any Git host
- No secrets to encrypt or rotate

### SOPS Fallback (HTTPS with Tokens)

If SSH is not available, use SOPS-encrypted tokens:

```yaml
# secrets.yaml (encrypted with SOPS)
github_token: ENC[AES256_GCM,data:...,tag:...]
```

```lua
local secrets = sops.load("./secrets.yaml")

M.inputs = {
    company = {
        url = "git:https://github.com/mycompany/private-pkgs.git",
        auth = secrets.github_token,
    },
}
```

## Resolution Algorithm Overview

1. **Parse** - Extract `M.inputs` declarations from config
2. **Fetch** - Clone/fetch each input, resolving refs to SHA
3. **Transitive** - Recursively resolve each input's declared dependencies
4. **Lock** - Apply per-input lock files for transitive dep versions
5. **Namespace scan** - Scan all `lua/` directories for namespaces
6. **Conflict check** - Detect namespace conflicts (same name, different source)
7. **Deduplicate** - Same URL+SHA = same content = no conflict
8. **Build package.path** - Construct flat search path from all `lua/` dirs
9. **Setup execution** - Call `setup()` functions in dependency order

## See Also

- [Lua API](./04-lua-api.md) - Entry point pattern (`M.inputs`/`M.setup`)
- [Builds](./01-builds.md) - How builds work
- [Binds](./02-binds.md) - How binds work
- [Modules](./07-modules.md) - Module system
