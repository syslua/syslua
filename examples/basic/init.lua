--- Basic syslua configuration example
--- Entry point must return a table with `inputs` and `setup` fields
return {
  inputs = {
    syslua = 'github:spirit-led-software/syslua/master', -- includes pkgs, lib, modules
    dotfiles = 'github:ianpascoe/dotfiles/master', -- includes dotfiles, not a lua module
  },
  setup = function(inputs)
    local syslua = require('syslua')
    local file = syslua.modules.file
    local path = sys.path

    file.setup({
      target = path.resolve(path.join(os.getenv('HOME'), '.config', 'starship.toml')),
      source = path.resolve(path.join(inputs.dotfiles.path, 'config', 'starship.toml')), -- config from an input
    })
    file.setup({
      target = path.resolve(path.join(os.getenv('HOME'), '.ssh')),
      source = path.resolve(path.join(__dir, '..', 'dotfiles', '.ssh')), -- config living alongside this init.lua
    })
  end,
}
