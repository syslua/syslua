# Store Design

> **Core Principle:** The store is the realization engine for builds.

Every object in `store/build/` is the output of realizing a build. Objects use content-addressed paths: `build/<hash>/` where hash is a 20-character truncated SHA-256.

The store provides:

- **Content addressing**: Objects are identified by their content hash
- **Immutability**: Once written, objects never change
- **Deduplication**: Same content → same hash → stored once
- **Caching**: Derivation hash → output hash mapping enables instant cache hits

## Store Locations

SysLua uses a multi-level store architecture:

### System Store (Managed by Admin/Root)

| Platform | System Store Path |
| -------- | ----------------- |
| Linux    | `/syslua/store`   |
| macOS    | `/syslua/store`   |
| Windows  | `C:\syslua\store` |

### User Store (Managed by Each User, No Sudo Required)

| Platform    | User Store Path               |
| ----------- | ----------------------------- |
| Linux/macOS | `~/.local/share/syslua/store` |
| Windows     | `%LOCALAPPDATA%\syslua\store` |

## System Store Layout

```
/syslua/store/
├── build/<hash>/                 # Realized build outputs (immutable, world-readable)
│   ├── bin/                      # Executables produced by the build
│   ├── lib/                      # Libraries (hash is 20-char truncated SHA-256)
│   └── ...
├── bind/<hash>/                  # Bind state tracking (20-char hash)
│   └── state.json                # Bind execution state
└── snapshots/
    ├── index.json                # Index of all snapshots
    └── <snapshot_id>.json        # Individual snapshot data
```

### Store Path Format

- Build path: `build/abc123def456789012/`
- Bind path: `bind/abc123def456789012/`
- Hash is 20 chars (truncated SHA-256, defined as `HASH_PREFIX_LEN` in `consts.rs`)

### Key Directories

| Directory    | Purpose                                                               |
| ------------ | --------------------------------------------------------------------- |
| `build/`     | **The actual store** - all build outputs live here                    |
| `bind/`      | Bind state tracking - execution state for each bind                   |
| `snapshots/` | State tracking - index and individual snapshot data                   |

## User Store Layout

```
~/.local/share/syslua/
├── store/
│   ├── build/<hash>/                 # User's build outputs (or hardlinks to system store)
│   ├── bind/<hash>/                  # User's bind state
│   │   └── state.json
│   └── snapshots/
│       ├── index.json                # User snapshot index
│       └── <snapshot_id>.json        # Individual snapshots
├── zshenv                            # Generated environment script (bash/zsh)
├── env.fish                          # Generated environment script (fish)
├── env.ps1                           # Generated environment script (PowerShell)
```

## Benefits of Multi-Level Store

- System packages installed once, shared by all users
- Users can hardlink to system packages (no duplication)
- Users can install additional packages without sudo
- System configuration protected from user modification
- User configurations independent of each other

## Store Realization

The store converts build descriptions into actual content:

```rust
impl Store {
    /// Realize a build, returning the store path of the output
    pub fn realize(&self, build: &BuildSpec) -> Result<StorePath> {
        // 1. Compute build hash (content-addressed)
        let build_hash = build.hash();

        // 2. Check if output already exists (cache hit)
        if let Some(output) = self.lookup_cache(&build_hash) {
            return Ok(output);
        }

        // 3. Realize input builds first
        let realized_inputs = self.realize_inputs(&build.inputs)?;

        // 4. Execute the build actions with ctx
        let output_path = self.execute_build(build, &realized_inputs)?;

        // 5. Compute output hash and move to final location
        let output_hash = self.hash_directory(&output_path)?;
        let final_path = self.store_path(build, &output_hash);
        self.move_to_store(output_path, &final_path)?;

        // 6. Make immutable and cache the mapping
        self.make_immutable(&final_path)?;
        self.cache_output(&build_hash, &final_path);

        Ok(final_path)
    }
}
```

**Key insight**: The store doesn't care what inputs look like or what the build actions do. It just:

1. Hashes the build description
2. Checks if output exists
3. Executes build if needed
4. Stores result immutably

This uniformity enables:

- **Unified caching**: Every build output is cached the same way
- **Composition**: Any build can depend on any other build
- **Parallelization**: Independent builds can be realized concurrently
- **Reproducibility**: Same build hash → same output (or cache hit)

## Store Deduplication

When a user installs a package that exists in the system store:

```bash
# System admin installs git
sudo sys apply /etc/syslua/init.lua
  → Installs to: /syslua/store/obj/abc123.../

# User wants git in their config
sys apply ~/.config/syslua/init.lua
  → Checks: Does /syslua/store/obj/abc123.../ exist?
  → If same filesystem: Creates hardlink
    ~/.local/share/syslua/store/pkg/git/2.40.0/ → /syslua/store/obj/abc123.../
  → If different filesystem: Just reference via PATH
```

**Hardlink deduplication:**

- Zero additional disk space for duplicate packages
- Both stores point to same inode
- Works if user store and system store are on same filesystem
- Falls back to PATH reference if different filesystems

## Immutability

Objects in `build/<hash>/` are made immutable after extraction:

### System Store Objects

- **Linux:** `chattr +i` (requires root to modify)
- **macOS:** `chflags uchg` (requires root to modify)
- **Windows:** ACL restrictions (requires admin to modify)
- **World-readable:** `chmod 755` (directories), `chmod 644` (files)

### User Store Objects

- **Same immutability flags** (user owns them, but makes immutable)
- **Purpose:** Prevent accidental modification
- **Removal:** User can run `sys gc` to remove (clears immutability first)

## Build-to-Store Flow Example

1. User writes config:

```lua
   local jq = sys.build({
     name = "jq",
     version = "1.7.1",
     inputs = function()
       return {
         url = "https://github.com/jqlang/jq/releases/download/jq-1.7.1/jq-" .. sys.platform,
         sha256 = "4dd2d8a0661df0b22f1bb9a1f9830f06b6f3b8f7c...",
       }
     end,
     apply = function(inputs, ctx)
       local archive = ctx:fetch_url(inputs.url, inputs.sha256)
       ctx:exec({ bin = "mkdir -p " .. ctx.out .. "/bin" })
       ctx:exec({ bin = "cp " .. archive .. " " .. ctx.out .. "/bin/jq" })
       ctx:exec({ bin = "chmod 755 " .. ctx.out .. "/bin/jq" })
       return { out = ctx.out }
     end,
   })

   sys.bind({
     inputs = function() return { build = jq } end,
     apply = function(inputs, ctx)
       ctx:exec("ln -sf " .. inputs.build.outputs.out .. "/bin/jq /usr/local/bin/jq")
     end,
     destroy = function(inputs, ctx)
       ctx:exec("rm /usr/local/bin/jq")
     end,
   })
```

2. Lua evaluation creates build object:

```
   BuildDef {
     name: "jq",
     version: "1.7.1",
     inputs: <evaluated>,
     apply_actions: [FetchUrl {...}, Cmd {...}, Cmd {...}, Cmd {...}],
     outputs: { out: "out" },
   }
```

3. Store computes build hash:

```
   build_hash = sha256(name + version + inputs + actions) = "abc123..."
```

4. Store checks cache:

```
   If build/abc123def456789012/ exists: CACHE HIT - skip build
```

5. If cache miss, store executes build:
   - Realize any input builds first
   - Execute actions (fetch, cmd, etc.)
   - Compute content hash from result
   - Move to build/<hash>/
   - Make immutable

6. Result in store:

```
   /syslua/store/
   └── build/
       └── abc123def456789012/
           └── bin/
               └── jq  # The actual binary
```

**Key insight**: The build hash (`abc123...`) is computed from the _description_, while the output hash is computed from the _content_. This separation enables:

- Cache hits even when the same content is described differently
- Reproducibility verification (same build → same output)
- Debugging (inspect the `.drv` file to see what was requested)
- Human-readable store paths for easier debugging

## Build Caching

Built packages are cached by **content hash** (hash computed from the build definition). This enables cache hits when the same build definition is used.

```
store/
└── build/<hash>/    # Built artifacts (immutable, content-addressed)
```

**Cache lookup order:**

1. Local store - check if `build/<hash>/` exists
2. Build from source - execute build actions, store result

## Related Documentation

- [01-builds.md](./01-builds.md) - What produces store content
- [05-snapshots.md](./05-snapshots.md) - How store state is tracked over time
- [08-apply-flow.md](./08-apply-flow.md) - How the store is populated during apply
