-- CLI Tools Example for sys.lua
-- This example demonstrates the derive{} and activate{} APIs for installing binary packages
--
-- Run with:
--   cargo run -- apply examples/cli-tools/init.lua
--
-- Or plan first (dry-run):
--   cargo run -- plan examples/cli-tools/init.lua

local M = {}

--------------------------------------------------------------------------------
-- Package Definitions
--------------------------------------------------------------------------------

-- SHA256 hashes for ripgrep 15.1.0 releases (per platform)
local ripgrep_hashes = {
    ["aarch64-darwin"] = "378e973289176ca0c6054054ee7f631a065874a352bf43f0fa60ef079b6ba715",
    ["x86_64-darwin"] = "7b440cb2ac00bca52dbaab8c12c96a7682c3014b4f0c88c3ea0e626a63771d86",
    ["x86_64-linux"] = "4a68be2a2ef8f7f67d79d39da6b4a0a2e1c20f4ecd4aaa78dac0a0dca0ba8e2e",
    ["aarch64-linux"] = "bdd70c31f6a6f3bcf1e1c0f9f2f2a5f5d5e5f5a5b5c5d5e5f5a5b5c5d5e5f5a5",
}

-- URL templates for ripgrep
local function ripgrep_url(platform)
    local base = "https://github.com/BurntSushi/ripgrep/releases/download/15.1.0/"
    local filenames = {
        ["aarch64-darwin"] = "ripgrep-15.1.0-aarch64-apple-darwin.tar.gz",
        ["x86_64-darwin"] = "ripgrep-15.1.0-x86_64-apple-darwin.tar.gz",
        ["x86_64-linux"] = "ripgrep-15.1.0-x86_64-unknown-linux-musl.tar.gz",
        ["aarch64-linux"] = "ripgrep-15.1.0-aarch64-unknown-linux-gnu.tar.gz",
    }
    return base .. (filenames[platform] or filenames["x86_64-linux"])
end

--------------------------------------------------------------------------------
-- Setup
--------------------------------------------------------------------------------

function M.setup()
    -- Install ripgrep - a fast grep alternative
    -- https://github.com/BurntSushi/ripgrep
    --
    -- derive{} returns a Derivation table with:
    --   .name     - package name
    --   .version  - version string
    --   .hash     - content hash
    --   .out      - store path (e.g., "~/.local/share/syslua/store/obj/ripgrep-15.1.0-abc123")
    --   .outputs  - table mapping output names to paths (e.g., { out = "<path>" })
    local rg = derive {
        name = "ripgrep",
        version = "15.1.0",

        -- opts can be a function that receives sys table for platform-specific values
        -- sys contains: platform, os, arch, hostname, username, is_darwin, is_linux, is_windows
        opts = function(sys)
            local url = ripgrep_url(sys.platform)
            local sha256 = ripgrep_hashes[sys.platform]

            if not sha256 then
                error("Unsupported platform for ripgrep: " .. sys.platform)
            end

            return {
                url = url,
                sha256 = sha256,
            }
        end,

        -- config function is called during realization with:
        --   opts: the resolved options table
        --   ctx: DerivationCtx with methods for building
        --
        -- ctx provides:
        --   ctx.out      - output directory path (string)
        --   ctx.sys      - system info table (platform, os, arch, etc.)
        --   ctx:fetch_url(url, sha256) -> path  - download with caching + verification
        --   ctx:unpack(archive, dest?)          - extract archive (dest defaults to ctx.out)
        --   ctx:mkdir(path)                     - create directory
        --   ctx:copy(src, dst)                  - copy file or directory
        --   ctx:write(path, content)            - write string to file
        --   ctx:symlink(target, link)           - create symbolic link
        --   ctx:chmod(path, mode)               - set file permissions (Unix only)
        config = function(opts, ctx)
            -- Download the archive (cached if already downloaded)
            local archive = ctx:fetch_url(opts.url, opts.sha256)

            -- Unpack to the output directory
            ctx:unpack(archive)
        end,
    }

    -- Activate ripgrep (add to PATH)
    --
    -- activate{} follows the same opts/config pattern as derive{}
    --
    -- The ActivationCtx provides:
    --   ctx.sys                          - system info table (same as DerivationCtx)
    --   ctx:add_to_path(bin_path)        - add directory to PATH
    --   ctx:symlink(source, target)      - create symlink
    --   ctx:source_in_shell(script)      - source a script in shell init
    --   ctx:run(cmd)                     - escape hatch for arbitrary commands
    activate {
        -- Pass the derivation in opts so config can access drv.out
        opts = {
            drv = rg,
        },
        config = function(opts, ctx)
            -- Use the derivation's .out path to add the bin directory to PATH
            -- The ripgrep binary is directly in the output directory (not in a subdirectory)
            ctx:add_to_path(opts.drv.out)
        end,
    }
end

return M
