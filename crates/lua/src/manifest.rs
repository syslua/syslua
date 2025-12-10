//! Manifest - the resolved configuration after Lua evaluation
//!
//! The manifest contains only the core primitives:
//! - Derivations (build recipes)
//! - Activations (making derivations visible)
//!
//! Note: file{}, env{}, user{} are Lua helpers that create
//! derivations/activations - they don't have separate manifest entries.

use serde::{Deserialize, Serialize};

use crate::globals::{Activation, Derivation};

/// The complete manifest produced by evaluating a Lua configuration
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Manifest {
    /// All derivations (package builds)
    pub derivations: Vec<Derivation>,

    /// All activations (making derivations visible)
    pub activations: Vec<Activation>,
}

impl Manifest {
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if the manifest has any changes to apply
    pub fn is_empty(&self) -> bool {
        self.derivations.is_empty() && self.activations.is_empty()
    }

    /// Get a summary of what's in the manifest
    pub fn summary(&self) -> ManifestSummary {
        ManifestSummary {
            derivation_count: self.derivations.len(),
            activation_count: self.activations.len(),
        }
    }
}

/// Summary statistics for a manifest
#[derive(Debug)]
pub struct ManifestSummary {
    pub derivation_count: usize,
    pub activation_count: usize,
}

impl std::fmt::Display for ManifestSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} derivations, {} activations",
            self.derivation_count, self.activation_count
        )
    }
}
