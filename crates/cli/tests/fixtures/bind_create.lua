--- Basic bind create/destroy lifecycle.
--- Tests that binds can be created and destroyed.

local TEST_DIR = os.getenv('TEST_OUTPUT_DIR')
if TEST_DIR then
  TEST_DIR = sys.path.canonicalize(TEST_DIR)
else
  TEST_DIR = '/tmp/syslua-test'
end

--- Cross-platform shell execution with PATH injection for sandbox.
--- @param ctx BuildCtx | BindCtx
--- @param script string
--- @return string
local function sh(ctx, script)
  if sys.os == 'windows' then
    local system_drive = os.getenv('SystemDrive') or 'C:'
    return ctx:exec({
      bin = 'powershell.exe',
      args = {
        '-NoProfile',
        '-NonInteractive',
        '-Command',
        script,
      },
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
          sh(ctx, 'New-Item -ItemType Directory -Force -Path "' .. TEST_DIR .. '" | Out-Null')
          sh(ctx, 'Set-Content -Path "' .. TEST_DIR .. '\\created.txt" -Value "created"')
        else
          sh(ctx, 'mkdir -p ' .. TEST_DIR)
          sh(ctx, 'echo created > ' .. TEST_DIR .. '/created.txt')
        end
        return { file = TEST_DIR .. (sys.os == 'windows' and '\\created.txt' or '/created.txt') }
      end,
      destroy = function(outputs, ctx)
        if sys.os == 'windows' then
          sh(ctx, 'Remove-Item -Force -ErrorAction SilentlyContinue -Path "' .. outputs.file .. '"')
        else
          sh(ctx, 'rm -f ' .. outputs.file)
        end
      end,
    })
  end,
}
