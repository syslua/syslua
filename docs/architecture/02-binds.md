# Binds

> **Core Principle:** Binds describe what to do with build outputs.

While builds are pure artifacts (content in the store), binds specify how those outputs should be made visible to the user and system.

This separation provides:

- **Better caching**: Same content with different targets = one build, multiple binds
- **Composability**: Future features (services, programs) use the same pattern
- **Cleaner rollback**: "Same builds, different binds" is a clear, understandable diff
- **Separation of concerns**: Build logic stays in builds; deployment logic in binds

## The Two Building Blocks

```
┌─────────────────────────────────────────────────────────────────┐
│  Build                                                          │
│  ═════                                                          │
│  Pure build artifact. Describes HOW to produce content.         │
│  Cached by hash in store/obj/<name>-<hash>/                     │
│  Immutable once built. Same inputs → same output.               │
└─────────────────────────────────────────────────────────────────┘
                              │
                              │ produces
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  Bind                                                           │
│  ════                                                           │
│  Describes WHAT TO DO with build output.                        │
│  Execute commands to modify system state.                       │
│  Multiple binds can reference the same build.                   │
└─────────────────────────────────────────────────────────────────┘
```

## The `sys.bind()` Function

Binds follow the same `inputs`/`apply` pattern as builds, with an optional `destroy` function for rollback:

```lua
sys.bind({
  inputs = function()
    return { ... }  -- Any data needed by apply/destroy functions
  end,
  apply = function(inputs, ctx)
    ctx:cmd({
      cmd = "...",
    })
  end,
  destroy = function(inputs, ctx)  -- Optional: for rollback support
    ctx:cmd({
      cmd = "...",
    })
  end,
})
```

## Bind Context (`BindCtx`)

The bind context provides a single, flexible action for executing system modifications. Each action returns a placeholder string for referencing its output.

```lua
---@class BindCtx

-- Execute a command, returns placeholder "${action:N}"
---@field cmd fun(opts: BindCmdOptions): string
```

### The `cmd` Action

The `cmd` action is the sole mechanism for executing operations during a bind. The `apply` function runs commands to create state, and the optional `destroy` function runs commands to reverse it:

```lua
-- Simple command (string)
ctx:cmd("ln -s /src /dest")

-- Command with environment and working directory
ctx:cmd({
  cmd = "npm install -g some-package",
  env = { HOME = os.getenv("HOME") },
  cwd = "/some/path",
})
```

**BindCmdOptions:**

| Field | Type | Description |
|-------|------|-------------|
| `cmd` | string | Required: the shell command to execute |
| `cwd` | string? | Optional: working directory for the command |
| `env` | table<string,string>? | Optional: environment variables for the command |

**Why `apply`/`destroy` instead of `undo_cmd`?**

- **Clear separation**: Apply and destroy logic are distinct functions
- **Access to outputs**: Destroy function can reference outputs from apply (via placeholders)
- **Flexibility**: Destroy can have different commands than a simple reversal
- **Composability**: Multiple destroy actions can be added independently

## Rust Types

The bind system uses a three-tier type architecture:

- **Spec** - Lua-side, contains closures, not serializable
- **Def** - Evaluated, serializable, stored in Manifest
- **Ref** - Content-addressed reference for cross-references

```rust
/// Hash for content-addressing binds
pub struct BindHash(pub String);

/// Actions that can be performed during a bind
pub enum BindAction {
    Cmd {
        cmd: String,
        env: Option<BTreeMap<String, String>>,
        cwd: Option<String>,
    },
}

/// Lua-side specification (not serializable, contains closures)
pub struct BindSpec {
    pub inputs: Option<InputsSpec>,
    pub apply: Function,           // Lua closure for creating state
    pub destroy: Option<Function>, // Lua closure for reversing state
}

/// Evaluated definition (serializable, stored in Manifest)
pub struct BindDef {
    pub inputs: Option<InputsRef>,
    pub apply_actions: Vec<BindAction>,
    pub outputs: Option<BTreeMap<String, String>>,
    pub destroy_actions: Option<Vec<BindAction>>,
}

/// Content-addressed reference (for cross-references in inputs)
pub struct BindRef {
    pub inputs: Option<InputsRef>,
    pub outputs: Option<HashMap<String, String>>,
    pub hash: BindHash,
}

/// Bind context provided to apply/destroy functions
pub struct BindCtx {
    actions: Vec<BindAction>,
}

impl BindCtx {
    /// Execute a command, returns placeholder "${action:N}"
    pub fn cmd(&mut self, opts: impl Into<BindCmdOptions>) -> String;
    
    /// Consume context and return accumulated actions
    pub fn into_actions(self) -> Vec<BindAction>;
}

/// Command options for bind actions
pub struct BindCmdOptions {
    pub cmd: String,
    pub env: Option<BTreeMap<String, String>>,
    pub cwd: Option<String>,
}
```

### Placeholder System

The `cmd` method returns a placeholder string that references the action's stdout:

- `${action:N}` - stdout of action at index N within the same bind

This allows destroy actions to reference values captured during apply:

```lua
apply = function(inputs, ctx)
  local container_id = ctx:cmd("docker run -d postgres")
  -- container_id is "${action:0}", stored in outputs
  return { container = container_id }
end,
destroy = function(inputs, ctx)
  -- inputs.container resolves to the actual container ID
  ctx:cmd("docker stop " .. inputs.container)
end
```

## How User APIs Map to Builds + Binds

### Package Installation

```lua
require('syslua.pkgs.cli.ripgrep').setup()
```

Internally creates:

- **Build**: Fetches and extracts the ripgrep binary
- **Bind**: Adds to PATH via shell integration

```lua
-- What the module does internally:
local rg_build = sys.build({
  name = 'ripgrep',
  version = '15.1.0',
  apply = function(inputs, ctx)
    local archive = ctx:fetch_url(inputs.url, inputs.sha256)
    ctx:cmd({ cmd = 'tar -xzf ' .. archive .. ' -C /build/out' })
  end,
})

sys.bind({
  inputs = { build = rg_build },
  apply = function(inputs, ctx)
    -- PATH integration is handled by shell scripts
    -- The bind registers this package for PATH inclusion
  end,
})
```

### `lib.file.setup()` - File Management

```lua
lib.file.setup({ path = '~/.gitconfig', source = './dotfiles/gitconfig' })
```

Internally creates:

- **Build**: Copies source content to store
- **Bind**: Creates symlink to target location

```lua
-- What happens internally:
local file_build = sys.build({
  name = 'file-gitconfig',
  apply = function(inputs, ctx)
    ctx:cmd({ cmd = 'cp ' .. inputs.source .. ' /build/out/content' })
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

### `lib.env.setup()` - Environment Variables

```lua
lib.env.setup({ EDITOR = 'nvim' })
```

Internally creates:

- **Build**: Generates shell fragments (`env.sh`, `env.fish`, `env.ps1`)
- **Bind**: Registers for shell sourcing

## Examples

### Simple Package Bind

```lua
sys.bind({
  inputs = function()
    return { build = ripgrep_build }
  end,
  apply = function(inputs, ctx)
    -- Create symlink to bin directory
    ctx:cmd('ln -sf ' .. inputs.build.outputs.out .. '/bin/rg /usr/local/bin/rg')
  end,
  destroy = function(inputs, ctx)
    ctx:cmd('rm /usr/local/bin/rg')
  end,
})
```

### Multiple Binds from Same Build

```lua
local my_tool = sys.build({ name = 'my-tool', ... })

-- Add to PATH
sys.bind({
  inputs = function()
    return { build = my_tool }
  end,
  apply = function(inputs, ctx)
    ctx:cmd('ln -sf ' .. inputs.build.outputs.out .. '/bin/mytool /usr/local/bin/mytool')
  end,
  destroy = function(inputs, ctx)
    ctx:cmd('rm /usr/local/bin/mytool')
  end,
})

-- Also create symlinks for shared resources
sys.bind({
  inputs = function()
    return { build = my_tool }
  end,
  apply = function(inputs, ctx)
    ctx:cmd('ln -sf ' .. inputs.build.outputs.out .. '/share/man/man1/mytool.1 ~/.local/share/man/man1/mytool.1')
    ctx:cmd('ln -sf ' .. inputs.build.outputs.out .. '/completions/_mytool ~/.zsh/completions/_mytool')
  end,
  destroy = function(inputs, ctx)
    ctx:cmd('rm ~/.local/share/man/man1/mytool.1')
    ctx:cmd('rm ~/.zsh/completions/_mytool')
  end,
})
```

### Platform-Specific Bind

```lua
sys.bind({
  inputs = function()
    return { build = neovim_build }
  end,
  apply = function(inputs, ctx)
    ctx:cmd('ln -sf ' .. inputs.build.outputs.out .. '/bin/nvim /usr/local/bin/nvim')

    if sys.os == 'darwin' then
      ctx:cmd('ln -sf ' .. inputs.build.outputs.out .. '/Applications/Neovim.app ~/Applications/Neovim.app')
    end
  end,
  destroy = function(inputs, ctx)
    ctx:cmd('rm /usr/local/bin/nvim')

    if sys.os == 'darwin' then
      ctx:cmd('rm ~/Applications/Neovim.app')
    end
  end,
})
```

### macOS Defaults

```lua
sys.bind({
  apply = function(inputs, ctx)
    if sys.os == 'darwin' then
      ctx:cmd('defaults write com.apple.finder AppleShowAllFiles -bool true')
      ctx:cmd('killall Finder')
    end
  end,
  destroy = function(inputs, ctx)
    if sys.os == 'darwin' then
      ctx:cmd('defaults write com.apple.finder AppleShowAllFiles -bool false')
      ctx:cmd('killall Finder')
    end
  end,
})
```

### Service Management

```lua
sys.bind({
  inputs = function()
    return { service_build = nginx_service_build }
  end,
  apply = function(inputs, ctx)
    if sys.os == 'linux' then
      ctx:cmd('ln -sf ' .. inputs.service_build.outputs.out .. '/nginx.service /etc/systemd/system/nginx.service')
      ctx:cmd('systemctl daemon-reload && systemctl enable nginx && systemctl start nginx')
    elseif sys.os == 'darwin' then
      ctx:cmd('ln -sf ' .. inputs.service_build.outputs.out .. '/nginx.plist ~/Library/LaunchAgents/nginx.plist')
      ctx:cmd('launchctl load ~/Library/LaunchAgents/nginx.plist')
    end
  end,
  destroy = function(inputs, ctx)
    if sys.os == 'linux' then
      ctx:cmd('systemctl stop nginx && systemctl disable nginx')
      ctx:cmd('rm /etc/systemd/system/nginx.service')
    elseif sys.os == 'darwin' then
      ctx:cmd('launchctl unload ~/Library/LaunchAgents/nginx.plist')
      ctx:cmd('rm ~/Library/LaunchAgents/nginx.plist')
    end
  end,
})
```

## Why This Matters for Snapshots

With builds and binds as separate concepts, snapshots become much simpler:

```rust
/// A snapshot captures system state as a manifest of builds and binds.
pub struct Snapshot {
    pub id: String,
    pub created_at: u64,
    pub config_path: Option<PathBuf>,
    pub manifest: Manifest,
}

pub struct Manifest {
    pub builds: Vec<BuildDef>,
    pub activations: Vec<BindDef>,  // Evaluated binds
}
```

**Benefits:**

1. **No separate types**: No need for `SnapshotFile`, `SnapshotEnv`, `SnapshotBuild` - just build defs and bind defs in a manifest
2. **Clear diffs**: Comparing snapshots shows exactly what changed:
   - Same builds, different binds = only deployment changed
   - Different builds, same binds = content changed
3. **GC-safe**: Builds referenced by any snapshot are protected from garbage collection
4. **Future-proof**: New bind patterns slot in naturally via `cmd`

## Rollback with `destroy_actions`

When a bind has `destroy_actions`, the system can cleanly rollback:

```
Apply begins
    │
    ├─► Bind 1: apply_actions=[cmd1, cmd2], destroy_actions=[undo1, undo2] ✓
    ├─► Bind 2: apply_actions=[cmd3], destroy_actions=[undo3] ✓
    ├─► Bind 3: apply_actions=[cmd4] ✗ FAILS
    │
    └─► Rollback triggered
            │
            ├─► Execute Bind 2's destroy_actions in reverse
            └─► Execute Bind 1's destroy_actions in reverse
```

The `destroy` function in Lua is evaluated at plan time, just like `apply`. This means:

- Destroy actions are recorded in the manifest, not computed at rollback time
- Destroy actions can reference outputs from apply via placeholders
- Rollback is deterministic and doesn't require re-evaluating Lua

## Related Documentation

- [01-builds.md](./01-builds.md) - How builds produce content
- [03-store.md](./03-store.md) - Where build outputs live
- [05-snapshots.md](./05-snapshots.md) - How binds enable rollback
