# Module System

> Part of the [SysLua Architecture](./00-overview.md) documentation.

This document covers the module system and composition.

## Core Value: Standard Lua Idioms

SysLua modules are **plain Lua modules**. No magic, no DSL, no hidden behavior:

- `require()` returns a table — just like any Lua library
- `setup(opts)` is a function call — it does the work immediately
- `options` is a table — documents defaults, used for merging
- No hidden globals, no auto-evaluation, no implicit behavior

If you know Lua, you know how SysLua modules work.

## Entry Point vs Regular Modules

The **entry point** (`init.lua`) and **regular modules** both use the `local M = {} ... return M` pattern, but serve different purposes:

| Aspect | Entry Point (`init.lua`) | Regular Module |
|--------|--------------------------|----------------|
| **Purpose** | Declare external inputs, configure system | Provide reusable functionality |
| **`M.inputs`** | External dependencies (git repos, paths) | Not used |
| **`M.options`** | Not used | Default configuration values |
| **`M.setup()`** | Receives resolved `inputs` table | Receives user `opts` table |
| **Called by** | syslua runtime | Other modules or entry point |

```lua
-- Entry point pattern (init.lua)
local M = {}
M.inputs = { pkgs = "git:https://..." }  -- external dependencies
function M.setup(inputs)                   -- called by syslua
    require("inputs.pkgs.cli.ripgrep").setup()
end
return M

-- Regular module pattern (modules/foo.lua)
local M = {}
M.options = { port = 8080 }               -- default config values
function M.setup(opts)                     -- called by user code
    opts = opts or {}
    -- merge with M.options...
end
return M
```

See [Lua API - Entry Point Pattern](./04-lua-api.md#entry-point-pattern) for details.

## The Module Pattern

Every module follows the same structure:

```lua
local M = {}

-- Default options (serves as schema + documentation)
M.options = {
    port = 80,
    workers = "auto",
}

-- setup() merges options with defaults and does the work
function M.setup(opts)
    opts = opts or {}
    for k, v in pairs(M.options) do
        if opts[k] == nil then opts[k] = v end
    end
    
    -- Builds and binds happen here
    local config_build = sys.build({
        name = "nginx-config",
        inputs = function() return opts end,
        apply = function(o, ctx)
            ctx:exec({
                cmd = 'echo "worker_processes ' .. o.workers .. ';" > ' .. ctx.out .. '/nginx.conf'
            })
            return { out = ctx.out }
        end,
    })
    
    sys.bind({
        inputs = function() return { build = config_build } end,
        apply = function(o, ctx)
            ctx:exec('ln -sf ' .. o.build.outputs.out .. '/nginx.conf /etc/nginx/nginx.conf')
        end,
        destroy = function(o, ctx)
            ctx:exec('rm /etc/nginx/nginx.conf')
        end,
    })
    
    return M
end

return M
```

That's it. You call `setup()`, it does the work. No callbacks, no enable flags, no deferred evaluation.

## Module Types

All modules follow the same pattern. They only differ in what they do internally.

### Package Modules

```lua
-- pkgs/cli/ripgrep/init.lua
local M = {}

M.options = {
    version = "15.1.0",
}

function M.setup(opts)
    opts = opts or {}
    for k, v in pairs(M.options) do
        if opts[k] == nil then opts[k] = v end
    end
    
    local build = sys.build({
        name = "ripgrep",
        version = opts.version,
        inputs = function()
            local urls = {
                ["aarch64-darwin"] = "https://github.com/.../ripgrep-darwin-arm64.tar.gz",
                ["x86_64-linux"] = "https://github.com/.../ripgrep-linux-x64.tar.gz",
            }
            return { url = urls[sys.platform], sha256 = "..." }
        end,
        apply = function(o, ctx)
            local archive = ctx:fetch_url(o.url, o.sha256)
            ctx:exec({ bin = "tar -xzf " .. archive .. " -C " .. ctx.out })
            return { out = ctx.out }
        end,
    })
    
    sys.bind({
        inputs = function() return { build = build } end,
        apply = function(o, ctx)
            ctx:exec("ln -sf " .. o.build.outputs.out .. "/bin/rg /usr/local/bin/rg")
        end,
        destroy = function(o, ctx)
            ctx:exec("rm /usr/local/bin/rg")
        end,
    })
    
    return M
end

return M
```

### Service Modules

```lua
-- modules/services/nginx/init.lua
local M = {}

M.options = {
    port = 80,
    workers = "auto",
}

function M.setup(opts)
    opts = opts or {}
    for k, v in pairs(M.options) do
        if opts[k] == nil then opts[k] = v end
    end
    
    local config_build = sys.build({
        name = "nginx-config",
        inputs = function() return opts end,
        apply = function(o, ctx)
            local conf = string.format([[
worker_processes %s;
http {
    server { listen %d; }
}
]], o.workers, o.port)
            ctx:exec({ bin = 'echo ' .. lib.shellQuote(conf) .. ' > ' .. ctx.out .. '/nginx.conf' })
            return { out = ctx.out }
        end,
    })
    
    local service_build = sys.build({
        name = "nginx-service",
        inputs = function() return { config_path = config_build } end,
        apply = function(o, ctx)
            if sys.os == "linux" then
                local unit = [[
[Unit]
Description=nginx
[Service]
ExecStart=/usr/sbin/nginx -c ]] .. o.config_path.outputs.out .. [[/nginx.conf
[Install]
WantedBy=multi-user.target
]]
                ctx:exec({ bin = 'echo ' .. lib.shellQuote(unit) .. ' > ' .. ctx.out .. '/nginx.service' })
            elseif sys.os == "macos" then
                local plist = generate_launchd_plist(o)
                ctx:exec({ bin = 'echo ' .. lib.shellQuote(plist) .. ' > ' .. ctx.out .. '/nginx.plist' })
            end
            return { out = ctx.out }
        end,
    })
    
    sys.bind({
        inputs = function() return { config = config_build, service = service_build } end,
        apply = function(o, ctx)
            ctx:exec('ln -sf ' .. o.config.outputs.out .. '/nginx.conf /etc/nginx/nginx.conf')
            if sys.os == "linux" then
                ctx:exec('ln -sf ' .. o.service.outputs.out .. '/nginx.service /etc/systemd/system/nginx.service && systemctl daemon-reload && systemctl enable --now nginx')
            elseif sys.os == "macos" then
                ctx:exec('ln -sf ' .. o.service.outputs.out .. '/nginx.plist ~/Library/LaunchAgents/nginx.plist && launchctl load ~/Library/LaunchAgents/nginx.plist')
            end
        end,
        destroy = function(o, ctx)
            if sys.os == "linux" then
                ctx:exec('systemctl disable --now nginx && rm /etc/systemd/system/nginx.service && systemctl daemon-reload')
            elseif sys.os == "macos" then
                ctx:exec('launchctl unload ~/Library/LaunchAgents/nginx.plist && rm ~/Library/LaunchAgents/nginx.plist')
            end
            ctx:exec('rm /etc/nginx/nginx.conf')
        end,
    })
    
    return M
end

return M
```

### Program Modules

```lua
-- modules/programs/vscode/init.lua
local M = {}

M.options = {
    extensions = {},
    settings = {},
}

function M.setup(opts)
    opts = opts or {}
    for k, v in pairs(M.options) do
        if opts[k] == nil then opts[k] = v end
    end
    
    local vscode_build = sys.build({
        name = "vscode",
        inputs = function()
            local urls = {
                ["aarch64-darwin"] = "https://...",
                ["x86_64-linux"] = "https://...",
            }
            return { url = urls[sys.platform], sha256 = "..." }
        end,
        apply = function(o, ctx)
            local archive = ctx:fetch_url(o.url, o.sha256)
            ctx:exec({ bin = 'tar -xzf ' .. archive .. ' -C ' .. ctx.out })
            return { out = ctx.out }
        end,
    })
    
    local settings_build = sys.build({
        name = "vscode-settings",
        inputs = function() return { settings = opts.settings } end,
        apply = function(o, ctx)
            ctx:exec({ bin = 'echo ' .. lib.shellQuote(lib.toJSON(o.settings)) .. ' > ' .. ctx.out .. '/settings.json' })
            return { out = ctx.out }
        end,
    })
    
    sys.bind({
        inputs = function() return { vscode = vscode_build, settings = settings_build, extensions = opts.extensions } end,
        apply = function(o, ctx)
            -- Add to PATH
            ctx:exec('ln -sf ' .. o.vscode.outputs.out .. '/bin/code /usr/local/bin/code')
            -- Symlink settings
            ctx:exec('mkdir -p ~/.config/Code/User && ln -sf ' .. o.settings.outputs.out .. '/settings.json ~/.config/Code/User/settings.json')
            -- Install extensions
            for _, ext in ipairs(o.extensions) do
                ctx:exec('code --install-extension ' .. ext)
            end
        end,
        destroy = function(o, ctx)
            -- Uninstall extensions (in reverse)
            for i = #o.extensions, 1, -1 do
                ctx:exec('code --uninstall-extension ' .. o.extensions[i])
            end
            ctx:exec('rm ~/.config/Code/User/settings.json')
            ctx:exec('rm /usr/local/bin/code')
        end,
    })
    
    return M
end

return M
```

## Usage

```lua
-- init.lua

-- Packages
require("pkgs.cli.ripgrep").setup()
require("pkgs.cli.fd").setup()
require("pkgs.cli.jq").setup({ version = "1.7" })

-- Services
require("modules.services.nginx").setup({ port = 8080 })
require("modules.services.postgres").setup({ port = 5433 })

-- Programs
require("modules.programs.vscode").setup({
    extensions = { "rust-analyzer", "sumneko.lua" },
    settings = { ["editor.fontSize"] = 14 },
})
```

## Module Composition

Modules can call other modules. It's just function calls:

```lua
-- modules/dev-environment.lua
local M = {}

M.options = {
    with_database = true,
    node_version = "20",
}

function M.setup(opts)
    opts = opts or {}
    for k, v in pairs(M.options) do
        if opts[k] == nil then opts[k] = v end
    end
    
    -- Just call other modules
    require("pkgs.cli.ripgrep").setup()
    require("pkgs.cli.fd").setup()
    require("pkgs.cli.jq").setup()
    require("pkgs.runtime.nodejs").setup({ version = opts.node_version })
    
    if opts.with_database then
        require("modules.services.postgres").setup()
    end
    
    -- Can also use file/env directly
    env { EDITOR = "nvim" }
    
    return M
end

return M
```

Usage:

```lua
require("modules.dev-environment").setup({ with_database = false })
```

## Scoping: `user {}` and `project {}`

These are simple scoping helpers:

```lua
user {
    name = "alice",
    config = function()
        require("pkgs.cli.ripgrep").setup()
        file { path = "~/.gitconfig", content = "..." }
        env { EDITOR = "nvim" }
    end,
}

project {
    name = "my-app",
    config = function()
        require("pkgs.runtime.nodejs").setup()
        env { NODE_ENV = "development" }
    end,
}
```

Implementation:

```lua
-- lib/user.lua
function user(spec)
    local prev = _G.__sys_current_user
    _G.__sys_current_user = spec.name
    spec.config()
    _G.__sys_current_user = prev
end
```

## Module Annotations

For IDE support, annotate your modules:

```lua
---@class NginxOptions
---@field port? integer Listen port (default: 80)
---@field workers? string|integer Worker processes (default: "auto")

local M = {}

---@type NginxOptions
M.options = {
    port = 80,
    workers = "auto",
}

---Configure nginx
---@param opts? NginxOptions
---@return table
function M.setup(opts)
    -- ...
end

return M
```

## Why No Auto-Evaluation?

We explicitly rejected auto-evaluation because:

1. **Implicit behavior is confusing** — When does code run? Magic.
2. **Order is unclear** — Explicit `setup()` calls make order obvious
3. **Standard Lua** — `require()` + function call is how Lua works
4. **Debugging** — Stack traces point to your `setup()` call
5. **No surprises** — What you write is what runs

## Configuration File Structure

```
~/.config/syslua/
├── init.lua              # Entry point
├── syslua.lock           # Lock file (auto-generated)
└── modules/              # Custom modules
    └── my-dev-setup.lua
```

Example `init.lua`:

```lua
local M = {}

M.inputs = {
    pkgs = "git:https://github.com/syslua/pkgs.git",
}

function M.setup(inputs)
    -- Inputs are available via their namespace in package.path
    -- The "pkgs" input provides the "pkgs" namespace (lua/pkgs/)
    local pkgs = require("pkgs")
    
    -- Packages
    pkgs.cli.ripgrep.setup()
    pkgs.cli.fd.setup()
    
    -- Services (if on server)
    if syslua.hostname == "server" then
        require("modules.services.nginx").setup({ port = 80 })
    end
    
    -- Custom module
    require("modules.my-dev-setup").setup()
    
    -- Direct declarations
    file {
        path = "~/.gitconfig",
        content = [[
[user]
    name = Alice
    email = alice@example.com
]],
    }
    
    env {
        EDITOR = "nvim",
    }
end

return M
```

## Built-in Modules

SysLua ships with built-in modules for common tasks.

### `syslua.modules.env`

Manages environment variables across shell configurations. Creates a single build containing env files and binds to inject them into shell RC files.

```lua
local syslua = require('syslua')
local prio = require('syslua.priority')

syslua.modules.env.setup({
    -- Simple assignment
    EDITOR = 'nvim',
    PAGER = 'less',
    
    -- PATH modifications with priority
    PATH = prio.before('/opt/bin'),      -- prepend to PATH
    PATH = prio.after('/custom/bin'),    -- append to PATH
    
    -- Force override (wins over other declarations)
    SHELL = prio.force('/bin/zsh'),
})
```

**Supported shells:**

- POSIX shells (bash, zsh) — sources from `~/.bashrc`, `~/.zshrc`
- Fish — sources from `~/.config/fish/config.fish`
- PowerShell (Windows) — modifies `$PROFILE`

**Multiple setup calls merge:**

```lua
-- In one module
syslua.modules.env.setup({ EDITOR = 'vim' })

-- In another module  
syslua.modules.env.setup({ PAGER = 'less' })

-- Result: both EDITOR and PAGER are set
```

**Conflict resolution:**

```lua
-- This will ERROR (same key, no priority):
syslua.modules.env.setup({ EDITOR = 'vim' })
syslua.modules.env.setup({ EDITOR = 'nano' })  -- conflict!

-- Use prio.force() to override:
syslua.modules.env.setup({ EDITOR = prio.force('nvim') })  -- wins
```

### `syslua.modules.file`

Manages individual files. See examples throughout this document.

```lua
syslua.modules.file.setup({
    target = '~/.gitconfig',
    content = [[
[user]
    name = Alice
]],
    mutable = false,  -- default: immutable (content-addressed)
})
```

## See Also

- [Overview](./00-overview.md) — Core values and principles
- [Lua API](./04-lua-api.md) — API layers, globals, and entry point pattern
- [Inputs](./06-inputs.md) — External dependencies and authentication
- [Builds](./01-builds.md) — How `sys.build()` works
- [Binds](./02-binds.md) — How `sys.bind()` works
