# ctx:script() Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add `ctx:script()` method to BuildCtx and BindCtx that writes a script file and executes it.

**Architecture:** Lua-only implementation using `sys.register_build_ctx_method()` and `sys.register_bind_ctx_method()` in `init.lua`. Composes existing `ctx:exec()` calls for file writing and script execution.

**Tech Stack:** Lua, existing syslua ctx method registration system

**Spec:** [2026-01-09-ctx-script-design.md](../specs/2026-01-09-ctx-script-design.md)

---

## Context Analysis

### Existing Patterns to Follow

| Pattern | Source | Application |
|---------|--------|-------------|
| Ctx method registration | `init.lua:4-60` | `sys.register_build_ctx_method()` pattern |
| Platform branching | `init.lua:13-59` | `if sys.os == 'windows'` for platform-specific logic |
| Script generation | `init.lua:20-58` | String formatting for shell/cmd scripts |
| File writing via exec | `init.lua:23-35`, `init.lua:45-58` | Using shell commands to write files |

### Key Constraints

1. **No Rust changes** - Pure Lua implementation using existing primitives
2. **Hermetic execution** - `ctx:exec()` runs with `PATH=/path-not-set`, must use full paths
3. **Cross-platform** - Must work on Unix (shell/bash) and Windows (powershell/cmd)
4. **Counter state** - Need to track script count per-context for default naming

---

## Task 1: Add Script Method Implementation

**Files:**

- Modify: `init.lua:152` (before final `end` of `setup` function)

**Step 1: Write the script implementation**

Add the following before the final `end` in the `setup` function:

```lua
  -- Script method implementation (shared by build and bind contexts)
  local function script_impl(ctx, format, content, opts)
    opts = opts or {}

    -- Track script count for default naming (store on ctx table)
    ctx._script_count = (ctx._script_count or 0) + 1
    local name = opts.name or ('script_' .. (ctx._script_count - 1))

    -- Determine extension and interpreter based on format
    local ext, bin, args_prefix
    if format == 'shell' then
      ext = '.sh'
      bin = '/bin/sh'
      args_prefix = {}
    elseif format == 'bash' then
      ext = '.bash'
      bin = '/bin/bash'
      args_prefix = {}
    elseif format == 'powershell' then
      ext = '.ps1'
      bin = 'powershell.exe'
      args_prefix = { '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File' }
    elseif format == 'cmd' then
      ext = '.cmd'
      bin = 'cmd.exe'
      args_prefix = { '/c' }
    else
      error("script() format must be 'shell', 'bash', 'powershell', or 'cmd', got: " .. tostring(format))
    end

    local script_path = sys.path.join(ctx.out, 'tmp', name .. ext)

    -- Write script file (platform-specific)
    if format == 'shell' or format == 'bash' then
      -- Unix: use sh to mkdir, write file, chmod +x
      ctx:exec({
        bin = '/bin/sh',
        args = {
          '-c',
          string.format(
            'mkdir -p "%s" && cat > "%s" << \'SYSLUA_SCRIPT_EOF\'\n%s\nSYSLUA_SCRIPT_EOF\nchmod +x "%s"',
            sys.path.join(ctx.out, 'tmp'),
            script_path,
            content,
            script_path
          ),
        },
      })
    else
      -- Windows: use powershell to create directory and write file
      -- Escape single quotes in content by doubling them for PowerShell here-string
      local escaped_content = content:gsub("'@", "' @"):gsub("@'", "@ '")
      ctx:exec({
        bin = 'powershell.exe',
        args = {
          '-NoProfile',
          '-Command',
          string.format(
            "New-Item -ItemType Directory -Force -Path '%s' | Out-Null; @'\n%s\n'@ | Set-Content -Path '%s' -Encoding UTF8",
            sys.path.join(ctx.out, 'tmp'),
            escaped_content,
            script_path
          ),
        },
      })
    end

    -- Build execution args
    local exec_args = {}
    for _, v in ipairs(args_prefix) do
      table.insert(exec_args, v)
    end
    table.insert(exec_args, script_path)

    -- Execute script and capture stdout
    local stdout = ctx:exec({ bin = bin, args = exec_args })

    return {
      stdout = stdout,
      path = script_path,
    }
  end

  sys.register_build_ctx_method('script', script_impl)
  sys.register_bind_ctx_method('script', script_impl)
```

**Step 2: Verify syntax**

Run: `lua -c init.lua` (or let the test in next task validate)

**Step 3: Commit**

```bash
git add init.lua
git commit -m "feat: add ctx:script() method for builds and binds"
```

---

## Task 2: Add Unit Tests for Shell Format

**Files:**

- Create: `tests/integration/script_method_test.lua` (or add to existing test file if one exists for init.lua)

First, check if there's an existing test pattern:

**Step 1: Find existing test location**

Run: `ls tests/` to identify test structure.

If no Lua integration tests exist, we'll test via a minimal `sys.build` that uses `script()`.

**Step 2: Create test fixture**

Create `tests/fixtures/script_method.lua`:

```lua
return {
  inputs = {},
  setup = function(_inputs)
    -- Test 1: Basic shell script
    sys.build({
      id = 'test-script-shell',
      create = function(_inputs, ctx)
        local result = ctx:script('shell', [[
echo "hello from shell"
]])
        return {
          out = ctx.out,
          stdout = result.stdout,
          script_path = result.path,
        }
      end,
    })

    -- Test 2: Named script
    sys.build({
      id = 'test-script-named',
      create = function(_inputs, ctx)
        local result = ctx:script('shell', [[
echo "named script"
]], { name = 'my-script' })
        return {
          out = ctx.out,
          stdout = result.stdout,
          script_path = result.path,
        }
      end,
    })

    -- Test 3: Multiple scripts (counter test)
    sys.build({
      id = 'test-script-counter',
      create = function(_inputs, ctx)
        local r1 = ctx:script('shell', [[echo "first"]])
        local r2 = ctx:script('shell', [[echo "second"]])
        return {
          out = ctx.out,
          path1 = r1.path,
          path2 = r2.path,
        }
      end,
    })

    -- Test 4: Bash format
    sys.build({
      id = 'test-script-bash',
      create = function(_inputs, ctx)
        local result = ctx:script('bash', [[
declare -a arr=("hello" "world")
echo "${arr[@]}"
]])
        return {
          out = ctx.out,
          stdout = result.stdout,
        }
      end,
    })
  end,
}
```

**Step 3: Run tests**

Run: `cargo test -p syslua-lib script` (adjust based on actual test runner)

Or manually test with: `cargo run -- eval tests/fixtures/script_method.lua`

**Step 4: Commit**

```bash
git add tests/fixtures/script_method.lua
git commit -m "test: add fixtures for ctx:script() method"
```

---

## Task 3: Add Integration Test for Windows Formats

**Files:**

- Modify: `tests/fixtures/script_method.lua`

**Step 1: Add Windows format tests**

Append to `tests/fixtures/script_method.lua` inside `setup`:

```lua
    -- Test 5: PowerShell format (Windows)
    if sys.os == 'windows' then
      sys.build({
        id = 'test-script-powershell',
        create = function(_inputs, ctx)
          local result = ctx:script('powershell', [[
Write-Output "hello from powershell"
]])
          return {
            out = ctx.out,
            stdout = result.stdout,
            script_path = result.path,
          }
        end,
      })

      -- Test 6: Cmd format (Windows)
      sys.build({
        id = 'test-script-cmd',
        create = function(_inputs, ctx)
          local result = ctx:script('cmd', [[
@echo off
echo hello from cmd
]])
          return {
            out = ctx.out,
            stdout = result.stdout,
            script_path = result.path,
          }
        end,
      })
    end
```

**Step 2: Run tests on Windows**

Run: `cargo test -p syslua-lib script` (on Windows machine or CI)

**Step 3: Commit**

```bash
git add tests/fixtures/script_method.lua
git commit -m "test: add Windows format tests for ctx:script()"
```

---

## Task 4: Add Error Handling Test

**Files:**

- Modify: `tests/fixtures/script_method.lua`

**Step 1: Add invalid format test**

Append to `tests/fixtures/script_method.lua` inside `setup`:

```lua
    -- Test 7: Invalid format should error
    -- This build intentionally errors - test should verify error message
    -- sys.build({
    --   id = 'test-script-invalid-format',
    --   create = function(_inputs, ctx)
    --     ctx:script('invalid', [[echo "bad"]])
    --     return { out = ctx.out }
    --   end,
    -- })
```

Note: Uncomment and wrap in pcall in Rust test to verify error behavior.

**Step 2: Write Rust integration test**

Create or modify `tests/integration/script_method.rs`:

```rust
#[test]
fn script_method_invalid_format_errors() {
    let lua_code = r#"
        sys.build({
            id = 'test-invalid',
            create = function(_, ctx)
                ctx:script('invalid', [[echo "bad"]])
                return { out = ctx.out }
            end,
        })
    "#;
    
    let result = eval_lua(lua_code);
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("format must be"));
}
```

**Step 3: Run test**

Run: `cargo test script_method_invalid`

**Step 4: Commit**

```bash
git add tests/
git commit -m "test: add error handling tests for ctx:script()"
```

---

## Task 5: Update Documentation

**Files:**

- Modify: `docs/architecture/04-lua-api.md`

**Step 1: Add script method to BuildCtx Methods table**

Find the `### BuildCtx Methods` section and add:

```markdown
| `ctx:script(format, content, opts?)` | Write and execute a script file | `{ stdout: string, path: string }` |
```

**Step 2: Add script method to BindCtx Methods table**

Find the `### BindCtx Methods` section and add:

```markdown
| `ctx:script(format, content, opts?)` | Write and execute a script file | `{ stdout: string, path: string }` |
```

**Step 3: Add script method section**

Add after the existing ctx methods documentation:

```markdown
### Script Method

The `ctx:script()` method writes a script file to `$out/tmp/` and executes it. This provides a cleaner API for multi-line scripts compared to embedding them in `ctx:exec()` calls.

```lua
ctx:script(format, content, opts?) -> { stdout: string, path: string }
```

**Formats:**

- `'shell'` - POSIX shell (`/bin/sh`)
- `'bash'` - Bash (`/bin/bash`)
- `'powershell'` - PowerShell (`powershell.exe`)
- `'cmd'` - Windows cmd.exe (`cmd.exe`)

**Options:**

- `opts.name` - Custom script filename (default: `script_N`)

**Example:**

```lua
sys.build({
  id = 'my-tool',
  create = function(inputs, ctx)
    ctx:script('shell', [[
      ./configure --prefix=$out
      make -j$(nproc)
      make install
    ]], { name = 'build' })
    
    return { out = ctx.out }
  end,
})
```

Script files are written to `$out/tmp/` and persist after execution for debugging.

```

**Step 4: Commit**

```bash
git add docs/architecture/04-lua-api.md
git commit -m "docs: add ctx:script() to Lua API documentation"
```

---

## Task 6: Update init.lua Module Documentation

**Files:**

- Modify: `init.lua`

**Step 1: Add comment block for script method**

Add a comment block before the `script_impl` function:

```lua
  --- Write a script file and execute it.
  ---
  --- Writes the script content to $out/tmp/<name>.<ext> and executes it with
  --- the appropriate interpreter. Script files are kept after execution for
  --- debugging.
  ---
  --- @param ctx table The build or bind context
  --- @param format string Script format: 'shell', 'bash', 'powershell', or 'cmd'
  --- @param content string Script content (written verbatim)
  --- @param opts? table Optional settings
  --- @param opts.name? string Script filename without extension (default: script_N)
  --- @return table result { stdout: placeholder string, path: script file path }
  local function script_impl(ctx, format, content, opts)
```

**Step 2: Commit**

```bash
git add init.lua
git commit -m "docs: add LuaLS annotations for ctx:script()"
```

---

## Task 7: Final Verification

**Step 1: Run full test suite**

```bash
cargo test
```

**Step 2: Run lints**

```bash
cargo fmt && cargo clippy --all-targets --all-features
```

**Step 3: Manual smoke test**

Create a temporary test:

```bash
cat > /tmp/test-script.lua << 'EOF'
return {
  inputs = {},
  setup = function()
    sys.build({
      id = 'smoke-test',
      create = function(_, ctx)
        local result = ctx:script('shell', [[
          echo "Hello from script!"
          pwd
        ]], { name = 'smoke' })
        print("stdout placeholder: " .. result.stdout)
        print("path: " .. result.path)
        return { out = ctx.out }
      end,
    })
  end,
}
EOF

cargo run -- eval /tmp/test-script.lua
```

**Step 4: Final commit**

```bash
git add -A
git commit -m "feat: ctx:script() implementation complete"
```

---

## Summary

| Task | Description | Files |
|------|-------------|-------|
| 1 | Add script method implementation | `init.lua` |
| 2 | Add unit tests for shell format | `tests/fixtures/script_method.lua` |
| 3 | Add Windows format tests | `tests/fixtures/script_method.lua` |
| 4 | Add error handling tests | `tests/integration/script_method.rs` |
| 5 | Update Lua API docs | `docs/architecture/04-lua-api.md` |
| 6 | Add LuaLS annotations | `init.lua` |
| 7 | Final verification | - |

**Estimated time:** 30-45 minutes

**Dependencies:** None (uses existing ctx method registration system)
