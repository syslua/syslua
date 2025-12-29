//! Hashing utilities for content-addressed storage and verification.
//!
//! This module provides:
//! - `ObjectHash`: A truncated 20-character hash for store paths
//! - `ContentHash`: A full 64-character hash for content verification
//! - `hash_directory()`: Deterministic directory hashing
//! - `hash_file()`: Single file hashing
//! - `hash_bytes()`: Arbitrary byte hashing

use std::fs;
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use walkdir::WalkDir;

use crate::consts::OBJ_HASH_PREFIX_LEN;

pub type HashError = serde_json::Error;

/// A content-addressed hash identifying a unique object.
///
/// The hash is a 20-character truncated SHA-256 of the JSON-serialized struct.
/// This provides sufficient collision resistance while keeping paths readable.
///
/// # Format
///
/// The hash is a lowercase hexadecimal string, e.g., `"a1b2c3d4e5f6789012ab"`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ObjectHash(pub String);

impl std::fmt::Display for ObjectHash {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.0)
  }
}

pub trait Hashable: Serialize {
  fn compute_hash(&self) -> Result<ObjectHash, HashError> {
    let serialized = serde_json::to_string(self)?;
    let mut hasher = Sha256::new();
    hasher.update(serialized.as_bytes());
    let full = format!("{:x}", hasher.finalize());
    Ok(ObjectHash(full[..OBJ_HASH_PREFIX_LEN].to_string()))
  }
}

/// A full 64-character SHA256 hash for content verification.
///
/// Unlike `ObjectHash` which is truncated for store paths, `ContentHash`
/// provides the full hash for maximum collision resistance when verifying
/// build outputs.
///
/// # Format
///
/// The hash is a lowercase hexadecimal string (64 characters).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentHash(pub String);

impl std::fmt::Display for ContentHash {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.0)
  }
}

/// Error during directory hashing.
#[derive(Debug, thiserror::Error, serde::Serialize, serde::Deserialize)]
pub enum DirHashError {
  #[error("failed to walk directory: {message}")]
  WalkDir { message: String },

  #[error("failed to read file {path}: {message}")]
  ReadFile {
    path: String,
    message: String,
  },

  #[error("failed to read symlink {path}: {message}")]
  ReadSymlink {
    path: String,
    message: String,
  },
}

/// Compute a deterministic hash of a directory's contents.
///
/// The hash includes:
/// - File contents (not metadata like timestamps or permissions)
/// - Directory structure
/// - Symlink targets
///
/// Entries are sorted by path for determinism.
///
/// # Arguments
///
/// * `path` - The directory to hash
/// * `exclude` - List of file/directory names to skip (e.g., `&[".syslua-complete", "tmp"]`)
///
/// # Returns
///
/// A full 64-character SHA256 hash of the directory contents.
///
/// # Example
///
/// ```ignore
/// let hash = hash_directory(&store_path, &[".syslua-complete", "tmp"])?;
/// ```
pub fn hash_directory(path: &Path, exclude: &[&str]) -> Result<ContentHash, DirHashError> {
  let mut entries: Vec<(String, String)> = Vec::new();

  let walker = WalkDir::new(path).sort_by_file_name().into_iter().filter_entry(|e| {
    // Filter out excluded entries
    e.file_name()
      .to_str()
      .map(|name| !exclude.contains(&name))
      .unwrap_or(true)
  });

  for entry in walker {
    let entry = entry.map_err(|e| DirHashError::WalkDir { message: e.to_string() })?;
    let entry_path = entry.path();

    // Get path relative to root
    let rel_path = entry_path
      .strip_prefix(path)
      .unwrap_or(entry_path)
      .to_string_lossy()
      .to_string();

    // Skip the root directory itself
    if rel_path.is_empty() {
      continue;
    }

    let file_type = entry.file_type();
    let entry_hash = if file_type.is_file() {
      let content_hash = hash_file(entry_path)?;
      format!("F:{}:{}", rel_path, content_hash.0)
    } else if file_type.is_dir() {
      format!("D:{}", rel_path)
    } else if file_type.is_symlink() {
      let target = fs::read_link(entry_path).map_err(|e| DirHashError::ReadSymlink {
        path: entry_path.display().to_string(),
        message: e.to_string(),
      })?;
      let target_hash = hash_bytes(target.to_string_lossy().as_bytes());
      format!("L:{}:{}", rel_path, target_hash.0)
    } else {
      // Skip special files (sockets, devices, etc.)
      continue;
    };

    entries.push((rel_path, entry_hash));
  }

  // Sort by path for determinism (WalkDir sorts, but be explicit)
  entries.sort_by(|a, b| a.0.cmp(&b.0));

  // Hash the collected entries
  let mut hasher = Sha256::new();
  for (_, entry_hash) in entries {
    hasher.update(entry_hash.as_bytes());
    hasher.update(b"\n");
  }

  Ok(ContentHash(format!("{:x}", hasher.finalize())))
}

/// Hash a file's contents.
///
/// Returns the full 64-character SHA256 hash of the file.
pub fn hash_file(path: &Path) -> Result<ContentHash, DirHashError> {
  let mut file = fs::File::open(path).map_err(|e| DirHashError::ReadFile {
    path: path.display().to_string(),
    message: e.to_string(),
  })?;

  let mut hasher = Sha256::new();
  let mut buffer = [0u8; 8192];

  loop {
    let bytes_read = file.read(&mut buffer).map_err(|e| DirHashError::ReadFile {
      path: path.display().to_string(),
      message: e.to_string(),
    })?;
    if bytes_read == 0 {
      break;
    }
    hasher.update(&buffer[..bytes_read]);
  }

  Ok(ContentHash(format!("{:x}", hasher.finalize())))
}

/// Hash arbitrary bytes.
///
/// Returns the full 64-character SHA256 hash.
pub fn hash_bytes(data: &[u8]) -> ContentHash {
  let mut hasher = Sha256::new();
  hasher.update(data);
  ContentHash(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
  use super::*;
  use std::fs;
  use std::path::Path;
  use tempfile::tempdir;

  /// Cross-platform symlink creation helper
  fn create_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
      std::os::unix::fs::symlink(target, link)
    }
    #[cfg(windows)]
    {
      if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
      } else {
        std::os::windows::fs::symlink_file(target, link)
      }
    }
  }

  #[test]
  fn hash_empty_directory() {
    let temp = tempdir().unwrap();
    let hash = hash_directory(temp.path(), &[]).unwrap();
    assert_eq!(hash.0.len(), 64);
  }

  #[test]
  fn hash_is_deterministic() {
    let temp = tempdir().unwrap();
    fs::write(temp.path().join("a.txt"), "content a").unwrap();
    fs::write(temp.path().join("b.txt"), "content b").unwrap();

    let hash1 = hash_directory(temp.path(), &[]).unwrap();
    let hash2 = hash_directory(temp.path(), &[]).unwrap();

    assert_eq!(hash1, hash2);
  }

  #[test]
  fn hash_changes_with_content() {
    let temp = tempdir().unwrap();
    fs::write(temp.path().join("file.txt"), "original").unwrap();
    let hash1 = hash_directory(temp.path(), &[]).unwrap();

    fs::write(temp.path().join("file.txt"), "modified").unwrap();
    let hash2 = hash_directory(temp.path(), &[]).unwrap();

    assert_ne!(hash1, hash2);
  }

  #[test]
  fn hash_changes_with_new_file() {
    let temp = tempdir().unwrap();
    fs::write(temp.path().join("file.txt"), "content").unwrap();
    let hash1 = hash_directory(temp.path(), &[]).unwrap();

    fs::write(temp.path().join("file2.txt"), "more").unwrap();
    let hash2 = hash_directory(temp.path(), &[]).unwrap();

    assert_ne!(hash1, hash2);
  }

  #[test]
  fn hash_includes_subdirectories() {
    let temp = tempdir().unwrap();
    fs::create_dir(temp.path().join("subdir")).unwrap();
    fs::write(temp.path().join("subdir/file.txt"), "nested").unwrap();

    let hash = hash_directory(temp.path(), &[]).unwrap();
    assert_eq!(hash.0.len(), 64);
  }

  #[test]
  fn hash_includes_symlinks() {
    let temp = tempdir().unwrap();
    let file = temp.path().join("target.txt");
    fs::write(&file, "target content").unwrap();
    create_symlink(&file, &temp.path().join("link")).unwrap();

    let hash = hash_directory(temp.path(), &[]).unwrap();
    assert_eq!(hash.0.len(), 64);
  }

  #[test]
  fn hash_respects_exclusions() {
    let temp = tempdir().unwrap();
    fs::write(temp.path().join("file.txt"), "content").unwrap();
    let hash1 = hash_directory(temp.path(), &[]).unwrap();

    // Add files that should be excluded
    fs::write(temp.path().join(".syslua-complete"), "marker").unwrap();
    fs::create_dir(temp.path().join("tmp")).unwrap();
    fs::write(temp.path().join("tmp/temp-file"), "temp").unwrap();

    let hash2 = hash_directory(temp.path(), &[".syslua-complete", "tmp"]).unwrap();

    // Hash should be the same - excluded items don't affect it
    assert_eq!(hash1, hash2);
  }

  #[test]
  fn same_content_different_structure_different_hash() {
    let temp1 = tempdir().unwrap();
    fs::write(temp1.path().join("file.txt"), "content").unwrap();

    let temp2 = tempdir().unwrap();
    fs::create_dir(temp2.path().join("subdir")).unwrap();
    fs::write(temp2.path().join("subdir/file.txt"), "content").unwrap();

    let hash1 = hash_directory(temp1.path(), &[]).unwrap();
    let hash2 = hash_directory(temp2.path(), &[]).unwrap();

    assert_ne!(hash1, hash2);
  }

  #[test]
  fn hash_file_works() {
    let temp = tempdir().unwrap();
    let file_path = temp.path().join("test.txt");
    fs::write(&file_path, "hello world").unwrap();

    let hash = hash_file(&file_path).unwrap();
    assert_eq!(hash.0.len(), 64);

    // Same content = same hash
    let hash2 = hash_file(&file_path).unwrap();
    assert_eq!(hash, hash2);
  }
}
