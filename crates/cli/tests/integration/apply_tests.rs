//! Apply command integration tests.

use predicates::prelude::*;

use super::common::TestEnv;

#[test]
fn apply_minimal_config() {
  let env = TestEnv::from_fixture("minimal.lua");

  env
    .sys_cmd()
    .arg("apply")
    .arg(&env.config_path)
    .assert()
    .success()
    .stdout(predicate::str::contains("Apply complete"));
}

#[test]
fn apply_build_with_execution() {
  let env = TestEnv::from_fixture("build_with_exec.lua");

  env
    .sys_cmd()
    .arg("apply")
    .arg(&env.config_path)
    .assert()
    .success()
    .stdout(predicate::str::contains("Builds realized: 1"));
}

#[test]
fn apply_is_idempotent() {
  let env = TestEnv::from_fixture("build_with_exec.lua");

  // First apply
  env.sys_cmd().arg("apply").arg(&env.config_path).assert().success();

  // Second apply should show cached
  env
    .sys_cmd()
    .arg("apply")
    .arg(&env.config_path)
    .assert()
    .success()
    .stdout(predicate::str::contains("Builds realized: 0"));
}

#[test]
fn apply_build_only() {
  let env = TestEnv::from_fixture("build_only.lua");

  env
    .sys_cmd()
    .arg("apply")
    .arg(&env.config_path)
    .assert()
    .success()
    .stdout(predicate::str::contains("Builds realized: 1"));
}

#[test]
fn apply_bind_create_and_destroy() {
  let env = TestEnv::from_fixture("bind_create.lua");
  let marker_file = env.output_path().join("created.txt");

  // Apply creates the bind
  env
    .sys_cmd()
    .arg("apply")
    .arg(&env.config_path)
    .assert()
    .success()
    .stdout(predicate::str::contains("Binds applied: 1"));

  assert!(marker_file.exists(), "bind should create marker file");
}

#[test]
fn apply_multi_build() {
  let env = TestEnv::from_fixture("multi_build.lua");

  env
    .sys_cmd()
    .arg("apply")
    .arg(&env.config_path)
    .assert()
    .success()
    .stdout(predicate::str::contains("Builds realized: 2"));
}

#[test]
fn apply_shows_no_drift_when_file_exists() {
  let env = TestEnv::from_fixture("bind_check.lua");
  let marker_file = env.output_path().join("check-marker.txt");

  env.sys_cmd().arg("apply").arg(&env.config_path).assert().success();

  assert!(marker_file.exists(), "bind should create marker file");

  env
    .sys_cmd()
    .arg("apply")
    .arg(&env.config_path)
    .assert()
    .success()
    .stdout(predicate::str::contains("Drift detected").not());
}

#[test]
fn apply_detects_drift_when_file_deleted() {
  let env = TestEnv::from_fixture("bind_check.lua");
  let marker_file = env.output_path().join("check-marker.txt");

  env.sys_cmd().arg("apply").arg(&env.config_path).assert().success();

  assert!(marker_file.exists(), "bind should create marker file");

  std::fs::remove_file(&marker_file).expect("failed to delete marker file");

  env
    .sys_cmd()
    .arg("apply")
    .arg(&env.config_path)
    .assert()
    .success()
    .stdout(predicate::str::contains("Drift detected"))
    .stdout(predicate::str::contains("check-test"));
}

#[test]
fn apply_with_repair_fixes_drift() {
  let env = TestEnv::from_fixture("bind_check.lua");
  let marker_file = env.output_path().join("check-marker.txt");

  env.sys_cmd().arg("apply").arg(&env.config_path).assert().success();

  assert!(marker_file.exists(), "bind should create marker file");

  std::fs::remove_file(&marker_file).expect("failed to delete marker file");

  assert!(!marker_file.exists(), "marker file should be deleted");

  env
    .sys_cmd()
    .arg("apply")
    .arg("--repair")
    .arg(&env.config_path)
    .assert()
    .success()
    .stdout(predicate::str::contains("Binds repaired:"));

  assert!(marker_file.exists(), "repair should recreate marker file");
}

#[test]
fn drift_does_not_affect_exit_code() {
  let env = TestEnv::from_fixture("bind_check.lua");
  let marker_file = env.output_path().join("check-marker.txt");

  env.sys_cmd().arg("apply").arg(&env.config_path).assert().success();

  std::fs::remove_file(&marker_file).expect("failed to delete marker file");

  env
    .sys_cmd()
    .arg("apply")
    .arg(&env.config_path)
    .assert()
    .success()
    .stdout(predicate::str::contains("Drift detected"));
}
