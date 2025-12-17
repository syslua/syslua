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
