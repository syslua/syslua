local prio = require('syslua.priority')
local f = require('syslua.interpolation')

---@class syslua.users
local M = {}

-- ============================================================================
-- Type Definitions
-- ============================================================================

---@class syslua.users.UserOptions
---@field description? syslua.Option<string> User description/comment
---@field homeDir syslua.Option<string> Home directory path (required)
---@field config syslua.Option<string> Path to user's syslua config (required)
---@field shell? syslua.Option<BuildRef> Login shell package
---@field initialPassword? syslua.Option<string> Initial password (plaintext, set on creation only)
---@field groups? syslua.MergeableOption<string[]> Groups to add user to (must exist)
---@field preserveHomeOnRemove? syslua.Option<boolean> Keep home directory when user is removed (default: false)

---@alias syslua.users.Options table<string, syslua.users.UserOptions>

-- Command builder input types (subset of Options for type safety)

---@class syslua.users.CreateCmdOpts
---@field description? string User description/comment
---@field homeDir string Home directory path (required)
---@field shell? BuildRef Login shell package
---@field groups? string[] Groups to add user to
---@field initialPassword? string Initial password (plaintext)

---@class syslua.users.UpdateCmdOpts
---@field description? string User description/comment
---@field shell? BuildRef Login shell package
---@field groups? string[] Groups to add user to

---@class syslua.users.DescriptionOnlyOpts
---@field description? string User description/comment

---@class syslua.users.UserOptionsDefaults
---@field description string
---@field homeDir nil
---@field config nil
---@field shell nil
---@field initialPassword nil
---@field groups syslua.MergeableOption<string[]>
---@field preserveHomeOnRemove boolean

-- ============================================================================
-- Constants
-- ============================================================================

local BIND_ID_PREFIX = '__syslua_user_'

-- ============================================================================
-- Default Options
-- ============================================================================

---@type syslua.users.UserOptionsDefaults
M.defaults = {
  description = '',
  groups = prio.mergeable({ default = {} }),
  preserveHomeOnRemove = false,
}

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
---@param opts syslua.users.CreateCmdOpts
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
---@param opts syslua.users.CreateCmdOpts
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

---Build Linux user update command (for existing users)
---@param name string
---@param opts syslua.users.UpdateCmdOpts
---@return string bin, string[] args
local function linux_update_user_cmd(name, opts)
  local args = {}

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

  return '/usr/sbin/usermod', args
end

---Build macOS user update commands (returns shell script)
---@param name string
---@param opts syslua.users.UpdateCmdOpts
---@return string
local function darwin_update_user_script(name, opts)
  local cmds = {}

  if opts.description and opts.description ~= '' then
    table.insert(
      cmds,
      f('dscl . -create /Users/{{name}} RealName "{{description}}"', { name = name, description = opts.description })
    )
  end

  local shell = get_shell_path(opts.shell)
  table.insert(cmds, f('dscl . -create /Users/{{name}} UserShell "{{shell}}"', { name = name, shell = shell }))

  return table.concat(cmds, ' && ')
end

---Build Windows user update PowerShell script (for existing users)
---@param name string
---@param opts syslua.users.DescriptionOnlyOpts
---@return string
local function windows_update_user_script(name, opts)
  local description = opts.description or ''
  return f('Set-LocalUser -Name "{{name}}" -Description "{{description}}"', { name = name, description = description })
end

---Build Windows user creation PowerShell script
---@param name string
---@param opts syslua.users.CreateCmdOpts
---@return string
local function windows_create_user_script(name, opts)
  local lines = {}
  local description = opts.description or ''

  -- Create user
  if opts.initialPassword then
    table.insert(
      lines,
      f('$securePass = ConvertTo-SecureString "{{password}}" -AsPlainText -Force', { password = opts.initialPassword })
    )
    table.insert(
      lines,
      f(
        'New-LocalUser -Name "{{name}}" -Description "{{description}}" -Password $securePass',
        { name = name, description = description }
      )
    )
  else
    table.insert(
      lines,
      f(
        'New-LocalUser -Name "{{name}}" -Description "{{description}}" -NoPassword',
        { name = name, description = description }
      )
    )
  end

  -- Create home directory
  table.insert(
    lines,
    f('New-Item -ItemType Directory -Path "{{homeDir}}" -Force | Out-Null', { homeDir = opts.homeDir })
  )

  -- Add to groups
  if opts.groups then
    for _, group in ipairs(opts.groups) do
      table.insert(
        lines,
        f(
          'Add-LocalGroupMember -Group "{{group}}" -Member "{{name}}" -ErrorAction Stop',
          { group = group, name = name }
        )
      )
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
    f('Remove-LocalUser -Name "{{name}}"', { name = name }),
  }
  if not preserve_home then
    table.insert(
      lines,
      f('Remove-Item -Recurse -Force "{{home_dir}}" -ErrorAction SilentlyContinue', { home_dir = home_dir })
    )
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

  local cmd = f(
    'SYSLUA_STORE={{user_store}} SYSLUA_PARENT_STORE={{parent_store}} sys apply {{config}}',
    { user_store = user_store, parent_store = parent_store, config = resolved_config }
  )

  return '/bin/su', { '-', username, '-c', cmd }
end

---Build Unix command to run sys destroy as user
---@param username string
---@param home_dir string
---@return string bin, string[] args
local function unix_destroy_as_user_cmd(username, home_dir)
  local user_store = get_user_store(home_dir)
  local parent_store = get_parent_store()

  local cmd = f(
    'SYSLUA_STORE={{user_store}} SYSLUA_PARENT_STORE={{parent_store}} sys destroy',
    { user_store = user_store, parent_store = parent_store }
  )

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

  return f(
    [[
$env:SYSLUA_STORE = "{{user_store}}"
$env:SYSLUA_PARENT_STORE = "{{parent_store}}"
$taskName = "SysluaApply_{{username}}"
$action = New-ScheduledTaskAction -Execute "sys" -Argument "apply {{config}}"
$principal = New-ScheduledTaskPrincipal -UserId "{{username}}" -LogonType Interactive
try {
  Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal -Force | Out-Null
  Start-ScheduledTask -TaskName $taskName
  $timeout = 300
  $elapsed = 0
  while (($task = Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue) -and $task.State -eq 'Running' -and $elapsed -lt $timeout) {
    Start-Sleep -Seconds 1
    $elapsed++
  }
  $info = Get-ScheduledTaskInfo -TaskName $taskName -ErrorAction SilentlyContinue
  if ($info -and $info.LastTaskResult -ne 0) {
    throw "sys apply failed with exit code $($info.LastTaskResult)"
  }
} finally {
  Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue
}
]],
    { user_store = user_store, parent_store = parent_store, username = username, config = resolved_config }
  )
end

---Build Windows command to run sys destroy as user
---@param username string
---@param home_dir string
---@return string
local function windows_destroy_as_user_script(username, home_dir)
  local user_store = get_user_store(home_dir):gsub('/', '\\')
  local parent_store = get_parent_store():gsub('/', '\\')

  return f(
    [[
$env:SYSLUA_STORE = "{{user_store}}"
$env:SYSLUA_PARENT_STORE = "{{parent_store}}"
$taskName = "SysluaDestroy_{{username}}"
$action = New-ScheduledTaskAction -Execute "sys" -Argument "destroy"
$principal = New-ScheduledTaskPrincipal -UserId "{{username}}" -LogonType Interactive
try {
  Register-ScheduledTask -TaskName $taskName -Action $action -Principal $principal -Force | Out-Null
  Start-ScheduledTask -TaskName $taskName
  $timeout = 300
  $elapsed = 0
  while (($task = Get-ScheduledTask -TaskName $taskName -ErrorAction SilentlyContinue) -and $task.State -eq 'Running' -and $elapsed -lt $timeout) {
    Start-Sleep -Seconds 1
    $elapsed++
  }
  $info = Get-ScheduledTaskInfo -TaskName $taskName -ErrorAction SilentlyContinue
  if ($info -and $info.LastTaskResult -ne 0) {
    throw "sys destroy failed with exit code $($info.LastTaskResult)"
  }
} finally {
  Unregister-ScheduledTask -TaskName $taskName -Confirm:$false -ErrorAction SilentlyContinue
}
]],
    { user_store = user_store, parent_store = parent_store, username = username }
  )
end

-- ============================================================================
-- User Existence Checks
-- ============================================================================

---Check if user exists on Linux
---@param username string
---@return string
local function linux_user_exists_check(username)
  return f('id "{{username}}" >/dev/null 2>&1', { username = username })
end

---Check if user exists on macOS
---@param username string
---@return string
local function darwin_user_exists_check(username)
  return f('dscl . -read /Users/{{username}} >/dev/null 2>&1', { username = username })
end

---Check if user exists on Windows (PowerShell condition expression)
---@param username string
---@return string
local function windows_user_exists_check(username)
  return f('(Get-LocalUser -Name "{{username}}" -ErrorAction SilentlyContinue)', { username = username })
end

-- ============================================================================
-- Validation Helpers
-- ============================================================================

---Get all existing groups on Linux (batch operation)
---@return table<string, boolean>
local function linux_get_all_groups()
  local handle = io.popen('getent group | cut -d: -f1')
  if not handle then
    error('Failed to execute getent: io.popen returned nil')
  end
  local existing = {}
  for line in handle:lines() do
    existing[line] = true
  end
  handle:close()
  return existing
end

---Get all existing groups on macOS (batch operation)
---@return table<string, boolean>
local function darwin_get_all_groups()
  local handle = io.popen('dscl . -list /Groups')
  if not handle then
    error('Failed to execute dscl: io.popen returned nil')
  end
  local existing = {}
  for line in handle:lines() do
    existing[line] = true
  end
  handle:close()
  return existing
end

---Get all existing groups on Windows (batch operation)
---@return table<string, boolean>
local function windows_get_all_groups()
  local handle = io.popen('powershell -NoProfile -Command "Get-LocalGroup | ForEach-Object { $_.Name }"')
  if not handle then
    error('Failed to execute PowerShell: io.popen returned nil')
  end
  local existing = {}
  for line in handle:lines() do
    local group = line:gsub('%s+$', '')
    if group ~= '' then
      existing[group] = true
    end
  end
  handle:close()
  return existing
end

---Get all existing groups (cross-platform, single shell spawn)
---@return table<string, boolean>
local function get_all_groups()
  if sys.os == 'linux' then
    return linux_get_all_groups()
  elseif sys.os == 'darwin' then
    return darwin_get_all_groups()
  elseif sys.os == 'windows' then
    return windows_get_all_groups()
  end
  return {}
end

---Validate that all groups exist (batch operation)
---@param groups string[] List of groups to check
---@return string[] List of missing groups
local function validate_groups_exist(groups)
  if #groups == 0 then
    return {}
  end

  local existing = get_all_groups()
  local missing = {}
  for _, group in ipairs(groups) do
    if not existing[group] then
      table.insert(missing, group)
    end
  end
  return missing
end

---Check if a config path exists (file or directory with init.lua)
---@param config_path string
---@return boolean, string? -- exists, resolved_path
local function validate_config_path(config_path)
  -- Check if it's a file ending in .lua
  if config_path:match('%.lua$') then
    local file = io.open(config_path, 'r')
    if file then
      file:close()
      return true, config_path
    end
    return false, nil
  end

  -- Check if it's a directory with init.lua
  local init_path = config_path .. '/init.lua'
  local file = io.open(init_path, 'r')
  if file then
    file:close()
    return true, init_path
  end

  -- Check if the path itself is a file (without .lua extension)
  file = io.open(config_path, 'r')
  if file then
    file:close()
    return true, config_path
  end

  return false, nil
end

-- ============================================================================
-- Validation
-- ============================================================================

---@param name string
---@param opts syslua.users.UserOptions
---@param groups string[]
---@param missing_groups_set table<string, boolean> Pre-computed set of missing groups
local function validate_user_options(name, opts, groups, missing_groups_set)
  local home_dir = prio.unwrap(opts.homeDir)
  local config = prio.unwrap(opts.config) --[[@as string]]

  if not home_dir then
    error(f("user '{{name}}': homeDir is required", { name = name }), 0)
  end
  if not config then
    error(f("user '{{name}}': config is required", { name = name }), 0)
  end
  if not sys.is_elevated then
    error('syslua.user requires elevated privileges (root/Administrator)', 0)
  end

  -- Validate config path exists
  local config_exists = validate_config_path(config)
  if not config_exists then
    error(f("user '{{name}}': config path does not exist: {{config}}", { name = name, config = config }), 0)
  end

  -- Check user's groups against pre-validated set
  local user_missing = {}
  for _, group in ipairs(groups) do
    if missing_groups_set[group] then
      table.insert(user_missing, group)
    end
  end
  if #user_missing > 0 then
    error(
      f("user '{{name}}': groups do not exist: {{groups}}", { name = name, groups = table.concat(user_missing, ', ') }),
      0
    )
  end
end

-- ============================================================================
-- Public API
-- ============================================================================

---Resolve groups from merged options (handles Mergeable type)
---@param groups_opt syslua.MergeableOption<string[]>|nil
---@return string[]
local function resolve_groups(groups_opt)
  if not groups_opt then
    return {}
  end
  -- If it's a mergeable, access will resolve it
  if prio.is_mergeable(groups_opt) then
    -- Access the merged value through the MergedTable mechanism
    -- For mergeables without separator, result is an array
    local result = {}
    for _, entry in ipairs(groups_opt.__entries or {}) do
      local val = entry.value
      if type(val) == 'table' then
        for _, v in ipairs(val) do
          table.insert(result, v)
        end
      else
        table.insert(result, val)
      end
    end
    return result
  end
  -- Otherwise unwrap and return
  local unwrapped = prio.unwrap(groups_opt)
  if type(unwrapped) == 'table' then
    return unwrapped
  end
  return {}
end

---Create a bind for a single user
---@param name string
---@param opts syslua.users.UserOptions
---@param groups string[]
local function create_user_bind(name, opts, groups)
  local bind_id = BIND_ID_PREFIX .. name

  -- Unwrap all priority values for bind inputs
  local description = prio.unwrap(opts.description) or ''
  local home_dir = prio.unwrap(opts.homeDir)
  local config_path = prio.unwrap(opts.config)
  local shell = prio.unwrap(opts.shell)
  local initial_password = prio.unwrap(opts.initialPassword)
  local preserve_home = prio.unwrap(opts.preserveHomeOnRemove) or false

  sys.bind({
    id = bind_id,
    replace = true,
    inputs = {
      username = name,
      description = description,
      home_dir = home_dir,
      config_path = config_path,
      shell = shell,
      initial_password = initial_password,
      groups = groups,
      preserve_home = preserve_home,
      os = sys.os,
    },
    create = function(inputs, ctx)
      -- Step 1: Create or update the user account
      if inputs.os == 'linux' then
        local exists_check = linux_user_exists_check(inputs.username)
        local _, create_args = linux_create_user_cmd(inputs.username, {
          description = inputs.description,
          homeDir = inputs.home_dir,
          shell = inputs.shell,
          groups = inputs.groups,
        })
        local create_cmd = '/usr/sbin/useradd ' .. table.concat(create_args, ' ')

        local _, update_args = linux_update_user_cmd(inputs.username, {
          description = inputs.description,
          shell = inputs.shell,
          groups = inputs.groups,
        })
        local update_cmd = '/usr/sbin/usermod ' .. table.concat(update_args, ' ')

        -- Create if doesn't exist, update if exists
        ctx:exec({
          bin = '/bin/sh',
          args = {
            '-c',
            f(
              'if ! {{exists_check}}; then {{create_cmd}}; else {{update_cmd}}; fi',
              { exists_check = exists_check, create_cmd = create_cmd, update_cmd = update_cmd }
            ),
          },
        })

        -- Set password separately on Linux
        if inputs.initial_password and inputs.initial_password ~= '' then
          ctx:exec({
            bin = '/bin/sh',
            args = {
              '-c',
              f(
                'echo "{{username}}:{{password}}" | chpasswd',
                { username = inputs.username, password = inputs.initial_password }
              ),
            },
          })
        end
      elseif inputs.os == 'darwin' then
        local exists_check = darwin_user_exists_check(inputs.username)
        local _, create_args = darwin_create_user_cmd(inputs.username, {
          description = inputs.description,
          homeDir = inputs.home_dir,
          shell = inputs.shell,
          initialPassword = inputs.initial_password,
        })
        local create_cmd = '/usr/sbin/sysadminctl ' .. table.concat(create_args, ' ')

        local update_script = darwin_update_user_script(inputs.username, {
          description = inputs.description,
          shell = inputs.shell,
        })

        -- Create if doesn't exist, update if exists
        ctx:exec({
          bin = '/bin/sh',
          args = {
            '-c',
            f(
              'if ! {{exists_check}}; then {{create_cmd}}; else {{update_script}}; fi',
              { exists_check = exists_check, create_cmd = create_cmd, update_script = update_script }
            ),
          },
        })

        -- Add to groups separately on macOS (idempotent - dseditgroup handles existing membership)
        for _, group in ipairs(inputs.groups) do
          local grp_bin, grp_args = darwin_add_to_group_cmd(inputs.username, group)
          ctx:exec({ bin = grp_bin, args = grp_args })
        end
      elseif inputs.os == 'windows' then
        local exists_check = windows_user_exists_check(inputs.username)
        local create_script = windows_create_user_script(inputs.username, {
          description = inputs.description,
          homeDir = inputs.home_dir,
          initialPassword = inputs.initial_password,
          groups = inputs.groups,
        })

        local update_script = windows_update_user_script(inputs.username, {
          description = inputs.description,
        })

        -- Create if doesn't exist, update if exists
        ctx:exec({
          bin = 'powershell.exe',
          args = {
            '-NoProfile',
            '-Command',
            f(
              'if (-not {{exists_check}}) { {{create_script}} } else { {{update_script}} }',
              { exists_check = exists_check, create_script = create_script, update_script = update_script }
            ),
          },
        })

        -- Update group membership (idempotent - Add-LocalGroupMember handles existing membership with -ErrorAction SilentlyContinue)
        for _, group in ipairs(inputs.groups) do
          ctx:exec({
            bin = 'powershell.exe',
            args = {
              '-NoProfile',
              '-Command',
              f(
                'Add-LocalGroupMember -Group "{{group}}" -Member "{{username}}" -ErrorAction SilentlyContinue',
                { group = group, username = inputs.username }
              ),
            },
          })
        end
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
      -- Only proceed if user exists (idempotency)
      if sys.os == 'linux' then
        local exists_check = linux_user_exists_check(outputs.username)

        -- Step 1: Destroy user's syslua config (only if user exists)
        local _, destroy_args = unix_destroy_as_user_cmd(outputs.username, outputs.home_dir)
        local destroy_cmd = '/bin/su ' .. table.concat(destroy_args, ' ')
        ctx:exec({
          bin = '/bin/sh',
          args = {
            '-c',
            f(
              'if {{exists_check}}; then {{destroy_cmd}}; fi',
              { exists_check = exists_check, destroy_cmd = destroy_cmd }
            ),
          },
        })

        -- Step 2: Remove user account (only if user exists)
        local _, delete_args = linux_delete_user_cmd(outputs.username, outputs.preserve_home)
        local delete_cmd = '/usr/sbin/userdel ' .. table.concat(delete_args, ' ')
        ctx:exec({
          bin = '/bin/sh',
          args = {
            '-c',
            f('if {{exists_check}}; then {{delete_cmd}}; fi', { exists_check = exists_check, delete_cmd = delete_cmd }),
          },
        })
      elseif sys.os == 'darwin' then
        local exists_check = darwin_user_exists_check(outputs.username)

        -- Step 1: Destroy user's syslua config (only if user exists)
        local _, destroy_args = unix_destroy_as_user_cmd(outputs.username, outputs.home_dir)
        local destroy_cmd = '/bin/su ' .. table.concat(destroy_args, ' ')
        ctx:exec({
          bin = '/bin/sh',
          args = {
            '-c',
            f(
              'if {{exists_check}}; then {{destroy_cmd}}; fi',
              { exists_check = exists_check, destroy_cmd = destroy_cmd }
            ),
          },
        })

        -- Step 2: Remove user account (only if user exists)
        local _, delete_args = darwin_delete_user_cmd(outputs.username, outputs.preserve_home)
        local delete_cmd = '/usr/sbin/sysadminctl ' .. table.concat(delete_args, ' ')
        ctx:exec({
          bin = '/bin/sh',
          args = {
            '-c',
            f('if {{exists_check}}; then {{delete_cmd}}; fi', { exists_check = exists_check, delete_cmd = delete_cmd }),
          },
        })
      elseif sys.os == 'windows' then
        local exists_check = windows_user_exists_check(outputs.username)

        -- Step 1: Destroy user's syslua config (only if user exists)
        local destroy_script = windows_destroy_as_user_script(outputs.username, outputs.home_dir)
        ctx:exec({
          bin = 'powershell.exe',
          args = {
            '-NoProfile',
            '-Command',
            f(
              'if ({{exists_check}}) { {{destroy_script}} }',
              { exists_check = exists_check, destroy_script = destroy_script }
            ),
          },
        })

        -- Step 2: Remove user account (only if user exists)
        local delete_script = windows_delete_user_script(outputs.username, outputs.home_dir, outputs.preserve_home)
        ctx:exec({
          bin = 'powershell.exe',
          args = {
            '-NoProfile',
            '-Command',
            f(
              'if ({{exists_check}}) { {{delete_script}} }',
              { exists_check = exists_check, delete_script = delete_script }
            ),
          },
        })
      end
    end,
  })
end

---Set up users according to the provided definitions
---@param users syslua.users.Options
function M.setup(users)
  if not users or next(users) == nil then
    error('syslua.user.setup: at least one user definition is required', 2)
  end

  -- Phase 1: Collect all data and unique groups from all users
  local user_data = {} -- name -> { merged, groups }
  local all_groups = {} -- unique groups across all users

  for name, opts in pairs(users) do
    local merged = prio.merge(M.defaults, opts)
    if not merged then
      error(f("user '{{name}}': failed to merge options", { name = name }), 2)
    end

    local groups = resolve_groups(merged.groups)
    user_data[name] = { merged = merged, groups = groups }

    -- Collect unique groups
    for _, group in ipairs(groups) do
      all_groups[group] = true
    end
  end

  -- Phase 2: Batch validate all groups with single shell spawn
  local unique_groups = {}
  for group in pairs(all_groups) do
    table.insert(unique_groups, group)
  end
  local missing_groups = validate_groups_exist(unique_groups)
  local missing_set = {}
  for _, group in ipairs(missing_groups) do
    missing_set[group] = true
  end

  -- Phase 3: Validate each user and create binds
  for name, data in pairs(user_data) do
    validate_user_options(name, data.merged, data.groups, missing_set)
    create_user_bind(name, data.merged, data.groups)
  end
end

return M
