# Inputs

> Part of the [sys.lua Architecture](./00-overview.md) documentation.

This document covers input sources, lock files, and how inputs are accessed in configuration.

## Overview

Inputs are external dependencies declared in your entry point's `M.inputs` table. An input can be any Git repository or local path - it doesn't need any special structure. Inputs are resolved before `M.setup(inputs)` runs, ensuring all external content is available during configuration evaluation.

## Input Declaration

Inputs are declared in the `M.inputs` table of your entry point (`init.lua`):

```lua
-- ~/.config/syslua/init.lua
local M = {}

M.inputs = {
    -- Git repository with init.lua (can be require()'d)
    syslua = "git:https://github.com/spirit-led-software/syslua.git",

    -- Git repository without init.lua (accessed via inputs.dotfiles.path)
    dotfiles = "git:git@github.com:myuser/dotfiles.git",

    -- Local path
    local_config = "path:~/code/my-config",
}

function M.setup(inputs)
    -- Inputs with init.lua can be require()'d using the input name
    local syslua = require("syslua")
    local path, lib = syslua.path, syslua.lib

    -- Install a package from syslua
    require("syslua.pkgs.cli.ripgrep").setup()

    -- Inputs without init.lua are accessed via inputs table
    lib.file.setup({
        target = "~/.gitconfig",
        source = path.join(inputs.dotfiles.path, ".gitconfig"),
    })

    lib.file.setup({
        target = "~/.zshrc",
        source = path.join(inputs.dotfiles.path, ".zshrc"),
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

## Accessing Inputs

There are two ways to access input content, depending on whether the input has a top-level `init.lua`:

### Inputs with `init.lua` (Requireable)

If an input has a top-level `init.lua`, it becomes a Lua module that can be `require()`'d using the input name as the namespace. The input searcher handles:

- **Exact match**: `require("input_name")` → loads `input_path/init.lua`
- **Submodules**: `require("input_name.utils")` → loads `input_path/utils.lua`
- **Nested submodules**: `require("input_name.sub.module")` → loads `input_path/sub/module.lua` or `input_path/sub/module/init.lua`
- **LuaRocks-style**: `require("input_name.utils")` → also checks `input_path/lua/utils.lua` or `input_path/lua/utils/init.lua`

The search order for submodules is:
1. `input_path/<module>.lua`
2. `input_path/<module>/init.lua`
3. `input_path/lua/<module>.lua` (LuaRocks-style)
4. `input_path/lua/<module>/init.lua` (LuaRocks-style)

This allows libraries with a `lua/` subdirectory (common in LuaRocks packages) to work seamlessly.

All files loaded from inputs have `sys.dir` injected, just like any other Lua file. This means relative `require()` and `dofile()` calls within inputs work correctly.

```lua
M.inputs = {
    syslua = "git:https://github.com/spirit-led-software/syslua.git",
    my_helpers = "git:git@github.com:myorg/lua-helpers.git",
}

function M.setup(inputs)
    -- require() using the input name directly
    local syslua = require("syslua")
    local helpers = require("my_helpers")

    -- Access submodules
    local ripgrep = require("syslua.pkgs.cli.ripgrep")
    ripgrep.setup()

    -- Use helpers from the input
    helpers.do_something()
end
```

### Inputs without `init.lua` (Path Access)

Inputs without a top-level `init.lua` cannot be `require()`'d, but their content is still accessible via the `inputs` table passed to `M.setup()`:

```lua
M.inputs = {
    syslua = "git:https://github.com/spirit-led-software/syslua.git",
    dotfiles = "git:git@github.com:myuser/dotfiles.git",  -- no init.lua
}

function M.setup(inputs)
    local syslua = require("syslua")
    local path, lib = syslua.path, syslua.lib

    -- Access dotfiles via inputs.dotfiles.path
    lib.file.setup({
        target = "~/.gitconfig",
        source = path.join(inputs.dotfiles.path, ".gitconfig"),
    })

    lib.file.setup({
        target = "~/.vimrc",
        source = path.join(inputs.dotfiles.path, "vim/vimrc"),
    })
end
```

### Input Table Structure

Each input in the `inputs` table passed to `M.setup()` has the following structure:

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

For simple inputs without transitive dependencies, the `inputs` field is omitted.

## Transitive Dependencies

Inputs can declare their own dependencies, which sys.lua resolves automatically. This works similarly to Nix flakes - each input can have its own `M.inputs` table, and those dependencies are resolved transitively.

### How Transitive Dependencies Work

When an input has an `init.lua` with its own `M.inputs`:

1. **Automatic Resolution**: sys.lua parses the input's `init.lua` and resolves its declared dependencies
2. **Content-Addressed Cache**: Each unique input (by URL + revision) is stored once in the cache
3. **`.inputs/` Symlinks**: Dependencies are linked into each input's `.inputs/` directory
4. **Custom Require**: A custom Lua searcher walks up the directory tree to find dependencies in `.inputs/`

### Example: Library with Dependencies

Consider a library `my-lib` that depends on `utils`:

```lua
-- my-lib/init.lua
local M = {}

M.inputs = {
    utils = "git:https://github.com/someorg/utils.git",
}

function M.setup(inputs)
    local utils = require("utils")  -- Resolved via .inputs/utils symlink
    -- ...
end

return M
```

When you use `my-lib` in your config:

```lua
-- ~/.config/syslua/init.lua
M.inputs = {
    my_lib = "git:https://github.com/myorg/my-lib.git",
}

function M.setup(inputs)
    local my_lib = require("my_lib")
    my_lib.setup(inputs.my_lib.inputs)
end
```

The cache structure looks like:

```
~/.cache/syslua/inputs/store/
├── my-lib-a1b2c3d4/
│   ├── init.lua
│   └── .inputs/
│       └── utils -> ../utils-e5f6g7h8  (symlink)
└── utils-e5f6g7h8/
    └── init.lua
```

### The `follows` Mechanism

You can override how an input's transitive dependencies are resolved using `follows`. This tells sys.lua to use a different input instead of what the library declares.

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

With this configuration:
- `my-lib` will use your `my_utils` instead of its declared `utils`
- The `.inputs/utils` symlink in `my-lib` points to `my_utils`
- Only one copy of utils exists in the cache

### Follows Chains

`follows` declarations can chain: if A's dep follows B, and B's dep follows C, then A gets C's version. The chain is limited to 10 hops to prevent infinite loops.

### Diamond Dependencies

When multiple inputs depend on the same library with the same URL:

```lua
M.inputs = {
    lib_a = "git:.../lib-a.git",  -- depends on utils v1
    lib_b = "git:.../lib-b.git",  -- also depends on utils v1
}
```

If both `lib_a` and `lib_b` depend on the same `utils` URL+revision, they share the same cached copy. Each gets their own `.inputs/utils` symlink pointing to the shared location.

### Circular Dependencies

Circular dependencies between inputs are supported for runtime usage:

```lua
-- lib_a/init.lua
M.inputs = { lib_b = "path:../lib_b" }

-- lib_b/init.lua
M.inputs = { lib_a = "path:../lib_a" }
```

The symlinks allow each library to `require()` the other at runtime. The resolution algorithm detects cycles by tracking URLs it has already resolved.

## Lock File

sys.lua generates a `syslua.lock` file in the same directory as the configuration. This ensures reproducible builds by pinning input revisions.

- **System configs**: `/etc/syslua/` → `/etc/syslua/syslua.lock`
- **User configs**: `~/.config/syslua/` → `~/.config/syslua/syslua.lock`
- **Project configs**: `./` → `./syslua.lock` (committed to version control)

### Lock File Format

The lock file uses a graph-based format (version 2) to support transitive dependencies:

```json
{
  "version": 2,
  "nodes": {
    "__root__": {
      "inputs": ["syslua", "dotfiles"]
    },
    "syslua": {
      "type": "git",
      "url": "https://github.com/spirit-led-software/syslua.git",
      "rev": "a1b2c3d4e5f6...",
      "sha256": "...",
      "lastModified": 1733667300,
      "inputs": ["syslua/utils"]
    },
    "syslua/utils": {
      "type": "git",
      "url": "https://github.com/spirit-led-software/lua-utils.git",
      "rev": "b2c3d4e5f6a1...",
      "sha256": "...",
      "lastModified": 1733667200
    },
    "dotfiles": {
      "type": "git",
      "url": "git@github.com:myuser/dotfiles.git",
      "rev": "f6e5d4c3b2a1...",
      "sha256": "...",
      "lastModified": 1733667400
    }
  }
}
```

Key features:
- `__root__` node lists direct dependencies
- Transitive deps use path notation (`parent/dep_name`)
- Each node tracks its own transitive dependencies
- Version 1 lock files are automatically migrated to version 2

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

## Input Authentication

### SSH-First (Recommended)

For private repositories, **SSH URLs are recommended**. They use your existing `~/.ssh/` keys with no additional configuration:

```lua
M.inputs = {
    -- Public (HTTPS, no auth)
    syslua = "git:https://github.com/spirit-led-software/syslua.git",

    -- Private (SSH - uses ~/.ssh/id_ed25519, ~/.ssh/id_rsa, etc.)
    company = "git:git@github.com:mycompany/internal-pkgs.git",
    private = "git:git@gitlab.com:myorg/dotfiles.git",
}
```

**Why SSH-first?**

- No token management - uses existing SSH keys
- Works with any Git host (GitHub, GitLab, Bitbucket, self-hosted)
- Keys already configured for `git clone` workflows
- No secrets to encrypt or rotate

### SOPS Fallback (HTTPS with Tokens)

If SSH is not available (CI environments, restricted networks), use SOPS-encrypted tokens for HTTPS authentication:

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

### Authentication Methods

| URL Format                  | Auth Method        | Use Case                          |
| --------------------------- | ------------------ | --------------------------------- |
| `git:git@github.com:...`    | SSH keys (~/.ssh/) | **Recommended** for private repos |
| `git:https://...` (public)  | None               | Public repositories               |
| `git:https://...` (private) | SOPS token         | CI/CD, restricted environments    |
| `path:...`                  | None               | Local development                 |

### Security Notes

- Prefer SSH URLs - no secrets to manage
- Never commit plaintext tokens
- Use SOPS only when SSH is not viable
- The `auth` field is never written to `syslua.lock`

## Input Resolution Algorithm

```
RESOLVE_INPUTS(config, lock_file):
    inputs = {}

    FOR EACH name, url IN config.inputs:
        // Check if lock file exists and has this input
        IF lock_file EXISTS AND lock_file.inputs[name] EXISTS:
            locked = lock_file.inputs[name]

            // Validate lock entry matches config
            IF locked.url != url:
                ERROR "Lock file mismatch for input '{name}'."
                      "Run 'sys update {name}' to update the lock file."

            // Use pinned revision from lock
            inputs[name] = FETCH_INPUT(name, url, locked.rev)
        ELSE:
            // No lock entry - resolve to latest
            resolved = RESOLVE_LATEST(url)
            inputs[name] = FETCH_INPUT(name, url, resolved.rev)

            // Add to lock file
            lock_file.inputs[name] = {
                type: PARSE_TYPE(url),
                url: url,
                rev: resolved.rev,
                sha256: resolved.sha256,
                lastModified: resolved.timestamp,
            }

    // Write updated lock file if changed
    IF lock_file WAS MODIFIED:
        WRITE_LOCK_FILE(lock_file)

    RETURN inputs

RESOLVE_LATEST(url):
    type = PARSE_TYPE(url)
    SWITCH type:
        CASE "git":
            RETURN GIT.ls_remote(url, ref="HEAD")

        CASE "path":
            // Local paths use directory mtime as "revision"
            RETURN { rev: "local", sha256: HASH_DIRECTORY(path), timestamp: DIR_MTIME(path) }

FETCH_INPUT(name, url, rev):
    cache_key = HASH(url + rev)
    cache_path = "~/.cache/syslua/inputs/{cache_key}"

    IF cache_path EXISTS:
        RETURN { path: cache_path, rev: rev }

    type = PARSE_TYPE(url)
    SWITCH type:
        CASE "git":
            GIT.clone(url, cache_path, rev=rev)
            REMOVE(cache_path + "/.git")  // Strip git metadata

        CASE "path":
            SYMLINK(EXPAND_PATH(url), cache_path)

    // Register as Lua module if init.lua exists
    IF FILE_EXISTS(cache_path + "/init.lua"):
        REGISTER_INPUT_SEARCHER(name, cache_path)

    RETURN { path: cache_path, rev: rev }

REGISTER_INPUT_SEARCHER(name, cache_path):
    // A custom package.searchers entry is added that:
    // 1. Maps require("name") → cache_path/init.lua
    // 2. Maps require("name.sub.module") to (in order):
    //    - cache_path/sub/module.lua
    //    - cache_path/sub/module/init.lua
    //    - cache_path/lua/sub/module.lua       (LuaRocks-style)
    //    - cache_path/lua/sub/module/init.lua  (LuaRocks-style)
    // 3. Uses load_file_with_dir() for sys.dir injection
    //
    // The searcher is inserted at position 2 in package.searchers,
    // before the standard file searcher, so input names take precedence.
```

### Lock File Validation Rules

| Scenario                       | Behavior                                 |
| ------------------------------ | ---------------------------------------- |
| Lock exists, input unchanged   | Use locked `rev`                         |
| Lock exists, input URL changed | Error (must run `sys update`)            |
| Lock missing for input         | Resolve latest, add to lock              |
| Lock file missing entirely     | Resolve all inputs, create lock          |
| `sys update` command           | Re-resolve specified inputs, update lock |

## Example: Complete Configuration

```lua
local M = {}

M.inputs = {
    -- Main syslua registry (has init.lua)
    syslua = "git:https://github.com/spirit-led-software/syslua.git",

    -- Personal dotfiles (no init.lua, just config files)
    dotfiles = "git:git@github.com:myuser/dotfiles.git",

    -- Company tools (has init.lua with custom packages)
    company = "git:git@github.com:mycompany/syslua-pkgs.git",
}

function M.setup(inputs)
    -- Load syslua helpers
    local syslua = require("syslua")
    local path, lib = syslua.path, syslua.lib

    -- Install packages from syslua registry
    require("syslua.pkgs.cli.ripgrep").setup()
    require("syslua.pkgs.cli.fd").setup()
    require("syslua.pkgs.editors.neovim").setup()

    -- Install company-specific tools
    require("company.tools.internal_cli").setup()

    -- Link dotfiles (accessed via path since no init.lua)
    local dotfiles = inputs.dotfiles.path

    lib.file.setup({ target = "~/.gitconfig", source = path.join(dotfiles, "git/gitconfig") })
    lib.file.setup({ target = "~/.zshrc", source = path.join(dotfiles, "zsh/zshrc") })
    lib.file.setup({ target = "~/.config/nvim", source = path.join(dotfiles, "nvim") })

    -- Set environment variables
    lib.env.setup({
        EDITOR = "nvim",
        PAGER = "less",
    })
end

return M
```

## See Also

- [Lua API](./04-lua-api.md) - Entry point pattern (`M.inputs`/`M.setup`)
- [Builds](./01-builds.md) - How builds work
- [Binds](./02-binds.md) - How binds work
- [Modules](./07-modules.md) - Module system
