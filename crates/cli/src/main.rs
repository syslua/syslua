mod cmd;

use clap::{Parser, Subcommand};
use cmd::{cmd_apply, cmd_destroy, cmd_info, cmd_plan};
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
  Apply { file: String },
  Plan { file: String },
  Destroy { file: String },
  Info,
}

fn main() {
  let cli = Cli::parse();

  let level = if cli.verbose { Level::DEBUG } else { Level::INFO };

  FmtSubscriber::builder()
    .with_max_level(level)
    .with_target(false)
    .without_time()
    .init();

  match cli.command {
    Commands::Apply { file } => {
      cmd_apply(&file);
    }
    Commands::Plan { file } => {
      cmd_plan(&file);
    }
    Commands::Destroy { file } => {
      cmd_destroy(&file);
    }
    Commands::Info => {
      cmd_info();
    }
  }
}
