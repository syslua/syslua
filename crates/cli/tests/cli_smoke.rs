//! CLI smoke tests for sys.
//!
//! These tests verify that all CLI commands run without panicking and
//! return appropriate exit codes.

use std::path::PathBuf;

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use tempfile::TempDir;

// =============================================================================
// Test Environment
// =============================================================================

/// Isolated test environment with consistent env var handling.
struct TestEnv {
  temp: TempDir,
  config_path: PathBuf,
}

impl TestEnv {
  /// Create a test environment with the given config content.
  fn with_config(content: &str) -> Self {
    let temp = TempDir::new().unwrap();
    let config_path = temp.path().join("init.lua");
    std::fs::write(&config_path, content).unwrap();
    Self { temp, config_path }
  }

  /// Create an empty test environment (no config file).
  fn empty() -> Self {
    let temp = TempDir::new().unwrap();
    let config_path = temp.path().join("init.lua");
    Self { temp, config_path }
  }

  /// Get a Command for the sys binary with isolated environment.
  ///
  /// Sets:
  /// - `SYSLUA_ROOT`: Isolated root for store/snapshots
  /// - `XDG_DATA_HOME`: Isolated data path (Unix)
  /// - `APPDATA`: Isolated data path (Windows)
  fn cmd(&self) -> Command {
    let mut cmd: Command = cargo_bin_cmd!("sys");
    cmd.env("SYSLUA_ROOT", self.temp.path().join("syslua"));
    cmd.env("XDG_DATA_HOME", self.temp.path().join("data"));
    cmd.env("XDG_CACHE_HOME", self.temp.path().join("cache"));
    cmd.env("APPDATA", self.temp.path().join("data"));
    cmd.env("LOCALAPPDATA", self.temp.path().join("cache"));
    cmd
  }

  /// Path to the config file.
  fn config(&self) -> &PathBuf {
    &self.config_path
  }
}

// =============================================================================
// Test Configs
// =============================================================================

/// Minimal valid config that does nothing (no exec calls).
const MINIMAL_CONFIG: &str = r#"
return {
    inputs = {},
    setup = function(_) end,
}
"#;

/// Config with a build that just returns output dir (no actions needed).
const BUILD_CONFIG: &str = r#"
return {
    inputs = {},
    setup = function(_)
        sys.build({
            id = "test-pkg",
            create = function(_, ctx)
                return { out = ctx.out }
            end,
        })
    end,
}
"#;

// =============================================================================
// Help & Version (no isolation needed)
// =============================================================================

/// Get a bare Command for the sys binary (no env isolation).
fn sys_cmd() -> Command {
  cargo_bin_cmd!("sys")
}

#[test]
fn help_flag_works() {
  sys_cmd()
    .arg("--help")
    .assert()
    .success()
    .stdout(predicate::str::contains("Usage"));
}

#[test]
fn version_flag_works() {
  sys_cmd()
    .arg("--version")
    .assert()
    .success()
    .stdout(predicate::str::contains("syslua"));
}

#[test]
fn subcommand_help_works() {
  for cmd in &["apply", "plan", "destroy", "init", "update", "info"] {
    sys_cmd()
      .arg(cmd)
      .arg("--help")
      .assert()
      .success()
      .stdout(predicate::str::contains("Usage"));
  }
}

// =============================================================================
// init
// =============================================================================

#[test]
fn init_creates_config_files() {
  let env = TestEnv::empty();
  let init_dir = env.temp.path().join("myconfig");

  env.cmd().arg("init").arg(&init_dir).assert().success();

  assert!(init_dir.join("init.lua").exists());
  assert!(init_dir.join(".luarc.json").exists());
}

#[test]
fn init_fails_if_config_exists() {
  let env = TestEnv::with_config(MINIMAL_CONFIG);

  env
    .cmd()
    .arg("init")
    .arg(env.temp.path())
    .assert()
    .failure()
    .stderr(predicate::str::contains("already exists"));
}

// =============================================================================
// plan
// =============================================================================

#[test]
fn plan_with_minimal_config() {
  let env = TestEnv::with_config(MINIMAL_CONFIG);

  env.cmd().arg("plan").arg(env.config()).assert().success();
}

#[test]
fn plan_with_build_shows_build_count() {
  let env = TestEnv::with_config(BUILD_CONFIG);

  env
    .cmd()
    .arg("plan")
    .arg(env.config())
    .assert()
    .success()
    .stdout(predicate::str::contains("Builds: 1"));
}

#[test]
fn plan_nonexistent_config_fails() {
  let env = TestEnv::empty();

  env
    .cmd()
    .arg("plan")
    .arg("/nonexistent/path/config.lua")
    .assert()
    .failure();
}

// =============================================================================
// apply
// =============================================================================

#[test]
fn apply_minimal_config() {
  let env = TestEnv::with_config(MINIMAL_CONFIG);

  env
    .cmd()
    .arg("apply")
    .arg(env.config())
    .assert()
    .success()
    .stdout(predicate::str::contains("Apply complete"));
}

#[test]
fn apply_with_build_succeeds() {
  let env = TestEnv::with_config(BUILD_CONFIG);

  env
    .cmd()
    .arg("apply")
    .arg(env.config())
    .assert()
    .success()
    .stdout(predicate::str::contains("Builds realized: 1"));
}

#[test]
fn apply_nonexistent_config_fails() {
  let env = TestEnv::empty();

  env
    .cmd()
    .arg("apply")
    .arg("/nonexistent/path/config.lua")
    .assert()
    .failure();
}

// =============================================================================
// destroy
// =============================================================================

#[test]
fn destroy_with_no_state_succeeds() {
  let env = TestEnv::empty();

  env
    .cmd()
    .arg("destroy")
    .assert()
    .success()
    .stdout(predicate::str::contains("Nothing to destroy"));
}

// =============================================================================
// update
// =============================================================================

#[test]
fn update_with_no_inputs() {
  let env = TestEnv::with_config(MINIMAL_CONFIG);

  env
    .cmd()
    .arg("update")
    .arg(env.config())
    .assert()
    .success()
    .stdout(predicate::str::contains("up to date"));
}

#[test]
fn update_dry_run() {
  let env = TestEnv::with_config(MINIMAL_CONFIG);

  env
    .cmd()
    .arg("update")
    .arg(env.config())
    .arg("--dry-run")
    .assert()
    .success()
    .stdout(predicate::str::contains("Dry run"));
}

// =============================================================================
// info
// =============================================================================

#[test]
fn info_shows_platform() {
  sys_cmd()
    .arg("info")
    .assert()
    .success()
    .stdout(predicate::str::contains("Platform"));
}

// =============================================================================
// status
// =============================================================================

#[test]
fn status_no_snapshot() {
  let env = TestEnv::empty();
  env
    .cmd()
    .arg("status")
    .assert()
    .success()
    .stdout(predicate::str::contains("No snapshot found"));
}

#[test]
fn status_after_apply() {
  let env = TestEnv::with_config(BUILD_CONFIG);

  env.cmd().arg("apply").arg(env.config()).assert().success();

  env
    .cmd()
    .arg("status")
    .assert()
    .success()
    .stdout(predicate::str::contains("Current snapshot:"))
    .stdout(predicate::str::contains("Builds: 1"))
    .stdout(predicate::str::contains("Binds: 0"));
}

#[test]
fn status_verbose() {
  let env = TestEnv::with_config(BUILD_CONFIG);

  env.cmd().arg("apply").arg(env.config()).assert().success();

  env
    .cmd()
    .arg("status")
    .arg("--verbose")
    .assert()
    .success()
    .stdout(predicate::str::contains("test-pkg-"));
}

#[test]
fn status_help() {
  sys_cmd()
    .arg("status")
    .arg("--help")
    .assert()
    .success()
    .stdout(predicate::str::contains("Show current system state"));
}

// =============================================================================
// Error Handling
// =============================================================================

#[test]
fn invalid_lua_syntax_fails() {
  let env = TestEnv::with_config("this is not valid lua {{{");

  env.cmd().arg("plan").arg(env.config()).assert().failure();
}

#[test]
fn missing_setup_function_fails() {
  let env = TestEnv::with_config("return { inputs = {} }");

  env.cmd().arg("plan").arg(env.config()).assert().failure();
}
