# Module System

> Part of the [sys.lua Architecture](./00-overview.md) documentation.

This document covers the module system and composition.

## Core Value: Standard Lua Idioms

sys.lua modules are **plain Lua modules**. No magic, no DSL, no hidden behavior:

- `require()` returns a table — just like any Lua library
- `setup(opts)` is a function call — it does the work immediately
- `options` is a table — documents defaults, used for merging
- No hidden globals, no auto-evaluation, no implicit behavior

If you know Lua, you know how sys.lua modules work.

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
    
    -- Derivations and activations happen here
    local config_drv = derive {
        name = "nginx-config",
        opts = function(sys) return opts end,
        config = function(o, ctx)
            ctx.write(ctx.out .. "/nginx.conf", generate_conf(o))
        end,
    }
    
    activate {
        opts = function(sys) return { drv = config_drv } end,
        config = function(o, ctx)
            ctx.symlink(o.drv.out .. "/nginx.conf", "/etc/nginx/nginx.conf")
        end,
    }
    
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
    
    local drv = derive {
        name = "ripgrep",
        version = opts.version,
        opts = function(sys)
            local urls = {
                ["aarch64-darwin"] = "https://github.com/.../ripgrep-darwin-arm64.tar.gz",
                ["x86_64-linux"] = "https://github.com/.../ripgrep-linux-x64.tar.gz",
            }
            return { url = urls[sys.platform], sha256 = "..." }
        end,
        config = function(o, ctx)
            local archive = ctx.fetch_url(o.url, o.sha256)
            ctx.unpack(archive, ctx.out)
        end,
    }
    
    activate {
        opts = function(sys) return { drv = drv } end,
        config = function(o, ctx)
            ctx.add_to_path(o.drv.out .. "/bin")
        end,
    }
    
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
    
    local config_drv = derive {
        name = "nginx-config",
        opts = function(sys) return opts end,
        config = function(o, ctx)
            local conf = string.format([[
worker_processes %s;
http {
    server { listen %d; }
}
]], o.workers, o.port)
            ctx.write(ctx.out .. "/nginx.conf", conf)
        end,
    }
    
    local service_drv = derive {
        name = "nginx-service",
        opts = function(sys) return { sys = sys, config_path = config_drv.out } end,
        config = function(o, ctx)
            if o.sys.os == "linux" then
                ctx.write(ctx.out .. "/nginx.service", [[
[Unit]
Description=nginx
[Service]
ExecStart=/usr/sbin/nginx -c ]] .. o.config_path .. [[/nginx.conf
[Install]
WantedBy=multi-user.target
]])
            elseif o.sys.os == "macos" then
                ctx.write(ctx.out .. "/nginx.plist", generate_launchd_plist(o))
            end
        end,
    }
    
    activate {
        opts = function(sys) return { config = config_drv, service = service_drv, sys = sys } end,
        config = function(o, ctx)
            ctx.symlink(o.config.out .. "/nginx.conf", "/etc/nginx/nginx.conf")
            if o.sys.os == "linux" then
                ctx.symlink(o.service.out .. "/nginx.service", "/etc/systemd/system/nginx.service")
                ctx.enable_service("nginx")
            elseif o.sys.os == "macos" then
                ctx.symlink(o.service.out .. "/nginx.plist", "~/Library/LaunchAgents/nginx.plist")
                ctx.enable_service("nginx")
            end
        end,
    }
    
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
    
    local vscode_drv = derive {
        name = "vscode",
        opts = function(sys)
            local urls = {
                ["aarch64-darwin"] = "https://...",
                ["x86_64-linux"] = "https://...",
            }
            return { url = urls[sys.platform], sha256 = "..." }
        end,
        config = function(o, ctx)
            local archive = ctx.fetch_url(o.url, o.sha256)
            ctx.unpack(archive, ctx.out)
        end,
    }
    
    local settings_drv = derive {
        name = "vscode-settings",
        opts = function(sys) return { settings = opts.settings } end,
        config = function(o, ctx)
            ctx.write(ctx.out .. "/settings.json", lib.toJSON(o.settings))
        end,
    }
    
    activate {
        opts = function(sys) return { vscode = vscode_drv, settings = settings_drv, extensions = opts.extensions } end,
        config = function(o, ctx)
            ctx.add_to_path(o.vscode.out .. "/bin")
            ctx.symlink(o.settings.out .. "/settings.json", "~/.config/Code/User/settings.json")
            for _, ext in ipairs(o.extensions) do
                ctx.run("code --install-extension " .. ext)
            end
        end,
    }
    
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
    local pkgs = require("inputs.pkgs")
    
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

## See Also

- [Overview](./00-overview.md) — Core values and principles
- [Lua API](./04-lua-api.md) — API layers, globals, and entry point pattern
- [Inputs](./06-inputs.md) — External dependencies and authentication
- [Derivations](./01-derivations.md) — How `derive {}` works
- [Activations](./02-activations.md) — How `activate {}` works
