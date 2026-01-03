use super::common::TestEnv;

#[test]
fn test_snapshot_list_empty() {
  let env = TestEnv::empty();

  let output = env.sys_cmd().args(["snapshot", "list"]).output().unwrap();
  assert!(output.status.success());
  let combined = format!(
    "{}{}",
    String::from_utf8_lossy(&output.stdout),
    String::from_utf8_lossy(&output.stderr)
  );
  assert!(combined.contains("No snapshots"));
}

#[test]
fn test_snapshot_list_after_apply() {
  let env = TestEnv::from_fixture("minimal.lua");

  let apply_output = env
    .sys_cmd()
    .args(["apply", env.config_path.to_str().unwrap()])
    .output()
    .unwrap();
  assert!(
    apply_output.status.success(),
    "apply failed: {}",
    String::from_utf8_lossy(&apply_output.stderr)
  );

  let output = env.sys_cmd().args(["snapshot", "list"]).output().unwrap();
  assert!(output.status.success());

  let stdout = String::from_utf8_lossy(&output.stdout);
  assert!(stdout.contains("(current)") || stdout.contains("snapshot"));
}

#[test]
fn test_snapshot_list_verbose() {
  let env = TestEnv::from_fixture("minimal.lua");

  let apply_output = env
    .sys_cmd()
    .args(["apply", env.config_path.to_str().unwrap()])
    .output()
    .unwrap();
  assert!(apply_output.status.success());

  let output = env.sys_cmd().args(["snapshot", "list", "--verbose"]).output().unwrap();
  assert!(output.status.success());

  let stdout = String::from_utf8_lossy(&output.stdout);
  assert!(stdout.contains("builds:") || stdout.contains("binds:"));
}

#[test]
fn test_snapshot_list_json() {
  let env = TestEnv::from_fixture("minimal.lua");

  let apply_output = env
    .sys_cmd()
    .args(["apply", env.config_path.to_str().unwrap()])
    .output()
    .unwrap();
  assert!(apply_output.status.success());

  let output = env.sys_cmd().args(["snapshot", "list", "-o", "json"]).output().unwrap();
  assert!(output.status.success());

  let stdout = String::from_utf8_lossy(&output.stdout);
  assert!(stdout.contains("\"snapshots\""));
  let parsed: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
  assert!(parsed["snapshots"].is_array());
}

#[test]
fn test_snapshot_show() {
  let env = TestEnv::from_fixture("minimal.lua");

  let apply_output = env
    .sys_cmd()
    .args(["apply", env.config_path.to_str().unwrap()])
    .output()
    .unwrap();
  assert!(apply_output.status.success());

  let list_output = env.sys_cmd().args(["snapshot", "list", "-o", "json"]).output().unwrap();
  let list_json: serde_json::Value = serde_json::from_slice(&list_output.stdout).expect("valid JSON");
  let snapshot_id = list_json["snapshots"][0]["id"].as_str().expect("snapshot ID");

  let output = env.sys_cmd().args(["snapshot", "show", snapshot_id]).output().unwrap();
  assert!(output.status.success());

  let stdout = String::from_utf8_lossy(&output.stdout);
  assert!(stdout.contains("Snapshot:"));
  assert!(stdout.contains(snapshot_id));
}

#[test]
fn test_snapshot_show_json() {
  let env = TestEnv::from_fixture("minimal.lua");

  let apply_output = env
    .sys_cmd()
    .args(["apply", env.config_path.to_str().unwrap()])
    .output()
    .unwrap();
  assert!(apply_output.status.success());

  let list_output = env.sys_cmd().args(["snapshot", "list", "-o", "json"]).output().unwrap();
  let list_json: serde_json::Value = serde_json::from_slice(&list_output.stdout).expect("valid JSON");
  let snapshot_id = list_json["snapshots"][0]["id"].as_str().expect("snapshot ID");

  let output = env
    .sys_cmd()
    .args(["snapshot", "show", snapshot_id, "-o", "json"])
    .output()
    .unwrap();
  assert!(output.status.success());

  let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).expect("valid JSON");
  assert_eq!(parsed["id"].as_str(), Some(snapshot_id));
  assert!(parsed["builds"].is_array());
  assert!(parsed["binds"].is_array());
}

#[test]
fn test_snapshot_delete_current_skipped() {
  let env = TestEnv::from_fixture("minimal.lua");

  let apply_output = env
    .sys_cmd()
    .args(["apply", env.config_path.to_str().unwrap()])
    .output()
    .unwrap();
  assert!(
    apply_output.status.success(),
    "apply failed: {}",
    String::from_utf8_lossy(&apply_output.stderr)
  );

  let list_output = env.sys_cmd().args(["snapshot", "list", "-o", "json"]).output().unwrap();
  assert!(
    list_output.status.success(),
    "list failed: {}",
    String::from_utf8_lossy(&list_output.stderr)
  );
  let list_json: serde_json::Value = serde_json::from_slice(&list_output.stdout).expect("valid JSON");
  let snapshot_id = list_json["current"].as_str().expect("current ID should exist");

  let output = env
    .sys_cmd()
    .args(["snapshot", "delete", snapshot_id, "--force"])
    .output()
    .unwrap();
  assert!(output.status.success());

  let stderr = String::from_utf8_lossy(&output.stderr);
  assert!(stderr.contains("current") || stderr.contains("destroy"));

  let verify = env.sys_cmd().args(["snapshot", "show", snapshot_id]).output().unwrap();
  assert!(verify.status.success());
}

#[test]
fn test_snapshot_tag_untag() {
  let env = TestEnv::from_fixture("minimal.lua");

  env
    .sys_cmd()
    .args(["apply", env.config_path.to_str().unwrap()])
    .output()
    .unwrap();

  let list_output = env.sys_cmd().args(["snapshot", "list", "-o", "json"]).output().unwrap();
  let list_json: serde_json::Value = serde_json::from_slice(&list_output.stdout).expect("valid JSON");
  let snapshot_id = list_json["snapshots"][0]["id"].as_str().expect("snapshot ID");

  let tag_output = env
    .sys_cmd()
    .args(["snapshot", "tag", snapshot_id, "my-tag"])
    .output()
    .unwrap();
  assert!(tag_output.status.success());

  let list2 = env.sys_cmd().args(["snapshot", "list"]).output().unwrap();
  let stdout = String::from_utf8_lossy(&list2.stdout);
  assert!(stdout.contains("[my-tag]"));

  let untag_output = env
    .sys_cmd()
    .args(["snapshot", "untag", snapshot_id, "my-tag"])
    .output()
    .unwrap();
  assert!(untag_output.status.success());

  let list3 = env.sys_cmd().args(["snapshot", "list"]).output().unwrap();
  let stdout3 = String::from_utf8_lossy(&list3.stdout);
  assert!(!stdout3.contains("[my-tag]"));
}

#[test]
fn test_snapshot_multiple_tags() {
  let env = TestEnv::from_fixture("minimal.lua");

  env
    .sys_cmd()
    .args(["apply", env.config_path.to_str().unwrap()])
    .output()
    .unwrap();

  let list_output = env.sys_cmd().args(["snapshot", "list", "-o", "json"]).output().unwrap();
  let list_json: serde_json::Value = serde_json::from_slice(&list_output.stdout).expect("valid JSON");
  let snapshot_id = list_json["snapshots"][0]["id"].as_str().expect("snapshot ID");

  env
    .sys_cmd()
    .args(["snapshot", "tag", snapshot_id, "tag1"])
    .output()
    .unwrap();
  env
    .sys_cmd()
    .args(["snapshot", "tag", snapshot_id, "tag2"])
    .output()
    .unwrap();

  let list2 = env.sys_cmd().args(["snapshot", "list"]).output().unwrap();
  let stdout = String::from_utf8_lossy(&list2.stdout);
  assert!(stdout.contains("tag1") && stdout.contains("tag2"));

  let untag_output = env.sys_cmd().args(["snapshot", "untag", snapshot_id]).output().unwrap();
  assert!(untag_output.status.success());

  let list3 = env.sys_cmd().args(["snapshot", "list"]).output().unwrap();
  let stdout3 = String::from_utf8_lossy(&list3.stdout);
  assert!(!stdout3.contains("tag1"));
  assert!(!stdout3.contains("tag2"));
}

#[test]
fn test_snapshot_delete_older_than() {
  let env = TestEnv::from_fixture("minimal.lua");

  env
    .sys_cmd()
    .args(["apply", env.config_path.to_str().unwrap()])
    .output()
    .unwrap();

  let output = env
    .sys_cmd()
    .args(["snapshot", "delete", "--older-than", "1s", "--force"])
    .output()
    .unwrap();
  assert!(output.status.success());

  let combined = format!(
    "{}{}",
    String::from_utf8_lossy(&output.stdout),
    String::from_utf8_lossy(&output.stderr)
  );
  assert!(combined.contains("No snapshots") || combined.contains("current") || combined.contains("Cancelled"));
}
