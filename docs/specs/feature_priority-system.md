---
beads_id: syslua-ooc
type: feature
priority: 1
created: 2026-01-06T11:21:19-05:00
status: implemented
plan: thoughts/plans/priority-system.md
keywords:
  - priority system
  - conflict resolution
  - force before default after order
  - mergeable values
  - sys.priority
patterns:
  - Conflict detection in manifest merging
  - Priority tracking during config evaluation
  - Mergeable value combination strategy
  - Error messages with source locations
---

# Implement Priority-Based Conflict Resolution System

## Description

Implement a comprehensive priority-based conflict resolution system for syslua configuration. This enables deterministic handling of overlapping declarations across modules with explicit precedence control.

The system provides four helper functions (`force`, `before`, `default`, `after`) that assign priority levels to values. When multiple modules declare the same key (e.g., `EDITOR`), the system selects the winning value based on priority. For mergeable keys (e.g., `PATH`), values are combined and sorted by priority. Users can also specify custom priority values using `order()`.

## Context

From the architecture docs (`08-apply-flow.md`):

> **Key Design Principle:** Lua configuration is evaluated into a manifest first, conflicts are resolved using priorities, then a DAG-based system applies changes. This ensures:
>
> - Order of declarations in Lua does not affect the final result
> - Conflicts are detected and resolved deterministically
> - The system determines optimal execution order, not the user

**Current State**: Priority system is documented but not implemented. Conflicts would currently cause undefined behavior or require manual resolution.

**Business Impact**: Without a priority system, users cannot safely compose modules from multiple sources (community modules, personal dotfiles, team configs). This limits syslua's composability story and makes it difficult to adopt in large organizations.

## Requirements

### Functional Requirements

#### 1. Core Priority Helpers (MUST)

Implement four Lua helper functions with fixed priority values:

```lua
sys.priority.force(value) -- priority: 50 (highest)
sys.priority.before(value) -- priority: 500
sys.priority.default(value) -- priority: 1000 (implicit default)
sys.priority.after(value) -- priority: 1500 (lowest)
```

**Requirements:**

- Priority values are FIXED and never configurable (50/500/1000/1500 are documented constants)
- `default` is the implicit priority for regular declarations (no wrapper needed)
- All helpers return a wrapper that tracks the priority and source location
- Wrappers are opaque to user code (no direct access to priority value)

#### 2. Custom Priority Function (MUST)

Allow users to specify custom priority values for fine-grained control:

```lua
sys.priority.order(750, value) -- explicit priority value
```

**Requirements:**

- Accepts two arguments: priority value (number) and the value to wrap
- Priority value can be any positive integer (lower number = higher priority)
- Returns same wrapper type as built-in helpers
- Validates priority is a number (throws error otherwise)
- No minimum/maximum limits (users control their own range)

#### 3. Mergeable Value Declaration (MUST)

Allow modules to configure mergeable keys with explicit merge strategies:

```lua
-- In module definition
local M = {}

M.opts = {
  PATH = sys.priority.mergeable({
    separator = sys.os == 'windows' and ';' or ':', -- separator for string merges
  }),

  PACKAGES = sys.priority.mergeable(), -- no separator, defaults to array merge
}

return M
```

**Requirements:**

- `M.opts` is a module-level table where keys are binding names
- `sys.priority.mergeable()` returns a merge configuration object
- Merge configuration supports:
  - `separator`: string separator for 'string' type (optional)
- Mergeable keys combine all declarations and sort by priority
- String merges: join with separator
- Array merges: concatenate arrays in priority order
- Non-mergeable keys use singular value selection (lowest priority wins)

#### 4. Conflict Detection (MUST)

Detect and handle same-priority conflicts:

- Same priority + different values = **ERROR**
- Error must include:
  - Key name
  - Priority level (e.g., "default: 1000")
  - Source locations (file:line) for all conflicting declarations
  - Suggested resolutions (use force, before, after, order)
  - Built-in priorities for context

#### 5. Source Location Tracking (MUST)

Track file and line number for all priorityed declarations:

**Requirements:**

- Use `debug.getinfo()` or similar Lua introspection
- Store `(file, line)` in the priority wrapper
- Include in all error messages
- Persist to snapshot for debugging

#### 6. Internal Type System (MUST)

Implement Rust types for priority tracking:

```rust
/// A value with associated priority and source location.
/// Generic over V - call sites decide the concrete value type.
pub struct PriorityValue<V> {
    pub value: V,
    pub pvalue: u32,        // lower = higher priority
    pub source: SourceLoc,  // file:line for error reporting
}

impl<V> PriorityValue<V> {
    pub fn force(value: V, source: SourceLoc) -> Self {
        Self { value, pvalue: 50, source }
    }
    pub fn before(value: V, source: SourceLoc) -> Self {
        Self { value, pvalue: 500, source }
    }
    pub fn default(value: V, source: SourceLoc) -> Self {
        Self { value, pvalue: 1000, source }
    }
    pub fn after(value: V, source: SourceLoc) -> Self {
        Self { value, pvalue: 1500, source }
    }
    pub fn order(value: V, pvalue: u32, source: SourceLoc) -> Self {
        Self { value, pvalue, source }
    }
}

/// Merge behavior is determined by separator:
/// - Some(sep) = string merge, join values with separator
/// - None = array merge, concatenate in priority order
pub type MergeSeparator = Option<String>;
```

Modify manifest merging to incorporate priority resolution:

**In `crates/lib/src/execute/apply.rs` or a dedicated module:**

```rust
impl Manifest {
    pub fn merge_with_priorities<V>(
        declarations: Vec<PriorityValue<V>>,
        merge_configs: HashMap<String, MergeSeparator>,
    ) -> Result<Manifest, ConflictError>
    where
        V: Eq + Clone,
    {
        // Group by key
        // For each key:
        //   - Check for same-priority conflicts (same pvalue, different value)
        //   - Apply resolution (singular: lowest pvalue wins, or mergeable: combine sorted by pvalue)
        //   - Build final manifest
    }
}
```

**Requirements:**

- Conflict detection happens BEFORE DAG construction
- Manifest stores resolved values (no priority metadata in final manifest)
- Conflict errors abort apply before any execution
- Merge configs collected from all modules' `M.opts` tables

Produce clear, actionable error messages:

**Example Error Format:**

```
Error: Priority conflict in 'nginx.opts.port'

  Conflicting declarations at same priority level (default: 1000):

  File: /home/user/modules/base.lua:15
    nginx.setup({ port = sys.priority.default(8080) })

  File: /home/user/modules/work.lua:8
    nginx.setup({ port = sys.priority.default(9000) })

  Resolution options:
  1. Use sys.priority.force() to explicitly override
  2. Use sys.priority.before() or after() to adjust priority
  3. Use sys.priority.order() for custom priority values
  4. Remove one of the conflicting declarations

  Built-in priorities:
    force:   50
    before:  500
    default: 1000
    after:   1500
```

**Requirements:**

- ANSI color support for terminals
- Clear hierarchy (error → details → options)
- Suggested fixes are actionable
- Built-in priorities shown for context (not configured - they're fixed)

### Non-Functional Requirements

#### Performance

- Conflict detection must complete in < 100ms for 1000 declarations
- Mergeable value sorting must be O(n log n) where n = number of declarations
- No performance regression in config evaluation (target: < 10% overhead)
- Memory overhead: < 1KB per priorityed declaration

#### Error Quality

- All errors include file:line location
- Error messages are actionable (user knows what to do)
- Warnings in non-strict mode are comprehensive
- Error codes for programmatic handling

#### Testing Coverage

- Unit tests: 90%+ coverage of priority resolution logic
- Integration tests: Full apply flow with priorities
- Performance tests: Validate performance requirements
- Property-based tests: Conflict resolution invariants

#### Documentation

- Lua API documented in `lua/syslua/globals.d.ts`
- Rust types documented with rustdoc
- Architecture doc updated with implementation details
- User guide with examples and best practices

## Current State

**Architecture:** Designed in `docs/architecture/08-apply-flow.md` (Priority-Based Conflict Resolution section)

**Implementation Status:**

- ❌ Priority helpers not implemented
- ❌ Conflict resolution logic not implemented
- ❌ Mergeable value system not implemented
- ❌ Error formatting not implemented
- ❌ Testing infrastructure not implemented

**Code Locations:**

- Lua API: `crates/lib/src/lua/globals.rs` (needs priority module)
- Conflict resolution: New module `crates/lib/src/priority/` (proposed)
- Manifest merging: `crates/lib/src/execute/apply.rs` (needs modification)
- Error types: `crates/lib/src/priority/types.rs` (proposed)

## Desired State

**After Implementation:**

1. Module authors define options with default priorities:

```lua
-- syslua/programs/nginx.lua
local M = {}

M.opts = {
  port = sys.priority.default(8080),
  workers = sys.priority.default(4),
}

M.setup = function(opts)
  -- Merge user opts with module defaults, respecting priorities
  M.opts = sys.priority.merge(M.opts, opts or {})

  sys.build({ id = '__syslua_nginx_build', ... })
  sys.bind({ id = '__syslua_nginx_bind', ... })
end

return M
```

2. Users override with explicit precedence:

```lua
-- user-config.lua
return {
  setup = function()
    local nginx = require('syslua.programs.nginx')

    -- before(9090) wins over default(8080)
    nginx.setup({ port = sys.priority.before(9090) })

    -- Can call setup multiple times, priorities resolve correctly
    nginx.setup({ port = sys.priority.after(80) }) -- lowest priority, ignored
  end,
}

-- Result: port = 9090 (before > default > after)
```

3. Mergeable values for combining across calls:

```lua
-- Module with mergeable option
local M = {}

M.opts = {
  -- Mergeable with ':' separator means values combine in priority order
  paths = sys.priority.mergeable(':'),
}

M.setup = function(opts)
  M.opts = sys.priority.merge(M.opts, opts or {})
  ...
end

-- User config
M.setup({ paths = sys.priority.default('/usr/bin') })
M.setup({ paths = sys.priority.before('/opt/bin') })
M.setup({ paths = sys.priority.after('/usr/local/bin') })

-- Result: paths = "/opt/bin:/usr/bin:/usr/local/bin" (sorted by priority)
```

3. Conflicts are detected with clear error messages:

```
Error: Priority conflict in bind 'editor-config'
  ...
  Use sys.priority.force() to explicitly override
```

4. Priority system is well-tested and documented:

- Unit tests for all conflict scenarios
- Integration tests for full apply flow
- Performance tests meet requirements
- User guide with examples

5. Codebase is organized cleanly:

- `crates/lib/src/priority/` module for priority logic
- Integration with `manifest/` and `execute/` modules
- No circular dependencies

## Research Context

### Keywords to Search

- `mlua error tracking` - How to capture Lua stack traces in Rust
- `BTreeMap deterministic ordering` - Confirm stable sorting behavior
- `thiserror derive` - Best practices for error enums
- `property-based testing Rust` - Tools for conflict resolution invariants

### Patterns to Investigate

- **Nix priority system**: How Nix handles `lib.mkForce`, `lib.mkDefault`
- **Rust config libraries**: How config-rs handles merge behavior
- **Lua module systems**: How to track source locations in Lua
- **Conflict detection algorithms**: Efficient O(n) grouping strategies

### External References

- [NixOS mkForce/mkDefault implementation](https://github.com/NixOS/nixpkgs/tree/master/lib)
- [config-rs merge behavior](https://github.com/mehcode/config-rs)
- [mlua traceback handling](https://github.com/mlua-rs/mlua)

### Key Decisions Made

1. **Fixed helper priorities**: 50/500/1000/1500 are documented constants and NEVER configurable
2. **Custom priorities**: Users must use `sys.priority.order(value, priority)` for custom priority values
3. **Mergeable declaration**: Explicit `sys.priority.mergeable()` required (not inferred)
4. **Conflict detection**: Happens before DAG construction (manifest merge phase)
5. **Strict mode default**: Errors abort execution by default (safer for users)
6. **Source location**: Use Lua `debug.getinfo()` to capture file:line at declaration time

## Success Criteria

### Automated Tests

- [ ] Unit tests for all four helpers (`force`, `before`, `default`, `after`)
- [ ] Unit tests for `order()` function with various priority values
- [ ] Unit tests for same-priority conflict detection (strict mode)
- [ ] Unit tests for warning mode (non-strict)
- [ ] Unit tests for singular value selection (lowest priority wins)
- [ ] Unit tests for mergeable string values (with separators)
- [ ] Unit tests for mergeable array values (concatenation)
- [ ] Unit tests for M.opts parsing and merge configuration
- [ ] Integration tests for multi-module configs with priorities
- [ ] Integration tests for rollback behavior with priorityed values
- [ ] Performance tests: 1000 declarations in < 100ms
- [ ] Property-based tests: conflict resolution invariants hold

### Manual Verification

- [ ] Test with real-world config example (editor + tool preferences)
- [ ] Verify error messages include file:line locations
- [ ] Verify M.opts parsing handles merge configurations correctly
- [ ] Verify mergeable PATH combines correctly with `:` separator
- [ ] Verify mergeable arrays concatenate in priority order
- [ ] Verify snapshot tracks priority metadata
- [ ] Verify performance regression < 10% in config evaluation
- [ ] Verify documentation is complete and accurate

## Implementation Notes

### Phased Approach

**Phase 1: Core API** (Week 1)

- Implement priority wrapper types in Rust
- Add Lua helpers (`force`, `default`, `before`, `after`)
- Add `order()` function for custom priorities
- Add source location tracking
- Basic unit tests

**Phase 2: Conflict Detection** (Week 1-2)

- Implement conflict detection logic
- Add error formatting with source locations
- Implement strict/warning modes
- Integration tests

**Phase 3: Mergeable Values** (Week 2)

- Implement M.opts parsing for merge configuration
- Add merge strategy (singular vs mergeable)
- Implement string merge with separator
- Implement array merge (concatenation)
- Integration tests

**Phase 4: Manifest Integration** (Week 2-3)

- Integrate conflict resolution into manifest merging
- Update apply flow to check for conflicts
- Add rollback support for priority state
- End-to-end integration tests

**Phase 5: Testing & Documentation** (Week 3)

- Performance tests
- Property-based tests
- Update architecture docs
- Write user guide
- Add examples

### Technical Risks

1. **Source location accuracy**: `debug.getinfo()` may not work reliably in all Lua execution contexts
   - **Mitigation**: Fallback to module-level tracking if line numbers unavailable
   - **Test**: Validate on all supported platforms (Linux, macOS, Windows)

2. **Performance impact**: Tracking priorities for every declaration may slow evaluation
   - **Mitigation**: Profile and optimize; consider lazy tracking
   - **Test**: Benchmark with large configs (1000+ declarations)

3. **Merge configuration conflicts**: Modules may have conflicting merge configurations for same key
   - **Mitigation**: First config wins (document behavior) or throw error
   - **Test**: Add tests for multiple modules configuring same key

### Dependencies

- Existing `manifest/` module
- Existing `lua/` module (mlua integration)
- Existing error handling (`thiserror`)
- New `priority/` module (to be created)

### Open Questions

1. **Merge configuration conflicts**: When multiple modules configure the same key differently (e.g., PATH with ':' vs ';'), which wins?
   - Current: First config wins (documented behavior)
   - Alternative: Throw error (safer, but may be too strict)
2. **Table merge type**: Should we add 'table' type for deep/shallow merge of nested structures? (deferred)
3. **Priority inheritance**: Should priority cascade through dependencies? (deferred)
4. **Order value range**: What are valid ranges for `sys.priority.order()`?
   - Current: No minimum/maximum (user-controlled)
   - Should we reserve 0-49 for future built-in helpers?
5. **Default separator**: Should string merge default to `:` or platform-specific (`:` on Unix, `;` on Windows)?

### Follow-up Tickets

- [ ] Add table merge type with deep/shallow merge options for nested structures
- [ ] Handle merge configuration conflicts (error or first-wins?)
- [ ] Add priority visualization/debug tools (`sys priority inspect`)
- [ ] Add priority migration guide for existing configs
- [ ] Consider platform-specific default separators (Unix `:`, Windows `;`)
