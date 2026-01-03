use anyhow::{Result, bail};
use std::io::{self, IsTerminal, Write};

pub fn confirm(message: &str, force: bool) -> Result<bool> {
  if force {
    return Ok(true);
  }

  if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
    bail!("Cannot prompt for confirmation in non-interactive mode. Use --force to proceed.");
  }

  write!(io::stderr(), "{} [y/N] ", message)?;
  io::stderr().flush()?;

  let mut input = String::new();
  io::stdin().read_line(&mut input)?;

  Ok(matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes"))
}
