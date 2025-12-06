use std::sync::{Arc, Mutex};

use env_logger::Env;
use lib::{
    cli::{self, Cli, Command, DaemonCommand, DaemonFocus, FocusCommand},
    daemon,
    event_history::{self, EventHistory},
    hypr_events::{self},
    types::{HyprEventHistory, SharedEventHistory, WindowEvent},
};

fn shared_mutex<T>(of: T) -> Arc<Mutex<T>> {
    Arc::new(Mutex::new(of))
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let cli: Cli = cli::parse_cli();

    let focus_events: SharedEventHistory<WindowEvent> = shared_mutex(EventHistory::default());

    let event_history = HyprEventHistory {
        focus_events: Some(focus_events),
    };

    hypr_events::listen(event_history.clone()).await?;

    match cli.command {
        Command::Daemon { command } => daemon::run(command, event_history.clone()).await?,
        Command::Focus { command } => match command {
          FocusCommand::Next => todo!(),
            FocusCommand::Prev => todo!(),
        },
    };

    Ok(())
}
