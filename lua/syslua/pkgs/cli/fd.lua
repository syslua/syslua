local prio = require('syslua.priority')
local lib = require('syslua.lib')

---@class syslua.pkgs.cli.fd
local M = {}

---@class FdRelease
---@field url string
---@field sha256 string
---@field format ArchiveFormat

---@type table<string, table<string, FdRelease>>
M.releases = {
  ['v10.2.0'] = {
    ['aarch64-darwin'] = {
      url = 'https://github.com/sharkdp/fd/releases/download/v10.2.0/fd-v10.2.0-aarch64-apple-darwin.tar.gz',
      sha256 = 'ae6327ba8c9a487cd63edd8bddd97da0207887a66d61e067dfe80c1430c5ae36',
      format = 'tar.gz',
    },
    ['x86_64-darwin'] = {
      url = 'https://github.com/sharkdp/fd/releases/download/v10.2.0/fd-v10.2.0-x86_64-apple-darwin.tar.gz',
      sha256 = '991a648a58870230af9547c1ae33e72cb5c5199a622fe5e540e162d6dba82d48',
      format = 'tar.gz',
    },
    ['x86_64-linux'] = {
      url = 'https://github.com/sharkdp/fd/releases/download/v10.2.0/fd-v10.2.0-x86_64-unknown-linux-musl.tar.gz',
      sha256 = 'd9bfa25ec28624545c222992e1b00673b7c9ca5eb15393c40369f10b28f9c932',
      format = 'tar.gz',
    },
    ['x86_64-windows'] = {
      url = 'https://github.com/sharkdp/fd/releases/download/v10.2.0/fd-v10.2.0-x86_64-pc-windows-msvc.zip',
      sha256 = '92ac9e6b0a0c6ecdab638ffe210dc786403fff4c66373604cf70df27be45e4fe',
      format = 'zip',
    },
  },
}

---@class FdMeta
M.meta = {
  name = 'fd',
  homepage = 'https://github.com/sharkdp/fd',
  description = 'A simple, fast and user-friendly alternative to find',
  license = 'MIT',
  versions = {
    stable = 'v10.2.0',
    latest = 'v10.2.0',
  },
}

---@class FdOptions
---@field version? string Version to install (default: stable)

local default_opts = {
  version = prio.default(M.meta.versions.stable),
}

---@type FdOptions
M.opts = default_opts

---Build fd package
---@param provided_opts? FdOptions
---@return BuildRef
function M.setup(provided_opts)
  local new_opts = prio.merge(M.opts, provided_opts or {})
  if not new_opts then
    error('Failed to merge fd options')
  end
  M.opts = new_opts

  local version = M.meta.versions[M.opts.version] or M.opts.version

  local release = M.releases[version]
  if not release then
    local available = {}
    for v in pairs(M.releases) do
      table.insert(available, v)
    end
    table.sort(available)
    error(string.format("fd version '%s' not found. Available: %s", version, table.concat(available, ', ')))
  end

  local platform_release = release[sys.platform]
  if not platform_release then
    local available = {}
    for p in pairs(release) do
      table.insert(available, p)
    end
    table.sort(available)
    error(
      string.format('fd %s not available for %s. Available: %s', version, sys.platform, table.concat(available, ', '))
    )
  end

  local extracted = lib.extract({
    url = platform_release.url,
    sha256 = platform_release.sha256,
    format = platform_release.format,
    strip_components = 1,
  })

  local bin_name = 'fd' .. (sys.os == 'windows' and '.exe' or '')
  return {
    outputs = {
      bin = extracted.outputs.out .. '/' .. bin_name,
      out = extracted.outputs.out,
    },
  }
end

return M
