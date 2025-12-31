use predicates::prelude::*;

use super::common::TestEnv;

#[test]
fn gc_with_no_store_succeeds() {
  let env = TestEnv::empty();

  env
    .sys_cmd()
    .arg("gc")
    .assert()
    .success()
    .stdout(predicate::str::contains("Garbage collection complete"));
}

#[test]
fn gc_dry_run_shows_what_would_be_removed() {
  let env = TestEnv::empty();

  env
    .sys_cmd()
    .arg("gc")
    .arg("--dry-run")
    .assert()
    .success()
    .stdout(predicate::str::contains("Dry run"));
}

#[test]
fn gc_json_output_is_valid() {
  let env = TestEnv::empty();

  env
    .sys_cmd()
    .arg("gc")
    .args(["-o", "json"])
    .assert()
    .success()
    .stdout(predicate::str::contains("builds_deleted"))
    .stdout(predicate::str::contains("inputs_deleted"))
    .stdout(predicate::str::contains("deleted_paths"));
}
