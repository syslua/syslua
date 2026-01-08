---@class syslua.pkgs
---@field cli syslua.pkgs.cli
local M = {}

setmetatable(M, {
  __index = function(t, k)
    local cached = rawget(t, k)
    if cached ~= nil then
      return cached
    end
    local ok, mod = pcall(require, 'syslua.pkgs.' .. k)
    if ok then
      rawset(t, k, mod)
      return mod
    else
      error("Module 'syslua.pkgs." .. k .. "' not found")
    end
  end,
})

---@class syslua.pkgs.Release
---@field url string
---@field sha256 string
---@field format "binary" | syslua.lib.extract.ArchiveFormat

---@class syslua.pkgs.Releases: table<Os, table<string, syslua.pkgs.Release>>

---@class syslua.pkgs.Meta
---@field name string
---@field homepage string
---@field description string
---@field license string
---@field versions table<string, string>

return M
