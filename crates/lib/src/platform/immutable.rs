//! Store object immutability management.
//!
//! After a build completes, its store path is made immutable (write-protected)
//! to prevent accidental modification. This mirrors Nix's approach.
//!
//! ## Platform Behavior
//!
//! - **Unix**: Sets permissions to 0444 (files) or 0555 (dirs/executables)
//! - **macOS**: Additionally clears BSD file flags to allow future GC
//! - **Windows**: Adds DENY ACE for write operations to "Everyone"

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

  #[cfg(windows)]
  #[error("failed to modify ACL for {path}: {message}")]
  Acl { path: String, message: String },
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
/// - **Windows**: Adds DENY ACE for write operations to "Everyone"
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
/// - **Windows**: Removes the DENY ACE added by make_immutable
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

// ============ Windows ACL Implementation ============

#[cfg(windows)]
mod windows_acl {
  use std::ffi::OsStr;
  use std::os::windows::ffi::OsStrExt;
  use std::path::Path;

  use windows_sys::Win32::Foundation::{ERROR_SUCCESS, GetLastError, LocalFree};
  use windows_sys::Win32::Security::Authorization::{
    ConvertStringSidToSidW, GetNamedSecurityInfoW, SE_FILE_OBJECT, SetNamedSecurityInfoW,
  };
  use windows_sys::Win32::Security::{
    ACCESS_DENIED_ACE, ACE_HEADER, ACL, ACL_REVISION, ACL_SIZE_INFORMATION, AclSizeInformation, AddAccessDeniedAceEx,
    AddAce, CONTAINER_INHERIT_ACE, DACL_SECURITY_INFORMATION, GetAce, GetAclInformation, GetLengthSid, InitializeAcl,
    IsValidAcl, OBJECT_INHERIT_ACE, PSID,
  };

  use super::ImmutableError;

  // Write permissions to deny
  const FILE_WRITE_DATA: u32 = 0x0002;
  const FILE_APPEND_DATA: u32 = 0x0004;
  const FILE_WRITE_EA: u32 = 0x0010;
  const FILE_DELETE_CHILD: u32 = 0x0040;
  const FILE_WRITE_ATTRIBUTES: u32 = 0x0100;
  const DELETE: u32 = 0x0001_0000;

  /// Combined mask of all write-related permissions to deny.
  const DENY_MASK: u32 =
    FILE_WRITE_DATA | FILE_APPEND_DATA | FILE_WRITE_EA | FILE_DELETE_CHILD | FILE_WRITE_ATTRIBUTES | DELETE;

  /// ACCESS_DENIED_ACE_TYPE value
  const ACCESS_DENIED_ACE_TYPE: u8 = 1;

  fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain(Some(0)).collect()
  }

  fn get_everyone_sid() -> Result<PSID, u32> {
    let sid_string = to_wide("S-1-1-0");
    let mut sid: PSID = std::ptr::null_mut();
    unsafe {
      if ConvertStringSidToSidW(sid_string.as_ptr(), &mut sid) == 0 {
        return Err(GetLastError());
      }
    }
    Ok(sid)
  }

  /// Add a DENY ACE for write operations to a path.
  pub fn add_deny_ace(path: &Path) -> Result<(), ImmutableError> {
    let path_str = path.to_str().unwrap_or_default();
    let path_wide = to_wide(path_str);

    let everyone_sid = get_everyone_sid().map_err(|e| ImmutableError::Acl {
      path: path.display().to_string(),
      message: format!("failed to get Everyone SID: error {}", e),
    })?;

    unsafe {
      // 1. Get existing DACL
      let mut existing_dacl: *mut ACL = std::ptr::null_mut();
      let mut sd: *mut core::ffi::c_void = std::ptr::null_mut();

      let result = GetNamedSecurityInfoW(
        path_wide.as_ptr(),
        SE_FILE_OBJECT,
        DACL_SECURITY_INFORMATION,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        &mut existing_dacl,
        std::ptr::null_mut(),
        &mut sd,
      );

      if result != ERROR_SUCCESS {
        LocalFree(everyone_sid as _);
        return Err(ImmutableError::Acl {
          path: path.display().to_string(),
          message: format!("GetNamedSecurityInfoW failed: {}", result),
        });
      }

      // 2. Calculate new ACL size
      let mut acl_info: ACL_SIZE_INFORMATION = std::mem::zeroed();
      let existing_acl_valid = !existing_dacl.is_null() && IsValidAcl(existing_dacl) != 0;

      if existing_acl_valid {
        GetAclInformation(
          existing_dacl,
          &mut acl_info as *mut _ as *mut _,
          std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
          AclSizeInformation,
        );
      }

      let sid_length = GetLengthSid(everyone_sid);
      let new_ace_size = std::mem::size_of::<ACCESS_DENIED_ACE>() - std::mem::size_of::<u32>() + sid_length as usize;
      let new_acl_size = std::mem::size_of::<ACL>() + new_ace_size + acl_info.AclBytesInUse as usize;

      // Allocate buffer for new ACL
      let mut new_acl_buffer = vec![0u8; new_acl_size];
      let new_acl = new_acl_buffer.as_mut_ptr() as *mut ACL;

      // 3. Initialize new ACL
      if InitializeAcl(new_acl, new_acl_size as u32, ACL_REVISION) == 0 {
        LocalFree(sd as _);
        LocalFree(everyone_sid as _);
        return Err(ImmutableError::Acl {
          path: path.display().to_string(),
          message: format!("InitializeAcl failed: {}", GetLastError()),
        });
      }

      // 4. Add our deny ACE first (deny ACEs come before allow)
      let inherit_flags = CONTAINER_INHERIT_ACE | OBJECT_INHERIT_ACE;
      if AddAccessDeniedAceEx(new_acl, ACL_REVISION, inherit_flags, DENY_MASK, everyone_sid) == 0 {
        LocalFree(sd as _);
        LocalFree(everyone_sid as _);
        return Err(ImmutableError::Acl {
          path: path.display().to_string(),
          message: format!("AddAccessDeniedAceEx failed: {}", GetLastError()),
        });
      }

      // 5. Copy existing ACEs
      if existing_acl_valid {
        for i in 0..acl_info.AceCount {
          let mut ace: *mut core::ffi::c_void = std::ptr::null_mut();
          if GetAce(existing_dacl, i, &mut ace) != 0 {
            let ace_header = ace as *const ACE_HEADER;
            AddAce(
              new_acl,
              ACL_REVISION,
              u32::MAX, // MAXDWORD - append at end
              ace,
              (*ace_header).AceSize as u32,
            );
          }
        }
      }

      // 6. Apply new DACL
      let result = SetNamedSecurityInfoW(
        path_wide.as_ptr() as *mut _,
        SE_FILE_OBJECT,
        DACL_SECURITY_INFORMATION,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        new_acl,
        std::ptr::null_mut(),
      );

      LocalFree(sd as _);
      LocalFree(everyone_sid as _);

      if result != ERROR_SUCCESS {
        return Err(ImmutableError::Acl {
          path: path.display().to_string(),
          message: format!("SetNamedSecurityInfoW failed: {}", result),
        });
      }
    }

    Ok(())
  }

  /// Remove the DENY ACE for write operations from a path.
  pub fn remove_deny_ace(path: &Path) -> Result<(), ImmutableError> {
    let path_str = path.to_str().unwrap_or_default();
    let path_wide = to_wide(path_str);

    let everyone_sid = get_everyone_sid().map_err(|e| ImmutableError::Acl {
      path: path.display().to_string(),
      message: format!("failed to get Everyone SID: error {}", e),
    })?;

    unsafe {
      // 1. Get existing DACL
      let mut existing_dacl: *mut ACL = std::ptr::null_mut();
      let mut sd: *mut core::ffi::c_void = std::ptr::null_mut();

      let result = GetNamedSecurityInfoW(
        path_wide.as_ptr(),
        SE_FILE_OBJECT,
        DACL_SECURITY_INFORMATION,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        &mut existing_dacl,
        std::ptr::null_mut(),
        &mut sd,
      );

      if result != ERROR_SUCCESS {
        LocalFree(everyone_sid as _);
        return Err(ImmutableError::Acl {
          path: path.display().to_string(),
          message: format!("GetNamedSecurityInfoW failed: {}", result),
        });
      }

      if existing_dacl.is_null() || IsValidAcl(existing_dacl) == 0 {
        LocalFree(sd as _);
        LocalFree(everyone_sid as _);
        return Ok(());
      }

      // 2. Get ACL info
      let mut acl_info: ACL_SIZE_INFORMATION = std::mem::zeroed();
      GetAclInformation(
        existing_dacl,
        &mut acl_info as *mut _ as *mut _,
        std::mem::size_of::<ACL_SIZE_INFORMATION>() as u32,
        AclSizeInformation,
      );

      // 3. Create new ACL without our deny ACE
      let buffer_size = acl_info.AclBytesInUse as usize + 256;
      let mut new_acl_buffer = vec![0u8; buffer_size];
      let new_acl = new_acl_buffer.as_mut_ptr() as *mut ACL;

      if InitializeAcl(new_acl, buffer_size as u32, ACL_REVISION) == 0 {
        LocalFree(sd as _);
        LocalFree(everyone_sid as _);
        return Err(ImmutableError::Acl {
          path: path.display().to_string(),
          message: format!("InitializeAcl failed: {}", GetLastError()),
        });
      }

      // 4. Copy all ACEs except our deny ACE
      for i in 0..acl_info.AceCount {
        let mut ace: *mut core::ffi::c_void = std::ptr::null_mut();
        if GetAce(existing_dacl, i, &mut ace) != 0 {
          let ace_header = ace as *const ACE_HEADER;

          // Check if this is our deny ACE
          if (*ace_header).AceType == ACCESS_DENIED_ACE_TYPE {
            let deny_ace = ace as *const ACCESS_DENIED_ACE;

            // Get the SID from the ACE (starts at SidStart field)
            let ace_sid = std::ptr::addr_of!((*deny_ace).SidStart) as PSID;

            // Skip if this matches our SID and mask
            if windows_sys::Win32::Security::EqualSid(ace_sid, everyone_sid) != 0 && (*deny_ace).Mask == DENY_MASK {
              continue;
            }
          }

          AddAce(new_acl, ACL_REVISION, u32::MAX, ace, (*ace_header).AceSize as u32);
        }
      }

      // 5. Apply new DACL
      let result = SetNamedSecurityInfoW(
        path_wide.as_ptr() as *mut _,
        SE_FILE_OBJECT,
        DACL_SECURITY_INFORMATION,
        std::ptr::null_mut(),
        std::ptr::null_mut(),
        new_acl,
        std::ptr::null_mut(),
      );

      LocalFree(sd as _);
      LocalFree(everyone_sid as _);

      if result != ERROR_SUCCESS {
        return Err(ImmutableError::Acl {
          path: path.display().to_string(),
          message: format!("SetNamedSecurityInfoW failed: {}", result),
        });
      }
    }

    Ok(())
  }
}

#[cfg(windows)]
fn make_entry_immutable(path: &Path) -> Result<(), ImmutableError> {
  windows_acl::add_deny_ace(path)
}

#[cfg(windows)]
fn make_entry_mutable(path: &Path) -> Result<(), ImmutableError> {
  windows_acl::remove_deny_ace(path)
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
