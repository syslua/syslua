//! Cmd action implementation.
//!
//! This module handles executing shell commands with isolated environments,
//! following Nix-inspired principles.

use std::collections::BTreeMap;
use std::path::Path;

use tokio::process::Command;
use tracing::{debug, info};

use crate::execute::types::ExecuteError;

/// Execute a Cmd action.
///
/// Runs the command in an isolated environment:
/// - Clears all environment variables
/// - Sets PATH to /path-not-set (to fail fast if deps aren't specified)
/// - Sets HOME to /homeless-shelter
/// - Sets TMPDIR/TMP/TEMP/TEMPDIR to a temp directory within out_dir
/// - Sets `out` to the output directory
/// - Merges user-specified environment variables
///
/// # Arguments
///
/// * `cmd` - The command string to execute
/// * `env` - Optional user-specified environment variables
/// * `cwd` - Optional working directory (defaults to out_dir)
/// * `out_dir` - The build's output directory
/// * `shell` - The shell to use (defaults to /bin/sh on Unix, cmd.exe on Windows)
///
/// # Returns
///
/// The stdout of the command on success (trimmed).
pub async fn execute_cmd(
  cmd: &str,
  env: Option<&BTreeMap<String, String>>,
  cwd: Option<&str>,
  out_dir: &Path,
  shell: Option<&str>,
) -> Result<String, ExecuteError> {
  info!(cmd = %cmd, "executing command");

  // Create temp directory for the build
  let tmp_dir = out_dir.join("tmp");
  tokio::fs::create_dir_all(&tmp_dir).await?;

  // Determine shell and arguments
  let (shell_cmd, shell_arg) = get_shell(shell);

  let working_dir = cwd.map(Path::new).unwrap_or(out_dir);

  // Build the command with isolated environment
  let mut command = Command::new(&shell_cmd);
  command
    .arg(&shell_arg)
    .arg(cmd)
    .current_dir(working_dir)
    // Clear all environment variables
    .env_clear()
    // Set isolated environment
    .env("PATH", "/path-not-set")
    .env("HOME", "/homeless-shelter")
    .env("TMPDIR", &tmp_dir)
    .env("TMP", &tmp_dir)
    .env("TEMP", &tmp_dir)
    .env("TEMPDIR", &tmp_dir)
    .env("out", out_dir)
    // Set a minimal locale
    .env("LANG", "C")
    .env("LC_ALL", "C")
    // Set SOURCE_DATE_EPOCH for reproducible timestamps
    // Value is 315532800 = January 1, 1980 00:00:00 UTC (ZIP epoch)
    .env("SOURCE_DATE_EPOCH", "315532800");

  // Merge user-specified environment variables
  if let Some(user_env) = env {
    for (key, value) in user_env {
      command.env(key, value);
    }
  }

  debug!(shell = %shell_cmd, working_dir = ?working_dir, "spawning process");

  let output = command.output().await?;

  if !output.status.success() {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Log output for debugging
    if !stderr.is_empty() {
      debug!(stderr = %stderr, "command stderr");
    }
    if !stdout.is_empty() {
      debug!(stdout = %stdout, "command stdout");
    }

    return Err(ExecuteError::CmdFailed {
      cmd: cmd.to_string(),
      code: output.status.code(),
    });
  }

  let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();

  if !stdout.is_empty() {
    debug!(stdout = %stdout, "command output");
  }

  Ok(stdout)
}

/// Get the shell command and argument for the current platform.
///
/// # Arguments
///
/// * `override_shell` - Optional shell override from config
///
/// # Returns
///
/// A tuple of (shell_command, shell_argument) where:
/// - shell_command is the path to the shell
/// - shell_argument is the flag to pass the command (e.g., "-c" for sh)
///
/// # Note
///
/// For isolated builds, we always use `/bin/sh` (Unix) or `cmd.exe` (Windows)
/// by default, rather than the user's configured shell. This is because
/// interactive shells like bash/zsh may source profile files that modify
/// the environment (e.g., adding to PATH), which would break isolation.
/// Use the `override_shell` parameter only when you explicitly want a
/// different shell.
fn get_shell(override_shell: Option<&str>) -> (String, String) {
  if let Some(shell) = override_shell {
    // User explicitly specified a shell - use it
    return (shell.to_string(), "-c".to_string());
  }

  // Use the default system shell for isolation
  // Don't use $SHELL as it may source user profiles
  #[cfg(unix)]
  {
    ("/bin/sh".to_string(), "-c".to_string())
  }

  #[cfg(windows)]
  {
    ("cmd.exe".to_string(), "/C".to_string())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use tempfile::TempDir;

  /// Get an echo command that prints an environment variable.
  /// Unix: echo $VAR
  /// Windows: echo %VAR%
  #[cfg(unix)]
  fn echo_env(var: &str) -> String {
    format!("echo ${}", var)
  }

  #[cfg(windows)]
  fn echo_env(var: &str) -> String {
    format!("echo %{}%", var)
  }

  /// Get a command that prints the current working directory.
  /// Unix: pwd
  /// Windows: cd (prints current directory)
  #[cfg(unix)]
  fn print_cwd() -> &'static str {
    "pwd"
  }

  #[cfg(windows)]
  fn print_cwd() -> &'static str {
    "cd"
  }

  #[tokio::test]
  async fn execute_simple_command() {
    let temp_dir = TempDir::new().unwrap();
    let out_dir = temp_dir.path();

    let result = execute_cmd("echo hello", None, None, out_dir, None).await.unwrap();

    assert_eq!(result, "hello");
  }

  #[tokio::test]
  async fn execute_command_with_env() {
    let temp_dir = TempDir::new().unwrap();
    let out_dir = temp_dir.path();

    let mut env = BTreeMap::new();
    env.insert("MY_VAR".to_string(), "my_value".to_string());

    let result = execute_cmd(&echo_env("MY_VAR"), Some(&env), None, out_dir, None)
      .await
      .unwrap();

    assert_eq!(result, "my_value");
  }

  #[tokio::test]
  async fn execute_command_out_env_set() {
    let temp_dir = TempDir::new().unwrap();
    let out_dir = temp_dir.path();

    let result = execute_cmd(&echo_env("out"), None, None, out_dir, None).await.unwrap();

    assert_eq!(result, out_dir.to_string_lossy());
  }

  #[tokio::test]
  async fn execute_command_isolated_path() {
    let temp_dir = TempDir::new().unwrap();
    let out_dir = temp_dir.path();

    let result = execute_cmd(&echo_env("PATH"), None, None, out_dir, None).await.unwrap();

    assert_eq!(result, "/path-not-set");
  }

  #[tokio::test]
  async fn execute_command_has_source_date_epoch() {
    let temp_dir = TempDir::new().unwrap();
    let out_dir = temp_dir.path();

    let result = execute_cmd(&echo_env("SOURCE_DATE_EPOCH"), None, None, out_dir, None)
      .await
      .unwrap();

    assert_eq!(result, "315532800");
  }

  #[tokio::test]
  async fn execute_command_failure() {
    let temp_dir = TempDir::new().unwrap();
    let out_dir = temp_dir.path();

    let result = execute_cmd("exit 1", None, None, out_dir, None).await;

    assert!(matches!(result, Err(ExecuteError::CmdFailed { code: Some(1), .. })));
  }

  #[tokio::test]
  async fn execute_command_with_cwd() {
    let temp_dir = TempDir::new().unwrap();
    let out_dir = temp_dir.path();

    // Create a subdirectory
    let sub_dir = out_dir.join("subdir");
    tokio::fs::create_dir(&sub_dir).await.unwrap();

    let result = execute_cmd(print_cwd(), None, Some(sub_dir.to_str().unwrap()), out_dir, None)
      .await
      .unwrap();

    // On macOS, /var is a symlink to /private/var, so we need to canonicalize
    let expected = sub_dir.canonicalize().unwrap();
    assert_eq!(result, expected.to_string_lossy());
  }

  #[tokio::test]
  async fn execute_command_creates_tmp_dir() {
    let temp_dir = TempDir::new().unwrap();
    let out_dir = temp_dir.path();

    execute_cmd(&echo_env("TMPDIR"), None, None, out_dir, None)
      .await
      .unwrap();

    // Verify tmp directory was created
    assert!(out_dir.join("tmp").exists());
  }

  #[tokio::test]
  #[cfg(unix)]
  async fn execute_multiline_command() {
    let temp_dir = TempDir::new().unwrap();
    let out_dir = temp_dir.path();

    let cmd = r#"
      x=1
      y=2
      echo $((x + y))
    "#;

    let result = execute_cmd(cmd, None, None, out_dir, None).await.unwrap();

    assert_eq!(result, "3");
  }

  #[tokio::test]
  #[cfg(windows)]
  async fn execute_multiline_command() {
    let temp_dir = TempDir::new().unwrap();
    let out_dir = temp_dir.path();

    // Windows cmd.exe equivalent using set /a for arithmetic
    let cmd = r#"set /a result=1+2 && echo %result%"#;

    let result = execute_cmd(cmd, None, None, out_dir, None).await.unwrap();

    // Note: set /a prints the result directly, so we may get "3" or need adjustment
    assert!(result.contains("3"));
  }

  #[test]
  fn get_shell_with_override() {
    let (shell, arg) = get_shell(Some("/usr/bin/bash"));
    assert_eq!(shell, "/usr/bin/bash");
    assert_eq!(arg, "-c");
  }

  #[test]
  fn get_shell_default() {
    // Default shell should be /bin/sh regardless of SHELL env var
    let (shell, arg) = get_shell(None);
    #[cfg(unix)]
    {
      assert_eq!(shell, "/bin/sh");
      assert_eq!(arg, "-c");
    }
    #[cfg(windows)]
    {
      assert_eq!(shell, "cmd.exe");
      assert_eq!(arg, "/C");
    }
  }
}
