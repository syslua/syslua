--- Basic bind create/destroy lifecycle.
--- Tests that binds can be created and destroyed.

local TEST_DIR = os.getenv('TEST_OUTPUT_DIR') or '/tmp/syslua-test'

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
    sys.bind({
      id = 'test-bind',
      create = function(_, ctx)
        if sys.os == 'windows' then
          sh(ctx, 'if not exist "' .. TEST_DIR .. '" mkdir "' .. TEST_DIR .. '"')
          sh(ctx, 'echo created > "' .. TEST_DIR .. '\\created.txt"')
        else
          sh(ctx, 'mkdir -p ' .. TEST_DIR)
          sh(ctx, 'echo created > ' .. TEST_DIR .. '/created.txt')
        end
        return { file = TEST_DIR .. (sys.os == 'windows' and '\\created.txt' or '/created.txt') }
      end,
      destroy = function(outputs, ctx)
        if sys.os == 'windows' then
          sh(ctx, 'del /f "' .. outputs.file .. '" 2>nul')
        else
          sh(ctx, 'rm -f ' .. outputs.file)
        end
      end,
    })
  end,
}
