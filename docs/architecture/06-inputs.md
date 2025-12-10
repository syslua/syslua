# Inputs and Registry

> Part of the [sys.lua Architecture](./00-overview.md) documentation.

This document covers input sources, registry structure, lock files, and authentication.

## Overview

sys.lua uses declarative inputs defined in the entry point's `M.inputs` table. Inputs are resolved in a separate phase before configuration evaluation, ensuring all external dependencies are available when `M.setup(inputs)` runs.

## Input Declaration

Inputs are declared in the `M.inputs` table of your entry point (`init.lua`):

```lua
-- ~/.config/syslua/init.lua
local M = {}

M.inputs = {
    -- Public registry (HTTPS, no auth needed)
    pkgs = "git:https://github.com/syslua/pkgs.git",
    
    -- Private repos (SSH recommended - uses ~/.ssh/ keys)
    private = "git:git@github.com:myorg/my-dotfiles.git",
    company = "git:git@github.com:mycompany/internal-pkgs.git",
    
    -- Local path (for development)
    local_pkgs = "path:~/code/my-packages",
}

function M.setup(inputs)
    -- Access input modules via require
    local pkgs = require("inputs.pkgs")
    local private = require("inputs.private")
    
    -- Use packages from inputs
    pkgs.cli.ripgrep.setup()
    pkgs.cli.neovim.setup()
    private.my_module.setup()
    
    -- Pin to specific version
    pkgs.cli.ripgrep.setup({ version = "14_1_0" })
end

return M
```

## Input URL Formats

| Format | Example | Auth Method |
|--------|---------|-------------|
| Git SSH | `git:git@github.com:org/repo.git` | SSH keys (~/.ssh/) |
| Git HTTPS | `git:https://github.com/org/repo.git` | None (public) or SOPS token |
| Local path | `path:~/code/my-packages` | None |
| Local path | `path:./relative/path` | None |

## Registry Structure

The official registry uses a hierarchical structure with `init.lua` entry points and versioned implementation files. Version files use underscores (e.g., `15_1_0.lua`) since Lua `require` doesn't work well with dots in module names.

```
sys-lua/pkgs/
├── cli/
│   ├── init.lua              # Category entry point
│   ├── ripgrep/
│   │   ├── init.lua          # Package entry point (latest + version routing)
│   │   ├── 15_1_0.lua        # Version implementation
│   │   ├── 14_1_0.lua
│   │   └── 13_0_0.lua
│   └── fd/
│       ├── init.lua
│       ├── 9_0_0.lua
│       └── 8_7_0.lua
├── editors/
│   ├── init.lua
│   ├── neovim/
│   │   ├── init.lua
│   │   ├── 0_10_0.lua
│   │   └── 0_9_5.lua
│   └── helix/
│       └── ...
└── init.lua                  # Root entry point
```

### Version File Example

**`pkgs/cli/ripgrep/15_1_0.lua`:**

```lua
---@class pkgs.cli.ripgrep.15_1_0
local M = {}

local hashes = {
    ["aarch64-darwin"] = "abc123...",
    ["x86_64-linux"] = "def456...",
    ["x86_64-windows"] = "ghi789...",
}

M.make_derivation = function()
    return derive({
        name = "ripgrep",
        version = "15.1.0",
        opts = function(sys)
            return {
                url = "https://github.com/BurntSushi/ripgrep/releases/download/15.1.0/ripgrep-15.1.0-" .. sys.platform .. ".tar.gz",
                sha256 = hashes[sys.platform],
            }
        end,
        config = function(opts, ctx)
            local archive = ctx.fetch_url(opts.url, opts.sha256)
            ctx.unpack(archive, ctx.out)
        end,
    })
end

M.make_activation = function(drv)
    return activate({
        opts = { drv = drv },
        config = function(opts, ctx)
            ctx.add_to_path(opts.drv.out .. "/bin")
        end,
    })
end

M.setup = function()
    local derivation = M.make_derivation()
    M.make_activation(derivation)
end

return M
```

### Package Entry Point Example

**`pkgs/cli/ripgrep/init.lua`:**

```lua
---@class pkgs.cli.ripgrep
---@field ["15_1_0"] pkgs.cli.ripgrep.15_1_0
---@field ["14_1_0"] pkgs.cli.ripgrep.14_1_0
local M = {}

-- Lazy loading of version modules
setmetatable(M, {
    __index = function(_, pkg)
        return require("pkgs.cli.ripgrep." .. pkg)
    end,
})

M.setup = function(opts)
    if opts == nil then
        return require("pkgs.cli.ripgrep.15_1_0").setup()
    end

    local version = opts.version or "15_1_0"
    local version_module = require("pkgs.cli.ripgrep." .. version)

    if opts.make_derivation then
        version_module.make_derivation = opts.make_derivation
    end
    if opts.make_activation then
        version_module.make_activation = opts.make_activation
    end

    return version_module.setup()
end

return M
```

### Version Selection

| Usage                                                              | Behavior              |
| ------------------------------------------------------------------ | --------------------- |
| `require("inputs.pkgs.cli.ripgrep").setup()`                       | Uses latest (15_1_0)  |
| `require("inputs.pkgs.cli.ripgrep").setup({ version = "14_1_0" })` | Uses specific version |
| `require("inputs.pkgs.cli.ripgrep")["14_1_0"].setup()`             | Direct version access |

## Package References

When you access `inputs.pkgs.cli.ripgrep`, it returns a **package module** with factory functions and a `setup()` method:

```lua
-- What inputs.pkgs.cli.ripgrep resolves to:
{
    -- Factory functions
    make_derivation = function() ... end,
    make_activation = function(drv) ... end,

    -- Setup orchestrates the installation
    setup = function(opts) ... end,

    -- Version modules accessible via metatable
    ["15_1_0"] = <lazy loaded version module>,
    ["14_1_0"] = <lazy loaded version module>,
}

-- Usage:
require("inputs.pkgs.cli.ripgrep").setup()                         -- Latest version
require("inputs.pkgs.cli.ripgrep").setup({ version = "14_1_0" })   -- Specific version
require("inputs.pkgs.cli.ripgrep")["14_1_0"].setup()               -- Direct access
```

**Note:** There is no separate `pkg()` function. Packages are installed by calling `setup()` on the package module, which internally calls `derive()` and `activate()` to register the package.

## Lock File

sys.lua generates a `syslua.lock` file in the same directory as the configuration. This enables:

- **System configs**: `/etc/syslua/` → `/etc/syslua/syslua.lock`
- **User configs**: `~/.config/syslua/` → `~/.config/syslua/syslua.lock`
- **Project configs**: `./` → `./syslua.lock` (committed to version control)

### Lock File Format

```json
{
  "version": 1,
  "inputs": {
    "pkgs": {
      "type": "github",
      "owner": "sys-lua",
      "repo": "pkgs",
      "rev": "a1b2c3d4e5f6...",
      "sha256": "...",
      "lastModified": 1733667300
    },
    "unstable": {
      "type": "github",
      "owner": "sys-lua",
      "repo": "pkgs",
      "branch": "unstable",
      "rev": "f6e5d4c3b2a1...",
      "sha256": "...",
      "lastModified": 1733667400
    }
  }
}
```

### Lock File Behavior

| Scenario              | Behavior                                 |
| --------------------- | ---------------------------------------- |
| `syslua.lock` exists  | Use pinned revisions from lock file      |
| `syslua.lock` missing | Resolve latest, create lock file         |
| `sys update`          | Re-resolve specified inputs, update lock |
| `sys update --commit` | Update lock and `git commit` it          |

### Team Workflow

```bash
# Developer A: Add new input, commit lock file
git add init.lua syslua.lock
git commit -m "Add nodejs to project"

# Developer B: Pull and apply (uses same pinned versions)
git pull
sudo sys apply sys.lua
```

### Commands

```bash
sys update                    # Update all inputs to latest
sys update pkgs               # Update specific input
sys update --commit           # Update and commit lock file
sys update --dry-run          # Show what would change
```

## Input Authentication

### SSH-First (Recommended)

For private repositories, **SSH URLs are recommended**. They use your existing `~/.ssh/` keys with no additional configuration:

```lua
M.inputs = {
    -- Public (HTTPS, no auth)
    pkgs = "git:https://github.com/syslua/pkgs.git",
    
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
    -- HTTPS with token auth
    company = {
        url = "git:https://github.com/mycompany/private-pkgs.git",
        auth = secrets.github_token,
    },
}
```

### Authentication Methods

| URL Format | Auth Method | Use Case |
|------------|-------------|----------|
| `git:git@github.com:...` | SSH keys (~/.ssh/) | **Recommended** for private repos |
| `git:https://...` (public) | None | Public repositories |
| `git:https://...` (private) | SOPS token | CI/CD, restricted environments |
| `path:...` | None | Local development |

### Security Notes

- Prefer SSH URLs - no secrets to manage
- Never commit plaintext tokens
- Use SOPS only when SSH is not viable
- The `auth` field is never written to `syslua.lock`

## Input Resolution Algorithm

```
RESOLVE_INPUTS(config, lock_file):
    inputs = {}

    FOR EACH input_decl IN config.inputs:
        input_id = input_decl.name

        // Check if lock file exists and has this input
        IF lock_file EXISTS AND lock_file.inputs[input_id] EXISTS:
            locked = lock_file.inputs[input_id]

            // Validate lock entry matches config
            IF locked.type != input_decl.type OR locked.url != input_decl.url:
                ERROR "Lock file mismatch for input '{input_id}'."
                      "Run 'sys update {input_id}' to update the lock file."

            // Use pinned revision from lock
            inputs[input_id] = FETCH_INPUT(input_decl, locked.rev)
        ELSE:
            // No lock entry - resolve to latest
            resolved = RESOLVE_LATEST(input_decl)
            inputs[input_id] = FETCH_INPUT(input_decl, resolved.rev)

            // Add to lock file
            lock_file.inputs[input_id] = {
                type: input_decl.type,
                url: input_decl.url,
                rev: resolved.rev,
                sha256: resolved.sha256,
                lastModified: resolved.timestamp,
            }

    // Write updated lock file if changed
    IF lock_file WAS MODIFIED:
        WRITE_LOCK_FILE(lock_file)

    RETURN inputs

RESOLVE_LATEST(input_decl):
    SWITCH input_decl.type:
        CASE "github":
            IF input_decl.branch SPECIFIED:
                RETURN GITHUB_API.get_branch_head(owner, repo, branch)
            ELSE:
                RETURN GITHUB_API.get_default_branch_head(owner, repo)

        CASE "gitlab":
            // Similar to GitHub

        CASE "git":
            RETURN GIT.ls_remote(url, ref="HEAD")

        CASE "path":
            // Local paths use directory mtime as "revision"
            RETURN { rev: "local", sha256: HASH_DIRECTORY(path), timestamp: DIR_MTIME(path) }

FETCH_INPUT(input_decl, rev):
    cache_key = HASH(input_decl.url + rev)
    cache_path = "~/.cache/syslua/inputs/{cache_key}"

    IF cache_path EXISTS:
        RETURN cache_path

    SWITCH input_decl.type:
        CASE "github", "gitlab":
            tarball_url = CONSTRUCT_ARCHIVE_URL(input_decl, rev)
            DOWNLOAD(tarball_url, cache_path, auth=input_decl.auth)
            EXTRACT(cache_path)

        CASE "git":
            GIT.clone(input_decl.url, cache_path, rev=rev, auth=input_decl.auth)
            REMOVE(cache_path + "/.git")  // Strip git metadata

        CASE "path":
            SYMLINK(input_decl.path, cache_path)

    RETURN cache_path
```

### Lock File Validation Rules

| Scenario                        | Behavior                                 |
| ------------------------------- | ---------------------------------------- |
| Lock exists, input unchanged    | Use locked `rev`                         |
| Lock exists, input URL changed  | Error (must run `sys update`)            |
| Lock exists, input type changed | Error (must run `sys update`)            |
| Lock missing for input          | Resolve latest, add to lock              |
| Lock file missing entirely      | Resolve all inputs, create lock          |
| `sys update` command            | Re-resolve specified inputs, update lock |

## Custom Package Definitions

Users can define custom derivations directly in their entry point:

```lua
local M = {}

M.inputs = {
    pkgs = "git:https://github.com/syslua/pkgs.git",
}

function M.setup(inputs)
    -- Use registry packages
    require("inputs.pkgs.cli.ripgrep").setup()
    
    -- Custom derivation from GitHub release (prebuilt binaries)
    local hashes = {
        ["x86_64-linux"] = "abc123...",
        ["aarch64-darwin"] = "def456...",
    }
    
    local internal_tool_drv = derive {
        name = "my-internal-tool",
        version = "2.1.0",
        
        opts = function(sys)
            return {
                url = "https://github.com/mycompany/internal-tool/releases/download/v2.1.0/internal-tool-2.1.0-" .. sys.platform .. ".tar.gz",
                sha256 = hashes[sys.platform],
            }
        end,
        
        config = function(opts, ctx)
            local archive = ctx.fetch_url(opts.url, opts.sha256)
            ctx.unpack(archive, ctx.out)
        end,
    }
    
    -- Install it with activation
    activate {
        opts = { drv = internal_tool_drv },
        config = function(opts, ctx)
            ctx.add_to_path(opts.drv.out .. "/bin")
        end,
    }
end

return M
```

## See Also

- [Lua API](./04-lua-api.md) - Entry point pattern (`M.inputs`/`M.setup`)
- [Derivations](./01-derivations.md) - How derivations work
- [Activations](./02-activations.md) - How activations work
- [Modules](./07-modules.md) - Module system
