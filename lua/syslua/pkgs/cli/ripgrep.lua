local prio = require('syslua.priority')
local lib = require('syslua.lib')

---@class syslua.pkgs.cli.ripgrep
local M = {}

-- ============================================================================
-- Metadata (exported for tooling/automation)
-- ============================================================================

---@class RipgrepRelease
---@field url string
---@field sha256 string
---@field format ArchiveFormat

---@type table<string, table<string, RipgrepRelease>>
M.releases = {
  ['15.1.0'] = {
    ['aarch64-darwin'] = {
      url = 'https://github.com/BurntSushi/ripgrep/releases/download/15.1.0/ripgrep-15.1.0-aarch64-apple-darwin.tar.gz',
      sha256 = '378e973289176ca0c6054054ee7f631a065874a352bf43f0fa60ef079b6ba715',
      format = 'tar.gz',
    },
    ['x86_64-darwin'] = {
      url = 'https://github.com/BurntSushi/ripgrep/releases/download/15.1.0/ripgrep-15.1.0-x86_64-apple-darwin.tar.gz',
      sha256 = '64811cb24e77cac3057d6c40b63ac9becf9082eedd54ca411b475b755d334882',
      format = 'tar.gz',
    },
    ['x86_64-linux'] = {
      url = 'https://github.com/BurntSushi/ripgrep/releases/download/15.1.0/ripgrep-15.1.0-x86_64-unknown-linux-musl.tar.gz',
      sha256 = '1c9297be4a084eea7ecaedf93eb03d058d6faae29bbc57ecdaf5063921491599',
      format = 'tar.gz',
    },
    ['x86_64-windows'] = {
      url = 'https://github.com/BurntSushi/ripgrep/releases/download/15.1.0/ripgrep-15.1.0-x86_64-pc-windows-msvc.zip',
      sha256 = '124510b94b6baa3380d051fdf4650eaa80a302c876d611e9dba0b2e18d87493a',
      format = 'zip',
    },
  },
}

---@class RipgrepMeta
M.meta = {
  name = 'ripgrep',
  homepage = 'https://github.com/BurntSushi/ripgrep',
  description = 'ripgrep recursively searches directories for a regex pattern',
  license = 'MIT',
  versions = {
    stable = '15.1.0',
    latest = '15.1.0',
  },
}

-- ============================================================================
-- Options
-- ============================================================================

---@class RipgrepOptions
---@field version? string Version to install (default: stable)

local default_opts = {
  version = prio.default(M.meta.versions.stable),
}

---@type RipgrepOptions
M.opts = default_opts

-- ============================================================================
-- Setup
-- ============================================================================

---Build ripgrep package
---@param provided_opts? RipgrepOptions
---@return BuildRef
function M.setup(provided_opts)
  local new_opts = prio.merge(M.opts, provided_opts or {})
  if not new_opts then
    error('Failed to merge ripgrep options')
  end
  M.opts = new_opts

  -- Resolve version alias
  local version = M.meta.versions[M.opts.version] or M.opts.version

  local release = M.releases[version]
  if not release then
    local available = {}
    for v in pairs(M.releases) do
      table.insert(available, v)
    end
    table.sort(available)
    error(string.format("ripgrep version '%s' not found. Available: %s", version, table.concat(available, ', ')))
  end

  local platform_release = release[sys.platform]
  if not platform_release then
    local available = {}
    for p in pairs(release) do
      table.insert(available, p)
    end
    table.sort(available)
    error(
      string.format(
        'ripgrep %s not available for %s. Available: %s',
        version,
        sys.platform,
        table.concat(available, ', ')
      )
    )
  end

  local extracted = lib.extract({
    url = platform_release.url,
    sha256 = platform_release.sha256,
    format = platform_release.format,
    strip_components = 1,
  })

  local bin_name = 'rg' .. (sys.os == 'windows' and '.exe' or '')
  return {
    outputs = {
      bin = extracted.outputs.out .. '/' .. bin_name,
      out = extracted.outputs.out,
    },
  }
end

return M
