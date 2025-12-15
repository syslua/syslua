# Builds

> **Core Principle:** Builds are the sole primitive for producing store content in sys.lua.

A build is an immutable description of:

- What inputs are needed (arbitrary data)
- How to transform those inputs into outputs (config function)
- What outputs are produced

All managed state in sys.lua uses builds - not just packages, but also files and environment variables.

## The `sys.build()` Function

```lua
local my_build = sys.build({
  name = "ripgrep",           -- Required: identifier for debugging/logging
  version = "15.1.0",         -- Optional: human-readable version

  inputs = <table | function()>,  -- Optional: input specification
  apply = function(inputs, ctx),  -- Required: build logic
})
```

## Inputs (`inputs`)

Inputs can be a static table or a function for platform-specific resolution. **Inputs are arbitrary data** - there is no magic interpretation. The `config` function consumes this data and uses `ctx` helpers as needed.

```lua
-- Static table (simple case)
inputs = {
  src = {
    url = 'https://example.com/tool.tar.gz',
    sha256 = 'abc123...',
  },
  settings = { feature = true },
}

-- Function for cross-platform
inputs = function()
  return {
    src = {
      url = 'https://example.com/tool-' .. sys.platform .. '.tar.gz',
      sha256 = hashes[sys.platform],
    },
  }
end
```

### Build References

Inputs can include other builds for build dependencies:

```lua
local rust = sys.build({ name = "rust", ... })

sys.build({
  name = "ripgrep",
  inputs = function()
    return {
      src_url = "...",
      rust = rust,  -- Build reference
    }
  end,
  apply = function(inputs, ctx)
    -- inputs.rust.outputs.out is the realized output path of the rust build
    ctx:cmd({
      cmd = "cargo build --release",
      env = { PATH = inputs.rust.outputs.out .. "/bin:" .. os.getenv("PATH") },
    })
  end,
})
```

## Apply Function

The apply function transforms inputs into outputs:

```lua
apply = function(inputs, ctx)
  -- inputs: the table returned by inputs function (build refs have .outputs paths)
  -- ctx: build context with helpers
end
```

## Build Context (`BuildCtx`)

The build context provides actions for fetching and shell execution. Each action returns a placeholder string that will be resolved during execution.

```lua
-- Fetch operations (returns placeholder)
ctx:fetch_url(url, sha256)  -- Download file, verify hash, return "${action:N}"

-- Shell execution (returns placeholder)
ctx:cmd(opts)               -- Execute a shell command, return "${action:N}"
                            -- opts: string | { cmd, env?, cwd? }
```

### The `cmd` Action

The `cmd` action is the primary mechanism for executing operations during a build. This flexible approach allows Lua configuration to specify platform-specific commands rather than relying on preset Rust-backed actions:

```lua
-- Simple command (string)
ctx:cmd("make")

-- Command with options (table)
ctx:cmd({
  cmd = "make install",
  cwd = "/build/src",
  env = { PREFIX = ctx.outputs.out },
})
```

**BuildCmdOptions:**

| Field | Type | Description |
|-------|------|-------------|
| `cmd` | string | Required: the shell command to execute |
| `cwd` | string? | Optional: working directory for the command |
| `env` | table<string,string>? | Optional: environment variables for the command |

**Why `cmd` instead of preset actions?**

- **Platform flexibility**: Lua config decides what commands to run per platform
- **No Rust changes needed**: Adding new operations doesn't require Rust code changes
- **Transparent**: Users can see exactly what commands will be executed
- **Composable**: Complex operations built from simple shell commands

**Error handling:** All `ctx` operations throw on failure (Lua `error()`). A failed build leaves the user-facing system unchanged - atomic apply semantics ensure the pre-apply state is restored.

## Build Return Value

`sys.build {}` returns a table representing the build AND registers it globally. The registration happens on require - users can conditionally require modules for platform-specific packages.

```lua
local rg = sys.build { name = "ripgrep", outputs = {"out"}, ... }

rg.name           -- "ripgrep"
rg.version        -- "15.1.0" or nil
rg.hash           -- Build hash (computed at evaluation time)
rg.outputs        -- { out = <realized-store-output-path> }
```

## Build Hashing

The build hash is computed from the serialized `BuildDef`:

- `name`
- `version` (if present)
- `inputs` (evaluated `InputsRef`)
- `apply_actions` (the commands and fetch operations)
- `outputs` (if present)

This means:

- Same inputs + different actions = different build
- Same build on different platforms = different hash (if inputs differ)
- Build dependencies are included via their hash in inputs
- Action order matters - same actions in different order = different hash

## Rust Types

The build system uses a three-tier type architecture:

- **Spec** - Lua-side, contains closures, not serializable
- **Def** - Evaluated, serializable, stored in Manifest
- **Ref** - Content-addressed reference for cross-references

```rust
/// Hash for content-addressing builds
pub struct BuildHash(pub String);

/// Actions that can be performed during a build
pub enum BuildAction {
    FetchUrl { url: String, sha256: String },
    Cmd {
        cmd: String,
        env: Option<BTreeMap<String, String>>,
        cwd: Option<String>,
    },
}

/// Lua-side specification (not serializable, contains closures)
pub struct BuildSpec {
    pub name: String,
    pub version: Option<String>,
    pub inputs: Option<InputsSpec>,
    pub apply: Function,  // Lua closure
}

/// Evaluated definition (serializable, stored in Manifest)
pub struct BuildDef {
    pub name: String,
    pub version: Option<String>,
    pub inputs: Option<InputsRef>,
    pub apply_actions: Vec<BuildAction>,
    pub outputs: Option<BTreeMap<String, String>>,
}

/// Content-addressed reference (for cross-references in inputs)
pub struct BuildRef {
    pub name: String,
    pub version: Option<String>,
    pub inputs: Option<InputsRef>,
    pub outputs: HashMap<String, String>,
    pub hash: BuildHash,
}

/// Build context provided to apply function
pub struct BuildCtx {
    actions: Vec<BuildAction>,
}

impl BuildCtx {
    /// Fetch a URL with hash verification, returns placeholder
    pub fn fetch_url(&mut self, url: &str, sha256: &str) -> String;
    
    /// Execute a shell command, returns placeholder
    pub fn cmd(&mut self, opts: impl Into<BuildCmdOptions>) -> String;
    
    /// Consume context and return accumulated actions
    pub fn into_actions(self) -> Vec<BuildAction>;
}

/// Command options for build actions
pub struct BuildCmdOptions {
    pub cmd: String,
    pub env: Option<BTreeMap<String, String>>,
    pub cwd: Option<String>,
}
```

### Placeholder System

Both `fetch_url` and `cmd` return placeholder strings that reference action outputs:

- `${action:N}` - stdout of action at index N within the same build

These placeholders are resolved during execution when action outputs become available.

```lua
apply = function(inputs, ctx)
  local archive = ctx:fetch_url(inputs.src.url, inputs.src.sha256)
  -- archive is "${action:0}"
  ctx:cmd({ cmd = "tar -xzf " .. archive .. " -C /build" })
  -- The command contains "${action:0}" which resolves to the downloaded file path
end
```

## Examples

### Prebuilt Binary

```lua
local hashes = {
  ['aarch64-darwin'] = 'abc...',
  ['x86_64-linux'] = 'def...',
}

local ripgrep = sys.build({
  name = 'ripgrep',
  version = '15.1.0',

  inputs = function()
    return {
      src = {
        url = 'https://github.com/BurntSushi/ripgrep/releases/download/15.1.0/ripgrep-15.1.0-' .. sys.platform .. '.tar.gz',
        sha256 = hashes[sys.platform],
      },
    }
  end,

  apply = function(inputs, ctx)
    local archive = ctx:fetch_url(inputs.src.url, inputs.src.sha256)
    ctx:cmd({ cmd = "tar -xzf " .. archive .. " -C " .. ctx.outputs.out })
  end,
})
```

### Build from Source

```lua
local rust = sys.build({ name = 'rust', ... })

local ripgrep = sys.build({
  name = 'ripgrep',
  version = '15.1.0',

  inputs = function()
    return {
      git_url = 'https://github.com/BurntSushi/ripgrep',
      rev = '15.1.0',
      sha256 = 'source-hash...',
      rust = rust,
    }
  end,

  apply = function(inputs, ctx)
    ctx:cmd({
      cmd = 'git clone --depth 1 --branch ' .. inputs.rev .. ' ' .. inputs.git_url .. ' /tmp/rg-src',
    })

    ctx:cmd({
      cmd = 'cargo build --release',
      cwd = '/tmp/rg-src',
      env = { PATH = inputs.rust.outputs.out .. '/bin:' .. os.getenv('PATH') },
    })

    ctx:cmd({ cmd = 'mkdir -p ' .. ctx.outputs.out .. '/bin' })
    ctx:cmd({ cmd = 'cp /tmp/rg-src/target/release/rg ' .. ctx.outputs.out .. '/bin/rg' })
  end,
})
```

### Platform-Specific Build Logic

```lua
sys.build({
  name = 'my-tool',

  inputs = function()
    return {
      url = 'https://example.com/my-tool-' .. sys.platform .. '.tar.gz',
      sha256 = hashes[sys.platform],
    }
  end,

  apply = function(inputs, ctx)
    local archive = ctx:fetch_url(inputs.url, inputs.sha256)
    ctx:cmd({ cmd = 'tar -xzf ' .. archive .. ' -C ' .. ctx.outputs.out })

    if sys.os == 'darwin' then
      -- macOS-specific post-processing
      ctx:cmd({
        cmd = 'install_name_tool -id @rpath/libfoo.dylib ' .. ctx.outputs.out .. '/lib/libfoo.dylib'
      })
    elseif sys.os == 'linux' then
      -- Linux-specific
      ctx:cmd({
        cmd = "patchelf --set-rpath '$ORIGIN' " .. ctx.outputs.out .. '/lib/libfoo.so'
      })
    end
  end,
})
```

## File and Env Builds

Every `lib.file.setup()` and `lib.env.setup()` declaration internally creates a build:

### File Builds

```lua
-- User writes:
lib.file.setup({ path = '~/.gitconfig', source = './dotfiles/gitconfig' })

-- Internally becomes:
local file_build = sys.build({
  name = 'file-gitconfig',
  inputs = { source = './dotfiles/gitconfig' },
  apply = function(inputs, ctx)
    ctx:cmd({ cmd = 'cp ' .. inputs.source .. ' ' .. ctx.outputs.out .. '/content' })
  end,
})

sys.bind({
  inputs = { build = file_build, target = '~/.gitconfig' },
  apply = function(inputs, ctx)
    ctx:cmd('ln -sf ' .. inputs.build.outputs.out .. '/content ' .. inputs.target)
  end,
  destroy = function(inputs, ctx)
    ctx:cmd('rm ' .. inputs.target)
  end,
})
```

### Env Builds

```lua
-- User writes:
lib.env.setup({ EDITOR = 'nvim', PAGER = 'less' })

-- Internally becomes:
local env_build = sys.build({
  name = 'env-editor-pager',
  inputs = { vars = { EDITOR = 'nvim', PAGER = 'less' } },
  apply = function(inputs, ctx)
    -- Generate shell-specific fragments
    ctx:cmd({
      cmd = 'echo \'export EDITOR="nvim"\nexport PAGER="less"\' > ' .. ctx.outputs.out .. '/env.sh'
    })
    ctx:cmd({
      cmd = 'echo \'set -gx EDITOR "nvim"\nset -gx PAGER "less"\' > ' .. ctx.outputs.out .. '/env.fish'
    })
  end,
})

sys.bind({
  inputs = { build = env_build },
  apply = function(inputs, ctx)
    -- Shell integration handles sourcing these files
  end,
})
```

## Benefits of Unified Build Model

| Aspect                 | Direct Management | Build-Based               |
| ---------------------- | ----------------- | ------------------------- |
| Content deduplication  | None              | Automatic                 |
| Rollback               | Manual tracking   | Free via generations      |
| Reproducibility        | Best-effort       | Guaranteed                |
| Atomic apply           | Complex           | Natural                   |
| Cross-file consistency | Must coordinate   | Store ensures consistency |

## Related Documentation

- [02-binds.md](./02-binds.md) - What to do with build outputs
- [03-store.md](./03-store.md) - Where builds are realized
- [08-apply-flow.md](./08-apply-flow.md) - How builds are executed during apply
