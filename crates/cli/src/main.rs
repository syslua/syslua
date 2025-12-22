mod cmd;

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use cmd::{cmd_apply, cmd_destroy, cmd_info, cmd_init, cmd_plan, cmd_update};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

#[derive(Parser)]
#[command(name = "syslua", author, version, about, long_about = None)]
struct Cli {
  #[arg(short, long, global = true)]
  verbose: bool,

  #[command(subcommand)]
  command: Commands,
}

#[derive(Subcommand)]
enum Commands {
  /// Initialize a new syslua configuration directory
  Init {
    /// Path to the configuration directory
    path: String,
  },
  /// Evaluate a config and apply changes to the system
  Apply { file: String },
  /// Evaluate a config and create a plan without applying
  Plan { file: String },
  /// Remove all binds from the current snapshot
  Destroy {
    /// Show what would be destroyed without making changes
    #[arg(long)]
    dry_run: bool,
  },
  /// Update inputs by re-resolving to latest revisions
  Update {
    /// Path to config file (default: ./init.lua or ~/.config/syslua/init.lua)
    #[arg(value_name = "CONFIG")]
    config: Option<String>,

    /// Update only specific input(s) (can be repeated)
    #[arg(short, long = "input", value_name = "NAME")]
    inputs: Vec<String>,

    /// Show what would change without making changes
    #[arg(long)]
    dry_run: bool,
  },
  /// Display system information
  Info,
}

fn main() -> ExitCode {
  let cli = Cli::parse();

  let level = if cli.verbose { Level::DEBUG } else { Level::INFO };

  FmtSubscriber::builder()
    .with_max_level(level)
    .with_target(false)
    .without_time()
    .init();

  let result = match cli.command {
    Commands::Init { path } => cmd_init(&path),
    Commands::Apply { file } => cmd_apply(&file),
    Commands::Plan { file } => cmd_plan(&file),
    Commands::Destroy { dry_run } => cmd_destroy(dry_run),
    Commands::Update {
      config,
      inputs,
      dry_run,
    } => cmd_update(config.as_deref(), inputs, dry_run),
    Commands::Info => {
      cmd_info();
      Ok(())
    }
  };

  match result {
    Ok(()) => ExitCode::SUCCESS,
    Err(err) => {
      eprintln!("Error: {err:?}");
      ExitCode::FAILURE
    }
  }
}
