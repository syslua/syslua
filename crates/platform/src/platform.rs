//! Platform and architecture detection

use serde::{Deserialize, Serialize};
use std::fmt;

/// Operating system
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Linux,
    Darwin,
    Windows,
}

impl Os {
    /// Detect the current operating system at compile time
    #[cfg(target_os = "linux")]
    pub const fn current() -> Self {
        Os::Linux
    }

    #[cfg(target_os = "macos")]
    pub const fn current() -> Self {
        Os::Darwin
    }

    #[cfg(target_os = "windows")]
    pub const fn current() -> Self {
        Os::Windows
    }

    /// Returns the OS name as used in platform strings
    pub const fn as_str(&self) -> &'static str {
        match self {
            Os::Linux => "linux",
            Os::Darwin => "darwin",
            Os::Windows => "windows",
        }
    }
}

impl fmt::Display for Os {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// CPU architecture
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Arch {
    X86_64,
    Aarch64,
    Arm,
}

impl Arch {
    /// Detect the current architecture at compile time
    #[cfg(target_arch = "x86_64")]
    pub const fn current() -> Self {
        Arch::X86_64
    }

    #[cfg(target_arch = "aarch64")]
    pub const fn current() -> Self {
        Arch::Aarch64
    }

    #[cfg(target_arch = "arm")]
    pub const fn current() -> Self {
        Arch::Arm
    }

    /// Returns the architecture name as used in platform strings
    pub const fn as_str(&self) -> &'static str {
        match self {
            Arch::X86_64 => "x86_64",
            Arch::Aarch64 => "aarch64",
            Arch::Arm => "arm",
        }
    }
}

impl fmt::Display for Arch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Combined platform identifier (e.g., "aarch64-darwin")
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Platform {
    pub arch: Arch,
    pub os: Os,
}

impl Platform {
    /// Create a new platform identifier
    pub const fn new(arch: Arch, os: Os) -> Self {
        Self { arch, os }
    }

    /// Detect the current platform at compile time
    pub const fn current() -> Self {
        Self {
            arch: Arch::current(),
            os: Os::current(),
        }
    }

    /// Returns the platform string (e.g., "aarch64-darwin")
    pub fn as_string(&self) -> String {
        format!("{}-{}", self.arch, self.os)
    }
}

impl fmt::Display for Platform {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}-{}", self.arch, self.os)
    }
}

/// Complete platform information including user details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformInfo {
    pub platform: Platform,
    pub os: Os,
    pub arch: Arch,
    pub hostname: String,
    pub username: String,
}

impl PlatformInfo {
    /// Gather current platform information
    pub fn current() -> Self {
        let platform = Platform::current();
        Self {
            platform,
            os: platform.os,
            arch: platform.arch,
            hostname: whoami::fallible::hostname().unwrap_or_else(|_| "unknown".to_string()),
            username: whoami::username(),
        }
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_platform_detection() {
        let info = PlatformInfo::current();

        // Should detect something
        assert!(!info.hostname.is_empty());
        assert!(!info.username.is_empty());

        // Platform string should be non-empty
        let platform_str = info.platform.to_string();
        assert!(platform_str.contains('-'));
    }

    #[test]
    fn test_platform_string_format() {
        let platform = Platform::new(Arch::Aarch64, Os::Darwin);
        assert_eq!(platform.to_string(), "aarch64-darwin");

        let platform = Platform::new(Arch::X86_64, Os::Linux);
        assert_eq!(platform.to_string(), "x86_64-linux");
    }
}
