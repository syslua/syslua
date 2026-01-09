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

---Add user to group on macOS
---@param username string
---@param group string
---@return string bin, string[] args
local function darwin_add_to_group_cmd(username, group)
  return '/usr/sbin/dseditgroup', { '-o', 'edit', '-a', username, '-t', 'user', group }
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

---Build Linux user deletion command
---@param name string
---@param preserve_home boolean
---@return string bin, string[] args
local function linux_delete_user_cmd(name, preserve_home)
  local args = {}
  if not preserve_home then
    table.insert(args, '-r')
  end
  table.insert(args, name)
  return '/usr/sbin/userdel', args
end

---Build macOS user deletion command
---@param name string
---@param preserve_home boolean
---@return string bin, string[] args
local function darwin_delete_user_cmd(name, preserve_home)
  local args = { '-deleteUser', name }
  if preserve_home then
    table.insert(args, '-keepHome')
  else
    table.insert(args, '-secure')
  end
  return '/usr/sbin/sysadminctl', args
end

---Build Windows user deletion PowerShell script
---@param name string
---@param home_dir string
---@param preserve_home boolean
---@return string
local function windows_delete_user_script(name, home_dir, preserve_home)
  local lines = {
    string.format('Remove-LocalUser -Name "%s"', name),
  }
  if not preserve_home then
    table.insert(lines, string.format('Remove-Item -Recurse -Force "%s" -ErrorAction SilentlyContinue', home_dir))
  end
  return table.concat(lines, '; ')
end

-- ============================================================================
-- User Config Execution
-- ============================================================================

---Get the store path for a user
---@param home_dir string
---@return string
local function get_user_store(home_dir)
  return home_dir .. '/.syslua/store'
end

---Get the parent store path (system store)
---@return string
local function get_parent_store()
  -- Use the current store as parent for user subprocesses
  local store = sys.getenv('SYSLUA_STORE')
  if store and store ~= '' then
    return store
  end
  -- Fallback to default system store
  if sys.os == 'windows' then
    local drive = sys.getenv('SYSTEMDRIVE') or 'C:'
    return drive .. '\\syslua\\store'
  else
    return '/syslua/store'
  end
end

---Resolve config path (file or directory with init.lua)
---@param config_path string
---@return string
local function resolve_config_path(config_path)
  -- If it's a directory, append init.lua
  -- The actual check happens at runtime in the bind
  if config_path:match('%.lua$') then
    return config_path
  else
    return config_path .. '/init.lua'
  end
end

---Build Unix command to run sys apply as user
---@param username string
---@param home_dir string
---@param config_path string
---@return string bin, string[] args
local function unix_run_as_user_cmd(username, home_dir, config_path)
  local user_store = get_user_store(home_dir)
  local parent_store = get_parent_store()
  local resolved_config = resolve_config_path(config_path)

  local env_prefix = string.format('SYSLUA_STORE=%s SYSLUA_PARENT_STORE=%s', user_store, parent_store)

  local cmd = string.format('%s sys apply %s', env_prefix, resolved_config)

  return '/bin/su', { '-', username, '-c', cmd }
end

---Build Unix command to run sys destroy as user
---@param username string
---@param home_dir string
---@return string bin, string[] args
local function unix_destroy_as_user_cmd(username, home_dir)
  local user_store = get_user_store(home_dir)
  local parent_store = get_parent_store()

  local env_prefix = string.format('SYSLUA_STORE=%s SYSLUA_PARENT_STORE=%s', user_store, parent_store)

  local cmd = string.format('%s sys destroy', env_prefix)

  return '/bin/su', { '-', username, '-c', cmd }
end

---Build Windows command to run sys apply as user (via scheduled task)
---@param username string
---@param home_dir string
---@param config_path string
---@return string
local function windows_run_as_user_script(username, home_dir, config_path)
  local user_store = get_user_store(home_dir):gsub('/', '\\')
  local parent_store = get_parent_store():gsub('/', '\\')
  local resolved_config = resolve_config_path(config_path):gsub('/', '\\')

  return string.format(
    [[
$env:SYSLUA_STORE = "%s"
$env:SYSLUA_PARENT_STORE = "%s"
$taskName = "SysluaApply_%s"
$action = New-ScheduledTaskAction -Execute "sys" -Argument "apply %s"
$principal = New-ScheduledTaskPrincipal -UserId "%s" -LogonType Interactive
Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal -Force | Out-Null
Start-ScheduledTask -TaskName $taskName
Start-Sleep -Seconds 2
Unregister-ScheduledTask -TaskName $taskName -Confirm:$false
]],
    user_store,
    parent_store,
    username,
    resolved_config,
    username
  )
end

---Build Windows command to run sys destroy as user
---@param username string
---@param home_dir string
---@return string
local function windows_destroy_as_user_script(username, home_dir)
  local user_store = get_user_store(home_dir):gsub('/', '\\')
  local parent_store = get_parent_store():gsub('/', '\\')

  return string.format(
    [[
$env:SYSLUA_STORE = "%s"
$env:SYSLUA_PARENT_STORE = "%s"
$taskName = "SysluaDestroy_%s"
$action = New-ScheduledTaskAction -Execute "sys" -Argument "destroy"
$principal = New-ScheduledTaskPrincipal -UserId "%s" -LogonType Interactive
Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal -Force | Out-Null
Start-ScheduledTask -TaskName $taskName
Start-Sleep -Seconds 2
Unregister-ScheduledTask -TaskName $taskName -Confirm:$false
]],
    user_store,
    parent_store,
    username,
    username
  )
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

---Create a bind for a single user
---@param name string
---@param opts syslua.user.Options
local function create_user_bind(name, opts)
  local bind_id = BIND_ID_PREFIX .. name

  sys.bind({
    id = bind_id,
    replace = true,
    inputs = {
      username = name,
      description = opts.description or '',
      home_dir = opts.homeDir,
      config_path = opts.config,
      shell = opts.shell,
      initial_password = opts.initialPassword,
      groups = opts.groups or {},
      preserve_home = opts.preserveHomeOnRemove or false,
      os = sys.os,
    },
    create = function(inputs, ctx)
      -- Step 1: Create the user account
      if inputs.os == 'linux' then
        local bin, args = linux_create_user_cmd(inputs.username, {
          description = inputs.description,
          homeDir = inputs.home_dir,
          shell = inputs.shell,
          groups = inputs.groups,
        })
        ctx:exec({ bin = bin, args = args })

        -- Set password separately on Linux
        if inputs.initial_password and inputs.initial_password ~= '' then
          ctx:exec({
            bin = '/bin/sh',
            args = {
              '-c',
              string.format('echo "%s:%s" | chpasswd', inputs.username, inputs.initial_password),
            },
          })
        end
      elseif inputs.os == 'darwin' then
        local bin, args = darwin_create_user_cmd(inputs.username, {
          description = inputs.description,
          homeDir = inputs.home_dir,
          shell = inputs.shell,
          initialPassword = inputs.initial_password,
        })
        ctx:exec({ bin = bin, args = args })

        -- Add to groups separately on macOS
        for _, group in ipairs(inputs.groups) do
          local grp_bin, grp_args = darwin_add_to_group_cmd(inputs.username, group)
          ctx:exec({ bin = grp_bin, args = grp_args })
        end
      elseif inputs.os == 'windows' then
        local script = windows_create_user_script(inputs.username, {
          description = inputs.description,
          homeDir = inputs.home_dir,
          initialPassword = inputs.initial_password,
          groups = inputs.groups,
        })
        ctx:exec({
          bin = 'powershell.exe',
          args = { '-NoProfile', '-Command', script },
        })
      end

      -- Step 2: Apply user's syslua config
      if inputs.os == 'windows' then
        local script = windows_run_as_user_script(inputs.username, inputs.home_dir, inputs.config_path)
        ctx:exec({
          bin = 'powershell.exe',
          args = { '-NoProfile', '-Command', script },
        })
      else
        local bin, args = unix_run_as_user_cmd(inputs.username, inputs.home_dir, inputs.config_path)
        ctx:exec({ bin = bin, args = args })
      end

      return {
        username = inputs.username,
        home_dir = inputs.home_dir,
        preserve_home = inputs.preserve_home,
      }
    end,
    destroy = function(outputs, ctx)
      -- Step 1: Destroy user's syslua config
      if sys.os == 'windows' then
        local script = windows_destroy_as_user_script(outputs.username, outputs.home_dir)
        ctx:exec({
          bin = 'powershell.exe',
          args = { '-NoProfile', '-Command', script },
        })
      else
        local bin, args = unix_destroy_as_user_cmd(outputs.username, outputs.home_dir)
        ctx:exec({ bin = bin, args = args })
      end

      -- Step 2: Remove user account
      if sys.os == 'linux' then
        local bin, args = linux_delete_user_cmd(outputs.username, outputs.preserve_home)
        ctx:exec({ bin = bin, args = args })
      elseif sys.os == 'darwin' then
        local bin, args = darwin_delete_user_cmd(outputs.username, outputs.preserve_home)
        ctx:exec({ bin = bin, args = args })
      elseif sys.os == 'windows' then
        local script = windows_delete_user_script(outputs.username, outputs.home_dir, outputs.preserve_home)
        ctx:exec({
          bin = 'powershell.exe',
          args = { '-NoProfile', '-Command', script },
        })
      end
    end,
  })
end

---Set up users according to the provided definitions
---@param users syslua.user.UserMap
function M.setup(users)
  if not users or next(users) == nil then
    error('syslua.user.setup: at least one user definition is required', 2)
  end

  for name, opts in pairs(users) do
    validate_user_options(name, opts)
    create_user_bind(name, opts)
  end
end

return M
