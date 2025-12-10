-- Development environment configuration with sys.lua
-- This example shows environment variable management for development

local M = {}

-- No external inputs needed for this example
-- M.inputs can be omitted when you don't have external dependencies

function M.setup()
    -- Basic editor and pager settings
    env {
        EDITOR = "nvim",
        VISUAL = "nvim",
        PAGER = "less",
        LESS = "-R", -- Enable ANSI colors in less
    }

    -- Language-specific environment
    env {
        -- Rust
        CARGO_HOME = "~/.cargo",
        RUSTUP_HOME = "~/.rustup",

        -- Go
        GOPATH = "~/go",

        -- Node.js
        NODE_ENV = "development",
    }

    -- PATH additions (arrays prepend to existing PATH)
    env {
        PATH = {
            "~/.local/bin",     -- User binaries
            "~/.cargo/bin",     -- Rust binaries
            "~/go/bin",         -- Go binaries
            "~/.npm-global/bin", -- Global npm packages
        },
    }

    -- XDG Base Directory specification
    env {
        XDG_CONFIG_HOME = "~/.config",
        XDG_DATA_HOME = "~/.local/share",
        XDG_CACHE_HOME = "~/.cache",
        XDG_STATE_HOME = "~/.local/state",
    }

    -- Git settings via environment
    env {
        GIT_PAGER = "delta", -- Use delta for git diffs (if installed)
    }

    -- Colorful terminal output
    env {
        CLICOLOR = "1",
        COLORTERM = "truecolor",
    }

    -- Create a shell aliases file
    file {
        path = "~/.config/shell/aliases.sh",
        content = [[
# Shell aliases managed by sys.lua

# Git shortcuts
alias g='git'
alias gs='git status'
alias gd='git diff'
alias gc='git commit'
alias gp='git push'
alias gl='git pull'

# Development shortcuts
alias c='cargo'
alias cb='cargo build'
alias ct='cargo test'
alias cr='cargo run'

# Navigation
alias ..='cd ..'
alias ...='cd ../..'
alias ....='cd ../../..'

# Safety aliases
alias rm='rm -i'
alias mv='mv -i'
alias cp='cp -i'
]],
    }

    -- Create a useful dev tools check script
    file {
        path = "~/.local/bin/check-dev-tools",
        content = [[
#!/usr/bin/env bash
# Check for common development tools

echo "Checking development tools..."
echo

check_tool() {
    if command -v "$1" &> /dev/null; then
        echo "✓ $1: $(command -v $1)"
    else
        echo "✗ $1: not found"
    fi
}

check_tool git
check_tool nvim
check_tool cargo
check_tool rustc
check_tool go
check_tool node
check_tool npm
check_tool python3
check_tool docker

echo
echo "Done!"
]],
        mode = "0755", -- Make executable
    }
end

return M
