use anyhow::Result;
use clap::{Parser, Subcommand};
use console::{Term, style};
use std::path::{Path, PathBuf};
use sys_core::{
    ApplyOptions, FileChangeKind, Manifest, Plan, Shell, apply, compute_plan, generate_env_script,
    source_command, write_env_scripts,
};
use sys_platform::Platform;
use tracing_subscriber::EnvFilter;

// Helper to convert CoreError to anyhow::Error (works around mlua not being Send+Sync)
fn map_core_err<T>(result: sys_core::Result<T>) -> Result<T> {
    result.map_err(|e| anyhow::anyhow!("{}", e))
}

/// sys.lua - Declarative system/environment manager
#[derive(Parser)]
#[command(name = "sys")]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Apply a configuration file
    Apply {
        /// Path to the configuration file (default: init.lua)
        #[arg(default_value = "init.lua")]
        config: PathBuf,

        /// Force overwrite of existing files
        #[arg(short, long)]
        force: bool,
    },

    /// Show what changes would be made (dry-run)
    Plan {
        /// Path to the configuration file (default: init.lua)
        #[arg(default_value = "init.lua")]
        config: PathBuf,
    },

    /// Print shell environment activation command
    Env {
        /// Path to the configuration file (default: init.lua)
        #[arg(default_value = "init.lua")]
        config: PathBuf,

        /// Shell to generate script for (auto-detected if not specified)
        #[arg(short, long)]
        shell: Option<String>,

        /// Print the script content instead of source command
        #[arg(long)]
        print: bool,
    },

    /// Show current status
    Status,
}

fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .without_time()
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Apply { config, force } => cmd_apply(&config, force, cli.verbose),
        Commands::Plan { config } => cmd_plan(&config, cli.verbose),
        Commands::Env {
            config,
            shell,
            print,
        } => cmd_env(&config, shell, print),
        Commands::Status => cmd_status(),
    }
}

fn cmd_apply(config: &Path, force: bool, verbose: bool) -> Result<()> {
    let term = Term::stderr();

    // Check config exists
    if !config.exists() {
        term.write_line(&format!(
            "{} Config file not found: {}",
            style("error:").red().bold(),
            config.display()
        ))?;
        std::process::exit(1);
    }

    term.write_line(&format!(
        "{} Evaluating {}",
        style("::").cyan().bold(),
        config.display()
    ))?;

    // Load manifest
    let manifest = match Manifest::from_config(config) {
        Ok(m) => m,
        Err(e) => {
            term.write_line(&format!(
                "{} Failed to evaluate config: {}",
                style("error:").red().bold(),
                e
            ))?;
            std::process::exit(1);
        }
    };

    // Compute plan
    let plan = map_core_err(compute_plan(&manifest))?;

    if !plan.has_changes() {
        term.write_line(&format!(
            "{} No changes to apply",
            style("::").cyan().bold()
        ))?;
        return Ok(());
    }

    // Show plan
    print_plan(&term, &plan, verbose)?;

    term.write_line("")?;
    term.write_line(&format!(
        "{} Applying {} change(s)",
        style("::").cyan().bold(),
        plan.change_count()
    ))?;

    // Apply
    let options = ApplyOptions {
        force,
        dry_run: false,
    };

    map_core_err(apply(&plan, &options))?;

    term.write_line(&format!("{} Done!", style("::").green().bold()))?;

    Ok(())
}

fn cmd_plan(config: &Path, verbose: bool) -> Result<()> {
    let term = Term::stderr();

    // Check config exists
    if !config.exists() {
        term.write_line(&format!(
            "{} Config file not found: {}",
            style("error:").red().bold(),
            config.display()
        ))?;
        std::process::exit(1);
    }

    term.write_line(&format!(
        "{} Evaluating {}",
        style("::").cyan().bold(),
        config.display()
    ))?;

    // Load manifest
    let manifest = match Manifest::from_config(config) {
        Ok(m) => m,
        Err(e) => {
            term.write_line(&format!(
                "{} Failed to evaluate config: {}",
                style("error:").red().bold(),
                e
            ))?;
            std::process::exit(1);
        }
    };

    // Compute plan
    let plan = map_core_err(compute_plan(&manifest))?;

    if !plan.has_changes() {
        term.write_line(&format!(
            "{} No changes would be made",
            style("::").cyan().bold()
        ))?;
        return Ok(());
    }

    term.write_line("")?;
    print_plan(&term, &plan, verbose)?;

    term.write_line("")?;
    term.write_line(&format!(
        "{} Would apply {} change(s)",
        style("::").cyan().bold(),
        plan.change_count()
    ))?;

    Ok(())
}

fn cmd_status() -> Result<()> {
    let term = Term::stderr();
    let platform = sys_platform::Platform::detect()?;

    term.write_line(&format!(
        "{} sys.lua v{}",
        style("::").cyan().bold(),
        env!("CARGO_PKG_VERSION")
    ))?;
    term.write_line("")?;
    term.write_line(&format!("  Platform: {}", platform.platform))?;
    term.write_line(&format!("  OS:       {}", platform.os.as_str()))?;
    term.write_line(&format!("  Arch:     {}", platform.arch.as_str()))?;
    term.write_line(&format!("  User:     {}", platform.username))?;
    term.write_line(&format!("  Hostname: {}", platform.hostname))?;
    term.write_line(&format!("  Home:     {}", platform.home_dir.display()))?;

    Ok(())
}

fn cmd_env(config: &Path, shell_name: Option<String>, print: bool) -> Result<()> {
    let term = Term::stderr();
    let platform = Platform::detect()?;

    // Check config exists
    if !config.exists() {
        term.write_line(&format!(
            "{} Config file not found: {}",
            style("error:").red().bold(),
            config.display()
        ))?;
        std::process::exit(1);
    }

    // Determine shell
    let shell = match shell_name {
        Some(name) => match name.to_lowercase().as_str() {
            "bash" => Shell::Bash,
            "zsh" => Shell::Zsh,
            "fish" => Shell::Fish,
            "sh" => Shell::Sh,
            "powershell" | "pwsh" => Shell::PowerShell,
            _ => {
                term.write_line(&format!(
                    "{} Unknown shell: {}. Supported: bash, zsh, fish, sh, powershell",
                    style("error:").red().bold(),
                    name
                ))?;
                std::process::exit(1);
            }
        },
        None => Shell::detect(),
    };

    // Load manifest
    let manifest = match Manifest::from_config(config) {
        Ok(m) => m,
        Err(e) => {
            term.write_line(&format!(
                "{} Failed to evaluate config: {}",
                style("error:").red().bold(),
                e
            ))?;
            std::process::exit(1);
        }
    };

    if manifest.envs.is_empty() {
        term.write_line(&format!(
            "{} No environment variables declared in config",
            style("::").cyan().bold()
        ))?;
        return Ok(());
    }

    if print {
        // Print the script content
        let script = generate_env_script(&manifest, &shell);
        println!("{}", script);
    } else {
        // Write env scripts and print source command
        let env_dir = platform.env_script_dir();

        term.write_line(&format!(
            "{} Writing environment scripts to {}",
            style("::").cyan().bold(),
            env_dir.display()
        ))?;

        map_core_err(write_env_scripts(&manifest, &env_dir))?;

        // Print info about what was written
        term.write_line(&format!(
            "{} Generated scripts for {} env var(s)",
            style("::").green().bold(),
            manifest.envs.len()
        ))?;

        term.write_line("")?;
        term.write_line(&format!(
            "Add this to your shell config (~/.{}rc):",
            shell.as_str()
        ))?;
        term.write_line("")?;

        let cmd = source_command(&shell, &env_dir);
        println!("  {}", style(&cmd).cyan());

        term.write_line("")?;
        term.write_line("Or run it directly in the current shell:")?;
        term.write_line("")?;
        println!("  eval \"$(sys env --print)\"");
    }

    Ok(())
}

fn print_plan(term: &Term, plan: &Plan, verbose: bool) -> Result<()> {
    for change in plan.changes() {
        let symbol = match &change.kind {
            FileChangeKind::CreateSymlink { .. } | FileChangeKind::CreateContent { .. } => {
                style("+").green().bold()
            }
            FileChangeKind::UpdateSymlink { .. } | FileChangeKind::UpdateContent { .. } => {
                style("~").yellow().bold()
            }
            FileChangeKind::CopyFile { .. } => style("+").green().bold(),
            FileChangeKind::Unchanged => style(" ").dim(),
        };

        let description = change.description();

        term.write_line(&format!(
            "  {} {} {}",
            symbol,
            change.path.display(),
            style(format!("({})", description)).dim()
        ))?;

        // Show details in verbose mode
        if verbose {
            match &change.kind {
                FileChangeKind::CreateContent { content } => {
                    for line in content.lines().take(5) {
                        term.write_line(&format!("      {}", style(line).dim()))?;
                    }
                    let line_count = content.lines().count();
                    if line_count > 5 {
                        term.write_line(&format!(
                            "      {}",
                            style(format!("... ({} more lines)", line_count - 5)).dim()
                        ))?;
                    }
                }
                FileChangeKind::UpdateContent {
                    old_content,
                    new_content,
                } => {
                    term.write_line(&format!("      {}", style("--- old").red()))?;
                    for line in old_content.lines().take(3) {
                        term.write_line(&format!("      {}", style(format!("- {}", line)).red()))?;
                    }
                    term.write_line(&format!("      {}", style("+++ new").green()))?;
                    for line in new_content.lines().take(3) {
                        term.write_line(&format!(
                            "      {}",
                            style(format!("+ {}", line)).green()
                        ))?;
                    }
                }
                _ => {}
            }
        }
    }

    Ok(())
}
