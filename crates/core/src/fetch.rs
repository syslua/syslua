//! URL fetching and archive extraction

use flate2::read::GzDecoder;
use sha2::{Digest, Sha256};
use std::fs::{self, File};
use std::io::{BufReader, Write};
use std::path::Path;
use tar::Archive;
use tracing::{debug, info};

use crate::{Error, Result};

/// Fetch a URL and save to the given path, verifying SHA256 hash
pub fn fetch_url(url: &str, dest: &Path, expected_sha256: Option<&str>) -> Result<()> {
    info!("Fetching {}", url);

    // Create parent directories if needed
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }

    // Use blocking reqwest for simplicity in PoC
    let response = reqwest::blocking::get(url)?;

    if !response.status().is_success() {
        return Err(Error::Http(
            response
                .error_for_status()
                .expect_err("should be error status"),
        ));
    }

    let bytes = response.bytes()?;

    // Verify hash if provided
    if let Some(expected) = expected_sha256 {
        let mut hasher = Sha256::new();
        hasher.update(&bytes);
        let actual = hex::encode(hasher.finalize());

        if actual != expected {
            return Err(Error::HashMismatch {
                expected: expected.to_string(),
                actual,
            });
        }
        debug!("Hash verified: {}", expected);
    }

    // Write to file
    let mut file = File::create(dest)?;
    file.write_all(&bytes)?;

    info!("Downloaded to {}", dest.display());
    Ok(())
}

/// Unpack an archive to the destination directory
///
/// Supports:
/// - `.tar.gz` / `.tgz`
/// - `.tar`
/// - `.zip`
pub fn unpack_archive(archive_path: &Path, dest: &Path) -> Result<()> {
    let path_str = archive_path
        .to_str()
        .ok_or_else(|| Error::Store("Invalid archive path".to_string()))?;

    fs::create_dir_all(dest)?;

    if path_str.ends_with(".tar.gz") || path_str.ends_with(".tgz") {
        unpack_tar_gz(archive_path, dest)?;
    } else if path_str.ends_with(".tar") {
        unpack_tar(archive_path, dest)?;
    } else if path_str.ends_with(".zip") {
        unpack_zip(archive_path, dest)?;
    } else {
        return Err(Error::UnsupportedArchive(path_str.to_string()));
    }

    info!("Unpacked to {}", dest.display());
    Ok(())
}

fn unpack_tar_gz(archive_path: &Path, dest: &Path) -> Result<()> {
    let file = File::open(archive_path)?;
    let decoder = GzDecoder::new(BufReader::new(file));
    let mut archive = Archive::new(decoder);

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;

        // Strip the first component (e.g., ripgrep-15.1.0-aarch64-apple-darwin/)
        let stripped: std::path::PathBuf = path.components().skip(1).collect();

        if stripped.as_os_str().is_empty() {
            continue;
        }

        let dest_path = dest.join(&stripped);

        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }

        entry.unpack(&dest_path)?;
    }

    Ok(())
}

fn unpack_tar(archive_path: &Path, dest: &Path) -> Result<()> {
    let file = File::open(archive_path)?;
    let mut archive = Archive::new(BufReader::new(file));

    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;

        // Strip the first component
        let stripped: std::path::PathBuf = path.components().skip(1).collect();

        if stripped.as_os_str().is_empty() {
            continue;
        }

        let dest_path = dest.join(&stripped);

        if let Some(parent) = dest_path.parent() {
            fs::create_dir_all(parent)?;
        }

        entry.unpack(&dest_path)?;
    }

    Ok(())
}

fn unpack_zip(archive_path: &Path, dest: &Path) -> Result<()> {
    let file = File::open(archive_path)?;
    let mut archive = zip::ZipArchive::new(BufReader::new(file))
        .map_err(|e| Error::Store(format!("Failed to open zip: {}", e)))?;

    for i in 0..archive.len() {
        let mut file = archive
            .by_index(i)
            .map_err(|e| Error::Store(format!("Failed to read zip entry: {}", e)))?;

        let path = file
            .enclosed_name()
            .ok_or_else(|| Error::Store("Invalid zip entry name".to_string()))?;

        // Strip the first component
        let stripped: std::path::PathBuf = path.components().skip(1).collect();

        if stripped.as_os_str().is_empty() {
            continue;
        }

        let dest_path = dest.join(&stripped);

        if file.is_dir() {
            fs::create_dir_all(&dest_path)?;
        } else {
            if let Some(parent) = dest_path.parent() {
                fs::create_dir_all(parent)?;
            }

            let mut outfile = File::create(&dest_path)?;
            std::io::copy(&mut file, &mut outfile)?;

            // Set executable bit on Unix if needed
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                if let Some(mode) = file.unix_mode() {
                    fs::set_permissions(&dest_path, fs::Permissions::from_mode(mode))?;
                }
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    // Integration tests would go here, but require network access
    // For unit tests, we'd mock the HTTP responses
}
