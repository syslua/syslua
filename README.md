# sys.lua

**Declarative cross-platform system management with the simplicity of Lua and the power of Nix.**

> **Note:** sys.lua is currently in the design phase. This README describes the target system. We're looking for contributors to help bring this vision to life.

## What is sys.lua?

sys.lua reimagines system configuration management by combining three powerful ideas:

1. **Declarative configuration** - Your `sys.lua` file is the single source of truth for your entire environment
2. **Reproducibility** - Same config + same inputs = identical environment, every time
3. **Simplicity** - No PhD required. Just Lua and straightforward concepts

```lua
local lib = require("sys.lib")

-- Declare your desired system state
local inputs = {
    pkgs = input "github:sys-lua/pkgs"
}

-- Install packages
pkg(inputs.pkgs.ripgrep)
pkg(inputs.pkgs.neovim)
pkg(inputs.pkgs.nodejs, "20.10.0")

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
```

**Apply it:**

```bash
$ sudo sys apply sys.lua
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
🎯 **Module system** - NixOS-style composability without the complexity
🌍 **True cross-platform** - Linux, macOS, and Windows as first-class citizens
🔐 **Built-in secrets management** - SOPS integration for sensitive data
⚡ **Fast and simple** - Content-addressed store with human-readable layout
🔄 **Atomic operations** - Apply succeeds completely or rolls back entirely

## Key Features

### 1. Declarative Package Management

Packages are fetched from registries (like GitHub repos) and installed to an immutable store:

```lua
-- Official registry
local inputs = {
    pkgs = input "github:sys-lua/pkgs"
}

-- Use latest stable version
pkg(inputs.pkgs.ripgrep)

-- Pin specific version
pkg(inputs.pkgs.nodejs, "18.20.0")

-- Multiple versions coexist peacefully
pkg(inputs.pkgs.python, "3.11.7")
pkg(inputs.pkgs.python, "3.12.0")
```

### 2. Reproducible Environments

Lock files ensure your team uses identical package versions:

```bash
# Developer A: Add packages and commit lock file
$ sudo sys apply sys.lua  # Creates sys.lock
$ git add sys.lua sys.lock && git commit

# Developer B: Get exact same versions
$ git pull
$ sudo sys apply sys.lua  # Uses pinned versions from sys.lock
```

### 3. NixOS-Style Modules

Reusable, composable configuration modules:

```lua
-- modules/docker.lua
return module "docker" {
    options = {
        enable = lib.mkOption { type = "bool", default = false },
        rootless = lib.mkOption { type = "bool", default = true },
    },

    config = function(opts)
        if not opts.enable then return end

        pkg("docker")
        pkg("docker-compose")

        service "docker" {
            enable = true,
            rootless = opts.rootless,
        }
    end,
}
```

```lua
-- sys.lua
local docker = require("./modules/docker")

docker.options.enable = true
docker.options.rootless = false
```

### 4. Priority-Based Conflict Resolution

When multiple declarations conflict, priorities determine the winner:

```lua
-- Default value (can be overridden)
env { EDITOR = lib.mkDefault("vim") }

-- User override (higher priority)
env { EDITOR = lib.mkForce("nvim") }

-- Mergeable values combine instead of conflict
env { PATH = lib.mkBefore({ "$HOME/.cargo/bin" }) }
env { PATH = lib.mkAfter({ "/usr/local/games" }) }
-- Result: $HOME/.cargo/bin:$PATH:/usr/local/games
```

### 5. Built-in Secrets Management

SOPS integration keeps credentials secure:

```lua
-- secrets.yaml (encrypted with age/GPG)
github_token: ENC[AES256_GCM,data:...,tag:...]

-- sys.lua
local secrets = sops.load("./secrets.yaml")

local inputs = {
    company = input "github:mycompany/private-pkgs" {
        auth = secrets.github_token,
    },
}
```

### 6. Atomic Rollbacks

Every `sys apply` creates a snapshot. Rollback instantly if something breaks:

```bash
$ sys history
Snapshots:
  #5  2024-12-08 14:23  Added Docker and PostgreSQL
  #4  2024-12-07 09:15  Updated neovim to 0.10.0
  #3  2024-12-06 18:42  Initial system config

$ sudo sys rollback 4  # Instant rollback to snapshot #4
```

### 7. Cross-Platform

First-class support for Linux, macOS, and Windows:

```lua
-- Platform-specific behavior
if lib.platform.isLinux then
    pkg("xclip")
elseif lib.platform.isMacOS then
    pkg("pbcopy")
elseif lib.platform.isWindows then
    pkg("clip")
end

-- Or let packages handle it
pkg "ripgrep" {
    version = "15.1.0",
    src = lib.fetchFromGitHub {
        owner = "BurntSushi",
        repo = "ripgrep",
        tag = "15.1.0",
        asset = "ripgrep-{version}-{platform}.tar.gz",
        sha256 = {
            ["x86_64-linux"] = "abc123...",
            ["aarch64-darwin"] = "def456...",
            ["x86_64-windows"] = "789xyz...",
        },
    },
}
```

## Architecture Highlights

### Content-Addressed Store

Packages are stored immutably with human-readable symlinks:

```
/syslua/store/
├── obj/<sha256>/              # Immutable content
│   └── bin/rg
└── pkg/ripgrep/15.1.0/x86_64-linux/  # Symlink → obj/<hash>
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

No centralized registry to sync. Declare your package sources right in config:

```lua
local inputs = {
    -- Official registry
    pkgs = input "github:sys-lua/pkgs",

    -- Unstable channel
    unstable = input "github:sys-lua/pkgs" { branch = "unstable" },

    -- Private corporate registry
    company = input "github:mycompany/pkgs" { auth = secrets.token },

    -- Local development
    local = input "path:./my-packages",
}
```

## Project Status

**sys.lua is currently in the design and architecture phase.** We have:

✅ Comprehensive architecture document (see [ARCHITECTURE.md](./ARCHITECTURE.md))  
✅ Crate structure defined (`cli`, `core`, `lua`, `platform`, `sops`)  
✅ Clear design philosophy and feature roadmap  
⏳ Implementation in progress

## Getting Involved

We're actively seeking contributors to help build sys.lua. Here's how you can help:

### For Developers

- **Rust developers**: Core functionality needs implementation
- **Lua experts**: Help design the Lua API and standard library
- **Platform specialists**: Windows/macOS/Linux-specific features
- **Package maintainers**: Build the official package registry

### For Early Adopters

- **Feedback**: Review the [ARCHITECTURE.md](./ARCHITECTURE.md) and share thoughts
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
$ cat ARCHITECTURE.md

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

See [ARCHITECTURE.md](./ARCHITECTURE.md) for the complete technical roadmap.

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
