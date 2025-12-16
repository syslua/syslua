//! Store object immutability management.
//!
//! After a build completes, its store path is made immutable (write-protected)
//! to prevent accidental modification. This mirrors Nix's approach of using
//! basic file permissions rather than ACLs.
//!
//! ## Platform Behavior
//!
//! - **Unix**: Sets permissions to 0444 (files) or 0555 (dirs/executables)
//! - **macOS**: Additionally clears BSD file flags to allow future GC
//! - **Windows**: Sets FILE_ATTRIBUTE_READONLY via `set_readonly(true)`

use std::path::Path;

use tracing::{debug, warn};
use walkdir::WalkDir;

/// Error during immutability operations.
#[derive(Debug, thiserror::Error)]
pub enum ImmutableError {
  #[error("failed to set permissions on {path}: {source}")]
  SetPermissions {
    path: String,
    #[source]
    source: std::io::Error,
  },

  #[error("failed to read metadata for {path}: {source}")]
  Metadata {
    path: String,
    #[source]
    source: std::io::Error,
  },

  #[error("failed to traverse directory {path}: {source}")]
  WalkDir {
    path: String,
    #[source]
    source: walkdir::Error,
  },
}

/// Make a store path immutable (write-protected).
///
/// Recursively removes write permissions from all files and directories.
/// On errors, logs warnings and continues (best-effort).
///
/// # Platform Behavior
///
/// - **Unix**: Sets permissions to 0444 (files) or 0555 (dirs/executables)
/// - **macOS**: Additionally clears BSD file flags
/// - **Windows**: Sets FILE_ATTRIBUTE_READONLY
pub fn make_immutable(path: &Path) -> Result<(), ImmutableError> {
  if !path.exists() {
    return Ok(());
  }

  debug!(path = ?path, "making store path immutable");

  // Process deepest entries first (post-order) so we can chmod directories
  // after their contents
  for entry in WalkDir::new(path).contents_first(true) {
    let entry = entry.map_err(|e| ImmutableError::WalkDir {
      path: path.display().to_string(),
      source: e,
    })?;

    if let Err(e) = make_entry_immutable(entry.path()) {
      warn!(path = ?entry.path(), error = %e, "failed to make immutable, continuing");
    }
  }

  // On macOS, clear BSD flags to allow future GC
  #[cfg(target_os = "macos")]
  clear_bsd_flags(path);

  Ok(())
}

/// Make a store path mutable again (for GC or rebuild).
///
/// Recursively restores write permissions to allow modification/deletion.
/// Must be called before attempting to delete or modify a store path.
///
/// # Platform Behavior
///
/// - **Unix**: Sets permissions to 0644 (files) or 0755 (dirs/executables)
/// - **Windows**: Clears FILE_ATTRIBUTE_READONLY
pub fn make_mutable(path: &Path) -> Result<(), ImmutableError> {
  if !path.exists() {
    return Ok(());
  }

  debug!(path = ?path, "making store path mutable");

  // Process directories before contents (pre-order) so we can enter them
  for entry in WalkDir::new(path) {
    let entry = entry.map_err(|e| ImmutableError::WalkDir {
      path: path.display().to_string(),
      source: e,
    })?;

    if let Err(e) = make_entry_mutable(entry.path()) {
      warn!(path = ?entry.path(), error = %e, "failed to make mutable, continuing");
    }
  }

  Ok(())
}

// ============ Unix Implementation ============

#[cfg(unix)]
fn make_entry_immutable(path: &Path) -> Result<(), ImmutableError> {
  use std::os::unix::fs::PermissionsExt;

  let metadata = std::fs::metadata(path).map_err(|e| ImmutableError::Metadata {
    path: path.display().to_string(),
    source: e,
  })?;

  let current_mode = metadata.permissions().mode();

  // Files: 0444, Executables/Dirs: 0555
  let new_mode = if metadata.is_dir() || (current_mode & 0o111 != 0) {
    0o555
  } else {
    0o444
  };

  let mut perms = metadata.permissions();
  perms.set_mode(new_mode);
  std::fs::set_permissions(path, perms).map_err(|e| ImmutableError::SetPermissions {
    path: path.display().to_string(),
    source: e,
  })?;

  Ok(())
}

#[cfg(unix)]
fn make_entry_mutable(path: &Path) -> Result<(), ImmutableError> {
  use std::os::unix::fs::PermissionsExt;

  let metadata = std::fs::metadata(path).map_err(|e| ImmutableError::Metadata {
    path: path.display().to_string(),
    source: e,
  })?;

  let current_mode = metadata.permissions().mode();

  // Files: 0644, Executables/Dirs: 0755
  let new_mode = if metadata.is_dir() || (current_mode & 0o111 != 0) {
    0o755
  } else {
    0o644
  };

  let mut perms = metadata.permissions();
  perms.set_mode(new_mode);
  std::fs::set_permissions(path, perms).map_err(|e| ImmutableError::SetPermissions {
    path: path.display().to_string(),
    source: e,
  })?;

  Ok(())
}

/// Clear BSD file flags on macOS to allow future deletion.
///
/// This clears flags like UF_IMMUTABLE that could prevent garbage collection.
#[cfg(target_os = "macos")]
fn clear_bsd_flags(path: &Path) {
  use std::ffi::CString;
  use std::os::unix::ffi::OsStrExt;

  for entry in WalkDir::new(path).into_iter().filter_map(|e| e.ok()) {
    if let Ok(cpath) = CString::new(entry.path().as_os_str().as_bytes()) {
      // chflags(path, 0) clears all flags including UF_IMMUTABLE
      // We use chflags (not lchflags) as libc doesn't export lchflags
      // This means symlinks themselves won't have flags cleared, but
      // that's acceptable for our use case
      // We ignore errors here as this is best-effort
      unsafe {
        libc::chflags(cpath.as_ptr(), 0);
      }
    }
  }
}

// ============ Windows Implementation ============
//
// On Windows, we use the simpler FILE_ATTRIBUTE_READONLY approach via
// std::fs::set_permissions. This is more reliable than ACL manipulation
// and properly reflects in permissions().readonly().

#[cfg(windows)]
fn make_entry_immutable(path: &Path) -> Result<(), ImmutableError> {
  let metadata = std::fs::metadata(path).map_err(|e| ImmutableError::Metadata {
    path: path.display().to_string(),
    source: e,
  })?;

  let mut perms = metadata.permissions();
  perms.set_readonly(true);
  std::fs::set_permissions(path, perms).map_err(|e| ImmutableError::SetPermissions {
    path: path.display().to_string(),
    source: e,
  })?;

  Ok(())
}

#[cfg(windows)]
fn make_entry_mutable(path: &Path) -> Result<(), ImmutableError> {
  let metadata = std::fs::metadata(path).map_err(|e| ImmutableError::Metadata {
    path: path.display().to_string(),
    source: e,
  })?;

  let mut perms = metadata.permissions();
  perms.set_readonly(false);
  std::fs::set_permissions(path, perms).map_err(|e| ImmutableError::SetPermissions {
    path: path.display().to_string(),
    source: e,
  })?;

  Ok(())
}

#[cfg(test)]
mod tests {
  use std::fs;

  use tempfile::TempDir;

  use super::*;

  #[test]
  fn immutable_nonexistent_path_succeeds() {
    let temp = TempDir::new().unwrap();
    let nonexistent = temp.path().join("does-not-exist");
    assert!(make_immutable(&nonexistent).is_ok());
  }

  #[test]
  fn mutable_nonexistent_path_succeeds() {
    let temp = TempDir::new().unwrap();
    let nonexistent = temp.path().join("does-not-exist");
    assert!(make_mutable(&nonexistent).is_ok());
  }

  #[test]
  fn immutable_prevents_write() {
    let temp = TempDir::new().unwrap();
    let file = temp.path().join("test.txt");
    fs::write(&file, "original").unwrap();

    make_immutable(temp.path()).unwrap();

    // Verify file is read-only
    let perms = fs::metadata(&file).unwrap().permissions();
    assert!(perms.readonly());

    // Writing should fail
    let write_result = fs::write(&file, "modified");
    assert!(write_result.is_err());

    // Cleanup: make mutable so tempdir can delete
    make_mutable(temp.path()).unwrap();
  }

  #[test]
  fn mutable_allows_write_after_immutable() {
    let temp = TempDir::new().unwrap();
    let file = temp.path().join("test.txt");
    fs::write(&file, "original").unwrap();

    make_immutable(temp.path()).unwrap();
    make_mutable(temp.path()).unwrap();

    // Writing should now succeed
    fs::write(&file, "modified").unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), "modified");
  }

  #[test]
  fn immutable_handles_nested_directories() {
    let temp = TempDir::new().unwrap();
    let subdir = temp.path().join("subdir");
    fs::create_dir(&subdir).unwrap();
    let file = subdir.join("nested.txt");
    fs::write(&file, "content").unwrap();

    make_immutable(temp.path()).unwrap();

    // Both directory and nested file should be immutable
    assert!(fs::metadata(&subdir).unwrap().permissions().readonly());
    assert!(fs::metadata(&file).unwrap().permissions().readonly());

    // Cleanup
    make_mutable(temp.path()).unwrap();
  }

  #[test]
  #[cfg(unix)]
  fn immutable_preserves_executable_bit() {
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new().unwrap();
    let file = temp.path().join("script.sh");
    fs::write(&file, "#!/bin/sh\necho hello").unwrap();

    // Make executable
    let mut perms = fs::metadata(&file).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&file, perms).unwrap();

    make_immutable(temp.path()).unwrap();

    // Should be 0555 (executable but not writable)
    let mode = fs::metadata(&file).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o555);

    // Cleanup
    make_mutable(temp.path()).unwrap();
  }

  #[test]
  #[cfg(unix)]
  fn immutable_sets_correct_file_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new().unwrap();
    let file = temp.path().join("data.txt");
    fs::write(&file, "content").unwrap();

    make_immutable(temp.path()).unwrap();

    // Non-executable file should be 0444
    let mode = fs::metadata(&file).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o444);

    // Directory should be 0555
    let dir_mode = fs::metadata(temp.path()).unwrap().permissions().mode();
    assert_eq!(dir_mode & 0o777, 0o555);

    // Cleanup
    make_mutable(temp.path()).unwrap();
  }

  #[test]
  #[cfg(unix)]
  fn mutable_restores_correct_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let temp = TempDir::new().unwrap();
    let file = temp.path().join("data.txt");
    fs::write(&file, "content").unwrap();

    make_immutable(temp.path()).unwrap();
    make_mutable(temp.path()).unwrap();

    // Non-executable file should be 0644
    let mode = fs::metadata(&file).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o644);

    // Directory should be 0755
    let dir_mode = fs::metadata(temp.path()).unwrap().permissions().mode();
    assert_eq!(dir_mode & 0o777, 0o755);
  }

  #[test]
  fn roundtrip_multiple_times() {
    let temp = TempDir::new().unwrap();
    let file = temp.path().join("test.txt");
    fs::write(&file, "original").unwrap();

    // Multiple immutable/mutable cycles should work
    for i in 0..3 {
      make_immutable(temp.path()).unwrap();

      // Should not be writable
      assert!(fs::write(&file, format!("attempt {}", i)).is_err());

      make_mutable(temp.path()).unwrap();

      // Should be writable now
      fs::write(&file, format!("success {}", i)).unwrap();
    }
  }
}
