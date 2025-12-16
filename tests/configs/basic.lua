--- Basic test configuration
--- Entry point returns a table with `inputs` and `setup` fields

-- Standard PATH for commands (syslua isolates the environment for reproducibility)
local PATH = '/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin'

return {
  inputs = {},
  setup = function(_)
    local rg = sys.build({
      name = "ripgrep",
      version = "15.0.0",
      apply = function(_, ctx)
        ctx:cmd({ cmd = "echo 'building ripgrep'", env = { PATH = PATH } })
        return { out = ctx.out }
      end,
    })

    sys.bind({
      inputs = { build = rg },
      apply = function(bind_inputs, ctx)
        ctx:cmd({
          cmd = "mkdir -p /tmp/syslua-test && ln -sf " .. bind_inputs.build.outputs.out .. "/bin/rg /tmp/syslua-test/rg",
          env = { PATH = PATH },
        })
        return { link = "/tmp/syslua-test/rg" }
      end,
      destroy = function(outputs, ctx)
        ctx:cmd({ cmd = "rm -f " .. outputs.link, env = { PATH = PATH } })
      end,
    })
  end,
}
