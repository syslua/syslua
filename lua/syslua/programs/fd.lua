local prio = require('syslua.priority')
local pkgs = require('syslua.pkgs')
local modules = require('syslua.modules')
local lib = require('syslua.lib')

---@class syslua.programs.fd
---@field opts syslua.programs.fd.Options
local M = {}

---@class syslua.programs.fd.Options
---@field version? string | syslua.priority.PriorityValue<string>
---@field bash_integration? boolean | syslua.priority.PriorityValue<boolean>
---@field zsh_integration? boolean | syslua.priority.PriorityValue<boolean>
---@field fish_integration? boolean | syslua.priority.PriorityValue<boolean>
---@field powershell_integration? boolean | syslua.priority.PriorityValue<boolean>
---@field config? syslua.environment.files.Options

local default_opts = {
  version = prio.default('stable'),
  bash_integration = prio.default(false),
  zsh_integration = prio.default(false),
  fish_integration = prio.default(false),
  powershell_integration = prio.default(false),
}

---@type syslua.programs.fd.Options
M.opts = default_opts

local COMPLETIONS = {
  bash = 'autocomplete/fd.bash',
  zsh = 'autocomplete/_fd',
  fish = 'autocomplete/fd.fish',
  ps1 = 'autocomplete/fd.ps1',
}

local MAN_PAGE = 'fd.1'

---@param provided_opts? syslua.programs.fd.Options
M.setup = function(provided_opts)
  provided_opts = provided_opts or {}

  local new_opts, err = prio.merge(M.opts, provided_opts)
  if not new_opts then
    error(string.format('Failed to merge fd options: %s', err or 'unknown error'))
  end
  M.opts = new_opts

  local pkg_build = pkgs.cli.fd.setup({ version = M.opts.version })

  modules.env.setup({
    PATH = prio.before(pkg_build.outputs.out),
  })

  lib.programs.create_completion_binds(pkg_build, 'fd', COMPLETIONS, M.opts)

  lib.programs.create_man_bind(pkg_build, MAN_PAGE, 'fd.1')

  if M.opts.config then
    modules.file.setup(M.opts.config)
  end
end

return M
