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
│  Cached by hash in store/obj/<id>-<hash>/                       │
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

Binds follow the same `inputs`/`create` pattern as builds, with required `destroy` and optional `update` functions:

```lua
sys.bind({
  id = 'my-bind', -- Optional (required if using update)
  inputs = function()
    return { ... } -- Any data needed by create/update/destroy
  end,
  create = function(inputs, ctx) -- Required: initial creation
    ctx:exec({
      bin = '/bin/ln',
      args = { '-sf', inputs.source, inputs.target },
    })
    return { path = inputs.target } -- Optional: outputs for destroy/update
  end,
  destroy = function(outputs, ctx) -- Required: cleanup
    ctx:exec({
      bin = '/bin/rm',
      args = { outputs.path },
    })
  end,
})
```

## Bind Context (`BindCtx`)

The bind context provides actions for executing system modifications. Each action returns a placeholder string for referencing its output.

```lua
---@class BindCtx

-- Execute a command, returns an opaque reference to stdout
---@field exec fun(opts: ExecOpts | string, args?: string[]): string

-- The output directory (placeholder)
---@field out string
```

### The `exec` Action

The `exec` action is the primary mechanism for executing operations during a bind:

```lua
-- Simple command (string)
ctx:exec('ln -s /src /dest')

-- Command with binary and args (recommended)
ctx:exec({
  bin = '/bin/ln',
  args = { '-sf', '/src', '/dest' },
})

-- Command with environment and working directory
ctx:exec({
  bin = '/usr/bin/npm',
  args = { 'install', '-g', 'some-package' },
  env = { HOME = os.getenv('HOME') },
  cwd = '/some/path',
})
```

**ExecOpts:**

| Field  | Type                  | Description                                     |
| ------ | --------------------- | ----------------------------------------------- |
| `bin`  | string                | Required: path to the binary to execute         |
| `args` | string[]?             | Optional: arguments to pass                     |
| `cwd`  | string?               | Optional: working directory for the command     |
| `env`  | table<string,string>? | Optional: environment variables for the command |

**Why `create`/`destroy` instead of `undo_cmd`?**

- **Clear separation**: Create and destroy logic are distinct functions
- **Access to outputs**: Destroy function receives outputs from create
- **Flexibility**: Destroy can have different commands than a simple reversal
- **Composability**: Multiple destroy actions can be added independently

## Rust Types

The bind system uses a two-tier type architecture:

- **Spec** - Lua-side, contains closures, not serializable
- **Def** - Evaluated, serializable, stored in Manifest (keyed by truncated hash)

```rust
/// Hash for content-addressing binds (20-char truncated SHA-256)
pub struct ObjectHash(pub String);

/// Actions that can be performed during a bind
/// Note: Unlike builds, binds do not support FetchUrl - they should only
/// use outputs from builds rather than fetching content directly.
pub enum Action {
    Exec {
        bin: String,
        args: Option<Vec<String>>,
        env: Option<BTreeMap<String, String>>,
        cwd: Option<String>,
    },
}

/// Evaluated definition (serializable, stored in Manifest)
pub struct BindDef {
    pub id: Option<String>,
    pub inputs: Option<BindInputs>,
    pub outputs: Option<BTreeMap<String, String>>,
    pub create_actions: Vec<Action>,
    pub update_actions: Option<Vec<Action>>,
    pub destroy_actions: Vec<Action>,
}

impl BindDef {
    /// Compute the truncated hash for use as manifest key.
    pub fn compute_hash(&self) -> Result<ObjectHash, serde_json::Error>;
}
```

Note: `BindRef` is not a separate Rust struct - it's a Lua table with a metatable that provides the bind's `hash`, `inputs`, and `outputs` fields.

### Placeholder System

The `exec` method returns an opaque string that can be stored and used later. This allows destroy actions to reference values captured during create:

```lua
create = function(inputs, ctx)
  -- exec returns an opaque reference to the command's stdout
  local container_id = ctx:exec("docker run -d postgres")
  -- Return it as an output so destroy can access it
  return { container = container_id }
end,
destroy = function(outputs, ctx)
  -- outputs.container resolves to the actual container ID at runtime
  ctx:exec("docker stop " .. outputs.container)
end
```

**Important:** Users never write placeholder syntax directly. The return values from context methods handle this automatically. Shell variables like `$HOME` work normally in command strings.

## The Update Callback

> **Warning:** The `update` callback does NOT have full rollback support and is inherently dangerous. If an update fails partway through, the bind may be left in an inconsistent state. **Use `create` and `destroy` when possible.**

The `update` callback allows in-place modification of a bind when its inputs change, without going through the destroy+create cycle. This is useful for cases where destroying and recreating would cause unnecessary disruption.

### When Update Is Used

Update is triggered when ALL of these conditions are met:

1. The bind has an `id` field
2. A bind with the same `id` exists in the current state
3. The bind's hash has changed (inputs, actions, or configuration differ)
4. The **new** bind definition has `update_actions` defined

If any condition is not met, the system falls back to destroy+create.

### Update Signature

```lua
sys.bind({
  id = 'my-service-link', -- REQUIRED when using update
  inputs = function()
    return { version = '2.0' }
  end,
  create = function(inputs, ctx)
    ctx:exec({
      bin = '/bin/ln',
      args = { '-sf', '/apps/' .. inputs.version, '/current' },
    })
    return { link = '/current' }
  end,
  update = function(outputs, inputs, ctx)
    -- outputs: the outputs from the PREVIOUS create (or update)
    -- inputs: the NEW inputs
    -- ctx: action context
    ctx:exec({ bin = '/bin/rm', args = { outputs.link } })
    ctx:exec({
      bin = '/bin/ln',
      args = { '-sf', '/apps/' .. inputs.version, '/current' },
    })
    return { link = '/current' } -- MUST return same keys as create
  end,
  destroy = function(outputs, ctx)
    ctx:exec({ bin = '/bin/rm', args = { outputs.link } })
  end,
})
```

### Update Requirements

| Requirement       | Description                                                |
| ----------------- | ---------------------------------------------------------- |
| `id` required     | The bind must have an `id` field to enable update tracking |
| Same output keys  | `update` must return the same output keys as `create`      |
| Outputs parameter | First parameter is outputs from previous create/update     |
| Inputs parameter  | Second parameter is the new inputs                         |

### Update Limitations

**No Automatic Rollback:** If `update` fails partway through, the bind is left in whatever state the failed actions left it. Unlike create failures (which trigger destroy of the partially-created bind), update failures have no automatic recovery.

**State Tracking:** The old bind state is only removed after a successful update. If update fails, the old state file remains, but the actual system state may be corrupted.

**Recommendation:** Only use `update` when:

- The destroy+create cycle would cause significant disruption (e.g., service downtime)
- The update operation is simple and unlikely to fail partway through
- You have manual recovery procedures in case of failure

### Example: When to Use Update

**Good use case - atomic file swap:**

```lua
update = function(outputs, inputs, ctx)
  -- Atomic: either succeeds completely or fails before changing anything
  ctx:exec({
    bin = '/bin/ln',
    args = { '-sfn', inputs.new_target, outputs.link },
  })
  return { link = outputs.link }
end
```

**Bad use case - multi-step process:**

```lua
update = function(outputs, inputs, ctx)
  -- DANGEROUS: if step 2 fails, step 1 has already modified state
  ctx:exec({ bin = 'stop-service', args = { outputs.service } }) -- Step 1
  ctx:exec({ bin = 'update-config', args = { inputs.config } }) -- Step 2 (might fail!)
  ctx:exec({ bin = 'start-service', args = { outputs.service } }) -- Step 3
  return { service = outputs.service }
end
```

For the "bad" example above, prefer destroy+create which has proper rollback support.

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
  id = 'ripgrep',
  inputs = function()
    return { url = '...', sha256 = '...' }
  end,
  create = function(inputs, ctx)
    local archive = ctx:fetch_url(inputs.url, inputs.sha256)
    ctx:exec({
      bin = '/bin/tar',
      args = { '-xzf', archive, '-C', ctx.out },
    })
    return { out = ctx.out }
  end,
})

sys.bind({
  inputs = function()
    return { build = rg_build }
  end,
  create = function(inputs, ctx)
    ctx:exec({
      bin = '/bin/ln',
      args = { '-sf', inputs.build.outputs.out .. '/bin/rg', '/usr/local/bin/rg' },
    })
    return { link = '/usr/local/bin/rg' }
  end,
  destroy = function(outputs, ctx)
    ctx:exec({ bin = '/bin/rm', args = { outputs.link } })
  end,
})
```

### File Management

```lua
require('syslua.modules.file').setup({ target = '~/.gitconfig', source = './dotfiles/gitconfig' })
```

Internally creates:

- **Build**: Copies source content to store
- **Bind**: Creates symlink to target location

```lua
-- What happens internally:
local file_build = sys.build({
  id = 'file-gitconfig',
  inputs = function()
    return { source = './dotfiles/gitconfig' }
  end,
  create = function(inputs, ctx)
    ctx:exec({
      bin = '/bin/cp',
      args = { inputs.source, ctx.out .. '/content' },
    })
    return { out = ctx.out }
  end,
})

sys.bind({
  inputs = function()
    return { build = file_build, target = '~/.gitconfig' }
  end,
  create = function(inputs, ctx)
    ctx:exec({
      bin = '/bin/ln',
      args = { '-sf', inputs.build.outputs.out .. '/content', inputs.target },
    })
    return { link = inputs.target }
  end,
  destroy = function(outputs, ctx)
    ctx:exec({ bin = '/bin/rm', args = { outputs.link } })
  end,
})
```

## Examples

### Simple Package Bind

```lua
sys.bind({
  inputs = function()
    return { build = ripgrep_build }
  end,
  create = function(inputs, ctx)
    ctx:exec({
      bin = '/bin/ln',
      args = { '-sf', inputs.build.outputs.out .. '/bin/rg', '/usr/local/bin/rg' },
    })
    return { link = '/usr/local/bin/rg' }
  end,
  destroy = function(outputs, ctx)
    ctx:exec({ bin = '/bin/rm', args = { outputs.link } })
  end,
})
```

### Multiple Binds from Same Build

```lua
local my_tool = sys.build({ id = 'my-tool', ... })

-- Add to PATH
sys.bind({
  inputs = function()
    return { build = my_tool }
  end,
  create = function(inputs, ctx)
    ctx:exec({
      bin = '/bin/ln',
      args = { '-sf', inputs.build.outputs.out .. '/bin/mytool', '/usr/local/bin/mytool' },
    })
    return { link = '/usr/local/bin/mytool' }
  end,
  destroy = function(outputs, ctx)
    ctx:exec({ bin = '/bin/rm', args = { outputs.link } })
  end,
})

-- Also create symlinks for shared resources
sys.bind({
  inputs = function()
    return { build = my_tool }
  end,
  create = function(inputs, ctx)
    ctx:exec({
      bin = '/bin/ln',
      args = {
        '-sf',
        inputs.build.outputs.out .. '/share/man/man1/mytool.1',
        sys.path.join(os.getenv('HOME'), '.local/share/man/man1/mytool.1'),
      },
    })
    return { man_link = sys.path.join(os.getenv('HOME'), '.local/share/man/man1/mytool.1') }
  end,
  destroy = function(outputs, ctx)
    ctx:exec({ bin = '/bin/rm', args = { outputs.man_link } })
  end,
})
```

### Platform-Specific Bind

```lua
sys.bind({
  inputs = function()
    return { build = neovim_build }
  end,
  create = function(inputs, ctx)
    ctx:exec({
      bin = '/bin/ln',
      args = { '-sf', inputs.build.outputs.out .. '/bin/nvim', '/usr/local/bin/nvim' },
    })

    if sys.os == 'darwin' then
      ctx:exec({
        bin = '/bin/ln',
        args = {
          '-sf',
          inputs.build.outputs.out .. '/Applications/Neovim.app',
          sys.path.join(os.getenv('HOME'), 'Applications/Neovim.app'),
        },
      })
    end

    return { bin_link = '/usr/local/bin/nvim' }
  end,
  destroy = function(outputs, ctx)
    ctx:exec({ bin = '/bin/rm', args = { outputs.bin_link } })

    if sys.os == 'darwin' then
      ctx:exec({ bin = '/bin/rm', args = { sys.path.join(os.getenv('HOME'), 'Applications/Neovim.app') } })
    end
  end,
})
```

### macOS Defaults

```lua
sys.bind({
  create = function(inputs, ctx)
    if sys.os == 'darwin' then
      ctx:exec({
        bin = '/usr/bin/defaults',
        args = { 'write', 'com.apple.finder', 'AppleShowAllFiles', '-bool', 'true' },
      })
      ctx:exec({ bin = '/usr/bin/killall', args = { 'Finder' } })
    end
    return {}
  end,
  destroy = function(outputs, ctx)
    if sys.os == 'darwin' then
      ctx:exec({
        bin = '/usr/bin/defaults',
        args = { 'write', 'com.apple.finder', 'AppleShowAllFiles', '-bool', 'false' },
      })
      ctx:exec({ bin = '/usr/bin/killall', args = { 'Finder' } })
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
  create = function(inputs, ctx)
    if sys.os == 'linux' then
      ctx:exec({
        bin = '/bin/ln',
        args = { '-sf', inputs.service_build.outputs.out .. '/nginx.service', '/etc/systemd/system/nginx.service' },
      })
      ctx:exec({
        bin = '/bin/systemctl',
        args = { 'daemon-reload' },
      })
      ctx:exec({
        bin = '/bin/systemctl',
        args = { 'enable', '--now', 'nginx' },
      })
    elseif sys.os == 'darwin' then
      ctx:exec({
        bin = '/bin/ln',
        args = {
          '-sf',
          inputs.service_build.outputs.out .. '/nginx.plist',
          sys.path.join(os.getenv('HOME'), 'Library/LaunchAgents/nginx.plist'),
        },
      })
      ctx:exec({
        bin = '/bin/launchctl',
        args = { 'load', sys.path.join(os.getenv('HOME'), 'Library/LaunchAgents/nginx.plist') },
      })
    end
    return {}
  end,
  destroy = function(outputs, ctx)
    if sys.os == 'linux' then
      ctx:exec({ bin = '/bin/systemctl', args = { 'disable', '--now', 'nginx' } })
      ctx:exec({ bin = '/bin/rm', args = { '/etc/systemd/system/nginx.service' } })
      ctx:exec({ bin = '/bin/systemctl', args = { 'daemon-reload' } })
    elseif sys.os == 'darwin' then
      ctx:exec({
        bin = '/bin/launchctl',
        args = { 'unload', sys.path.join(os.getenv('HOME'), 'Library/LaunchAgents/nginx.plist') },
      })
      ctx:exec({ bin = '/bin/rm', args = { sys.path.join(os.getenv('HOME'), 'Library/LaunchAgents/nginx.plist') } })
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

/// Manifest keyed by truncated hash (20 chars)
pub struct Manifest {
    pub builds: BTreeMap<ObjectHash, BuildDef>,
    pub bindings: BTreeMap<ObjectHash, BindDef>,
}
```

**Benefits:**

1. **No separate types**: No need for `SnapshotFile`, `SnapshotEnv`, `SnapshotBuild` - just build defs and bind defs in a manifest
2. **Clear diffs**: Comparing snapshots shows exactly what changed:
   - Same builds, different binds = only deployment changed
   - Different builds, same binds = content changed
3. **GC-safe**: Builds referenced by any snapshot are protected from garbage collection
4. **Future-proof**: New bind patterns slot in naturally via `exec`

## Rollback with `destroy_actions`

When a bind has `destroy_actions`, the system can cleanly rollback:

```
Apply begins
    │
    ├─► Bind 1: create_actions=[cmd1, cmd2], destroy_actions=[undo1, undo2] ✓
    ├─► Bind 2: create_actions=[cmd3], destroy_actions=[undo3] ✓
    ├─► Bind 3: create_actions=[cmd4] ✗ FAILS
    │
    └─► Rollback triggered
            │
            ├─► Execute Bind 2's destroy_actions in reverse
            └─► Execute Bind 1's destroy_actions in reverse
```

The `destroy` function in Lua is evaluated at plan time, just like `create`. This means:

- Destroy actions are recorded in the manifest, not computed at rollback time
- Destroy actions can reference outputs from create via the outputs parameter
- Rollback is deterministic and doesn't require re-evaluating Lua

**Note:** Binds with `update_actions` do NOT have automatic rollback. If an update fails, the bind may be left in an inconsistent state. See [The Update Callback](#the-update-callback) for details.

## The Check Callback (Drift Detection)

The optional `check` callback enables drift detection for binds. It allows you to verify that the system state still matches what the bind created, without re-running the full create/destroy cycle.

### When to Use Check

Use the `check` callback when:

- The bind creates state that can be externally modified (files, symlinks, config)
- You want to detect if someone manually changed what the bind manages
- You need to verify system state matches expected state during `sys apply`

### Check Signature

```lua
sys.bind({
  id = 'my-file-link',
  inputs = function()
    return { source = '/store/content', target = '~/.config/myapp' }
  end,
  create = function(inputs, ctx)
    ctx:exec({
      bin = '/bin/ln',
      args = { '-sf', inputs.source, inputs.target },
    })
    return { link = inputs.target }
  end,
  check = function(outputs, ctx)
    -- outputs: the outputs from create (or update)
    -- ctx: action context for verification commands
    local result = ctx:exec({
      bin = '/bin/test',
      args = { '-L', outputs.link },
    })
    -- Return table with drifted status and optional message
    return { drifted = (result ~= '0'), message = 'symlink missing or broken' }
  end,
  destroy = function(outputs, ctx)
    ctx:exec({ bin = '/bin/rm', args = { outputs.link } })
  end,
})
```

### Check Callback Parameters

| Parameter | Description                                              |
| --------- | -------------------------------------------------------- |
| `outputs` | The outputs from the last successful create/update       |
| `ctx`     | Action context for running verification commands         |

### Check Return Value

The check callback must return a table with:

| Field     | Type    | Description                                         |
| --------- | ------- | --------------------------------------------------- |
| `drifted` | boolean | `true` if system state doesn't match expected state |
| `message` | string? | Optional: explanation of what drifted               |

### How Drift Detection Works

1. During `sys apply`, after applying changes, the system checks **unchanged binds** (binds that exist in both old and new state with the same hash)
2. For each unchanged bind with a `check` callback, the check actions are executed
3. The `drifted` field from the return value indicates whether drift was detected
4. Drift results are reported in the apply summary

### Repair Mode

When drift is detected, you can repair it using the `--repair` flag:

```bash
# Detect drift only (default)
$ sys apply init.lua
# Output: Drift detected: 2 bind(s)
#         Run with --repair to fix drifted binds

# Repair drifted binds
$ sys apply init.lua --repair
# Output: Binds repaired: 2
```

Repair works by re-executing the `create_actions` for drifted binds, effectively recreating the expected state.

### Check Does Not Affect Hash

**Important:** The `check` callback and its actions are intentionally **excluded from the bind hash calculation**. This means:

- Adding or modifying a `check` callback does not cause the bind to be re-applied
- The check is purely for monitoring/verification, not for determining bind identity
- Two binds with identical create/update/destroy but different check callbacks have the same hash

### Rust Types

```rust
/// Result of checking a bind for drift
pub struct BindCheckResult {
    /// Whether the bind has drifted from expected state
    pub drifted: bool,
    /// Optional message explaining the drift
    pub message: Option<String>,
}

/// Evaluated bind definition (excerpt showing check fields)
pub struct BindDef {
    // ... other fields ...
    
    /// Actions to run during check (optional)
    pub check_actions: Option<Vec<Action>>,
    /// Expected outputs from check callback (optional)
    pub check_outputs: Option<BindCheckOutputs>,
}
```

### Example: File Existence Check

```lua
sys.bind({
  inputs = function()
    return { content = 'Hello World', path = '/tmp/myfile.txt' }
  end,
  create = function(inputs, ctx)
    ctx:exec({
      bin = '/bin/sh',
      args = { '-c', 'echo "' .. inputs.content .. '" > ' .. inputs.path },
    })
    return { file = inputs.path }
  end,
  check = function(outputs, ctx)
    -- Check if file exists and has expected content
    local exists = ctx:exec({
      bin = '/bin/test',
      args = { '-f', outputs.file },
    })
    return { drifted = (exists ~= '0'), message = 'file deleted or modified' }
  end,
  destroy = function(outputs, ctx)
    ctx:exec({ bin = '/bin/rm', args = { '-f', outputs.file } })
  end,
})
```

### Example: Symlink Verification

```lua
sys.bind({
  inputs = function()
    return { source = my_build.outputs.out, target = '/usr/local/bin/mytool' }
  end,
  create = function(inputs, ctx)
    ctx:exec({
      bin = '/bin/ln',
      args = { '-sf', inputs.source, inputs.target },
    })
    return { link = inputs.target, expected_target = inputs.source }
  end,
  check = function(outputs, ctx)
    -- Verify symlink exists and points to correct target
    local actual = ctx:exec({
      bin = '/bin/readlink',
      args = { outputs.link },
    })
    local drifted = (actual ~= outputs.expected_target)
    return { drifted = drifted, message = drifted and 'symlink target changed' or nil }
  end,
  destroy = function(outputs, ctx)
    ctx:exec({ bin = '/bin/rm', args = { outputs.link } })
  end,
})
```

## Related Documentation

- [01-builds.md](./01-builds.md) - How builds produce content
- [03-store.md](./03-store.md) - Where build outputs live
- [05-snapshots.md](./05-snapshots.md) - How binds enable rollback
