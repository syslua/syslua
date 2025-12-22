//! Destroy command integration tests.

use predicates::prelude::*;

use super::common::TestEnv;

#[test]
fn destroy_removes_bind_artifacts() {
  let env = TestEnv::from_fixture("bind_create.lua");
  let marker_file = env.output_path().join("created.txt");

  // First apply to create the bind
  env.sys_cmd().arg("apply").arg(&env.config_path).assert().success();

  assert!(marker_file.exists(), "marker file should exist after apply");

  // Destroy should remove it
  env
    .sys_cmd()
    .arg("destroy")
    .assert()
    .success()
    .stdout(predicate::str::contains("Destroy complete"));

  assert!(!marker_file.exists(), "marker file should be removed after destroy");
}

#[test]
fn destroy_with_no_state_succeeds() {
  // Destroy with no previous state should succeed gracefully
  let env = TestEnv::from_fixture("minimal.lua");

  env
    .sys_cmd()
    .arg("destroy")
    .assert()
    .success()
    .stdout(predicate::str::contains("Nothing to destroy"));
}

#[test]
fn destroy_is_idempotent() {
  let env = TestEnv::from_fixture("bind_create.lua");
  let marker_file = env.output_path().join("created.txt");

  // Apply to create state
  env.sys_cmd().arg("apply").arg(&env.config_path).assert().success();
  assert!(marker_file.exists(), "marker file should exist after apply");

  // First destroy
  env
    .sys_cmd()
    .arg("destroy")
    .assert()
    .success()
    .stdout(predicate::str::contains("Destroy complete"));

  assert!(
    !marker_file.exists(),
    "marker file should be removed after first destroy"
  );

  // Second destroy should be a no-op
  env
    .sys_cmd()
    .arg("destroy")
    .assert()
    .success()
    .stdout(predicate::str::contains("Nothing to destroy"));
}

#[test]
fn destroy_dry_run_shows_plan() {
  let env = TestEnv::from_fixture("bind_create.lua");
  let marker_file = env.output_path().join("created.txt");

  // Apply to create state
  env.sys_cmd().arg("apply").arg(&env.config_path).assert().success();
  assert!(marker_file.exists(), "marker file should exist after apply");

  // Dry run should show what would be destroyed but not actually destroy
  env
    .sys_cmd()
    .arg("destroy")
    .arg("--dry-run")
    .assert()
    .success()
    .stdout(predicate::str::contains("dry run"))
    .stdout(predicate::str::contains("Would destroy"));

  // File should still exist after dry run
  assert!(marker_file.exists(), "marker file should still exist after dry run");

  // Actual destroy should still work
  env.sys_cmd().arg("destroy").assert().success();
  assert!(
    !marker_file.exists(),
    "marker file should be removed after actual destroy"
  );
}
