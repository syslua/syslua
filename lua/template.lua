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
---@param inputs table<string, {path:string,rev:string}> Resolved inputs with path and rev fields
function M.setup(inputs)
  local syslua = require('syslua')
  local f = syslua.f -- string interpolation with {{}} delimiters

  -- Example: Install a CLI tool
  syslua.pkgs.cli.ripgrep.setup()

  -- Example: Link a dotfile using interpolation
  syslua.environment.files.setup({
    ['~/.gitconfig'] = {
      source = f('{{path}}/.gitconfig', { path = inputs.dotfiles.path }),
    },
  })

  -- Example: Set environment variables
  syslua.environment.variables.setup({
    EDITOR = 'nvim',
  })

  -- Example: String interpolation features (uses {{}} to avoid shell confusion)
  -- local name = "world"
  -- print(f("Hello {{name}}!"))              --> Hello world!
  -- print(f("{{1 + 2}}"))                    --> 3
  -- print(f("{{x=}}", {x = 42}))             --> x=42
  -- print(f("{{pi:%0.2f}}", {pi = 3.14159})) --> 3.14
  -- print(f("echo $HOME/{{name}}"))          --> echo $HOME/world (shell vars preserved)
end

return M
