//! Cross-platform abstractions.
//!
//! Provides platform detection, path conventions, and OS-specific utilities.

pub mod arch;
pub mod immutable;
pub mod os;
pub mod paths;

use arch::Arch;
use os::Os;
use std::fmt;

pub use immutable::{ImmutableError, make_immutable, make_mutable};

/// Platform identifier combining architecture and OS (e.g., "aarch64-darwin")
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Platform {
  pub arch: Arch,
  pub os: Os,
}

impl Platform {
  /// Create a new platform identifier
  pub fn new(arch: Arch, os: Os) -> Self {
    Self { arch, os }
  }

  /// Detect the current platform at runtime
  ///
  /// Returns `None` if the OS or architecture is not supported
  pub fn current() -> Option<Self> {
    Some(Self {
      arch: Arch::current()?,
      os: Os::current()?,
    })
  }

  /// Returns the platform triple string (e.g., "aarch64-darwin")
  pub fn triple(&self) -> String {
    format!("{}-{}", self.arch, self.os)
  }
}

impl fmt::Display for Platform {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{}", self.triple())
  }
}

/// Returns the platform triple for the current system (e.g., "aarch64-darwin")
///
/// Returns `None` if the current platform is not supported
pub fn platform_triple() -> Option<String> {
  Platform::current().map(|p| p.triple())
}

/// Check if the current process is running with elevated privileges.
///
/// On Unix systems, this checks if the effective user ID is root (0).
/// On Windows, this checks if the process has administrator privileges.
#[cfg(unix)]
pub fn is_elevated() -> bool {
  rustix::process::geteuid().is_root()
}

#[cfg(windows)]
pub fn is_elevated() -> bool {
  use std::mem::{size_of, zeroed};
  use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    Security::{GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation},
    System::Threading::{GetCurrentProcess, OpenProcessToken},
  };

  unsafe {
    let mut token: HANDLE = std::ptr::null_mut();
    if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
      return false;
    }

    let mut elevation: TOKEN_ELEVATION = zeroed();
    let mut size: u32 = 0;
    let result = GetTokenInformation(
      token,
      TokenElevation,
      &mut elevation as *mut _ as *mut _,
      size_of::<TOKEN_ELEVATION>() as u32,
      &mut size,
    );

    CloseHandle(token);
    result != 0 && elevation.TokenIsElevated != 0
  }
}
