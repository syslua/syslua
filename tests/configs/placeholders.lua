--- Placeholder test configuration
--- Demonstrates the $${...} placeholder syntax for deferred value resolution
---
--- Key features demonstrated:
--- 1. Action chaining - ctx:fetch_url() returns $${action:0}, used in ctx:cmd()
--- 2. Multiple outputs - Build returning multiple named outputs
--- 3. Build -> Bind references - Using build outputs in bind commands
--- 4. Shell variables - $HOME, $PATH passing through unchanged (no escaping needed)
--- 5. Destroy actions - Cleanup commands for rollback

-- Standard PATH for commands (syslua isolates the environment for reproducibility)
local PATH = "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"

return {
  inputs = {},
  setup = function(_)
    -- Build ripgrep from release tarball
    -- Demonstrates action chaining: fetch returns $${action:0}, used by extract command
    -- Note: Using a real release URL that contains the binary
    local rg = sys.build({
      name = "ripgrep",
      version = "14.1.1",
      apply = function(_, ctx)
        -- fetch_url returns $${action:0} - the download location
        local archive = ctx:fetch_url(
          "https://github.com/BurntSushi/ripgrep/releases/download/14.1.1/ripgrep-14.1.1-x86_64-apple-darwin.tar.gz",
          "fc87e78f7cb3fea12d69072e7ef3b21509754717b746368fd40d88963630e2b3"
        )

        -- Create output directories using ctx.out (resolves to $${out}, the build's store path)
        ctx:cmd({ cmd = "mkdir -p " .. ctx.out .. "/bin " .. ctx.out .. "/share/man/man1", env = { PATH = PATH } })

        -- Extract to TMPDIR (automatically set by syslua to a clean temp space)
        -- Also demonstrates shell variable $TMPDIR passing through unchanged
        ctx:cmd({ cmd = "tar xf " .. archive .. " -C $TMPDIR", env = { PATH = PATH } })

        -- Copy the binary and man page to output using ctx.out
        ctx:cmd({
          cmd = "cp $TMPDIR/ripgrep-14.1.1-x86_64-apple-darwin/rg " .. ctx.out .. "/bin/ && "
            .. "cp $TMPDIR/ripgrep-14.1.1-x86_64-apple-darwin/doc/rg.1 " .. ctx.out .. "/share/man/man1/",
          env = { PATH = PATH },
        })

        -- Return multiple named outputs using ctx.out
        return {
          out = ctx.out,
          bin = ctx.out .. "/bin/rg",
          man = ctx.out .. "/share/man/man1/rg.1",
        }
      end,
    })

    -- Build fd (another CLI tool)
    -- Demonstrates simpler build with environment variables
    local fd = sys.build({
      name = "fd",
      version = "10.2.0",
      apply = function(_, ctx)
        local archive = ctx:fetch_url(
          "https://github.com/sharkdp/fd/releases/download/v10.2.0/fd-v10.2.0-x86_64-apple-darwin.tar.gz",
          "991a648a58870230af9547c1ae33e72cb5c5199a622fe5e540e162d6dba82d48"
        )

        ctx:cmd({ cmd = "mkdir -p " .. ctx.out .. "/bin", env = { PATH = PATH } })
        ctx:cmd({ cmd = "tar xf " .. archive .. " -C $TMPDIR", env = { PATH = PATH } })
        ctx:cmd({ cmd = "cp $TMPDIR/fd-v10.2.0-x86_64-apple-darwin/fd " .. ctx.out .. "/bin/", env = { PATH = PATH } })

        return { out = ctx.out }
      end,
    })

    -- Bind ripgrep to system
    -- Demonstrates using build outputs in bind commands
    sys.bind({
      inputs = { rg = rg },
      apply = function(bind_inputs, ctx)
        -- Create target directory first
        ctx:cmd({ cmd = "mkdir -p /tmp/syslua-test/.local/bin /tmp/syslua-test/.local/share/man/man1", env = { PATH = PATH } })

        -- Reference build output via inputs
        ctx:cmd({
          cmd = "ln -sf " .. bind_inputs.rg.outputs.bin .. " /tmp/syslua-test/.local/bin/rg",
          env = { PATH = PATH },
        })

        -- Create man page symlink using build outputs
        ctx:cmd({
          cmd = "ln -sf " .. bind_inputs.rg.outputs.man .. " /tmp/syslua-test/.local/share/man/man1/rg.1",
          env = { PATH = PATH },
        })
      end,
      destroy = function(_, ctx)
        -- Cleanup commands
        ctx:cmd({ cmd = "rm -f /tmp/syslua-test/.local/bin/rg", env = { PATH = PATH } })
        ctx:cmd({ cmd = "rm -f /tmp/syslua-test/.local/share/man/man1/rg.1", env = { PATH = PATH } })
      end,
    })

    -- Bind fd to system
    sys.bind({
      inputs = { fd = fd },
      apply = function(bind_inputs, ctx)
        ctx:cmd({ cmd = "mkdir -p /tmp/syslua-test/.local/bin", env = { PATH = PATH } })
        ctx:cmd({
          cmd = "ln -sf " .. bind_inputs.fd.outputs.out .. "/bin/fd /tmp/syslua-test/.local/bin/fd",
          env = { PATH = PATH },
        })
      end,
      destroy = function(_, ctx)
        ctx:cmd({ cmd = "rm -f /tmp/syslua-test/.local/bin/fd", env = { PATH = PATH } })
      end,
    })

    -- Bind that creates an env file combining multiple builds
    -- Demonstrates referencing multiple builds and complex shell variable usage
    sys.bind({
      inputs = { rg = rg, fd = fd },
      apply = function(bind_inputs, ctx)
        ctx:cmd({ cmd = "mkdir -p /tmp/syslua-test/.local", env = { PATH = PATH } })

        -- Create an env.sh that sets up PATH with both tools
        -- Note: $PATH at end is a shell variable (preserved)
        -- The build output paths are Lua string concatenation
        local env_content = "export PATH="
          .. bind_inputs.rg.outputs.out
          .. "/bin:"
          .. bind_inputs.fd.outputs.out
          .. "/bin:$PATH"

        ctx:cmd({
          cmd = 'echo "' .. env_content .. '" > /tmp/syslua-test/.local/env.sh',
          env = { PATH = PATH },
        })
      end,
      destroy = function(_, ctx)
        ctx:cmd({ cmd = "rm -f /tmp/syslua-test/.local/env.sh", env = { PATH = PATH } })
      end,
    })
  end,
}
