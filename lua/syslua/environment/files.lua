local prio = require('syslua.priority')

---@class syslua.environment.files
local M = {}

---@class syslua.environment.files.FileOptions
---@field source? string | syslua.priority.PriorityValue<string> Path to the source file or directory
---@field content? string | syslua.priority.PriorityValue<string> | syslua.priority.Mergeable<string> Content to write to the target file (if source is not provided)
---@field mutable? boolean | syslua.priority.PriorityValue<boolean> Whether the target should be mutable (default: false)

---@class syslua.environment.files.Options: table<string, syslua.environment.files.FileOptions>

---@type syslua.environment.files.FileOptions
---@diagnostic disable-next-line: missing-fields
local default_file_opts = {
  mutable = prio.default(false),
}

---@type syslua.environment.files.Options
M.opts = {}

--- Set up a file or directory according to the provided options
---@param provided_opts syslua.environment.files.Options
M.setup = function(provided_opts)
  local new_opts = prio.merge(M.opts, provided_opts)
  if not new_opts then
    error('Failed to merge file options')
  end

  M.opts = new_opts

  for target, provided_file_opts in pairs(M.opts) do
    local file_opts = prio.merge(default_file_opts, provided_file_opts)
    if not file_opts.source and not file_opts.content then
      error("File setup requires either a 'source' or 'content' option")
    end

    if file_opts.mutable then
      sys.bind({
        inputs = {
          target = target,
          source = file_opts.source,
          content = file_opts.content,
          mutable = file_opts.mutable,
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
      local basename = sys.path.basename(target)
      local build = sys.build({
        id = basename .. '-file',
        inputs = {
          source = file_opts.source,
          content = file_opts.content,
          mutable = file_opts.mutable,
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
          target = target,
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
end

return M
