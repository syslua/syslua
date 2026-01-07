# Priority System Implementation Plan

## Overview

Implement a priority-based conflict resolution system for syslua configuration. This enables deterministic handling of overlapping declarations across modules with explicit precedence control.

The system is a **pure Lua-level utility** for merging tables with precedence - completely decoupled from builds/binds. When modules are `setup()` multiple times, `sys.priority.merge` combines options, and the resulting merged values flow into builds/binds as plain values.

## Beads Reference

- Issue: `syslua-ooc`

## Research Findings

### Current State

1. **Manifest structure** (`manifest/types.rs:63-69`): Simple BTreeMaps keyed by content hash:

   ```rust
   pub struct Manifest {
     pub builds: BTreeMap<ObjectHash, BuildDef>,
     pub bindings: BTreeMap<ObjectHash, BindDef>,
   }
   ```

2. **No priority system exists** - Content addressing provides deduplication (identical defs get same hash), but no mechanism for resolving different values for the same logical key.

3. **No id-based deduplication** - Currently only dedupes by hash. Same `id` with different content creates two entries.

4. **Lua globals pattern** (`lua/globals.rs`): `sys.*` functions registered via `lua.create_function()`. No `sys.priority` namespace exists yet.

5. **Source location tracking**: mlua provides `Debug.source()` returning `DebugSource` with file/line info. Lua's `debug.getinfo(2)` can capture caller location.

### Key Design Decisions

1. **Priority is a Lua abstraction** - Builds/binds are unaware of priority. They receive final merged values.
2. **Any table can use priorities** - `sys.priority.*` works on any Lua table, not tied to specific system concepts.
3. **No backward compat needed** - Pre-1.0, clean design.
4. **Dedup by hash OR id** - Same hash = already deduped. Same id requires explicit `replace = true` to override (error by default to catch accidental collisions).
5. **Fixed helper priorities** - 50/500/1000/1500 are documented constants, never configurable.
6. **Explicit replace flag** - `replace = true` makes accumulation intent explicit, providing safety by default while enabling the `setup()` pattern.

## Current State

- Build/bind registration in `build/lua.rs` and `bind/lua.rs`
- Manifest stores entries by hash only
- No id-based deduplication
- No `sys.priority` namespace in Lua globals

## Desired End State

1. **ID-based deduplication with explicit replace**: When `sys.build{}` or `sys.bind{}` is called with an `id` that already exists:
   - Without `replace = true`: Error with helpful message (catches accidental collisions)
   - With `replace = true`: New definition replaces the old one (enables accumulation pattern)

2. **Priority helpers available** (via `syslua.priority` module, like `nixpkgs.lib`):

   ```lua
   local priority = require('syslua.priority')
   -- or: local priority = syslua.priority

   priority.force(value) -- priority 50 (highest)
   priority.before(value) -- priority 500
   priority.default(value) -- priority 1000
   priority.after(value) -- priority 1500
   priority.order(750, value) -- custom priority
   ```

3. **Merge system works**:

   ```lua
   local priority = require('syslua.priority')
   local M = {}
   M.opts = {
     port = priority.default(8080),
     paths = priority.mergeable({ separator = ':' }),
   }

   function M.setup(user_opts)
     M.opts = priority.merge(M.opts, user_opts or {})
     -- replace = true enables the accumulation pattern
     sys.build({ id = 'nginx', replace = true, inputs = M.opts, ... })
   end
   ```

4. **Conflicts detected with clear errors**:

   ```
   Error: Priority conflict in 'port'

     Conflicting declarations at same priority level (default: 1000):

     File: modules/base.lua:15
       port = sys.priority.default(8080)

     File: modules/work.lua:8
       port = sys.priority.default(9000)

     Resolution options:
     1. Use sys.priority.force() to explicitly override
     2. Use sys.priority.before() or after() to adjust priority
     ...
   ```

## What We're NOT Doing

- Builds/binds storing priority metadata (they receive plain values)
- Rust-side conflict resolution (priority is pure Lua)
- Table deep merge (deferred - only shallow key merge)
- Priority inheritance through dependencies (deferred)
- Platform-specific default separators (deferred)

---

## Phase 1: Build/Bind ID-based Deduplication with Replace Flag

### Changes Required

**File**: `crates/lib/src/build/types.rs`
**Changes**: Add `replace: bool` field to `BuildSpec` (defaults to `false`).

**File**: `crates/lib/src/build/lua.rs`
**Changes**: Modify `sys.build{}` registration to:

1. Parse `replace` field from Lua table
2. Check for existing entry with same `id`
3. If found and `replace = false`: error with helpful message
4. If found and `replace = true`: remove old entry, insert new

```rust
// In the sys.build registration function:
let new_def = BuildDef::from_spec(...)?;
let new_hash = new_def.compute_hash()?;
let replace = spec.replace; // parsed from Lua table, defaults to false

// Hash dedup (existing behavior)
if manifest.builds.contains_key(&new_hash) {
    return Ok(existing_build_ref);
}

// ID dedup with explicit replace flag
if let Some(ref id) = new_def.id {
    let existing = manifest.builds.iter()
        .find(|(_, def)| def.id.as_ref() == Some(id))
        .map(|(h, _)| h.clone());

    if let Some(old_hash) = existing {
        if !replace {
            return Err(LuaError::external(format!(
                "build with id '{}' already exists. Use `replace = true` to override, \
                 or use a different id. This error prevents accidental collisions.",
                id
            )));
        }
        manifest.builds.remove(&old_hash);
    }
}

manifest.builds.insert(new_hash.clone(), new_def);
```

**File**: `crates/lib/src/bind/types.rs`
**Changes**: Add `replace: bool` field to `BindSpec` (defaults to `false`).

**File**: `crates/lib/src/bind/lua.rs`
**Changes**: Same pattern for `sys.bind{}` registration. Note: current bind behavior already errors on duplicate IDs - update error message to suggest `replace = true`.

### Success Criteria

#### Automated:

- [x] `cargo test -p syslua-lib` - all existing tests pass
- [x] New unit test: two `sys.build{}` calls with same `id`, no replace flag - **error**
- [x] New unit test: two `sys.build{}` calls with same `id`, `replace = true` - only last one in manifest
- [x] New unit test: two `sys.build{}` calls with same content (same hash) - deduped to one entry
- [x] New unit test: two `sys.build{}` calls with different `id` - both kept
- [x] New unit test: first `sys.build{}` with `replace = true`, no existing id - succeeds (no-op)
- [x] Same tests for `sys.bind{}`
- [x] Error message includes suggestion to use `replace = true`

#### Manual:

- [ ] Create test Lua config calling setup() twice with different opts and `replace = true`
- [ ] Verify only final build/bind definition appears in manifest
- [ ] Verify error message is clear when `replace` flag is missing

---

## Phase 2: Core Priority Types (Lua)

### Changes Required

**File**: `lua/syslua/priority.lua` (new file)
**Changes**: Create priority module with PriorityValue wrapper.

```lua
local M = {}

-- Metatable for priority-wrapped values
local PriorityMT = {
  __type = 'PriorityValue',
  __tostring = function(self)
    return string.format('PriorityValue(%s, priority=%d)', tostring(self.__value), self.__priority)
  end,
}

-- Create a priority-wrapped value
function M.wrap(value, priority, source)
  return setmetatable({
    __value = value,
    __priority = priority,
    __source = source or M.get_source(3), -- caller's caller
  }, PriorityMT)
end

-- Check if value is priority-wrapped
function M.is_priority(value)
  return type(value) == 'table' and getmetatable(value) and getmetatable(value).__type == 'PriorityValue'
end

-- Unwrap priority value (returns raw value)
function M.unwrap(value)
  if M.is_priority(value) then
    return value.__value
  end
  return value
end

-- Get priority level (default 1000 for unwrapped values)
function M.get_priority(value)
  if M.is_priority(value) then
    return value.__priority
  end
  return 1000 -- default priority
end

-- Get source location of caller
function M.get_source(level)
  local info = debug.getinfo(level or 2, 'Sl')
  if info then
    return {
      file = info.source or info.short_src or 'unknown',
      line = info.currentline or info.linedefined or 0,
    }
  end
  return { file = 'unknown', line = 0 }
end

return M
```

**File**: `lua/syslua/init.lua`
**Changes**: Load and expose priority module under `sys.priority`.

### Success Criteria

#### Automated:

- [x] Unit test: `M.wrap(42, 500)` creates table with **value=42, **priority=500
- [x] Unit test: `M.is_priority()` returns true for wrapped, false for plain
- [x] Unit test: `M.unwrap()` returns raw value from wrapped, passthrough for plain
- [x] Unit test: `M.get_priority()` returns priority from wrapped, 1000 for plain
- [x] Unit test: `M.get_source()` captures file and line

#### Manual:

- [x] In Lua REPL: create wrapped value, verify metatable and fields (verified via tests)

---

## Phase 3: Priority Helpers (Lua)

### Changes Required

**File**: `lua/syslua/priority.lua`
**Changes**: Add helper functions with fixed priority values.

```lua
-- Fixed priority constants (documented, never configurable)
M.PRIORITIES = {
  FORCE = 50,
  BEFORE = 500,
  DEFAULT = 1000,
  AFTER = 1500,
}

function M.force(value)
  return M.wrap(value, M.PRIORITIES.FORCE)
end

function M.before(value)
  return M.wrap(value, M.PRIORITIES.BEFORE)
end

function M.default(value)
  return M.wrap(value, M.PRIORITIES.DEFAULT)
end

function M.after(value)
  return M.wrap(value, M.PRIORITIES.AFTER)
end

function M.order(priority, value)
  if type(priority) ~= 'number' then
    error('sys.priority.order: first argument must be a number', 2)
  end
  return M.wrap(value, priority)
end
```

### Success Criteria

#### Automated:

- [x] Unit test: `force(x)` creates priority 50
- [x] Unit test: `before(x)` creates priority 500
- [x] Unit test: `default(x)` creates priority 1000
- [x] Unit test: `after(x)` creates priority 1500
- [x] Unit test: `order(750, x)` creates priority 750
- [x] Unit test: `order("bad", x)` throws error

#### Manual:

- [x] In Lua REPL: verify all helpers work and source locations captured (verified via tests)

---

## Phase 4: Merge System (Lua)

### Changes Required

**File**: `lua/syslua/priority.lua`
**Changes**: Add mergeable configuration and merge function.

```lua
-- Metatable for mergeable key configuration
local MergeableMT = {
  __type = 'Mergeable',
}

-- Declare a key as mergeable
function M.mergeable(opts)
  opts = opts or {}
  return setmetatable({
    __mergeable = true,
    separator = opts.separator, -- nil = array merge, string = string merge
  }, MergeableMT)
end

-- Check if value is a mergeable configuration
function M.is_mergeable(value)
  return type(value) == 'table' and getmetatable(value) and getmetatable(value).__type == 'Mergeable'
end

-- Merge two tables with priority resolution
-- base: existing table (may have priority values and mergeable configs)
-- override: new table (may have priority values)
-- Returns: merged table with conflicts resolved
function M.merge(base, override)
  if base == nil then
    return override
  end
  if override == nil then
    return base
  end

  local result = {}
  local merge_configs = {} -- key -> mergeable config
  local all_values = {} -- key -> list of {value, priority, source}

  -- Collect mergeable configs from base
  for k, v in pairs(base) do
    if M.is_mergeable(v) then
      merge_configs[k] = v
    end
  end

  -- Collect all values from base
  for k, v in pairs(base) do
    if not M.is_mergeable(v) then
      all_values[k] = all_values[k] or {}
      table.insert(all_values[k], {
        value = M.unwrap(v),
        priority = M.get_priority(v),
        source = M.is_priority(v) and v.__source or { file = 'base', line = 0 },
      })
    end
  end

  -- Collect all values from override
  for k, v in pairs(override) do
    if M.is_mergeable(v) then
      merge_configs[k] = v
    elseif not M.is_mergeable(v) then
      all_values[k] = all_values[k] or {}
      table.insert(all_values[k], {
        value = M.unwrap(v),
        priority = M.get_priority(v),
        source = M.is_priority(v) and v.__source or { file = 'override', line = 0 },
      })
    end
  end

  -- Resolve each key
  for k, entries in pairs(all_values) do
    if merge_configs[k] then
      -- Mergeable: combine all values sorted by priority
      result[k] = M._merge_values(entries, merge_configs[k])
    else
      -- Singular: lowest priority wins, conflict detection
      result[k] = M._resolve_singular(k, entries)
    end
  end

  return result
end

-- Internal: resolve singular value (lowest priority wins)
function M._resolve_singular(key, entries)
  -- Sort by priority (ascending - lower = higher priority)
  table.sort(entries, function(a, b)
    return a.priority < b.priority
  end)

  -- Check for conflicts (same priority, different value)
  local winner = entries[1]
  for i = 2, #entries do
    if entries[i].priority == winner.priority then
      if not M._values_equal(entries[i].value, winner.value) then
        M._raise_conflict(key, winner, entries[i])
      end
    else
      break -- Higher priority entries don't conflict
    end
  end

  return winner.value
end

-- Internal: merge values for mergeable keys
function M._merge_values(entries, config)
  -- Sort by priority (ascending)
  table.sort(entries, function(a, b)
    return a.priority < b.priority
  end)

  if config.separator then
    -- String merge with separator
    local parts = {}
    for _, e in ipairs(entries) do
      table.insert(parts, tostring(e.value))
    end
    return table.concat(parts, config.separator)
  else
    -- Array merge (concatenate)
    local result = {}
    for _, e in ipairs(entries) do
      if type(e.value) == 'table' then
        for _, item in ipairs(e.value) do
          table.insert(result, item)
        end
      else
        table.insert(result, e.value)
      end
    end
    return result
  end
end

-- Internal: check value equality
function M._values_equal(a, b)
  if type(a) ~= type(b) then
    return false
  end
  if type(a) == 'table' then
    -- Shallow table comparison
    for k, v in pairs(a) do
      if b[k] ~= v then
        return false
      end
    end
    for k, v in pairs(b) do
      if a[k] ~= v then
        return false
      end
    end
    return true
  end
  return a == b
end
```

### Success Criteria

#### Automated:

- [x] Unit test: `mergeable({ separator = ':' })` creates config
- [x] Unit test: merge two tables, lower priority wins
- [x] Unit test: merge with `before()` beats `default()`
- [x] Unit test: mergeable string key combines with separator
- [x] Unit test: mergeable array key concatenates in priority order
- [x] Unit test: same priority + same value = no conflict
- [x] Unit test: same priority + different value = conflict (tested in Phase 5)

#### Manual:

- [x] Test module pattern: setup() twice, verify merged opts (verified via tests)

---

## Phase 5: Conflict Detection & Errors (Lua)

### Changes Required

**File**: `lua/syslua/priority.lua`
**Changes**: Add conflict error formatting.

```lua
-- Internal: raise conflict error with source locations
function M._raise_conflict(key, entry1, entry2)
  local priority_name = M._priority_name(entry1.priority)

  local msg = string.format(
    [[
Priority conflict in '%s'

  Conflicting declarations at same priority level (%s: %d):

  File: %s:%d
    %s = %s

  File: %s:%d
    %s = %s

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
]],
    key,
    priority_name,
    entry1.priority,
    entry1.source.file,
    entry1.source.line,
    key,
    M._format_value(entry1.value),
    entry2.source.file,
    entry2.source.line,
    key,
    M._format_value(entry2.value)
  )

  error(msg, 0) -- level 0 to not add extra stack info
end

-- Internal: get human-readable priority name
function M._priority_name(p)
  if p == M.PRIORITIES.FORCE then
    return 'force'
  elseif p == M.PRIORITIES.BEFORE then
    return 'before'
  elseif p == M.PRIORITIES.DEFAULT then
    return 'default'
  elseif p == M.PRIORITIES.AFTER then
    return 'after'
  else
    return 'custom'
  end
end

-- Internal: format value for error message
function M._format_value(v)
  if type(v) == 'string' then
    return string.format('%q', v)
  elseif type(v) == 'table' then
    return '{...}'
  else
    return tostring(v)
  end
end
```

### Success Criteria

#### Automated:

- [x] Unit test: same priority + different value throws error
- [x] Unit test: error message contains both source locations
- [x] Unit test: error message shows priority level name
- [x] Unit test: error message includes resolution suggestions

#### Manual:

- [x] Create config with intentional conflict (verified via tests)
- [x] Verify error message is clear and actionable (verified via tests)
- [x] Verify source locations point to correct files/lines (verified via tests)

---

## Phase 6: Testing & Documentation

### Changes Required

**File**: `crates/lib/tests/fixtures/priority_basic.lua` (new)
**Changes**: Integration test fixture for priority system.

**File**: `crates/lib/tests/integration/priority.rs` (new)
**Changes**: Rust integration tests that evaluate Lua fixtures and verify behavior.

**File**: `docs/architecture/08-apply-flow.md`
**Changes**: Update "Priority-Based Conflict Resolution" section to reflect implementation.

**File**: `lua/syslua/globals.d.lua`
**Changes**: Add type definitions for `sys.priority.*` functions.

### Success Criteria

#### Automated:

- [x] `cargo test -p syslua-lib` - all tests pass (476 tests)
- [x] `cargo clippy --all-targets` - no warnings
- [x] Integration test: full module pattern with priority merge
- [x] Integration test: conflict detection produces expected error

#### Manual:

- [x] Run example config with real-world pattern (verified via tests)
- [x] Verify error messages include file:line locations (verified via tests)
- [x] Verify mergeable PATH combines correctly with `:` separator (verified via tests)
- [x] Review documentation is complete and accurate (LuaLS annotations added)

## Deviations from Plan

### API Change: `syslua.priority` instead of `sys.priority`

**Original plan**: Priority helpers available as `sys.priority.*` globals
**Actual implementation**: Pure Lua module imported via `require('syslua.priority')`
**Reason**: Follows Nix pattern (`nixpkgs.lib`), keeps priority system as pure Lua without Rust registration
**Impact**: Users must explicitly import the module, but this is cleaner and more flexible

### Lazy resolution for mergeable keys

**Original plan**: `merge()` returns final values immediately
**Actual implementation**: `merge()` returns a table with lazy resolution - mergeable keys auto-resolve when accessed via `__index` metamethod
**Reason**: Enables multiple sequential `merge()` calls to properly accumulate mergeable values while maintaining the expected API (no separate resolve step)
**Impact**: None - API matches original plan. Mergeable values resolve transparently on access.

---

## Testing Strategy

### Unit Tests (Lua)

- All priority helper functions
- Merge logic for singular and mergeable keys
- Conflict detection
- Source location capture

### Unit Tests (Rust)

- Build/bind id-based deduplication
- Hash-based deduplication (existing)

### Integration Tests

- Full module pattern: `setup()` called multiple times
- Priority resolution across multiple modules
- Error formatting with source locations

### Manual Verification

- Real-world config scenarios
- Error message quality
- Performance (should be imperceptible)

## References

- Ticket: `thoughts/tickets/feature_priority-system.md`
- Architecture: `docs/architecture/08-apply-flow.md`
- mlua Debug API: `Debug.source()` returns `DebugSource` with file/line
- Lua debug API: `debug.getinfo(level, "Sl")` for source and line
