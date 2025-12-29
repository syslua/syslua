mod cmd;
mod output;

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use cmd::{cmd_apply, cmd_destroy, cmd_diff, cmd_info, cmd_init, cmd_plan, cmd_status, cmd_update};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum ColorChoice {
  #[default]
  Auto,
  Always,
  Never,
}

#[derive(Parser)]
#[command(name = "syslua", author, version, about, long_about = None)]
struct Cli {
  /// Enable debug logging (DEBUG level instead of INFO)
  #[arg(short = 'd', long, global = true)]
  debug: bool,

  /// Control colored output
  #[arg(long, value_enum, default_value = "auto", global = true)]
  color: ColorChoice,

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
  Apply {
    file: String,
    /// Check unchanged binds for drift and repair if needed
    #[arg(long)]
    repair: bool,
    /// Output JSON instead of human-readable text
    #[arg(long)]
    json: bool,
  },
  /// Evaluate a config and create a plan without applying
  Plan {
    file: String,
    /// Output JSON instead of human-readable text
    #[arg(long)]
    json: bool,
  },
  /// Remove all binds from the current snapshot
  Destroy {
    /// Show what would be destroyed without making changes
    #[arg(long)]
    dry_run: bool,
    /// Output JSON instead of human-readable text
    #[arg(long)]
    json: bool,
  },
  /// Compare two snapshots and show differences
  Diff {
    /// First snapshot ID (defaults to previous if not specified)
    #[arg(value_name = "SNAPSHOT_A")]
    snapshot_a: Option<String>,

    /// Second snapshot ID (defaults to current if not specified)
    #[arg(value_name = "SNAPSHOT_B")]
    snapshot_b: Option<String>,

    /// Show detailed changes with actions
    /// Output JSON instead of human-readable text
    #[arg(long)]
    json: bool,
    #[arg(short, long)]
    verbose: bool,
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
  /// Show current system state
  Status {
    /// Show all builds and binds
    #[arg(short, long)]
    verbose: bool,
    /// Output JSON instead of human-readable text
    #[arg(long)]
    json: bool,
  },
}

fn main() -> ExitCode {
  let cli = Cli::parse();

  match cli.color {
    ColorChoice::Always => owo_colors::set_override(true),
    ColorChoice::Never => owo_colors::set_override(false),
    ColorChoice::Auto => {}
  }

  let level = if cli.debug { Level::DEBUG } else { Level::INFO };

  let subscriber = FmtSubscriber::builder().with_max_level(level).with_target(false);

  if cli.debug {
    subscriber.init();
  } else {
    subscriber.without_time().init();
  }

  let result = match cli.command {
    Commands::Init { path } => cmd_init(&path),
    Commands::Apply { file, repair, json } => cmd_apply(&file, repair, json),
    Commands::Plan { file, json } => cmd_plan(&file, json),
    Commands::Destroy { dry_run, json } => cmd_destroy(dry_run, json),
    Commands::Diff {
      snapshot_a,
      snapshot_b,
      verbose,
      json,
    } => cmd_diff(snapshot_a, snapshot_b, verbose, json),
    Commands::Update {
      config,
      inputs,
      dry_run,
    } => cmd_update(config.as_deref(), inputs, dry_run),
    Commands::Info => {
      cmd_info();
      Ok(())
    }
    Commands::Status { verbose, json } => cmd_status(verbose, json),
  };

  match result {
    Ok(()) => ExitCode::SUCCESS,
    Err(err) => {
      eprintln!("Error: {err:?}");
      ExitCode::FAILURE
    }
  }
}
