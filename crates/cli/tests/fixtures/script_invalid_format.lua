return {
  inputs = {
    syslua = 'path:./lua',
  },
  setup = function()
    require('syslua').setup()

    sys.build({
      id = 'test-invalid-format',
      create = function(_inputs, ctx)
        ctx:script('invalid', [[echo "bad"]])
        return { out = ctx.out }
      end,
    })
  end,
}
