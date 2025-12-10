-- Basic dotfiles management with sys.lua
-- This example shows how to manage your shell and editor configurations

local M = {}

-- No external inputs needed for this simple example
-- M.inputs can be omitted when you don't have external dependencies

function M.setup()
    -- Manage .gitconfig as a store-backed file (content goes to store, symlink at target)
    file {
        path = "~/.gitconfig",
        source = "./dotfiles/gitconfig",
    }

    -- Manage shell configuration
    file {
        path = "~/.bashrc",
        source = "./dotfiles/bashrc",
    }

    file {
        path = "~/.zshrc",
        source = "./dotfiles/zshrc",
    }

    -- Manage vim/neovim config with inline content
    file {
        path = "~/.vimrc",
        content = [[
" Basic vim configuration managed by sys.lua
set nocompatible
set number
set relativenumber
set expandtab
set tabstop=4
set shiftwidth=4
set autoindent
set smartindent
set hlsearch
set incsearch
set ignorecase
set smartcase
syntax on
filetype plugin indent on
]],
    }

    -- Use mutable = true for files you want to edit directly
    -- (creates a direct symlink instead of store-backed)
    file {
        path = "~/.config/git/ignore",
        source = "./dotfiles/gitignore_global",
        mutable = true,
    }

    -- Nested directory example - parent dirs are created automatically
    file {
        path = "~/.config/nvim/init.lua",
        content = [[
-- Neovim configuration managed by sys.lua
vim.opt.number = true
vim.opt.relativenumber = true
vim.opt.expandtab = true
vim.opt.tabstop = 4
vim.opt.shiftwidth = 4
]],
    }
end

return M
