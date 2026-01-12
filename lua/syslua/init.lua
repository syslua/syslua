---@class syslua
---@field pkgs syslua.pkgs
---@field environment syslua.environment
---@field programs syslua.programs
---@field users syslua.users
---@field group syslua.group
---@field lib syslua.lib
---@field f fun(str: string, values?: table): string String interpolation (f-string style)
---@field interpolate fun(str: string, values?: table): string String interpolation
local M = {}

-- String interpolation for user configs (uses {{}} delimiters to avoid shell confusion)
-- Usage: syslua.f("Hello {{name}}!") or syslua.f("{{x + y}}", {x=1, y=2})
M.f = require('syslua.interpolation')
M.interpolate = M.f

setmetatable(M, {
  __index = function(t, k)
    local cached = rawget(t, k)
    if cached ~= nil then
      return cached
    end
    local ok, mod = pcall(require, 'syslua.' .. k)
    if ok then
      rawset(t, k, mod)
      return mod
    else
      error("Module 'syslua." .. k .. "' not found")
    end
  end,
})

---@alias syslua.Option<T> T | syslua.priority.PriorityValue<T>
---@alias syslua.MergeableOption<T> T | syslua.priority.PriorityValue<T> | syslua.priority.Mergeable<T>

---@class BuildCtx
---@field script fun(self: BuildCtx, format: "shell"|"bash"|"cmd"|"powershell", content: string, opts?: {name?: string}): {stdout: string, path: string}
---@field wrap_binary fun(self: BuildCtx, opts: {binary: string, env?: table<string,string>}): nil
---@field patch_rpath fun(self: BuildCtx, opts: {deps: BuildRef[], patchelf?: BuildRef}|BuildRef[]): nil
---@field patch_shebang fun(self: BuildCtx, interpreter: string): nil

M.setup = function()
  local unix_path = '/bin:/usr/bin'
  local win_path = (os.getenv('SystemDrive') or 'C:')
    .. '\\Windows\\System32;'
    .. (os.getenv('SystemDrive') or 'C:')
    .. '\\Windows'

  local function script_impl(ctx, format, content, opts)
    opts = opts or {}
    local name = opts.name or ('script_' .. ctx.action_count)

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

    if format == 'shell' or format == 'bash' then
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
        env = { PATH = unix_path },
      })
    else
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
        env = { PATH = win_path },
      })
    end

    local exec_args = {}
    for _, v in ipairs(args_prefix) do
      table.insert(exec_args, v)
    end
    table.insert(exec_args, script_path)

    local exec_env = (format == 'shell' or format == 'bash') and { PATH = unix_path } or { PATH = win_path }
    local stdout = ctx:exec({ bin = bin, args = exec_args, env = exec_env })

    return {
      stdout = stdout,
      path = script_path,
    }
  end

  sys.register_build_ctx_method('script', script_impl)
  sys.register_bind_ctx_method('script', script_impl)

  sys.register_build_ctx_method('wrap_binary', function(ctx, opts)
    if not opts.binary then
      error('wrap_binary requires opts.binary')
    end
    opts.env = opts.env or {}

    local binary_path = opts.binary
    local binary_name = binary_path:match('([^/\\]+)$')

    if sys.os == 'windows' then
      local env_lines = {}
      for k, v in pairs(opts.env) do
        table.insert(env_lines, string.format('set %s=%s', k, v))
      end
      local env_block = table.concat(env_lines, '\r\n')

      local wrapper =
        string.format('@echo off\r\n%s\r\n"%%~dp0%s.real.exe" %%*', env_block, binary_name:gsub('%.exe$', ''))

      ctx:script(
        'cmd',
        string.format(
          'move "%s" "%s.real.exe"\r\necho %s > "%s"',
          binary_path,
          binary_path:gsub('%.exe$', ''),
          wrapper:gsub('\r\n', '&echo.'),
          binary_path:gsub('%.exe$', '.cmd')
        ),
        { name = 'wrap_binary' }
      )
    else
      local env_lines = {}
      for k, v in pairs(opts.env) do
        table.insert(env_lines, string.format('export %s="%s"', k, v))
      end
      local env_block = table.concat(env_lines, '\n')

      local wrapper = string.format('#!/bin/sh\n%s\nexec "%s.real" "$@"', env_block, binary_path)

      ctx:script(
        'shell',
        string.format(
          "mv '%s' '%s.real'\nprintf '%s' > '%s'\nchmod +x '%s'",
          binary_path,
          binary_path,
          wrapper:gsub("'", "'\\''"),
          binary_path,
          binary_path
        ),
        { name = 'wrap_binary' }
      )
    end
  end)

  sys.register_build_ctx_method('patch_rpath', function(ctx, opts)
    if sys.os == 'windows' then
      return
    end

    local deps = opts.deps or opts
    local patchelf_bin = opts.patchelf and opts.patchelf.outputs.bin or nil

    local rpath_entries = {}
    for _, dep in pairs(deps) do
      if dep.outputs and dep.outputs.lib then
        table.insert(rpath_entries, dep.outputs.lib)
      end
    end

    if #rpath_entries == 0 then
      return
    end

    if sys.os == 'linux' then
      local rpath = table.concat(rpath_entries, ':')
      if patchelf_bin then
        ctx:script(
          'shell',
          string.format(
            [[
find "%s" -type f -executable | while read f; do
  "%s" --set-rpath "%s" "$f" 2>/dev/null || true
done
]],
            ctx.out,
            patchelf_bin,
            rpath
          ),
          { name = 'patch_rpath' }
        )
      else
        ctx:script(
          'shell',
          string.format(
            [[
if ! command -v patchelf >/dev/null 2>&1; then
  echo "WARNING: patchelf not found. Install patchelf to enable RPATH patching." >&2
  echo "         Binaries may fail to find their library dependencies at runtime." >&2
  exit 0
fi
find "%s" -type f -executable | while read f; do
  patchelf --set-rpath "%s" "$f" 2>/dev/null || true
done
]],
            ctx.out,
            rpath
          ),
          { name = 'patch_rpath' }
        )
      end
    elseif sys.os == 'darwin' then
      local install_cmds = {}
      for _, p in ipairs(rpath_entries) do
        table.insert(install_cmds, string.format('install_name_tool -add_rpath "%s" "$f" 2>/dev/null || true', p))
      end
      ctx:script(
        'shell',
        string.format(
          [[
find "%s" -type f -perm +111 | while read f; do
  if codesign -d "$f" 2>/dev/null | grep -q 'runtime'; then
    echo "WARNING: Skipping hardened runtime binary: $f" >&2
    echo "         Use ad-hoc signing after modification if needed." >&2
    continue
  fi
  %s
done
]],
          ctx.out,
          table.concat(install_cmds, '; ')
        ),
        { name = 'patch_rpath' }
      )
    end
  end)

  sys.register_build_ctx_method('patch_shebang', function(ctx, interpreter)
    if not interpreter then
      error('patch_shebang requires interpreter path')
    end

    ctx:script(
      'shell',
      string.format(
        [[
find "%s" -type f | while read f; do
  head -c2 "$f" 2>/dev/null | grep -q '#!' && { sed '1s|^#!.*|#!%s|' "$f" > "$f.tmp" && mv "$f.tmp" "$f"; }
done
]],
        ctx.out,
        interpreter
      ),
      { name = 'patch_shebang' }
    )
  end)
end

return M
