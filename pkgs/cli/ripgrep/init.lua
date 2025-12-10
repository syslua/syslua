---@class syslua.pkgs.cli.ripgrep
---@field ["15_1_0"] syslua.pkgs.cli.ripgrep.15_1_0
local M = {}

setmetatable(M, {
	__index = function(_, pkg)
		return require("syslua.pkgs.cli.ripgrep." .. pkg)
	end,
})

M.setup = function(opts)
	-- default to latest
	if opts == nil then
		return require("pkgs.cli.ripgrep")["15_1_0"].setup()
	end
end

return M
