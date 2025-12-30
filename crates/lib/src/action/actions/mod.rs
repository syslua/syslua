//! Individual action implementations.
//!
//! This module contains the concrete implementations for each action type:
//!
//! - [`exec`] - Shell command execution with environment and working directory support
//! - [`fetch_url`] - HTTP/HTTPS file download with SHA256 integrity verification

pub mod exec;
pub mod fetch_url;
