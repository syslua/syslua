---@class syslua.pkgs.cli
---@field fd syslua.pkgs.cli.fd
---@field ripgrep syslua.pkgs.cli.ripgrep
---@field jq syslua.pkgs.cli.jq
local M = {}

setmetatable(M, {
  __index = function(t, k)
    local cached = rawget(t, k)
    if cached ~= nil then
      return cached
    end
    local ok, mod = pcall(require, 'syslua.pkgs.cli.' .. k)
    if ok then
      rawset(t, k, mod)
      return mod
    else
      error("Module 'syslua.pkgs.cli." .. k .. "' not found")
    end
  end,
})

return M
