use std::fmt;

/// CPU architecture variants supported by SysLua
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Arch {
  X86_64,
  Aarch64,
}

impl Arch {
  /// Detect the current CPU architecture at runtime
  pub fn current() -> Option<Self> {
    match std::env::consts::ARCH {
      "x86_64" => Some(Self::X86_64),
      "aarch64" => Some(Self::Aarch64),
      _ => None,
    }
  }

  /// Returns the lowercase string identifier for this architecture
  pub fn as_str(&self) -> &'static str {
    match self {
      Self::X86_64 => "x86_64",
      Self::Aarch64 => "aarch64",
    }
  }
}

impl fmt::Display for Arch {
  fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
    write!(f, "{}", self.as_str())
  }
}

/// Returns the current CPU architecture
///
/// Returns `None` if the architecture is not supported
pub fn arch() -> Option<Arch> {
  Arch::current()
}
