//! Hash computation for content addressing

use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use crate::Result;

/// Compute SHA256 hash of file contents and return as hex string
pub fn compute_hash(path: &Path) -> Result<String> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = Sha256::new();

    let mut buffer = [0u8; 8192];
    loop {
        let bytes_read = reader.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    let hash = hasher.finalize();
    Ok(hex::encode(hash))
}

/// Compute SHA256 hash of bytes and return as hex string
#[allow(dead_code)] // Will be used by derivation system
pub fn hash_bytes(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

/// Compute a short hash (first 12 characters) for use in paths
pub fn short_hash(full_hash: &str) -> &str {
    &full_hash[..12.min(full_hash.len())]
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_hash_bytes() {
        let hash = hash_bytes(b"hello world");
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_compute_hash() -> Result<()> {
        let mut file = NamedTempFile::new()?;
        file.write_all(b"hello world")?;
        file.flush()?;

        let hash = compute_hash(file.path())?;
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
        Ok(())
    }

    #[test]
    fn test_short_hash() {
        let full = "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9";
        assert_eq!(short_hash(full), "b94d27b9934d");
    }
}
