---@class syslua
---@field pkgs syslua.pkgs
---@field modules syslua.modules
---@field lib syslua.lib
local M = {}

setmetatable(M, {
  __index = function(t, k)
    if t[k] == nil then
      local ok, mod = pcall(require, 'syslua.' .. k)
      if ok then
        t[k] = mod
        return mod
      else
        error("Module 'syslua." .. k .. "' not found")
      end
    else
      return t[k]
    end
  end,
})

return M
