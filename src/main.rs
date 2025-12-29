use std::sync::Arc;

use clap::Parser;
use env_logger::Env;
use tokio::sync::Mutex;

use lib::{
    cli::{Cli, Command, DaemonArgs, DaemonCommand},
    daemon,
    event_history::EventHistory,
    hypr_utils::current_focused_window_event,
    socket,
    types::{FocusEvents, HyprEvents, SharedEventHistory, SortedDistinctVec, WindowEvent},
};

fn shared_mutex<T>(of: T) -> Arc<Mutex<T>> {
    Arc::new(Mutex::new(of))
}

fn window_on_requested_monitor(window_event: &WindowEvent, requested_monitors: &[String]) -> bool {
    requested_monitors.is_empty()
        || window_event
            .monitor
            .as_ref()
            .is_some_and(|event_monitor| requested_monitors.contains(event_monitor))
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    env_logger::Builder::from_env(Env::default().default_filter_or("info")).init();

    let cli: &'static Cli = Box::leak(Box::new(Cli::parse()));

    match &cli.command {
        Command::Daemon { command } => match command {
            DaemonCommand::Focus(DaemonArgs {
                requested_monitors,
                history_size,
            }) => {
                let focus_events: SharedEventHistory<WindowEvent> = {
                    let event_history = match current_focused_window_event().await {
                        Some(window_event)
                            if window_on_requested_monitor(&window_event, requested_monitors) =>
                        {
                            EventHistory::bootstrap(window_event, *history_size)
                        }
                        _ => EventHistory::new(*history_size),
                    };

                    shared_mutex(event_history)
                };

                let requested_monitors: SortedDistinctVec<String> =
                    SortedDistinctVec::new(requested_monitors.clone());

                let hypr_events: HyprEvents = HyprEvents::Focus(FocusEvents {
                    focus_events,
                    requested_monitors: Box::leak(Box::new(requested_monitors)),
                });

                tokio::try_join!(
                    daemon::run(hypr_events.clone()),
                    socket::listen(hypr_events)
                )?;
            }
        },
        Command::Focus { command } => socket::send_focus_command(command).await?,
    }

    Ok(())
}
