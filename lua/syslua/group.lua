local prio = require('syslua.priority')
local interpolate = require('syslua.interpolation')

---@class syslua.group
local M = {}

-- ============================================================================
-- Type Definitions
-- ============================================================================

---@class syslua.group.Options
---@field description? syslua.Option<string> Group description/comment
---@field gid? syslua.Option<number> Specific GID (optional, auto-assigned if nil)
---@field system? syslua.Option<boolean> Create as system group (low GID range)

---@alias syslua.group.GroupMap table<string, syslua.group.Options>

---@class syslua.group.Defaults
---@field description string
---@field gid nil
---@field system boolean

-- ============================================================================
-- Constants
-- ============================================================================

local BIND_ID_PREFIX = '__syslua_group_'

-- ============================================================================
-- Default Options
-- ============================================================================

---@type syslua.group.Defaults
M.defaults = {
  description = '',
  gid = nil,
  system = false,
}

return M
