return {
  inputs = {
    syslua = 'path:./lua',
  },
  setup = function()
    require('syslua').setup()

    sys.build({
      id = 'test-script-custom-name',
      create = function(_inputs, ctx)
        local result = ctx:script('shell', 'echo "custom"', { name = 'my-build-script' })
        return { out = ctx.out }
      end,
    })
  end,
}
