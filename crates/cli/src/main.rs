//! sys-cli: Command-line interface for sys.lua
//!
//! Provides the `sys` command with subcommands:
//! - `sys apply <path>` - Apply a sys.lua configuration
//! - `sys plan <path>` - Show what would be applied (dry-run)
//! - `sys info` - Show system information

use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use console::style;
use std::path::{Path, PathBuf};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

use sys_core::Store;
use sys_lua::{ActivationAction, ActivationCtx, DerivationCtx, Manifest, Runtime};
use sys_platform::{PlatformInfo, SysluaPaths};

#[derive(Parser)]
#[command(name = "sys")]
#[command(author, version, about = "Declarative system configuration with Lua")]
struct Cli {
    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Apply a sys.lua configuration
    Apply {
        /// Path to the init.lua configuration file
        #[arg(default_value = "init.lua")]
        path: PathBuf,

        /// Show what would be done without making changes
        #[arg(long)]
        dry_run: bool,
    },

    /// Show what would be applied (same as apply --dry-run)
    Plan {
        /// Path to the init.lua configuration file
        #[arg(default_value = "init.lua")]
        path: PathBuf,
    },

    /// Show system information
    Info,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    // Set up logging
    let level = if cli.verbose {
        Level::DEBUG
    } else {
        Level::INFO
    };
    FmtSubscriber::builder()
        .with_max_level(level)
        .with_target(false)
        .without_time()
        .init();

    match cli.command {
        Commands::Apply { path, dry_run } => {
            if dry_run {
                cmd_plan(&path)
            } else {
                cmd_apply(&path)
            }
        }
        Commands::Plan { path } => cmd_plan(&path),
        Commands::Info => cmd_info(),
    }
}

/// Apply a sys.lua configuration
fn cmd_apply(path: &Path) -> Result<()> {
    println!("{} {}", style("Applying").green().bold(), path.display());

    // Evaluate the Lua configuration
    let runtime = Runtime::new().map_err(|e| anyhow!("Failed to create Lua runtime: {}", e))?;
    let manifest = runtime
        .evaluate_file(path)
        .map_err(|e| anyhow!("Failed to evaluate configuration: {}", e))?;

    if manifest.is_empty() {
        println!("{}", style("No changes to apply.").yellow());
        return Ok(());
    }

    // Show what we're going to do
    print_manifest_summary(&manifest);

    // Initialize the store
    let store = Store::new();
    store.init()?;

    let platform_info = PlatformInfo::current();

    // Process derivations by realizing them
    for deriv in &manifest.derivations {
        let version = deriv.version.as_deref().unwrap_or("latest");

        println!("  {} {} {}", style("+").green(), deriv.name, version);

        // Check if already in store
        if store.has_object(&deriv.name, version, &deriv.hash) {
            println!("    {} already in store", style("✓").green());
            continue;
        }

        // Create the output directory
        let obj_path = store.object_path(&deriv.name, version, &deriv.hash);
        std::fs::create_dir_all(&obj_path)?;

        // Create DerivationCtx with callbacks to sys_core functions
        let cache_dir = store.paths().downloads_cache();

        let ctx = DerivationCtx::new(
            obj_path.clone(),
            cache_dir.clone(),
            // fetch_url callback
            Box::new(|url, dest, sha256| {
                sys_core::fetch_url(url, dest, sha256).map_err(|e| e.to_string())
            }),
            // unpack_archive callback
            Box::new(|archive, dest| {
                sys_core::unpack_archive(archive, dest).map_err(|e| e.to_string())
            }),
        );

        // Realize the derivation by calling its config function
        println!("    {} building...", style("→").cyan());
        runtime
            .realize_derivation(deriv, ctx)
            .map_err(|e| anyhow!("Failed to realize '{}': {}", deriv.name, e))?;

        // Register the package (create symlink)
        store.register_package(&deriv.name, version, &deriv.hash, &platform_info.platform)?;
        println!("    {} installed", style("✓").green());
    }

    // Collect all activation actions
    let mut all_actions: Vec<ActivationAction> = Vec::new();

    // Process activations
    for activation in &manifest.activations {
        println!(
            "  {} running activation {}",
            style("→").blue(),
            &activation.hash[..8]
        );

        // Create ActivationCtx
        let ctx = ActivationCtx::new(store.paths().store.root.clone());

        // Realize the activation - calls the config function and collects actions
        let actions = runtime
            .realize_activation(activation, ctx)
            .map_err(|e| anyhow!("Failed to realize activation: {}", e))?;

        for action in &actions {
            match action {
                ActivationAction::AddToPath { bin_path } => {
                    println!("    {} PATH += {}", style("+").green(), bin_path);
                }
                ActivationAction::Symlink {
                    source,
                    target,
                    mutable,
                } => {
                    let kind = if *mutable { "mutable" } else { "symlink" };
                    println!(
                        "    {} {} {} -> {}",
                        style("→").cyan(),
                        kind,
                        source,
                        target
                    );
                }
                ActivationAction::SourceInShell { script, shells } => {
                    println!(
                        "    {} source {} ({})",
                        style("$").yellow(),
                        script,
                        shells.join(", ")
                    );
                }
                ActivationAction::Run { cmd } => {
                    println!("    {} run: {}", style("!").red(), cmd);
                }
            }
        }

        all_actions.extend(actions);
    }

    // Execute collected actions
    execute_actions(&all_actions, &store)?;

    // Generate environment script
    store.generate_env_script()?;

    println!();
    println!("{}", style("Apply complete!").green().bold());
    println!();
    println!("To activate the environment, run:");
    println!(
        "  {}",
        style(format!("source {}", store.env_script_path().display())).cyan()
    );

    Ok(())
}

/// Execute collected activation actions
fn execute_actions(actions: &[ActivationAction], store: &Store) -> Result<()> {
    for action in actions {
        match action {
            ActivationAction::AddToPath { bin_path } => {
                // PATH additions are handled by the env script generator
                // We just need to track them
                store.add_path_entry(bin_path)?;
            }
            ActivationAction::Symlink {
                source,
                target,
                mutable: _,
            } => {
                // Create the symlink
                let target_path = expand_tilde(target);
                if let Some(parent) = target_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                // Remove existing symlink if present
                if target_path.exists() || target_path.is_symlink() {
                    std::fs::remove_file(&target_path).ok();
                }
                #[cfg(unix)]
                std::os::unix::fs::symlink(source, &target_path)?;
            }
            ActivationAction::SourceInShell { script, shells: _ } => {
                // Source scripts are handled by the env script generator
                store.add_source_script(script)?;
            }
            ActivationAction::Run { cmd } => {
                // Execute the command
                println!("    {} executing: {}", style("→").cyan(), cmd);
                let status = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(cmd)
                    .status()?;
                if !status.success() {
                    return Err(anyhow!("Command failed: {}", cmd));
                }
            }
        }
    }
    Ok(())
}

/// Expand ~ to home directory
fn expand_tilde(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }
    PathBuf::from(path)
}

/// Show what would be applied (dry-run)
fn cmd_plan(path: &Path) -> Result<()> {
    println!("{} {}", style("Planning").blue().bold(), path.display());

    // Evaluate the Lua configuration
    let runtime = Runtime::new().map_err(|e| anyhow!("Failed to create Lua runtime: {}", e))?;
    let manifest = runtime
        .evaluate_file(path)
        .map_err(|e| anyhow!("Failed to evaluate configuration: {}", e))?;

    if manifest.is_empty() {
        println!("{}", style("No changes to apply.").yellow());
        return Ok(());
    }

    print_manifest_summary(&manifest);

    println!();
    println!("Run {} to apply these changes.", style("sys apply").cyan());

    Ok(())
}

/// Print a summary of the manifest
fn print_manifest_summary(manifest: &Manifest) {
    println!();
    println!("{}", style("Changes:").bold());

    for deriv in &manifest.derivations {
        println!(
            "  {} {} {}",
            style("+").green(),
            style(&deriv.name).green(),
            deriv.version.as_deref().unwrap_or("")
        );
    }

    for activation in &manifest.activations {
        println!(
            "  {} activation {}",
            style("→").blue(),
            &activation.hash[..8]
        );
    }

    println!();
}

/// Show system information
fn cmd_info() -> Result<()> {
    let platform_info = PlatformInfo::current();
    let paths = SysluaPaths::detect();

    println!("{}", style("System Information").bold());
    println!();
    println!("  Platform:  {}", platform_info.platform);
    println!("  OS:        {}", platform_info.platform.os);
    println!("  Arch:      {}", platform_info.platform.arch);
    println!("  Hostname:  {}", platform_info.hostname);
    println!("  Username:  {}", platform_info.username);
    println!();
    println!("{}", style("Paths").bold());
    println!();
    println!("  Store:     {}", paths.store.root.display());
    println!("  Cache:     {}", paths.cache.display());
    println!("  Config:    {}", paths.config.display());

    Ok(())
}
