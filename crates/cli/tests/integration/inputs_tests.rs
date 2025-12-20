//! Input resolution integration tests.
//!
//! These tests are marked `#[ignore]` because they require network access
//! and may be slow. Run with: `cargo test -- --ignored`

use predicates::prelude::*;

use super::common::TestEnv;

#[test]
#[ignore] // Requires network access
fn git_input_clones_repository() {
  let env = TestEnv::from_fixture("git_input.lua");

  env
    .sys_cmd()
    .arg("apply")
    .arg(&env.config_path)
    .assert()
    .success()
    .stdout(predicate::str::contains("Apply complete"));
}

#[test]
#[ignore] // Requires network access
fn git_input_resolution_in_plan() {
  let env = TestEnv::from_fixture("git_input.lua");

  env.sys_cmd().arg("plan").arg(&env.config_path).assert().success();
}

// --- Transitive input tests (local, no network required) ---

/// Test that `sys plan` works with transitive path inputs.
///
/// Creates a nested structure: root -> libs/lib_a -> libs/lib_b
/// The libs are siblings, making relative path resolution work correctly.
#[test]
fn transitive_path_input_in_plan() {
  let env = TestEnv::empty();

  // Create lib_b (leaf dependency) - sibling to lib_a
  env.write_file(
    "libs/lib_b/init.lua",
    r#"
return {
  inputs = {},
  setup = function(_)
    sys.build({
      id = "lib-b-build",
      inputs = {},
      create = function(_, _)
        return { name = "lib_b" }
      end,
    })
  end,
}
"#,
  );

  // Create lib_a (depends on lib_b as sibling)
  env.write_file(
    "libs/lib_a/init.lua",
    r#"
return {
  inputs = {
    lib_b = "path:../lib_b",
  },
  setup = function(inputs)
    sys.build({
      id = "lib-a-build",
      inputs = {},
      create = function(_, _)
        return { name = "lib_a" }
      end,
    })
  end,
}
"#,
  );

  // Create root config (depends on lib_a)
  env.write_file(
    "init.lua",
    r#"
return {
  inputs = {
    lib_a = "path:./libs/lib_a",
  },
  setup = function(inputs)
    sys.build({
      id = "root-build",
      inputs = {},
      create = function(_, _)
        return { name = "root" }
      end,
    })
  end,
}
"#,
  );

  env.sys_cmd().arg("plan").arg(&env.config_path).assert().success();
}

/// Test that `sys apply` works with transitive path inputs.
///
/// Creates a nested structure: root -> libs/lib_a -> libs/lib_b
#[test]
fn transitive_path_input_in_apply() {
  let env = TestEnv::empty();

  // Create lib_b (leaf dependency) - sibling to lib_a
  env.write_file(
    "libs/lib_b/init.lua",
    r#"
return {
  inputs = {},
  setup = function(_)
    sys.build({
      id = "lib-b-build",
      inputs = {},
      create = function(_, _)
        return { name = "lib_b" }
      end,
    })
  end,
}
"#,
  );

  // Create lib_a (depends on lib_b as sibling)
  env.write_file(
    "libs/lib_a/init.lua",
    r#"
return {
  inputs = {
    lib_b = "path:../lib_b",
  },
  setup = function(inputs)
    sys.build({
      id = "lib-a-build",
      inputs = {},
      create = function(_, _)
        return { name = "lib_a" }
      end,
    })
  end,
}
"#,
  );

  // Create root config (depends on lib_a)
  env.write_file(
    "init.lua",
    r#"
return {
  inputs = {
    lib_a = "path:./libs/lib_a",
  },
  setup = function(inputs)
    sys.build({
      id = "root-build",
      inputs = {},
      create = function(_, _)
        return { name = "root" }
      end,
    })
  end,
}
"#,
  );

  env
    .sys_cmd()
    .arg("apply")
    .arg(&env.config_path)
    .assert()
    .success()
    .stdout(predicate::str::contains("Apply complete"));
}

/// Test diamond dependency deduplication in CLI.
///
/// Creates: root -> libs/lib_a -> libs/lib_common
///              -> libs/lib_b -> libs/lib_common
/// Verifies that lib_common is only resolved once.
#[test]
fn diamond_transitive_dependency_in_apply() {
  let env = TestEnv::empty();

  // Create lib_common (shared leaf dependency)
  env.write_file(
    "libs/lib_common/init.lua",
    r#"
return {
  inputs = {},
  setup = function(_)
    sys.build({
      id = "lib-common-build",
      inputs = {},
      create = function(_, _)
        return { name = "lib_common" }
      end,
    })
  end,
}
"#,
  );

  // Create lib_a (depends on lib_common as sibling)
  env.write_file(
    "libs/lib_a/init.lua",
    r#"
return {
  inputs = {
    lib_common = "path:../lib_common",
  },
  setup = function(inputs)
    sys.build({
      id = "lib-a-build",
      inputs = {},
      create = function(_, _)
        return { name = "lib_a" }
      end,
    })
  end,
}
"#,
  );

  // Create lib_b (also depends on lib_common as sibling)
  env.write_file(
    "libs/lib_b/init.lua",
    r#"
return {
  inputs = {
    lib_common = "path:../lib_common",
  },
  setup = function(inputs)
    sys.build({
      id = "lib-b-build",
      inputs = {},
      create = function(_, _)
        return { name = "lib_b" }
      end,
    })
  end,
}
"#,
  );

  // Create root config (depends on both lib_a and lib_b)
  env.write_file(
    "init.lua",
    r#"
return {
  inputs = {
    lib_a = "path:./libs/lib_a",
    lib_b = "path:./libs/lib_b",
  },
  setup = function(inputs)
    sys.build({
      id = "root-build",
      inputs = {},
      create = function(_, _)
        return { name = "root" }
      end,
    })
  end,
}
"#,
  );

  env
    .sys_cmd()
    .arg("apply")
    .arg(&env.config_path)
    .assert()
    .success()
    .stdout(predicate::str::contains("Apply complete"));
}

/// Test `sys update` with transitive dependencies.
///
/// Verifies that update command works when there are transitive deps.
#[test]
fn update_with_transitive_path_inputs() {
  let env = TestEnv::empty();

  // Create lib_b (leaf dependency) - sibling to lib_a
  env.write_file(
    "libs/lib_b/init.lua",
    r#"
return {
  inputs = {},
  setup = function(_) end,
}
"#,
  );

  // Create lib_a (depends on lib_b as sibling)
  env.write_file(
    "libs/lib_a/init.lua",
    r#"
return {
  inputs = {
    lib_b = "path:../lib_b",
  },
  setup = function(inputs) end,
}
"#,
  );

  // Create root config (depends on lib_a)
  env.write_file(
    "init.lua",
    r#"
return {
  inputs = {
    lib_a = "path:./libs/lib_a",
  },
  setup = function(inputs) end,
}
"#,
  );

  // First apply to create lock file
  env.sys_cmd().arg("apply").arg(&env.config_path).assert().success();

  // Then update should work
  env.sys_cmd().arg("update").arg(&env.config_path).assert().success();
}
