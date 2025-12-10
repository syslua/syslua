---@class syslua.pkgs.cli.ripgrep.15_1_0
local M = {}

-- SHA256 hashes for each platform
local hashes = {
	["aarch64-darwin"] = "378e973289176ca0c6054054ee7f631a065874a352bf43f0fa60ef079b6ba715",
	["x86_64-darwin"] = "7b440cb2ac00bca52dbaab8c12c96a7682c3014b4f0c88c3ea0e626a63771d86",
	["x86_64-linux"] = "4a68be2a2ef8f7f67d79d39da6b4a0a2e1c20f4ecd4aaa78dac0a0dca0ba8e2e",
	["aarch64-linux"] = "bdd70c31f6a6f3bcf1e1c0f9f2f2a5f5d5e5f5a5b5c5d5e5f5a5b5c5d5e5f5a5",
}

-- URL templates for each platform
local function get_url(platform)
	local base = "https://github.com/BurntSushi/ripgrep/releases/download/15.1.0/"
	local filenames = {
		["aarch64-darwin"] = "ripgrep-15.1.0-aarch64-apple-darwin.tar.gz",
		["x86_64-darwin"] = "ripgrep-15.1.0-x86_64-apple-darwin.tar.gz",
		["x86_64-linux"] = "ripgrep-15.1.0-x86_64-unknown-linux-musl.tar.gz",
		["aarch64-linux"] = "ripgrep-15.1.0-aarch64-unknown-linux-gnu.tar.gz",
	}
	return base .. (filenames[platform] or filenames["x86_64-linux"])
end

M.setup = function()
	-- Create derivation - returns table with .out (store path)
	local drv = derive({
		name = "ripgrep",
		version = "15.1.0",

		-- opts can be a function that receives sys table for platform-specific values
		opts = function(sys)
			local url = get_url(sys.platform)
			local sha256 = hashes[sys.platform]

			if not sha256 then
				error("Unsupported platform: " .. sys.platform)
			end

			return {
				url = url,
				sha256 = sha256,
			}
		end,

		-- config function is called during realization with resolved opts and ctx
		config = function(opts, ctx)
			-- Download the archive (ctx.fetch_url handles caching and hash verification)
			local archive = ctx:fetch_url(opts.url, opts.sha256)

			-- Unpack to ctx.out (the output directory in the store)
			ctx:unpack(archive)
		end,
	})

	-- Activate - add to PATH using drv.out
	activate({
		opts = function(sys)
			return { drv = drv }
		end,
		config = function(opts, ctx)
			-- ripgrep binary is directly in the output directory
			ctx:add_to_path(opts.drv.out)
		end,
	})

	return M
end

return M
