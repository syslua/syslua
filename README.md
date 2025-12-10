# sys.lua

**Declarative cross-platform system management with the simplicity of Lua and the power of Nix.**

> **Note:** sys.lua is currently in the design phase. This README describes the target system. We're looking for contributors to help bring this vision to life.

## What is sys.lua?

sys.lua reimagines system configuration management by combining three powerful ideas:

1. **Declarative configuration** - Your `sys.lua` file is the single source of truth for your entire environment
2. **Reproducibility** - Same config + same inputs = identical environment, every time
3. **Simplicity** - No PhD required. Just Lua and straightforward concepts

```lua
local M = {}

-- Declare your inputs (external dependencies)
M.inputs = {
    pkgs = "git:https://github.com/syslua/pkgs.git",
}

-- Configure your system
function M.setup(inputs)
    local pkgs = require("inputs.pkgs")
    local lib = require("syslua.lib")

    -- Install packages
    pkgs.cli.ripgrep.setup()
    pkgs.cli.neovim.setup({ version = "0.10.0" })

    -- Configure your environment
    env {
        EDITOR = "nvim",
        PATH = lib.mkBefore({ "$HOME/.local/bin" }),
    }

    -- Manage dotfiles
    file {
        path = "~/.gitconfig",
        content = [[
[user]
    name = Your Name
    email = you@example.com
]],
    }
end

return M
```

**Apply it:**

```bash
$ sudo sys apply
```

Your system now matches your declaration. Packages installed, environment configured, dotfiles in place.

## Why sys.lua?

### The Problem with Current Tools

**Traditional package managers** (apt, brew, pacman) are imperative and stateful. You run commands that mutate system state, making it impossible to reproduce or rollback.

**Nix** solves this brilliantly but has a steep learning curve: custom language, complex module system, and impenetrable error messages scare away newcomers.

**sys.lua bridges this gap.**

### What Makes sys.lua Different

✨ **Lua instead of Nix language** - Familiar, widely-used, easy to learn  
📦 **Prebuilt binaries first** - Install instantly without compilation  
🔒 **Reproducible by default** - Lock files pin exact versions  
👥 **Per-user configuration** - System and user-level configs coexist seamlessly  
🎯 **Module system** - NixOS-style composability without the complexity  
🌍 **True cross-platform** - Linux, macOS, and Windows as first-class citizens  
🔐 **Built-in secrets management** - SOPS integration for sensitive data  
⚡ **Fast and simple** - Content-addressed store with human-readable layout  
🔄 **Atomic operations** - Apply succeeds completely or rolls back entirely

## Key Features

### 1. Declarative Package Management

Packages are fetched from registries (like GitHub repos) and installed to an immutable store:

```lua
local M = {}

M.inputs = {
    -- Official registry (public)
    pkgs = "git:https://github.com/syslua/pkgs.git",
    -- Private registry (SSH - uses ~/.ssh/ keys)
    company = "git:git@github.com:mycompany/internal-pkgs.git",
}

function M.setup(inputs)
    local pkgs = require("inputs.pkgs")

    -- Use latest stable version
    pkgs.cli.ripgrep.setup()

    -- Pin specific version
    pkgs.cli.nodejs.setup({ version = "18.20.0" })
end

return M
```

### 2. User-Scoped Configuration

System-level and user-level configurations coexist seamlessly. Each user gets their own isolated environment while sharing system packages:

```lua
local M = {}

M.inputs = {
    pkgs = "git:https://github.com/syslua/pkgs.git",
}

function M.setup(inputs)
    local pkgs = require("inputs.pkgs")
    local lib = require("syslua.lib")

    -- System-level packages (available to all users)
    pkgs.cli.git.setup()
    pkgs.cli.curl.setup()

    -- Per-user configuration
    user {
        name = "ian",
        config = function()
            -- User-scoped packages (only in ian's PATH)
            pkgs.cli.neovim.setup()
            pkgs.cli.ripgrep.setup()

            -- User-scoped dotfiles
            file {
                path = "~/.gitconfig",
                content = [[
[user]
    name = Ian
    email = ian@example.com
]],
            }

            -- User-scoped environment
            env {
                EDITOR = "nvim",
                PATH = lib.mkBefore({ "$HOME/.local/bin" }),
            }
        end,
    }

    user {
        name = "admin",
        config = function()
            pkgs.cli.htop.setup()
            pkgs.cli.docker.setup()
        end,
    }
end

return M
```

**Each user sources their own environment:**

```bash
# In ~/.bashrc or ~/.zshrc
[ -f ~/.local/share/sys/env.sh ] && source ~/.local/share/sys/env.sh
[ -f ~/.local/share/sys/users/ian/env.sh ] && source ~/.local/share/sys/users/ian/env.sh
```

### 3. Reproducible Environments

Lock files ensure your team uses identical package versions:

```bash
# Developer A: Add packages and commit lock file
$ sudo sys apply # Creates syslua.lock
$ git add . && git commit

# Developer B: Get exact same versions
$ git pull
$ sudo sys apply # Uses pinned versions from syslua.lock
```

### 4. NixOS-Style Modules

Reusable, composable configuration modules following standard Lua patterns:

```lua
-- modules/services/docker.lua
local M = {}

M.options = {
	rootless = true,
}

function M.setup(opts)
	opts = opts or {}
	for k, v in pairs(M.options) do
		if opts[k] == nil then
			opts[k] = v
		end
	end

	require("pkgs.cli.docker").setup()
	require("pkgs.cli.docker-compose").setup()

	-- Service configuration via derive/activate
	local service_drv = derive({
		name = "docker-service",
		opts = function(sys)
			return { rootless = opts.rootless, sys = sys }
		end,
		config = function(o, ctx)
			if o.sys.os == "linux" then
				ctx.write(ctx.out .. "/docker.service", generate_systemd_unit(o))
			end
		end,
	})

	activate({
		opts = function(sys)
			return { drv = service_drv, sys = sys }
		end,
		config = function(o, ctx)
			if o.sys.os == "linux" then
				ctx.symlink(o.drv.out .. "/docker.service", "/etc/systemd/system/docker.service")
				ctx.enable_service("docker")
			end
		end,
	})

	return M
end

return M
```

```lua
-- init.lua
require("modules.services.docker").setup({ rootless = false })
```

### 5. Priority-Based Conflict Resolution

When multiple declarations conflict, priorities determine the winner:

```lua
-- Default value (can be overridden)
env({ EDITOR = lib.mkDefault("vim") })

-- User override (higher priority)
env({ EDITOR = lib.mkForce("nvim") })

-- Mergeable values combine instead of conflict
env({ PATH = lib.mkBefore({ "$HOME/.cargo/bin" }) })
env({ PATH = lib.mkAfter({ "/usr/local/games" }) })
-- Result: $HOME/.cargo/bin:$PATH:/usr/local/games
```

### 6. Built-in Secrets Management

For private repositories, **SSH is recommended** (uses existing `~/.ssh/` keys):

```lua
M.inputs = {
    -- SSH URLs - no secrets to manage
    company = "git:git@github.com:mycompany/private-pkgs.git",
}
```

For HTTPS-only environments, SOPS integration keeps credentials secure:

```yaml
# secrets.yaml (encrypted with age/GPG)
github_token: ENC[AES256_GCM,data:...,tag:...]
```

```lua
local secrets = sops.load("./secrets.yaml")

M.inputs = {
    -- HTTPS with token (fallback for CI/restricted networks)
    company = {
        url = "git:https://github.com/mycompany/private-pkgs.git",
        auth = secrets.github_token,
    },
}
```

### 7. Atomic Rollbacks

Every `sys apply` creates a snapshot. Rollback instantly if something breaks:

```bash
$ sys history
Snapshots:
#5  2024-12-08 14:23  Added Docker and PostgreSQL
#4  2024-12-07 09:15  Updated neovim to 0.10.0
#3  2024-12-06 18:42  Initial system config

$ sudo sys rollback 4 # Instant rollback to snapshot #4
```

### 8. Cross-Platform

First-class support for Linux, macOS, and Windows:

```lua
-- Platform-specific behavior using syslua globals
if syslua.is_linux then
    require("pkgs.cli.xclip").setup()
elseif syslua.is_darwin then
    -- pbcopy is built-in on macOS
elseif syslua.is_windows then
    -- clip is built-in on Windows
end

-- Packages handle platform differences internally
require("pkgs.cli.ripgrep").setup({ version = "15.1.0" })
-- The package module uses syslua.platform to fetch the correct binary
```

## Architecture Highlights

### The Two Primitives

Everything in sys.lua builds on two fundamental concepts:

```
Derivation (derive {})          Activation (activate {})
━━━━━━━━━━━━━━━━━━━━━━━━       ━━━━━━━━━━━━━━━━━━━━━━━━━━
Describes HOW to produce        Describes WHAT TO DO with
content for the store.          derivation output.

- Fetch from URL                - Add to PATH
- Clone git repo                - Create symlink
- Build from source             - Source in shell
- Generate config file          - Enable service

Output: immutable store object  Output: system side effects
```

All user-facing APIs (`file {}`, `env {}`, package `setup()`) internally create derivations and activations.

### Content-Addressed Store

Packages are stored immutably with human-readable naming:

```
/syslua/store/
├── obj/<name>-<version>-<hash>/  # Immutable content (e.g., ripgrep-15.1.0-abc123def/)
│   └── bin/rg
├── pkg/ripgrep/15.1.0/x86_64-linux/  # Human-readable symlink → obj/
├── drv/<hash>.drv                    # Serialized derivation descriptions
└── drv-out/<hash>                    # Maps derivation hash → output hash
```

### Dependency Graph (DAG)

sys.lua builds an execution graph to parallelize operations and handle dependencies:

```
┌──────────┐     ┌──────────┐
│ ripgrep  │     │  neovim  │
└──────────┘     └────┬─────┘
                      │ depends_on
                      ▼
                ┌────────────┐
                │ init.lua   │
                └────────────┘
```

### Flakes-Style Inputs

No centralized registry to sync. Declare your package sources in `M.inputs`:

```lua
local M = {}

M.inputs = {
    -- Official registry (HTTPS - public)
    pkgs = "git:https://github.com/syslua/pkgs.git",

    -- Unstable channel
    unstable = "git:https://github.com/syslua/pkgs.git#unstable",

    -- Private corporate registry (SSH - recommended)
    company = "git:git@github.com:mycompany/pkgs.git",

    -- Local development
    local_pkgs = "path:./my-packages",
}

function M.setup(inputs)
    local pkgs = require("inputs.pkgs")
    local company = require("inputs.company")

    pkgs.cli.ripgrep.setup()
    company.internal_tool.setup()
end

return M
```

## Project Status

**sys.lua is currently in the design and architecture phase.** We have:

- Comprehensive architecture documentation (see [docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md))
- Split architecture docs covering [derivations](./docs/architecture/01-derivations.md), [activations](./docs/architecture/02-activations.md), [store](./docs/architecture/03-store.md), [Lua API](./docs/architecture/04-lua-api.md), and more
- Crate structure defined (`sys-cli`, `sys-core`, `sys-lua`, `sys-platform`, `sys-sops`)
- Clear design philosophy and feature roadmap
- Implementation in progress

## Getting Involved

We're actively seeking contributors to help build sys.lua. Here's how you can help:

### For Developers

- **Rust developers**: Core functionality needs implementation
- **Lua experts**: Help design the Lua API and standard library
- **Platform specialists**: Windows/macOS/Linux-specific features
- **Package maintainers**: Build the official package registry

### For Early Adopters

- **Feedback**: Review the [architecture docs](./docs/ARCHITECTURE.md) and share thoughts
- **Use cases**: Tell us about your workflow and how sys.lua could help
- **Testing**: Try early releases and report issues

### Getting Started

```bash
# Clone the repository
$ git clone https://github.com/sys-lua/sys.lua.git
$ cd sys.lua

# Build the project (once implemented)
$ cargo build --release

# Read the architecture
$ cat docs/ARCHITECTURE.md

# Check contributor guidelines
$ cat AGENTS.md
```

## Design Principles

sys.lua is guided by these core principles:

1. **Declarative Configuration** - Config file is the single source of truth
2. **Reproducibility** - Same config = same environment, always
3. **Immutability** - Package contents never change after installation
4. **Simplicity** - Prebuilt binaries, human-readable store, straightforward Lua
5. **Cross-platform** - Linux, macOS, Windows as equals

## Inspiration

sys.lua stands on the shoulders of giants:

- **Nix/NixOS** - Reproducibility, immutability, declarative config
- **Home Manager** - User-level configuration management
- **Ansible** - Simple, readable syntax
- **Lua** - Approachable, powerful language

We're taking the best ideas from these projects and making them accessible to everyone.

## Comparison

| Feature            | sys.lua      | Nix          | Ansible    | Homebrew   |
| ------------------ | ------------ | ------------ | ---------- | ---------- |
| Declarative        | ✅           | ✅           | ✅         | ❌         |
| Reproducible       | ✅           | ✅           | ⚠️ Partial | ❌         |
| Rollback           | ✅           | ✅           | ❌         | ❌         |
| Cross-platform     | ✅           | ⚠️ Partial   | ✅         | macOS only |
| Easy to learn      | ✅           | ❌           | ✅         | ✅         |
| Prebuilt binaries  | ✅ (default) | ⚠️ Sometimes | N/A        | ✅         |
| Immutable store    | ✅           | ✅           | ❌         | ❌         |
| Secrets management | ✅ (SOPS)    | ⚠️ External  | ✅ (Vault) | ❌         |
| Module system      | ✅           | ✅           | ✅ (roles) | ❌         |

## License

[To be determined - waiting for project maintainer decision]

## Community

- **GitHub Discussions**: [Share ideas and ask questions](https://github.com/sys-lua/sys.lua/discussions)
- **Issues**: [Report bugs and request features](https://github.com/sys-lua/sys.lua/issues)
- **Discord**: [Join the community](https://discord.gg/sys-lua) _(coming soon)_

## Roadmap

See [docs/ARCHITECTURE.md](./docs/ARCHITECTURE.md) for the complete technical roadmap.

**Phase 1 - Foundation** (Current)

- [ ] Core Rust crates structure
- [ ] Lua runtime integration
- [ ] Store implementation
- [ ] Package installation/removal

**Phase 2 - Essential Features**

- [ ] Lock file support
- [ ] Input resolution
- [ ] DAG execution
- [ ] Snapshot/rollback

**Phase 3 - Advanced Features**

- [ ] Module system
- [ ] SOPS integration
- [ ] Service management
- [ ] Shell completions

**Phase 4 - Ecosystem**

- [ ] Official package registry
- [ ] Documentation site
- [ ] Tutorial series
- [ ] Community modules

---

**sys.lua** - System management that makes sense.

_Declarative. Reproducible. Simple._
