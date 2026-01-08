local prio = require('syslua.priority')
local pkgs = require('syslua.pkgs')
local modules = require('syslua.modules')
local lib = require('syslua.lib')

---@class syslua.programs.ripgrep
---@field opts syslua.programs.ripgrep.Options
local M = {}

---@class syslua.programs.ripgrep.Options
---@field version? string | syslua.priority.PriorityValue<string>
---@field bash_integration? boolean | syslua.priority.PriorityValue<boolean>
---@field zsh_integration? boolean | syslua.priority.PriorityValue<boolean>
---@field fish_integration? boolean | syslua.priority.PriorityValue<boolean>
---@field powershell_integration? boolean | syslua.priority.PriorityValue<boolean>
---@field config? syslua.modules.file.Options

local default_opts = {
  version = prio.default('stable'),
  bash_integration = prio.default(false),
  zsh_integration = prio.default(false),
  fish_integration = prio.default(false),
  powershell_integration = prio.default(false),
}

---@type syslua.programs.ripgrep.Options
M.opts = default_opts

local COMPLETIONS = {
  bash = 'complete/rg.bash',
  zsh = 'complete/_rg',
  fish = 'complete/rg.fish',
  ps1 = 'complete/_rg.ps1',
}

local MAN_PAGE = 'doc/rg.1'

---@param provided_opts? syslua.programs.ripgrep.Options
M.setup = function(provided_opts)
  provided_opts = provided_opts or {}

  local new_opts, err = prio.merge(M.opts, provided_opts)
  if not new_opts then
    error(string.format('Failed to merge ripgrep options: %s', err or 'unknown error'))
  end
  M.opts = new_opts

  local pkg_build = pkgs.cli.ripgrep.setup({ version = M.opts.version })

  modules.env.setup({
    PATH = prio.before(pkg_build.outputs.out),
  })

  lib.programs.create_completion_binds(pkg_build, 'rg', COMPLETIONS, M.opts)

  lib.programs.create_man_bind(pkg_build, MAN_PAGE, 'rg.1')

  if M.opts.config then
    modules.file.setup(M.opts.config)
  end
end

return M
