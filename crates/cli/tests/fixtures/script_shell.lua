return {
  inputs = {
    syslua = 'path:./lua',
  },
  setup = function()
    require('syslua').setup()

    sys.build({
      id = 'test-script-shell',
      create = function(_inputs, ctx)
        local result = ctx:script('shell', [[
echo "hello from script"
echo "second line"
]])
        return {
          out = ctx.out,
          captured = result.stdout,
          script_path = result.path,
        }
      end,
    })
  end,
}
