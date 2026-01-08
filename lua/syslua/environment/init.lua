---@class syslua.environment
---@field files syslua.environment.files
---@field variables syslua.environment.variables
---@field aliases syslua.environment.aliases
local M = {}

setmetatable(M, {
  __index = function(t, k)
    local cached = rawget(t, k)
    if cached ~= nil then
      return cached
    end
    local ok, mod = pcall(require, 'syslua.environment.' .. k)
    if ok then
      rawset(t, k, mod)
      return mod
    else
      error("Module 'syslua.environment." .. k .. "' not found")
    end
  end,
})

return M
