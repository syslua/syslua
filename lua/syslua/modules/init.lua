---@class syslua.modules
---@field file syslua.modules.file
local M = {}

setmetatable(M, {
  __index = function(t, k)
    if t[k] == nil then
      local ok, mod = pcall(require, 'syslua.modules.' .. k)
      if ok then
        t[k] = mod
        return mod
      else
        error("Module 'syslua.modules." .. k .. "' not found")
      end
    else
      return t[k]
    end
  end,
})

return M
