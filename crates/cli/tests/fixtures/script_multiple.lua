return {
  inputs = {
    syslua = 'path:./lua',
  },
  setup = function()
    require('syslua').setup()

    sys.build({
      id = 'test-script-names',
      create = function(_inputs, ctx)
        local r1 = ctx:script('shell', 'echo "first"')
        local r2 = ctx:script('shell', 'echo "second"')
        local r3 = ctx:script('shell', 'echo "third"')
        return { out = ctx.out }
      end,
    })
  end,
}
