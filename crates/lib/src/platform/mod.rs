pub mod arch;
pub mod os;
pub mod paths;

use arch::Arch;
use os::Os;
use std::fmt;

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

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn platform_triple_format() {
    // Verifies the triple format is "arch-os"
    let platform = Platform::new(Arch::Aarch64, Os::MacOs);
    assert_eq!(platform.triple(), "aarch64-darwin");

    let platform = Platform::new(Arch::X86_64, Os::Linux);
    assert_eq!(platform.triple(), "x86_64-linux");
  }
}
