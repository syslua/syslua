//! Shared test helpers for CLI integration tests.

use std::path::PathBuf;

use assert_cmd::Command;
use assert_cmd::cargo::cargo_bin_cmd;
use tempfile::TempDir;

/// Get path to a fixture file.
pub fn fixture_path(name: &str) -> PathBuf {
  PathBuf::from(env!("CARGO_MANIFEST_DIR"))
    .join("tests")
    .join("fixtures")
    .join(name)
}

/// Read fixture content.
pub fn fixture_content(name: &str) -> String {
  std::fs::read_to_string(fixture_path(name)).unwrap_or_else(|e| panic!("Failed to load fixture {}: {}", name, e))
}

/// Isolated test environment.
///
/// Each test gets its own temporary directory with isolated store, data, and output paths.
pub struct TestEnv {
  pub temp: TempDir,
  pub config_path: PathBuf,
}

impl TestEnv {
  /// Create from a fixture file.
  ///
  /// Copies the fixture content to a temporary `init.lua` file.
  pub fn from_fixture(name: &str) -> Self {
    let temp = TempDir::new().unwrap();
    let config_path = temp.path().join("init.lua");
    let content = fixture_content(name);
    std::fs::write(&config_path, content).unwrap();
    Self { temp, config_path }
  }

  /// Create an empty test environment.
  ///
  /// Use this when you need to manually set up the directory structure.
  pub fn empty() -> Self {
    let temp = TempDir::new().unwrap();
    let config_path = temp.path().join("init.lua");
    Self { temp, config_path }
  }

  /// Write a file relative to the temp directory.
  pub fn write_file(&self, relative_path: &str, content: &str) {
    let path = self.temp.path().join(relative_path);
    if let Some(parent) = path.parent() {
      std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&path, content).unwrap();
  }

  /// Store path (isolated per test).
  pub fn root_path(&self) -> PathBuf {
    let p = self.temp.path().join("syslua");
    std::fs::create_dir_all(&p).unwrap();
    dunce::canonicalize(&p).unwrap_or(p)
  }

  /// Data path for snapshots, plans, etc.
  pub fn data_path(&self) -> PathBuf {
    let p = self.temp.path().join("data");
    std::fs::create_dir_all(&p).unwrap();
    dunce::canonicalize(&p).unwrap_or(p)
  }

  /// Output path for bind test artifacts.
  pub fn output_path(&self) -> PathBuf {
    let p = self.temp.path().join("output");
    std::fs::create_dir_all(&p).unwrap();
    dunce::canonicalize(&p).unwrap_or(p)
  }

  /// Cache path for downloads, etc.
  pub fn cache_path(&self) -> PathBuf {
    let p = self.temp.path().join("cache");
    std::fs::create_dir_all(&p).unwrap();
    dunce::canonicalize(&p).unwrap_or(p)
  }

  /// Get a pre-configured Command for the sys binary.
  ///
  /// Sets environment variables for isolated testing:
  /// - `SYSLUA_ROOT`: Isolated root path
  /// - `XDG_DATA_HOME`: Isolated data path (for snapshots)
  /// - `XDG_CACHE_HOME`: Isolated cache path
  /// - `APPDATA`: Isolated data path (for Windows)
  /// - `LOCALAPPDATA`: Isolated cache path (for Windows)
  /// - `TEST_OUTPUT_DIR`: Output path for test artifacts
  pub fn sys_cmd(&self) -> Command {
    let mut cmd: Command = cargo_bin_cmd!("sys");
    cmd.env("SYSLUA_ROOT", self.root_path());
    cmd.env("XDG_DATA_HOME", self.data_path());
    cmd.env("XDG_CACHE_HOME", self.cache_path());
    cmd.env("APPDATA", self.data_path()); // For Windows
    cmd.env("LOCALAPPDATA", self.cache_path()); // For Windows cache
    cmd.env("TEST_OUTPUT_DIR", self.output_path());
    cmd
  }
}
