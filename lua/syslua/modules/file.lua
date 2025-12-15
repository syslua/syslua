---@class syslua.modules.file
local M = {}

---@class FileOptions
---@field target string Path to the target file or directory
---@field source? string Path to the source file or directory
---@field content? string Content to write to the target file (if source is not provided)
---@field mutable? boolean Whether the target should be mutable (default: false)

local default_options = {
  mutable = false,
}

--- Set up a file or directory according to the provided options
---@param opts FileOptions
M.setup = function(opts)
  if not opts.target then
    error("File setup requires a 'target' option")
  end

  if not opts.source and not opts.content then
    error("File setup requires either a 'source' or 'content' option")
  end

  local mutable = opts.mutable or default_options.mutable

  if mutable then
    sys.bind({
      inputs = {
        target = opts.target,
        source = opts.source,
        content = opts.content,
        mutable = mutable,
      },
      config = function(inputs, ctx)
        if opts.source then
          if sys.os == 'windows' then
            ctx:cmd({
              cmd = string.format('xcopy /E /I /Y "%s" "%s"', inputs.source, inputs.target),
            })
          else
            ctx:cmd({
              cmd = string.format('cp -r "%s" "%s"', inputs.source, inputs.target),
            })
          end
        else
          ctx:cmd({
            cmd = string.format('echo "%s" > "%s"', inputs.content, inputs.target),
          })
        end
      end,
    })
  else
    local build = sys.build({
      name = sys.path.basename(opts.target) .. '_bld',
      outputs = {
        out = sys.path.basename(opts.target),
      },
      inputs = {
        source = opts.source,
        content = opts.content,
        mutable = mutable,
      },
      config = function(inputs, ctx)
        if inputs.source then
          if sys.os == 'windows' then
            ctx:cmd({
              cmd = string.format('xcopy /E /I /Y "%s" "%s"', inputs.source, ctx.outputs.out),
            })
          else
            ctx:cmd({
              cmd = string.format('cp -r "%s" "%s"', inputs.source, ctx.outputs.out),
            })
          end
        else
          ctx:cmd({
            cmd = string.format('echo "%s" > "%s"', inputs.content, ctx.outputs.out),
          })
        end
      end,
    })

    sys.bind({
      inputs = {
        build = build,
        target = opts.target,
      },
      config = function(inputs, ctx)
        if sys.os == 'windows' then
          ctx:cmd({
            cmd = string.format(
              'New-Item -ItemType SymbolicLink -Path "%s" -Target "%s"',
              inputs.target,
              inputs.build.outputs.out
            ),
            undo_cmd = string.format('Remove-Item -Path "%s"', inputs.target),
          })
        else
          ctx:cmd({
            cmd = string.format('ln -s "%s" "%s"', inputs.build.outputs.out, inputs.target),
            undo_cmd = string.format('rm "%s"', inputs.target),
          })
        end
      end,
    })
  end
end

return M
