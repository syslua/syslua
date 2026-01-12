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

-- ============================================================================
-- Platform-Specific Commands: Creation
-- ============================================================================

---Build Linux group creation command
---@param name string
---@param opts {description?: string, gid?: number, system?: boolean}
---@return string bin, string[] args
local function linux_create_group_cmd(name, opts)
  local args = {}

  if opts.gid then
    table.insert(args, '-g')
    table.insert(args, tostring(opts.gid))
  end

  if opts.system then
    table.insert(args, '-r')
  end

  table.insert(args, name)
  return '/usr/sbin/groupadd', args
end

---Build macOS group creation script (multiple dscl commands)
---@param name string
---@param opts {description?: string, gid?: number, system?: boolean}
---@return string
local function darwin_create_group_script(name, opts)
  local cmds = {
    interpolate('dscl . -create /Groups/{{name}}', { name = name }),
  }

  if opts.gid then
    table.insert(cmds, interpolate(
      'dscl . -create /Groups/{{name}} PrimaryGroupID {{gid}}',
      { name = name, gid = opts.gid }
    ))
  else
    -- Auto-assign GID: find max existing + 1
    local start_gid = opts.system and 100 or 1000
    table.insert(cmds, interpolate(
      'gid=$(dscl . -list /Groups PrimaryGroupID | awk "\\$2 >= {{start}} {print \\$2}" | sort -n | tail -1); dscl . -create /Groups/{{name}} PrimaryGroupID $((gid + 1))',
      { name = name, start = start_gid }
    ))
  end

  if opts.description and opts.description ~= '' then
    table.insert(cmds, interpolate(
      'dscl . -create /Groups/{{name}} RealName "{{desc}}"',
      { name = name, desc = opts.description }
    ))
  end

  return table.concat(cmds, ' && ')
end

---Build Windows group creation PowerShell script
---@param name string
---@param opts {description?: string}
---@return string
local function windows_create_group_script(name, opts)
  local desc = opts.description or ''
  return interpolate(
    'New-LocalGroup -Name "{{name}}" -Description "{{desc}}"',
    { name = name, desc = desc }
  )
end

-- ============================================================================
-- Platform-Specific Commands: Existence Checks
-- ============================================================================

---Check if group exists on Linux
---@param name string
---@return string
local function linux_group_exists_check(name)
  return interpolate('getent group "{{name}}" >/dev/null 2>&1', { name = name })
end

---Check if group exists on macOS
---@param name string
---@return string
local function darwin_group_exists_check(name)
  return interpolate('dscl . -read /Groups/{{name}} >/dev/null 2>&1', { name = name })
end

---Check if group exists on Windows (PowerShell condition)
---@param name string
---@return string
local function windows_group_exists_check(name)
  return interpolate(
    '(Get-LocalGroup -Name "{{name}}" -ErrorAction SilentlyContinue)',
    { name = name }
  )
end

-- ============================================================================
-- Platform-Specific Commands: Deletion
-- ============================================================================

---Build Linux group deletion command
---@param name string
---@return string bin, string[] args
local function linux_delete_group_cmd(name)
  return '/usr/sbin/groupdel', { name }
end

---Build macOS group deletion command
---@param name string
---@return string bin, string[] args
local function darwin_delete_group_cmd(name)
  return '/usr/bin/dscl', { '.', '-delete', '/Groups/' .. name }
end

---Build Windows group deletion PowerShell script
---@param name string
---@return string
local function windows_delete_group_script(name)
  return interpolate('Remove-LocalGroup -Name "{{name}}"', { name = name })
end

return M
