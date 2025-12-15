---@class syslua.pkgs
local M = {}

setmetatable(M, {
  __index = function(t, k)
    if t[k] == nil then
      local ok, mod = pcall(require, 'syslua.pkgs.' .. k)
      if ok then
        t[k] = mod
        return mod
      else
        error("Module 'syslua.pkgs." .. k .. "' not found")
      end
    else
      return t[k]
    end
  end,
})

return M
