//! Store operations for syslua.
//!
//! The store is the content-addressed storage for all build outputs and bind state.
//!
//! # Layout
//!
//! ```text
//! store/
//! ├── obj/                    # Build outputs (immutable, content-addressed)
//! │   └── <name>-<version>-<hash>/
//! └── bind/                   # Bind state (outputs for destroy)
//!     └── <hash>/
//!         └── state.json
//! ```

pub mod paths;
