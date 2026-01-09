---@class syslua.user
local M = {}

-- ============================================================================
-- Type Definitions
-- ============================================================================

---@class syslua.user.Options
---@field description? string User description/comment
---@field homeDir string Home directory path (required)
---@field config string Path to user's syslua config (required)
---@field shell? BuildRef Login shell package
---@field initialPassword? string Initial password (plaintext, set on creation only)
---@field groups? string[] Groups to add user to (must exist)
---@field preserveHomeOnRemove? boolean Keep home directory when user is removed (default: false)

---@alias syslua.user.UserMap table<string, syslua.user.Options>

-- ============================================================================
-- Constants
-- ============================================================================

local BIND_ID_PREFIX = '__syslua_user_'

-- ============================================================================
-- Platform-Specific Commands
-- ============================================================================

---Get the default shell for the current platform
---@return string
local function get_default_shell()
  if sys.os == 'windows' then
    return 'cmd.exe'
  elseif sys.os == 'darwin' then
    return '/bin/zsh'
  else
    return '/bin/bash'
  end
end

---Get shell path from BuildRef or use default
---@param shell? BuildRef
---@return string
local function get_shell_path(shell)
  if shell and shell.outputs and shell.outputs.bin then
    return shell.outputs.bin
  end
  return get_default_shell()
end

---Build Linux user creation command
---@param name string
---@param opts syslua.user.Options
---@return string bin, string[] args
local function linux_create_user_cmd(name, opts)
  local args = { '-m', '-d', opts.homeDir }

  if opts.description and opts.description ~= '' then
    table.insert(args, '-c')
    table.insert(args, opts.description)
  end

  local shell = get_shell_path(opts.shell)
  table.insert(args, '-s')
  table.insert(args, shell)

  if opts.groups and #opts.groups > 0 then
    table.insert(args, '-G')
    table.insert(args, table.concat(opts.groups, ','))
  end

  table.insert(args, name)

  return '/usr/sbin/useradd', args
end

---Build macOS user creation command
---@param name string
---@param opts syslua.user.Options
---@return string bin, string[] args
local function darwin_create_user_cmd(name, opts)
  local args = { '-addUser', name }

  if opts.description and opts.description ~= '' then
    table.insert(args, '-fullName')
    table.insert(args, opts.description)
  end

  table.insert(args, '-home')
  table.insert(args, opts.homeDir)

  local shell = get_shell_path(opts.shell)
  table.insert(args, '-shell')
  table.insert(args, shell)

  if opts.initialPassword then
    table.insert(args, '-password')
    table.insert(args, opts.initialPassword)
  end

  return '/usr/sbin/sysadminctl', args
end

---Build Windows user creation PowerShell script
---@param name string
---@param opts syslua.user.Options
---@return string
local function windows_create_user_script(name, opts)
  local lines = {}

  -- Create user
  if opts.initialPassword then
    table.insert(
      lines,
      string.format('$securePass = ConvertTo-SecureString "%s" -AsPlainText -Force', opts.initialPassword)
    )
    table.insert(
      lines,
      string.format('New-LocalUser -Name "%s" -Description "%s" -Password $securePass', name, opts.description or '')
    )
  else
    table.insert(
      lines,
      string.format('New-LocalUser -Name "%s" -Description "%s" -NoPassword', name, opts.description or '')
    )
  end

  -- Create home directory
  table.insert(lines, string.format('New-Item -ItemType Directory -Path "%s" -Force | Out-Null', opts.homeDir))

  -- Add to groups
  if opts.groups then
    for _, group in ipairs(opts.groups) do
      table.insert(lines, string.format('Add-LocalGroupMember -Group "%s" -Member "%s" -ErrorAction Stop', group, name))
    end
  end

  return table.concat(lines, '; ')
end

-- ============================================================================
-- Validation
-- ============================================================================

---@param name string
---@param opts syslua.user.Options
local function validate_user_options(name, opts)
  if not opts.homeDir then
    error(string.format("user '%s': homeDir is required", name), 0)
  end
  if not opts.config then
    error(string.format("user '%s': config is required", name), 0)
  end
  if not sys.is_elevated then
    error('syslua.user requires elevated privileges (root/Administrator)', 0)
  end
end

-- ============================================================================
-- Public API
-- ============================================================================

---Set up users according to the provided definitions
---@param users syslua.user.UserMap
function M.setup(users)
  if not users or next(users) == nil then
    error('syslua.user.setup: at least one user definition is required', 2)
  end

  for name, opts in pairs(users) do
    validate_user_options(name, opts)
    -- TODO: Create bind for user
  end
end

return M
