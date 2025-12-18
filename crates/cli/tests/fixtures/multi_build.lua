--- Multiple builds with dependencies.
--- Tests that multiple builds are realized in the correct order.

--- Cross-platform shell execution with PATH injection for sandbox.
--- @param ctx ActionCtx
--- @param script string
--- @return string
local function sh(ctx, script)
  if sys.os == 'windows' then
    local system_drive = os.getenv('SystemDrive') or 'C:'
    local cmd = os.getenv('COMSPEC') or system_drive .. '\\Windows\\System32\\cmd.exe'
    return ctx:exec({
      bin = cmd,
      args = { '/c', script },
      env = { PATH = system_drive .. '\\Windows\\System32;' .. system_drive .. '\\Windows' },
    })
  else
    return ctx:exec({
      bin = '/bin/sh',
      args = { '-c', script },
      env = { PATH = '/bin:/usr/bin' },
    })
  end
end

return {
  inputs = {},
  setup = function(_)
    -- First build: creates a data file
    local data_build = sys.build({
      id = 'data-1.0.0',
      create = function(_, ctx)
        if sys.os == 'windows' then
          sh(ctx, 'echo DATA > ' .. ctx.out .. '\\data.txt')
        else
          sh(ctx, 'echo DATA > ' .. ctx.out .. '/data.txt')
        end
        return { data_file = ctx.out .. (sys.os == 'windows' and '\\data.txt' or '/data.txt') }
      end,
    })

    -- Second build: uses the data file from first build
    sys.build({
      id = 'processor-1.0.0',
      inputs = { data = data_build },
      create = function(inputs, ctx)
        if sys.os == 'windows' then
          sh(ctx, 'type "' .. inputs.data.outputs.data_file .. '" > ' .. ctx.out .. '\\processed.txt')
        else
          sh(ctx, 'cat ' .. inputs.data.outputs.data_file .. ' > ' .. ctx.out .. '/processed.txt')
        end
        return { processed_file = ctx.out .. (sys.os == 'windows' and '\\processed.txt' or '/processed.txt') }
      end,
    })
  end,
}
