--- Tests for ctx:script() method.
--- Exercises shell and bash formats for script execution.

return {
  inputs = {},
  setup = function(_inputs)
    -- Test 1: Basic shell script
    sys.build({
      id = 'test-script-shell',
      create = function(_inputs, ctx)
        local result = ctx:script('shell', [[
echo "hello from shell"
]])
        return {
          out = ctx.out,
          stdout = result.stdout,
          script_path = result.path,
        }
      end,
    })

    -- Test 2: Named script
    sys.build({
      id = 'test-script-named',
      create = function(_inputs, ctx)
        local result = ctx:script('shell', [[
echo "named script"
]], { name = 'my-script' })
        return {
          out = ctx.out,
          stdout = result.stdout,
          script_path = result.path,
        }
      end,
    })

    -- Test 3: Multiple scripts (counter test)
    sys.build({
      id = 'test-script-counter',
      create = function(_inputs, ctx)
        local r1 = ctx:script('shell', [[echo "first"]])
        local r2 = ctx:script('shell', [[echo "second"]])
        return {
          out = ctx.out,
          path1 = r1.path,
          path2 = r2.path,
        }
      end,
    })

    -- Test 4: Bash format
    sys.build({
      id = 'test-script-bash',
      create = function(_inputs, ctx)
        local result = ctx:script('bash', [[
declare -a arr=("hello" "world")
echo "${arr[@]}"
]])
        return {
          out = ctx.out,
          stdout = result.stdout,
        }
      end,
    })

    -- Test 5: PowerShell format (Windows)
    if sys.os == 'windows' then
      sys.build({
        id = 'test-script-powershell',
        create = function(_inputs, ctx)
          local result = ctx:script('powershell', [[
Write-Output "hello from powershell"
]])
          return {
            out = ctx.out,
            stdout = result.stdout,
            script_path = result.path,
          }
        end,
      })

      -- Test 6: Cmd format (Windows)
      sys.build({
        id = 'test-script-cmd',
        create = function(_inputs, ctx)
          local result = ctx:script('cmd', [[
@echo off
echo hello from cmd
]])
          return {
            out = ctx.out,
            stdout = result.stdout,
            script_path = result.path,
          }
        end,
      })
    end
  end,
}
