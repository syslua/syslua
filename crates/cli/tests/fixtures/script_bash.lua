return {
  inputs = {
    syslua = 'path:./lua',
  },
  setup = function()
    require('syslua').setup()

    sys.build({
      id = 'test-script-bash',
      create = function(_inputs, ctx)
        local result = ctx:script('bash', [[
arr=(one two three)
echo "${arr[1]}"
]])
        return { out = ctx.out }
      end,
    })
  end,
}
