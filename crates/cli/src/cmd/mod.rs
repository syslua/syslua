//! CLI command implementations.
//!
//! Each submodule implements a single CLI command:
//!
//! - [`apply`] - Evaluate config and apply changes to the system
//! - [`destroy`] - Remove all managed binds from the system
//! - [`diff`] - Show differences between snapshots
//! - [`info`] - Display information about builds, binds, or inputs
//! - [`init`] - Initialize a new syslua configuration
//! - [`plan`] - Show what changes would be made without applying
//! - [`status`] - Show current system state vs expected state
//! - [`update`] - Update input locks to latest versions

mod apply;
mod destroy;
mod diff;
mod gc;
mod info;
mod init;
mod plan;
mod status;
mod update;

pub use apply::cmd_apply;
pub use destroy::cmd_destroy;
pub use diff::cmd_diff;
pub use gc::cmd_gc;
pub use info::cmd_info;
pub use init::cmd_init;
pub use plan::cmd_plan;
pub use status::cmd_status;
pub use update::cmd_update;
