--- Tests rollback when a bind fails after destroying previous binds.
---
--- Test flow:
--- 1. First apply with TEST_PHASE=initial creates 'original-bind'
--- 2. Second apply with TEST_PHASE=failure removes 'original-bind' and adds 'failing-bind'
--- 3. 'failing-bind' fails during create
--- 4. Rollback should restore 'original-bind'

local TEST_DIR = os.getenv('TEST_OUTPUT_DIR')
if TEST_DIR then
  TEST_DIR = sys.path.canonicalize(TEST_DIR)
else
  TEST_DIR = '/tmp/syslua-test'
end
local PHASE = os.getenv('TEST_PHASE') or 'initial'

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
    if PHASE == 'initial' then
      -- This bind will be destroyed on second apply
      sys.bind({
        id = 'original-bind',
        create = function(_, ctx)
          if sys.os == 'windows' then
            sh(ctx, 'New-Item -ItemType Directory -Force -Path "' .. TEST_DIR .. '" | Out-Null')
            sh(ctx, 'Set-Content -Path "' .. TEST_DIR .. '\\original.txt" -Value "original"')
          else
            sh(ctx, 'mkdir -p ' .. TEST_DIR)
            sh(ctx, 'echo original > ' .. TEST_DIR .. '/original.txt')
          end
          return { file = TEST_DIR .. (sys.os == 'windows' and '\\original.txt' or '/original.txt') }
        end,
        destroy = function(outputs, ctx)
          if sys.os == 'windows' then
            sh(ctx, 'Remove-Item -Force -ErrorAction SilentlyContinue -Path "' .. outputs.file .. '"')
          else
            sh(ctx, 'rm -f ' .. outputs.file)
          end
        end,
      })
    elseif PHASE == 'failure' then
      -- This bind will fail during create
      sys.bind({
        id = 'failing-bind',
        create = function(_, ctx)
          sh(ctx, 'exit 1') -- deliberate failure
          return {}
        end,
        destroy = function(_, _) end,
      })
    end
  end,
}
