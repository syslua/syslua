use std::fmt;

/// Operating system variants supported by sys.lua
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Os {
  Linux,
  MacOs,
  Windows,
}

impl Os {
  /// Detect the current operating system at runtime
  pub fn current() -> Option<Self> {
    match std::env::consts::OS {
      "linux" => Some(Self::Linux),
      "macos" => Some(Self::MacOs),
      "windows" => Some(Self::Windows),
      _ => None,
    }
  }

  /// Returns the lowercase string identifier for this OS
  pub fn as_str(&self) -> &'static str {
    match self {
      Self::Linux => "linux",
      Self::MacOs => "darwin",
      Self::Windows => "windows",
    }
  }
}

impl fmt::Display for Os {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{}", self.as_str())
  }
}

/// Returns the current operating system
///
/// Returns `None` if the OS is not supported
pub fn os() -> Option<Os> {
  Os::current()
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn current_returns_supported_os() {
    // Verifies we're running on a supported OS
    assert!(Os::current().is_some(), "Current OS should be supported");
  }

  #[test]
  fn macos_uses_darwin_identifier() {
    // Darwin is the expected identifier for macOS in platform triples
    assert_eq!(Os::MacOs.as_str(), "darwin");
  }
}
