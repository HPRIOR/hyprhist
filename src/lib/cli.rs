use clap::{Args, Parser, Subcommand};

use crate::event_history::HistorySize;

#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct FocusCommandArgs {
    #[arg(long = "monitor")]
    pub requested_monitors: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum FocusCommand {
    Next(FocusCommandArgs),
    Prev(FocusCommandArgs),
}

#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct DaemonArgs {
    /// Restrict focus tracking to specific monitors (can be repeated)
    #[arg(long = "monitor")]
    pub requested_monitors: Vec<String>,
    /// Maximum number of focus events to retain in history (must be >= 1)
    #[arg(long = "history-size", default_value_t = HistorySize::default())]
    pub history_size: HistorySize,
}

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum DaemonCommand {
    Focus(DaemonArgs),
}

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum Command {
    Daemon {
        #[command(subcommand)]
        command: DaemonCommand,
    },
    Focus {
        #[command(subcommand)]
        command: FocusCommand,
    },
}

/// Root CLI type as parsed directly from the command line.
#[derive(Debug, Clone, PartialEq, Eq, Parser)]
#[command(name = "hyprhist", about = "hyprhist CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}
