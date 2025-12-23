# Builds

> **Core Principle:** Builds are the sole primitive for producing store content in SysLua.

A build is an immutable description of:

- What inputs are needed (arbitrary data)
- How to transform those inputs into outputs (config function)
- What outputs are produced

All managed state in SysLua uses builds - not just packages, but also files and environment variables.

## The `sys.build()` Function

```lua
local my_build = sys.build({
  id = "ripgrep-15.1.0",         -- Optional: identifier for debugging/logging

  inputs = <table | function()>,  -- Optional: input specification
  create = function(inputs, ctx), -- Required: build logic
})
```

## Inputs (`inputs`)

Inputs can be a static table or a function for platform-specific resolution. **Inputs are arbitrary data** - there is no magic interpretation. The `create` function consumes this data and uses `ctx` helpers as needed.

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
local rust = sys.build({ id = 'rust', ... })

sys.build({
  id = 'ripgrep',
  inputs = function()
    return {
      src_url = '...',
      rust = rust, -- Build reference
    }
  end,
  create = function(inputs, ctx)
    -- inputs.rust.outputs.out is the realized output path of the rust build
    ctx:exec({
      bin = 'cargo',
      args = { 'build', '--release' },
      env = { PATH = inputs.rust.outputs.out .. '/bin:' .. os.getenv('PATH') },
    })
  end,
})
```

## Create Function

The create function transforms inputs into outputs:

```lua
create = function(inputs, ctx)
  -- inputs: the table returned by inputs function (build refs have .outputs paths)
  -- ctx: build context with helpers
end
```

## Build Context (`BuildCtx`)

The build context provides actions for fetching, file writing, and shell execution. Each action returns an opaque string that can be stored and used in subsequent commands.

```lua
-- Fetch operations (returns opaque reference to downloaded file)
ctx:fetch_url(url, sha256) -- Download file, verify hash

-- Shell execution (returns opaque reference to stdout)
ctx:exec(opts) -- Execute a command
-- opts: string | { bin, args?, env?, cwd? }
```

### The `exec` Action

The `exec` action is the primary mechanism for executing operations during a build. This flexible approach allows Lua configuration to specify platform-specific commands rather than relying on preset Rust-backed actions:

```lua
-- Simple command (string) - bin only, no args
ctx:exec('make')

-- Command with options (table)
ctx:exec({
  bin = 'make',
  args = { 'install' },
  cwd = '/build/src',
  env = { PREFIX = ctx.out },
})
```

**ExecOpts:**

| Field  | Type                  | Description                                     |
| ------ | --------------------- | ----------------------------------------------- |
| `bin`  | string                | Required: the binary/command to execute         |
| `args` | string[]?             | Optional: arguments to pass to the command      |
| `cwd`  | string?               | Optional: working directory for the command     |
| `env`  | table<string,string>? | Optional: environment variables for the command |

**Why `exec` instead of preset actions?**

- **Platform flexibility**: Lua config decides what commands to run per platform
- **No Rust changes needed**: Adding new operations doesn't require Rust code changes
- **Transparent**: Users can see exactly what commands will be executed
- **Composable**: Complex operations built from simple shell commands

**Error handling:** All `ctx` operations throw on failure (Lua `error()`). A failed build leaves the user-facing system unchanged - atomic apply semantics ensure the pre-apply state is restored.

## Build Return Value

`sys.build {}` returns a table representing the build AND registers it globally. The registration happens on require - users can conditionally require modules for platform-specific packages.

```lua
local rg = sys.build { id = "ripgrep", outputs = {"out"}, ... }

rg.id             -- "ripgrep" or nil
rg.hash           -- Build hash (computed at evaluation time)
rg.outputs        -- { out = <realized-store-output-path> }
```

## Build Hashing

The build hash is a 20-character truncated SHA-256, computed from the serialized `BuildDef`:

- `id` (if present)
- `inputs` (evaluated `BuildInputs` - see below)
- `create_actions` (the commands and fetch operations)
- `outputs` (if present)

This means:

- Same inputs + different actions = different build
- Same build on different platforms = different hash (if inputs differ)
- Build dependencies are included via their hash in inputs
- Action order matters - same actions in different order = different hash

### BuildInputs and Build Dependencies

When a build references another build in its inputs, the `BuildInputs` stores only the referenced build's hash (not the full definition). This ensures:

- **Efficient hashing**: Build hashes depend on dependency hashes, not full definitions
- **Deduplication**: Same dependency = same hash regardless of how it's referenced
- **Clean serialization**: No circular references or duplicate data

```rust
/// Evaluated inputs (serializable)
pub enum BuildInputs {
    String(String),
    Number(f64),
    Boolean(bool),
    Table(BTreeMap<String, BuildInputs>),
    Array(Vec<BuildInputs>),
    Build(ObjectHash),  // Just the 20-char hash, not full BuildRef
}
```

## Rust Types

The build system uses a two-tier type architecture:

- **Spec** - Lua-side, contains closures, not serializable
- **Def** - Evaluated, serializable, stored in Manifest (keyed by truncated hash)

```rust
/// Actions that can be performed during a build
pub enum Action {
    FetchUrl { url: String, sha256: String },
    Exec(ExecOpts),
}

/// Command options for build and bind actions
pub struct ExecOpts {
    pub bin: String,
    pub args: Option<Vec<String>>,
    pub env: Option<BTreeMap<String, String>>,
    pub cwd: Option<String>,
}

/// Evaluated definition (serializable, stored in Manifest)
pub struct BuildDef {
    pub id: Option<String>,
    pub inputs: Option<BuildInputs>,
    pub outputs: Option<BTreeMap<String, String>>,
    pub create_actions: Vec<Action>,
}

impl BuildDef {
    /// Compute the truncated hash for use as manifest key.
    pub fn compute_hash(&self) -> Result<ObjectHash, serde_json::Error>;
}

/// Build context provided to create function
pub struct BuildCtx {
    actions: Vec<Action>,
}

impl BuildCtx {
    /// Returns a placeholder that resolves to the build's output directory
    pub fn out(&self) -> &'static str;

    /// Fetch a URL with hash verification, returns an opaque reference
    /// that resolves to the downloaded file path at execution time
    pub fn fetch_url(&mut self, url: &str, sha256: &str) -> String;

    /// Execute a command, returns an opaque reference
    /// that resolves to the command's stdout at execution time
    pub fn exec(&mut self, opts: impl Into<ExecOpts>) -> String;

    /// Consume context and return accumulated actions
    pub fn into_actions(self) -> Vec<Action>;
}
```

Note: `BuildRef` is not a separate Rust struct - it's a Lua table with a metatable that provides the build's `id`, `hash`, `inputs`, and `outputs` fields.

### Placeholder System

Both `fetch_url`, and `exec` return opaque strings that can be stored in variables and used in subsequent commands. These are resolved during execution when action outputs become available.

```lua
create = function(inputs, ctx)
  -- fetch_url returns an opaque reference to the downloaded file
  local archive = ctx:fetch_url(inputs.src.url, inputs.src.sha256)

  -- Use the reference in the next command - it resolves to the actual path at runtime
  ctx:exec({ bin = 'tar', args = { '-xzf', archive, '-C', '/build' } })
end
```

**Important:** Users never write placeholder syntax directly. The return values from context methods handle this automatically. Shell variables like `$HOME` and `$PATH` work normally in command strings.

## Examples

### Prebuilt Binary

```lua
local hashes = {
  ['aarch64-darwin'] = 'abc...',
  ['x86_64-linux'] = 'def...',
}

local ripgrep = sys.build({
  id = 'ripgrep-15.1.0',

  inputs = function()
    return {
      src = {
        url = 'https://github.com/BurntSushi/ripgrep/releases/download/15.1.0/ripgrep-15.1.0-'
          .. sys.platform
          .. '.tar.gz',
        sha256 = hashes[sys.platform],
      },
    }
  end,

  create = function(inputs, ctx)
    local archive = ctx:fetch_url(inputs.src.url, inputs.src.sha256)
    ctx:exec({ bin = 'tar', args = { '-xzf', archive, '-C', ctx.out } })
    return { out = ctx.out }
  end,
})
```

### Build from Source

```lua
local rust = sys.build({ id = 'rust', ... })

local ripgrep = sys.build({
  id = 'ripgrep-15.1.0',

  inputs = function()
    return {
      git_url = 'https://github.com/BurntSushi/ripgrep',
      rev = '15.1.0',
      sha256 = 'source-hash...',
      rust = rust,
    }
  end,

  create = function(inputs, ctx)
    ctx:exec({
      bin = 'git',
      args = { 'clone', '--depth', '1', '--branch', inputs.rev, inputs.git_url, '/tmp/rg-src' },
    })

    ctx:exec({
      bin = 'cargo',
      args = { 'build', '--release' },
      cwd = '/tmp/rg-src',
      env = { PATH = inputs.rust.outputs.out .. '/bin:' .. os.getenv('PATH') },
    })

    ctx:exec({ bin = 'mkdir', args = { '-p', ctx.out .. '/bin' } })
    ctx:exec({ bin = 'cp', args = { '/tmp/rg-src/target/release/rg', ctx.out .. '/bin/rg' } })
    return { out = ctx.out }
  end,
})
```

### Platform-Specific Build Logic

```lua
sys.build({
  id = 'my-tool',

  inputs = function()
    return {
      url = 'https://example.com/my-tool-' .. sys.platform .. '.tar.gz',
      sha256 = hashes[sys.platform],
    }
  end,

  create = function(inputs, ctx)
    local archive = ctx:fetch_url(inputs.url, inputs.sha256)
    ctx:exec({ bin = 'tar', args = { '-xzf', archive, '-C', ctx.out } })

    if sys.os == 'darwin' then
      -- macOS-specific post-processing
      ctx:exec({
        bin = 'install_name_tool',
        args = { '-id', '@rpath/libfoo.dylib', ctx.out .. '/lib/libfoo.dylib' },
      })
    elseif sys.os == 'linux' then
      -- Linux-specific
      ctx:exec({
        bin = 'patchelf',
        args = { '--set-rpath', '$ORIGIN', ctx.out .. '/lib/libfoo.so' },
      })
    end

    return { out = ctx.out }
  end,
})
```

## File and Env Builds

Every `lib.file.setup()` and `lib.env.setup()` declaration internally creates a build:

> **Note:** To use `lib.file` and `lib.env`, you must first `require('syslua.modules')`.

### File Builds

```lua
local modules = require('syslua.modules')

-- User writes:
modules.file.setup({ path = '~/.gitconfig', source = './dotfiles/gitconfig' })

-- Internally becomes:
local file_build = sys.build({
  id = 'file-gitconfig',
  inputs = { source = './dotfiles/gitconfig' },
  create = function(inputs, ctx)
    ctx:exec({ bin = 'cp', args = { inputs.source, ctx.out .. '/content' } })
    return { out = ctx.out }
  end,
})

sys.bind({
  inputs = { build = file_build, target = '~/.gitconfig' },
  create = function(inputs, ctx)
    ctx:exec({ bin = 'ln', args = { '-sf', inputs.build.outputs.out .. '/content', inputs.target } })
  end,
  destroy = function(outputs, ctx)
    ctx:exec({ bin = 'rm', args = { outputs.target } })
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
