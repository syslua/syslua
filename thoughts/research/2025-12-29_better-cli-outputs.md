---
date: 2025-12-29T14:28:27-05:00
git_commit: 026acc29438c685efe462a8cb810470d602f0737
branch: feat/sys-status-and-diff
repository: syslua
topic: "Better CLI Outputs"
tags: [research, cli, output-formatting, json, colors, ux]
last_updated: 2025-12-29
---

## Ticket Synopsis

Feature request to improve CLI outputs for better user experience:
- `--json` flag for machine-readable output
- Color coding (errors red, success green)
- Timestamps in log messages
- Summary statistics at end of operations
- Concise but informative outputs
- `--verbose` flag for detailed output

## Summary

The syslua CLI currently uses plain text output via `println!`/`eprintln!` with tracing for logging. The `--verbose` flag is partially implemented for `diff` and `status` commands. No color library is installed, and JSON output capability does not exist. The codebase has strong serialization foundations (serde) but execution result types lack `Serialize` derives.

**Key findings:**
1. **Partial implementation exists**: `--verbose` already works in `diff` and `status` commands
2. **Infrastructure ready for JSON**: 18 types already have `Serialize`, ~12 execution types need it added
3. **No color support**: Need to add `owo-colors` crate (recommended for 2025)
4. **Timestamps disabled**: Tracing configured with `.without_time()` - simple fix
5. **Output helpers scattered**: `format_bytes()`, `truncate_hash()` duplicated across commands

## Detailed Findings

### Current CLI Architecture

#### Entry Point and Command Structure
- **File**: `crates/cli/src/main.rs`
- **Framework**: clap 4.5.53 with derive macros
- **Pattern**: `Cli` struct with global flags, `Commands` enum with subcommands

```rust
struct Cli {
    #[arg(short, long, global = true)]
    verbose: bool,  // Controls tracing level (DEBUG vs INFO)
    
    #[command(subcommand)]
    command: Commands,
}
```

#### Commands Overview
| Command | Handler | Notable Flags |
|---------|---------|---------------|
| `apply` | `cmd_apply(&file, repair)` | `--repair` |
| `destroy` | `cmd_destroy(dry_run)` | `--dry-run` |
| `diff` | `cmd_diff(snap_a, snap_b, verbose)` | `--verbose` |
| `info` | `cmd_info()` | none |
| `init` | `cmd_init(&path)` | none |
| `plan` | `cmd_plan(&file)` | none |
| `status` | `cmd_status(verbose)` | `--verbose` |
| `update` | `cmd_update(config, inputs, dry_run)` | `--dry-run`, `--input` |

### Output Patterns

#### Semantic Symbols (diff.rs)
| Symbol | Meaning |
|--------|---------|
| `+` | Added |
| `-` | Removed |
| `~` | Updated |
| `=` | Unchanged |

#### Formatting Helpers
- `format_bytes(u64) -> String` - Human-readable sizes (status.rs:94-108)
- `truncate_hash(&str) -> &str` - 12-char hash truncation (diff.rs:270-272)
- `format_action()`, `format_exec()` - Action display (diff.rs:237-268)

#### Indentation Convention
- 2-space increments for nesting
- Blank line before new sections
- Section headers end with colon

### Tracing Configuration

**Current setup** (main.rs:80-89):
```rust
let level = if cli.verbose { Level::DEBUG } else { Level::INFO };

FmtSubscriber::builder()
    .with_max_level(level)
    .with_target(false)   // No module paths
    .without_time()       // No timestamps
    .init();
```

**Issue**: Timestamps explicitly disabled. Feature request asks for timestamps.

### Serialization Status

#### Types WITH Serialize (ready for JSON)
| Type | Location |
|------|----------|
| `Snapshot` | snapshot/types.rs:20 |
| `SnapshotMetadata` | snapshot/types.rs:72 |
| `SnapshotIndex` | snapshot/types.rs:111 |
| `Manifest` | manifest/types.rs:63 |
| `ObjectHash` | util/hash.rs:30 |
| `ContentHash` | util/hash.rs:58 |
| `BuildDef` | build/types.rs:182 |
| `BindDef` | bind/types.rs:235 |
| `Action` | action/types.rs:25 |
| `ExecOpts` | action/actions/exec.rs:35 |

#### Types NEEDING Serialize
| Type | Location | Complexity |
|------|----------|------------|
| `StateDiff` | snapshot/diff.rs:18 | Low |
| `ApplyResult` | execute/apply.rs:45 | Medium |
| `DestroyResult` | execute/apply.rs:153 | Low |
| `DagResult` | execute/types.rs:152 | High (contains ExecuteError) |
| `BuildResult` | execute/types.rs:116 | Low |
| `BindResult` | execute/types.rs:130 | Low |
| `DriftResult` | execute/types.rs:141 | Low |
| `BindCheckResult` | bind/types.rs:186 | Low |

**Challenge**: `DagResult` contains `ExecuteError` which includes `std::io::Error` (not serializable). Need to create a serializable error summary type.

### Color Library Recommendation

**Recommended**: `owo-colors` with `supports-colors` feature

**Rationale**:
- Zero-cost abstraction, zero allocations
- Native Windows 10+ ANSI support
- Built-in TTY detection
- Respects `NO_COLOR` and CI environments
- Active maintenance (2025)
- Used by major projects: Vector, Nextest, UV

**Implementation pattern**:
```rust
use owo_colors::{OwoColorize, Stream};

eprintln!("{}", "✓ Success".if_supports_color(Stream::Stderr, |t| t.green()));
eprintln!("{}", "✗ Error".if_supports_color(Stream::Stderr, |t| t.red()));
```

**Color scheme**:
| Type | Color | Symbol |
|------|-------|--------|
| Success | Green | ✓ |
| Error | Red | ✗ |
| Warning | Yellow | ⚠ |
| Info | Cyan | ℹ |

## Code References

- `crates/cli/src/main.rs:12-18` - CLI struct with global --verbose flag
- `crates/cli/src/main.rs:80-89` - Tracing subscriber configuration
- `crates/cli/src/cmd/diff.rs:94-129` - Summary diff output with semantic symbols
- `crates/cli/src/cmd/diff.rs:270-272` - Hash truncation helper
- `crates/cli/src/cmd/status.rs:94-108` - Bytes formatting helper
- `crates/cli/src/cmd/apply.rs:39-47` - Summary statistics pattern
- `crates/lib/src/execute/types.rs:152-175` - DagResult (needs Serialize)
- `crates/lib/src/snapshot/diff.rs:18-42` - StateDiff (needs Serialize)
- `Cargo.toml:25-27` - Workspace tracing dependencies

## Architecture Insights

### Output Mode Architecture

For `--json` support, recommend a dual-output pattern:

```rust
// cli/src/output.rs (new module)
pub enum OutputFormat {
    Human,
    Json,
}

pub struct Output {
    format: OutputFormat,
    color: bool,
}

impl Output {
    pub fn print_result<T: Serialize + HumanDisplay>(&self, result: &T) {
        match self.format {
            OutputFormat::Json => println!("{}", serde_json::to_string_pretty(result).unwrap()),
            OutputFormat::Human => result.human_display(self.color),
        }
    }
}
```

### Global Flag Strategy

Current state has a conflict:
- Global `-v/--verbose` → Sets tracing to DEBUG level
- Per-command `--verbose` → Shows detailed output

**Recommendation**: Rename global flag to `--debug/-d` to avoid confusion:
```rust
struct Cli {
    #[arg(short, long, global = true)]
    debug: bool,  // Renamed from verbose
    
    #[arg(long, value_enum, default_value = "auto", global = true)]
    color: ColorChoice,
    
    #[arg(long, global = true)]
    json: bool,
}
```

### Shared Output Module

Create `crates/cli/src/output/mod.rs` to consolidate:
- `format_bytes()`
- `truncate_hash()`
- `format_action()`
- Color helpers
- JSON serialization
- Output format detection

## Historical Context (from thoughts/)

- `thoughts/plans/diff-command.md` - Implemented `--verbose`, explicitly deferred color and JSON
- `thoughts/plans/sys-status-command.md` - Implemented `--verbose`, deferred color and JSON
- Both plans prioritized core functionality over formatting enhancements
- Consistent pattern: human-readable first, machine-readable later

## Implementation Recommendations

### Priority Order

1. **Create shared output module** - Consolidate scattered helpers
2. **Add `--json` flag and Serialize derives** - Highest value for tooling integration
3. **Add `owo-colors` and `--color` flag** - Cosmetic but improves UX
4. **Enable timestamps** - Simple change to tracing config
5. **Standardize summary statistics** - All commands should report counts + timing

### Effort Estimates

| Feature | Effort | Files Changed |
|---------|--------|---------------|
| Shared output module | Medium | New file + refactor 8 cmd files |
| `--json` flag + types | Medium | main.rs + ~6 type files |
| Color support | Low-Medium | Cargo.toml + all cmd files |
| Timestamps | Low | main.rs (1 line change) |
| Summary statistics | Low | Individual cmd files |

### Breaking Changes

- None expected if `--json` is opt-in
- Renaming `--verbose` to `--debug` would be breaking but is optional

## Open Questions

1. **JSON schema specification**: What exact fields should each command output?
2. **Exit codes**: Should different error types return different exit codes?
3. **Progress indicators**: Related UX concern not covered by ticket
4. **Terminal width**: Should long lines be truncated/wrapped?

## Related Research

No prior research documents exist in thoughts/research/.

This is the first formal research document for CLI improvements.
