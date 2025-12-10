//! sys.lua core functionality
//!
//! This crate provides:
//! - Store management (content-addressed storage)
//! - Derivation execution (fetch, build, hash)
//! - Manifest handling

mod error;
mod fetch;
mod hash;
mod store;

pub use error::{Error, Result};
pub use fetch::{fetch_url, unpack_archive};
pub use hash::compute_hash;
pub use store::Store;
