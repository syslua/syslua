# Package Development Guide

## Structure

```
pkgs/
├── init.lua           # Lazy-loads categories (cli/, gui/, etc.)
├── cli/
│   ├── init.lua       # Lazy-loads packages, declares @field for each
│   ├── ripgrep.lua    # Archive-based package (tar.gz/zip)
│   ├── fd.lua         # Archive-based package
│   └── jq.lua         # Binary-only package (direct download)
└── AGENTS.md
```

## Package File Template

```lua
local prio = require('syslua.priority')
local lib = require('syslua.lib')

---@class syslua.pkgs.{category}.{name}
local M = {}

---@type syslua.pkgs.Releases
M.releases = {
  ['1.0.0'] = {
    ['aarch64-darwin'] = { url = '...', sha256 = '...', format = 'tar.gz' },
    ['x86_64-darwin']  = { url = '...', sha256 = '...', format = 'tar.gz' },
    ['x86_64-linux']   = { url = '...', sha256 = '...', format = 'tar.gz' },
    ['x86_64-windows'] = { url = '...', sha256 = '...', format = 'zip' },
  },
}

---@type syslua.pkgs.Meta
M.meta = {
  name = '{name}',
  homepage = 'https://github.com/...',
  description = '...',
  license = 'MIT',
  versions = { stable = '1.0.0', latest = '1.0.0' },
}

---@class syslua.pkgs.{category}.{name}.Options
---@field version? string | syslua.priority.PriorityValue<string>

local default_opts = {
  version = prio.default(M.meta.versions.stable),
}

---@type syslua.pkgs.{category}.{name}.Options
M.opts = default_opts

---@param provided_opts? syslua.pkgs.{category}.{name}.Options
---@return BuildRef
function M.setup(provided_opts)
  -- 1. Merge options
  -- 2. Resolve version alias
  -- 3. Validate release exists for version and platform
  -- 4. Fetch, extract (if archive), build
  -- 5. Return BuildRef with outputs
end

return M
```

## Two Release Patterns

### Archive Releases (tar.gz, zip)

For packages distributed as archives containing binary + extras (man pages, completions):

```lua
local archive = lib.fetch_url({
  url = platform_release.url,
  sha256 = platform_release.sha256,
})

local extracted = lib.extract({
  archive = archive.outputs.out,
  format = platform_release.format,  -- 'tar.gz' or 'zip'
  strip_components = 1,              -- Remove top-level directory
})

return sys.build({
  inputs = { extracted = extracted },
  create = function(inputs, ctx)
    local src = inputs.extracted.outputs.out
    -- Copy files to ctx.out
    return { bin = ..., man = ..., completions = ..., out = ctx.out }
  end,
})
```

### Binary Releases (direct download)

For packages distributed as standalone executables:

```lua
-- format = 'binary' in releases table
local downloaded = lib.fetch_url({
  url = platform_release.url,
  sha256 = platform_release.sha256,
})

return sys.build({
  inputs = { downloaded = downloaded },
  create = function(inputs, ctx)
    local src = inputs.downloaded.outputs.out
    -- Copy and chmod +x
    return { bin = ..., out = ctx.out }
  end,
})
```

## Hermetic Execution

`ctx:exec` runs in an isolated environment with `PATH=/path-not-set`. You MUST use full paths:

```lua
-- Unix: Use /bin/sh with full path
ctx:exec({
  bin = '/bin/sh',
  args = { '-c', 'cp "src" "dst" && chmod +x "dst"' },
})

-- Windows: cmd.exe works (system vars preserved)
ctx:exec({
  bin = 'cmd.exe',
  args = { '/c', 'copy "src" "dst"' },
})
```

## Cross-Platform File Operations

```lua
-- Binary name
local bin_name = 'tool' .. (sys.os == 'windows' and '.exe' or '')

-- Path construction (ALWAYS use sys.path.join)
local bin_path = sys.path.join(ctx.out, bin_name)

-- Platform-specific commands
if sys.os == 'windows' then
  ctx:exec({
    bin = 'cmd.exe',
    args = { '/c', string.format('copy "%s" "%s"', src, dst) },
  })
else
  ctx:exec({
    bin = '/bin/sh',
    args = { '-c', string.format('cp "%s" "%s" && chmod +x "%s"', src, dst, dst) },
  })
end
```

## Build Outputs

Return paths to key artifacts:

| Output | Description | Example |
|--------|-------------|---------|
| `bin` | Path to executable | `sys.path.join(ctx.out, 'rg')` |
| `out` | Build output directory | `ctx.out` |
| `man` | Man page (optional) | `sys.path.join(ctx.out, 'doc', 'rg.1')` |
| `completions` | Completions directory (optional) | `sys.path.join(ctx.out, 'complete')` |

## Adding a New Package

1. Create `pkgs/{category}/{name}.lua` following the template
2. Add `---@field {name} syslua.pkgs.{category}.{name}` to `pkgs/{category}/init.lua`
3. Get release URLs and SHA256 hashes from GitHub releases
4. Determine archive structure (use `tar -tzf` or unzip to inspect)
5. Note completion file locations and man page paths

## Platforms

Supported platforms (use as keys in releases table):

- `aarch64-darwin` (Apple Silicon Mac)
- `x86_64-darwin` (Intel Mac)
- `x86_64-linux` (Linux x64)
- `aarch64-linux` (Linux ARM64, if available)
- `x86_64-windows` (Windows x64)
- `aarch64-windows` (Windows ARM64, if available)

## Common Archive Structures

| Tool | Binary | Man Page | Completions |
|------|--------|----------|-------------|
| ripgrep | `rg` | `doc/rg.1` | `complete/{rg.bash,_rg,rg.fish,_rg.ps1}` |
| fd | `fd` | `fd.1` | `autocomplete/{fd.bash,_fd,fd.fish,fd.ps1}` |
| bat | `bat` | `bat.1` | `autocomplete/{bat.bash,bat.zsh,bat.fish,bat.ps1}` |
| delta | `delta` | - | `etc/completion/{delta.bash,_delta,delta.fish}` |
