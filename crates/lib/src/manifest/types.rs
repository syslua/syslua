//! Manifest types for syslua.
//!
//! The manifest is the central data structure that captures the complete desired
//! state of a system. It's produced by evaluating Lua configuration and contains
//! all builds and bindings to be applied.
//!
//! # Structure
//!
//! The manifest contains:
//! - `builds`: Content-addressed map of [`BuildDef`]s, keyed by [`BuildHash`]
//! - `bindings`: Content-addressed map of [`BindDef`]s, keyed by [`BindHash`]
//!
//! # Content Addressing
//!
//! Using hashes as keys provides automatic deduplication: if two different parts
//! of the configuration define identical builds, they're stored only once.
//!
//! # Serialization
//!
//! The manifest is fully serializable and can be:
//! - Stored in snapshots for state tracking
//! - Diffed against previous manifests to compute changes
//! - Hashed for quick equality checks

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::bind::BindDef;
use crate::build::BuildDef;
use crate::util::hash::{Hashable, ObjectHash};

/// The complete desired state manifest.
///
/// This struct represents the evaluated system configuration, containing all
/// builds and bindings that should be applied.
///
/// # Content Addressing
///
/// Both maps use content-addressed hashes as keys:
/// - Enables automatic deduplication of identical definitions
/// - Makes equality checking efficient (just compare hashes)
/// - Supports incremental updates by diffing manifests
///
/// # Ordering
///
/// Uses [`BTreeMap`] to ensure deterministic serialization order, which is
/// important for reproducible manifest hashes.
///
/// # Example
///
/// ```json
/// {
///   "builds": {
///     "a1b2c3d4e5f6789012ab": { "name": "ripgrep", ... },
///     "b2c3d4e5f6789012abc1": { "name": "fd", ... }
///   },
///   "bindings": {
///     "c3d4e5f6789012abc1d2": { "apply_actions": [...], ... }
///   }
/// }
/// ```
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
  /// All builds in the manifest, keyed by their content hash.
  pub builds: BTreeMap<ObjectHash, BuildDef>,
  /// All bindings in the manifest, keyed by their content hash.
  pub bindings: BTreeMap<ObjectHash, BindDef>,
}

impl Hashable for Manifest {}
