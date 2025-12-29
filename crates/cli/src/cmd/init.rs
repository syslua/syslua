//! Implementation of the `sys init` command.
//!
//! This command initializes a new syslua configuration directory with
//! template files and sets up the store structure.

use std::path::Path;

use anyhow::{Context, Result};
use owo_colors::OwoColorize;

use syslua_lib::init::{InitOptions, init};
use syslua_lib::platform;

use crate::output::symbols;

/// Execute the init command.
///
/// Initializes a new syslua configuration directory at the given path with:
/// - `init.lua` entry point with examples
/// - `.luarc.json` for LuaLS IDE integration
/// - Store structure and type definitions
///
/// # Errors
///
/// Returns an error if files already exist or if there are permission issues.
pub fn cmd_init(path: &str) -> Result<()> {
  let config_path = Path::new(path);
  let system = platform::is_elevated();

  let options = InitOptions {
    config_path: config_path.to_path_buf(),
    system,
  };

  let result = init(&options).context("Failed to initialize configuration")?;

  println!(
    "{} {}",
    symbols::SUCCESS.green(),
    "Initialized syslua configuration!".green().bold()
  );
  println!();
  println!(
    "  {} Config directory: {}",
    symbols::INFO.cyan(),
    result.config_dir.display()
  );
  println!(
    "  {} Entry point:      {}",
    symbols::INFO.cyan(),
    result.init_lua.display()
  );
  println!(
    "  {} LuaLS config:     {}",
    symbols::INFO.cyan(),
    result.luarc_json.display()
  );
  println!(
    "  {} Type definitions: {}",
    symbols::INFO.cyan(),
    result.types_dir.display()
  );
  println!(
    "  {} Store:            {}",
    symbols::INFO.cyan(),
    result.store_dir.display()
  );
  println!();
  println!("{}", "Next steps:".bold());
  println!(
    "  1. Edit {} to configure your system",
    result.init_lua.display().to_string().cyan()
  );
  println!(
    "  2. Run: {}",
    format!("sys apply {}", result.config_dir.display()).cyan()
  );

  Ok(())
}
