use std::sync::Arc;

use env_logger::Env;
use lib::{
    cli::{self, Cli, Command},
    daemon,
    event_history::EventHistory,
    socket,
    types::{HyprEventHistory, SharedEventHistory, WindowEvent},
};
use tokio::sync::Mutex;

fn shared_mutex<T>(of: T) -> Arc<Mutex<T>> {
    Arc::new(Mutex::new(of))
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let cli: Cli = cli::parse_cli();

    match cli.command {
        Command::Daemon { command } => {
            let history_size = match &command {
                cli::DaemonCommand::Focus(opts) => opts.history_size,
            };
            let focus_events: SharedEventHistory<WindowEvent> =
                shared_mutex(EventHistory::new(history_size));

            let event_history = HyprEventHistory {
                focus_events: Some(focus_events),
            };

            tokio::try_join!(
                daemon::run(command.clone(), event_history.clone()),
                socket::listen(command, event_history)
            )?;
        }
        Command::Focus { command } => socket::send_focus_command(command).await?,
    }

    Ok(())
}
