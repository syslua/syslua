//! Script method integration tests.

use predicates::prelude::*;

use super::common::TestEnv;

#[test]
fn script_shell_executes_and_returns_stdout() {
  if cfg!(windows) {
    return;
  }

  let env = TestEnv::from_fixture("script_shell.lua");

  env
    .sys_cmd()
    .arg("apply")
    .arg(&env.config_path)
    .assert()
    .success()
    .stdout(predicate::str::contains("Builds realized: 1"));
}

#[test]
fn script_bash_executes_bash_syntax() {
  if cfg!(windows) {
    return;
  }

  let env = TestEnv::from_fixture("script_bash.lua");

  env
    .sys_cmd()
    .arg("apply")
    .arg(&env.config_path)
    .assert()
    .success()
    .stdout(predicate::str::contains("Builds realized: 1"));
}

#[test]
fn script_multiple_calls_get_unique_names() {
  if cfg!(windows) {
    return;
  }

  let env = TestEnv::from_fixture("script_multiple.lua");

  env
    .sys_cmd()
    .arg("apply")
    .arg(&env.config_path)
    .assert()
    .success()
    .stdout(predicate::str::contains("Builds realized: 1"));
}

#[test]
fn script_custom_name_option() {
  if cfg!(windows) {
    return;
  }

  let env = TestEnv::from_fixture("script_custom_name.lua");

  env
    .sys_cmd()
    .arg("apply")
    .arg(&env.config_path)
    .assert()
    .success()
    .stdout(predicate::str::contains("Builds realized: 1"));
}

#[test]
fn script_works_in_bind_context() {
  if cfg!(windows) {
    return;
  }

  let env = TestEnv::from_fixture("script_bind.lua");
  let marker_file = env.output_path().join("bind-script-marker.txt");

  env
    .sys_cmd()
    .arg("apply")
    .arg(&env.config_path)
    .assert()
    .success()
    .stdout(predicate::str::contains("Binds applied: 1"));

  assert!(marker_file.exists(), "bind script should have created marker file");
}

#[test]
fn script_invalid_format_errors() {
  let env = TestEnv::from_fixture("script_invalid_format.lua");

  env
    .sys_cmd()
    .arg("apply")
    .arg(&env.config_path)
    .assert()
    .failure()
    .stderr(predicate::str::contains("format must be"));
}

#[test]
fn script_powershell_format() {
  if !cfg!(windows) {
    return;
  }

  let env = TestEnv::from_fixture("script_powershell.lua");

  env
    .sys_cmd()
    .arg("apply")
    .arg(&env.config_path)
    .assert()
    .success()
    .stdout(predicate::str::contains("Builds realized: 1"));
}

#[test]
fn script_cmd_format() {
  if !cfg!(windows) {
    return;
  }

  let env = TestEnv::from_fixture("script_cmd.lua");

  env
    .sys_cmd()
    .arg("apply")
    .arg(&env.config_path)
    .assert()
    .success()
    .stdout(predicate::str::contains("Builds realized: 1"));
}
