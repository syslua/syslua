--- Input resolution test configuration
--- Tests that git and path inputs are resolved correctly, including #ref syntax

-- Standard PATH for commands (syslua isolates the environment for reproducibility)
local PATH = "/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"

return {
  inputs = {
    -- Git input from GitHub with specific commit ref
    -- Using the initial commit of the repo for a stable test
    syslua = "git:https://github.com/spirit-led-software/syslua.git#3d522f5e2baf56a5e2f750d4664c174a2099833b",
  },
  setup = function(inputs)
    -- Verify the syslua input was resolved
    assert(inputs.syslua, "syslua input should be present")
    assert(inputs.syslua.path, "syslua input should have a path")
    assert(inputs.syslua.rev, "syslua input should have a rev")
    assert(#inputs.syslua.rev == 40, "syslua rev should be a full git hash (40 chars)")

    -- Verify the rev matches what we requested (it should resolve to the same commit)
    assert(
      inputs.syslua.rev == "3d522f5e2baf56a5e2f750d4664c174a2099833b",
      "syslua rev should match requested commit"
    )

    -- Print input info for debugging
    print("syslua input resolved:")
    print("  path: " .. inputs.syslua.path)
    print("  rev:  " .. inputs.syslua.rev)

    -- Create a simple build that uses the input
    local example = sys.build({
      name = "example-from-input",
      version = "1.0.0",
      inputs = {
        src = inputs.syslua,
      },
      apply = function(build_inputs, ctx)
        -- Reference the input path in a build command
        ctx:cmd({ cmd = "ls -la " .. build_inputs.src.path, env = { PATH = PATH } })
        return { out = "/store/example" }
      end,
    })

    -- Bind that references the build
    sys.bind({
      inputs = { example = example },
      apply = function(bind_inputs, ctx)
        ctx:cmd({
          cmd = "echo 'Example output: " .. bind_inputs.example.outputs.out .. "'",
          env = { PATH = PATH },
        })
      end,
    })
  end,
}
