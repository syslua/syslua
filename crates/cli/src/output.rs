//! CLI output formatting utilities.
//!
//! Provides consistent formatting for terminal output including colored status
//! messages, human-readable byte/duration formatting, and Unicode symbols.

use std::time::Duration;

use anyhow::Context;
use clap::ValueEnum;
use owo_colors::{OwoColorize, Stream};

#[derive(Debug, Clone, Copy, Default, ValueEnum)]
pub enum OutputFormat {
  #[default]
  Text,
  Json,
}

impl OutputFormat {
  pub fn is_json(self) -> bool {
    matches!(self, OutputFormat::Json)
  }
}

pub mod symbols {
  pub const SUCCESS: &str = "✓";
  pub const ERROR: &str = "✗";
  pub const WARNING: &str = "⚠";
  pub const INFO: &str = "•";
  pub const ARROW: &str = "→";
  pub const PLUS: &str = "+";
  pub const MINUS: &str = "-";
  pub const TILDE: &str = "~";
  pub const ADD: &str = "+";
  pub const MODIFY: &str = "~";
  pub const REMOVE: &str = "-";
}

pub fn truncate_hash(hash: &str) -> &str {
  let len = hash.len().min(12);
  &hash[..len]
}

pub fn format_bytes(bytes: u64) -> String {
  const KB: u64 = 1024;
  const MB: u64 = KB * 1024;
  const GB: u64 = MB * 1024;

  if bytes >= GB {
    format!("{:.1} GB", bytes as f64 / GB as f64)
  } else if bytes >= MB {
    format!("{:.1} MB", bytes as f64 / MB as f64)
  } else if bytes >= KB {
    format!("{:.1} KB", bytes as f64 / KB as f64)
  } else {
    format!("{} B", bytes)
  }
}

pub fn format_duration(duration: Duration) -> String {
  let secs = duration.as_secs();
  let millis = duration.subsec_millis();

  if secs >= 60 {
    let mins = secs / 60;
    let remaining_secs = secs % 60;
    format!("{}m {}s", mins, remaining_secs)
  } else if secs > 0 {
    format!("{}.{:02}s", secs, millis / 10)
  } else {
    format!("{}ms", millis)
  }
}

pub fn print_success(message: &str) {
  println!(
    "{} {}",
    symbols::SUCCESS.if_supports_color(Stream::Stdout, |s| s.green()),
    message
  );
}

pub fn print_error(message: &str) {
  eprintln!(
    "{} {}",
    symbols::ERROR.if_supports_color(Stream::Stderr, |s| s.red()),
    message.if_supports_color(Stream::Stderr, |s| s.red())
  );
}

pub fn print_warning(message: &str) {
  eprintln!(
    "{} {}",
    symbols::WARNING.if_supports_color(Stream::Stderr, |s| s.yellow()),
    message.if_supports_color(Stream::Stderr, |s| s.yellow())
  );
}

pub fn print_info(message: &str) {
  println!(
    "{} {}",
    symbols::INFO.if_supports_color(Stream::Stdout, |s| s.blue()),
    message
  );
}

pub fn print_stat(label: &str, value: &str) {
  println!(
    "  {}: {}",
    label.if_supports_color(Stream::Stdout, |s| s.dimmed()),
    value
  );
}

pub fn print_json<T: serde::Serialize>(value: &T) -> anyhow::Result<()> {
  let json = serde_json::to_string_pretty(value).context("Failed to serialize to JSON")?;
  println!("{}", json);
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn test_truncate_hash() {
    assert_eq!(truncate_hash("abcdef123456789"), "abcdef123456");
    assert_eq!(truncate_hash("short"), "short");
    assert_eq!(truncate_hash(""), "");
  }

  #[test]
  fn test_format_bytes() {
    assert_eq!(format_bytes(500), "500 B");
    assert_eq!(format_bytes(1024), "1.0 KB");
    assert_eq!(format_bytes(1536), "1.5 KB");
    assert_eq!(format_bytes(1048576), "1.0 MB");
    assert_eq!(format_bytes(1073741824), "1.0 GB");
  }

  #[test]
  fn test_format_duration() {
    assert_eq!(format_duration(Duration::from_millis(50)), "50ms");
    assert_eq!(format_duration(Duration::from_millis(1500)), "1.50s");
    assert_eq!(format_duration(Duration::from_secs(65)), "1m 5s");
  }
}
