---@class syslua.lib.programs
local M = {}

---@return string
local function get_home()
  return sys.getenv('HOME')
end

---@return table<string, string>
function M.get_completion_paths()
  local home = get_home()

  if sys.is_elevated then
    return {
      bash = sys.os == 'linux' and '/usr/share/bash-completion/completions/'
        or '/usr/local/share/bash-completion/completions/',
      zsh = '/usr/local/share/zsh/site-functions/',
      fish = sys.os == 'linux' and '/usr/share/fish/vendor_completions.d/'
        or '/usr/local/share/fish/vendor_completions.d/',
    }
  else
    return {
      bash = home .. '/.local/share/bash-completion/completions/',
      zsh = home .. '/.zsh/completions/',
      fish = home .. '/.config/fish/completions/',
    }
  end
end

---@return table<string, string>
function M.get_man_paths()
  local home = get_home()

  if sys.is_elevated then
    return {
      man1 = sys.os == 'linux' and '/usr/share/man/man1/' or '/usr/local/share/man/man1/',
    }
  else
    return {
      man1 = home .. '/.local/share/man/man1/',
    }
  end
end

---@return string
function M.get_powershell_profile()
  local home = get_home()
  if sys.is_elevated then
    return 'C:\\Program Files\\PowerShell\\7\\profile.ps1'
  else
    return home .. '/Documents/PowerShell/profile.ps1'
  end
end

---@param pkg_build table
---@param name string
---@param completions table
---@param opts table
function M.create_completion_binds(pkg_build, name, completions, opts)
  local paths = M.get_completion_paths()

  if opts.bash_integration and completions.bash then
    sys.bind({
      id = '__syslua_programs_' .. name .. '_bash_completion',
      replace = true,
      inputs = {
        source = pkg_build.outputs.out .. '/' .. completions.bash,
        target = paths.bash .. name,
      },
      create = function(inputs, ctx)
        ctx:exec({
          bin = '/bin/sh',
          args = {
            '-c',
            string.format(
              'mkdir -p "$(dirname "%s")" && ln -sf "%s" "%s"',
              inputs.target,
              inputs.source,
              inputs.target
            ),
          },
        })
        return { link = inputs.target }
      end,
      destroy = function(outputs, ctx)
        ctx:exec({
          bin = '/bin/sh',
          args = { '-c', string.format('rm -f "%s"', outputs.link) },
        })
      end,
    })
  end

  if opts.zsh_integration and completions.zsh then
    sys.bind({
      id = '__syslua_programs_' .. name .. '_zsh_completion',
      replace = true,
      inputs = {
        source = pkg_build.outputs.out .. '/' .. completions.zsh,
        target = paths.zsh .. '_' .. name,
      },
      create = function(inputs, ctx)
        ctx:exec({
          bin = '/bin/sh',
          args = {
            '-c',
            string.format(
              'mkdir -p "$(dirname "%s")" && ln -sf "%s" "%s"',
              inputs.target,
              inputs.source,
              inputs.target
            ),
          },
        })
        return { link = inputs.target }
      end,
      destroy = function(outputs, ctx)
        ctx:exec({
          bin = '/bin/sh',
          args = { '-c', string.format('rm -f "%s"', outputs.link) },
        })
      end,
    })
  end

  if opts.fish_integration and completions.fish then
    sys.bind({
      id = '__syslua_programs_' .. name .. '_fish_completion',
      replace = true,
      inputs = {
        source = pkg_build.outputs.out .. '/' .. completions.fish,
        target = paths.fish .. name .. '.fish',
      },
      create = function(inputs, ctx)
        ctx:exec({
          bin = '/bin/sh',
          args = {
            '-c',
            string.format(
              'mkdir -p "$(dirname "%s")" && ln -sf "%s" "%s"',
              inputs.target,
              inputs.source,
              inputs.target
            ),
          },
        })
        return { link = inputs.target }
      end,
      destroy = function(outputs, ctx)
        ctx:exec({
          bin = '/bin/sh',
          args = { '-c', string.format('rm -f "%s"', outputs.link) },
        })
      end,
    })
  end

  if opts.powershell_integration and completions.ps1 then
    local shell_configs = M.get_powershell_profile()
    local BEGIN_MARKER = '# BEGIN SYSLUA ' .. name:upper() .. ' COMPLETION - DO NOT EDIT'
    local END_MARKER = '# END SYSLUA ' .. name:upper() .. ' COMPLETION'

    sys.bind({
      id = '__syslua_programs_' .. name .. '_ps1_completion',
      replace = true,
      inputs = {
        source = pkg_build.outputs.out .. '/' .. completions.ps1,
        config_path = shell_configs,
        begin_marker = BEGIN_MARKER,
        end_marker = END_MARKER,
      },
      create = function(inputs, ctx)
        ctx:exec({
          bin = 'powershell.exe',
          args = {
            '-NoProfile',
            '-Command',
            string.format(
              [[
$configPath = '%s'
$configDir = Split-Path -Parent $configPath
if (-not (Test-Path $configDir)) { New-Item -ItemType Directory -Path $configDir -Force | Out-Null }
if (-not (Test-Path $configPath)) { New-Item -ItemType File -Path $configPath -Force | Out-Null }
$content = Get-Content $configPath -Raw -ErrorAction SilentlyContinue
if ($content -notmatch [regex]::Escape('%s')) {
  Add-Content -Path $configPath -Value "`n%s`n. `"%s`"`n%s"
}
]],
              inputs.config_path,
              inputs.begin_marker,
              inputs.begin_marker,
              inputs.source,
              inputs.end_marker
            ),
          },
        })
        return { config = inputs.config_path }
      end,
      destroy = function(outputs, ctx)
        ctx:exec({
          bin = 'powershell.exe',
          args = {
            '-NoProfile',
            '-Command',
            string.format(
              [[
$configPath = '%s'
if (Test-Path $configPath) {
  $content = Get-Content $configPath -Raw
  $pattern = '(?s)%s.*?%s\r?\n?'
  $newContent = $content -replace $pattern, ''
  Set-Content -Path $configPath -Value $newContent -NoNewline
}
]],
              outputs.config,
              BEGIN_MARKER,
              END_MARKER
            ),
          },
        })
      end,
    })
  end
end

---@param pkg_build table
---@param man_source string
---@param man_name string
function M.create_man_bind(pkg_build, man_source, man_name)
  if sys.os == 'windows' then
    return
  end

  local paths = M.get_man_paths()

  sys.bind({
    id = '__syslua_programs_' .. man_name:gsub('%.', '_') .. '_man',
    replace = true,
    inputs = {
      source = pkg_build.outputs.out .. '/' .. man_source,
      target = paths.man1 .. man_name,
    },
    create = function(inputs, ctx)
      ctx:exec({
        bin = '/bin/sh',
        args = {
          '-c',
          string.format('mkdir -p "$(dirname "%s")" && ln -sf "%s" "%s"', inputs.target, inputs.source, inputs.target),
        },
      })
      return { link = inputs.target }
    end,
    destroy = function(outputs, ctx)
      ctx:exec({
        bin = '/bin/sh',
        args = { '-c', string.format('rm -f "%s"', outputs.link) },
      })
    end,
  })
end

return M
