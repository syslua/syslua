# Better CLI Outputs Implementation Plan

## Overview

Improve the syslua CLI outputs by completing `--json` flag support, adding colored output with `owo-colors`, enabling timestamps, and standardizing output formatting across all commands. This includes renaming the global `--verbose` flag to `--debug` for clarity.

## Current State Analysis

### What Exists

- CLI uses clap 4.5.53 with derive macros (`crates/cli/src/main.rs`)
- `--json` flag exists on Apply, Plan, Destroy commands but Diff/Status are missing it (causing compile errors)
- `--verbose` flag works for `diff` and `status` commands (per-command, shows detailed output)
- Global `-v/--verbose` flag sets tracing level (DEBUG vs INFO) - confusing naming
- Tracing configured with `.without_time()` - no timestamps
- Most types already have `Serialize` derives - ready for JSON output
- Output helpers scattered: `format_bytes()` in status.rs, `truncate_hash()` in diff.rs

### What's Missing

- Color support (no color library installed)
- Timestamps in log output
- Consistent hash truncation (12 chars in diff.rs, 8 in update.rs, 20 in status.rs)
- Shared output module to consolidate formatting logic
- `--color` global flag for color control

### Key Constraints

- Windows must be supported (owo-colors handles this natively)
- Breaking change: renaming `--verbose` to `--debug` (approved by user)
- Error types use `{ message: String }` pattern for serialization

## Desired End State

After implementation:

```bash
# Colored output (auto-detected TTY)
$ sys apply ./init.lua
✓ Apply complete!
  Snapshot: abc123def456
  Builds realized: 3
  Binds applied: 5
  Duration: 2.3s

# JSON output for tooling
$ sys apply ./init.lua --json
{
  "snapshot": { "id": "abc123def456", ... },
  "execution": { ... },
  "duration_ms": 2300
}

# Debug logging with timestamps
$ sys --debug apply ./init.lua
2025-12-29T14:30:00 DEBUG inputs::resolve: resolving input foo
2025-12-29T14:30:01 DEBUG execute::apply: applying bind bar
✓ Apply complete!
```

### Verification

- All commands support `--json` flag
- Colors appear in TTY, disabled in pipes/CI
- `--color always|never|auto` controls color behavior
- `--debug` enables DEBUG-level tracing with timestamps
- All tests pass: `cargo test`

## What We're NOT Doing

- Progress indicators (spinners, progress bars) - separate feature
- Interactive mode - out of scope
- Field-level diff within binds - out of scope
- Custom exit codes per error type - out of scope
- Terminal width detection/wrapping - out of scope

## Implementation Approach

1. Fix existing compile errors first (missing --json on Diff/Status)
2. Add color infrastructure (owo-colors dependency, --color flag)
3. Create shared output module to consolidate helpers
4. Update all commands to use shared module with colors
5. Enable timestamps in debug mode
6. Add timing to all command outputs

---

## Phase 1: Fix Compile Errors and Rename --verbose to --debug

### Overview

Fix the missing `--json` flag on Diff and Status commands, and rename the global `--verbose` flag to `--debug` for clarity.

### Changes Required

#### 1. Update Cli struct

**File**: `crates/cli/src/main.rs`
**Lines**: 10-18

```rust
#[derive(Parser)]
#[command(name = "syslua", author, version, about, long_about = None)]
struct Cli {
    /// Enable debug logging (DEBUG level instead of INFO)
    #[arg(short, long, global = true)]
    debug: bool,  // Renamed from verbose

    #[command(subcommand)]
    command: Commands,
}
```

#### 2. Add --json to Diff command

**File**: `crates/cli/src/main.rs`
**Lines**: 55-68 (Commands::Diff variant)

Add `json: bool` field:

```rust
Diff {
    #[arg(value_name = "SNAPSHOT_A")]
    snapshot_a: Option<String>,

    #[arg(value_name = "SNAPSHOT_B")]
    snapshot_b: Option<String>,

    #[arg(short, long)]
    verbose: bool,

    /// Output JSON instead of human-readable text
    #[arg(long)]
    json: bool,  // ADD THIS
},
```

#### 3. Add --json to Status command

**File**: `crates/cli/src/main.rs`
**Lines**: 73-77 (Commands::Status variant)

Add `json: bool` field:

```rust
Status {
    #[arg(short, long)]
    verbose: bool,

    /// Output JSON instead of human-readable text
    #[arg(long)]
    json: bool,  // ADD THIS
},
```

#### 4. Update command dispatch

**File**: `crates/cli/src/main.rs`
**Lines**: 96-127

Update tracing level variable name and command dispatch:

```rust
let level = if cli.debug { Level::DEBUG } else { Level::INFO };  // Renamed

// ... in match ...
Commands::Diff { snapshot_a, snapshot_b, verbose, json } => {
    cmd_diff(snapshot_a, snapshot_b, verbose, json)
}
// ...
Commands::Status { verbose, json } => {
    cmd_status(verbose, json);
    Ok(())
}
```

### Success Criteria

#### Automated Verification

- [ ] `cargo build -p syslua-cli` compiles without errors
- [ ] `cargo test -p syslua-cli` passes
- [ ] `cargo clippy -p syslua-cli` has no warnings

#### Manual Verification

- [ ] `sys --help` shows `--debug` instead of `--verbose`
- [ ] `sys diff --help` shows `--json` flag
- [ ] `sys status --help` shows `--json` flag
- [ ] `sys --debug apply ./init.lua` shows DEBUG-level logs

---

## Phase 2: Add Color Infrastructure

### Overview

Add `owo-colors` dependency with `--color` global flag for controlling colored output.

### Changes Required

#### 1. Add owo-colors dependency

**File**: `Cargo.toml` (workspace root)
**Section**: `[workspace.dependencies]`

```toml
owo-colors = { version = "4", features = ["supports-colors"] }
```

**File**: `crates/cli/Cargo.toml`
**Section**: `[dependencies]`

```toml
owo-colors = { workspace = true }
```

#### 2. Add ColorChoice enum and --color flag

**File**: `crates/cli/src/main.rs`

Add after imports:

```rust
use owo_colors::OwoColorize;

#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum ColorChoice {
    /// Auto-detect TTY (default)
    #[default]
    Auto,
    /// Always use colors
    Always,
    /// Never use colors
    Never,
}
```

Update Cli struct:

```rust
struct Cli {
    /// Enable debug logging
    #[arg(short, long, global = true)]
    debug: bool,

    /// Control colored output
    #[arg(long, value_enum, default_value = "auto", global = true)]
    color: ColorChoice,

    #[command(subcommand)]
    command: Commands,
}
```

#### 3. Configure color override in main()

**File**: `crates/cli/src/main.rs`

Add after parsing CLI args:

```rust
fn main() -> ExitCode {
    let cli = Cli::parse();

    // Configure color output
    match cli.color {
        ColorChoice::Always => owo_colors::set_override(true),
        ColorChoice::Never => owo_colors::set_override(false),
        ColorChoice::Auto => {}, // Use default TTY detection
    }

    // ... rest of main
}
```

### Success Criteria

#### Automated Verification

- [ ] `cargo build -p syslua-cli` compiles
- [ ] `cargo test -p syslua-cli` passes

#### Manual Verification

- [ ] `sys --help` shows `--color` flag with auto/always/never options
- [ ] `sys --color never status` produces no ANSI codes
- [ ] `sys status | cat` produces no ANSI codes (TTY detection)

---

## Phase 3: Create Shared Output Module

### Overview

Create a shared output module consolidating formatting helpers and providing colored output utilities.

### Changes Required

#### 1. Create output module

**File**: `crates/cli/src/output.rs` (new file)

```rust
//! Shared output formatting utilities for CLI commands.

use owo_colors::{OwoColorize, Stream};
use serde::Serialize;
use std::fmt::Display;
use std::time::Duration;

/// Truncate a hash string to the specified length.
pub fn truncate_hash(hash: &str, len: usize) -> &str {
    if hash.len() >= len {
        &hash[..len]
    } else {
        hash
    }
}

/// Default hash truncation length (12 characters).
pub const HASH_DISPLAY_LEN: usize = 12;

/// Format bytes as human-readable string (KB, MB, GB).
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}

/// Format a duration as human-readable string.
pub fn format_duration(duration: Duration) -> String {
    let secs = duration.as_secs_f64();
    if secs >= 60.0 {
        let mins = secs / 60.0;
        format!("{:.1}m", mins)
    } else if secs >= 1.0 {
        format!("{:.1}s", secs)
    } else {
        format!("{:.0}ms", duration.as_millis())
    }
}

/// Print a success message with green checkmark.
pub fn print_success(msg: &str) {
    let prefix = "✓".if_supports_color(Stream::Stdout, |t| t.green());
    println!("{} {}", prefix, msg);
}

/// Print an error message with red X to stderr.
pub fn print_error(msg: &str) {
    let prefix = "✗".if_supports_color(Stream::Stderr, |t| t.red());
    eprintln!("{} {}", prefix, msg);
}

/// Print a warning message with yellow warning symbol.
pub fn print_warning(msg: &str) {
    let prefix = "⚠".if_supports_color(Stream::Stderr, |t| t.yellow());
    eprintln!("{} {}", prefix, msg);
}

/// Print an info message with cyan info symbol.
pub fn print_info(msg: &str) {
    let prefix = "ℹ".if_supports_color(Stream::Stdout, |t| t.cyan());
    println!("{} {}", prefix, msg);
}

/// Print a labeled statistic with consistent formatting.
pub fn print_stat(label: &str, value: impl Display) {
    println!("  {}: {}", label, value);
}

/// Print result as JSON to stdout.
pub fn print_json<T: Serialize>(value: &T) -> Result<(), serde_json::Error> {
    let json = serde_json::to_string_pretty(value)?;
    println!("{}", json);
    Ok(())
}

/// Symbols for diff output.
pub mod symbols {
    pub const ADDED: &str = "+";
    pub const REMOVED: &str = "-";
    pub const UPDATED: &str = "~";
    pub const UNCHANGED: &str = "=";
}
```

#### 2. Export from lib

**File**: `crates/cli/src/main.rs`

Add module declaration:

```rust
mod cmd;
mod output;  // ADD THIS

use output::{print_success, print_error};  // Use as needed
```

### Success Criteria

#### Automated Verification

- [ ] `cargo build -p syslua-cli` compiles
- [ ] `cargo test -p syslua-cli` passes
- [ ] `cargo clippy -p syslua-cli` has no warnings

#### Manual Verification

- [ ] Module compiles and exports are accessible

---

## Phase 4: Update Commands for Colored Output

### Overview

Update all command handlers to use the shared output module for consistent colored output.

### Changes Required

#### 1. Update apply.rs

**File**: `crates/cli/src/cmd/apply.rs`

Replace success output:

```rust
use crate::output::{print_success, print_stat, print_error, format_duration, print_json};

// At end of successful apply:
if json {
    print_json(&result)?;
} else {
    println!();
    print_success("Apply complete!");
    print_stat("Snapshot", &result.snapshot.id);
    print_stat("Builds realized", result.execution.realized.len());
    print_stat("Builds cached", result.diff.builds_cached.len());
    print_stat("Binds applied", result.execution.applied.len());
    print_stat("Binds updated", result.binds_updated);
    print_stat("Binds destroyed", result.binds_destroyed);
    print_stat("Binds unchanged", result.diff.binds_unchanged.len());
    print_stat("Duration", format_duration(elapsed));
}
```

Replace error output:

```rust
if let Some((hash, ref err)) = result.execution.build_failed {
    print_error(&format!("Build failed: {} - {}", hash.0, err));
}
```

#### 2. Update destroy.rs

**File**: `crates/cli/src/cmd/destroy.rs`

Similar pattern - use `print_success`, `print_stat` for output.

#### 3. Update diff.rs

**File**: `crates/cli/src/cmd/diff.rs`

- Import shared `truncate_hash` and `symbols`
- Remove local `truncate_hash` function
- Use `HASH_DISPLAY_LEN` constant

#### 4. Update status.rs

**File**: `crates/cli/src/cmd/status.rs`

- Import shared `format_bytes`, `truncate_hash`
- Remove local `format_bytes` function
- Add JSON output support

#### 5. Update remaining commands

Apply similar patterns to:

- `init.rs` - use `print_success` for initialization message
- `plan.rs` - use `print_stat` for plan statistics
- `update.rs` - use shared `truncate_hash` with consistent length

### Success Criteria

#### Automated Verification

- [ ] `cargo build -p syslua-cli` compiles
- [ ] `cargo test -p syslua-cli` passes
- [ ] `cargo clippy -p syslua-cli` has no warnings

#### Manual Verification

- [ ] `sys apply ./init.lua` shows green checkmark on success
- [ ] `sys status` shows colored output in terminal
- [ ] `sys status --json` outputs valid JSON
- [ ] `sys diff` uses consistent hash truncation
- [ ] Output colors are disabled when piped: `sys status | cat`

---

## Phase 5: Enable Timestamps in Debug Mode

### Overview

Enable timestamps in tracing output when `--debug` flag is used.

### Changes Required

#### 1. Update tracing subscriber configuration

**File**: `crates/cli/src/main.rs`

```rust
let level = if cli.debug { Level::DEBUG } else { Level::INFO };

let subscriber = FmtSubscriber::builder()
    .with_max_level(level)
    .with_target(false);

// Add timestamps only in debug mode
let subscriber = if cli.debug {
    subscriber.with_timer(tracing_subscriber::fmt::time::LocalTime::rfc_3339())
} else {
    subscriber.without_time()
};

subscriber.init();
```

Note: This requires the `time` feature on `tracing-subscriber`. Check if already enabled, otherwise add to Cargo.toml:

**File**: `Cargo.toml` (workspace root)

```toml
tracing-subscriber = { version = "0.3", features = ["env-filter", "local-time"] }
```

### Success Criteria

#### Automated Verification

- [ ] `cargo build -p syslua-cli` compiles
- [ ] `cargo test -p syslua-cli` passes

#### Manual Verification

- [ ] `sys apply ./init.lua` shows no timestamps (default mode)
- [ ] `sys --debug apply ./init.lua` shows timestamps like `2025-12-29T14:30:00`
- [ ] Timestamps are in local timezone

---

## Phase 6: Add Timing to All Commands

### Overview

Add execution timing to all command outputs for summary statistics.

### Changes Required

#### 1. Add timing wrapper pattern

For each command that performs work, wrap execution with timing:

```rust
use std::time::Instant;
use crate::output::format_duration;

pub fn cmd_apply(file: &str, repair: bool, json: bool) -> Result<()> {
    let start = Instant::now();

    // ... existing logic ...

    let elapsed = start.elapsed();

    if !json {
        print_stat("Duration", format_duration(elapsed));
    }
    // For JSON, include in result struct
}
```

#### 2. Commands to update

- `apply.rs` - Add timing around apply operation
- `destroy.rs` - Add timing around destroy operation
- `plan.rs` - Add timing around plan evaluation
- `update.rs` - Add timing around update operation

Commands that don't need timing (instant operations):

- `info.rs` - Just prints static info
- `status.rs` - Just reads snapshot
- `init.rs` - Just creates files

### Success Criteria

#### Automated Verification

- [ ] `cargo build -p syslua-cli` compiles
- [ ] `cargo test -p syslua-cli` passes

#### Manual Verification

- [ ] `sys apply ./init.lua` shows "Duration: X.Xs" at the end
- [ ] `sys destroy --dry-run` shows duration
- [ ] `sys plan ./init.lua` shows duration
- [ ] Duration format is human-readable (ms, s, or m as appropriate)

---

## Testing Strategy

### Unit Tests

- `output.rs` - Test `format_bytes`, `format_duration`, `truncate_hash`
- Test edge cases: 0 bytes, exactly 1 KB, hash shorter than truncation length

### Integration Tests

- Existing CLI smoke tests should continue to pass
- Add tests for `--json` output parsing
- Add tests for `--color never` producing no ANSI codes

### Manual Testing Steps

1. Run `sys apply` on a valid config and verify colored output
2. Run `sys status --json | jq .` to verify valid JSON
3. Run `sys --debug status` to verify timestamps appear
4. Run `sys status | cat` to verify no color codes in pipe
5. Test on Windows if available to verify ANSI support

## Performance Considerations

- `owo-colors` is zero-cost abstraction - no runtime overhead
- JSON serialization is only performed when `--json` flag is used
- Timing uses `std::time::Instant` which is low overhead

## Migration Notes

### Breaking Changes

1. **`--verbose` renamed to `--debug`**: Users with scripts using `-v` or `--verbose` globally will need to update to `-d` or `--debug`

### Deprecation Strategy

Consider adding a hidden `--verbose` alias that prints a deprecation warning:

```rust
#[arg(long, global = true, hide = true)]
verbose: bool,  // Deprecated, maps to debug
```

Then in main():

```rust
if cli.verbose {
    eprintln!("Warning: --verbose is deprecated, use --debug instead");
}
let debug = cli.debug || cli.verbose;
```

This is optional and can be skipped if a clean break is preferred.

## References

- Original ticket: `thoughts/tickets/better-outputs.md`
- Research document: `thoughts/research/2025-12-29_better-cli-outputs.md`
- diff-command.md plan: Pattern for --verbose implementation
- sys-status-command.md plan: Pattern for format_bytes and output structure
- owo-colors docs: https://docs.rs/owo-colors
