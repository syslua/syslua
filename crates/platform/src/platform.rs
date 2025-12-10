//! Platform detection and system information

use crate::error::PlatformError;

/// Operating system identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Os {
    Linux,
    Darwin,
    Windows,
}

impl Os {
    /// Detect the current operating system
    pub fn current() -> Self {
        #[cfg(target_os = "linux")]
        return Os::Linux;

        #[cfg(target_os = "macos")]
        return Os::Darwin;

        #[cfg(target_os = "windows")]
        return Os::Windows;

        #[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
        compile_error!("Unsupported operating system");
    }

    /// Get the OS as a string identifier
    pub fn as_str(&self) -> &'static str {
        match self {
            Os::Linux => "linux",
            Os::Darwin => "darwin",
            Os::Windows => "windows",
        }
    }
}

/// CPU architecture identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Arch {
    X86_64,
    Aarch64,
    Arm,
}

impl Arch {
    /// Detect the current CPU architecture
    pub fn current() -> Self {
        #[cfg(target_arch = "x86_64")]
        return Arch::X86_64;

        #[cfg(target_arch = "aarch64")]
        return Arch::Aarch64;

        #[cfg(target_arch = "arm")]
        return Arch::Arm;

        #[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64", target_arch = "arm")))]
        compile_error!("Unsupported architecture");
    }

    /// Get the architecture as a string identifier
    pub fn as_str(&self) -> &'static str {
        match self {
            Arch::X86_64 => "x86_64",
            Arch::Aarch64 => "aarch64",
            Arch::Arm => "arm",
        }
    }
}

/// Platform information container
///
/// Provides access to all platform-specific information needed by sys.lua,
/// including OS, architecture, user info, and standard paths.
#[derive(Debug, Clone)]
pub struct Platform {
    /// Operating system
    pub os: Os,
    /// CPU architecture
    pub arch: Arch,
    /// Combined platform identifier (e.g., "aarch64-darwin")
    pub platform: String,
    /// Current username
    pub username: String,
    /// Machine hostname
    pub hostname: String,
    /// User's home directory
    pub home_dir: std::path::PathBuf,
}

impl Platform {
    /// Detect and create platform information for the current system
    pub fn detect() -> Result<Self, PlatformError> {
        let os = Os::current();
        let arch = Arch::current();
        let platform = format!("{}-{}", arch.as_str(), os.as_str());

        let username = whoami::username();
        let hostname =
            whoami::fallible::hostname().map_err(|e| PlatformError::Hostname(e.to_string()))?;

        let home_dir = dirs::home_dir().ok_or(PlatformError::NoHomeDirectory)?;

        Ok(Self {
            os,
            arch,
            platform,
            username,
            hostname,
            home_dir,
        })
    }

    /// Check if running on Linux
    pub fn is_linux(&self) -> bool {
        self.os == Os::Linux
    }

    /// Check if running on macOS
    pub fn is_darwin(&self) -> bool {
        self.os == Os::Darwin
    }

    /// Check if running on Windows
    pub fn is_windows(&self) -> bool {
        self.os == Os::Windows
    }

    /// Get the user store path
    ///
    /// - Linux: `~/.local/share/syslua/store`
    /// - macOS: `~/Library/Application Support/syslua/store`
    /// - Windows: `%LOCALAPPDATA%\syslua\store`
    pub fn user_store_path(&self) -> std::path::PathBuf {
        match self.os {
            Os::Linux => self.home_dir.join(".local/share/syslua/store"),
            Os::Darwin => self
                .home_dir
                .join("Library/Application Support/syslua/store"),
            Os::Windows => dirs::data_local_dir()
                .unwrap_or_else(|| self.home_dir.clone())
                .join("syslua")
                .join("store"),
        }
    }

    /// Get the system store path
    ///
    /// - Linux/macOS: `/syslua/store`
    /// - Windows: `C:\syslua\store`
    pub fn system_store_path(&self) -> std::path::PathBuf {
        match self.os {
            Os::Linux | Os::Darwin => std::path::PathBuf::from("/syslua/store"),
            Os::Windows => std::path::PathBuf::from(r"C:\syslua\store"),
        }
    }

    /// Get the user config directory
    ///
    /// - Linux: `~/.config/syslua`
    /// - macOS: `~/.config/syslua` (or `~/Library/Application Support/syslua`)
    /// - Windows: `%APPDATA%\syslua`
    pub fn user_config_dir(&self) -> std::path::PathBuf {
        match self.os {
            Os::Linux | Os::Darwin => self.home_dir.join(".config/syslua"),
            Os::Windows => dirs::config_dir()
                .unwrap_or_else(|| self.home_dir.clone())
                .join("syslua"),
        }
    }

    /// Get the path where environment scripts are stored
    ///
    /// - Linux/macOS: `~/.config/syslua/env`
    /// - Windows: `%APPDATA%\syslua\env`
    pub fn env_script_dir(&self) -> std::path::PathBuf {
        self.user_config_dir().join("env")
    }

    /// Get the path for a specific shell's environment script
    pub fn env_script_path(&self, shell: &crate::Shell) -> std::path::PathBuf {
        let filename = format!("env.{}", shell.script_extension());
        self.env_script_dir().join(filename)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_detect() {
        let platform = Platform::detect().expect("Failed to detect platform");

        // Basic sanity checks
        assert!(!platform.username.is_empty());
        assert!(!platform.hostname.is_empty());
        assert!(platform.home_dir.exists());
        assert!(!platform.platform.is_empty());
    }

    #[test]
    fn test_os_as_str() {
        assert_eq!(Os::Linux.as_str(), "linux");
        assert_eq!(Os::Darwin.as_str(), "darwin");
        assert_eq!(Os::Windows.as_str(), "windows");
    }

    #[test]
    fn test_arch_as_str() {
        assert_eq!(Arch::X86_64.as_str(), "x86_64");
        assert_eq!(Arch::Aarch64.as_str(), "aarch64");
        assert_eq!(Arch::Arm.as_str(), "arm");
    }

    #[test]
    fn test_platform_checks() {
        let platform = Platform::detect().expect("Failed to detect platform");

        // Only one of these should be true
        let checks = [
            platform.is_linux(),
            platform.is_darwin(),
            platform.is_windows(),
        ];
        assert_eq!(checks.iter().filter(|&&x| x).count(), 1);
    }
}
