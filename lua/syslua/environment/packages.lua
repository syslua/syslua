local prio = require('syslua.priority')
local lib = require('syslua.lib')

---@class syslua.environment.packages
local M = {}

-- ============================================================================
-- Type Definitions
-- ============================================================================

---@class syslua.environment.packages.LinkOptions
---@field bin? boolean Link binaries to ~/.syslua/bin/ (default: true)
---@field man? boolean Link man pages to ~/.syslua/share/man/ (default: true)
---@field completions? boolean|string[] Link completions; true for all shells, or list like {'zsh', 'bash'} (default: true)
---@field lib? boolean Link libraries to ~/.syslua/lib/ (default: false)
---@field include? boolean Link headers to ~/.syslua/include/ (default: false)

---@class syslua.environment.packages.Options
---@field use (BuildRef|syslua.priority.PriorityValue<BuildRef>)[] List of packages to include
---@field link? syslua.environment.packages.LinkOptions What to link
---@field shell_integration? boolean Auto-add PATH to shell configs (default: true)

---@class syslua.environment.packages.ResolvedPackage
---@field pkg BuildRef The package BuildRef
---@field priority number The priority level
---@field name string Package name (from meta or id)

---@class syslua.environment.packages.BinaryEntry
---@field name string Binary filename
---@field source string Full path to binary
---@field pkg_name string Package that provides this binary
---@field priority number Priority level

---@class syslua.environment.packages.ManEntry
---@field name string Man page filename (e.g., "rg.1")
---@field section string Section number (e.g., "1")
---@field source string Full path to man page
---@field pkg_name string Package that provides this

---@class syslua.environment.packages.CompletionEntry
---@field name string Completion filename
---@field shell string Shell type (bash, zsh, fish, powershell)
---@field source string Full path to completion file
---@field pkg_name string Package that provides this

---@class syslua.environment.packages.LibEntry
---@field source string Full path to lib directory
---@field pkg_name string Package that provides this

---@class syslua.environment.packages.IncludeEntry
---@field source string Full path to include directory
---@field pkg_name string Package that provides this

-- ============================================================================
-- Constants
-- ============================================================================

local SYSLUA_DIR = lib.get_home() .. '/.syslua'

-- Shell markers for shell integration
local BEGIN_MARKER = '# BEGIN SYSLUA PACKAGES'
local END_MARKER = '# END SYSLUA PACKAGES'

-- Completion extension to shell mapping
local COMPLETION_EXTENSIONS = {
  ['.bash'] = 'bash',
  ['.zsh'] = 'zsh',
  ['.fish'] = 'fish',
  ['.ps1'] = 'powershell',
}

-- Default options
local default_opts = {
  use = {},
  link = {
    bin = true,
    man = true,
    completions = true,
    lib = false,
    include = false,
  },
  shell_integration = true,
}

---@type syslua.environment.packages.Options
M.opts = default_opts

-- ============================================================================
-- Helper Functions
-- ============================================================================

--- Get package name from BuildRef
---@param pkg BuildRef
---@return string
local function get_pkg_name(pkg)
  if pkg.id then
    return pkg.id
  end
  -- Fallback: extract from hash
  return 'pkg-' .. (pkg.hash or 'unknown'):sub(1, 8)
end

--- Format a priority level for error messages
---@param p number
---@return string
local function priority_name(p)
  if p == prio.PRIORITIES.FORCE then
    return 'force'
  elseif p == prio.PRIORITIES.BEFORE then
    return 'before'
  elseif p == prio.PRIORITIES.PLAIN then
    return 'plain'
  elseif p == prio.PRIORITIES.DEFAULT then
    return 'default'
  elseif p == prio.PRIORITIES.AFTER then
    return 'after'
  else
    return 'custom'
  end
end

--- Raise a collision error with helpful resolution options
---@param binary_name string
---@param entry1 syslua.environment.packages.BinaryEntry
---@param entry2 syslua.environment.packages.BinaryEntry
local function raise_collision_error(binary_name, entry1, entry2)
  local pname = priority_name(entry1.priority)
  local msg = string.format(
    [[
Priority conflict in '%s'

  Conflicting packages at same priority level (%s: %d):

  Package: %s
    Binary: %s
    Source: %s

  Package: %s
    Binary: %s
    Source: %s

  Resolution options:
  1. Use prio.before(pkg) to make one package win
  2. Use prio.after(pkg) to make one package lose
  3. Remove one of the conflicting packages

  Example:
    use = {
      prio.before(pkgs.cli.%s),  -- wins for '%s'
      pkgs.cli.%s,
    }
]],
    binary_name,
    pname,
    entry1.priority,
    entry1.pkg_name,
    entry1.name,
    entry1.source,
    entry2.pkg_name,
    entry2.name,
    entry2.source,
    entry1.pkg_name:gsub('^__syslua_', ''):gsub('[^%w]', '_'),
    binary_name,
    entry2.pkg_name:gsub('^__syslua_', ''):gsub('[^%w]', '_')
  )
  error(msg, 0)
end

--- Detect shell from completion filename
---@param filename string
---@return string|nil shell type or nil if unknown
local function detect_completion_shell(filename)
  -- Check extension first
  for ext, shell in pairs(COMPLETION_EXTENSIONS) do
    if filename:sub(-#ext) == ext then
      return shell
    end
  end
  -- Zsh convention: files starting with underscore
  if filename:sub(1, 1) == '_' and not filename:match('%.') then
    return 'zsh'
  end
  return nil
end

--- Detect man section from filename
---@param filename string
---@return string|nil section number
local function detect_man_section(filename)
  -- Match patterns like "rg.1", "config.5", etc.
  local section = filename:match('%.(%d+)$')
  return section
end

--- Escape a string for sed regex patterns
---@param str string
---@return string
local function escape_sed_pattern(str)
  return str:gsub('([#/\\%.%[%]%*%^%$])', '\\%1')
end

--- Get shell config paths
---@return table<string, string>
local function get_shell_configs()
  local home = lib.get_home()

  if sys.is_elevated then
    local bash_global = sys.os == 'darwin' and '/etc/profile' or '/etc/profile.d/syslua-packages.sh'
    return {
      zsh = '/etc/zshenv',
      bash = bash_global,
      fish = '/etc/fish/conf.d/syslua-packages.fish',
      powershell = 'C:\\Program Files\\PowerShell\\7\\profile.ps1',
    }
  else
    return {
      zsh = home .. '/.zshenv',
      bash = home .. '/.bashrc',
      fish = home .. '/.config/fish/config.fish',
      powershell = home .. '/Documents/PowerShell/profile.ps1',
    }
  end
end

--- Get completion target paths for each shell
---@return table<string, string>
local function get_completion_paths()
  local home = lib.get_home()

  if sys.is_elevated then
    return {
      bash = sys.os == 'linux' and '/usr/share/bash-completion/completions/'
        or '/usr/local/share/bash-completion/completions/',
      zsh = '/usr/local/share/zsh/site-functions/',
      fish = sys.os == 'linux' and '/usr/share/fish/vendor_completions.d/'
        or '/usr/local/share/fish/vendor_completions.d/',
      powershell = 'C:\\Program Files\\PowerShell\\7\\Modules\\SysluaCompletions\\',
    }
  else
    return {
      bash = home .. '/.local/share/bash-completion/completions/',
      zsh = home .. '/.zsh/completions/',
      fish = home .. '/.config/fish/completions/',
      powershell = home .. '/Documents/PowerShell/Modules/SysluaCompletions/',
    }
  end
end

-- ============================================================================
-- Package Resolution
-- ============================================================================

--- Resolve packages from the use list, extracting priorities
---@param use_list (BuildRef|syslua.priority.PriorityValue<BuildRef>)[]
---@return syslua.environment.packages.ResolvedPackage[]
local function resolve_packages(use_list)
  local resolved = {}

  for _, item in ipairs(use_list) do
    local pkg = prio.unwrap(item)
    local priority = prio.get_priority(item)
    local name = get_pkg_name(pkg)

    table.insert(resolved, {
      pkg = pkg,
      priority = priority,
      name = name,
    })
  end

  return resolved
end

--- Collect all binaries from resolved packages and handle collisions
---@param packages syslua.environment.packages.ResolvedPackage[]
---@return syslua.environment.packages.BinaryEntry[]
local function collect_binaries(packages)
  -- Map: binary_name -> list of entries
  ---@type table<string, syslua.environment.packages.BinaryEntry[]>
  local collision_map = {}

  for _, resolved in ipairs(packages) do
    local pkg = resolved.pkg
    local bin_output = pkg.outputs and pkg.outputs.bin

    if bin_output then
      -- For now, treat bin output as a single binary file
      -- The filename is the basename of the path
      local bin_name = bin_output:match('([^/\\]+)$') or bin_output

      -- Remove .exe extension for collision detection on Windows
      local collision_key = bin_name:gsub('%.exe$', '')

      local entry = {
        name = bin_name,
        source = bin_output,
        pkg_name = resolved.name,
        priority = resolved.priority,
      }

      if not collision_map[collision_key] then
        collision_map[collision_key] = {}
      end
      table.insert(collision_map[collision_key], entry)
    end
  end

  -- Resolve collisions
  local result = {}
  for collision_key, entries in pairs(collision_map) do
    if #entries == 1 then
      -- No collision
      table.insert(result, entries[1])
    else
      -- Sort by priority (lower wins)
      table.sort(entries, function(a, b)
        return a.priority < b.priority
      end)

      local winner = entries[1]
      -- Check for same-priority conflicts
      for i = 2, #entries do
        if entries[i].priority == winner.priority then
          raise_collision_error(collision_key, winner, entries[i])
        end
      end
      table.insert(result, winner)
    end
  end

  return result
end

--- Collect all man pages from resolved packages
---@param packages syslua.environment.packages.ResolvedPackage[]
---@return syslua.environment.packages.ManEntry[]
local function collect_man_pages(packages)
  local result = {}

  for _, resolved in ipairs(packages) do
    local pkg = resolved.pkg
    local man_output = pkg.outputs and pkg.outputs.man

    if man_output then
      local man_name = man_output:match('([^/\\]+)$') or man_output
      local section = detect_man_section(man_name)

      if section then
        table.insert(result, {
          name = man_name,
          section = section,
          source = man_output,
          pkg_name = resolved.name,
        })
      end
    end
  end

  return result
end

--- Collect all completions from resolved packages
---@param packages syslua.environment.packages.ResolvedPackage[]
---@param shell_filter? string[] Only include these shells (nil = all)
---@return syslua.environment.packages.CompletionEntry[]
local function collect_completions(packages, shell_filter)
  local result = {}
  local filter_set = nil

  if shell_filter then
    filter_set = {}
    for _, shell in ipairs(shell_filter) do
      filter_set[shell] = true
    end
  end

  for _, resolved in ipairs(packages) do
    local pkg = resolved.pkg
    local completions_output = pkg.outputs and pkg.outputs.completions

    if completions_output then
      -- For now, treat as a single file and detect shell
      local comp_name = completions_output:match('([^/\\]+)$') or completions_output
      local shell = detect_completion_shell(comp_name)

      if shell and (not filter_set or filter_set[shell]) then
        table.insert(result, {
          name = comp_name,
          shell = shell,
          source = completions_output,
          pkg_name = resolved.name,
        })
      end
    end
  end

  return result
end

---@param packages syslua.environment.packages.ResolvedPackage[]
---@return syslua.environment.packages.LibEntry[]
local function collect_libs(packages)
  local result = {}

  for _, resolved in ipairs(packages) do
    local pkg = resolved.pkg
    local lib_output = pkg.outputs and pkg.outputs.lib

    if lib_output then
      table.insert(result, {
        source = lib_output,
        pkg_name = resolved.name,
      })
    end
  end

  return result
end

---@param packages syslua.environment.packages.ResolvedPackage[]
---@return syslua.environment.packages.IncludeEntry[]
local function collect_includes(packages)
  local result = {}

  for _, resolved in ipairs(packages) do
    local pkg = resolved.pkg
    local include_output = pkg.outputs and pkg.outputs.include

    if include_output then
      table.insert(result, {
        source = include_output,
        pkg_name = resolved.name,
      })
    end
  end

  return result
end

-- ============================================================================
-- Build and Bind Steps
-- ============================================================================

---@param binaries syslua.environment.packages.BinaryEntry[]
---@param man_pages syslua.environment.packages.ManEntry[]
---@param completions syslua.environment.packages.CompletionEntry[]
---@param libs syslua.environment.packages.LibEntry[]
---@param includes syslua.environment.packages.IncludeEntry[]
---@param link_opts syslua.environment.packages.LinkOptions
---@return table BuildRef
local function create_env_build(binaries, man_pages, completions, libs, includes, link_opts)
  return sys.build({
    id = '__syslua_env_packages',
    replace = true,
    inputs = {
      binaries = binaries,
      man_pages = man_pages,
      completions = completions,
      libs = libs,
      includes = includes,
      link_opts = link_opts,
      os = sys.os,
    },
    create = function(inputs, ctx)
      -- Create directory structure
      if inputs.os == 'windows' then
        ctx:exec({
          bin = 'cmd.exe',
          args = { '/c', string.format('mkdir "%s\\bin" 2>nul & mkdir "%s\\share\\man" 2>nul', ctx.out, ctx.out) },
        })
      else
        ctx:exec({
          bin = '/bin/sh',
          args = { '-c', string.format('mkdir -p "%s/bin" "%s/share/man"', ctx.out, ctx.out) },
        })
      end

      if inputs.link_opts.bin then
        for _, bin in ipairs(inputs.binaries) do
          if inputs.os == 'windows' then
            ctx:exec({
              bin = 'cmd.exe',
              args = {
                '/c',
                string.format(
                  [[
if exist "%s\*" (
  for %%%%f in ("%s\*") do mklink /H "%s\bin\%%%%~nxf" "%%%%f" 2>nul || copy "%%%%f" "%s\bin\%%%%~nxf"
) else (
  mklink /H "%s\bin\%s" "%s" 2>nul || copy "%s" "%s\bin\%s"
)
]],
                  bin.source,
                  bin.source,
                  ctx.out,
                  ctx.out,
                  ctx.out,
                  bin.name,
                  bin.source,
                  bin.source,
                  ctx.out,
                  bin.name
                ),
              },
            })
          else
            ctx:exec({
              bin = '/bin/sh',
              args = {
                '-c',
                string.format(
                  [[
if [ -d "%s" ]; then
  for f in "%s"/*; do
    [ -f "$f" ] && [ -x "$f" ] && ln -sf "$f" "%s/bin/$(basename "$f")"
  done
else
  ln -sf "%s" "%s/bin/%s"
fi
]],
                  bin.source,
                  bin.source,
                  ctx.out,
                  bin.source,
                  ctx.out,
                  bin.name
                ),
              },
            })
          end
        end
      end

      -- Create symlinks for man pages
      if inputs.link_opts.man then
        for _, man in ipairs(inputs.man_pages) do
          local man_section_dir = ctx.out .. '/share/man/man' .. man.section
          if inputs.os == 'windows' then
            -- Skip man pages on Windows
          else
            ctx:exec({
              bin = '/bin/sh',
              args = {
                '-c',
                string.format('mkdir -p "%s" && ln -sf "%s" "%s/%s"', man_section_dir, man.source, man_section_dir, man.name),
              },
            })
          end
        end
      end

      if inputs.link_opts.lib and #inputs.libs > 0 then
        if inputs.os ~= 'windows' then
          ctx:exec({
            bin = '/bin/sh',
            args = { '-c', string.format('mkdir -p "%s/lib"', ctx.out) },
          })
          for _, lib_entry in ipairs(inputs.libs) do
            ctx:exec({
              bin = '/bin/sh',
              args = {
                '-c',
                string.format(
                  'if [ -d "%s" ]; then for f in "%s"/*; do ln -sf "$f" "%s/lib/"; done; else ln -sf "%s" "%s/lib/"; fi',
                  lib_entry.source,
                  lib_entry.source,
                  ctx.out,
                  lib_entry.source,
                  ctx.out
                ),
              },
            })
          end
        end
      end

      if inputs.link_opts.include and #inputs.includes > 0 then
        if inputs.os ~= 'windows' then
          ctx:exec({
            bin = '/bin/sh',
            args = { '-c', string.format('mkdir -p "%s/include"', ctx.out) },
          })
          for _, inc_entry in ipairs(inputs.includes) do
            ctx:exec({
              bin = '/bin/sh',
              args = {
                '-c',
                string.format(
                  'if [ -d "%s" ]; then for f in "%s"/*; do ln -sf "$f" "%s/include/"; done; else ln -sf "%s" "%s/include/"; fi',
                  inc_entry.source,
                  inc_entry.source,
                  ctx.out,
                  inc_entry.source,
                  ctx.out
                ),
              },
            })
          end
        end
      end

      return {
        bin = ctx.out .. '/bin',
        man = ctx.out .. '/share/man',
        lib = ctx.out .. '/lib',
        include = ctx.out .. '/include',
        out = ctx.out,
      }
    end,
  })
end

---@param env_build table BuildRef from create_env_build
---@param env_build table BuildRef from create_env_build
local function create_env_bind(env_build)
  local home = lib.get_home()
  local syslua_dir = home .. '/.syslua'

  sys.bind({
    id = '__syslua_env_packages_link',
    replace = true,
    inputs = {
      env_build = env_build,
      syslua_dir = syslua_dir,
      os = sys.os,
    },
    create = function(inputs, ctx)
      local bin_target = inputs.syslua_dir .. '/bin'
      local man_target = inputs.syslua_dir .. '/share/man'
      local lib_target = inputs.syslua_dir .. '/lib'
      local include_target = inputs.syslua_dir .. '/include'

      if inputs.os == 'windows' then
        ctx:exec({
          bin = 'cmd.exe',
          args = {
            '/c',
            string.format(
              [[
if exist "%s" rmdir "%s" 2>nul
if exist "%s" rmdir /s /q "%s"
mkdir "%s" 2>nul
mklink /J "%s" "%s"
]],
              bin_target,
              bin_target,
              bin_target,
              bin_target,
              inputs.syslua_dir,
              bin_target,
              inputs.env_build.outputs.bin
            ),
          },
        })
      else
        ctx:exec({
          bin = '/bin/sh',
          args = {
            '-c',
            string.format(
              [[
mkdir -p "%s" "%s/share"
ln -s "%s" "%s.tmp.$$" && mv -f "%s.tmp.$$" "%s"
ln -s "%s" "%s.tmp.$$" && mv -f "%s.tmp.$$" "%s"
[ -d "%s" ] && { ln -s "%s" "%s.tmp.$$" && mv -f "%s.tmp.$$" "%s"; } || true
[ -d "%s" ] && { ln -s "%s" "%s.tmp.$$" && mv -f "%s.tmp.$$" "%s"; } || true
]],
              inputs.syslua_dir,
              inputs.syslua_dir,
              inputs.env_build.outputs.bin,
              bin_target,
              bin_target,
              bin_target,
              inputs.env_build.outputs.man,
              man_target,
              man_target,
              man_target,
              inputs.env_build.outputs.lib,
              inputs.env_build.outputs.lib,
              lib_target,
              lib_target,
              lib_target,
              inputs.env_build.outputs.include,
              inputs.env_build.outputs.include,
              include_target,
              include_target,
              include_target
            ),
          },
        })
      end

      return {
        bin_link = bin_target,
        man_link = man_target,
        lib_link = lib_target,
        include_link = include_target,
      }
    end,
    destroy = function(outputs, ctx)
      if sys.os == 'windows' then
        ctx:exec({
          bin = 'cmd.exe',
          args = { '/c', string.format('rmdir "%s" 2>nul & rmdir "%s" 2>nul', outputs.bin_link, outputs.man_link) },
        })
      else
        ctx:exec({
          bin = '/bin/sh',
          args = { '-c', string.format('rm -f "%s" "%s" "%s" "%s"', outputs.bin_link, outputs.man_link, outputs.lib_link, outputs.include_link) },
        })
      end
    end,
  })
end

--- Create shell integration binds (add ~/.syslua/bin to PATH)
---@param enabled boolean
local function create_shell_integration_binds(enabled)
  if not enabled then
    -- Print manual instructions
    print([[
Shell integration disabled. Add to your shell config:

  Bash/Zsh: export PATH="$HOME/.syslua/bin:$PATH"
  Fish:     fish_add_path ~/.syslua/bin
  PowerShell: $env:PATH = "$HOME/.syslua/bin;$env:PATH"
]])
    return
  end

  local shell_configs = get_shell_configs()
  local home = lib.get_home()
  local bin_path = home .. '/.syslua/bin'

  if sys.os == 'windows' then
    -- PowerShell integration
    sys.bind({
      id = '__syslua_env_packages_shell_ps1',
      replace = true,
      inputs = {
        config_path = shell_configs.powershell,
        bin_path = bin_path,
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
  Add-Content -Path $configPath -Value "`n%s`n`$env:PATH = `"%s;`$env:PATH`"`n%s"
}
]],
              inputs.config_path,
              inputs.begin_marker,
              inputs.begin_marker,
              inputs.bin_path,
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
              escape_sed_pattern(BEGIN_MARKER),
              escape_sed_pattern(END_MARKER)
            ),
          },
        })
      end,
    })
  else
    -- POSIX shells (bash, zsh)
    for _, shell in ipairs({ 'zsh', 'bash' }) do
      local config_path = shell_configs[shell]
      if config_path then
        sys.bind({
          id = '__syslua_env_packages_shell_' .. shell,
          replace = true,
          inputs = {
            config_path = config_path,
            bin_path = bin_path,
            shell = shell,
            begin_marker = BEGIN_MARKER,
            end_marker = END_MARKER,
          },
          create = function(inputs, ctx)
            local export_line = string.format('export PATH="%s:$PATH"', inputs.bin_path)

            ctx:exec({
              bin = '/bin/sh',
              args = {
                '-c',
                string.format(
                  [[
config_path="%s"
config_dir=$(dirname "$config_path")
mkdir -p "$config_dir"
touch "$config_path"
if ! grep -qF "%s" "$config_path" 2>/dev/null; then
  printf '\n%s\n%s\n%s\n' >> "$config_path"
fi
]],
                  inputs.config_path,
                  inputs.begin_marker,
                  inputs.begin_marker,
                  export_line,
                  inputs.end_marker
                ),
              },
            })
            return { config = inputs.config_path }
          end,
          destroy = function(outputs, ctx)
            ctx:exec({
              bin = '/bin/sh',
              args = {
                '-c',
                string.format(
                  [[
config_path="%s"
if [ -f "$config_path" ]; then
  sed '/%s/,/%s/d' "$config_path" > "$config_path.tmp" && mv "$config_path.tmp" "$config_path"
fi
]],
                  outputs.config,
                  escape_sed_pattern(BEGIN_MARKER),
                  escape_sed_pattern(END_MARKER)
                ),
              },
            })
          end,
        })
      end
    end

    -- Fish shell
    if shell_configs.fish then
      sys.bind({
        id = '__syslua_env_packages_shell_fish',
        replace = true,
        inputs = {
          config_path = shell_configs.fish,
          bin_path = bin_path,
          begin_marker = BEGIN_MARKER,
          end_marker = END_MARKER,
        },
        create = function(inputs, ctx)
          local fish_line = string.format('fish_add_path %s', inputs.bin_path)

          ctx:exec({
            bin = '/bin/sh',
            args = {
              '-c',
              string.format(
                [[
config_path="%s"
config_dir=$(dirname "$config_path")
mkdir -p "$config_dir"
touch "$config_path"
if ! grep -qF "%s" "$config_path" 2>/dev/null; then
  printf '\n%s\n%s\n%s\n' >> "$config_path"
fi
]],
                inputs.config_path,
                inputs.begin_marker,
                inputs.begin_marker,
                fish_line,
                inputs.end_marker
              ),
            },
          })
          return { config = inputs.config_path }
        end,
        destroy = function(outputs, ctx)
          ctx:exec({
            bin = '/bin/sh',
            args = {
              '-c',
              string.format(
                [[
config_path="%s"
if [ -f "$config_path" ]; then
  sed '/%s/,/%s/d' "$config_path" > "$config_path.tmp" && mv "$config_path.tmp" "$config_path"
fi
]],
                outputs.config,
                escape_sed_pattern(BEGIN_MARKER),
                escape_sed_pattern(END_MARKER)
              ),
            },
          })
        end,
      })
    end
  end
end

--- Create completion binds
---@param completions syslua.environment.packages.CompletionEntry[]
---@param link_opts syslua.environment.packages.LinkOptions
local function create_completion_binds(completions, link_opts)
  if not link_opts.completions then
    return
  end

  local paths = get_completion_paths()

  for _, comp in ipairs(completions) do
    local target_dir = paths[comp.shell]
    if target_dir then
      local target_name = comp.name
          if comp.shell == 'zsh' and not target_name:match('^_') then
        target_name = '_' .. target_name:gsub('%.zsh$', '')
      elseif comp.shell == 'fish' and not target_name:match('%.fish$') then
        target_name = target_name .. '.fish'
      end

      sys.bind({
        id = '__syslua_env_packages_completion_' .. comp.pkg_name .. '_' .. comp.shell,
        replace = true,
        inputs = {
          source = comp.source,
          target = target_dir .. target_name,
          target_dir = target_dir,
          shell = comp.shell,
          os = sys.os,
        },
        create = function(inputs, ctx)
          if inputs.os == 'windows' then
            ctx:exec({
              bin = 'cmd.exe',
              args = {
                '/c',
                string.format('mkdir "%s" 2>nul & copy "%s" "%s"', inputs.target_dir, inputs.source, inputs.target),
              },
            })
          else
            ctx:exec({
              bin = '/bin/sh',
              args = {
                '-c',
                string.format(
                  [[
mkdir -p "%s"
if [ -d "%s" ]; then
  for f in "%s"/*; do
    [ -f "$f" ] || continue
    name=$(basename "$f")
    case "$name" in
      *.bash) [ "%s" = "bash" ] && ln -sf "$f" "%s/$name" ;;
      *.zsh|_*) [ "%s" = "zsh" ] && ln -sf "$f" "%s/$name" ;;
      *.fish) [ "%s" = "fish" ] && ln -sf "$f" "%s/$name" ;;
      *.ps1) [ "%s" = "powershell" ] && ln -sf "$f" "%s/$name" ;;
    esac
  done
else
  ln -sf "%s" "%s"
fi
]],
                  inputs.target_dir,
                  inputs.source,
                  inputs.source,
                  inputs.shell,
                  inputs.target_dir,
                  inputs.shell,
                  inputs.target_dir,
                  inputs.shell,
                  inputs.target_dir,
                  inputs.shell,
                  inputs.target_dir,
                  inputs.source,
                  inputs.target
                ),
              },
            })
          end
          return { link = inputs.target }
        end,
        destroy = function(outputs, ctx)
          if sys.os == 'windows' then
            ctx:exec({ bin = 'cmd.exe', args = { '/c', string.format('del "%s" 2>nul', outputs.link) } })
          else
            ctx:exec({ bin = '/bin/sh', args = { '-c', string.format('rm -f "%s"', outputs.link) } })
          end
        end,
      })
    end
  end
end

-- ============================================================================
-- Public API
-- ============================================================================

--- Set up environment packages according to the provided options
---@param provided_opts syslua.environment.packages.Options
function M.setup(provided_opts)
  provided_opts = provided_opts or {}

  if not provided_opts.use or #provided_opts.use == 0 then
    error('environment.packages.setup: "use" field is required and must contain at least one package', 2)
  end

  local new_opts = prio.merge(default_opts, provided_opts)
  if not new_opts then
    error('Failed to merge environment.packages options')
  end
  M.opts = new_opts

  local resolved_packages = resolve_packages(M.opts.use)
  local binaries = collect_binaries(resolved_packages)

  local man_pages = {}
  if M.opts.link.man then
    man_pages = collect_man_pages(resolved_packages)
  end

  local completions = {}
  if M.opts.link.completions then
    ---@type string[]|nil
    local shell_filter = nil
    if type(M.opts.link.completions) == 'table' then
      shell_filter = M.opts.link.completions --[[@as string[] ]]
    end
    completions = collect_completions(resolved_packages, shell_filter)
  end

  local libs = {}
  if M.opts.link.lib then
    libs = collect_libs(resolved_packages)
  end

  local includes = {}
  if M.opts.link.include then
    includes = collect_includes(resolved_packages)
  end

  local env_build = create_env_build(binaries, man_pages, completions, libs, includes, M.opts.link)
  create_env_bind(env_build)
  create_shell_integration_binds(M.opts.shell_integration)
  create_completion_binds(completions, M.opts.link)
end

return M
