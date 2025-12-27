--- Bind with check callback for drift detection tests.
--- Tests that check callbacks detect drift when files are modified/deleted.

local TEST_DIR = os.getenv('TEST_OUTPUT_DIR') or '/tmp/syslua-test'

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
      id = 'check-test',
      create = function(_, ctx)
        if sys.os == 'windows' then
          sh(ctx, 'New-Item -ItemType Directory -Force -Path "' .. TEST_DIR .. '" | Out-Null')
          sh(ctx, 'Set-Content -Path "' .. TEST_DIR .. '\\check-marker.txt" -Value "exists"')
        else
          sh(ctx, 'mkdir -p ' .. TEST_DIR)
          sh(ctx, 'echo exists > ' .. TEST_DIR .. '/check-marker.txt')
        end
        return { file = TEST_DIR .. (sys.os == 'windows' and '\\check-marker.txt' or '/check-marker.txt') }
      end,
      check = function(outputs, _, ctx)
        local drifted
        if sys.os == 'windows' then
          drifted = sh(ctx, 'if (Test-Path "' .. outputs.file .. '") { Write-Host -NoNewline "false" } else { Write-Host -NoNewline "true" }')
        else
          drifted = sh(ctx, 'test -f "' .. outputs.file .. '" && printf false || printf true')
        end
        return { drifted = drifted, message = 'file does not exist' }
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
