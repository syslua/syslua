local prio = require('syslua.priority')
local f = require('syslua.interpolation')

---@class syslua.environment.files
local M = {}

---@class syslua.environment.files.FileOptions
---@field source? syslua.Option<string> Path to the source file or directory
---@field content? syslua.MergeableOption<string> Content to write to the target file (if source is not provided)
---@field mutable? syslua.Option<boolean> Whether the target should be mutable (default: false)

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
                  f('Copy-Item -Recurse -Path "{{source}}" -Destination "{{target}}"', inputs),
                },
              })
            else
              ctx:exec({
                bin = '/bin/sh',
                args = { '-c', f('cp -r "{{source}}" "{{target}}"', inputs) },
              })
            end
          else
            if sys.os == 'windows' then
              ctx:exec({
                bin = 'powershell.exe',
                args = {
                  '-NoProfile',
                  '-Command',
                  f('Set-Content -Path "{{target}}" -Value "{{content}}"', inputs),
                },
              })
            else
              ctx:exec({
                bin = '/bin/sh',
                args = { '-c', f('echo "{{content}}" > "{{target}}"', inputs) },
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
                f('Remove-Item -Path "{{target}}" -Recurse -Force -ErrorAction SilentlyContinue', outputs),
              },
            })
          else
            ctx:exec({ bin = '/bin/sh', args = { '-c', f('rm -rf "{{target}}"', outputs) } })
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
          local out_path = f('{{out}}/{{basename}}', { out = ctx.out, basename = basename })
          if inputs.source then
            if sys.os == 'windows' then
              ctx:exec({
                bin = 'powershell.exe',
                args = {
                  '-NoProfile',
                  '-Command',
                  f('Copy-Item -Recurse -Path "{{source}}" -Destination "{{out_path}}"', {
                    source = inputs.source,
                    out_path = out_path,
                  }),
                },
              })
            else
              ctx:exec({
                bin = '/bin/sh',
                args = { '-c', f('cp -r "{{source}}" "{{out_path}}"', { source = inputs.source, out_path = out_path }) },
              })
            end
          else
            if sys.os == 'windows' then
              ctx:exec({
                bin = 'powershell.exe',
                args = {
                  '-NoProfile',
                  '-Command',
                  f('Set-Content -Path "{{out_path}}" -Value "{{content}}"', {
                    out_path = out_path,
                    content = inputs.content,
                  }),
                },
              })
            else
              ctx:exec({
                bin = '/bin/sh',
                args = {
                  '-c',
                  f('echo "{{content}}" > "{{out_path}}"', { content = inputs.content, out_path = out_path }),
                },
              })
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
          local build_path = inputs.build.outputs.path
          if sys.os == 'windows' then
            ctx:exec({
              bin = 'powershell.exe',
              args = {
                '-NoProfile',
                '-Command',
                f('New-Item -ItemType SymbolicLink -Path "{{target}}" -Target "{{build_path}}"', {
                  target = inputs.target,
                  build_path = build_path,
                }),
              },
            })
          else
            ctx:exec({
              bin = '/bin/sh',
              args = {
                '-c',
                f('ln -s "{{build_path}}" "{{target}}"', { build_path = build_path, target = inputs.target }),
              },
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
                f('Remove-Item -Path "{{link}}" -Recurse -Force', outputs),
              },
            })
          else
            ctx:exec({ bin = '/bin/sh', args = { '-c', f('rm -rf "{{link}}"', outputs) } })
          end
        end,
      })
    end
  end
end

return M
