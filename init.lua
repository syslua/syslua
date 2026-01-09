return {
  inputs = {},
  setup = function(_inputs)
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

        ctx:exec({
          bin = 'cmd.exe',
          args = {
            '/c',
            string.format(
              'move "%s" "%s.real.exe" && echo %s > "%s"',
              binary_path,
              binary_path:gsub('%.exe$', ''),
              wrapper:gsub('\r\n', '&echo.'),
              binary_path:gsub('%.exe$', '.cmd')
            ),
          },
        })
      else
        local env_lines = {}
        for k, v in pairs(opts.env) do
          table.insert(env_lines, string.format('export %s="%s"', k, v))
        end
        local env_block = table.concat(env_lines, '\n')

        local wrapper = string.format('#!/bin/sh\n%s\nexec "%s.real" "$@"', env_block, binary_path)

        ctx:exec({
          bin = '/bin/sh',
          args = {
            '-c',
            string.format(
              'mv "%s" "%s.real" && printf \'%s\' > "%s" && chmod +x "%s"',
              binary_path,
              binary_path,
              wrapper:gsub("'", "'\\''"),
              binary_path,
              binary_path
            ),
          },
        })
      end
    end)

    sys.register_build_ctx_method('patch_rpath', function(ctx, deps)
      if sys.os == 'windows' then
        return
      end

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
        ctx:exec({
          bin = '/bin/sh',
          args = {
            '-c',
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
          },
        })
      elseif sys.os == 'darwin' then
        ctx:exec({
          bin = '/bin/sh',
          args = {
            '-c',
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
              table.concat(
                (function()
                  local cmds = {}
                  for _, p in ipairs(rpath_entries) do
                    table.insert(cmds, string.format('install_name_tool -add_rpath "%s" "$f" 2>/dev/null || true', p))
                  end
                  return cmds
                end)(),
                '; '
              )
            ),
          },
        })
      end
    end)

    sys.register_build_ctx_method('patch_shebang', function(ctx, interpreter)
      if not interpreter then
        error('patch_shebang requires interpreter path')
      end

      ctx:exec({
        bin = '/bin/sh',
        args = {
          '-c',
          string.format(
            [[find "%s" -type f | while read f; do
  head -c2 "$f" 2>/dev/null | grep -q '#!' && { sed '1s|^#!.*|#!%s|' "$f" > "$f.tmp" && mv "$f.tmp" "$f"; }
done]],
            ctx.out,
            interpreter
          ),
        },
      })
    end)

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
  end,
}
