# Plan: Service Modules

## Goal

Create service module examples that manage system services via systemd (Linux) and launchd (macOS).

## Problem

The architecture describes service management, but no service modules exist. Users have no examples of declarative service configuration.

## Architecture Reference

- [07-modules.md](../architecture/07-modules.md):154-232 - Service module pattern
- [09-platform.md](../architecture/09-platform.md):161-267 - Service management platforms

## Approach

### Phase 1: Service Infrastructure

1. Create helper functions for systemd unit generation
2. Create helper functions for launchd plist generation
3. Handle service installation and enablement in binds

### Phase 2: Example Services

1. `syncthing` - File synchronization (user service)
2. `tailscale` - VPN (system service)

## Service Module Structure

```lua
-- lua/syslua/modules/services/syncthing.lua
local M = {}

M.options = {
    version = "1.27.0",
}

function M.setup(opts)
    opts = opts or {}
    local version = opts.version or M.options.version
    
    -- Build: fetch syncthing binary
    local binary_build = sys.build({
        name = "syncthing",
        version = version,
        apply = function(inputs, ctx)
            local archive = ctx:fetch_url(inputs.url, inputs.sha256)
            ctx:exec("tar -xzf " .. archive .. " -C " .. ctx.out)
            return { out = ctx.out }
        end,
    })
    
    -- Build: generate service unit
    local service_build = sys.build({
        name = "syncthing-service",
        inputs = function() return { binary = binary_build } end,
        apply = function(inputs, ctx)
            if sys.os == "linux" then
                -- Generate systemd unit
                ctx:write_file(ctx.out .. "/syncthing.service", [[
[Unit]
Description=Syncthing
After=network.target

[Service]
ExecStart=]] .. inputs.binary.outputs.out .. [[/syncthing serve
Restart=on-failure

[Install]
WantedBy=default.target
]])
            elseif sys.os == "darwin" then
                -- Generate launchd plist
                ctx:write_file(ctx.out .. "/syncthing.plist", ...)
            end
            return { out = ctx.out }
        end,
    })
    
    -- Bind: install and enable service
    sys.bind({
        inputs = function() return { service = service_build } end,
        apply = function(inputs, ctx)
            if sys.os == "linux" then
                ctx:exec("ln -sf " .. inputs.service.outputs.out .. "/syncthing.service ~/.config/systemd/user/")
                ctx:exec("systemctl --user daemon-reload")
                ctx:exec("systemctl --user enable --now syncthing")
            elseif sys.os == "darwin" then
                ctx:exec("ln -sf " .. inputs.service.outputs.out .. "/syncthing.plist ~/Library/LaunchAgents/")
                ctx:exec("launchctl load ~/Library/LaunchAgents/syncthing.plist")
            end
        end,
        destroy = function(inputs, ctx)
            if sys.os == "linux" then
                ctx:exec("systemctl --user disable --now syncthing")
                ctx:exec("rm ~/.config/systemd/user/syncthing.service")
            elseif sys.os == "darwin" then
                ctx:exec("launchctl unload ~/Library/LaunchAgents/syncthing.plist")
                ctx:exec("rm ~/Library/LaunchAgents/syncthing.plist")
            end
        end,
    })
    
    return M
end

return M
```

## Files to Create

| Path | Purpose |
|------|---------|
| `lua/syslua/modules/services/init.lua` | Services namespace |
| `lua/syslua/modules/services/syncthing.lua` | Syncthing service |
| `lua/syslua/lib/systemd.lua` | Systemd unit helpers |
| `lua/syslua/lib/launchd.lua` | Launchd plist helpers |

## Success Criteria

1. At least one service works on Linux (systemd)
2. At least one service works on macOS (launchd)
3. Services can be enabled/disabled via config
4. Destroy properly stops and removes services
5. User vs system services handled correctly

## Open Questions

- [ ] How to handle Windows services?
- [ ] Should services be started during apply or just enabled?
- [ ] How to handle service dependencies?
- [ ] What about service logs and status checking?
