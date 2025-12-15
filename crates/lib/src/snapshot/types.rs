use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::manifest::Manifest;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snapshot {
  pub id: String,
  pub created_at: u64,
  pub config_path: Option<PathBuf>,
  pub manifest: Manifest,
}
