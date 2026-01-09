return {
  inputs = {
    syslua = 'path:./lua',
  },
  setup = function(_inputs)
    require('syslua').setup()

    sys.build({
      id = 'test-script-shell',
      create = function(_inputs, ctx)
        local result = ctx:script(
          'shell',
          [[
echo "hello from shell"
]]
        )
        return {
          out = ctx.out,
          stdout = result.stdout,
          script_path = result.path,
        }
      end,
    })

    sys.build({
      id = 'test-script-named',
      create = function(_inputs, ctx)
        local result = ctx:script(
          'shell',
          [[
echo "named script"
]],
          { name = 'my-script' }
        )
        return {
          out = ctx.out,
          stdout = result.stdout,
          script_path = result.path,
        }
      end,
    })

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

    sys.build({
      id = 'test-script-bash',
      create = function(_inputs, ctx)
        local result = ctx:script(
          'bash',
          [[
declare -a arr=("hello" "world")
echo "${arr[@]}"
]]
        )
        return {
          out = ctx.out,
          stdout = result.stdout,
        }
      end,
    })

    if sys.os == 'windows' then
      sys.build({
        id = 'test-script-powershell',
        create = function(_inputs, ctx)
          local result = ctx:script(
            'powershell',
            [[
Write-Output "hello from powershell"
]]
          )
          return {
            out = ctx.out,
            stdout = result.stdout,
            script_path = result.path,
          }
        end,
      })

      sys.build({
        id = 'test-script-cmd',
        create = function(_inputs, ctx)
          local result = ctx:script(
            'cmd',
            [[
@echo off
echo hello from cmd
]]
          )
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
