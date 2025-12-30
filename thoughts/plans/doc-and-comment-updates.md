# Documentation and Comment Updates Implementation Plan

## Overview

Update documentation and code comments across the syslua codebase to ensure accuracy, clarity, and consistency. This includes fixing architecture doc discrepancies with current code, adding module-level documentation to Rust files, documenting public APIs, and expanding Lua type definitions.

## Current State Analysis

### Documentation Inventory
- 45 documentation files across the codebase
- 10 architecture docs in `docs/architecture/`
- 77-line `globals.d.lua` with core type definitions
- Established patterns in `crates/lib/src/inputs/` serve as templates

### Key Discoveries

- **73% of mod.rs files lack module documentation** (`inputs/mod.rs:1-14` is the gold standard)
- **9 public functions in `output.rs`** have no documentation
- **7 architecture doc discrepancies** with actual implementation:
  - `03-store.md` claims `obj/<name>-<version>-<hash>/` but code uses `build/<hash>/`
  - `05-snapshots.md` uses `metadata.json` but code uses `index.json`
  - `05-snapshots.md` uses `apply_actions` but code uses `create_actions`
- **Lua type definitions** for `syslua.lib` and `syslua.modules` are inline, not centralized

## Desired End State

After this plan is complete:

1. All architecture docs accurately reflect current implementation
2. All core Rust modules have `//!` module-level documentation
3. All public functions in CLI have `///` documentation
4. `globals.d.lua` includes type definitions for `syslua.lib` and `syslua.modules`
5. Documentation follows established patterns from `inputs/` module

### Verification

```bash
# Automated: Build and test pass
cargo build
cargo test
cargo clippy --all-targets --all-features

# Automated: No new warnings from rustdoc
cargo doc --no-deps 2>&1 | grep -c "warning" # Should be 0 or unchanged

# Manual: Review each modified file for clarity and accuracy
```

## What We're NOT Doing

- Implementing new features (`sys.hostname`, `sys.username`, `sys.version`)
- Changing store naming convention (update docs to match code, not vice versa)
- Auto-generating Lua types from Rust (future consideration)
- Adding documentation style guide to AGENTS.md (follow-up task)
- Splitting `globals.d.lua` into multiple files

## Implementation Approach

Work in priority order: fix incorrect docs first, then add missing docs. Use `inputs/mod.rs` and `placeholder.rs` as templates for all new documentation.

---

## Phase 1: Fix Architecture Documentation Discrepancies

### Overview

Correct factual inaccuracies in architecture docs where documentation doesn't match implementation.

### Changes Required

#### 1. Store Structure (`docs/architecture/03-store.md`)

**Changes**: Update store path format from planned design to actual implementation.

| Line(s) | Current | Correct |
|---------|---------|---------|
| 5 | `store/obj/` ... `obj/name-version-hash/` | `store/build/` ... `build/<hash>/` |
| 37 | `obj/<name>-<version>-<hash>/` | `build/<hash>/` |
| 41-43, 62 | References to `drv/`, `drv-out/` | Remove (not implemented) |
| 52-54 | `obj/ripgrep-15.1.0-abc123...` | `build/abc123def456789012/` |
| 70 | `obj/<name>-<version>-<hash>/` | `build/<hash>/` |
| 165 | `obj/<hash>/` | `build/<hash>/` |

Update the ASCII diagram to show actual structure:
```text
{store_dir}/
├── build/<hash>/     # Realized build outputs (immutable)
│   ├── bin/          # Executables
│   ├── lib/          # Libraries
│   └── ...
└── bind/<hash>/      # Bind state directory
    └── state.json    # Bind execution state
```

#### 2. Snapshots Terminology (`docs/architecture/05-snapshots.md`)

**Changes**: Update terminology and type definitions to match code.

| Line | Current | Correct |
|------|---------|---------|
| 66 | `metadata.json` | `index.json` |
| 44-48 | `apply_actions: Vec<BindAction>` | `create_actions: Vec<Action>` |
| 45 | `inputs: Option<InputsRef>` | `inputs: Option<BindInputsDef>` |
| 48 | `destroy_actions: Option<Vec<BindAction>>` | `destroy_actions: Vec<Action>` |
| 87 | `activation_count` | `bind_count` |
| 111, 121 | `"activations"` | `"bindings"` |

Update the `BindDef` struct example to match `bind/types.rs:236-257`:
```rust
pub struct BindDef {
    pub id: Option<String>,
    pub inputs: Option<BindInputsDef>,
    pub outputs: Option<BTreeMap<String, String>>,
    pub create_actions: Vec<Action>,
    pub update_actions: Option<Vec<Action>>,
    pub destroy_actions: Vec<Action>,
    pub check_actions: Option<Vec<Action>>,
    pub check_outputs: Option<BindCheckOutputs>,
}
```

#### 3. Apply Flow (`docs/architecture/08-apply-flow.md`)

**Changes**: Add documentation for repair mode and drift detection.

Add new section after "Rollback Behavior":

```markdown
## Repair Mode

When `--repair` is passed to `sys apply`, the system checks for drift in unchanged binds:

1. For each bind in `binds_unchanged`, run its `check` callback (if present)
2. If `check` returns `drifted: true`, add to repair list
3. Re-run `create` or `update` for drifted binds
4. Report drift results in `ApplyResult.drift_results`

This enables detecting and fixing configuration drift without a full re-apply.
```

### Success Criteria

#### Automated Verification
- [ ] `cargo build` passes
- [ ] `cargo test` passes
- [ ] No broken internal links in docs (check with markdown linter if available)

#### Manual Verification
- [ ] Store structure diagram matches `build/store.rs` and `bind/store.rs` implementations
- [ ] BindDef struct in docs matches `bind/types.rs:236-257`
- [ ] Snapshot index filename matches `snapshot/storage.rs:24`

---

## Phase 2: Add Rust Module Documentation

### Overview

Add `//!` module-level documentation to 5 core modules following the pattern from `inputs/mod.rs`.

### Changes Required

#### 1. Action Module (`crates/lib/src/action/mod.rs`)

Add at top of file:
```rust
//! Action execution and dispatch.
//!
//! This module handles executing actions recorded during build and bind Lua callbacks.
//! Actions are primitive operations like command execution or URL fetching that are
//! recorded via context methods (e.g., `ctx:exec()`) and executed at apply time.
//!
//! # Action Flow
//!
//! 1. During Lua evaluation, `ctx:exec()` and `ctx:fetch_url()` record actions
//! 2. Actions are stored in [`BuildDef`](crate::build::BuildDef) or [`BindDef`](crate::bind::BindDef)
//! 3. At apply time, [`execute_action`] dispatches each action to its handler
//! 4. Placeholders (e.g., `$${action:0}`, `$${out}`) are resolved before execution
//!
//! # Modules
//!
//! - [`actions`] - Action handler implementations (exec, fetch_url)
//! - [`types`] - Core types ([`Action`], [`ActionCtx`])
```

#### 2. Action Handlers (`crates/lib/src/action/actions/mod.rs`)

Add at top of file:
```rust
//! Action handler implementations.
//!
//! Each sub-module provides the execution logic for a specific action type:
//!
//! - [`exec`] - Execute commands with isolated environment
//! - [`fetch_url`] - Download files with SHA256 integrity verification
```

#### 3. Bind Module (`crates/lib/src/bind/mod.rs`)

Add at top of file:
```rust
//! Bind definition and execution.
//!
//! Bindings represent system state changes (symlinks, config files, services)
//! that can be created and destroyed. Unlike builds, binds are not cached—they
//! modify mutable system state.
//!
//! # Two-Tier Architecture
//!
//! - [`BindSpec`](types::BindSpec): Lua-side specification containing closures (not serializable)
//! - [`BindDef`](types::BindDef): Evaluated, serializable definition stored in manifests
//!
//! # Lifecycle
//!
//! Binds support `create`, `update` (optional), and `destroy` callbacks:
//! - `create(inputs, ctx)` - Apply the binding, return outputs
//! - `update(outputs, inputs, ctx)` - Update an existing binding (requires `id`)
//! - `destroy(outputs, ctx)` - Remove the binding
//! - `check(outputs, inputs, ctx)` - Detect drift (optional)
//!
//! # Modules
//!
//! - [`execute`] - Bind action execution at apply time
//! - [`lua`] - Lua integration for `sys.bind{}`
//! - [`state`] - Bind state persistence
//! - [`store`] - Bind output storage paths
//! - [`types`] - Core types ([`BindSpec`], [`BindDef`], [`BindCtx`])
```

#### 4. Build Module (`crates/lib/src/build/mod.rs`)

Add at top of file:
```rust
//! Build definition and execution.
//!
//! Builds are reproducible artifacts that produce content-addressed store objects.
//! They are identified by hashes computed from their definitions, enabling
//! deduplication and caching.
//!
//! # Two-Tier Architecture
//!
//! - [`BuildSpec`](types::BuildSpec): Lua-side specification containing closures (not serializable)
//! - [`BuildDef`](types::BuildDef): Evaluated, serializable definition stored in manifests
//!
//! # Build Process
//!
//! 1. Lua calls `sys.build{}` with a `create` function
//! 2. The `create` function records actions via `ctx:exec()`, `ctx:fetch_url()`
//! 3. Actions and outputs are stored in [`BuildDef`]
//! 4. At apply time, actions execute in isolated environment
//! 5. Results are stored in content-addressed store path
//!
//! # Modules
//!
//! - [`execute`] - Build action execution at apply time
//! - [`lua`] - Lua integration for `sys.build{}`
//! - [`store`] - Build storage and cache checking
//! - [`types`] - Core types ([`BuildSpec`], [`BuildDef`], [`BuildCtx`])
```

#### 5. Lua Module (`crates/lib/src/lua/mod.rs`)

Add at top of file:
```rust
//! Lua runtime and integration.
//!
//! This module provides the Lua runtime environment used to evaluate configuration
//! files. It creates the Lua VM, registers globals, and loads user code.
//!
//! # The `sys` Global Table
//!
//! The runtime registers a `sys` table providing:
//! - `sys.platform` - Platform triple (e.g., "aarch64-darwin")
//! - `sys.os` - Operating system (e.g., "darwin", "linux", "windows")
//! - `sys.arch` - CPU architecture (e.g., "x86_64", "aarch64")
//! - `sys.dir` - Directory containing the config file
//! - `sys.path` - Path manipulation utilities
//! - `sys.build{}` - Define a build
//! - `sys.bind{}` - Define a bind
//!
//! # Modules
//!
//! - [`entrypoint`] - Input declaration extraction from entrypoints
//! - [`globals`] - Global table registration
//! - [`helpers`] - Lua-accessible utility functions
//! - [`runtime`] - Lua VM creation and file loading
```

#### 6. CLI Commands Module (`crates/cli/src/cmd/mod.rs`)

Add at top of file:
```rust
//! CLI command implementations.
//!
//! Each submodule implements a single CLI command:
//!
//! | Command | Function | Description |
//! |---------|----------|-------------|
//! | `sys apply` | [`cmd_apply`] | Apply configuration to the system |
//! | `sys destroy` | [`cmd_destroy`] | Remove all managed state |
//! | `sys diff` | [`cmd_diff`] | Compare snapshots |
//! | `sys info` | [`cmd_info`] | Display system information |
//! | `sys init` | [`cmd_init`] | Initialize a new syslua project |
//! | `sys plan` | [`cmd_plan`] | Preview changes without applying |
//! | `sys status` | [`cmd_status`] | Show current system state |
//! | `sys update` | [`cmd_update`] | Update input lock file |
//!
//! Commands follow a consistent pattern:
//! - Accept parsed CLI arguments
//! - Return `anyhow::Result<()>`
//! - Use [`crate::output`] for formatted terminal output
```

### Success Criteria

#### Automated Verification
- [ ] `cargo build` passes
- [ ] `cargo doc --no-deps` generates docs without new warnings
- [ ] `cargo clippy` passes

#### Manual Verification
- [ ] Each module doc accurately describes the module's purpose
- [ ] Cross-references (`[`Type`]`) resolve correctly in generated docs
- [ ] Sub-module listings match actual exports

---

## Phase 3: Document CLI Output Utilities

### Overview

Add comprehensive documentation to `crates/cli/src/output.rs` (9 public functions + symbols module).

### Changes Required

#### File: `crates/cli/src/output.rs`

Add module documentation at top:
```rust
//! CLI output formatting utilities.
//!
//! This module provides consistent formatting for terminal output including:
//! - Unicode status symbols (success, error, warning, info)
//! - Human-readable formatting for bytes and durations
//! - Colored output helpers with automatic terminal detection
//!
//! All color functions use `owo_colors` with stream-aware conditional coloring,
//! automatically disabling colors when output is piped or redirected.
//!
//! # Symbols
//!
//! The [`symbols`] module contains Unicode characters for CLI output:
//! - `SUCCESS` (✓), `ERROR` (✗), `WARNING` (⚠), `INFO` (•)
//! - Diff indicators: `PLUS` (+), `MINUS` (-), `TILDE` (~), `ARROW` (→)
```

Add documentation to `symbols` module:
```rust
/// Unicode symbols for CLI status indicators and diff output.
pub mod symbols {
    /// Success indicator (✓)
    pub const SUCCESS: &str = "✓";
    /// Error indicator (✗)
    pub const ERROR: &str = "✗";
    /// Warning indicator (⚠)
    pub const WARNING: &str = "⚠";
    /// Info/bullet indicator (•)
    pub const INFO: &str = "•";
    /// Arrow for transitions (→)
    pub const ARROW: &str = "→";
    /// Addition indicator (+)
    pub const PLUS: &str = "+";
    /// Removal indicator (-)
    pub const MINUS: &str = "-";
    /// Modification indicator (~)
    pub const TILDE: &str = "~";
    /// Alias for PLUS
    pub const ADD: &str = "+";
    /// Alias for TILDE
    pub const MODIFY: &str = "~";
    /// Alias for MINUS
    pub const REMOVE: &str = "-";
}
```

Add function documentation (before each function):

```rust
/// Truncate a hash string for display.
///
/// Returns the first 12 characters of the hash, or the entire string
/// if shorter. Provides a readable preview while maintaining identification.
///
/// # Arguments
///
/// * `hash` - The hash string to truncate
pub fn truncate_hash(hash: &str) -> &str

/// Format a byte count for human-readable display.
///
/// Converts bytes to the most appropriate unit (B, KB, MB, or GB)
/// with one decimal place for units larger than bytes.
///
/// # Arguments
///
/// * `bytes` - The byte count to format
pub fn format_bytes(bytes: u64) -> String

/// Format a duration for human-readable display.
///
/// - Durations ≥60s: `Xm Ys` (minutes and seconds)
/// - Durations ≥1s: `X.YYs` (seconds with centiseconds)
/// - Durations <1s: `Xms` (milliseconds)
///
/// # Arguments
///
/// * `duration` - The duration to format
pub fn format_duration(duration: Duration) -> String

/// Print a success message with a green checkmark symbol.
///
/// Writes to stdout: `✓ <message>`
/// The checkmark is colored green when the terminal supports colors.
///
/// # Arguments
///
/// * `message` - The success message to display
pub fn print_success(message: &str)

/// Print an error message with a red X symbol.
///
/// Writes to stderr: `✗ <message>`
/// Both symbol and message are colored red when supported.
///
/// # Arguments
///
/// * `message` - The error message to display
pub fn print_error(message: &str)

/// Print a warning message with a yellow warning symbol.
///
/// Writes to stderr: `⚠ <message>`
/// Both symbol and message are colored yellow when supported.
///
/// # Arguments
///
/// * `message` - The warning message to display
pub fn print_warning(message: &str)

/// Print an informational message with a blue bullet symbol.
///
/// Writes to stdout: `• <message>`
/// The bullet is colored blue when supported.
///
/// # Arguments
///
/// * `message` - The informational message to display
pub fn print_info(message: &str)

/// Print a labeled statistic with indentation.
///
/// Writes to stdout: `  <label>: <value>`
/// The label is dimmed when the terminal supports colors.
///
/// # Arguments
///
/// * `label` - The statistic label (displayed dimmed)
/// * `value` - The statistic value
pub fn print_stat(label: &str, value: &str)

/// Print a value as pretty-printed JSON.
///
/// Serializes the provided value to JSON with indentation and prints
/// to stdout. Useful for `--json` output flags in CLI commands.
///
/// # Arguments
///
/// * `value` - Any value that implements `serde::Serialize`
///
/// # Errors
///
/// Returns an error if the value cannot be serialized to JSON.
pub fn print_json<T: serde::Serialize>(value: &T) -> anyhow::Result<()>
```

### Success Criteria

#### Automated Verification
- [ ] `cargo build -p syslua-cli` passes
- [ ] `cargo doc -p syslua-cli --no-deps` generates docs without warnings
- [ ] `cargo clippy -p syslua-cli` passes

#### Manual Verification
- [ ] Each function's doc accurately describes its behavior
- [ ] Output destinations (stdout vs stderr) are correctly documented
- [ ] Color behavior is correctly described

---

## Phase 4: Document Store and Command Files

### Overview

Add documentation to store utility files and CLI command implementations.

### Changes Required

#### 1. Bind Store (`crates/lib/src/bind/store.rs`)

Add module documentation and function docs:
```rust
//! Bind store path utilities.
//!
//! Binds are stored in the content-addressed store at `<store>/bind/<hash>/`.
//!
//! # Store Layout
//!
//! ```text
//! {store_dir}/bind/
//! └── <hash>/           # Bind state directory
//!     └── state.json    # Bind execution state
//! ```

/// Get the directory name for a bind in the store.
///
/// Returns the full hash string as the directory name.
///
/// # Arguments
///
/// * `hash` - The bind's content hash
pub fn bind_dir_name(hash: &ObjectHash) -> String

/// Get the full path to a bind's directory in the store.
///
/// # Arguments
///
/// * `hash` - The bind's content hash
///
/// # Returns
///
/// Absolute path: `<store>/bind/<hash>/`
pub fn bind_dir_path(hash: &ObjectHash) -> PathBuf
```

#### 2. Build Store (`crates/lib/src/build/store.rs`)

Add module documentation and function docs:
```rust
//! Build store path utilities.
//!
//! Builds are stored in the content-addressed store at `<store>/build/<hash>/`.
//!
//! # Store Layout
//!
//! ```text
//! {store_dir}/build/
//! └── <hash>/           # Build output directory
//!     ├── bin/          # Executables
//!     ├── lib/          # Libraries
//!     └── ...           # Other artifacts
//! ```

/// Get the directory name for a build in the store.
///
/// Returns the full hash string as the directory name.
///
/// # Arguments
///
/// * `hash` - The build's content hash
pub fn build_dir_name(hash: &ObjectHash) -> String

/// Get the full path to a build's directory in the store.
///
/// # Arguments
///
/// * `hash` - The build's content hash
///
/// # Returns
///
/// Absolute path: `<store>/build/<hash>/`
pub fn build_dir_path(hash: &ObjectHash) -> PathBuf

/// Check if a build already exists in the store (cache hit).
///
/// Used during apply to skip builds that have already been realized.
///
/// # Arguments
///
/// * `hash` - The build's content hash
/// * `store_path` - The base store directory path
///
/// # Returns
///
/// `true` if the build directory exists, `false` otherwise.
pub fn build_exists_in_store(hash: &ObjectHash, store_path: &Path) -> bool
```

#### 3. Status Command (`crates/cli/src/cmd/status.rs`)

Add module and function documentation:
```rust
//! The `sys status` command implementation.
//!
//! Displays information about the current system state including:
//! - Current snapshot ID and creation time
//! - Build and bind counts
//! - Store disk usage
//! - Detailed listings (with `--verbose`)

/// Execute the `sys status` command.
///
/// Loads the current snapshot and displays system state information.
/// If no snapshot exists, prints a message directing the user to run `sys apply`.
///
/// # Arguments
///
/// * `verbose` - If true, list all builds and binds with their IDs and hashes
/// * `json` - If true, output machine-readable JSON instead of human text
pub fn cmd_status(verbose: bool, json: bool) -> Result<()>
```

#### 4. Diff Command (`crates/cli/src/cmd/diff.rs`)

Add module and function documentation:
```rust
//! The `sys diff` command implementation.
//!
//! Compares two snapshots and displays changes to builds and binds.
//!
//! # Usage Modes
//!
//! - No arguments: Compare previous snapshot → current snapshot
//! - Two snapshot IDs: Compare snapshot A → snapshot B

/// Execute the `sys diff` command.
///
/// Compares two snapshots and displays changes. With no arguments, compares
/// the previous snapshot to the current one.
///
/// # Arguments
///
/// * `snapshot_a` - Optional first snapshot ID (the "before" state)
/// * `snapshot_b` - Optional second snapshot ID (the "after" state)
/// * `verbose` - If true, show detailed action listings
/// * `json` - If true, output machine-readable JSON
///
/// # Errors
///
/// Returns an error if fewer than 2 snapshots exist (for default comparison),
/// only one snapshot ID is provided, or a specified snapshot cannot be loaded.
pub fn cmd_diff(snapshot_a: Option<String>, snapshot_b: Option<String>, verbose: bool, json: bool) -> Result<()>
```

#### 5. Info Command (`crates/cli/src/cmd/info.rs`)

Add module and function documentation:
```rust
//! The `sys info` command implementation.
//!
//! Displays system information relevant to syslua operation.

/// Execute the `sys info` command.
///
/// Prints system information to stdout, currently including
/// the platform triple (e.g., "x86_64-unknown-linux-gnu").
pub fn cmd_info()
```

### Success Criteria

#### Automated Verification
- [ ] `cargo build` passes
- [ ] `cargo doc --no-deps` passes without new warnings
- [ ] `cargo clippy` passes

#### Manual Verification
- [ ] Store layout diagrams match actual directory structure
- [ ] Command descriptions match actual behavior
- [ ] All public functions have documentation

---

## Phase 5: Complete Remaining Modules

### Overview

Add module-level documentation to remaining mod.rs files.

### Changes Required

#### 1. Lua Helpers (`crates/lib/src/lua/helpers/mod.rs`)

```rust
//! Lua helper utilities.
//!
//! Utility functions exposed to Lua code via the `sys` global table.
//!
//! # Modules
//!
//! - [`path`] - Path manipulation (`sys.path.join`, `sys.path.dirname`, etc.)
```

#### 2. Manifest (`crates/lib/src/manifest/mod.rs`)

```rust
//! Manifest types and operations.
//!
//! The manifest captures the complete desired state of a system. It's produced
//! by evaluating Lua configuration and contains all builds and bindings to apply.
//!
//! # Content Addressing
//!
//! Both builds and bindings use content-addressed hashes as keys:
//! - Enables automatic deduplication of identical definitions
//! - Makes equality checking efficient (compare hashes)
//! - Supports incremental updates by diffing manifests
```

#### 3. Outputs (`crates/lib/src/outputs/mod.rs`)

```rust
//! Output handling and conversion.
//!
//! Outputs are named values returned from build and bind `create` callbacks.
//! They can be referenced by subsequent builds/binds via placeholders
//! (e.g., `$${build:hash:output_name}`).
//!
//! # Modules
//!
//! - [`lua`] - Lua table parsing and conversion utilities
```

#### 4. Platform (`crates/lib/src/platform/mod.rs`)

```rust
//! Platform detection and cross-platform abstractions.
//!
//! Provides consistent APIs for platform-specific operations across
//! Linux, macOS, and Windows.
//!
//! # Platform Detection
//!
//! - [`Platform`] - Combined architecture and OS identifier
//! - [`platform_triple`] - Returns platform string (e.g., "aarch64-darwin")
//! - [`is_elevated`] - Check for root/admin privileges
//!
//! # Modules
//!
//! - [`arch`] - CPU architecture detection
//! - [`os`] - Operating system detection
//! - [`paths`] - Platform-appropriate directory paths
//! - [`immutable`] - Store path write protection
```

#### 5. Util (`crates/lib/src/util/mod.rs`)

```rust
//! General-purpose utilities.
//!
//! # Modules
//!
//! - [`hash`] - SHA256 hashing for content-addressed storage
//! - [`testutil`] - Cross-platform test helpers (test builds only)
```

### Success Criteria

#### Automated Verification
- [ ] `cargo build` passes
- [ ] `cargo doc --no-deps` passes without new warnings

#### Manual Verification
- [ ] Each module doc accurately describes purpose
- [ ] Sub-module listings match actual exports

---

## Phase 6: Expand Lua Type Definitions

### Overview

Add centralized type definitions for `syslua.lib` and `syslua.modules` to `globals.d.lua`.

### Changes Required

#### File: `lua/syslua/globals.d.lua`

Add after the existing `Sys` class definition (before line 74):

```lua
-- Library types (syslua.lib)

---@class FetchUrlOptions
---@field url string URL to fetch
---@field sha256 string Expected SHA256 checksum for integrity verification

---@class syslua.lib
---@field fetch_url fun(opts: FetchUrlOptions): BuildRef Fetches a file from a URL with SHA256 verification

-- Module types (syslua.modules)

---@class FileOptions
---@field target string Path to the target file or directory
---@field source? string Path to the source file or directory (mutually exclusive with content)
---@field content? string Content to write to the target file (mutually exclusive with source)
---@field mutable? boolean Whether the target should be mutable (default: false, creates symlink to build)

---@class syslua.modules.file
---@field setup fun(opts: FileOptions) Set up a file or directory according to options

---@class syslua.modules
---@field file syslua.modules.file File management module

-- Main syslua namespace

---@class syslua
---@field lib syslua.lib Library functions for common operations
---@field modules syslua.modules Pre-built modules for common patterns
---@field pkgs syslua.pkgs Package installation (lazy-loaded)
```

The complete file should have this structure after modifications:
1. `---@meta` header
2. Core types (ExecOpts, BuildCtx, BindCtx, etc.) - existing
3. Spec and Ref types (BuildSpec, BindSpec, etc.) - existing
4. PathHelpers - existing
5. Platform aliases - existing
6. Sys class - existing
7. **NEW: Library types (FetchUrlOptions, syslua.lib)**
8. **NEW: Module types (FileOptions, syslua.modules.file, syslua.modules)**
9. **NEW: Main syslua namespace class**
10. `sys = {}` declaration - existing

### Success Criteria

#### Automated Verification
- [ ] LuaLS validates the type definitions without errors (if LuaLS is available)
- [ ] No syntax errors when loading the file

#### Manual Verification
- [ ] IDE autocompletion works for `syslua.lib.fetch_url()`
- [ ] IDE autocompletion works for `syslua.modules.file.setup()`
- [ ] Type definitions match implementations in `lib/init.lua` and `modules/file.lua`
- [ ] Hover documentation appears for all new types

---

## Testing Strategy

### Automated Tests

All phases should pass:
```bash
cargo build
cargo test
cargo clippy --all-targets --all-features
cargo doc --no-deps
```

### Manual Verification

1. **Documentation accuracy**: Spot-check 3-5 documented items against actual implementation
2. **Cross-references**: Verify `[`TypeName`]` links resolve in generated rustdoc
3. **Lua types**: Test IDE autocompletion with the updated `globals.d.lua`

---

## References

- Original ticket: `thoughts/tickets/doc-and-comment-updates.md`
- Research document: `thoughts/research/2025-12-30_doc-and-comment-updates.md`
- Documentation template: `crates/lib/src/inputs/mod.rs:1-14`
- Module docs example: `crates/lib/src/placeholder.rs:1-33`
