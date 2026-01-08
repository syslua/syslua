---@class syslua
---@field pkgs syslua.pkgs
---@field environment syslua.environment
---@field programs syslua.programs
---@field lib syslua.lib
local M = {}

setmetatable(M, {
  __index = function(t, k)
    local cached = rawget(t, k)
    if cached ~= nil then
      return cached
    end
    local ok, mod = pcall(require, 'syslua.' .. k)
    if ok then
      rawset(t, k, mod)
      return mod
    else
      error("Module 'syslua." .. k .. "' not found")
    end
  end,
})

return M
