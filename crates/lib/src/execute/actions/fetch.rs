//! FetchUrl action implementation.
//!
//! This module handles downloading files from URLs with SHA256 verification.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use tracing::{debug, info};

use crate::execute::types::ExecuteError;

/// Execute a FetchUrl action.
///
/// Downloads the file from the given URL to a temporary location within `out_dir`,
/// verifies the SHA256 hash, and returns the path to the downloaded file.
///
/// # Arguments
///
/// * `url` - The URL to download from
/// * `expected_sha256` - The expected SHA256 hash (lowercase hex)
/// * `out_dir` - The output directory for the build (file is stored in `out_dir/downloads/`)
///
/// # Returns
///
/// The path to the downloaded file on success.
pub async fn execute_fetch(url: &str, expected_sha256: &str, out_dir: &Path) -> Result<PathBuf, ExecuteError> {
  info!(url = %url, "fetching URL");

  // Create downloads directory
  let downloads_dir = out_dir.join("downloads");
  fs::create_dir_all(&downloads_dir).await?;

  // Derive filename from URL
  let filename = url_to_filename(url);
  let dest_path = downloads_dir.join(&filename);

  // Check if file already exists with correct hash (cache hit)
  if dest_path.exists() {
    debug!(path = ?dest_path, "checking cached file");
    if let Ok(actual_hash) = hash_file(&dest_path).await {
      if actual_hash == expected_sha256 {
        info!(path = ?dest_path, "using cached file");
        return Ok(dest_path);
      }
      debug!(expected = %expected_sha256, actual = %actual_hash, "cached file hash mismatch, re-downloading");
    }
  }

  // Download the file
  let response = reqwest::get(url).await.map_err(|e| ExecuteError::FetchFailed {
    url: url.to_string(),
    message: e.to_string(),
  })?;

  if !response.status().is_success() {
    return Err(ExecuteError::FetchFailed {
      url: url.to_string(),
      message: format!("HTTP {}", response.status()),
    });
  }

  let bytes = response.bytes().await.map_err(|e| ExecuteError::FetchFailed {
    url: url.to_string(),
    message: e.to_string(),
  })?;

  // Compute hash while writing
  let actual_hash = {
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    hex::encode(hasher.finalize())
  };

  // Verify hash before writing
  if actual_hash != expected_sha256 {
    return Err(ExecuteError::HashMismatch {
      url: url.to_string(),
      expected: expected_sha256.to_string(),
      actual: actual_hash,
    });
  }

  // Write to file
  let mut file = fs::File::create(&dest_path).await?;
  file.write_all(&bytes).await?;
  file.flush().await?;

  info!(path = ?dest_path, size = bytes.len(), "download complete");

  Ok(dest_path)
}

/// Compute SHA256 hash of a file.
async fn hash_file(path: &Path) -> Result<String, std::io::Error> {
  let bytes = fs::read(path).await?;
  let mut hasher = Sha256::new();
  hasher.update(&bytes);
  Ok(hex::encode(hasher.finalize()))
}

/// Convert a URL to a safe filename.
///
/// Takes the last path component and sanitizes it. Falls back to hash of URL
/// if no suitable filename can be extracted.
fn url_to_filename(url: &str) -> String {
  // Try to extract filename from URL path
  if let Some(filename) = url.rsplit('/').next() {
    // Remove query string
    let filename = filename.split('?').next().unwrap_or(filename);

    // Sanitize: only allow alphanumeric, dash, underscore, dot
    let sanitized: String = filename
      .chars()
      .map(|c| {
        if c.is_alphanumeric() || c == '-' || c == '_' || c == '.' {
          c
        } else {
          '_'
        }
      })
      .collect();

    if !sanitized.is_empty() && sanitized != "." && sanitized != ".." {
      return sanitized;
    }
  }

  // Fallback: hash the URL
  let mut hasher = Sha256::new();
  hasher.update(url.as_bytes());
  format!("download_{}", &hex::encode(hasher.finalize())[..16])
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn url_to_filename_simple() {
    assert_eq!(url_to_filename("https://example.com/file.tar.gz"), "file.tar.gz");
  }

  #[test]
  fn url_to_filename_with_query() {
    assert_eq!(
      url_to_filename("https://example.com/file.tar.gz?token=abc"),
      "file.tar.gz"
    );
  }

  #[test]
  fn url_to_filename_sanitizes_special_chars() {
    assert_eq!(
      url_to_filename("https://example.com/file name.tar.gz"),
      "file_name.tar.gz"
    );
  }

  #[test]
  fn url_to_filename_fallback_for_empty() {
    let result = url_to_filename("https://example.com/");
    assert!(result.starts_with("download_"));
  }

  #[test]
  fn url_to_filename_version_in_path() {
    assert_eq!(
      url_to_filename("https://github.com/user/repo/releases/download/v1.0.0/app-linux-x64.tar.gz"),
      "app-linux-x64.tar.gz"
    );
  }

  // Integration tests that require network would go in a separate test module
  // with #[ignore] or behind a feature flag
}
