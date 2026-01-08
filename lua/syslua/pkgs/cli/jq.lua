local prio = require('syslua.priority')

---@class syslua.pkgs.cli.jq
local M = {}

---@class JqRelease
---@field url string
---@field sha256 string

---@type table<string, table<string, JqRelease>>
M.releases = {
  ['1.7.1'] = {
    ['aarch64-darwin'] = {
      url = 'https://github.com/jqlang/jq/releases/download/jq-1.7.1/jq-macos-arm64',
      sha256 = '0bbe619e663e0de2c550be2fe0d240d076799d6f8a652b70fa04aea8a8362e8a',
    },
    ['x86_64-darwin'] = {
      url = 'https://github.com/jqlang/jq/releases/download/jq-1.7.1/jq-macos-amd64',
      sha256 = '4155822bbf5ea90f5c79cf254665975eb4274d426d0709770c21774de5407443',
    },
    ['x86_64-linux'] = {
      url = 'https://github.com/jqlang/jq/releases/download/jq-1.7.1/jq-linux-amd64',
      sha256 = '5942c9b0934e510ee61eb3e30273f1b3fe2590df93933a93d7c58b81d19c8ff5',
    },
    ['x86_64-windows'] = {
      url = 'https://github.com/jqlang/jq/releases/download/jq-1.7.1/jq-windows-amd64.exe',
      sha256 = '7451fbbf37feffb9bf262bd97c54f0da558c63f0748e64152dd87b0a07b6d6ab',
    },
  },
}

---@class JqMeta
M.meta = {
  name = 'jq',
  homepage = 'https://github.com/jqlang/jq',
  description = 'Command-line JSON processor',
  license = 'MIT',
  versions = {
    stable = '1.7.1',
    latest = '1.7.1',
  },
}

---@class JqOptions
---@field version? string Version to install (default: stable)

local default_opts = {
  version = prio.default(M.meta.versions.stable),
}

---@type JqOptions
M.opts = default_opts

---Build jq package (standalone binary, no extraction needed)
---@param provided_opts? JqOptions
---@return BuildRef
function M.setup(provided_opts)
  local new_opts = prio.merge(M.opts, provided_opts or {})
  if not new_opts then
    error('Failed to merge jq options')
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
    error(string.format("jq version '%s' not found. Available: %s", version, table.concat(available, ', ')))
  end

  local platform_release = release[sys.platform]
  if not platform_release then
    local available = {}
    for p in pairs(release) do
      table.insert(available, p)
    end
    table.sort(available)
    error(
      string.format('jq %s not available for %s. Available: %s', version, sys.platform, table.concat(available, ', '))
    )
  end

  return sys.build({
    id = 'jq-' .. version,
    inputs = {
      url = platform_release.url,
      sha256 = platform_release.sha256,
      version = version,
    },
    create = function(inputs, ctx)
      local downloaded = ctx:fetch_url(inputs.url, inputs.sha256)
      local bin_name = 'jq' .. (sys.os == 'windows' and '.exe' or '')
      local bin_path = ctx.out .. '/' .. bin_name

      if sys.os == 'windows' then
        ctx:exec({ bin = 'cmd.exe', args = { '/c', 'copy', downloaded, bin_path } })
      else
        ctx:exec({ bin = 'cp', args = { downloaded, bin_path } })
        ctx:exec({ bin = 'chmod', args = { '+x', bin_path } })
      end

      return {
        bin = bin_path,
        out = ctx.out,
      }
    end,
  })
end

return M
