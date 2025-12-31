//! File-based store locking for mutual exclusion.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::platform::paths::store_dir;

const LOCK_FILENAME: &str = ".lock";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockMode {
  Shared,
  Exclusive,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct LockMetadata {
  pub version: u32,
  pub pid: u32,
  pub started_at_unix: u64,
  pub command: String,
  pub store: PathBuf,
}

#[derive(Debug, Error)]
pub enum StoreLockError {
  #[error(
    "Store is locked by another process: {command} (PID {pid}, started {started_at})\n\
             If you're sure no syslua process is running, remove the lock file:\n  {lock_path}"
  )]
  Contention {
    command: String,
    pid: u32,
    started_at: String,
    lock_path: PathBuf,
  },

  #[error(
    "Store is locked (could not read lock metadata)\n\
             If you're sure no syslua process is running, remove the lock file:\n  {lock_path}"
  )]
  ContentionUnknown { lock_path: PathBuf },

  #[error("Failed to create store directory: {0}")]
  CreateDir(#[source] io::Error),

  #[error("Failed to open lock file: {0}")]
  OpenFile(#[source] io::Error),

  #[error("Failed to write lock metadata: {0}")]
  WriteMetadata(#[source] io::Error),

  #[error("Failed to acquire lock: {0}")]
  LockFailed(#[source] io::Error),
}

pub struct StoreLock {
  _file: File,
  lock_path: PathBuf,
}

impl StoreLock {
  /// Reads the lock metadata from the held file handle.
  ///
  /// This is useful for tests and diagnostics where the caller already holds
  /// the lock and needs to verify metadata without opening a new file handle
  /// (which would fail on Windows due to mandatory locking).
  pub fn read_metadata(&self) -> io::Result<LockMetadata> {
    use std::io::{Seek, SeekFrom};

    let mut file = &self._file;
    file.seek(SeekFrom::Start(0))?;
    let mut contents = String::new();
    file.read_to_string(&mut contents)?;
    serde_json::from_str(&contents).map_err(io::Error::other)
  }

  pub fn acquire(mode: LockMode, command: &str) -> Result<Self, StoreLockError> {
    let store = store_dir();
    let lock_path = store.join(LOCK_FILENAME);

    if !store.exists() {
      std::fs::create_dir_all(&store).map_err(StoreLockError::CreateDir)?;
    }

    let file = OpenOptions::new()
      .read(true)
      .write(true)
      .create(true)
      .truncate(false)
      .open(&lock_path)
      .map_err(StoreLockError::OpenFile)?;

    if let Err(err) = try_lock(&file, mode) {
      if err.kind() == io::ErrorKind::WouldBlock {
        return Err(Self::read_contention_error(&lock_path));
      }
      return Err(StoreLockError::LockFailed(err));
    }

    if mode == LockMode::Exclusive {
      Self::write_metadata(&file, command, &store)?;
    }

    Ok(StoreLock { _file: file, lock_path })
  }

  fn write_metadata(file: &File, command: &str, store: &std::path::Path) -> Result<(), StoreLockError> {
    let metadata = LockMetadata {
      version: 1,
      pid: std::process::id(),
      started_at_unix: SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs(),
      command: command.to_string(),
      store: store.to_path_buf(),
    };

    file.set_len(0).map_err(StoreLockError::WriteMetadata)?;
    let mut writer = io::BufWriter::new(file);
    serde_json::to_writer_pretty(&mut writer, &metadata)
      .map_err(|e| StoreLockError::WriteMetadata(io::Error::other(e)))?;
    writer.flush().map_err(StoreLockError::WriteMetadata)?;

    Ok(())
  }

  fn read_contention_error(lock_path: &std::path::Path) -> StoreLockError {
    if let Ok(mut file) = File::open(lock_path) {
      let mut contents = String::new();
      if file.read_to_string(&mut contents).is_ok()
        && let Ok(metadata) = serde_json::from_str::<LockMetadata>(&contents)
      {
        let started_at = format!("Unix timestamp {}", metadata.started_at_unix);

        return StoreLockError::Contention {
          command: metadata.command,
          pid: metadata.pid,
          started_at,
          lock_path: lock_path.to_path_buf(),
        };
      }
    }

    StoreLockError::ContentionUnknown {
      lock_path: lock_path.to_path_buf(),
    }
  }

  pub fn lock_path(&self) -> &std::path::Path {
    &self.lock_path
  }
}

#[cfg(unix)]
fn try_lock(file: &File, mode: LockMode) -> io::Result<()> {
  use rustix::fs::{FlockOperation, flock};
  use std::os::unix::io::AsFd;

  let operation = match mode {
    LockMode::Shared => FlockOperation::NonBlockingLockShared,
    LockMode::Exclusive => FlockOperation::NonBlockingLockExclusive,
  };

  flock(file.as_fd(), operation).map_err(|e| io::Error::from_raw_os_error(e.raw_os_error()))
}

#[cfg(windows)]
fn try_lock(file: &File, mode: LockMode) -> io::Result<()> {
  use std::os::windows::io::AsRawHandle;
  use windows_sys::Win32::Foundation::HANDLE;
  use windows_sys::Win32::Storage::FileSystem::{LOCKFILE_EXCLUSIVE_LOCK, LOCKFILE_FAIL_IMMEDIATELY, LockFileEx};

  let handle = file.as_raw_handle() as HANDLE;
  let flags = match mode {
    LockMode::Shared => LOCKFILE_FAIL_IMMEDIATELY,
    LockMode::Exclusive => LOCKFILE_FAIL_IMMEDIATELY | LOCKFILE_EXCLUSIVE_LOCK,
  };

  // SAFETY: OVERLAPPED is a plain data struct that is valid when zero-initialized.
  // LockFileEx is safe to call with a valid file handle and zeroed OVERLAPPED.
  let result = unsafe {
    let mut overlapped = std::mem::zeroed();
    LockFileEx(handle, flags, 0, 1, 0, &mut overlapped)
  };

  if result == 0 {
    Err(io::Error::last_os_error())
  } else {
    Ok(())
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use serial_test::serial;
  use tempfile::TempDir;

  fn with_temp_store<F>(f: F)
  where
    F: FnOnce(),
  {
    let temp_dir = TempDir::new().unwrap();
    temp_env::with_var("SYSLUA_STORE", Some(temp_dir.path().to_str().unwrap()), f);
  }

  #[test]
  #[serial]
  fn acquire_exclusive_lock() {
    with_temp_store(|| {
      let lock = StoreLock::acquire(LockMode::Exclusive, "test").unwrap();
      assert!(lock.lock_path().exists());
    });
  }

  #[test]
  #[serial]
  fn acquire_shared_lock() {
    with_temp_store(|| {
      let lock = StoreLock::acquire(LockMode::Shared, "test").unwrap();
      assert!(lock.lock_path().exists());
    });
  }

  #[test]
  #[serial]
  fn multiple_shared_locks() {
    with_temp_store(|| {
      let lock1 = StoreLock::acquire(LockMode::Shared, "test1").unwrap();
      let lock2 = StoreLock::acquire(LockMode::Shared, "test2").unwrap();
      assert!(lock1.lock_path().exists());
      assert!(lock2.lock_path().exists());
    });
  }

  #[test]
  #[serial]
  fn lock_metadata_written() {
    with_temp_store(|| {
      let lock = StoreLock::acquire(LockMode::Exclusive, "my-command").unwrap();

      let metadata = lock.read_metadata().unwrap();

      assert_eq!(metadata.version, 1);
      assert_eq!(metadata.command, "my-command");
      assert_eq!(metadata.pid, std::process::id());
    });
  }

  #[test]
  #[serial]
  fn lock_released_on_drop() {
    with_temp_store(|| {
      {
        let _lock = StoreLock::acquire(LockMode::Exclusive, "test").unwrap();
      }

      let lock2 = StoreLock::acquire(LockMode::Exclusive, "test2").unwrap();
      assert!(lock2.lock_path().exists());
    });
  }
}
