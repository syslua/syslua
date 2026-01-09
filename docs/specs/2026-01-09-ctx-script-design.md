# ctx:script() Design

**Date:** 2026-01-09  
**Status:** Draft  
**Authors:** Ian Pascoe, Claude

## Overview

This document describes the design for `ctx:script()`, a context method for builds and binds that writes a script file and executes it. This provides a cleaner API for multi-line, platform-specific scripting compared to manually constructing `ctx:exec()` calls with shell heredocs.

### Goals

1. **Simplify multi-line scripts** - Write readable scripts without escape gymnastics
2. **Cross-platform support** - First-class support for shell, bash, PowerShell, and cmd.exe
3. **Debuggability** - Script files persist in `$out/tmp/` for inspection on failure
4. **Composability** - Implemented as a Lua ctx method using existing `ctx:exec()` primitive

### Non-Goals

- New Rust `Action` variant (uses existing `Exec` action)
- Automatic platform detection (user specifies format explicitly)
- Placeholder resolution inside script body (use `string.format` or interpolation)

## API

### Basic Usage

```lua
-- Shell script (Unix)
ctx:script('shell', [[
  ./configure --prefix=$out
  make -j$(nproc)
  make install
]])

-- Bash script (Unix, bash-specific features)
ctx:script('bash', [[
  declare -A opts=([debug]=1 [verbose]=0)
  [[ -f config.sh ]] && source config.sh
  make -j$(nproc)
]])

-- PowerShell script (Windows)
ctx:script('powershell', [[
  $env:PATH = "$env:out\bin;$env:PATH"
  msbuild /p:Configuration=Release
]])

-- Cmd script (Windows legacy)
ctx:script('cmd', [[
  set PATH=%out%\bin;%PATH%
  nmake
]])
```

### With Options

```lua
-- Named script for clarity
ctx:script('shell', [[
  ./configure --prefix=$out
]], { name = 'configure' })

-- Results in: $out/tmp/configure.sh
```

### Return Value

Returns a table with two fields:

```lua
local result = ctx:script('shell', [[echo "hello"]], { name = 'setup' })

result.stdout  -- "$${action:N}" placeholder for captured stdout
result.path    -- "$${out}/tmp/setup.sh" path to the script file
```

### Signature

```lua
ctx:script(format, content, opts?) -> { stdout: string, path: string }
```

| Parameter | Type | Description |
|-----------|------|-------------|
| `format` | `'shell' \| 'bash' \| 'powershell' \| 'cmd'` | Script interpreter format |
| `content` | `string` | Script body (written verbatim to file) |
| `opts` | `table?` | Optional settings |
| `opts.name` | `string?` | Script filename (default: `script_N`) |

## Implementation

The `script` method is registered via `sys.register_build_ctx_method()` and `sys.register_bind_ctx_method()` in `init.lua`. It composes existing `ctx:exec()` calls.

### Format Mapping

| Format | Extension | Interpreter | Invocation |
|--------|-----------|-------------|------------|
| `shell` | `.sh` | `/bin/sh` | `/bin/sh <script>` |
| `bash` | `.bash` | `/bin/bash` | `/bin/bash <script>` |
| `powershell` | `.ps1` | `powershell.exe` | `powershell.exe -NoProfile -ExecutionPolicy Bypass -File <script>` |
| `cmd` | `.cmd` | `cmd.exe` | `cmd.exe /c <script>` |

### Script File Location

Scripts are written to `$out/tmp/<name>.<ext>`:
- Default name: `script_N` where N is a counter maintained per-context
- Custom name: provided via `opts.name`
- Always kept after execution (success or failure)

### Pseudocode

```lua
sys.register_build_ctx_method('script', function(ctx, format, content, opts)
  opts = opts or {}
  
  -- Track script count for default naming
  ctx._script_count = (ctx._script_count or 0) + 1
  local name = opts.name or ('script_' .. (ctx._script_count - 1))
  
  -- Determine extension and interpreter
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
    error('script() format must be shell, bash, powershell, or cmd')
  end
  
  local script_path = ctx.out .. '/tmp/' .. name .. ext
  
  -- Write script file
  if format == 'shell' or format == 'bash' then
    -- Unix: use printf to write, then chmod +x
    ctx:exec({
      bin = '/bin/sh',
      args = { '-c', string.format(
        'mkdir -p "%s/tmp" && printf \'%%s\' %q > "%s" && chmod +x "%s"',
        ctx.out, content, script_path, script_path
      )}
    })
  else
    -- Windows: use powershell to write file
    ctx:exec({
      bin = 'powershell.exe',
      args = { '-NoProfile', '-Command', string.format(
        'New-Item -ItemType Directory -Force -Path "%s\\tmp" | Out-Null; Set-Content -Path "%s" -Value @\'\n%s\n\'@',
        ctx.out, script_path, content
      )}
    })
  end
  
  -- Execute script
  local exec_args = {}
  for _, v in ipairs(args_prefix) do
    table.insert(exec_args, v)
  end
  table.insert(exec_args, script_path)
  
  local stdout = ctx:exec({ bin = bin, args = exec_args })
  
  return {
    stdout = stdout,
    path = script_path,
  }
end)
```

### Registration

Both `BuildCtx` and `BindCtx` receive the same implementation:

```lua
-- In init.lua setup()
local script_impl = function(ctx, format, content, opts)
  -- ... implementation above
end

sys.register_build_ctx_method('script', script_impl)
sys.register_bind_ctx_method('script', script_impl)
```

## Examples

### Basic Build Script

```lua
sys.build({
  id = 'my-tool',
  inputs = function()
    return {
      url = 'https://example.com/my-tool-1.0.tar.gz',
      sha256 = 'abc123...',
    }
  end,
  create = function(inputs, ctx)
    local archive = ctx:fetch_url(inputs.url, inputs.sha256)
    
    ctx:script('shell', string.format([[
      tar -xzf %s
      cd my-tool-1.0
      ./configure --prefix=$out
      make -j$(nproc)
      make install
    ]], archive), { name = 'build' })
    
    return { out = ctx.out }
  end,
})
```

### Cross-Platform Build

```lua
sys.build({
  id = 'cross-platform-tool',
  create = function(inputs, ctx)
    if sys.os == 'windows' then
      ctx:script('powershell', [[
        cmake -B build -G "Visual Studio 17 2022"
        cmake --build build --config Release
        cmake --install build --prefix $env:out
      ]], { name = 'build' })
    else
      ctx:script('shell', [[
        cmake -B build
        cmake --build build
        cmake --install build --prefix $out
      ]], { name = 'build' })
    end
    
    return { out = ctx.out }
  end,
})
```

### Capturing Output

```lua
sys.build({
  id = 'versioned-tool',
  create = function(inputs, ctx)
    -- ... build steps ...
    
    local result = ctx:script('shell', [[
      ./my-tool --version | head -1
    ]], { name = 'get-version' })
    
    return {
      out = ctx.out,
      version = result.stdout,
    }
  end,
})
```

### Bind with Script

```lua
sys.bind({
  id = 'setup-service',
  inputs = { tool = my_tool },
  create = function(inputs, ctx)
    ctx:script('shell', string.format([[
      mkdir -p ~/.config/my-tool
      cp %s/share/default-config.toml ~/.config/my-tool/config.toml
      
      if command -v systemctl >/dev/null 2>&1; then
        systemctl --user enable my-tool
        systemctl --user start my-tool
      fi
    ]], inputs.tool.outputs.out), { name = 'setup' })
    
    return { config = '~/.config/my-tool/config.toml' }
  end,
  destroy = function(outputs, ctx)
    ctx:script('shell', [[
      systemctl --user stop my-tool 2>/dev/null || true
      systemctl --user disable my-tool 2>/dev/null || true
      rm -rf ~/.config/my-tool
    ]], { name = 'teardown' })
  end,
})
```

## Design Decisions

### Why Lua-level, not Rust Action?

A new Rust `Action::Script` variant would require:
- Changes to `action/types.rs` and serialization
- New execution logic in `action/actions/`
- Placeholder resolver updates

Instead, `ctx:script()` composes existing primitives:
- Uses `ctx:exec()` for file writing and script execution
- Follows the pattern established by `wrap_binary`, `patch_rpath`, `patch_shebang` in `init.lua`
- Can be iterated on in Lua without Rust rebuilds

### Why explicit format parameter?

Auto-detection based on `sys.os` was considered but rejected:
- **Ambiguity**: Windows supports both PowerShell and cmd.exe
- **Clarity**: Reader immediately knows what interpreter runs
- **Portability**: Cross-platform scripts are intentionally verbose about their requirements

### Why always keep script files?

Options considered:
1. Always keep
2. Keep on failure, delete on success
3. Always delete

Decision: **Always keep** because:
- Script files are small (bytes to kilobytes)
- Debugging successful builds sometimes requires inspecting what ran
- Consistent behavior is easier to reason about
- Users can clean `$out/tmp/` manually if space is a concern

### Why sequential default naming?

Options considered:
1. Sequential index (`script_0.sh`)
2. Random/UUID suffix (`script_a7f3b2.sh`)
3. User-provided with sequential fallback

Decision: **User-provided with sequential fallback** because:
- Sequential is predictable and correlates with code order
- Optional names improve readability for complex builds
- No collision risk since counter is per-context

### Why return table instead of string?

`ctx:exec()` returns a single stdout placeholder string. `ctx:script()` returns `{ stdout, path }` because:
- Script path is useful for re-execution or referencing in subsequent commands
- Path is computable but awkward (`ctx.out .. '/tmp/' .. name .. ext`)
- Table is extensible for future additions (e.g., `stderr` if needed)

Keeping `ctx:exec()` as-is avoids breaking existing code.

## Future Considerations

### Stderr Capture

Currently, stderr goes to build logs on failure but isn't capturable as a placeholder. If use cases emerge for capturing stderr from successful commands (e.g., tools that output diagnostics to stderr), we could:

1. Add `$${action:N:stderr}` placeholder format
2. Update `ctx:exec()` to return `{ stdout, stderr }`
3. Add `stderr` field to `ctx:script()` return value

This is deferred until there's a concrete need.

### Script Templating

Some build systems support template syntax in scripts (e.g., `@OUT@` replaced before execution). We explicitly chose not to do this:

- Lua's `string.format()` and interpolation libraries handle this
- Placeholders like `$${out}` embedded in strings resolve at execution time
- Adding another templating layer creates confusion about when substitution happens

### Additional Formats

Potential future formats if demand exists:

| Format | Use Case |
|--------|----------|
| `zsh` | Zsh-specific scripts |
| `python` | Inline Python scripts |
| `lua` | Lua scripts (would need careful sandboxing) |

These would follow the same pattern: map format to extension, interpreter, and invocation args.

### Error Recovery Options

A future `opts.ignore_errors` flag could allow scripts to fail without failing the build:

```lua
ctx:script('shell', [[
  some-optional-check || true
]], { ignore_errors = true })
```

This would require changes to how `ctx:exec()` handles non-zero exit codes.
