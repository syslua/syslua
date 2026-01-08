local prio = require('syslua.priority')
local pkgs = require('syslua.pkgs')
local modules = require('syslua.modules')

---@class syslua.programs.jq
---@field opts syslua.programs.jq.Options
local M = {}

---@class syslua.programs.jq.Options
---@field version? string | syslua.priority.PriorityValue<string>
---@field config? syslua.environment.files.Options

local default_opts = {
  version = prio.default('stable'),
}

---@type syslua.programs.jq.Options
M.opts = default_opts

---@param provided_opts? syslua.programs.jq.Options
M.setup = function(provided_opts)
  provided_opts = provided_opts or {}

  local new_opts, err = prio.merge(M.opts, provided_opts)
  if not new_opts then
    error(string.format('Failed to merge jq options: %s', err or 'unknown error'))
  end
  M.opts = new_opts

  local pkg_build = pkgs.cli.jq.setup({ version = M.opts.version })

  modules.env.setup({
    PATH = prio.before(pkg_build.outputs.out),
  })

  if M.opts.config then
    modules.file.setup(M.opts.config)
  end
end

return M
