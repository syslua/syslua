//! Shell detection and environment script generation

use std::env;
use std::path::PathBuf;

/// Supported shell types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
    PowerShell,
    Sh,
}

impl Shell {
    /// Detect the current shell from environment
    ///
    /// Checks `$SHELL` on Unix, falls back to reasonable defaults.
    pub fn detect() -> Self {
        // First check $SHELL environment variable
        if let Ok(shell) = env::var("SHELL") {
            let shell_name = PathBuf::from(&shell)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("")
                .to_lowercase();

            return match shell_name.as_str() {
                "zsh" => Shell::Zsh,
                "bash" => Shell::Bash,
                "fish" => Shell::Fish,
                "sh" => Shell::Sh,
                "pwsh" | "powershell" => Shell::PowerShell,
                _ => {
                    // Check if it contains the shell name
                    if shell_name.contains("zsh") {
                        Shell::Zsh
                    } else if shell_name.contains("bash") {
                        Shell::Bash
                    } else if shell_name.contains("fish") {
                        Shell::Fish
                    } else {
                        Shell::Sh // Safe fallback for POSIX
                    }
                }
            };
        }

        // Platform-specific defaults
        #[cfg(target_os = "windows")]
        return Shell::PowerShell;

        #[cfg(not(target_os = "windows"))]
        Shell::Sh
    }

    /// Get the shell name as a string
    pub fn as_str(&self) -> &'static str {
        match self {
            Shell::Bash => "bash",
            Shell::Zsh => "zsh",
            Shell::Fish => "fish",
            Shell::PowerShell => "powershell",
            Shell::Sh => "sh",
        }
    }

    /// Get the file extension for this shell's scripts
    pub fn script_extension(&self) -> &'static str {
        match self {
            Shell::Bash => "sh",
            Shell::Zsh => "zsh",
            Shell::Fish => "fish",
            Shell::PowerShell => "ps1",
            Shell::Sh => "sh",
        }
    }

    /// Generate an export statement for setting an environment variable
    pub fn export_var(&self, name: &str, value: &str) -> String {
        match self {
            Shell::Fish => format!("set -gx {} {:?}", name, value),
            Shell::PowerShell => format!("$env:{} = {:?}", name, value),
            Shell::Bash | Shell::Zsh | Shell::Sh => format!("export {}={:?}", name, value),
        }
    }

    /// Generate a prepend statement for a PATH-like variable
    pub fn prepend_path(&self, name: &str, value: &str) -> String {
        match self {
            Shell::Fish => format!("set -gx {} {:?} ${}", name, value, name),
            Shell::PowerShell => format!(
                "$env:{} = {:?} + [IO.Path]::PathSeparator + $env:{}",
                name, value, name
            ),
            Shell::Bash | Shell::Zsh | Shell::Sh => {
                format!("export {}={:?}:${}", name, value, name)
            }
        }
    }

    /// Generate an append statement for a PATH-like variable
    pub fn append_path(&self, name: &str, value: &str) -> String {
        match self {
            Shell::Fish => format!("set -gx {} ${} {:?}", name, name, value),
            Shell::PowerShell => format!(
                "$env:{} = $env:{} + [IO.Path]::PathSeparator + {:?}",
                name, name, value
            ),
            Shell::Bash | Shell::Zsh | Shell::Sh => {
                format!("export {}=${}:{:?}", name, name, value)
            }
        }
    }

    /// Generate a comment for this shell
    pub fn comment(&self, text: &str) -> String {
        match self {
            Shell::PowerShell => format!("# {}", text),
            Shell::Bash | Shell::Zsh | Shell::Fish | Shell::Sh => format!("# {}", text),
        }
    }

    /// Generate the script header/shebang
    pub fn header(&self) -> &'static str {
        match self {
            Shell::Bash => "#!/usr/bin/env bash",
            Shell::Zsh => "#!/usr/bin/env zsh",
            Shell::Fish => "# Fish shell environment",
            Shell::PowerShell => "# PowerShell environment",
            Shell::Sh => "#!/bin/sh",
        }
    }
}

impl std::fmt::Display for Shell {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_as_str() {
        assert_eq!(Shell::Bash.as_str(), "bash");
        assert_eq!(Shell::Zsh.as_str(), "zsh");
        assert_eq!(Shell::Fish.as_str(), "fish");
        assert_eq!(Shell::PowerShell.as_str(), "powershell");
        assert_eq!(Shell::Sh.as_str(), "sh");
    }

    #[test]
    fn test_shell_script_extension() {
        assert_eq!(Shell::Bash.script_extension(), "sh");
        assert_eq!(Shell::Zsh.script_extension(), "zsh");
        assert_eq!(Shell::Fish.script_extension(), "fish");
        assert_eq!(Shell::PowerShell.script_extension(), "ps1");
    }

    #[test]
    fn test_bash_export() {
        let export = Shell::Bash.export_var("EDITOR", "nvim");
        assert_eq!(export, r#"export EDITOR="nvim""#);
    }

    #[test]
    fn test_fish_export() {
        let export = Shell::Fish.export_var("EDITOR", "nvim");
        assert_eq!(export, r#"set -gx EDITOR "nvim""#);
    }

    #[test]
    fn test_powershell_export() {
        let export = Shell::PowerShell.export_var("EDITOR", "nvim");
        assert_eq!(export, r#"$env:EDITOR = "nvim""#);
    }

    #[test]
    fn test_bash_prepend_path() {
        let prepend = Shell::Bash.prepend_path("PATH", "/usr/local/bin");
        assert_eq!(prepend, r#"export PATH="/usr/local/bin":$PATH"#);
    }

    #[test]
    fn test_fish_prepend_path() {
        let prepend = Shell::Fish.prepend_path("PATH", "/usr/local/bin");
        assert_eq!(prepend, r#"set -gx PATH "/usr/local/bin" $PATH"#);
    }

    #[test]
    fn test_shell_detect() {
        // This test just ensures detection doesn't panic
        let shell = Shell::detect();
        assert!(!shell.as_str().is_empty());
    }

    #[test]
    fn test_shell_header() {
        assert!(Shell::Bash.header().contains("bash"));
        assert!(Shell::Zsh.header().contains("zsh"));
    }
}
