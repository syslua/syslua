# sys.lua Architecture

> **Note:** This is a design document describing the target architecture for sys.lua. It serves as the specification for implementation.

sys.lua is a cross-platform declarative system/environment manager inspired by Nix.

## Terminology Glossary

To ensure consistency throughout this document, key terms are defined here:

| Term                    | Definition                                                                                                            |
| ----------------------- | --------------------------------------------------------------------------------------------------------------------- |
| **Input**               | A declared source of packages (GitHub repo, local path, Git URL). Defined in config with `input "..."`.               |
| **Registry**            | A repository of package definitions (Lua files). The official registry is `github:sys-lua/pkgs`. Accessed via inputs. |
| **Package Set**         | The collection of package definitions available from a specific input. Synonymous with "registry" in context.         |
| **Manifest**            | The intermediate representation produced by evaluating Lua config. Contains resolved packages, files, env vars, etc.  |
| **Config**              | The user's `sys.lua` file (Lua code) that declares desired system state.                                              |
| **Store**               | The global, immutable location where packages are installed (`/syslua/store`).                                        |
| **Store Object**        | An immutable, content-addressed directory in `store/obj/<hash>/`.                                                     |
| **Package Link**        | A human-readable symlink in `store/pkg/<name>/<version>/<platform>` pointing to a store object.                       |
| **Snapshot**            | A point-in-time capture of complete system state (packages, files, env, services).                                    |
| **Derivation**          | A description of how to obtain content (fetch from URL, Git, build from source).                                      |
| **Priority**            | A numeric value (lower = higher precedence) used to resolve conflicts between declarations.                           |
| **Session Variable**    | Environment variable set in shell session via sourced scripts (lost on shell exit).                                   |
| **Persistent Variable** | Environment variable written to OS-level config (survives reboot, available to all processes).                        |
| **Singular Value**      | A config value that can only have one final result (resolved by priority). Examples: `EDITOR`, package version.       |
| **Mergeable Value**     | A config value that combines all declarations (sorted by priority). Examples: `PATH`, file content sections.          |
| **DAG**                 | Directed Acyclic Graph. Represents execution order based on dependencies between packages, files, services.           |

## Design Philosophy

sys.lua is built on these core principles:

1. **Declarative Configuration**: The Lua config file is the single source of truth. The system state should always match what's declared in config.
2. **Reproducibility**: Same config + same inputs = same environment, regardless of platform.
3. **Immutability**: Package contents in the store are immutable. Changes happen by installing new versions, not modifying existing ones.
4. **Simplicity**: Prebuilt binaries when available, human-readable store layout, straightforward Lua syntax.
5. **Cross-platform**: First-class support for Linux, macOS, and Windows.

## Crate Structure

```
sys.lua/
├── crates/
│   ├── cli/       # CLI application
│   ├── core/      # Core logic: store, inputs, snapshots, build
│   ├── lua/       # Lua config parsing and module system
│   ├── platform/  # OS-specific functionality (services, env, paths)
│   └── sops/      # SOPS integration for secrets
├── lib/           # Standard library modules (Lua)
├── pkgs/          # Package definitions (Lua files)
├── modules/       # Reusable module definitions (Lua files)
├── examples/      # Example configurations
└── docs/          # Documentation
```

## Rust Dependencies

This section documents the essential Rust libraries used across all crates. All dependencies are well-established with strong community support and documentation.

### Shared Dependencies (Workspace-level)

These dependencies are used across multiple crates and defined at the workspace level:

| Crate                | Version | Purpose                                                           | Documentation                      |
| -------------------- | ------- | ----------------------------------------------------------------- | ---------------------------------- |
| `mlua`               | 0.11    | Lua 5.4 runtime with serialization and async support              | https://docs.rs/mlua               |
| `serde`              | 1.0     | Serialization/deserialization for manifest, lock files, JSON/YAML | https://serde.rs                   |
| `serde_json`         | 1.0     | JSON support for lock files, package metadata, daemon.json        | https://docs.rs/serde_json         |
| `serde_yaml`         | 0.9     | YAML support for SOPS secrets                                     | https://docs.rs/serde_yaml         |
| `thiserror`          | 2.0     | Error type derivation with Display/Error traits                   | https://docs.rs/thiserror          |
| `anyhow`             | 1.0     | Flexible error handling with context                              | https://docs.rs/anyhow             |
| `tracing`            | 0.1     | Structured logging and diagnostics                                | https://docs.rs/tracing            |
| `tracing-subscriber` | 0.3     | Log formatting and filtering                                      | https://docs.rs/tracing-subscriber |
| `tokio`              | 1.0     | Async runtime for HTTP, Git, parallel execution                   | https://tokio.rs                   |

### sys-cli Dependencies

CLI-specific dependencies for command parsing, completions, and user interaction:

| Crate           | Version   | Purpose                                                   | Documentation                 |
| --------------- | --------- | --------------------------------------------------------- | ----------------------------- |
| `clap`          | 4.5       | Command-line argument parsing with derive macros          | https://docs.rs/clap          |
| `clap_complete` | 4.5       | Shell completion generation (bash, zsh, fish, powershell) | https://docs.rs/clap_complete |
| `console`       | 0.16      | Terminal colors, progress bars, user interaction          | https://docs.rs/console       |
| `indicatif`     | 0.18      | Progress bars for downloads and long operations           | https://docs.rs/indicatif     |
| `dialoguer`     | 0.12      | Interactive prompts and confirmations                     | https://docs.rs/dialoguer     |
| `atty`          | 0.2       | Detect if running in TTY for colored output               | https://docs.rs/atty          |
| `sys-core`      | workspace | Core logic (internal)                                     | -                             |
| `sys-platform`  | workspace | Platform abstractions (internal)                          | -                             |

### sys-core Dependencies

Core logic dependencies for store management, HTTP, Git, hashing, and manifest handling:

| Crate          | Version   | Purpose                                              | Documentation              |
| -------------- | --------- | ---------------------------------------------------- | -------------------------- |
| `mlua`         | 0.11      | Lua runtime integration (used for config evaluation) | https://docs.rs/mlua       |
| `reqwest`      | 0.12      | HTTP client for fetchUrl, GitHub/GitLab releases     | https://docs.rs/reqwest    |
| `gix`          | 0.75      | Git operations for fetchGit, input cloning           | https://docs.rs/gix        |
| `sha2`         | 0.10      | SHA-256 hashing for content addressing               | https://docs.rs/sha2       |
| `hex`          | 0.4       | Hex encoding/decoding for hashes                     | https://docs.rs/hex        |
| `tar`          | 0.4       | Extract .tar.gz archives from downloads              | https://docs.rs/tar        |
| `flate2`       | 1.0       | Gzip compression/decompression                       | https://docs.rs/flate2     |
| `zip`          | 6.0       | Extract .zip archives (Windows releases)             | https://docs.rs/zip        |
| `walkdir`      | 2.5       | Recursive directory traversal for store GC           | https://docs.rs/walkdir    |
| `tempfile`     | 3.10      | Temporary directories for downloads, builds          | https://docs.rs/tempfile   |
| `semver`       | 1.0       | Semantic version parsing and comparison              | https://docs.rs/semver     |
| `petgraph`     | 0.8       | DAG construction and topological sorting             | https://docs.rs/petgraph   |
| `rayon`        | 1.10      | Parallel execution of DAG nodes                      | https://docs.rs/rayon      |
| `toml`         | 0.9       | TOML parsing for lock files (alternative to JSON)    | https://docs.rs/toml       |
| `sys-lua`      | workspace | Lua integration (internal)                           | -                          |
| `sys-platform` | workspace | Platform abstractions (internal)                     | -                          |
| `sys-sops`     | workspace | SOPS integration (internal)                          | -                          |

### sys-lua Dependencies

Lua-specific dependencies for runtime, config parsing, and module system:

| Crate   | Version | Purpose                            | Documentation        |
| ------- | ------- | ---------------------------------- | -------------------- |
| `mlua`  | 0.11    | Lua 5.4 runtime with safe bindings | https://docs.rs/mlua |
| `serde` | 1.0     | Convert Lua tables to Rust structs | https://serde.rs     |

**mlua features enabled:**

- `lua54` - Use Lua 5.4 (latest stable)
- `serialize` - Serde integration for Lua tables
- `async` - Async function support for HTTP/Git operations
- `vendored` - Bundle Lua to avoid system dependency

### sys-platform Dependencies

Platform-specific dependencies for OS detection, paths, services, and environment variables:

| Crate     | Version | Purpose                                         | Documentation           |
| --------- | ------- | ----------------------------------------------- | ----------------------- |
| `dirs`    | 6.0     | Standard directory paths (home, config, cache)  | https://docs.rs/dirs    |
| `whoami`  | 1.5     | User/system information (username, hostname)    | https://docs.rs/whoami  |
| `libc`    | 0.2     | Unix system calls (chattr, chflags)             | https://docs.rs/libc    |
| `winapi`  | 0.3     | Windows API bindings (ACLs, registry, services) | https://docs.rs/winapi  |
| `nix`     | 0.30    | Unix/POSIX system APIs (Linux/macOS)            | https://docs.rs/nix     |
| `sysinfo` | 0.37    | System information (OS version, architecture)   | https://docs.rs/sysinfo |

**Platform-specific features:**

- Linux: `libc`, `nix` for immutability flags, systemd service management
- macOS: `libc`, `nix` for immutability flags, launchd service management
- Windows: `winapi` for ACLs, registry, Windows services

### sys-sops Dependencies

SOPS integration dependencies for encrypted secrets management:

| Crate    | Version | Purpose                              | Documentation          | Notes              |
| -------- | ------- | ------------------------------------ | ---------------------- | ------------------ |
| `age`    | 0.11    | Age encryption/decryption            | https://docs.rs/age    | Pure Rust, default |
| `base64` | 0.22    | Base64 encoding for encrypted values | https://docs.rs/base64 | Always enabled     |

**Features:**
- `default` = `["age"]` - Age encryption enabled by default

**Note:** SOPS file format handling is implemented in Rust rather than shelling out to the `sops` binary for better cross-platform support and error handling. Only Age encryption is supported (pure Rust, no system dependencies). GPG support is not included - users needing GPG should use the `sops` CLI directly.

### Development Dependencies

Testing and development dependencies used across crates:

| Crate        | Version | Purpose                                     | Documentation              |
| ------------ | ------- | ------------------------------------------- | -------------------------- |
| `tempfile`   | 3.10    | Temporary directories for integration tests | https://docs.rs/tempfile   |
| `assert_cmd` | 2.0     | CLI testing utilities                       | https://docs.rs/assert_cmd |
| `predicates` | 3.1     | Assertions for CLI output                   | https://docs.rs/predicates |
| `mockito`    | 1.4     | HTTP mock server for testing fetchUrl       | https://docs.rs/mockito    |
| `proptest`   | 1.4     | Property-based testing for manifest merging | https://docs.rs/proptest   |

### Dependency Selection Criteria

All dependencies were selected based on these criteria:

1. **Maturity**: Version 1.0+ or widely adopted in the Rust ecosystem
2. **Maintenance**: Active development with recent releases
3. **Documentation**: Comprehensive docs and examples
4. **Community**: Large user base and ecosystem support
5. **Cross-platform**: Works on Linux, macOS, and Windows
6. **Performance**: Efficient implementation with minimal overhead
7. **Security**: Regular security audits and CVE tracking

### sys-cli

The command-line interface. Provides commands for applying configs, managing packages, and system introspection.

**Commands:**

- `apply [sys.lua]` - Apply a configuration file (declarative - installs and removes)
- `plan [sys.lua]` - Dry-run showing what changes would be made (no root required)
- `status` - Show current environment status (no root required)
- `list` - List installed packages
- `history` - Show snapshot history with details
- `rollback [snapshot_id]` - Rollback to a previous snapshot
- `gc` - Garbage collect orphaned objects from store
- `update [input]` - Update lock file inputs (all or specific)
- `shell` - Enter project environment or ephemeral shell
- `env` - Print environment activation script for current shell
- `secrets rotate` - Re-encrypt secrets with new keys
- `secrets set <key>` - Set a secret value
- `completions <shell>` - Generate shell completions

**Shell Completions:**

sys.lua provides shell completion scripts for all major shells. These provide tab-completion for commands, flags, and dynamic values like package names and snapshot IDs.

```bash
# Generate completions (writes to stdout)
$ sys completions bash
$ sys completions zsh
$ sys completions fish
$ sys completions powershell

# Install for bash (add to ~/.bashrc)
$ sys completions bash > ~/.local/share/bash-completion/completions/sys
# Or source directly:
$ echo 'eval "$(sys completions bash)"' >> ~/.bashrc

# Install for zsh (add to ~/.zshrc)
$ sys completions zsh > ~/.local/share/zsh/site-functions/_sys
# Or source directly:
$ echo 'eval "$(sys completions zsh)"' >> ~/.zshrc

# Install for fish
$ sys completions fish > ~/.config/fish/completions/sys.fish

# Install for PowerShell (add to $PROFILE)
$ sys completions powershell >> $PROFILE
```

**What Gets Completed:**

| Context              | Completions                                             |
| -------------------- | ------------------------------------------------------- |
| `sys <TAB>`          | Commands: apply, plan, status, list, rollback, gc, etc. |
| `sys apply <TAB>`    | Files: \*.lua files in current directory                |
| `sys rollback <TAB>` | Dynamic: snapshot IDs from history                      |
| `sys --<TAB>`        | Flags: --help, --version, --verbose, etc.               |
| `sys update <TAB>`   | Dynamic: input names from current config                |

**Implementation Notes:**

- Built using `clap_complete` crate for static completions
- Dynamic completions (snapshots, inputs) use shell-specific hooks
- Completions are generated at build time, not runtime (fast startup)

### sys-core

Core functionality shared across CLI and agent.

**Modules:**

- `manifest` - Manifest data structures and validation
- `priority` - Priority-based conflict resolution (mkDefault, mkForce, etc.)
- `merge` - Declaration merging for singular and mergeable values
- `dag` - Execution DAG construction and topological sorting
- `store` - Package installation, uninstallation, garbage collection
- `inputs` - Input resolution, lock file management
- `snapshot` - State tracking, rollback support
- `plan` - Diff computation between manifest and current state
- `executor` - DAG execution engine with parallel support
- `service` - Cross-platform service management (systemd/launchd/Windows)
- `build` - Build-from-source derivation support
- `secrets` - SOPS integration for secrets management
- `env` - Environment script generation
- `activation` - Activation script hooks
- `types` - Shared data structures
- `error` - Error types

### sys-lua

Lua integration using `mlua` crate.

**Responsibilities:**

- Parse user config files (`sys.lua`)
- Parse registry package definitions (`registry/*.lua`)
- Provide Lua APIs at multiple abstraction levels
- Evaluate configuration and produce a declarative manifest

**Lua API Layers:**

The Lua API is structured in layers, from low-level primitives to high-level abstractions:

```
┌─────────────────────────────────────────────┐
│  High-level: pkg(), file{}, env{}, user{}   │  ← User-facing config API
├─────────────────────────────────────────────┤
│  Mid-level: fetchUrl, fetchGit, etc.        │  ← Building blocks for packages
├─────────────────────────────────────────────┤
│  Low-level: Rust bindings (mlua)            │  ← Core runtime
└─────────────────────────────────────────────┘
```

**Global Fetch Helpers (`sys.lib`):**

**Global API:**

sys.lua provides a minimal global API. System information is available via the `sys` global table:

```lua
-- System information
sys.platform       -- "aarch64-darwin"
sys.os             -- "darwin" | "linux" | "windows"
sys.arch           -- "aarch64" | "x86_64" | "arm"
sys.hostname       -- "macbook-pro"
sys.username       -- "ian"

-- Boolean helpers for common checks
sys.is_linux       -- true if Linux
sys.is_darwin      -- true if macOS
sys.is_windows     -- true if Windows
```

**Fetch helpers** are available via the `lib` module:

```lua
local lib = require("sys.lib")

-- Fetch from a URL with hash verification
lib.fetchUrl {
    url = "https://example.com/tool-1.0.tar.gz",
    sha256 = "abc123...",
}

-- Fetch source code from a Git repository
lib.fetchGit {
    url = "https://github.com/user/repo",
    rev = "v1.0.0",  -- tag, branch, or commit
    sha256 = "def456...",
}

-- Fetch source code from GitHub (convenience wrapper for fetchGit)
lib.fetchFromGitHub {
    owner = "BurntSushi",
    repo = "ripgrep",
    rev = "15.1.0",  -- tag, branch, or commit
    sha256 = "...",
}

-- Fetch source code from GitLab (convenience wrapper for fetchGit)
lib.fetchFromGitLab {
    owner = "user",
    repo = "project",
    rev = "v1.0.0",
    sha256 = "...",
}

-- JSON encoding for config files
lib.toJSON(table)  -- Returns JSON string
```

**Derivations:**

Fetch helpers return derivation objects that describe how to obtain content. These are composed to build packages:

```lua
-- pkgs/ripgrep.lua - Example 1: Prebuilt binaries from GitHub releases
local lib = require("sys.lib")

local hashes = {
    ["x86_64-linux"] = "abc123...",
    ["aarch64-darwin"] = "def456...",
    ["x86_64-darwin"] = "789012...",
}

pkg "ripgrep" {
    version = "15.1.0",
    src = lib.fetchUrl {
        url = "https://github.com/BurntSushi/ripgrep/releases/download/15.1.0/ripgrep-15.1.0-" .. sys.platform .. ".tar.gz",
        sha256 = hashes[sys.platform],
    },
    bin = { "rg" },
}

-- pkgs/ripgrep.lua - Example 2: Build from source
pkg "ripgrep" {
    version = "15.1.0",
    src = lib.fetchFromGitHub {
        owner = "BurntSushi",
        repo = "ripgrep",
        rev = "15.1.0",
        sha256 = "source_code_hash...",
    },
    build = function(src, opts)
        return {
            buildInputs = { "rust" },
            buildPhase = [[cargo build --release]],
            installPhase = [[
                mkdir -p $out/bin
                cp target/release/rg $out/bin/
            ]],
        }
    end,
    bin = { "rg" },
}
```

**Platform-specific hashes:**

When `sha256` is a table keyed by platform, sys.lua looks up the current platform's hash at **evaluation time**. If the platform is not found, evaluation fails immediately with a clear error:

```lua
local lib = require("sys.lib")

local hashes = {
    ["x86_64-linux"] = "abc123...",
    ["aarch64-darwin"] = "def456...",
    -- x86_64-darwin not listed
}

pkg "ripgrep" {
    version = "15.1.0",
    src = lib.fetchUrl {
        url = "https://github.com/BurntSushi/ripgrep/releases/download/15.1.0/ripgrep-15.1.0-" .. sys.platform .. ".tar.gz",
        sha256 = hashes[sys.platform],  -- Error if platform not in table
    },
    bin = { "rg" },
}
```

```
$ sys apply sys.lua
Error: No sha256 hash for platform 'x86_64-darwin' in package 'ripgrep@15.1.0'
  Available platforms: x86_64-linux, aarch64-darwin

  Either add a hash for x86_64-darwin, or provide a 'build' function
  to build from source on unsupported platforms.
```

**This is intentional:** Missing platform support should never be silently ignored. The user must either:

1. Add the missing platform hash
2. Provide a `build` function for source builds
3. Use platform conditionals to exclude the package

```lua
-- Option 3: Platform conditionals (native Lua)
if sys.platform ~= "x86_64-darwin" then
    pkg "ripgrep" { ... }
end

-- Or using sys.os
if sys.is_linux then
    pkg "xclip" { ... }
end
```

### Fetch Helper Implementation

Fetch helpers (`fetchUrl`, `fetchGit`, `fetchFromGitHub`, etc.) are evaluated during the **manifest generation phase**, not at apply time. They produce derivation objects that describe how to obtain content.

**fetchUrl Behavior:**

```lua
lib.fetchUrl {
    url = "https://example.com/tool-1.0.tar.gz",
    sha256 = "abc123...",

    -- Optional: authentication
    headers = {
        ["Authorization"] = "Bearer " .. secrets.token,
    },

    -- Optional: follow redirects (default: true, max: 10)
    followRedirects = true,

    -- Optional: timeout in seconds (default: 300)
    timeout = 600,
}
```

**HTTP behavior:**

- Follows redirects up to 10 hops by default
- Validates TLS certificates (uses system CA bundle)
- Respects proxy environment variables (`HTTP_PROXY`, `HTTPS_PROXY`, `NO_PROXY`)
- User-Agent: `sys.lua/<version> (platform)`
- Returns a derivation object with the download URL and hash

**fetchFromGitHub/fetchFromGitLab:**

These are convenience wrappers around `fetchGit` that construct repository URLs:

```lua
-- fetchFromGitHub expands to:
lib.fetchGit {
    url = "https://github.com/" .. owner .. "/" .. repo,
    rev = rev,
    sha256 = sha256,
}

-- fetchFromGitLab expands to:
lib.fetchGit {
    url = "https://gitlab.com/" .. owner .. "/" .. repo,
    rev = rev,
    sha256 = sha256,
}
```

**Use `fetchUrl` for release assets:**

To download prebuilt binaries or release archives, use `fetchUrl` directly:

```lua
local lib = require("sys.lib")

-- Download release asset
lib.fetchUrl {
    url = "https://github.com/owner/repo/releases/download/v1.0.0/binary-" .. sys.platform .. ".tar.gz",
    sha256 = hashes[sys.platform],
}
```

**fetchGit Behavior:**

```lua
lib.fetchGit {
    url = "https://github.com/user/repo",
    rev = "v1.0.0",  -- Can be: tag, branch, or commit SHA
    sha256 = "...",

    -- Optional: shallow clone (default: true for tags/commits, false for branches)
    shallow = true,

    -- Optional: submodules (default: false)
    fetchSubmodules = true,
}
```

**Git cloning process:**

1. Clone repository to temporary location
2. Checkout specified rev
3. Remove `.git` directory (makes it a plain directory)
4. Compute content hash of result
5. If hash matches, move to store; otherwise error

### sys-platform

OS abstraction layer.

**Provides:**

- Store/config/cache paths per OS
- Platform identifier (e.g., `aarch64-darwin`)
- Immutability flags (`chflags`, `chattr`, ACLs)
- Environment variable management

---

## Store Design

### Store Locations

sys.lua uses a multi-level store architecture:

**System Store (managed by admin/root):**

| Platform | System Store Path |
| -------- | ----------------- |
| Linux    | `/syslua/store`   |
| macOS    | `/syslua/store`   |
| Windows  | `C:\syslua\store` |

**User Store (managed by each user, no sudo required):**

| Platform | User Store Path |
| -------- | --------------- |
| Linux    | `~/.local/share/sys/store` |
| macOS    | `~/Library/Application Support/sys/store` |
| Windows  | `%LOCALAPPDATA%\sys\store` |

### System Store Layout

```
/syslua/store/
├── obj/<sha256>/           # Immutable content-addressed objects (world-readable)
│   ├── bin/
│   ├── lib/
│   └── ...
├── pkg/<name>/<ver>/<plat> # Symlinks → obj/<hash> (human-readable)
├── drv/<hash>.drv          # Derivation files (build instructions)
├── drv-out/<hash>          # Maps derivation hash → output hash
└── metadata/
    ├── manifest.json       # Current system manifest
    ├── snapshots.json      # System snapshots
    └── gc-roots/           # GC roots to prevent cleanup
```

### User Store Layout

```
~/.local/share/sys/
├── store/
│   ├── obj/<sha256>/       # User's packages (or hardlinks to system store)
│   ├── pkg/<name>/<ver>/   # User's package symlinks
│   ├── drv/<hash>.drv      # User's build derivations
│   └── metadata/
│       ├── manifest.json   # Current user manifest
│       ├── snapshots.json  # User snapshots
│       └── gc-roots/       # User GC roots
├── env.sh                  # Generated environment script (bash/zsh)
├── env.fish                # Generated environment script (fish)
├── env.ps1                 # Generated environment script (PowerShell)
└── config/
    └── sys.lua             # User's configuration (optional default location)
```

**Benefits of Multi-Level Store:**

- ✅ System packages installed once, shared by all users
- ✅ Users can hardlink to system packages (no duplication)
- ✅ Users can install additional packages without sudo
- ✅ System configuration protected from user modification
- ✅ User configurations independent of each other

### Store Deduplication

When a user installs a package that exists in the system store:

```bash
# System admin installs git
sudo sys apply /etc/sys/system.lua
  → Installs to: /syslua/store/obj/abc123.../

# User wants git in their config
sys apply ~/.config/sys/sys.lua
  → Checks: Does /syslua/store/obj/abc123.../ exist?
  → If same filesystem: Creates hardlink
    ~/.local/share/sys/store/pkg/git/2.40.0/ → /syslua/store/obj/abc123.../
  → If different filesystem: Just reference via PATH
```

**Hardlink deduplication:**
- Zero additional disk space for duplicate packages
- Both stores point to same inode
- Works if user store and system store are on same filesystem
- Falls back to PATH reference if different filesystems

### Immutability

Objects in `obj/<hash>/` are made immutable after extraction:

**System store objects:**
- **Linux:** `chattr +i` (requires root to modify)
- **macOS:** `chflags uchg` (requires root to modify)
- **Windows:** ACL restrictions (requires admin to modify)
- **World-readable:** `chmod 755` (directories), `chmod 644` (files)

**User store objects:**
- **Same immutability flags** (user owns them, but makes immutable)
- **Purpose:** Prevent accidental modification
- **Removal:** User can run `sys gc` to remove (clears immutability first)

---

## Inputs and Registry (Flakes-style)

Instead of a separate registry sync mechanism, sys.lua uses declarative inputs defined in the config file itself, similar to Nix Flakes:

### Input Declaration

```lua
-- sys.lua
local lib = require("sys.lib")
local secrets = sops.load("./secrets.yaml")  -- For private input auth

local inputs = {
    -- Official package registry (public, no auth)
    pkgs = input "github:sys-lua/pkgs" {
        rev = "a1b2c3d4...",  -- pinned commit (optional, defaults to latest)
    },

    -- Additional package sets
    unstable = input "github:sys-lua/pkgs" {
        branch = "unstable",
    },

    -- Private/corporate registry (authenticated via SOPS)
    company = input "github:mycompany/sys-pkgs" {
        rev = "...",
        auth = secrets.github_token,  -- GitHub PAT from secrets
    },
        rev = "...",
    },

    -- Local path (for development)
    local_pkgs = input "path:./my-packages",

    -- Git URL
    custom = input "git:https://git.example.com/pkgs.git" {
        rev = "v1.0.0",
    },
}

-- Use packages from inputs (latest version in registry)
pkg(inputs.pkgs.ripgrep)
pkg(inputs.unstable.neovim)
pkg(inputs.company.internal_tool)

-- Pin to specific version (registry contains multiple versions)
pkg(inputs.pkgs.nodejs, "18.20.0")  -- Use nodejs 18 LTS
pkg(inputs.pkgs.nodejs, "20.10.0")  -- Different config could use nodejs 20

-- Version constraints
pkg(inputs.pkgs.python, "^3.11")    -- Latest 3.11.x
pkg(inputs.pkgs.go, "~1.21")        -- Latest 1.21.x patch
```

### Registry Structure

The official registry contains multiple versions of each package, solving the Nix version pinning problem:

```
sys-lua/pkgs/
├── ripgrep/
│   ├── default.lua          # Points to latest stable
│   ├── 15.1.0.lua
│   ├── 14.1.0.lua
│   └── 13.0.0.lua
├── nodejs/
│   ├── default.lua          # Points to latest LTS
│   ├── 22.0.0.lua           # Current
│   ├── 20.10.0.lua          # LTS
│   ├── 18.20.0.lua          # LTS
│   └── 16.20.0.lua          # Maintenance
└── python/
    ├── default.lua
    ├── 3.12.0.lua
    ├── 3.11.7.lua
    └── 3.10.13.lua
```

**Version selection behavior:**

- `pkg(inputs.pkgs.ripgrep)` - Uses `default.lua` (latest stable)
- `pkg(inputs.pkgs.ripgrep, "14.1.0")` - Uses exact version `14.1.0.lua`
- `pkg(inputs.pkgs.nodejs, "^18")` - Finds latest `18.x.x` in registry
- Version not found in registry = error (no automatic fetching)

### Package References

When you access `inputs.pkgs.ripgrep`, it returns a **package definition** - a Lua table containing all the metadata needed to install the package:

```lua
-- What inputs.pkgs.ripgrep resolves to (from registry's ripgrep/default.lua):
{
    name = "ripgrep",
    version = "15.1.0",
    src = { ... },  -- fetchFromGitHub derivation
    bin = { "rg" },
}

-- The pkg() function accepts either:
pkg(inputs.pkgs.ripgrep)              -- Package definition table
pkg(inputs.pkgs.ripgrep, "14.1.0")    -- Definition + version override
pkg "my-tool" { ... }                  -- Inline definition
```

**How version override works:**

```lua
-- When you call:
pkg(inputs.pkgs.nodejs, "18.20.0")

-- sys.lua looks up nodejs/18.20.0.lua in the registry instead of nodejs/default.lua
-- The second argument is NOT modifying the package - it's selecting a different definition
```

### Lock File

sys.lua generates a `sys.lock` file **alongside each `sys.lua` config**. This enables:

- **System configs**: `~/sys.lua` → `~/sys.lock`
- **Project configs**: `./sys.lua` → `./sys.lock` (committed to version control)

Lock files pin all inputs to exact revisions for reproducibility:

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

**Lock file behavior:**

| Scenario           | Behavior                             |
| ------------------ | ------------------------------------ |
| `sys.lock` exists  | Use pinned revisions from lock file  |
| `sys.lock` missing | Resolve latest, create lock file     |
| `sys update`       | Update lock file to latest revisions |
| Lock file mismatch | Error (prevents accidental updates)  |

**Team workflow:**

```bash
# Developer A: Add new input, commit lock file
git add sys.lua sys.lock
git commit -m "Add nodejs to project"

# Developer B: Pull and apply (uses same pinned versions)
git pull
sudo sys apply sys.lua
```

**Commands:**

```bash
sys update                    # Update all inputs to latest
sys update pkgs               # Update specific input
sys update --commit           # Update and commit lock file
```

### Input Authentication

Private inputs (corporate registries, private GitHub repos) require authentication. sys.lua uses **SOPS-encrypted secrets** for secure credential storage:

```lua
-- secrets.yaml (encrypted with SOPS)
github_token: ENC[AES256_GCM,data:...,tag:...]
gitlab_token: ENC[AES256_GCM,data:...,tag:...]
```

```lua
-- sys.lua
local secrets = sops.load("./secrets.yaml")

local inputs = {
    -- Public input (no auth)
    pkgs = input "github:sys-lua/pkgs",

    -- Private GitHub input
    company = input "github:mycompany/private-pkgs" {
        auth = secrets.github_token,
    },

    -- Private GitLab input
    internal = input "gitlab:internal.company.com/pkgs" {
        auth = secrets.gitlab_token,
    },

    -- SSH-based input (uses system SSH keys)
    private = input "git:git@github.com:mycompany/pkgs.git" {
        -- No auth needed, uses ~/.ssh/id_* keys
    },
}
```

**Authentication methods:**

| Input Type          | Auth Method                  |
| ------------------- | ---------------------------- |
| `github:` (public)  | None required                |
| `github:` (private) | `auth = "<PAT>"` from SOPS   |
| `gitlab:`           | `auth = "<token>"` from SOPS |
| `git:` (HTTPS)      | `auth = "<token>"` from SOPS |
| `git:` (SSH)        | System SSH keys (~/.ssh/)    |

**Security notes:**

- Never commit plaintext tokens to `sys.lua`
- Use SOPS to encrypt credentials in `secrets.yaml`
- The `auth` field is never written to `sys.lock`

### Input Resolution Algorithm

When `sys apply` runs, inputs are resolved using this algorithm:

```
RESOLVE_INPUTS(config, lock_file):
    inputs = {}

    FOR EACH input_decl IN config.inputs:
        input_id = input_decl.name

        // Check if lock file exists and has this input
        IF lock_file EXISTS AND lock_file.inputs[input_id] EXISTS:
            locked = lock_file.inputs[input_id]

            // Validate lock entry matches config (ignore rev/branch changes)
            IF locked.type != input_decl.type OR locked.url != input_decl.url:
                ERROR "Lock file mismatch for input '{input_id}'."
                      "Config specifies {input_decl.type}:{input_decl.url}, "
                      "but lock has {locked.type}:{locked.url}."
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
    cache_path = "~/.cache/sys/inputs/{cache_key}"

    IF cache_path EXISTS:
        RETURN cache_path

    SWITCH input_decl.type:
        CASE "github", "gitlab":
            // Download tarball archive
            tarball_url = CONSTRUCT_ARCHIVE_URL(input_decl, rev)
            DOWNLOAD(tarball_url, cache_path, auth=input_decl.auth)
            EXTRACT(cache_path)

        CASE "git":
            GIT.clone(input_decl.url, cache_path, rev=rev, auth=input_decl.auth)
            REMOVE(cache_path + "/.git")  // Strip git metadata

        CASE "path":
            // Symlink or copy local directory
            SYMLINK(input_decl.path, cache_path)

    RETURN cache_path
```

**Lock file validation rules:**

| Scenario                        | Behavior                                 |
| ------------------------------- | ---------------------------------------- |
| Lock exists, input unchanged    | Use locked `rev`                         |
| Lock exists, input URL changed  | Error (must run `sys update`)            |
| Lock exists, input type changed | Error (must run `sys update`)            |
| Lock missing for input          | Resolve latest, add to lock              |
| Lock file missing entirely      | Resolve all inputs, create lock          |
| `sys update` command            | Re-resolve specified inputs, update lock |
| `sys update --commit`           | Update lock and `git commit` it          |

**Update strategies:**

```bash
# Update all inputs to latest
$ sys update

# Update specific input
$ sys update pkgs

# Update and commit (useful for CI)
$ sys update --commit -m "Update package registry"

# Dry-run (show what would change)
$ sys update --dry-run
```

### Custom Package Definitions

Users can define custom packages directly in their `sys.lua` using the same primitives as registry packages. This is useful for:

- Internal/proprietary tools not in the public registry
- Forks or patched versions of existing packages
- Tools from private repositories

```lua
local lib = require("sys.lib")

-- Custom package from GitHub release (prebuilt binaries)
local hashes = {
    ["x86_64-linux"] = "abc123...",
    ["aarch64-darwin"] = "def456...",
}

pkg "my-internal-tool" {
    version = "2.1.0",
    src = lib.fetchUrl {
        url = "https://github.com/mycompany/internal-tool/releases/download/v2.1.0/internal-tool-2.1.0-" .. sys.platform .. ".tar.gz",
        sha256 = hashes[sys.platform],
    },
    bin = { "internal-tool" },
}

-- Custom package from URL
pkg "legacy-app" {
    version = "1.0.0",
    src = lib.fetchUrl {
        url = "https://internal.mycompany.com/releases/legacy-app-1.0.0.tar.gz",
        sha256 = "...",
    },
    bin = { "legacy-app" },
}

-- Build from source (when no prebuilt binary exists)
pkg "custom-cli" {
    version = "0.5.0",
    src = lib.fetchFromGitHub {
        owner = "user",
        repo = "custom-cli",
        rev = "v0.5.0",
        sha256 = "...",
    },
    build = function(src, opts)
        return {
            buildInputs = { "rust" },
            buildPhase = [[cargo build --release]],
            installPhase = [[
                mkdir -p $out/bin
                cp target/release/custom-cli $out/bin/
            ]],
        }
    end,
    bin = { "custom-cli" },
}

-- Mix registry packages with custom packages
pkg(inputs.pkgs.ripgrep)           -- From registry
pkg "my-internal-tool" { ... }     -- Custom definition
```

**Package definition fields:**

| Field     | Required | Description                                                   |
| --------- | -------- | ------------------------------------------------------------- |
| `version` | Yes      | Semantic version string                                       |
| `src`     | Yes      | Source derivation (fetchUrl, fetchGit, fetchFromGitHub, etc.) |
| `bin`     | No       | List of binary names to add to PATH                           |
| `build`   | No       | Build function for source builds (omit for prebuilt)          |
| `config`  | No       | Runtime configuration function                                |
| `options` | No       | Configurable options for this package                         |
| `hooks`   | No       | Lifecycle hooks (postInstall, postUpdate, preRemove)          |

### Package Options System

Packages can declare configurable options that users can set when installing them. Options use Lua's type annotation system for IDE/LSP support.

**Defining package options:**

```lua
-- pkgs/neovim.lua

---@class NeovimOptions
---@field withPython boolean Enable Python provider
---@field withNodejs boolean Enable Node.js provider  
---@field withClipboard boolean Enable system clipboard integration

pkg "neovim" {
    version = "0.10.0",
    src = lib.fetchFromGitHub { ... },
    bin = { "nvim" },

    -- Declare configurable options with defaults
    options = {
        withPython = false,
        withNodejs = false,
        withClipboard = true,
    },

    -- Config function receives resolved options
    ---@param opts NeovimOptions
    config = function(opts)
        -- Runtime dependencies based on options
        if opts.withPython then
            pkg("python3")
            pkg("pynvim")
        end

        if opts.withNodejs then
            pkg("nodejs")
            pkg("neovim-node-client")
        end

        if opts.withClipboard and sys.is_linux then
            pkg("xclip")
        end

        env {
            EDITOR = lib.mkDefault("nvim"),
        }
    end,
}
```

**Setting package options (user config):**

```lua
-- sys.lua - Method 1: inline options
pkg(inputs.pkgs.neovim, {
    withPython = true,
    withNodejs = true,
})

-- Method 2: modify package definition
local nvim = inputs.pkgs.neovim
nvim.options.withPython = true
nvim.options.withNodejs = true
pkg(nvim)
```

**Option resolution order:**

1. User-specified options (highest priority)
2. Package default values

**Type safety:**

Using `---@class` and `---@field` annotations provides:
- IDE autocomplete for option names
- Type checking in editors with Lua Language Server
- Documentation in hover tooltips
- Zero runtime cost (comments are ignored)

**Options with dependencies:**

Package options can depend on other packages being installed:

```lua
---@class MyAppOptions
---@field database "postgresql" | "mysql" | "sqlite" Database backend to use

pkg "myapp" {
    version = "1.0.0",
    src = { ... },

    options = {
        database = "sqlite",  -- default
    },

    ---@param opts MyAppOptions
    config = function(opts)
        -- Install appropriate database package
        if opts.database == "postgresql" then
            pkg("postgresql")
            env { DATABASE_URL = "postgresql://localhost/myapp" }
        elseif opts.database == "mysql" then
            pkg("mysql")
            env { DATABASE_URL = "mysql://localhost/myapp" }
        end
    end,
}
```

---

## Module System

sys.lua uses standard Lua `require()` for imports and provides a NixOS-inspired module system for reusable, composable configurations.

### Module Evaluation

**Modules are automatically evaluated via system introspection.** When you `require()` a module and set its options, you don't need to manually call its config function. The sys.lua runtime:

1. Tracks all modules that were `require()`'d during config evaluation
2. After the main config finishes, introspects all loaded modules
3. Evaluates each module's `config` function with its resolved options
4. Merges all declarations into the final manifest

```lua
-- sys.lua
local docker = require("./modules/docker")
local postgres = require("./modules/postgres")

-- Just set options - no need to call config() manually
docker.options.enable = true
docker.options.rootless = false

postgres.options.enable = true
postgres.options.port = 5433

-- sys.lua runtime automatically:
-- 1. Sees docker and postgres were required
-- 2. Calls docker.config(docker.options)
-- 3. Calls postgres.config(postgres.options)
-- 4. Merges results into manifest
```

### Module Evaluation Implementation

The module auto-evaluation system uses Lua's require tracking to automatically evaluate modules:

**Module Registration (in sys-lua crate):**

```rust
// Modules are just plain Lua tables - no special registration needed
// sys.lua tracks which modules were require()'d by hooking into package.loaded
impl LuaModule {
    pub fn track_requires(lua: &Lua) -> Result<()> {
        // Hook into Lua's require() to track loaded modules
        let modules_table = lua.create_table()?;
        lua.globals().set("__sys_modules", modules_table)?;
        
        // Override require() to track module loads
        lua.load(r#"
            local original_require = require
            function require(name)
                local module = original_require(name)
                
                -- If module has options and config, track it
                if type(module) == "table" and module.options and module.config then
                    __sys_modules[name] = module
                end
                
                return module
            end
        "#).exec()?;
        
        Ok(())
    }
}
```

**Evaluation Algorithm:**

```
EVALUATE_CONFIG(config_path):
    // Phase 1: Execute user config
    lua = CREATE_LUA_RUNTIME()
    INJECT_SYS_GLOBALS(lua)  // pkg, file, env, etc.
    TRACK_MODULE_REQUIRES(lua)  // Hook into require()
    lua.globals().__sys_declarations = []

    EXECUTE_LUA_FILE(lua, config_path)

    // Phase 2: Collect top-level declarations
    top_level_decls = lua.globals().__sys_declarations

    // Phase 3: Auto-evaluate all loaded modules
    modules = lua.globals().__sys_modules
    module_decls = []

    FOR EACH (name, mod) IN modules:
        IF mod.options IS MODIFIED BY USER:
            // Call module's config function with resolved options
            mod_result = mod.config(mod.options)
            module_decls.append(mod_result.declarations)

    // Phase 4: Merge all declarations
    all_decls = top_level_decls + module_decls
    manifest = BUILD_MANIFEST(all_decls)

    RETURN manifest

INJECT_SYS_GLOBALS(lua):
    // pkg() - adds to __sys_declarations
    lua.globals().pkg = |args| {
        decl = CREATE_PACKAGE_DECL(args)
        lua.globals().__sys_declarations.append(decl)
    }

    // file{} - adds to __sys_declarations
    lua.globals().file = |args| {
        decl = CREATE_FILE_DECL(args)
        lua.globals().__sys_declarations.append(decl)
    }

    // env{} - adds to __sys_declarations
    lua.globals().env = |args| {
        decl = CREATE_ENV_DECL(args)
        lua.globals().__sys_declarations.append(decl)
    }
```

**Example trace:**

```lua
-- user's sys.lua
local docker = require("./modules/docker")  -- Step 1: module() registers "docker"
docker.options.enable = true                 -- Step 2: user sets options

-- After config finishes executing:
-- Step 3: Runtime sees docker in __sys_modules
-- Step 4: Runtime calls docker.config(docker.options)
-- Step 5: docker.config() calls pkg(), service{}, etc.
-- Step 6: Those declarations are added to __sys_declarations
-- Step 7: All declarations merged into manifest
```

**Module dependency resolution:**

When modules depend on other modules, the runtime ensures correct evaluation order:

```
RESOLVE_MODULE_ORDER(modules):
    graph = {}

    // Build dependency graph
    FOR EACH module IN modules:
        deps = FIND_REQUIRED_MODULES(module.config)
        graph[module.name] = deps

    // Topological sort
    sorted = TOPOLOGICAL_SORT(graph)

    IF sorted HAS CYCLE:
        ERROR "Circular module dependency: {CYCLE_PATH}"

    RETURN sorted

EVALUATE_MODULES_IN_ORDER(modules, sorted_order):
    FOR EACH module_name IN sorted_order:
        module = modules[module_name]
        // Module's dependencies have already been evaluated
        CALL module.config(module.options)
```

### Module Structure

Modules are **plain Lua modules** that return a table with `options` and a `config` function. No special syntax required.

```lua
-- modules/docker.lua
local lib = require("sys.lib")

---@class DockerOptions
---@field enable boolean Enable Docker
---@field rootless boolean Run Docker in rootless mode
---@field storageDriver string Docker storage driver

-- Plain Lua module - just a table
local M = {
    -- Declare options with defaults
    options = {
        enable = false,
        rootless = true,
        storageDriver = "overlay2",
    },
}

-- Config function receives resolved options
---@param opts DockerOptions
function M.config(opts)
    if not opts.enable then return end

    pkg("docker")
    pkg("docker-compose")

    if sys.os == "linux" then
        service "docker" {
            enable = true,
            rootless = opts.rootless,
        }
    end

    file {
        path = "/etc/docker/daemon.json",
        content = lib.toJSON {
            ["storage-driver"] = opts.storageDriver,
        },
    }
end

return M
```

### Module Composition

Modules can depend on and configure other modules:

```lua
-- modules/dev-environment.lua
local docker = require("./docker")
local postgres = require("./postgres")

---@class DevEnvironmentOptions
---@field enable boolean Enable development environment
---@field withDatabase boolean Include database in dev environment

-- Plain Lua module
local M = {
    options = {
        enable = false,
        withDatabase = true,
    },
}

---@param opts DevEnvironmentOptions
function M.config(opts)
    if not opts.enable then return end

    -- Enable docker (this module depends on it)
    docker.options.enable = true

    if opts.withDatabase then
        postgres.options.enable = true
    end

    -- Dev tools
    pkg("git")
    pkg("ripgrep")
    pkg("fd")
    pkg("jq")

    env {
        EDITOR = lib.mkDefault("nvim"),
    }
end

return M
```

---

## The `config` Property Pattern

The `config` property is a **universal pattern** used consistently across all sys.lua primitives. It provides a scoped context for declaring packages, files, environment variables, and services.

### Consistency Across Primitives

Every major primitive that can contain nested declarations uses the same `config` pattern:

| Primitive    | `config` Scope               |
| ------------ | ---------------------------- |
| Modules      | Module-scoped declarations   |
| `user {}`    | User-scoped declarations     |
| `project {}` | Project-scoped declarations  |
| `pkg {}`     | Package runtime dependencies |
| `service {}` | Service-scoped declarations  |

### How `config` Works

The `config` property is always a function that receives resolved options and declares nested resources:

```lua
-- Pattern: config = function(opts) ... end

-- In modules (plain Lua)
local M = {
    options = { enable = false },
}
function M.config(opts)
    if not opts.enable then return end
    pkg("docker")
    service "docker" { enable = true }
end
return M

-- In users
user {
    name = "ian",
    config = function()
        pkg("neovim")
        file { path = "~/.gitconfig", content = "..." }
        env { EDITOR = "nvim" }
    end,
}

-- In projects
project {
    name = "my-app",
    config = function()
        pkg("nodejs", "20.0.0")
        env { NODE_ENV = "development" }
    end,
}

-- In packages (runtime dependencies)
pkg "neovim" {
    version = "0.10.0",
    src = { ... },
    config = function(opts)
        pkg("ripgrep")   -- Runtime dependency
        pkg("fd")        -- Runtime dependency
        env { EDITOR = lib.mkDefault("nvim") }
    end,
}
```

### Why This Pattern?

1. **Consistency**: One pattern to learn, used everywhere
2. **Scoping**: Declarations inside `config` are scoped to the parent (user, project, module)
3. **Lazy evaluation**: `config` functions are evaluated during manifest generation, not at parse time
4. **Composability**: Any `config` can call any other primitive (pkg, file, env, service)
5. **Options**: `config` receives resolved options, enabling conditional logic

### Evaluation Order

1. Parse `sys.lua` and collect all declarations
2. Resolve module options (set by user)
3. Evaluate all `config` functions in dependency order
4. Merge results into final manifest

---

## Configuration API

### User Config (`sys.lua`)

```lua
local lib = require("sys.lib")
local inputs = { ... }  -- see Inputs section

-- Declare packages from inputs
pkg(inputs.pkgs.ripgrep)
pkg(inputs.pkgs.fd, "9.0.0")

-- Environment modifications (session variables - set in shell env)
env {
    EDITOR = "nvim",
    PATH = lib.mkOrder(0, { "$HOME/.local/bin" }),
    MANPATH = lib.mkOrder(1000, { "/usr/local/man" }),
}

-- Persistent environment variables (written to system/user profile)
env.persistent {
    JAVA_HOME = "/usr/lib/jvm/java-17",
    GOPATH = "$HOME/go",
}
```

### Environment Variables

sys.lua supports two types of environment variables:

| Type           | API                 | Persistence               | Use Case                                        |
| -------------- | ------------------- | ------------------------- | ----------------------------------------------- |
| **Session**    | `env {}`            | Shell session only        | Editor, path modifications, shell customization |
| **Persistent** | `env.persistent {}` | Written to system profile | SDK paths, system-wide settings                 |

**Session variables** (default):

- Set via sourced shell scripts (`env.sh`, `env.fish`, `env.ps1`)
- Applied when user sources the sys.lua environment
- Lost when shell session ends (unless re-sourced)
- Best for: `PATH`, `EDITOR`, shell customization

**Persistent variables**:

- Written directly to system/user profile files
- Survive reboots and are available to all processes
- Platform-specific storage:
  - **Linux**: `/etc/environment` (system), `~/.pam_environment` (user)
  - **macOS**: `launchctl setenv` + `/etc/launchd.conf` (system), `~/Library/LaunchAgents/` (user)
  - **Windows**: Registry `HKLM\...\Environment` (system), `HKCU\Environment` (user)
- Best for: `JAVA_HOME`, `GOPATH`, SDK paths needed by GUI apps

**Singular vs Mergeable variables:**

Environment variables are either singular (one value) or mergeable (combined):

| Variable Type | Behavior                                    | Examples                             |
| ------------- | ------------------------------------------- | ------------------------------------ |
| Singular      | Lower priority number wins, conflicts error | `EDITOR`, `JAVA_HOME`, `GOPATH`      |
| Mergeable     | All values combined, sorted by priority     | `PATH`, `MANPATH`, `LD_LIBRARY_PATH` |

```lua
local lib = require("sys.lib")

-- Singular: only one value (uses priority resolution)
env {
    EDITOR = lib.mkDefault("vim"),
    JAVA_HOME = "/usr/lib/jvm/java-17",
}

-- Mergeable: multiple values combined
env {
    PATH = lib.mkBefore({ "$HOME/.local/bin" }),  -- prepend
    PATH = lib.mkAfter({ "/usr/local/games" }),   -- append
}

-- Persistent variables (same priority rules apply)
env.persistent {
    JAVA_HOME = lib.mkDefault("/usr/lib/jvm/java-11"),
}

-- User override
user {
    name = "ian",
    config = function()
        env { EDITOR = lib.mkForce("nvim") }
        env.persistent { JAVA_HOME = lib.mkOverride(900, "/usr/lib/jvm/java-17") }
    end,
}
```

### Environment Variable Classification

sys.lua determines whether an environment variable is singular or mergeable using a **builtin classification** plus user overrides:

**Builtin Mergeable Variables:**

```rust
// sys-core/src/env.rs
pub static MERGEABLE_ENV_VARS: &[&str] = &[
    // Path-like variables (colon-separated on Unix, semicolon on Windows)
    "PATH",
    "MANPATH",
    "INFOPATH",
    "LD_LIBRARY_PATH",
    "DYLD_LIBRARY_PATH",
    "PKG_CONFIG_PATH",
    "ACLOCAL_PATH",
    "PYTHONPATH",
    "PERL5LIB",
    "CLASSPATH",
    "GOPATH",
    "NODE_PATH",
    "GEM_PATH",
    "RUBYLIB",
    "LUA_PATH",
    "LUA_CPATH",

    // Other mergeable variables
    "CFLAGS",
    "CXXFLAGS",
    "LDFLAGS",
    "NIX_CFLAGS_COMPILE",
    "NIX_LDFLAGS",
];

pub fn is_mergeable(var_name: &str) -> bool {
    MERGEABLE_ENV_VARS.contains(&var_name)
}
```

**All other variables are treated as singular by default.**

**User-defined classification:**

Users can override the classification for custom environment variables:

```lua
-- sys.lua
local lib = require("sys.lib")

-- Mark custom variable as mergeable
lib.env.defineMergeable("MY_CUSTOM_PATH")

-- Now MY_CUSTOM_PATH behaves like PATH
env {
    MY_CUSTOM_PATH = lib.mkBefore({ "/opt/custom" }),
    MY_CUSTOM_PATH = lib.mkAfter({ "/usr/local/custom" }),
}
-- Result: /opt/custom:/usr/local/custom

-- Mark variable as singular (override builtin)
lib.env.defineSingular("GOPATH")  -- Force GOPATH to be singular

env {
    GOPATH = lib.mkDefault("$HOME/go"),
}
```

**Merging behavior:**

```
MERGE_ENV_VAR(var_name, declarations):
    IF is_mergeable(var_name):
        // Combine all values, sorted by priority
        sorted_decls = SORT_BY_PRIORITY(declarations)

        values = []
        FOR EACH decl IN sorted_decls:
            values.extend(decl.value)  // decl.value is array

        // Join with platform-specific separator
        separator = IF platform.isWindows THEN ";" ELSE ":"
        RETURN JOIN(values, separator)
    ELSE:
        // Singular - use lowest priority
        winner = MIN_BY_PRIORITY(declarations)

        // Check for conflicts at same priority
        conflicts = FILTER(declarations, d => d.priority == winner.priority)
        IF conflicts.length > 1:
            ERROR "Conflicting values for '{var_name}' at priority {winner.priority}:"
                  FOR EACH c IN conflicts:
                      "  {c.value} (declared at {c.location})"

        RETURN winner.value
```

### Parsed Structure

Lua config is evaluated into a `Manifest` - a declarative specification that is order-independent:

```rust
/// The manifest is the intermediate representation between
/// Lua config and system state. Order of declarations in
/// Lua does not affect the manifest.
pub struct Manifest {
    pub packages: Vec<PackageSpec>,       // System-level packages
    pub files: Vec<FileSpec>,             // System-level files
    pub env: EnvConfig,                   // System-level session env vars
    pub env_persistent: EnvConfig,        // System-level persistent env vars
    pub users: Vec<UserConfig>,           // Per-user configuration
}

pub struct UserConfig {
    pub name: String,
    pub packages: Vec<PackageSpec>,       // User-scoped packages
    pub files: Vec<FileSpec>,             // User-scoped files (~ expanded)
    pub env: EnvConfig,                   // User-scoped session env vars
    pub env_persistent: EnvConfig,        // User-scoped persistent env vars
}

pub struct PackageSpec {
    pub name: String,
    pub version: String,
    pub source: Source,
    pub bin: Vec<String>,
    pub depends_on: Vec<String>,
    pub priority: i32,  // For conflict resolution
}

pub enum Source {
    Url { url: String, sha256: String },
    Git { url: String, rev: String, sha256: String },
    GitHub { owner: String, repo: String, tag: String, asset: String, sha256: String },
    GitLab { owner: String, repo: String, tag: String, asset: String, sha256: String },
}

pub struct FileSpec {
    pub path: PathBuf,
    pub content: FileContent,
    pub mode: Option<u32>,
    pub depends_on: Vec<String>,
    pub priority: i32,  // For conflict resolution
}

pub enum FileContent {
    Inline(String),
    Symlink(PathBuf),
    Copy(PathBuf),
}

/// All config values are wrapped with priority for conflict resolution
pub struct Prioritized<T> {
    pub value: T,
    pub priority: i32,
}

/// Environment variables support both singular and mergeable values
pub enum EnvValue {
    /// Single value (e.g., EDITOR) - lowest priority wins
    Singular(Prioritized<String>),
    /// Mergeable value (e.g., PATH) - all values combined, sorted by priority
    Mergeable(Vec<Prioritized<Vec<String>>>),
}
```

---

## Priority and Conflict Resolution

sys.lua uses a **priority-based system** (inspired by NixOS modules) to resolve conflicts when the same item is declared multiple times. This applies to **all** declarations: packages, files, and environment variables.

**Priority semantics:** Lower numeric value = higher precedence (wins in conflicts). Think of it as "execution order" - priority 50 executes before priority 1000.

### Priority Scale

| Numeric Priority | Function                          | Use Case                                  |
| ---------------- | --------------------------------- | ----------------------------------------- |
| 50               | `lib.mkForce(value)`              | Force a value, override everything        |
| 500              | `lib.mkBefore(value)`             | Prepend (for mergeable values)            |
| 1000             | `lib.mkDefault(value)`            | Default value (implicit if not specified) |
| 1500             | `lib.mkAfter(value)`              | Append (for mergeable values)             |
| Custom           | `lib.mkOverride(priority, value)` | Explicit priority control                 |

```lua
local lib = require("sys.lib")

-- Explicit priority control
lib.mkForce(value)              -- Priority 50 (highest precedence)
lib.mkBefore(value)             -- Priority 500
lib.mkDefault(value)            -- Priority 1000 (implicit default)
lib.mkAfter(value)              -- Priority 1500 (lowest precedence)
lib.mkOverride(priority, value) -- Explicit priority
lib.mkOrder(priority, value)    -- Alias for mkOverride (clearer for ordered lists)
```

### Conflict Resolution Rules

**Rule 1: Lower priority number wins for singular values**

When two declarations conflict, the one with the numerically lower priority takes precedence:

```lua
-- Base config
pkg("neovim", { version = lib.mkDefault("0.9.0") })  -- priority 1000

-- User override (priority 50 < 1000, so this wins)
pkg("neovim", { version = lib.mkForce("0.10.0") })   -- priority 50 wins
```

**Rule 2: Same priority + different values = error**

```lua
pkg("neovim", { version = "0.9.0" })  -- implicit priority 1000
pkg("neovim", { version = "0.10.0" }) -- implicit priority 1000
-- Error: conflicting versions for package 'neovim' at priority 1000
```

**Rule 3: Mergeable values are combined and sorted by priority**

For values that can have multiple entries (PATH, file content sections), all declarations are merged and sorted by priority (lower priority first):

```lua
env {
    PATH = lib.mkBefore({ "$HOME/.cargo/bin" }),   -- priority 500
}
env {
    PATH = lib.mkAfter({ "/usr/local/games" }),    -- priority 1500
}
env {
    PATH = lib.mkOrder(100, { "$HOME/.local/bin" }), -- priority 100
}
-- Result (sorted by priority):
// priority 100, then 500, then 1500
// $HOME/.local/bin:$HOME/.cargo/bin:$PATH:/usr/local/games
```

### Applying to All Declaration Types

**Packages:**

```lua
-- Default package version (can be overridden)
pkg("nodejs", { version = lib.mkDefault("18.0.0") })

-- Force specific version (overrides defaults)
pkg("nodejs", { version = lib.mkForce("20.0.0") })

-- Conflicting declarations without priority = error
pkg("ripgrep", "14.0.0")
pkg("ripgrep", "15.0.0")  -- Error: conflicting versions
```

**Files:**

```lua
-- Default file content (can be overridden by higher-priority declaration)
file {
    path = "~/.gitconfig",
    content = lib.mkDefault([[
[core]
    editor = vim
]]),
}

-- Override in user config (lower priority number = wins)
file {
    path = "~/.gitconfig",
    content = lib.mkForce([[
[core]
    editor = nvim
[user]
    name = Ian
]]),
}

-- Conflicting file declarations without priority = error
file { path = "~/.bashrc", content = "config A" }
file { path = "~/.bashrc", content = "config B" }  -- Error: conflicting content
```

**Note:** Files are fully managed - the winning declaration's content replaces the entire file. There is no content merging for files.

**Environment variables:**

```lua
-- Singular (only one value)
env { EDITOR = lib.mkDefault("vim") }
env { EDITOR = lib.mkForce("nvim") }  -- Wins

-- Mergeable (combined)
env { PATH = lib.mkBefore({ "$HOME/bin" }) }
env { PATH = lib.mkAfter({ "/opt/bin" }) }
```

### Integration with DAG

The priority system integrates with the execution DAG:

1. **Evaluation phase**: All declarations are collected with their priorities
2. **Merge phase**: Conflicts are resolved using priority rules
3. **DAG construction**: Resolved values are used to build the execution graph
4. **Execution phase**: DAG determines execution order based on dependencies

```
Config Evaluation
       │
       ▼
┌─────────────────────────────────────────┐
│  Collect all declarations with priority │
│  pkg("foo", { version = mkDefault(...)})│
│  pkg("foo", { version = mkForce(...)})  │
└─────────────────────────────────────────┘
       │
       ▼
┌─────────────────────────────────────────┐
│  Merge & Resolve Conflicts              │
│  - Same key? Compare priorities         │
│  - Lower priority number wins (singular)│
│  - Sort by priority (mergeable)         │
│  - Same priority + conflict? ERROR      │
└─────────────────────────────────────────┘
       │
       ▼
┌─────────────────────────────────────────┐
│  Build Execution DAG                    │
│  - Nodes: resolved packages, files, env │
│  - Edges: depends_on relationships      │
└─────────────────────────────────────────┘
       │
       ▼
    Execute
```

### DAG Construction

After manifest generation and conflict resolution, sys.lua builds a Directed Acyclic Graph (DAG) that represents execution order.

**Node Types:**

```rust
pub enum DagNode {
    Package(PackageNode),
    File(FileNode),
    EnvVar(EnvVarNode),
    Service(ServiceNode),
}

pub struct PackageNode {
    pub id: String,              // "ripgrep@15.1.0"
    pub spec: PackageSpec,
    pub action: PackageAction,   // Install, Update, Remove
}

pub struct FileNode {
    pub id: String,              // "/home/ian/.gitconfig"
    pub spec: FileSpec,
    pub action: FileAction,      // Create, Update, Remove
}

pub struct EnvVarNode {
    pub id: String,              // "env:PATH" or "env.persistent:JAVA_HOME"
    pub name: String,
    pub value: EnvValue,
    pub persistent: bool,
}

pub struct ServiceNode {
    pub id: String,              // "service:postgresql"
    pub spec: ServiceSpec,
    pub action: ServiceAction,   // Start, Stop, Restart, Reload
}
```

**Edge Types:**

```rust
pub enum DagEdge {
    // Hard dependency: target must complete successfully before source starts
    DependsOn {
        source: NodeId,
        target: NodeId,
    },

    // Soft dependency: target should complete before source, but source proceeds even if target fails
    After {
        source: NodeId,
        target: NodeId,
    },

    // Ordering hint: prefer running source before target if no other constraints
    Before {
        source: NodeId,
        target: NodeId,
    },
}
```

**DAG Construction Algorithm:**

```
BUILD_DAG(manifest):
    nodes = []
    edges = []

    // Phase 1: Create nodes for all manifest items
    FOR EACH pkg IN manifest.packages:
        nodes.append(PackageNode {
            id: pkg.name + "@" + pkg.version,
            spec: pkg,
            action: DETERMINE_PACKAGE_ACTION(pkg),
        })

    FOR EACH file IN manifest.files:
        nodes.append(FileNode {
            id: file.path,
            spec: file,
            action: DETERMINE_FILE_ACTION(file),
        })

    FOR EACH (name, value) IN manifest.env:
        nodes.append(EnvVarNode {
            id: "env:" + name,
            name: name,
            value: value,
            persistent: false,
        })

    FOR EACH (name, value) IN manifest.env_persistent:
        nodes.append(EnvVarNode {
            id: "env.persistent:" + name,
            name: name,
            value: value,
            persistent: true,
        })

    FOR EACH service IN manifest.services:
        nodes.append(ServiceNode {
            id: "service:" + service.name,
            spec: service,
            action: DETERMINE_SERVICE_ACTION(service),
        })

    // Phase 2: Add explicit dependency edges
    FOR EACH node IN nodes:
        FOR EACH dep_id IN node.spec.depends_on:
            target = FIND_NODE_BY_ID(nodes, dep_id)
            IF target IS NULL:
                ERROR "Dependency '{dep_id}' not found for node '{node.id}'"
            edges.append(DependsOn { source: node.id, target: target.id })

        FOR EACH after_id IN node.spec.after:
            target = FIND_NODE_BY_ID(nodes, after_id)
            IF target IS NULL:
                WARNING "After-dependency '{after_id}' not found for node '{node.id}'"
            ELSE:
                edges.append(After { source: node.id, target: target.id })

    // Phase 3: Add implicit dependency edges
    // Files that reference packages must wait for packages
    FOR EACH file_node IN nodes WHERE node.type == File:
        referenced_pkgs = EXTRACT_PACKAGE_REFS(file_node.spec.content)
        FOR EACH pkg_name IN referenced_pkgs:
            pkg_node = FIND_PACKAGE_NODE(nodes, pkg_name)
            IF pkg_node IS NOT NULL:
                edges.append(DependsOn { source: file_node.id, target: pkg_node.id })

    // Services depend on their package being installed
    FOR EACH service_node IN nodes WHERE node.type == Service:
        pkg_node = FIND_PACKAGE_NODE(nodes, service_node.spec.package)
        IF pkg_node IS NOT NULL:
            edges.append(DependsOn { source: service_node.id, target: pkg_node.id })

    // Package runtime dependencies (from config function)
    FOR EACH pkg_node IN nodes WHERE node.type == Package:
        FOR EACH runtime_dep IN pkg_node.spec.runtime_deps:
            dep_node = FIND_PACKAGE_NODE(nodes, runtime_dep)
            IF dep_node IS NOT NULL:
                edges.append(DependsOn { source: pkg_node.id, target: dep_node.id })

    // Phase 4: Detect cycles
    IF HAS_CYCLE(nodes, edges):
        cycle_path = FIND_CYCLE(nodes, edges)
        ERROR "Circular dependency detected: {cycle_path}"

    // Phase 5: Topological sort
    sorted = TOPOLOGICAL_SORT(nodes, edges)

    RETURN DAG { nodes: nodes, edges: edges, sorted: sorted }

TOPOLOGICAL_SORT(nodes, edges):
    // Kahn's algorithm
    in_degree = {}
    FOR EACH node IN nodes:
        in_degree[node.id] = 0

    FOR EACH edge IN edges WHERE edge.type == DependsOn:
        in_degree[edge.source] += 1

    queue = []
    FOR EACH node IN nodes WHERE in_degree[node.id] == 0:
        queue.append(node)

    sorted = []
    WHILE queue IS NOT EMPTY:
        node = queue.pop_front()
        sorted.append(node)

        // Find all nodes that depend on this node
        FOR EACH edge IN edges WHERE edge.target == node.id AND edge.type == DependsOn:
            in_degree[edge.source] -= 1
            IF in_degree[edge.source] == 0:
                queue.append(FIND_NODE(nodes, edge.source))

    IF sorted.length != nodes.length:
        ERROR "Cycle detected during topological sort"

    RETURN sorted
```

**Parallel Execution:**

Nodes with no dependencies between them can execute in parallel:

```
EXECUTE_DAG(dag):
    completed = {}
    failed = {}
    in_progress = {}

    WHILE NOT ALL_NODES_COMPLETE(dag.nodes, completed, failed):
        // Find all nodes ready to execute
        ready = []
        FOR EACH node IN dag.sorted:
            IF node IN completed OR node IN failed OR node IN in_progress:
                CONTINUE

            // Check if all dependencies are completed
            deps_satisfied = TRUE
            FOR EACH edge IN dag.edges WHERE edge.source == node.id AND edge.type == DependsOn:
                IF edge.target NOT IN completed:
                    deps_satisfied = FALSE
                    BREAK
                IF edge.target IN failed:
                    // Hard dependency failed - mark this node as failed too
                    failed.add(node.id)
                    deps_satisfied = FALSE
                    BREAK

            IF deps_satisfied:
                ready.append(node)

        // Execute ready nodes in parallel (up to max parallelism)
        max_parallel = GET_CONFIG("max_parallel_jobs", default=4)
        batch = ready[0:max_parallel]

        FOR EACH node IN batch:
            in_progress.add(node.id)
            SPAWN_ASYNC(EXECUTE_NODE(node, completed, failed, in_progress))

        // Wait for at least one to complete
        WAIT_FOR_ANY_COMPLETION(in_progress)

    IF failed IS NOT EMPTY:
        ROLLBACK(completed)
        ERROR "DAG execution failed: {failed}"
```

**Example DAG visualization:**

```
User config:
  pkg("neovim")
  pkg("ripgrep")
  file { path = "~/.config/nvim/init.lua", depends_on = { "neovim" } }
  service "postgresql" { enable = true }

Generated DAG:
  ┌──────────┐     ┌──────────┐
  │ ripgrep  │     │  neovim  │
  └──────────┘     └────┬─────┘
                        │ DependsOn
                        ▼
                  ┌───────────────┐
                  │ nvim/init.lua │
                  └───────────────┘

  ┌──────────────┐
  │ postgresql   │
  │  (package)   │
  └──────┬───────┘
         │ DependsOn
         ▼
  ┌──────────────┐
  │ postgresql   │
  │  (service)   │
  └──────────────┘

Execution order:
  [Wave 1] ripgrep, neovim, postgresql (package) - parallel
  [Wave 2] nvim/init.lua, postgresql (service) - parallel (after wave 1)
```

---

## Apply Flow

The apply command is fully declarative - it makes the current state match the config exactly by both installing new packages and removing packages not in the config.

**Key Design Principle:** Lua configuration is evaluated into a manifest first, conflicts are resolved using priorities, then a DAG-based system applies changes. This ensures:

- Order of declarations in Lua does not affect the final result
- Conflicts are detected and resolved deterministically
- The system determines optimal execution order, not the user
- Dependencies are resolved before dependents
- Parallel execution where possible

```
sys apply sys.lua
    │
    ├─► PHASE 1: EVALUATION
    │   ├─► Parse sys.lua with Lua runtime
    │   ├─► Execute all pkg(), file{}, env{}, user{} declarations
    │   ├─► Collect all declarations with their priorities
    │   └─► Resolve fetch helpers (fetchUrl, fetchGit, etc.)
    │
    ├─► PHASE 2: MERGE & CONFLICT RESOLUTION
    │   ├─► Group declarations by key (package name, file path, env var)
    │   ├─► For each group:
    │   │   ├─► Singular values: lowest priority wins
    │   │   ├─► Mergeable values: combine and sort by priority
    │   │   └─► Same priority + different values: ERROR
    │   └─► Produce resolved Manifest
    │
    ├─► PHASE 3: PLANNING
    │   ├─► Load registry from effective path
    │   ├─► Get current installed state from store
    │   ├─► Compute diff: desired (manifest) vs current
    │   │   ├─► to_install = desired - current
    │   │   └─► to_remove = current - desired
    │   ├─► Build execution DAG from manifest
    │   │   ├─► Nodes: packages, files, env vars
    │   │   └─► Edges: depends_on relationships
    │   └─► Topologically sort DAG for execution order
    │
    ├─► PHASE 4: EXECUTION
    │   ├─► Display plan (always shown)
    │   ├─► If no changes: exit early
    │   ├─► Create pre-apply snapshot (with config content)
    │   ├─► Execute DAG in topological order:
    │   │   ├─► Parallel execution for independent nodes
    │   │   ├─► Download/verify/extract packages
    │   │   ├─► Create/update files
    │   │   └─► Update environment
    │   ├─► On failure: rollback completed nodes, abort
    │   ├─► Create post-apply snapshot (with config content)
    │   └─► Generate env scripts (env.sh, env.fish)
    │
    └─► Print summary and shell setup instructions
```

### Manifest Structure

The manifest is the intermediate representation between Lua config and system state:

```rust
pub struct Manifest {
    pub packages: Vec<PackageSpec>,
    pub files: Vec<FileSpec>,
    pub env: EnvConfig,
    pub users: Vec<UserConfig>,
}

pub struct PackageSpec {
    pub name: String,
    pub version: String,
    pub source: Source,           // Resolved from fetch helpers
    pub bin: Vec<String>,
    pub depends_on: Vec<String>,  // Package dependencies
}

pub enum Source {
    Url { url: String, sha256: String },
    Git { url: String, rev: String, sha256: String },
    GitHub { owner: String, repo: String, tag: String, asset: String, sha256: String },
}
```

### Execution DAG

The DAG ensures correct ordering regardless of config declaration order:

```
Example: User declares in any order:
  pkg("neovim")
  pkg("ripgrep")
  file { path = "~/.config/nvim/init.lua", ... }  -- depends on neovim

DAG constructed:
  ┌──────────┐     ┌──────────┐
  │ ripgrep  │     │  neovim  │
  └──────────┘     └────┬─────┘
                        │ depends_on
                        ▼
                  ┌───────────────┐
                  │ nvim/init.lua │
                  └───────────────┘

Execution order (determined by system, not user):
  1. ripgrep, neovim (parallel - no dependencies between them)
  2. nvim/init.lua (after neovim completes)
```

### Atomic Apply (All-or-Nothing)

**sys.lua uses atomic semantics for the apply operation.** Either all changes succeed or the system remains in its previous state - there is no partial application.

#### Why Atomic?

Partial application creates broken states that are difficult to debug and recover from:

- A file might reference a package that failed to install
- Environment variables might point to missing paths
- Services might fail because their dependencies aren't available
- Users would need to manually figure out what succeeded vs failed

#### How It Works

```
Apply begins
    │
    ├─► Create pre-apply snapshot
    │
    ├─► Execute DAG nodes...
    │       │
    │       ├─► Node 1: Success ✓ (tracked)
    │       ├─► Node 2: Success ✓ (tracked)
    │       ├─► Node 3: FAILURE ✗
    │       │
    │       └─► Rollback triggered
    │               │
    │               ├─► Undo Node 2
    │               ├─► Undo Node 1
    │               └─► Restore pre-apply snapshot
    │
    └─► Exit with error (system unchanged)
```

#### Rollback Behavior

When any node in the DAG fails:

1. **Stop execution** - No further nodes are attempted
2. **Undo completed nodes** - In reverse order of completion
3. **Restore snapshot** - Revert to the pre-apply snapshot
4. **Report failure** - Show which node failed and why

```bash
$ sudo sys apply sys.lua
Evaluating sys.lua...
Building execution plan...

Executing:
  [1/4] ✓ ripgrep@15.1.0
  [2/4] ✓ fd@9.0.0
  [3/4] ✗ custom-tool@1.0.0
        Error: Build failed: missing dependency 'libfoo'

Rolling back...
  - Removing fd@9.0.0 from profile
  - Removing ripgrep@15.1.0 from profile
  - Restoring pre-apply state

Apply failed. System unchanged.
Run 'sys plan' to review the execution plan.
```

#### What Gets Rolled Back

| Component       | Rollback Action                                        |
| --------------- | ------------------------------------------------------ |
| **Packages**    | Remove from `pkg/` symlinks (objects remain in `obj/`) |
| **Files**       | Restore from pre-apply snapshot backup                 |
| **Symlinks**    | Restore original target or remove                      |
| **Environment** | Regenerate env scripts from previous state             |
| **Services**    | Stop newly started services, restart stopped services  |

#### Edge Cases

**Already-installed packages**: If a package already exists in the store from a previous apply, it's not re-downloaded. Rollback simply removes the symlink - the cached object remains for future use.

**External changes during apply**: If the system is modified externally during apply (rare), rollback restores to the snapshot which reflects state at apply-start, not the external changes.

**Idempotent re-apply**: After a failed apply and rollback, running `sys apply` again will attempt the same changes. Fix the underlying issue (e.g., the missing `libfoo` dependency) before re-running.

### Plan Command

Preview changes without applying (evaluates config to manifest, builds DAG, but doesn't execute):

```bash
$ sys plan sys.lua
Evaluating sys.lua...
Building execution plan...

Install:
  + fd@9.0.0
  + bat@0.24.0
Remove:
  - ripgrep@14.1.1
Unchanged:
  = jq@1.7.1

Execution order:
  1. [parallel] fd@9.0.0, bat@0.24.0
  2. [remove] ripgrep@14.1.1
```

---

## Snapshot Design

Snapshots capture the **complete system state** before and after changes, enabling full rollback. This includes packages, files, environment variables, and services.

### Structure

```
~/.local/share/sys/snapshots/
├── metadata.json
└── files/
    └── <snapshot_id>/      # Backed up file contents
```

```json
{
  "snapshots": [
    {
      "id": "1765208363188",
      "created_at": "1733667300",
      "description": "After successful apply",
      "config_path": "/path/to/sys.lua",
      "config_content": "pkg(inputs.pkgs.ripgrep)\n...",

      "packages": [
        { "name": "ripgrep", "version": "15.1.0", "hash": "abc123..." }
      ],

      "files": [
        { "path": "/home/ian/.gitconfig", "hash": "def456...", "mode": "0644" },
        { "path": "/home/ian/.config/nvim", "type": "symlink", "target": "..." }
      ],

      "env": {
        "session": { "EDITOR": "nvim", "PATH": ["..."] },
        "persistent": { "JAVA_HOME": "/usr/lib/jvm/java-17" }
      },

      "services": [
        { "name": "nginx", "enabled": true },
        { "name": "postgresql", "enabled": true }
      ],

      "users": [
        {
          "name": "ian",
          "packages": [...],
          "files": [...],
          "env": {...}
        }
      ]
    }
  ],
  "current": "1765208363188"
}
```

### What Gets Snapshotted

| Component          | Captured                   | Restored                                   |
| ------------------ | -------------------------- | ------------------------------------------ |
| **Packages**       | Name, version, hash        | Re-linked from store (no re-download)      |
| **Files**          | Path, content hash, mode   | Content restored from backup               |
| **Symlinks**       | Path, target               | Symlink recreated                          |
| **Session env**    | Variable names and values  | Env scripts regenerated                    |
| **Persistent env** | Variable names and values  | Written back to OS config                  |
| **Services**       | Name, enabled state        | Service started/stopped + enabled/disabled |
| **User config**    | All of the above, per user | Restored per user                          |

### File Backup

For files managed by `file {}`, the actual content is backed up:

```
~/.local/share/sys/snapshots/files/
└── 1765208363188/
    ├── home-ian-.gitconfig        # Flattened path
    ├── home-ian-.config-nvim      # Directory archived as tarball
    └── etc-nginx-nginx.conf
```

This ensures rollback can restore exact file contents even if the original source is unavailable.

### Rollback

Rollback restores **all state** from a snapshot:

```bash
$ sys rollback                    # Rollback to previous snapshot
$ sys rollback <snapshot_id>      # Rollback to specific snapshot
$ sys rollback --dry-run          # Preview what would change
```

The rollback process:

1. Computes diff between current state and target snapshot
2. **Packages**: Remove packages not in target, restore packages from `obj/`
3. **Files**: Restore file contents from backup, recreate symlinks
4. **Environment**: Regenerate env scripts, restore persistent variables
5. **Services**: Stop/disable services not in target, start/enable services in target
6. Creates a new snapshot documenting the rollback

### Rollback Algorithm

```
ROLLBACK_TO_SNAPSHOT(target_snapshot_id, dry_run=false):
    // Phase 1: Load snapshot data
    target = LOAD_SNAPSHOT(target_snapshot_id)
    IF target IS NULL:
        ERROR "Snapshot '{target_snapshot_id}' not found"

    current = GET_CURRENT_STATE()

    // Phase 2: Compute diff
    diff = COMPUTE_ROLLBACK_DIFF(current, target)

    // Phase 3: Display changes
    PRINT_ROLLBACK_PLAN(diff)

    IF dry_run:
        RETURN  // Exit without making changes

    IF NOT CONFIRM("Proceed with rollback?"):
        RETURN

    // Phase 4: Create pre-rollback snapshot
    pre_rollback_snapshot = CREATE_SNAPSHOT("Before rollback to " + target_snapshot_id)

    // Phase 5: Execute rollback (atomic - all or nothing)
    TRY:
        EXECUTE_ROLLBACK(diff, target)

        // Phase 6: Create post-rollback snapshot
        post_rollback_snapshot = CREATE_SNAPSHOT("After rollback to " + target_snapshot_id)

        PRINT "Rollback successful"

    CATCH error:
        // Rollback failed - restore pre-rollback state
        ERROR "Rollback failed: {error}"
        PRINT "Restoring pre-rollback state..."
        EXECUTE_ROLLBACK(
            COMPUTE_ROLLBACK_DIFF(GET_CURRENT_STATE(), pre_rollback_snapshot),
            pre_rollback_snapshot
        )
        ERROR "Rollback aborted. System restored to pre-rollback state."

COMPUTE_ROLLBACK_DIFF(current, target):
    diff = {
        packages_to_add: [],
        packages_to_remove: [],
        files_to_restore: [],
        files_to_remove: [],
        env_changes: {},
        env_persistent_changes: {},
        services_to_start: [],
        services_to_stop: [],
    }

    // Packages
    current_pkgs = SET(current.packages.map(p => p.name + "@" + p.version))
    target_pkgs = SET(target.packages.map(p => p.name + "@" + p.version))

    diff.packages_to_add = target_pkgs - current_pkgs
    diff.packages_to_remove = current_pkgs - target_pkgs

    // Files
    current_files = MAP(current.files, key=path, value=hash)
    target_files = MAP(target.files, key=path, value=hash)

    FOR EACH (path, target_hash) IN target_files:
        IF path NOT IN current_files:
            diff.files_to_restore.append({ path, target_hash })
        ELSE IF current_files[path] != target_hash:
            diff.files_to_restore.append({ path, target_hash })

    FOR EACH path IN current_files.keys():
        IF path NOT IN target_files:
            diff.files_to_remove.append(path)

    // Environment variables (session)
    FOR EACH (name, target_value) IN target.env.session:
        IF name NOT IN current.env.session OR current.env.session[name] != target_value:
            diff.env_changes[name] = target_value

    FOR EACH name IN current.env.session.keys():
        IF name NOT IN target.env.session:
            diff.env_changes[name] = NULL  // Remove

    // Environment variables (persistent)
    FOR EACH (name, target_value) IN target.env.persistent:
        IF name NOT IN current.env.persistent OR current.env.persistent[name] != target_value:
            diff.env_persistent_changes[name] = target_value

    FOR EACH name IN current.env.persistent.keys():
        IF name NOT IN target.env.persistent:
            diff.env_persistent_changes[name] = NULL  // Remove

    // Services
    current_services = MAP(current.services, key=name, value=enabled)
    target_services = MAP(target.services, key=name, value=enabled)

    FOR EACH (name, target_enabled) IN target_services:
        current_enabled = current_services.get(name, false)
        IF target_enabled AND NOT current_enabled:
            diff.services_to_start.append(name)
        ELSE IF NOT target_enabled AND current_enabled:
            diff.services_to_stop.append(name)

    RETURN diff

EXECUTE_ROLLBACK(diff, target_snapshot):
    completed_actions = []

    TRY:
        // Step 1: Stop services that shouldn't be running
        FOR EACH service_name IN diff.services_to_stop:
            STOP_SERVICE(service_name)
            DISABLE_SERVICE(service_name)
            completed_actions.append({ type: "service_stop", name: service_name })

        // Step 2: Remove packages not in target
        FOR EACH pkg_id IN diff.packages_to_remove:
            REMOVE_PACKAGE_LINK(pkg_id)  // Remove symlink from pkg/
            completed_actions.append({ type: "package_remove", id: pkg_id })

        // Step 3: Restore packages from target
        FOR EACH pkg_id IN diff.packages_to_add:
            pkg_spec = FIND_IN_SNAPSHOT(target_snapshot.packages, pkg_id)
            hash = pkg_spec.hash

            IF NOT EXISTS_IN_STORE(hash):
                ERROR "Package '{pkg_id}' (hash {hash}) not found in store. "
                      "It may have been garbage collected. "
                      "Run 'sys apply' to reinstall, or rollback to a more recent snapshot."

            CREATE_PACKAGE_LINK(pkg_id, hash)
            completed_actions.append({ type: "package_add", id: pkg_id })

        // Step 4: Remove files not in target
        FOR EACH file_path IN diff.files_to_remove:
            IF IS_MANAGED_FILE(file_path):
                REMOVE_FILE(file_path)
                completed_actions.append({ type: "file_remove", path: file_path })

        // Step 5: Restore files from target
        FOR EACH { path, hash } IN diff.files_to_restore:
            backup_path = SNAPSHOT_FILE_BACKUP_PATH(target_snapshot.id, path)

            IF NOT EXISTS(backup_path):
                ERROR "File backup for '{path}' not found in snapshot"

            // Handle external modifications
            IF EXISTS(path) AND NOT IS_MANAGED_FILE(path):
                ERROR "File '{path}' exists but is not managed by sys.lua. "
                      "Manual intervention required."

            RESTORE_FILE(backup_path, path)
            SET_FILE_MODE(path, target_snapshot.files[path].mode)
            completed_actions.append({ type: "file_restore", path: path })

        // Step 6: Update environment variables (session)
        FOR EACH (name, value) IN diff.env_changes:
            IF value IS NULL:
                REMOVE_FROM_ENV_SCRIPT(name)
            ELSE:
                UPDATE_ENV_SCRIPT(name, value)
        completed_actions.append({ type: "env_update", count: diff.env_changes.length })

        // Step 7: Update environment variables (persistent)
        FOR EACH (name, value) IN diff.env_persistent_changes:
            IF value IS NULL:
                REMOVE_PERSISTENT_ENV_VAR(name)
            ELSE:
                SET_PERSISTENT_ENV_VAR(name, value)
        completed_actions.append({ type: "env_persistent_update", count: diff.env_persistent_changes.length })

        // Step 8: Start services that should be running
        FOR EACH service_name IN diff.services_to_start:
            ENABLE_SERVICE(service_name)
            START_SERVICE(service_name)
            completed_actions.append({ type: "service_start", name: service_name })

        RETURN SUCCESS

    CATCH error:
        // Undo all completed actions in reverse order
        FOR EACH action IN REVERSE(completed_actions):
            TRY:
                UNDO_ACTION(action)
            CATCH undo_error:
                // Log but continue - best effort rollback
                LOG_ERROR("Failed to undo {action}: {undo_error}")

        THROW error
```

**Conflict resolution during rollback:**

| Scenario                    | Behavior                                                       |
| --------------------------- | -------------------------------------------------------------- |
| File modified externally    | Error - manual intervention required                           |
| Package GC'd since snapshot | Error - suggest re-running `sys apply` or using newer snapshot |
| Service fails to start      | Rollback continues, logs error, marks service as failed        |
| Store object missing        | Error - cannot complete rollback                               |
| Symlink target changed      | Overwrites with snapshot target                                |

---

## Garbage Collection

Objects in `obj/<hash>/` are never deleted during normal operations. Uninstalling a package only removes its symlink from `pkg/`.

The `gc` command cleans up orphaned objects:

```bash
$ sys gc
Garbage collecting...
Removed 3 orphaned objects
Freed 12.5 MB
```

**How it works:**

1. Scans all symlinks in `pkg/` to find referenced hashes
2. Scans `obj/` for all hashes
3. Makes unreferenced objects mutable (removes immutability flags)
4. Removes unreferenced objects
5. Reports freed space

This design allows rollbacks to work even after removing packages from config, as long as `gc` hasn't been run.

### Garbage Collection with Locking

To prevent race conditions, GC uses a global lock:

```
GC_COLLECT():
    // Acquire exclusive lock
    lock = ACQUIRE_STORE_LOCK(exclusive=true, timeout=30s)
    IF lock IS NULL:
        ERROR "Could not acquire store lock. Another sys.lua operation may be running."

    TRY:
        // Phase 1: Find all roots (things that shouldn't be GC'd)
        roots = SET()

        // Add all package symlinks
        FOR EACH symlink IN GLOB("store/pkg/**/*"):
            IF IS_SYMLINK(symlink):
                target = READ_LINK(symlink)
                hash = EXTRACT_HASH_FROM_PATH(target)
                roots.add(hash)

        // Add all snapshots
        FOR EACH snapshot IN LOAD_ALL_SNAPSHOTS():
            FOR EACH pkg IN snapshot.packages:
                roots.add(pkg.hash)
            FOR EACH file IN snapshot.files:
                IF file.is_symlink:
                    target_hash = EXTRACT_HASH_FROM_PATH(file.target)
                    IF target_hash IS NOT NULL:
                        roots.add(target_hash)

        // Phase 2: Find unreferenced objects
        unreferenced = []
        FOR EACH obj_path IN GLOB("store/obj/*"):
            hash = BASENAME(obj_path)
            IF hash NOT IN roots:
                unreferenced.append({ hash, path: obj_path })

        // Phase 3: Remove unreferenced objects
        total_size = 0
        FOR EACH { hash, path } IN unreferenced:
            size = GET_DIRECTORY_SIZE(path)
            total_size += size

            // Make mutable first
            MAKE_MUTABLE(path)
            REMOVE_DIRECTORY(path)

        PRINT "Removed {unreferenced.length} objects, freed {total_size} bytes"

    FINALLY:
        RELEASE_STORE_LOCK(lock)
```

**Concurrent operation protection:**

| Operation    | Lock Type     | Blocks GC? | Blocked by GC? |
| ------------ | ------------- | ---------- | -------------- |
| `sys apply`  | Exclusive     | Yes        | Yes            |
| `sys gc`     | Exclusive     | N/A        | Yes (by apply) |
| `sys plan`   | Shared (read) | No         | No             |
| `sys status` | Shared (read) | No         | No             |
| `sys shell`  | Shared (read) | No         | No             |

**GC and snapshots:**

Snapshots protect their referenced objects from GC:

```
$ sys apply sys.lua           # Installs ripgrep@15.1.0 (creates snapshot 1)
$ # Edit sys.lua to remove ripgrep
$ sys apply sys.lua           # Removes ripgrep symlink (creates snapshot 2)
$ sys gc                      # Does NOT delete ripgrep object (snapshot 1 references it)
$ sys rollback <snapshot 1>   # Can still rollback (object exists)
$ sys gc --delete-old-snapshots --keep 5  # Delete old snapshots
$ sys gc                      # NOW ripgrep object can be deleted
```

---

## Environment Scripts

### Per-User Profiles

sys.lua generates **separate environment scripts for each user** defined in the configuration. This ensures user-scoped packages and environment variables are isolated:

```
~/.local/share/sys/
├── env.sh              # System-level env (all users)
├── env.fish
├── env.ps1
└── users/
    ├── ian/
    │   ├── env.sh      # ian's packages + env vars
    │   ├── env.fish
    │   └── env.ps1
    └── admin/
        ├── env.sh      # admin's packages + env vars
        ├── env.fish
        └── env.ps1
```

**How it works:**

1. System-level `pkg()` and `env{}` go into the root env scripts
2. User-scoped declarations (inside `user { config = ... }`) go into per-user scripts
3. Users source both: system env first, then their user env
4. User env can override/extend system env

```bash
# ian's ~/.bashrc
[ -f ~/.local/share/sys/env.sh ] && source ~/.local/share/sys/env.sh
[ -f ~/.local/share/sys/users/ian/env.sh ] && source ~/.local/share/sys/users/ian/env.sh
```

### Session Variables

Session variables are written to shell-specific scripts:

| Platform    | Script Location               | Shell Integration                    |
| ----------- | ----------------------------- | ------------------------------------ |
| Linux/macOS | `~/.local/share/sys/env.sh`   | Sourced in `.bashrc`/`.zshrc`        |
| Linux/macOS | `~/.local/share/sys/env.fish` | Sourced in `config.fish`             |
| Windows     | `~/.local/share/sys/env.ps1`  | Sourced in PowerShell `$PROFILE`     |
| Windows     | `~/.local/share/sys/env.cmd`  | Via `AutoRun` registry key (cmd.exe) |

```bash
# Unix: env.sh (sourced by user's shell)
export PATH="/path/to/store/pkg/ripgrep/15.1.0/aarch64-darwin:$PATH"
export EDITOR="nvim"
```

```powershell
# Windows: env.ps1 (sourced by PowerShell profile)
$env:PATH = "C:\syslua\store\pkg\ripgrep\15.1.0\x86_64-windows;$env:PATH"
$env:EDITOR = "nvim"
```

**Shell integration (user adds to their config):**

```bash
# Unix: ~/.bashrc or ~/.zshrc
[ -f ~/.local/share/sys/env.sh ] && source ~/.local/share/sys/env.sh
```

```powershell
# Windows: $PROFILE (e.g., ~\Documents\PowerShell\Microsoft.PowerShell_profile.ps1)
if (Test-Path "$env:USERPROFILE\.local\share\sys\env.ps1") {
    . "$env:USERPROFILE\.local\share\sys\env.ps1"
}
```

### Persistent Variables

Persistent variables are written directly to OS-level configuration, available to all processes (including GUI apps and services):

| Platform | System Location                   | User Location                          |
| -------- | --------------------------------- | -------------------------------------- |
| Linux    | `/etc/environment`                | `~/.pam_environment`                   |
| macOS    | `/etc/launchd.conf` + `launchctl` | `~/Library/LaunchAgents/sys.env.plist` |
| Windows  | Registry `HKLM\...\Environment`   | Registry `HKCU\Environment`            |

```bash
# Example: After applying config with env.persistent { JAVA_HOME = "..." }

# Linux /etc/environment (appended)
JAVA_HOME="/usr/lib/jvm/java-17"

# macOS: launchctl setenv is called, and plist is created for persistence

# Windows: Registry value is set under HKCU\Environment
# A WM_SETTINGCHANGE broadcast notifies running applications
```

**Why Registry for Windows persistent vars (not PowerShell profile):**

- Registry is the canonical location for Windows environment variables
- Available to all processes: GUI apps, services, scheduled tasks, all shells
- PowerShell profiles only affect PowerShell sessions
- `env.persistent {}` semantics require system-wide visibility

**Rollback behavior:** Persistent variables are tracked in snapshots and restored during rollback.

---

## File Management

sys.lua provides declarative file management for creating configuration files, symlinks, and file copies.

**Important: Files are fully managed by sys.lua.** When you declare a file, sys.lua takes complete ownership:

- The file's entire content is replaced with what you specify
- Existing content is NOT preserved or merged
- Removing a file declaration removes the file from disk
- Changes made outside sys.lua will be overwritten on next `sys apply`

For files like `.bashrc` where you want sys.lua to manage the whole file, include all desired content in your config. If you have existing content you want to keep, migrate it into your sys.lua configuration.

### File Types

| Type      | Description                          | Example Use Case              |
| --------- | ------------------------------------ | ----------------------------- |
| `content` | Create file with inline content      | Shell configs, dotfiles       |
| `symlink` | Create symbolic link to target       | Link configs to dotfiles repo |
| `copy`    | Copy file from source to destination | Templates, backup copies      |

### Lua API

```lua
-- Create a file with inline content
file {
    path = "~/.gitconfig",
    content = [[
[user]
    name = Ian
    email = ian@example.com
]],
    mode = "0644",  -- Optional: file permissions (octal string)
}

-- Create a symlink
file {
    path = "~/.config/nvim",
    symlink = "~/.dotfiles/nvim",
}

-- Copy a file
file {
    path = "~/.bashrc.backup",
    copy = "~/.bashrc",
}
```

### User-Scoped Configuration

The `user {}` block defines user-scoped packages, files, and environment variables. The `config` property takes a function that describes all user-specific configuration:

```lua
user {
    name = "ian",
    config = function()
        -- User-scoped packages (installed to user's PATH)
        pkg("neovim")
        pkg("ripgrep", "15.1.0")

        -- User-scoped environment variables
        env {
            EDITOR = "nvim",
            VISUAL = "nvim",
        }

        -- User-scoped files (~ expands to /Users/ian or /home/ian)
        file {
            path = "~/.gitconfig",
            content = [[
[user]
    name = Ian
    email = ian@example.com
]],
        }

        file {
            path = "~/.config/nvim",
            symlink = "~/.dotfiles/nvim",
        }
    end,
}
```

**Scoping behavior:**

- `pkg()` calls inside `config` are scoped to that user's environment
- `file {}` paths starting with `~` expand to the user's home directory
- `env {}` variables are set in the user's shell environment
- Multiple users can have different packages/configs on the same system

```lua
-- System-level packages (available to all users)
pkg("git")
pkg("curl")

-- Per-user configuration
user {
    name = "ian",
    config = function()
        pkg("neovim")  -- Only in ian's PATH
        file { path = "~/.bashrc", content = "..." }
    end,
}

user {
    name = "admin",
    config = function()
        pkg("htop")  -- Only in admin's PATH
        file { path = "~/.bashrc", content = "..." }
    end,
}
```

### File Permissions

Mode strings support multiple formats:

- `"0644"` - Standard octal with leading zero
- `"644"` - Octal without leading zero
- `"0o644"` - Rust-style octal prefix

On Windows, file permissions are ignored (Windows has a different permission model).

### File Tracking

sys.lua tracks which files it manages in `~/.local/share/sys/managed_files.json`. This enables:

- Removing files when they're removed from config
- Detecting changes to managed files
- Clean rollback behavior

---

## Service Management

sys.lua provides cross-platform declarative service management using native init systems.

### Platform Backends

| Platform | Init System                       | Service Location                                     |
| -------- | --------------------------------- | ---------------------------------------------------- |
| Linux    | systemd                           | `/etc/systemd/system/`                               |
| macOS    | launchd                           | `/Library/LaunchDaemons/`, `~/Library/LaunchAgents/` |
| Windows  | Windows Services + Task Scheduler | Registry / Task Scheduler                            |

### Declaring Services

```lua
-- Simple service declaration
-- enable = true means: start now AND start on boot
service "nginx" {
    enable = true,
}

-- Disable a service (stop now AND don't start on boot)
service "nginx" {
    enable = false,
}

-- Service with configuration
service "postgresql" {
    enable = true,
    config = function(opts)
        -- Service-specific options are set via module options
        return {
            port = opts.port or 5432,
            dataDir = opts.dataDir or "/var/lib/postgresql/data",
        }
    end,
}

-- Custom service definition
service "myapp" {
    enable = true,

    -- Platform-specific definitions
    systemd = {
        Unit = {
            Description = "My Application",
            After = "network.target",
        },
        Service = {
            Type = "simple",
            ExecStart = "/usr/local/bin/myapp",
            Restart = "always",
            User = "myapp",
        },
        Install = {
            WantedBy = "multi-user.target",
        },
    },

    launchd = {
        Label = "com.example.myapp",
        ProgramArguments = { "/usr/local/bin/myapp" },
        RunAtLoad = true,
        KeepAlive = true,
    },

    windows = {
        name = "MyApp",
        displayName = "My Application",
        execPath = "C:\\Program Files\\MyApp\\myapp.exe",
        startType = "auto",  -- auto, manual, disabled
    },
}
```

**Service state behavior:**

| `enable`  | Effect                                     |
| --------- | ------------------------------------------ |
| `true`    | Start service immediately + enable on boot |
| `false`   | Stop service immediately + disable on boot |
| (omitted) | Service not managed by sys.lua             |

### Service Configuration System

Services can use the `config` property for platform-specific configuration:

```lua
-- modules/postgresql.lua
local lib = require("sys.lib")

return module "postgresql" {
    options = {
        enable = lib.mkOption { type = "bool", default = false },
        port = lib.mkOption { type = "int", default = 5432 },
        dataDir = lib.mkOption {
            type = "path",
            default = "/var/lib/postgresql/data"
        },
        authentication = lib.mkOption {
            type = "enum { 'md5', 'scram-sha-256', 'trust' }",
            default = "scram-sha-256",
        },
    },

    config = function(opts)
        if not opts.enable then return end

        -- Install package
        pkg("postgresql")

        -- Generate config file
        file {
            path = "/etc/postgresql/postgresql.conf",
            content = string.format([[
                port = %d
                data_directory = '%s'
                authentication = %s
            ]], opts.port, opts.dataDir, opts.authentication),
        }

        -- Declare service
        service "postgresql" {
            enable = true,

            -- Service config is platform-specific
            -- Use string.format() or concatenation to build config strings
            systemd = {
                Unit = {
                    Description = "PostgreSQL Database Server",
                    After = "network.target",
                },
                Service = {
                    Type = "forking",
                    User = "postgres",
                    ExecStart = string.format("/usr/bin/pg_ctl start -D %s", opts.dataDir),
                    ExecStop = string.format("/usr/bin/pg_ctl stop -D %s", opts.dataDir),
                    ExecReload = string.format("/usr/bin/pg_ctl reload -D %s", opts.dataDir),
                    Restart = "on-failure",
                },
                Install = {
                    WantedBy = "multi-user.target",
                },
            },

            launchd = {
                Label = "org.postgresql.server",
                ProgramArguments = {
                    "/usr/local/bin/postgres",
                    "-D", opts.dataDir,
                },
                RunAtLoad = true,
                KeepAlive = true,
            },

            windows = {
                name = "PostgreSQL",
                displayName = "PostgreSQL Database Server",
                execPath = "C:\\Program Files\\PostgreSQL\\bin\\postgres.exe",
                args = { "-D", opts.dataDir },
                startType = "auto",
            },
        }
    end,
}
```

**How service `config` works:**

1. Module declares service with platform-specific definitions
2. Use `string.format()` or concatenation to build config strings with option values
3. sys.lua selects appropriate platform definition at apply time
4. Service manager (systemd/launchd/Windows) is configured accordingly

**Service dependencies:**

```lua
service "myapp" {
    enable = true,

    systemd = {
        Unit = {
            Description = "My Application",
            After = { "network.target", "postgresql.service" },
            Requires = { "postgresql.service" },
        },
        Service = {
            ExecStart = "/usr/local/bin/myapp",
            Restart = "always",
        },
    },

    launchd = {
        Label = "com.example.myapp",
        ProgramArguments = { "/usr/local/bin/myapp" },
        RunAtLoad = true,
        KeepAlive = true,
    },
}
```

### User Services

Services can be scoped to users (runs as user, not root):

```lua
user {
    name = "ian",
    config = function()
        service "syncthing" {
            enable = true,
            user = true,  -- User-level service (launchd LaunchAgents, systemd --user)
        }
    end,
}
```

### Service Dependencies

```lua
service "myapp" {
    enable = true,
    after = { "postgresql", "redis" },  -- Start after these services
    requires = { "postgresql" },         -- Fail if postgresql isn't running
}
```

---

## Build System (Derivations)

While sys.lua prefers prebuilt binaries for speed, it supports building from source when necessary.

### Prebuilt vs Source

```lua
-- Prebuilt binary (preferred, fast)
pkg "ripgrep" {
    version = "15.1.0",
    src = lib.fetchFromGitHub {
        owner = "BurntSushi",
        repo = "ripgrep",
        tag = "15.1.0",
        asset = "ripgrep-{version}-{platform}.tar.gz",
        sha256 = { ... },
    },
    bin = { "rg" },
}

-- Build from source (when no prebuilt available)
pkg "custom-tool" {
    version = "1.0.0",
    src = lib.fetchGit {
        url = "https://github.com/user/custom-tool",
        rev = "v1.0.0",
        sha256 = "...",
    },

    build = function(src, opts)
        -- Build phases (inspired by Nix stdenv)
        return {
            buildInputs = { "rust", "pkg-config", "openssl" },

            configurePhase = [[
                export OPENSSL_DIR=${openssl}
            ]],

            buildPhase = [[
                cargo build --release
            ]],

            installPhase = [[
                mkdir -p $out/bin
                cp target/release/custom-tool $out/bin/
            ]],
        }
    end,

    bin = { "custom-tool" },
}
```

### Build Inputs and Dependencies

```lua
pkg "myapp" {
    build = function(src, opts)
        return {
            -- Build-time dependencies (only needed during build)
            buildInputs = { "cmake", "ninja", "pkg-config" },

            -- Runtime dependencies (propagated to user's environment)
            propagatedBuildInputs = { "openssl", "zlib" },
        }
    end,
}
```

### Cross-Compilation (Future)

Cross-compilation (building for a different target platform) is **not supported in the initial release**. sys.lua focuses on native builds first.

**Rationale:**

- Cross-compilation adds significant complexity (toolchains, sysroots, platform-specific flags)
- Most users need native builds; cross-compilation is a niche use case
- Prebuilt binaries (the preferred path) already cover multiple platforms
- Can be added later without breaking changes

**Current recommendation:** If you need binaries for multiple platforms:

1. Use prebuilt binaries from releases (preferred)
2. Build natively on each target platform (CI/CD)
3. Use Docker/VMs for foreign platform builds

### Package `config` for Runtime Dependencies

Packages use a `config` function to declare runtime behavior and dependencies:

```lua
pkg "neovim" {
    version = "0.10.0",
    src = lib.fetchFromGitHub { ... },
    bin = { "nvim" },

    -- Config function handles runtime setup
    config = function(opts)
        -- Runtime dependencies
        pkg("ripgrep")   -- for telescope
        pkg("fd")        -- for telescope
        pkg("tree-sitter")

        -- Environment setup
        env {
            EDITOR = lib.mkDefault("nvim"),
        }

        -- Optional features based on options
        if opts.withPython then
            pkg("python3")
            pkg("pynvim")
        end

        if opts.withNodejs then
            pkg("nodejs")
            pkg("neovim-node-client")
        end
    end,

    options = {
        withPython = lib.mkOption { type = "bool", default = false },
        withNodejs = lib.mkOption { type = "bool", default = false },
    },
}
```

### Build Sandbox

Builds execute in a **fully sandboxed environment** to ensure reproducibility:

**Sandbox Properties:**

- **Isolated filesystem**: Build sees only explicitly declared inputs
- **No network access**: All dependencies must be fetched ahead of time via `buildInputs`
- **Clean environment**: No inherited environment variables
- **Platform-native shell**: bash (Linux), zsh (macOS), pwsh (Windows)

**Build Location:**

```
# Cross-platform temp directory
Linux/macOS: /tmp/sys-build-<hash>/
Windows:     %TEMP%\sys-build-<hash>\
```

**Sandbox Implementation:**
| Platform | Mechanism |
|----------|-----------|
| Linux | User namespaces + bind mounts (or bubblewrap) |
| macOS | `sandbox-exec` with custom profile |
| Windows | Job objects + restricted tokens |

**Example build environment:**

```
/tmp/sys-build-abc123/
├── src/                    # Unpacked source (read-only)
├── build/                  # Build working directory
├── out/                    # Output directory ($out)
└── deps/                   # Symlinks to buildInputs
    ├── rust -> /syslua/store/obj/...
    ├── openssl -> /syslua/store/obj/...
    └── pkg-config -> /syslua/store/obj/...
```

### Build Sandbox Implementation Details

**Linux Sandbox (User Namespaces):**

```rust
// sys-core/src/build/sandbox_linux.rs
pub fn create_sandbox(build_dir: &Path, inputs: &[StoreObject]) -> Result<Sandbox> {
    let config = SandboxConfig {
        root: build_dir.to_path_buf(),
        binds: vec![
            // Bind mount inputs read-only
            BindMount {
                source: "/syslua/store",
                target: "/syslua/store",
                readonly: true,
            },
            // Bind mount build directory read-write
            BindMount {
                source: build_dir,
                target: "/build",
                readonly: false,
            },
        ],
        env: HashMap::from([
            ("PATH", "/usr/bin:/bin"),
            ("HOME", "/homeless-shelter"),  // Non-existent to catch $HOME references
            ("out", "/build/out"),
        ]),
        uid: 1000,  // Non-root UID
        gid: 1000,
        network: false,  // Disable network
    };

    // Use unshare() to create new namespaces
    unsafe {
        if unshare(CLONE_NEWUSER | CLONE_NEWNET | CLONE_NEWNS | CLONE_NEWPID) != 0 {
            return Err(Error::SandboxCreationFailed);
        }
    }

    // Set up user namespace mappings
    write_file("/proc/self/uid_map", &format!("1000 {} 1", getuid()))?;
    write_file("/proc/self/setgroups", "deny")?;
    write_file("/proc/self/gid_map", &format!("1000 {} 1", getgid()))?;

    // Bind mounts
    for bind in &config.binds {
        mount(
            Some(bind.source.as_path()),
            bind.target.as_path(),
            None::<&str>,
            MsFlags::MS_BIND | if bind.readonly { MsFlags::MS_RDONLY } else { MsFlags::empty() },
            None::<&str>,
        )?;
    }

    Ok(Sandbox { config })
}
```

**macOS Sandbox (sandbox-exec):**

```rust
// sys-core/src/build/sandbox_macos.rs
pub fn create_sandbox(build_dir: &Path, inputs: &[StoreObject]) -> Result<Sandbox> {
    let profile = format!(r#"
        (version 1)
        (deny default)
        (allow process-exec
            (literal "/bin/sh")
            (literal "/bin/bash")
            (literal "/usr/bin/env")
        )
        (allow file-read*
            (subpath "/syslua/store")
            (subpath "/usr")
            (subpath "/bin")
            (subpath "/System")
            (literal "/etc/resolv.conf")
        )
        (allow file-write*
            (subpath "{}")
        )
        (deny network*)
    "#, build_dir.display());

    // Write profile to temp file
    let profile_path = format!("{}/sandbox.sb", build_dir.display());
    std::fs::write(&profile_path, profile)?;

    Ok(Sandbox {
        profile_path,
        build_dir: build_dir.to_path_buf(),
    })
}

pub fn execute_in_sandbox(sandbox: &Sandbox, script: &str) -> Result<Output> {
    Command::new("sandbox-exec")
        .arg("-f")
        .arg(&sandbox.profile_path)
        .arg("sh")
        .arg("-c")
        .arg(script)
        .current_dir(&sandbox.build_dir)
        .output()
        .map_err(Into::into)
}
```

**Windows Sandbox (Job Objects):**

```rust
// sys-core/src/build/sandbox_windows.rs
use windows::Win32::System::JobObjects::*;

pub fn create_sandbox(build_dir: &Path, inputs: &[StoreObject]) -> Result<Sandbox> {
    unsafe {
        // Create job object
        let job = CreateJobObjectW(None, None)?;

        // Configure job limits
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE |  // Kill processes when job closes
            JOB_OBJECT_LIMIT_ACTIVE_PROCESS |      // Limit number of processes
            JOB_OBJECT_LIMIT_PRIORITY_CLASS;       // Limit priority
        limits.BasicLimitInformation.ActiveProcessLimit = 100;

        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const _,
            std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )?;

        // Network restrictions (requires Windows Firewall API)
        // Implemented via temporary firewall rule that blocks the job's processes

        Ok(Sandbox {
            job_handle: job,
            build_dir: build_dir.to_path_buf(),
        })
    }
}
```

**Build Environment Variables:**

Only these environment variables are available in the sandbox:

```bash
# Common across platforms
PATH=/usr/bin:/bin              # Minimal PATH
out=/build/out                  # Output directory
src=/build/src                  # Source directory
HOME=/homeless-shelter          # Intentionally broken
TMPDIR=/build/tmp              # Temp directory

# Platform-specific
# Linux
NIX_STORE=/syslua/store
# macOS
SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX.sdk
# Windows
TEMP=C:\build\tmp
```

**Escape prevention:**

- `/proc`, `/sys`, `/dev` not mounted (Linux)
- No network namespace access
- No setuid/setgid binaries
- File writes restricted to build directory
- No access to user's home directory
- Build runs as non-root user (Linux/macOS)

### Build Caching

Built packages are cached by **output hash** (hash of the actual built artifacts), not derivation hash. This avoids Nix's cache invalidation pitfalls where rebuilding with the same inputs produces a different hash.

```
store/
├── obj/<output-hash>/      # Built artifacts (immutable)
├── drv/<drv-hash>.drv      # Derivation files (build instructions)
└── drv-out/<drv-hash>      # Maps derivation hash → output hash
```

**Why output hash instead of derivation hash:**

- Same source code built on different machines produces same output hash
- Compiler version changes don't invalidate cache if output is identical
- Binary cache hits are based on what you need, not how it was built

**Cache lookup order:**

1. Local store - check if output hash exists in `obj/`
2. Binary cache - query official cache by output hash
3. Build from source - execute build, compute output hash, store result

**Cache key computation:**

```
output_hash = sha256(
    sorted_file_contents(out_directory) +
    sorted_file_metadata(out_directory)
)
```

---

## Binary Cache Infrastructure

sys.lua supports remote binary caches to avoid rebuilding packages that have already been built by others. This is similar to Nix's binary cache system.

### Cache Server Protocol

Binary caches use a simple HTTP-based protocol:

**Cache Server Endpoints:**

```
GET  /info                              # Server info and capabilities
GET  /obj/<hash>.narinfo                # Metadata for store object
GET  /nar/<hash>.nar.xz                 # Compressed store object
HEAD /obj/<hash>.narinfo                # Check if object exists
POST /obj/<hash>.narinfo                # Upload metadata (requires auth)
POST /nar/<hash>.nar.xz                 # Upload object (requires auth)
```

**Narinfo format** (metadata):

```
StorePath: /syslua/store/obj/abc123...
URL: nar/abc123def456.nar.xz
Compression: xz
FileHash: sha256:def456...
FileSize: 12345678
NarHash: sha256:abc123...
NarSize: 45678901
References: xyz789... uvw456...
Deriver: hello-2.10.drv
Sig: cache.example.com-1:abc123def456...
```

**NAR (Nix Archive) format:**

NAR is a deterministic archive format that preserves:

- File contents
- Directory structure
- File permissions
- Symlinks

sys.lua uses the NAR format for cache entries to ensure reproducibility.

### Cache Configuration

```lua
-- sys.lua
cache {
    -- Official sys.lua cache (public, read-only)
    official = "https://cache.syslua.org",

    -- Private cache (authenticated)
    company = {
        url = "https://cache.company.com",
        publicKey = "cache.company.com-1:abc123...",
        auth = secrets.cache_token,
    },

    -- Local cache (for CI/CD)
    local = {
        url = "file:///var/cache/syslua",
    },

    -- S3-compatible cache
    s3 = {
        url = "s3://my-bucket/cache",
        region = "us-east-1",
        auth = {
            accessKeyId = secrets.aws_access_key,
            secretAccessKey = secrets.aws_secret_key,
        },
    },
}
```

### Cache Lookup Algorithm

```
FETCH_PACKAGE(pkg_spec):
    output_hash = COMPUTE_OUTPUT_HASH(pkg_spec)

    // Check local store first
    IF EXISTS_IN_STORE(output_hash):
        RETURN STORE_PATH(output_hash)

    // Try each configured cache in order
    FOR EACH cache IN config.caches:
        narinfo = CACHE_GET(cache, output_hash)
        IF narinfo IS NOT NULL:
            // Verify signature
            IF NOT VERIFY_SIGNATURE(narinfo, cache.publicKey):
                WARNING "Invalid signature for {output_hash} from {cache.url}"
                CONTINUE

            // Download NAR
            nar_path = CACHE_DOWNLOAD(cache, narinfo.URL)

            // Verify hash
            actual_hash = HASH_FILE(nar_path)
            IF actual_hash != narinfo.NarHash:
                ERROR "Hash mismatch for {output_hash}: expected {narinfo.NarHash}, got {actual_hash}"

            // Extract to store
            EXTRACT_NAR(nar_path, STORE_OBJ_PATH(output_hash))
            MAKE_IMMUTABLE(STORE_OBJ_PATH(output_hash))

            RETURN STORE_PATH(output_hash)

    // No cache hit - build from source
    RETURN BUILD_FROM_SOURCE(pkg_spec)
```

### Cache Upload (CI/CD)

For maintainers building packages for the cache:

```bash
# Build package and upload to cache
$ sys build --upload-to cache.company.com pkg.lua

# Or upload existing store objects
$ sys cache push ripgrep@15.1.0 --cache company
```

**Upload process:**

1. Build package (or verify already in store)
2. Compute output hash
3. Create NAR archive
4. Sign NAR with private key
5. Generate narinfo
6. Upload NAR to cache
7. Upload narinfo to cache

### Cache Security

**Signature verification:**

All cache entries are signed to prevent tampering:

```rust
pub struct CacheSignature {
    pub key_name: String,        // "cache.example.com-1"
    pub signature: Vec<u8>,      // Ed25519 signature
}

pub fn verify_cache_entry(narinfo: &NarInfo, public_key: &PublicKey) -> Result<bool> {
    let message = format!("{}\n{}\n{}",
        narinfo.store_path,
        narinfo.nar_hash,
        narinfo.nar_size
    );

    Ok(public_key.verify(message.as_bytes(), &narinfo.signature))
}
```

**Trust model:**

- Official cache: Trusted by default, public key embedded in sys.lua
- Private caches: User must configure public key
- Unsigned caches: Rejected (no `--insecure` flag - security first)

---

## Network Configuration

sys.lua respects standard proxy environment variables and provides additional network configuration options.

### Proxy Configuration

**Environment variable support:**

sys.lua automatically respects these environment variables:

```bash
# HTTP/HTTPS proxies
export HTTP_PROXY=http://proxy.example.com:8080
export HTTPS_PROXY=http://proxy.example.com:8080
export NO_PROXY=localhost,127.0.0.1,.example.com

# Alternative (lowercase)
export http_proxy=http://proxy.example.com:8080
export https_proxy=http://proxy.example.com:8080
export no_proxy=localhost,127.0.0.1
```

**Declarative proxy configuration:**

```lua
-- sys.lua
local secrets = sops.load("./secrets.yaml")

network {
    proxy = {
        http = "http://proxy.example.com:8080",
        https = "http://proxy.example.com:8080",
        noProxy = { "localhost", "127.0.0.1", "*.internal.com" },
    },

    -- Authenticated proxy using string.format()
    proxy = {
        http = string.format("http://%s:%s@proxy.example.com:8080", 
                           secrets.proxy_user, secrets.proxy_pass),
        https = string.format("http://%s:%s@proxy.example.com:8080",
                            secrets.proxy_user, secrets.proxy_pass),
    },
}
```

### TLS Configuration

**Certificate validation:**

```lua
network {
    tls = {
        -- Use system CA bundle (default)
        caBundle = "system",

        -- Or custom CA bundle
        caBundle = "/etc/ssl/certs/ca-certificates.crt",

        -- Or additional CAs (merged with system)
        additionalCAs = {
            "/path/to/company-ca.crt",
        },

        -- Verify hostnames (default: true)
        verifyHostname = true,

        -- Minimum TLS version (default: TLS 1.2)
        minVersion = "1.2",
    },
}
```

**Certificate pinning (for high-security environments):**

```lua
network {
    tls = {
        pins = {
            ["cache.example.com"] = {
                -- Pin certificate fingerprint (SHA-256)
                certSha256 = "abc123def456...",
            },
        },
    },
}
```

### Timeouts and Retries

```lua
network {
    -- Global timeouts (seconds)
    timeout = {
        connect = 30,
        read = 300,
        write = 60,
    },

    -- Retry configuration
    retry = {
        attempts = 3,
        backoff = "exponential",  -- or "linear"
        maxDelay = 60,
    },

    -- Rate limiting (requests per second)
    rateLimit = {
        ["github.com"] = 10,
        ["gitlab.com"] = 5,
    },
}
```

### Offline Mode

For air-gapped environments:

```bash
# Work offline (fail if network needed)
$ sys apply --offline sys.lua

# Or configure in Lua
network {
    mode = "offline",  -- "online" (default), "offline", or "prefer-cache"
}
```

**Offline mode behavior:**

| Mode           | Behavior                                            |
| -------------- | --------------------------------------------------- |
| `online`       | Fetch from network as needed (default)              |
| `offline`      | Never use network, fail if not in cache/store       |
| `prefer-cache` | Use cache/store if available, only fetch if missing |

### DNS Configuration

```lua
network {
    dns = {
        -- Custom DNS servers
        servers = { "8.8.8.8", "8.8.4.4" },

        -- DNS-over-HTTPS
        doh = "https://dns.google/dns-query",

        -- Host overrides (like /etc/hosts)
        hosts = {
            ["cache.internal.com"] = "10.0.0.5",
        },
    },
}
```

---

## Project Environments

sys.lua supports project-local configurations similar to Python's virtualenv or Node's node_modules.

### Project Configuration

```lua
-- project/sys.lua
local lib = require("sys.lib")

project {
    name = "my-web-app",

    -- Project-specific packages (uses config pattern)
    config = function()
        pkg("nodejs", "20.0.0")
        pkg("pnpm")
        pkg("postgresql", "15")

        env {
            NODE_ENV = "development",
            DATABASE_URL = "postgresql://localhost:5432/myapp",
        }

        service "postgresql" {
            enable = true,
        }
    end,

    -- Shell hook (runs when entering project)
    shellHook = [[
        echo "Welcome to my-web-app development environment"
        pnpm install
    ]],
}
```

### Activating Project Environment

**`sys shell` does not require root.** It only uses packages already in the store.

```bash
# Enter project environment (like nix develop)
$ cd my-project
$ sys shell

# Or with direnv integration
$ echo 'use sys' >> .envrc
$ direnv allow
```

**Requirements for `sys shell`:**

- All packages referenced by the project must already exist in the store
- If a package is missing, `sys shell` will error and tell you to run `sudo sys apply` first
- This ensures non-root users cannot modify the global store

```bash
$ sys shell
Error: Package 'nodejs@20.0.0' not found in store.
Run 'sudo sys apply' to install missing packages first.
```

### Direnv Integration

sys.lua integrates with direnv for automatic environment activation:

```bash
# ~/.config/direnv/lib/sys.sh
use_sys() {
    if [[ -f sys.lua ]]; then
        eval "$(sys env --shell bash)"
    fi
}
```

```bash
# project/.envrc
use sys
```

### Project Isolation

**Project environment takes priority over system environment.** When a project declares a package that's also in the system config, the project's version is used within that shell session.

- Project packages are added to PATH **before** system packages
- Project env vars override system env vars for that shell
- If system has `nodejs@18` and project has `nodejs@20`, the project shell uses `nodejs@20`
- Exiting the shell restores the system environment

```
System environment:
  PATH = /syslua/store/pkg/nodejs/18.0.0/bin:/syslua/store/pkg/git/2.40.0/bin:...
  EDITOR = vim

Project environment (active):
  PATH = /syslua/store/pkg/nodejs/20.0.0/bin:/syslua/store/pkg/git/2.40.0/bin:...
  EDITOR = vim
  NODE_ENV = development
  # nodejs@20 shadows nodejs@18 due to PATH ordering
```

---

## Secrets Management (SOPS)

sys.lua has first-class SOPS integration for managing secrets declaratively using Age encryption.

**SOPS integration is built-in**: sys.lua includes pure Rust Age encryption support - no external dependencies required.

### Setup

```lua
-- sys.lua
sops {
    -- Age key (pure Rust, no dependencies)
    age = {
        keyFile = "~/.config/sops/age/keys.txt",
        -- Or recipients for encryption
        recipients = {
            "age1xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
        },
    },

    -- Or cloud KMS (for enterprise)
    awsKms = { arn = "arn:aws:kms:..." },
    gcpKms = { resourceId = "projects/..." },
    azureKv = { vaultUrl = "https://..." },
}
```

**Note:** Only Age encryption is supported. Age is modern, secure, and has no system dependencies. If you need GPG, use the `sops` CLI directly.

### Encrypted Secrets File

```yaml
# secrets.yaml (encrypted with sops)
database_password: ENC[AES256_GCM,data:...,tag:...]
api_key: ENC[AES256_GCM,data:...,tag:...]
ssh_private_key: ENC[AES256_GCM,data:...,tag:...]
```

### Using Secrets

```lua
local secrets = sops.load("./secrets.yaml")

-- In environment variables
env {
    DATABASE_PASSWORD = secrets.database_password,
    API_KEY = secrets.api_key,
}

-- In files (using string formatting)
file {
    path = "/etc/myapp/config.toml",
    content = string.format([[
        [database]
        password = "%s"

        [api]
        key = "%s"
    ]], secrets.database_password, secrets.api_key),
    mode = "0600",
}

-- SSH keys
file {
    path = "~/.ssh/id_ed25519",
    content = secrets.ssh_private_key,
    mode = "0600",
}
```

### String Formatting

sys.lua uses Lua's built-in string formatting capabilities for generating configuration files with dynamic values.

**Using `string.format()`:**

```lua
local lib = require("sys.lib")
local secrets = sops.load("./secrets.yaml")

-- Simple formatting
local greeting = string.format("Hello %s!", "World")
-- Result: "Hello World!"

-- Multiple values
local config = string.format([[
    server {
        host = %s
        port = %d
        ssl = %s
    }
]], "localhost", 8080, "true")

-- With secrets
local db_config = string.format([[
    database_url = "%s"
    api_key = "%s"
]], secrets.db_url, secrets.api_key)
```

**Format specifiers:**

| Specifier | Type    | Description               | Example                         |
| --------- | ------- | ------------------------- | ------------------------------- |
| `%s`      | string  | String substitution       | `string.format("%s", "text")`   |
| `%d`      | integer | Integer                   | `string.format("%d", 42)`       |
| `%f`      | float   | Floating point            | `string.format("%.2f", 3.14)`   |
| `%q`      | string  | Quoted string (escaped)   | `string.format("%q", "a\"b")`   |
| `%%`      | literal | Literal percent sign      | `string.format("100%%")`        |

**Alternative: String concatenation:**

For simple cases, Lua's concatenation is clearest:

```lua
local secrets = sops.load("./secrets.yaml")

-- Simple concatenation
local config = "database_url = " .. secrets.db_url .. "\n" ..
               "api_key = " .. secrets.api_key

-- Multi-line with concat
local nginx_config = [[
server {
    listen 80;
    server_name ]] .. domain .. [[;
    
    location / {
        proxy_pass ]] .. backend_url .. [[;
    }
}
]]
```

**Security notes:**

- Always validate and sanitize user input before formatting
- Use `%q` for shell-safe quoting when needed
- When formatting secrets, ensure file permissions are restrictive (e.g., `mode = "0600"`)
- Prefer concatenation or `string.format()` over building shell commands (use proper escaping instead)

### Secret Scoping

```lua
-- System secrets
local system_secrets = sops.load("./secrets/system.yaml")

-- User-specific secrets
user {
    name = "ian",
    config = function()
        local user_secrets = sops.load("./secrets/ian.yaml")

        file {
            path = "~/.config/gh/hosts.yml",
            content = string.format([[
                github.com:
                    oauth_token: %s
            ]], user_secrets.gh_token),
        }
    end,
}
```

### Secret Rotation

```bash
# Re-encrypt all secrets with new keys
$ sys secrets rotate --add-key age1newkey...

# Update specific secret
$ sys secrets set database_password
Enter new value: ********
```

---

## Platform Conditionals

sys.lua provides platform detection for conditional configuration.

### Platform Information

```lua
### Platform and System Information

System information is available via the global `sys` table:

```lua
-- Platform detection
sys.platform   -- "x86_64-linux", "aarch64-darwin", etc.
sys.os         -- "linux", "darwin", "windows"
sys.arch       -- "x86_64", "aarch64", "arm"

-- Boolean helpers for common checks
sys.is_linux   -- true/false
sys.is_darwin  -- true/false
sys.is_windows -- true/false

-- Host information
sys.hostname   -- "my-laptop"
sys.username   -- "ian"
```

### Conditional Configuration

Use native Lua conditionals for platform-specific configuration:

```lua
-- Platform-specific packages
if sys.is_darwin then
    pkg("mas")  -- Mac App Store CLI
end

if sys.is_linux then
    pkg("apt-file")
end

-- Platform-specific services
service "tailscale" {
    enable = true,
    systemd = { ... },  -- Linux-specific
    launchd = { ... },  -- macOS-specific
    windows = { ... },  -- Windows-specific
}

-- Host-specific config
if sys.hostname == "work-laptop" then
    require("./modules/work")
end

if sys.hostname == "home-server" then
    require("./modules/server")
end

-- OS-specific environment variables
env {
    BROWSER = sys.is_darwin and "open" or "xdg-open",
}

-- Platform-specific PATH entries
if sys.is_darwin then
    env {
        PATH = lib.mkBefore({ "/opt/homebrew/bin" }),
    }
end
```

---

## Activation Scripts

sys.lua supports hooks that run at various points during the apply process.

### System Activation

```lua
activation {
    -- Run before any changes
    pre = [[
        echo "Starting sys apply..."
    ]],

    -- Run after all changes complete
    post = [[
        echo "Apply complete!"
        # Reload shell configs
        if command -v fish &> /dev/null; then
            fish -c 'source ~/.config/fish/config.fish'
        fi
    ]],

    -- Run on first install only
    firstBoot = [[
        echo "Welcome to sys.lua!"
        # One-time setup
    ]],
}
```

### Package Hooks

```lua
pkg "neovim" {
    version = "0.10.0",
    src = { ... },

    -- Hooks for this package
    hooks = {
        postInstall = [[
            # Install plugins on first install
            nvim --headless "+Lazy! sync" +qa
        ]],

        postUpdate = [[
            # Update plugins when neovim updates
            nvim --headless "+Lazy! update" +qa
        ]],

        preRemove = [[
            echo "Removing neovim..."
        ]],
    },
}
```

### Service Hooks

```lua
service "postgresql" {
    enable = true,

    hooks = {
        preStart = [[
            # Initialize database if needed
            if [ ! -d /var/lib/postgresql/data ]; then
                initdb -D /var/lib/postgresql/data
            fi
        ]],

        postStart = [[
            # Wait for postgres to be ready
            until pg_isready; do sleep 1; done
            echo "PostgreSQL is ready"
        ]],
    },
}
```

### Conditional Hooks

Use native Lua conditionals for platform-specific hooks:

```lua
-- Simple approach: separate activation blocks
activation {
    post = [[echo "Apply complete"]],
}

if sys.is_linux then
    activation {
        post = [[systemctl daemon-reload]],
    }
end

if sys.is_darwin then
    activation {
        post = [[killall Finder]],
    }
end
```

---

## Multi-Level Store Architecture

**sys.lua operates at two levels: system (managed by admins) and user (managed by individuals).**

### Key Principle

System configuration provides the foundation; user configuration extends and customizes without breaking system guarantees.

### Store Layout

```
# System store (managed by admin/root)
/syslua/store/
├── obj/              # Shared read-only objects (world-readable)
├── pkg/              # System-level package symlinks
└── metadata/         # System state and snapshots

# Per-user stores (managed by each user, no sudo required)
~/.local/share/sys/
├── store/
│   ├── obj/          # User's packages (hardlinks to system when possible)
│   ├── pkg/          # User's package symlinks
│   └── metadata/     # User state and snapshots
├── env.sh            # User environment script
├── env.fish          # User environment script (fish shell)
└── snapshots/        # User snapshots
```

### Privilege Separation

**System administrator (requires sudo/admin):**

```bash
# IT manages system configuration
sudo sys apply /etc/sys/system.lua
```

System config controls:
- ✅ System packages (available to all users)
- ✅ System services (sshd, nginx, postgresql, etc.)
- ✅ System files (`/etc`, `/Library`, `/opt`, etc.)
- ✅ Persistent environment variables (system-wide, all processes)
- ✅ User provisioning (IT can pre-configure users)

**Regular users (no sudo required):**

```bash
# Users manage their own configuration
sys apply ~/.config/sys/sys.lua
```

User config controls:
- ✅ Personal packages (installed to `~/.local/share/sys/store`)
- ✅ Personal files (anywhere in home directory)
- ✅ Session environment variables (shell only)
- ✅ User-scoped services (runs as user, not root)
- ✅ Can override IT-set user files (with warning)
- ❌ Cannot touch system files (`/etc`, `/Library`, etc.)
- ❌ Cannot manage system services
- ❌ Cannot set persistent environment variables

### Example: System Configuration (IT Admin)

```lua
-- /etc/sys/system.lua (managed by IT, applied with sudo)

-- System-wide packages (available to all users)
pkg("git")
pkg("curl")
pkg("vim")
pkg("docker")

-- System services
service "sshd" {
    enable = true,
}

service "docker" {
    enable = true,
}

-- System files
file {
    path = "/etc/ssh/sshd_config",
    content = [[
        PasswordAuthentication no
        PubkeyAuthentication yes
        Port 22
    ]],
}

-- System environment
env {
    LANG = "en_US.UTF-8",
    TZ = "America/New_York",
}

-- IT-managed user configurations
user {
    name = "alice",
    config = function()
        -- Provision Alice with dev tools
        pkg("python3")
        pkg("nodejs")
        
        file {
            path = "~/.gitconfig",
            content = [[
                [user]
                    name = Alice
                    email = alice@company.com
            ]],
        }
    end,
}

user {
    name = "bob",
    config = function()
        pkg("ruby")
        pkg("postgresql-client")
    end,
}
```

### Example: User Configuration (Alice)

```lua
-- ~/.config/sys/sys.lua (managed by Alice, applied without sudo)

-- Alice's personal packages
pkg("neovim")
pkg("ripgrep")
pkg("fzf")
pkg("tmux")
pkg("bat")

-- Alice's personal files
file {
    path = "~/.config/nvim/init.lua",
    content = [[
        vim.opt.number = true
        vim.opt.relativenumber = true
    ]],
}

-- Alice can customize her gitconfig
-- (IT set a basic one, Alice wants to extend it)
file {
    path = "~/.gitconfig",
    content = [[
        [user]
            name = Alice Smith
            email = alice@company.com
        [core]
            editor = nvim
        [alias]
            st = status
            co = checkout
            br = branch
    ]],
}

-- Alice's personal environment
env {
    EDITOR = "nvim",
    VISUAL = "nvim",
    FZF_DEFAULT_COMMAND = "rg --files",
}

-- Alice's personal user service
service "syncthing" {
    enable = true,
    user = true,  -- Runs as Alice, not root
}
```

### Apply Flow

```bash
# System configuration (IT admin)
$ sudo sys apply /etc/sys/system.lua
Evaluating /etc/sys/system.lua...
Installing to /syslua/store...
  ✓ git@2.40.0
  ✓ docker@24.0.0
Configuring system services...
  ✓ sshd.service enabled
  ✓ docker.service enabled
Writing system files...
  ✓ /etc/ssh/sshd_config
System apply complete!

# User configuration (no sudo)
$ sys apply ~/.config/sys/sys.lua
Evaluating /home/alice/.config/sys/sys.lua...
Installing to /home/alice/.local/share/sys/store...
  ✓ neovim@0.10.0
  ✓ ripgrep@15.1.0 (hardlinked from system store)
  ✓ fzf@0.48.0
Warning: Overriding system-managed file: ~/.gitconfig
  System config: /etc/sys/system.lua (line 35)
  Continue? [y/N] y
Writing user files...
  ✓ ~/.gitconfig (overridden)
  ✓ ~/.config/nvim/init.lua
Configuring user services...
  ✓ syncthing.service enabled (user)
Generating environment scripts...
  ✓ ~/.local/share/sys/env.sh
  ✓ ~/.local/share/sys/env.fish

User apply complete!

Add to your ~/.bashrc:
  [ -f ~/.local/share/sys/env.sh ] && source ~/.local/share/sys/env.sh
```

### Conflict Resolution

When a user applies their config, sys.lua enforces boundaries:

| Resource Type | System Owns | User Owns | Conflict Resolution |
|---------------|-------------|-----------|---------------------|
| **Packages** | System packages | User packages | Merged (no conflict) |
| **System files** | `/etc`, `/Library`, `/opt` | N/A | User blocked (error) |
| **User files** | Can provision | Can override | User warned, allowed |
| **System services** | `sshd`, `docker`, etc. | N/A | User blocked (error) |
| **User services** | N/A | `syncthing`, etc. | User allowed |
| **Session env** | Can set defaults | Can override | User wins (warned) |
| **Persistent env** | System only | N/A | User blocked (error) |

### Security Model

```
┌─────────────────────────────────────────────────────────┐
│         System Administrator (sudo/admin)                │
├─────────────────────────────────────────────────────────┤
│  • System packages (/syslua/store)                       │
│  • System services (sshd, nginx, docker)                 │
│  • System files (/etc, /Library, /opt)                   │
│  • Persistent environment variables                      │
│  • User provisioning (IT-managed user configs)           │
│                                                           │
│  Command: sudo sys apply /etc/sys/system.lua             │
└───────────────────────┬─────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────────┐
│              Regular User (no sudo)                      │
├─────────────────────────────────────────────────────────┤
│  • Personal packages (~/.local/share/sys/store)          │
│  • Personal files (~/...)                                │
│  • Session environment variables                         │
│  • User-scoped services (syncthing, etc.)                │
│  • Can read system packages                              │
│  • Can override IT-set user files (with warning)         │
│                                                           │
│  Command: sys apply ~/.config/sys/sys.lua                │
└─────────────────────────────────────────────────────────┘
```

### Store Efficiency: Deduplication

Users can reference system packages without duplication:

```
# System installed git@2.40.0
/syslua/store/obj/abc123.../
└── bin/git

# Alice's store can hardlink to system store (same filesystem)
~/.local/share/sys/store/pkg/git/2.40.0/
└── bin/git → /syslua/store/obj/abc123.../bin/git  (hardlink)

# Or just reference via PATH (different filesystem)
~/.local/share/sys/env.sh:
  export PATH="/syslua/store/pkg/git/2.40.0/bin:$PATH"
```

### User Environment Script

```bash
# ~/.local/share/sys/env.sh (generated by sys apply)

# System packages (from /syslua/store)
export PATH="/syslua/store/pkg/git/2.40.0/bin:$PATH"
export PATH="/syslua/store/pkg/vim/9.0.0/bin:$PATH"
export PATH="/syslua/store/pkg/docker/24.0.0/bin:$PATH"

# User packages (from ~/.local/share/sys/store)
export PATH="$HOME/.local/share/sys/store/pkg/neovim/0.10.0/bin:$PATH"
export PATH="$HOME/.local/share/sys/store/pkg/ripgrep/15.1.0/bin:$PATH"
export PATH="$HOME/.local/share/sys/store/pkg/fzf/0.48.0/bin:$PATH"

# System env vars (session)
export LANG="en_US.UTF-8"
export TZ="America/New_York"

# User env vars (can override system session vars)
export EDITOR="nvim"
export VISUAL="nvim"
export FZF_DEFAULT_COMMAND="rg --files"
```

### Conflict Resolution Algorithm

When a user applies their configuration, sys.lua enforces clear boundaries between system and user control:

```
APPLY_USER_CONFIG(user_config):
    user_manifest = EVALUATE(user_config)
    system_manifest = LOAD_SYSTEM_MANIFEST()  // From /etc/sys/system.lua
    current_user = GET_CURRENT_USER()
    
    // Phase 1: Package validation and merging
    FOR EACH pkg IN user_manifest.packages:
        // Users can install any packages they want
        // Check if package exists in system store (for hardlinking)
        IF EXISTS_IN_SYSTEM_STORE(pkg.name, pkg.version):
            // Reuse system package via hardlink (efficient)
            user_manifest.packages[pkg].source = "system-store"
        ELSE:
            // Download to user store
            user_manifest.packages[pkg].source = "user-store"
    
    // Phase 2: File validation
    FOR EACH file IN user_manifest.files:
        // Block system file modification
        IF file.path STARTS_WITH "/etc" OR 
           file.path STARTS_WITH "/Library" OR
           file.path STARTS_WITH "/opt" OR
           file.path STARTS_WITH "C:\\Windows" OR
           file.path STARTS_WITH "C:\\Program Files":
            ERROR "Cannot modify system files: {file.path}"
            ERROR "System files are managed by: sudo sys apply /etc/sys/system.lua"
            ABORT
        
        // Check if overriding IT-managed user file
        FOR EACH system_user IN system_manifest.users:
            IF system_user.name == current_user:
                FOR EACH system_file IN system_user.files:
                    IF file.path == system_file.path:
                        WARN "⚠ Overriding IT-managed file: {file.path}"
                        WARN "  System config: /etc/sys/system.lua"
                        WARN "  Your version will take precedence"
                        IF NOT user_confirms("Continue?"):
                            ABORT
        
        // Validate path is in user's home directory
        IF NOT file.path STARTS_WITH "~/" AND
           NOT file.path STARTS_WITH HOME_DIRECTORY:
            ERROR "Users can only manage files in their home directory"
            ABORT
    
    // Phase 3: Service validation
    FOR EACH service IN user_manifest.services:
        // Block system service management
        IF NOT service.user:
            ERROR "Cannot manage system services: {service.name}"
            ERROR "Use 'user = true' for user-scoped services"
            ERROR "System services are managed by: sudo sys apply /etc/sys/system.lua"
            ABORT
        
        // Check for conflicts with system services
        IF service.name IN system_manifest.services:
            ERROR "Service {service.name} is managed by system config"
            ERROR "System config: /etc/sys/system.lua"
            ABORT
        
        // User-scoped service is allowed
        service.run_as_user = current_user
    
    // Phase 4: Environment variable validation
    FOR EACH (var, value) IN user_manifest.env.session:
        // Check for system persistent env vars (cannot override)
        IF var IN system_manifest.env.persistent:
            WARN "⚠ Cannot override system persistent env var: {var}"
            WARN "  System value: {system_manifest.env.persistent[var]}"
            WARN "  Skipping user value: {value}"
            SKIP
        
        // Warn about system session env var override
        IF var IN system_manifest.env.session:
            WARN "⚠ Overriding system env var: {var}"
            WARN "  System value: {system_manifest.env.session[var]}"
            WARN "  User value: {value}"
        
        // Allowed
        user_manifest.env.session[var] = value
    
    // Phase 5: Block persistent env vars for users
    IF user_manifest.env.persistent IS NOT EMPTY:
        ERROR "Users cannot set persistent environment variables"
        ERROR "Persistent vars are system-wide and managed by: sudo sys apply /etc/sys/system.lua"
        ERROR "Use session env vars instead (they apply to your shell only)"
        ABORT
    
    // Phase 6: Apply user configuration
    APPLY_USER_MANIFEST(user_manifest, "~/.local/share/sys/store")
    GENERATE_USER_ENV_SCRIPTS(user_manifest, system_manifest)
    CREATE_USER_SNAPSHOT(user_manifest)
    
    PRINT "User apply complete!"

LOAD_SYSTEM_MANIFEST():
    // Load system manifest if exists
    IF EXISTS("/etc/sys/system.lua"):
        RETURN CACHED_MANIFEST("/syslua/store/metadata/manifest.json")
    ELSE:
        RETURN EMPTY_MANIFEST

GENERATE_USER_ENV_SCRIPTS(user_manifest, system_manifest):
    env_script = ""
    
    // Add system packages first
    FOR EACH pkg IN system_manifest.packages:
        env_script += "export PATH=\"/syslua/store/pkg/{pkg.name}/{pkg.version}/bin:$PATH\"\n"
    
    // Add user packages (higher priority in PATH)
    FOR EACH pkg IN user_manifest.packages:
        IF pkg.source == "system-store":
            // Already in PATH via system, skip
            CONTINUE
        ELSE:
            env_script += "export PATH=\"$HOME/.local/share/sys/store/pkg/{pkg.name}/{pkg.version}/bin:$PATH\"\n"
    
    // Add system session env vars
    FOR EACH (var, value) IN system_manifest.env.session:
        IF var NOT IN user_manifest.env.session:
            env_script += "export {var}=\"{value}\"\n"
    
    // Add user env vars (overrides system session vars)
    FOR EACH (var, value) IN user_manifest.env.session:
        env_script += "export {var}=\"{value}\"\n"
    
    WRITE("~/.local/share/sys/env.sh", env_script)
```

### Platform-Specific Admin Detection

| Platform | System Config Requires | User Config Requires |
|----------|----------------------|---------------------|
| Linux    | `geteuid() == 0`     | Regular user (non-root) |
| macOS    | `geteuid() == 0`     | Regular user (non-root) |
| Windows  | Administrator token  | Regular user |

**Admin check pseudocode:**

```
IS_SYSTEM_CONFIG_PATH(config_path):
    // System configs are in specific locations
    RETURN config_path STARTS_WITH "/etc/sys/" OR
           config_path STARTS_WITH "C:\\ProgramData\\sys\\" OR
           config_path STARTS_WITH "/Library/sys/"

VALIDATE_PRIVILEGES(config_path):
    IF IS_SYSTEM_CONFIG_PATH(config_path):
        IF NOT IS_ADMIN():
            ERROR "System configuration requires administrator privileges"
            ERROR "Run: sudo sys apply {config_path}"
            ABORT
    ELSE:
        IF IS_ADMIN():
            WARN "⚠ Running user config as admin is not recommended"
            WARN "System configs should be in /etc/sys/"
            WARN "User configs should be in ~/.config/sys/"
            IF NOT user_confirms("Continue?"):
                ABORT
```

### Benefits of Multi-Level Store

✅ **IT maintains control** - System configuration cannot be overridden by users
✅ **Users have freedom** - Can add packages and customize their environment
✅ **No sudo fatigue** - Users don't need sudo for daily config changes
✅ **Clear boundaries** - Explicit errors when users try to modify system resources
✅ **Efficient** - System packages shared via hardlinks (no duplication)
✅ **Safe** - Users cannot break system configuration or services
✅ **Auditable** - System changes require sudo, user changes tracked separately
✅ **Multi-user friendly** - Each user manages their own config independently

---

## Logging and Observability

sys.lua provides structured logging and debugging capabilities.

### Log Levels

| Level   | Usage                            |
| ------- | -------------------------------- |
| `ERROR` | Unrecoverable errors             |
| `WARN`  | Recoverable issues, deprecations |
| `INFO`  | Key operations (default)         |
| `DEBUG` | Detailed operation info          |
| `TRACE` | Very verbose debugging           |

### Log Configuration

```bash
# Set log level via environment variable
$ SYS_LOG=debug sys apply sys.lua

# Or via command line flag
$ sys apply --log-level debug sys.lua

# Log to file
$ sys apply --log-file /var/log/sys.log sys.lua

# JSON structured logging (for parsing)
$ sys apply --log-format json sys.lua
```

**Declarative logging config:**

```lua
-- sys.lua
logging {
    level = "info",
    file = "/var/log/sys/apply.log",
    format = "pretty",  -- "pretty", "json", "compact"

    -- Per-component log levels
    components = {
        ["sys-core::build"] = "debug",
        ["sys-core::store"] = "trace",
    },
}
```

### Log Output Format

**Pretty format (human-readable):**

```
[2024-01-15 10:30:45] INFO  sys-cli: Starting apply
[2024-01-15 10:30:45] DEBUG sys-core::manifest: Evaluating sys.lua
[2024-01-15 10:30:46] INFO  sys-core::store: Installing ripgrep@15.1.0
[2024-01-15 10:30:47] DEBUG sys-core::store: Downloading from https://...
[2024-01-15 10:30:50] INFO  sys-core::store: Verifying hash...
[2024-01-15 10:30:51] INFO  sys-core::store: Extracting to /syslua/store/obj/abc123...
[2024-01-15 10:30:52] INFO  sys-cli: Apply complete (7s)
```

**JSON format (machine-parseable):**

```json
{"timestamp":"2024-01-15T10:30:45Z","level":"INFO","target":"sys-cli","message":"Starting apply"}
{"timestamp":"2024-01-15T10:30:45Z","level":"DEBUG","target":"sys-core::manifest","message":"Evaluating sys.lua","file":"/home/user/sys.lua"}
{"timestamp":"2024-01-15T10:30:46Z","level":"INFO","target":"sys-core::store","message":"Installing ripgrep@15.1.0","package":"ripgrep","version":"15.1.0"}
```

### Log Locations

| Platform | Default Log Location                                                                 |
| -------- | ------------------------------------------------------------------------------------ |
| Linux    | `/var/log/sys/sys.log` (system), `~/.local/state/sys/sys.log` (user)                 |
| macOS    | `/var/log/sys/sys.log` (system), `~/Library/Logs/sys/sys.log` (user)                 |
| Windows  | `C:\ProgramData\sys\logs\sys.log` (system), `%LOCALAPPDATA%\sys\logs\sys.log` (user) |

### Debugging Tools

**Trace mode:**

```bash
# Show execution trace
$ sys apply --trace sys.lua

# Output:
TRACE [eval] Loading sys.lua
TRACE [eval] Calling pkg("ripgrep")
TRACE [manifest] Added package: ripgrep@15.1.0
TRACE [eval] Calling file{path="~/.gitconfig"}
TRACE [manifest] Added file: /home/user/.gitconfig
TRACE [dag] Building DAG with 2 nodes
TRACE [dag] Edge: file:/home/user/.gitconfig -> package:ripgrep@15.1.0
TRACE [executor] Executing node: package:ripgrep@15.1.0
```

**Config introspection:**

```bash
# Show evaluated manifest (before execution)
$ sys plan --show-manifest sys.lua

# Show DAG visualization
$ sys plan --show-dag sys.lua
```

**Performance profiling:**

```bash
# Show timing breakdown
$ sys apply --profile sys.lua

# Output:
Phase                    Time      %
─────────────────────────────────────
Evaluation               1.2s     10%
Input resolution         2.5s     20%
DAG construction         0.5s      4%
Package downloads        6.8s     55%
Extraction               1.0s      8%
Post-install hooks       0.4s      3%
─────────────────────────────────────
Total                   12.4s    100%
```

### Error Reporting

sys.lua provides detailed, actionable error messages:

**Example: Hash mismatch**

```
Error: Hash verification failed for ripgrep@15.1.0

  Expected: abc123def456...
  Got:      def456abc123...

This usually means:
  1. The download was corrupted (try again)
  2. The upstream file changed (package maintainer should update hash)
  3. You're behind a transparent proxy that modifies downloads

To fix:
  - Retry: sys apply sys.lua
  - Update hash: sys hash https://github.com/.../ripgrep-15.1.0.tar.gz
  - Report issue: https://github.com/sys-lua/pkgs/issues

Location: sys.lua:42
Package definition: github:sys-lua/pkgs/ripgrep/15.1.0.lua
```

**Example: Circular dependency**

```
Error: Circular dependency detected

  neovim -> ripgrep -> fd -> neovim

Dependency chain:
  1. Package 'neovim' depends on 'ripgrep' (sys.lua:15)
  2. Package 'ripgrep' depends on 'fd' (pkgs/ripgrep.lua:23)
  3. Package 'fd' depends on 'neovim' (pkgs/fd.lua:18)

To fix: Remove one of these dependencies.
```

**Example: Platform not supported**

```
Error: Platform 'x86_64-darwin' not supported for package 'custom-tool@1.0.0'

Available platforms:
  - x86_64-linux
  - aarch64-linux

Options:
  1. Add platform-specific binary:
     sha256 = {
       ["x86_64-linux"] = "...",
       ["aarch64-linux"] = "...",
       ["x86_64-darwin"] = "...",  // Add this
     }

  2. Provide a build function to compile from source:
     build = function(src, opts)
       return { buildPhase = "...", installPhase = "..." }
     end

  3. Use platform conditionals to skip on unsupported platforms:
     if not sys.is_darwin then
       pkg("custom-tool")
     end

Location: sys.lua:42
```

---

## Self-Update Strategy

sys.lua can update itself using the same mechanisms it uses for packages.

### Updating sys.lua

```bash
# Update to latest stable
$ sys self-update

# Update to specific version
$ sys self-update --version 0.5.0

# Update to latest from channel
$ sys self-update --channel unstable
```

### Update Process

```
SELF_UPDATE(target_version):
    current_version = GET_CURRENT_VERSION()

    IF target_version <= current_version:
        PRINT "Already at version {current_version}"
        RETURN

    // Fetch new sys.lua binary
    binary_url = RESOLVE_BINARY_URL(target_version, platform)
    new_binary = DOWNLOAD(binary_url)

    // Verify signature
    signature = DOWNLOAD(binary_url + ".sig")
    IF NOT VERIFY_SIGNATURE(new_binary, signature, OFFICIAL_PUBLIC_KEY):
        ERROR "Invalid signature for sys.lua {target_version}"

    // Backup current binary
    current_binary = GET_EXECUTABLE_PATH()
    backup_path = current_binary + ".backup"
    COPY(current_binary, backup_path)

    // Replace binary (atomic on Unix, best-effort on Windows)
    TRY:
        ATOMIC_REPLACE(current_binary, new_binary)
        PRINT "Updated sys.lua to version {target_version}"

        // Verify new version works
        result = RUN_COMMAND([current_binary, "--version"])
        IF result.version != target_version:
            // Rollback
            ATOMIC_REPLACE(current_binary, backup_path)
            ERROR "Update verification failed, rolled back"

        REMOVE(backup_path)
    CATCH error:
        // Restore backup
        IF EXISTS(backup_path):
            ATOMIC_REPLACE(current_binary, backup_path)
        ERROR "Update failed: {error}"
```

### Update Channels

| Channel    | Description               | Update Frequency |
| ---------- | ------------------------- | ---------------- |
| `stable`   | Production-ready releases | Monthly          |
| `beta`     | Pre-release testing       | Weekly           |
| `unstable` | Latest development        | Daily            |

**Configuring update channel:**

```lua
-- sys.lua
self {
    updateChannel = "stable",
    autoUpdate = false,  -- Disable automatic update checks
}
```

### Compatibility

sys.lua maintains backward compatibility with config files:

| sys.lua Version | Compatible Config Versions |
| --------------- | -------------------------- |
| 0.5.x           | 0.4.x, 0.5.x               |
| 0.4.x           | 0.3.x, 0.4.x               |
| 0.3.x           | 0.3.x only                 |

**Version detection:**

```lua
-- sys.lua
if sys.version < "0.5" then
    error("This config requires sys.lua >= 0.5")
end
```

**Deprecation warnings:**

```
Warning: sys.registry() is deprecated and will be removed in 0.6
  Use: local inputs = { pkgs = input "github:sys-lua/pkgs" }
  Location: sys.lua:10
```

Format: `<arch>-<os>`

| Platform       | Identifier        |
| -------------- | ----------------- |
| Linux x86_64   | `x86_64-linux`    |
| Linux ARM64    | `aarch64-linux`   |
| macOS x86_64   | `x86_64-darwin`   |
| macOS ARM64    | `aarch64-darwin`  |
| Windows x86_64 | `x86_64-windows`  |
| Windows ARM64  | `aarch64-windows` |

---

## Error Handling

- **Library crates** (`sys-core`, `sys-lua`): Use `thiserror` with custom `Error` enums
- **Application crates** (`sys-cli`): Use `anyhow::Result`

```rust
// In sys-core
#[derive(Error, Debug)]
pub enum Error {
    #[error("Package not found: {name}@{version}")]
    PackageNotFound { name: String, version: String },

    #[error("Hash mismatch for {name}: expected {expected}, got {actual}")]
    HashMismatch { name: String, expected: String, actual: String },
    // ...
}
```

---

## Development

### Building

```bash
cargo build                    # Build all crates
cargo build -p sys-cli      # Build specific crate
```

### Testing

```bash
cargo test                     # Run all tests
cargo test -p sys-core      # Test specific crate
```

### Linting

```bash
cargo clippy                   # Lint
cargo fmt                      # Format
```

### Local Development

Place a `pkgs/` directory in the project root to use as a local input for testing package definitions:

```lua
-- Use local packages for development
local inputs = {
    pkgs = input "path:./pkgs",
}
```

---

## Architecture Summary

This document has specified the complete architecture for sys.lua, a cross-platform declarative system/environment manager. The key architectural decisions are:

### Core Principles

1. **Declarative Configuration**: The `sys.lua` config file is the single source of truth
2. **Reproducibility**: Same config + same inputs = same environment
3. **Immutability**: Store objects are immutable and content-addressed
4. **Atomicity**: Apply operations are all-or-nothing with automatic rollback
5. **Cross-platform**: First-class support for Linux, macOS, and Windows

### System Architecture

```
┌─────────────────────────────────────────────────────────┐
│                     User Config (sys.lua)                │
│  - Declares packages, files, env vars, services         │
│  - Uses Lua for logic and composition                   │
└───────────────────────┬─────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────────┐
│                  Evaluation & Resolution                 │
│  - Parse Lua config → Manifest                          │
│  - Resolve inputs from lock file                        │
│  - Auto-evaluate modules                                │
│  - Resolve priority conflicts                           │
└───────────────────────┬─────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────────┐
│                   DAG Construction                       │
│  - Build execution graph from manifest                  │
│  - Add dependency edges (explicit + implicit)           │
│  - Topological sort                                     │
│  - Detect cycles                                        │
└───────────────────────┬─────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────────┐
│                   Parallel Execution                     │
│  - Execute DAG nodes in waves                           │
│  - Download/build packages                              │
│  - Create files                                         │
│  - Configure services                                   │
│  - Update environment                                   │
└───────────────────────┬─────────────────────────────────┘
                        │
                        ▼
┌─────────────────────────────────────────────────────────┐
│                  Immutable Store                         │
│  obj/<hash>/   - Content-addressed objects              │
│  pkg/name/ver/ - Human-readable symlinks                │
└─────────────────────────────────────────────────────────┘
```

### Key Components

| Component        | Responsibility                                 |
| ---------------- | ---------------------------------------------- |
| **sys-cli**      | User interface, command handling               |
| **sys-core**     | Core logic: store, DAG, manifest, execution    |
| **sys-lua**      | Lua integration, config parsing, module system |
| **sys-platform** | OS abstraction (services, paths, permissions)  |
| **sys-sops**     | Secrets management integration                 |

### Data Flow

```
sys.lua config
  ↓ (parse & evaluate)
Manifest (intermediate representation)
  ↓ (resolve conflicts)
Resolved Manifest
  ↓ (build DAG)
Execution DAG
  ↓ (execute)
System State
  ↓ (snapshot)
Snapshot History
```

### Critical Algorithms

1. **Input Resolution**: Lock file validation, latest resolution, caching
2. **Module Evaluation**: Auto-evaluation via Lua introspection, dependency ordering
3. **Priority Resolution**: Numeric priority (lower wins), conflict detection
4. **DAG Construction**: Node/edge types, topological sort, cycle detection
5. **Parallel Execution**: Wave-based execution, dependency tracking, rollback on failure
6. **Snapshot Rollback**: State diff computation, atomic restoration, conflict handling
7. **Garbage Collection**: Root finding, locking, safe cleanup

### Security Model

- **Privilege separation**: Root for apply, non-root for plan/shell/status
- **Immutability**: Store objects protected with filesystem flags
- **Sandboxing**: Network-isolated, filesystem-restricted builds
- **Cryptographic verification**: Hash checking for all downloads
- **Signature verification**: Binary cache entries must be signed
- **Secrets management**: SOPS integration for encrypted credentials

### Extensibility Points

1. **Modules**: Reusable configuration bundles with options
2. **Custom packages**: Inline package definitions in user config
3. **Fetch helpers**: URL, Git, GitHub, GitLab, custom
4. **Build system**: Derivations with custom build phases
5. **Hooks**: Activation scripts, package hooks, service hooks
6. **Platform conditionals**: OS/architecture-specific configuration

### Performance Characteristics

- **Parallel downloads**: Multiple packages fetched concurrently
- **Parallel execution**: DAG waves execute in parallel
- **Binary cache**: Avoid rebuilds with shared cache
- **Local cache**: Input caching reduces repeated downloads
- **Incremental updates**: Only changed packages are updated
- **Lazy evaluation**: Modules evaluated only if used

### Implementation Status

This architecture document serves as the specification for implementation. All major subsystems are fully specified with:

- ✅ Concrete algorithms (pseudocode provided)
- ✅ Data structures (Rust types specified)
- ✅ Error handling patterns
- ✅ Platform-specific implementations
- ✅ Security considerations
- ✅ Performance optimizations

### Next Steps for Implementation

1. **Phase 1: Core Foundation**
   - Store management (obj/, pkg/ layout)
   - Basic package installation
   - Hash verification
   - Immutability flags

2. **Phase 2: Lua Integration**
   - Lua runtime setup
   - pkg(), file{}, env{} primitives
   - Manifest generation
   - Priority system

3. **Phase 3: Inputs & Registry**
   - Input resolution algorithm
   - Lock file management
   - GitHub/GitLab/Git fetching
   - Package registry structure

4. **Phase 4: DAG & Execution**
   - DAG construction
   - Topological sort
   - Parallel execution
   - Rollback on failure

5. **Phase 5: Advanced Features**
   - Module system
   - Service management
   - Build system & sandbox
   - Binary cache
   - Snapshots & rollback

6. **Phase 6: Polish**
   - Shell completions
   - Error messages
   - Logging & observability
   - Documentation
   - Self-update

### Testing Strategy

- **Unit tests**: Individual algorithms and data structures
- **Integration tests**: Full apply cycles with sample configs
- **Platform tests**: Test suite runs on Linux, macOS, Windows
- **Regression tests**: Snapshot tests for output consistency
- **Performance tests**: Benchmark critical paths (eval, DAG, execution)

### Documentation Needs

- **User Guide**: Getting started, common patterns, examples
- **API Reference**: All Lua functions and options
- **Package Authoring Guide**: How to write package definitions
- **Module Development Guide**: Creating reusable modules
- **Contributor Guide**: Development setup, coding standards

This architecture provides a solid foundation for building a production-ready system manager that combines Nix's reproducibility with Lua's simplicity and readability.
