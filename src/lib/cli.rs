use anyhow::Context;
use clap::{Args, Parser, Subcommand};
use hyprland::{data::Monitors, shared::HyprData};

use crate::event_history::HistorySize;

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum FocusCommand {
    Next,
    Prev,
}

#[derive(Debug, Clone, PartialEq, Eq, Args)]
pub struct RawDaemonArgs {
    /// Restrict focus tracking to specific monitors (can be repeated)
    #[arg(long = "monitor")]
    pub monitors: Vec<String>,
    /// Maximum number of focus events to retain in history (must be >= 1)
    #[arg(long = "history-size", default_value_t = HistorySize::default())]
    pub history_size: HistorySize,
}

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum RawDaemonCommand {
    Focus(RawDaemonArgs),
}

#[derive(Debug, Clone, PartialEq, Eq, Subcommand)]
pub enum RawCommand {
    Daemon {
        #[command(subcommand)]
        command: RawDaemonCommand,
    },
    Focus {
        #[command(subcommand)]
        command: FocusCommand,
    },
}

/// Root CLI type as parsed directly from the command line.
#[derive(Debug, Clone, PartialEq, Eq, Parser)]
#[command(name = "hyprhist", about = "hyprhist CLI")]
pub struct RawCli {
    #[command(subcommand)]
    pub command: RawCommand,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MonitorSelection {
    pub name: String,
    pub id: i128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DaemonArgs {
    pub monitors: Option<Vec<MonitorSelection>>,
    pub history_size: HistorySize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DaemonCommand {
    Focus(DaemonArgs),
}

/// Top-level commands: `daemon` and `focus`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Daemon { command: DaemonCommand },
    Focus { command: FocusCommand },
}

/// Root CLI type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cli {
    pub command: Command,
}

async fn enrich_daemon_args(
    RawDaemonArgs {
        monitors,
        history_size,
    }: RawDaemonArgs,
) -> anyhow::Result<DaemonArgs> {
    if monitors.is_empty() {
        Ok(DaemonArgs {
            monitors: None,
            history_size,
        })
    } else {
        let requested_monitors: Vec<MonitorSelection> = Monitors::get_async()
            .await
            .context("Failed to fetch monitors from Hyprland")?
            .into_iter()
            .filter_map(|monitor| {
                if monitors.contains(&monitor.name) {
                    Some(MonitorSelection {
                        id: monitor.id,
                        name: monitor.name,
                    })
                } else {
                    None
                }
            })
            .collect();

        Ok(DaemonArgs {
            monitors: Some(requested_monitors),
            history_size,
        })
    }
}
#[allow(clippy::missing_errors_doc)]
pub async fn enrich_raw_cli(raw: RawCli) -> anyhow::Result<Cli> {
    let command = match raw.command {
        RawCommand::Daemon { command } => match command {
            RawDaemonCommand::Focus(args) => Command::Daemon {
                command: DaemonCommand::Focus(enrich_daemon_args(args).await?),
            },
        },
        RawCommand::Focus { command } => Command::Focus { command },
    };

    Ok(Cli { command })
}

#[allow(clippy::missing_errors_doc)]
pub async fn parse_cli() -> anyhow::Result<Cli> {
    enrich_raw_cli(RawCli::parse()).await
}
