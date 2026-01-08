--- syslua configuration
--- See https://syslua.dev/docs for documentation
local M = {}

--- External inputs
--- Inputs are resolved before M.setup() runs
--- Examples:
---   syslua = "git:https://github.com/syslua/syslua.git"
---   dotfiles = "git:git@github.com:myuser/dotfiles.git"
---   local_config = "path:~/code/my-config"
M.inputs = {
  syslua = 'git:https://github.com/syslua/syslua.git',
}

--- Configuration setup
---@param inputs table<string, {path:string}> Resolved inputs with path and rev fields
function M.setup(inputs)
  local syslua = require('syslua')

  -- Example: Install a CLI tool
  syslua.pkgs.cli.ripgrep.setup()

  -- Example: Link a dotfile
  syslua.modules.file.setup({
    target = '~/.gitconfig',
    source = sys.path.join(inputs.dotfiles.path, '.gitconfig'),
  })

  -- Example: Set environment variables
  syslua.modules.setup({
    EDITOR = 'nvim',
  })
end

return M
