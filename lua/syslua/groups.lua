local prio = require('syslua.priority')
local interpolate = require('syslua.interpolation')

---@class syslua.groups
local M = {}

-- ============================================================================
-- Type Definitions
-- ============================================================================

---@class syslua.groups.Options
---@field description? syslua.Option<string> Group description/comment
---@field gid? syslua.Option<number> Specific GID (optional, auto-assigned if nil)
---@field system? syslua.Option<boolean> Create as system group (low GID range)

---@alias syslua.groups.GroupMap table<string, syslua.groups.Options>

---@class syslua.groups.Defaults
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

---@type syslua.groups.Defaults
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
    table.insert(
      cmds,
      interpolate('dscl . -create /Groups/{{name}} PrimaryGroupID {{gid}}', { name = name, gid = opts.gid })
    )
  else
    -- Auto-assign GID: find max existing + 1, fallback to start_gid if no groups exist
    local start_gid = opts.system and 100 or 1000
    table.insert(
      cmds,
      interpolate(
        'gid=$(dscl . -list /Groups PrimaryGroupID | awk "\\$2 >= {{start}} {print \\$2}" | sort -n | tail -1); gid=${gid:-{{fallback}}}; dscl . -create /Groups/{{name}} PrimaryGroupID $((gid + 1))',
        { name = name, start = start_gid, fallback = start_gid - 1 }
      )
    )
  end

  if opts.description and opts.description ~= '' then
    table.insert(
      cmds,
      interpolate('dscl . -create /Groups/{{name}} RealName "{{desc}}"', { name = name, desc = opts.description })
    )
  end

  return table.concat(cmds, ' && ')
end

---Build Windows group creation PowerShell script
---@param name string
---@param opts {description?: string}
---@return string
local function windows_create_group_script(name, opts)
  local desc = opts.description or ''
  return interpolate('New-LocalGroup -Name "{{name}}" -Description "{{desc}}"', { name = name, desc = desc })
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
  return interpolate('(Get-LocalGroup -Name "{{name}}" -ErrorAction SilentlyContinue)', { name = name })
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

-- ============================================================================
-- Platform-Specific Commands: Update
-- ============================================================================

---Build macOS group update script (description only)
---@param name string
---@param opts {description?: string}
---@return string
local function darwin_update_group_script(name, opts)
  if opts.description ~= nil then
    return interpolate(
      'dscl . -create /Groups/{{name}} RealName "{{desc}}"',
      { name = name, desc = opts.description }
    )
  end
  return 'true'
end

---Build Windows group update PowerShell script
---@param name string
---@param opts {description?: string}
---@return string
local function windows_update_group_script(name, opts)
  return interpolate(
    'Set-LocalGroup -Name "{{name}}" -Description "{{desc}}"',
    { name = name, desc = opts.description or '' }
  )
end

-- ============================================================================
-- Validation Helpers
-- ============================================================================

---Get group members on Linux
---@param name string
---@return string[]
local function linux_get_group_members(name)
  local handle = io.popen(interpolate('getent group "{{name}}" 2>/dev/null | cut -d: -f4', { name = name }))
  if not handle then
    return {}
  end
  local members_str = handle:read('*a'):gsub('%s+$', '')
  handle:close()
  if members_str == '' then
    return {}
  end
  local members = {}
  for member in members_str:gmatch('[^,]+') do
    table.insert(members, member)
  end
  return members
end

---Get group members on macOS
---@param name string
---@return string[]
local function darwin_get_group_members(name)
  local handle = io.popen(
    interpolate(
      'dscl . -read /Groups/{{name}} GroupMembership 2>/dev/null | sed "s/GroupMembership://" | tr " " "\\n" | grep -v "^$"',
      { name = name }
    )
  )
  if not handle then
    return {}
  end
  local members = {}
  for line in handle:lines() do
    local member = line:gsub('%s+', '')
    if member ~= '' then
      table.insert(members, member)
    end
  end
  handle:close()
  return members
end

---Get group members on Windows
---@param name string
---@return string[]
local function windows_get_group_members(name)
  local handle = io.popen(
    interpolate(
      'powershell -NoProfile -Command "Get-LocalGroupMember -Group \\"{{name}}\\" -ErrorAction SilentlyContinue | ForEach-Object { $_.Name }"',
      { name = name }
    )
  )
  if not handle then
    return {}
  end
  local members = {}
  for line in handle:lines() do
    local member = line:gsub('%s+$', '')
    if member ~= '' then
      table.insert(members, member)
    end
  end
  handle:close()
  return members
end

---Get group members (cross-platform)
---@param name string
---@param os string
---@return string[]
local function get_group_members(name, os)
  if os == 'linux' then
    return linux_get_group_members(name)
  elseif os == 'darwin' then
    return darwin_get_group_members(name)
  elseif os == 'windows' then
    return windows_get_group_members(name)
  end
  return {}
end

---Validate GID range and warn if in system range
---@param name string
---@param gid number?
---@param is_system boolean
local function validate_gid(name, gid, is_system)
  if not gid then
    return
  end

  local system_max = 999
  if gid <= system_max and not is_system then
    print(
      string.format(
        "Warning: group '%s' has GID %d which is in system range (<%d). Consider using system=true or a higher GID.\n",
        name,
        gid,
        system_max + 1
      )
    )
  end
end

---Validate group options
---@param name string
---@param opts syslua.groups.Options
local function validate_group_options(name, opts)
  if not sys.is_elevated then
    error('syslua.groups requires elevated privileges (root/Administrator)', 0)
  end

  local gid = prio.unwrap(opts.gid)
  local is_system = prio.unwrap(opts.system) or false

  if gid then
    validate_gid(name, gid, is_system)
  end
end

-- ============================================================================
-- Bind Creation
-- ============================================================================

---Create a bind for a single group
---@param name string
---@param opts syslua.groups.Options
local function create_group_bind(name, opts)
  local bind_id = BIND_ID_PREFIX .. name

  local description = prio.unwrap(opts.description) or ''
  local gid = prio.unwrap(opts.gid)
  local is_system = prio.unwrap(opts.system) or false

  sys.bind({
    id = bind_id,
    replace = true,
    inputs = {
      groupname = name,
      description = description,
      gid = gid,
      system = is_system,
      os = sys.os,
    },

    create = function(inputs, ctx)
      if inputs.os == 'linux' then
        local exists_check = linux_group_exists_check(inputs.groupname)
        local _, create_args = linux_create_group_cmd(inputs.groupname, {
          description = inputs.description,
          gid = inputs.gid,
          system = inputs.system,
        })
        local create_cmd = '/usr/sbin/groupadd ' .. table.concat(create_args, ' ')

        ctx:exec({
          bin = '/bin/sh',
          args = {
            '-c',
            interpolate(
              'if ! {{exists_check}}; then {{create_cmd}}; fi',
              { exists_check = exists_check, create_cmd = create_cmd }
            ),
          },
        })
      elseif inputs.os == 'darwin' then
        local exists_check = darwin_group_exists_check(inputs.groupname)
        local create_script = darwin_create_group_script(inputs.groupname, {
          description = inputs.description,
          gid = inputs.gid,
          system = inputs.system,
        })

        ctx:exec({
          bin = '/bin/sh',
          args = {
            '-c',
            interpolate(
              'if ! {{exists_check}}; then {{create_script}}; fi',
              { exists_check = exists_check, create_script = create_script }
            ),
          },
        })
      elseif inputs.os == 'windows' then
        local exists_check = windows_group_exists_check(inputs.groupname)
        local create_script = windows_create_group_script(inputs.groupname, {
          description = inputs.description,
        })

        ctx:exec({
          bin = 'powershell.exe',
          args = {
            '-NoProfile',
            '-Command',
            interpolate(
              'if (-not {{exists_check}}) { {{create_script}} }',
              { exists_check = exists_check, create_script = create_script }
            ),
          },
        })
      end

      return { groupname = inputs.groupname, os = inputs.os }
    end,

    update = function(_outputs, inputs, ctx)
      if inputs.os == 'linux' then
        print(
          string.format(
            "Warning: group '%s' description cannot be updated on Linux (groupmod limitation). Recreate group to change.\n",
            inputs.groupname
          )
        )
      elseif inputs.os == 'darwin' then
        local update_script = darwin_update_group_script(inputs.groupname, {
          description = inputs.description,
        })
        ctx:exec({
          bin = '/bin/sh',
          args = { '-c', update_script },
        })
      elseif inputs.os == 'windows' then
        local update_script = windows_update_group_script(inputs.groupname, {
          description = inputs.description,
        })
        ctx:exec({
          bin = 'powershell.exe',
          args = { '-NoProfile', '-Command', update_script },
        })
      end

      return { groupname = inputs.groupname, os = inputs.os }
    end,

    destroy = function(outputs, ctx)
      local members = get_group_members(outputs.groupname, outputs.os)
      if #members > 0 then
        print(
          string.format(
            "Warning: deleting group '%s' which has %d member(s): %s\n",
            outputs.groupname,
            #members,
            table.concat(members, ', ')
          )
        )
      end

      if outputs.os == 'linux' then
        local exists_check = linux_group_exists_check(outputs.groupname)
        local bin, args = linux_delete_group_cmd(outputs.groupname)
        local delete_cmd = bin .. ' ' .. table.concat(args, ' ')

        ctx:exec({
          bin = '/bin/sh',
          args = {
            '-c',
            interpolate(
              'if {{exists_check}}; then {{delete_cmd}}; fi',
              { exists_check = exists_check, delete_cmd = delete_cmd }
            ),
          },
        })
      elseif outputs.os == 'darwin' then
        local exists_check = darwin_group_exists_check(outputs.groupname)
        local bin, args = darwin_delete_group_cmd(outputs.groupname)
        local delete_cmd = bin .. ' ' .. table.concat(args, ' ')

        ctx:exec({
          bin = '/bin/sh',
          args = {
            '-c',
            interpolate(
              'if {{exists_check}}; then {{delete_cmd}}; fi',
              { exists_check = exists_check, delete_cmd = delete_cmd }
            ),
          },
        })
      elseif outputs.os == 'windows' then
        local exists_check = windows_group_exists_check(outputs.groupname)
        local delete_script = windows_delete_group_script(outputs.groupname)

        ctx:exec({
          bin = 'powershell.exe',
          args = {
            '-NoProfile',
            '-Command',
            interpolate(
              'if ({{exists_check}}) { {{delete_script}} }',
              { exists_check = exists_check, delete_script = delete_script }
            ),
          },
        })
      end
    end,
  })
end

-- ============================================================================
-- Public API
-- ============================================================================

---Set up groups according to the provided definitions
---@param groups syslua.groups.GroupMap
function M.setup(groups)
  if not groups or next(groups) == nil then
    error('syslua.groups.setup: at least one group definition is required', 2)
  end

  for name, opts in pairs(groups) do
    local merged = prio.merge(M.defaults, opts)
    if not merged then
      error(interpolate("group '{{name}}': failed to merge options", { name = name }), 2)
    end

    validate_group_options(name, merged)
    create_group_bind(name, merged)
  end
end

return M
