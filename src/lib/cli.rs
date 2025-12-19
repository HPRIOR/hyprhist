use clap::{Args, Parser, Subcommand};

use crate::event_history::HistorySize;

#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct DaemonFocus {
    /// Restrict focus tracking to specific monitors (can be repeated)
    #[arg(long = "monitor")]
    pub monitors: Vec<i128>,
    /// Maximum number of focus events to retain in history (must be >= 1)
    #[arg(long = "history-size", default_value_t = HistorySize::default())]
    pub history_size: HistorySize,
}

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum DaemonCommand {
    Focus(DaemonFocus),
}

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum FocusCommand {
    Next,
    Prev,
}

/// Top-level commands: `daemon` and `focus`.
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

/// Root CLI type.
#[derive(Debug, Clone, PartialEq, Eq, Parser)]
#[command(name = "hyprhist", about = "hyprhist CLI")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[must_use]
pub fn parse_cli() -> Cli {
    Cli::parse()
}
