---@class syslua.programs
---@field ripgrep syslua.programs.ripgrep
---@field fd syslua.programs.fd
---@field jq syslua.programs.jq
local M = {}

setmetatable(M, {
  __index = function(t, k)
    local cached = rawget(t, k)
    if cached then
      return cached
    end

    local ok, mod = pcall(require, 'syslua.programs.' .. k)
    if ok then
      rawset(t, k, mod)
      return mod
    else
      error(string.format('Program not found: %s', k))
    end
  end,
})

return M
