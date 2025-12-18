--- Tests that dependent binds are skipped when a build fails.
---
--- Expected behavior:
--- 1. 'failing-build' fails during create
--- 2. 'depends-on-failing-build' bind is skipped (not executed)
--- 3. No file should be created at TEST_OUTPUT_DIR/should-not-exist.txt

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
    local build = sys.build({
      id = 'failing-build',
      create = function(_, ctx)
        sh(ctx, 'exit 1') -- deliberate failure
        return { out = ctx.out }
      end,
    })

    -- This bind depends on the failing build, should be skipped
    sys.bind({
      id = 'depends-on-failing-build',
      inputs = { build = build },
      create = function(_, ctx)
        if sys.os == 'windows' then
          sh(ctx, 'if not exist "' .. TEST_DIR .. '" mkdir "' .. TEST_DIR .. '"')
          sh(ctx, 'echo should not exist > "' .. TEST_DIR .. '\\should-not-exist.txt"')
        else
          sh(ctx, 'mkdir -p ' .. TEST_DIR)
          sh(ctx, 'touch ' .. TEST_DIR .. '/should-not-exist.txt')
        end
        return {}
      end,
      destroy = function(_, _) end,
    })
  end,
}
