---@class syslua.modules
---@field file syslua.modules.file
---@field env syslua.modules.env
---@field alias syslua.modules.alias
local M = {}

setmetatable(M, {
  __index = function(t, k)
    local cached = rawget(t, k)
    if cached ~= nil then
      return cached
    end
    local ok, mod = pcall(require, 'syslua.modules.' .. k)
    if ok then
      rawset(t, k, mod)
      return mod
    else
      error("Module 'syslua.modules." .. k .. "' not found")
    end
  end,
})

return M
