return {
  inputs = {
    syslua = 'path:./lua',
  },
  setup = function()
    require('syslua').setup()

    sys.build({
      id = 'test-script-powershell',
      create = function(_inputs, ctx)
        local result = ctx:script('powershell', [[
Write-Output "hello from powershell"
]])
        return { out = ctx.out }
      end,
    })
  end,
}
