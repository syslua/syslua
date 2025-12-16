use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::consts::HASH_PREFIX_LEN;

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
  fn compute_hash(&self) -> Result<ObjectHash, serde_json::Error> {
    let serialized = serde_json::to_string(self)?;
    let mut hasher = Sha256::new();
    hasher.update(serialized.as_bytes());
    let full = format!("{:x}", hasher.finalize());
    Ok(ObjectHash(full[..HASH_PREFIX_LEN].to_string()))
  }
}
