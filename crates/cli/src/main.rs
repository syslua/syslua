mod cmd;
mod output;

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use cmd::{cmd_apply, cmd_destroy, cmd_diff, cmd_gc, cmd_info, cmd_init, cmd_plan, cmd_status, cmd_update};
use output::OutputFormat;
use tracing::Level;
use tracing_subscriber::{Layer, fmt, layer::SubscriberExt, util::SubscriberInitExt};

/// Log verbosity level
#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum LogLevel {
  /// Show only errors
  Error,
  /// Show warnings and errors
  Warn,
  /// Show informational messages (default)
  #[default]
  Info,
  /// Show debug messages
  Debug,
  /// Show all messages including trace
  Trace,
}

impl From<LogLevel> for Level {
  fn from(level: LogLevel) -> Self {
    match level {
      LogLevel::Error => Level::ERROR,
      LogLevel::Warn => Level::WARN,
      LogLevel::Info => Level::INFO,
      LogLevel::Debug => Level::DEBUG,
      LogLevel::Trace => Level::TRACE,
    }
  }
}

/// Log output format
#[derive(Debug, Clone, Copy, Default, clap::ValueEnum)]
pub enum LogFormat {
  /// Human-readable format (default)
  #[default]
  Pretty,
  /// JSON format for structured logging
  Json,
}

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
  /// Log verbosity level
  #[arg(short = 'l', long, value_enum, default_value = "info", global = true)]
  log_level: LogLevel,

  /// Log output format
  #[arg(long, value_enum, default_value = "pretty", global = true)]
  log_format: LogFormat,

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
    /// Output format
    #[arg(short, long, value_enum, default_value = "text")]
    output: OutputFormat,
  },
  /// Evaluate a config and create a plan without applying
  Plan {
    file: String,
    /// Output format
    #[arg(short, long, value_enum, default_value = "text")]
    output: OutputFormat,
  },
  /// Remove all binds from the current snapshot
  Destroy {
    /// Show what would be destroyed without making changes
    #[arg(long)]
    dry_run: bool,
    /// Output format
    #[arg(short, long, value_enum, default_value = "text")]
    output: OutputFormat,
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
    #[arg(short, long)]
    verbose: bool,
    /// Output format
    #[arg(short, long, value_enum, default_value = "text")]
    output: OutputFormat,
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
    /// Output format
    #[arg(short, long, value_enum, default_value = "text")]
    output: OutputFormat,
  },
  /// Clean up unused builds and inputs from the store
  Gc {
    /// Show what would be removed without making changes
    #[arg(long)]
    dry_run: bool,
    /// Output format
    #[arg(short, long, value_enum, default_value = "text")]
    output: OutputFormat,
  },
}

fn main() -> ExitCode {
  let cli = Cli::parse();

  match cli.color {
    ColorChoice::Always => owo_colors::set_override(true),
    ColorChoice::Never => owo_colors::set_override(false),
    ColorChoice::Auto => {}
  }

  let level: Level = cli.log_level.into();
  let show_timestamps = matches!(cli.log_level, LogLevel::Debug | LogLevel::Trace);

  match cli.log_format {
    LogFormat::Pretty => {
      if show_timestamps {
        tracing_subscriber::registry()
          .with(
            fmt::layer()
              .with_target(true)
              .with_filter(tracing_subscriber::filter::LevelFilter::from_level(level)),
          )
          .init();
      } else {
        tracing_subscriber::registry()
          .with(
            fmt::layer()
              .without_time()
              .with_target(false)
              .with_filter(tracing_subscriber::filter::LevelFilter::from_level(level)),
          )
          .init();
      }
    }
    LogFormat::Json => {
      tracing_subscriber::registry()
        .with(
          fmt::layer()
            .json()
            .with_file(true)
            .with_line_number(true)
            .with_target(true)
            .with_filter(tracing_subscriber::filter::LevelFilter::from_level(level)),
        )
        .init();
    }
  }

  let result = match cli.command {
    Commands::Init { path } => cmd_init(&path),
    Commands::Apply { file, repair, output } => cmd_apply(&file, repair, output),
    Commands::Plan { file, output } => cmd_plan(&file, output),
    Commands::Destroy { dry_run, output } => cmd_destroy(dry_run, output),
    Commands::Diff {
      snapshot_a,
      snapshot_b,
      verbose,
      output,
    } => cmd_diff(snapshot_a, snapshot_b, verbose, output),
    Commands::Update {
      config,
      inputs,
      dry_run,
    } => cmd_update(config.as_deref(), inputs, dry_run),
    Commands::Info => {
      cmd_info();
      Ok(())
    }
    Commands::Status { verbose, output } => cmd_status(verbose, output),
    Commands::Gc { dry_run, output } => cmd_gc(dry_run, output),
  };

  match result {
    Ok(()) => ExitCode::SUCCESS,
    Err(err) => {
      eprintln!("Error: {err:?}");
      ExitCode::FAILURE
    }
  }
}
