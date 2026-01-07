local prio = require('syslua.priority')

---@class syslua.modules.file
local M = {}

---@class FileOptions
---@field target string Path to the target file or directory
---@field source? string Path to the source file or directory
---@field content? string Content to write to the target file (if source is not provided)
---@field mutable? boolean Whether the target should be mutable (default: false)

local default_opts = {
  mutable = prio.default(false),
}

---@type FileOptions
M.opts = default_opts

--- Set up a file or directory according to the provided options
---@param provided_opts FileOptions
M.setup = function(provided_opts)
  local new_opts = prio.merge(M.opts, provided_opts)
  if not new_opts then
    error('Failed to merge file options')
  end

  M.opts = new_opts
  if not M.opts.target then
    error("File setup requires a 'target' option")
  end

  if not M.opts.source and not M.opts.content then
    error("File setup requires either a 'source' or 'content' option")
  end

  if M.opts.mutable then
    sys.bind({
      inputs = {
        target = M.opts.target,
        source = M.opts.source,
        content = M.opts.content,
        mutable = M.opts.mutable,
      },
      create = function(inputs, ctx)
        if inputs.source then
          if sys.os == 'windows' then
            ctx:exec({
              bin = 'powershell.exe',
              args = {
                '-NoProfile',
                '-Command',
                string.format('Copy-Item -Recurse -Path "%s" -Destination "%s"', inputs.source, inputs.target),
              },
            })
          else
            ctx:exec({
              bin = '/bin/sh',
              args = { '-c', string.format('cp -r "%s" "%s"', inputs.source, inputs.target) },
            })
          end
        else
          if sys.os == 'windows' then
            ctx:exec({
              bin = 'powershell.exe',
              args = {
                '-NoProfile',
                '-Command',
                string.format('Set-Content -Path "%s" -Value "%s"', inputs.target, inputs.content),
              },
            })
          else
            ctx:exec({
              bin = '/bin/sh',
              args = { '-c', string.format('echo "%s" > "%s"', inputs.content, inputs.target) },
            })
          end
        end

        return {
          target = inputs.target,
        }
      end,
      destroy = function(outputs, ctx)
        if sys.os == 'windows' then
          ctx:exec({
            bin = 'powershell.exe',
            args = {
              '-NoProfile',
              '-Command',
              string.format('Remove-Item -Path "%s" -Recurse -Force -ErrorAction SilentlyContinue', outputs.target),
            },
          })
        else
          ctx:exec({ bin = '/bin/sh', args = { '-c', string.format('rm -rf "%s"', outputs.target) } })
        end
      end,
    })
  else
    local basename = sys.path.basename(M.opts.target)
    local build = sys.build({
      id = basename .. '-file',
      inputs = {
        source = M.opts.source,
        content = M.opts.content,
        mutable = M.opts.mutable,
      },
      create = function(inputs, ctx)
        local out_path = ctx.out .. '/' .. basename
        if inputs.source then
          if sys.os == 'windows' then
            ctx:exec({
              bin = 'powershell.exe',
              args = {
                '-NoProfile',
                '-Command',
                string.format('Copy-Item -Recurse -Path "%s" -Destination "%s"', inputs.source, out_path),
              },
            })
          else
            ctx:exec({ bin = '/bin/sh', args = { '-c', string.format('cp -r "%s" "%s"', inputs.source, out_path) } })
          end
        else
          if sys.os == 'windows' then
            ctx:exec({
              bin = 'powershell.exe',
              args = {
                '-NoProfile',
                '-Command',
                string.format('Set-Content -Path "%s" -Value "%s"', out_path, inputs.content),
              },
            })
          else
            ctx:exec({ bin = '/bin/sh', args = { '-c', string.format('echo "%s" > "%s"', inputs.content, out_path) } })
          end
        end

        return {
          path = out_path,
        }
      end,
    })

    sys.bind({
      inputs = {
        build = build,
        target = M.opts.target,
      },
      create = function(inputs, ctx)
        if sys.os == 'windows' then
          ctx:exec({
            bin = 'powershell.exe',
            args = {
              '-NoProfile',
              '-Command',
              string.format(
                'New-Item -ItemType SymbolicLink -Path "%s" -Target "%s"',
                inputs.target,
                inputs.build.outputs.path
              ),
            },
          })
        else
          ctx:exec({
            bin = '/bin/sh',
            args = { '-c', string.format('ln -s "%s" "%s"', inputs.build.outputs.path, inputs.target) },
          })
        end

        return {
          link = inputs.target,
        }
      end,
      destroy = function(outputs, ctx)
        if sys.os == 'windows' then
          ctx:exec({
            bin = 'powershell.exe',
            args = {
              '-NoProfile',
              '-Command',
              string.format('Remove-Item -Path "%s" -Recurse -Force', outputs.link),
            },
          })
        else
          ctx:exec({ bin = '/bin/sh', args = { '-c', string.format('rm -rf "%s"', outputs.link) } })
        end
      end,
    })
  end
end

return M
