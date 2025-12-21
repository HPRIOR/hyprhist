use std::sync::Arc;

use chrono::Local;
use env_logger::Env;
use hyprland::{data::Client, shared::HyprDataActiveOptional};
use log::error;
use tokio::sync::Mutex;

use lib::{
    cli::{self, Cli, Command},
    daemon,
    event_history::EventHistory,
    socket,
    types::{HyprEventHistory, SharedEventHistory, WindowEvent},
};

fn shared_mutex<T>(of: T) -> Arc<Mutex<T>> {
    Arc::new(Mutex::new(of))
}

async fn current_focused_window_event() -> Option<WindowEvent> {
    match Client::get_active_async().await {
        Ok(Some(client)) => Some(WindowEvent {
            class: client.class,
            monitor: client.monitor,
            address: client.address.to_string(),
            time: Local::now().naive_local(),
        }),
        Ok(None) => None,
        Err(err) => {
            error!("Failed to fetch active window: {err}");
            None
        }
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let cli: Cli = cli::parse_cli().await?;

    match cli.command {
        Command::Daemon { command } => {
            let (history_size, requested_monitor_ids) = match &command {
                cli::DaemonCommand::Focus(opts) => (
                    opts.history_size,
                    opts.monitors.as_ref().map_or_else(Vec::default, |m| {
                        m.iter().map(|x| Some(x.id)).collect::<Vec<Option<i128>>>()
                    }),
                ),
            };

            let focus_events: SharedEventHistory<WindowEvent> = {
                let event_history = match current_focused_window_event().await {
                    Some(event) if requested_monitor_ids.contains(&event.monitor) => {
                        EventHistory::bootstrap(event, history_size)
                    }
                    _ => EventHistory::new(history_size),
                };

                shared_mutex(event_history)
            };

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
