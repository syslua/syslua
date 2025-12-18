//! CLI smoke tests for sys.
//!
//! These tests verify that all CLI commands run without panicking and
//! return appropriate exit codes.

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use predicates::prelude::*;
use tempfile::TempDir;

/// Get a Command for the sys binary.
fn sys_cmd() -> Command {
  cargo_bin_cmd!("sys")
}

/// Create a temp directory with a config file.
fn temp_config(content: &str) -> TempDir {
  let temp = TempDir::new().unwrap();
  std::fs::write(temp.path().join("init.lua"), content).unwrap();
  temp
}

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
// Help & Version
// =============================================================================

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
  let temp = TempDir::new().unwrap();
  let init_dir = temp.path().join("myconfig");

  sys_cmd()
    .arg("init")
    .arg(&init_dir)
    .env("SYSLUA_SYSTEM_STORE", temp.path().join("store"))
    .env("SYSLUA_USER_STORE", temp.path().join("store"))
    .env("XDG_DATA_HOME", temp.path().join("data"))
    .assert()
    .success();

  assert!(init_dir.join("init.lua").exists());
  assert!(init_dir.join(".luarc.json").exists());
}

#[test]
fn init_fails_if_config_exists() {
  let temp = temp_config(MINIMAL_CONFIG);

  sys_cmd()
    .arg("init")
    .arg(temp.path())
    .env("SYSLUA_SYSTEM_STORE", temp.path().join("store"))
    .env("SYSLUA_USER_STORE", temp.path().join("store"))
    .env("XDG_DATA_HOME", temp.path().join("data"))
    .assert()
    .failure()
    .stderr(predicate::str::contains("already exists"));
}

// =============================================================================
// plan
// =============================================================================

#[test]
fn plan_with_minimal_config() {
  let temp = temp_config(MINIMAL_CONFIG);

  sys_cmd()
    .arg("plan")
    .arg(temp.path().join("init.lua"))
    .env("SYSLUA_SYSTEM_STORE", temp.path().join("store"))
    .env("SYSLUA_USER_STORE", temp.path().join("store"))
    .env("XDG_DATA_HOME", temp.path().join("data"))
    .assert()
    .success();
}

#[test]
fn plan_with_build_shows_build_count() {
  let temp = temp_config(BUILD_CONFIG);

  sys_cmd()
    .arg("plan")
    .arg(temp.path().join("init.lua"))
    .env("SYSLUA_SYSTEM_STORE", temp.path().join("store"))
    .env("SYSLUA_USER_STORE", temp.path().join("store"))
    .env("XDG_DATA_HOME", temp.path().join("data"))
    .assert()
    .success()
    .stdout(predicate::str::contains("Builds: 1"));
}

#[test]
fn plan_nonexistent_config_fails() {
  let temp = TempDir::new().unwrap();

  sys_cmd()
    .arg("plan")
    .arg("/nonexistent/path/config.lua")
    .env("SYSLUA_SYSTEM_STORE", temp.path().join("store"))
    .env("SYSLUA_USER_STORE", temp.path().join("store"))
    .env("XDG_DATA_HOME", temp.path().join("data"))
    .assert()
    .failure();
}

// =============================================================================
// apply
// =============================================================================

#[test]
fn apply_minimal_config() {
  let temp = temp_config(MINIMAL_CONFIG);

  sys_cmd()
    .arg("apply")
    .arg(temp.path().join("init.lua"))
    .env("SYSLUA_SYSTEM_STORE", temp.path().join("store"))
    .env("SYSLUA_USER_STORE", temp.path().join("store"))
    .env("XDG_DATA_HOME", temp.path().join("data"))
    .assert()
    .success()
    .stdout(predicate::str::contains("Apply complete"));
}

#[test]
fn apply_with_build_succeeds() {
  let temp = temp_config(BUILD_CONFIG);

  sys_cmd()
    .arg("apply")
    .arg(temp.path().join("init.lua"))
    .env("SYSLUA_SYSTEM_STORE", temp.path().join("store"))
    .env("SYSLUA_USER_STORE", temp.path().join("store"))
    .env("XDG_DATA_HOME", temp.path().join("data"))
    .assert()
    .success()
    .stdout(predicate::str::contains("Builds realized: 1"));
}

#[test]
fn apply_nonexistent_config_fails() {
  let temp = TempDir::new().unwrap();

  sys_cmd()
    .arg("apply")
    .arg("/nonexistent/path/config.lua")
    .env("SYSLUA_SYSTEM_STORE", temp.path().join("store"))
    .env("SYSLUA_USER_STORE", temp.path().join("store"))
    .env("XDG_DATA_HOME", temp.path().join("data"))
    .assert()
    .failure();
}

// =============================================================================
// destroy
// =============================================================================

#[test]
fn destroy_placeholder_works() {
  // destroy is currently a placeholder that just prints a message
  let temp = TempDir::new().unwrap();

  sys_cmd()
    .arg("destroy")
    .arg(temp.path().join("init.lua"))
    .env("SYSLUA_SYSTEM_STORE", temp.path().join("store"))
    .env("SYSLUA_USER_STORE", temp.path().join("store"))
    .env("XDG_DATA_HOME", temp.path().join("data"))
    .assert()
    .success()
    .stdout(predicate::str::contains("destroy"));
}

// =============================================================================
// update
// =============================================================================

#[test]
fn update_with_no_inputs() {
  let temp = temp_config(MINIMAL_CONFIG);

  sys_cmd()
    .arg("update")
    .arg(temp.path().join("init.lua"))
    .env("SYSLUA_SYSTEM_STORE", temp.path().join("store"))
    .env("SYSLUA_USER_STORE", temp.path().join("store"))
    .env("XDG_DATA_HOME", temp.path().join("data"))
    .assert()
    .success()
    .stdout(predicate::str::contains("up to date"));
}

#[test]
fn update_dry_run() {
  let temp = temp_config(MINIMAL_CONFIG);

  sys_cmd()
    .arg("update")
    .arg(temp.path().join("init.lua"))
    .arg("--dry-run")
    .env("SYSLUA_SYSTEM_STORE", temp.path().join("store"))
    .env("SYSLUA_USER_STORE", temp.path().join("store"))
    .env("XDG_DATA_HOME", temp.path().join("data"))
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
// Error Handling
// =============================================================================

#[test]
fn invalid_lua_syntax_fails() {
  let temp = temp_config("this is not valid lua {{{");

  sys_cmd()
    .arg("plan")
    .arg(temp.path().join("init.lua"))
    .env("SYSLUA_SYSTEM_STORE", temp.path().join("store"))
    .env("SYSLUA_USER_STORE", temp.path().join("store"))
    .env("XDG_DATA_HOME", temp.path().join("data"))
    .assert()
    .failure();
}

#[test]
fn missing_setup_function_fails() {
  let temp = temp_config("return { inputs = {} }");

  sys_cmd()
    .arg("plan")
    .arg(temp.path().join("init.lua"))
    .env("SYSLUA_SYSTEM_STORE", temp.path().join("store"))
    .env("SYSLUA_USER_STORE", temp.path().join("store"))
    .env("XDG_DATA_HOME", temp.path().join("data"))
    .assert()
    .failure();
}
