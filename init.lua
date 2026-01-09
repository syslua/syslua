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

        local wrapper = string.format(
          '@echo off\r\n%s\r\n"%%~dp0%s.real.exe" %%*',
          env_block,
          binary_name:gsub('%.exe$', '')
        )

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

        local wrapper = string.format(
          '#!/bin/sh\n%s\nexec "%s.real" "$@"',
          env_block,
          binary_path
        )

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
              'find "%s" -type f -executable | while read f; do patchelf --set-rpath "%s" "$f" 2>/dev/null || true; done',
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
              'find "%s" -type f -perm +111 | while read f; do %s; done',
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
  end,
}
