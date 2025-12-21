use chrono::Local;
use hyprland::{
    data::Clients,
    event_listener::{AsyncEventListener, WindowEventData, WindowMoveEvent},
    shared::{Address, HyprData},
};
use log::{debug, error, info};
use std::{future::Future, pin::Pin, sync::Arc};

use crate::{
    cli::{DaemonArgs, DaemonCommand},
    types::{HyprEventHistory, SharedEventHistory, WindowEvent},
};

type ListenerFuture<T> =
    Box<dyn Fn(T) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync + 'static>;

async fn get_window_monitor(address: &Address) -> Option<i128> {
    let clients = match Clients::get_async().await {
        Ok(clients) => clients,
        Err(e) => {
            error!("Failed to fetch clients: {e}");
            return None;
        }
    };

    clients.iter().find_map(|c| {
        if c.address == *address {
            c.monitor
        } else {
            None
        }
    })
}

fn window_closed_handler(focus_events: SharedEventHistory<WindowEvent>) -> ListenerFuture<Address> {
    Box::new(move |address: Address| {
        let focus_events = focus_events.clone();
        Box::pin(async move {
            let mut event_history = focus_events.lock().await;
            event_history.remove(&address.to_string());
        })
    })
}

fn window_moved_handler(
    focus_events: SharedEventHistory<WindowEvent>,
    requested_monitors: Arc<Vec<i128>>,
) -> ListenerFuture<WindowMoveEvent> {
    Box::new(move |window_move_event: WindowMoveEvent| {
        let focus_events = focus_events.clone();
        let requested_monitors = requested_monitors.clone();

        Box::pin(async move {
            if requested_monitors.is_empty() {
                // Active/Inactive winows aren't necessary if all monitors are tracked
                return;
            }

            let window_monitor_opt = get_window_monitor(&window_move_event.window_address).await;

            let window_moved_to_tracked_monitor = window_monitor_opt
                .map_or_else(|| false, |monitor| requested_monitors.contains(&monitor));
            if window_moved_to_tracked_monitor {
                let mut focus_history = focus_events.lock().await;
                focus_history.activate(&window_move_event.window_address.to_string());
            } else {
                let mut focus_history = focus_events.lock().await;
                focus_history.deactivate(&window_move_event.window_address.to_string());
            }
        })
    })
}

fn active_window_changed_handler(
    focus_events: SharedEventHistory<WindowEvent>,
    requested_monitors: Arc<Vec<i128>>,
) -> ListenerFuture<Option<WindowEventData>> {
    Box::new(move |maybe_window_event_data| {
        let focus_events = focus_events.clone();
        let requested_monitors = Arc::clone(&requested_monitors);

        Box::pin(async move {
            let now_time = Local::now().naive_local();
            let Some(window_event_data) = maybe_window_event_data else {
                return;
            };

            let monitor = get_window_monitor(&window_event_data.address).await;

            let window_event_on_untracked_monitor = !requested_monitors.is_empty()
                && monitor.map_or_else(|| false, |m| requested_monitors.contains(&m));
            if window_event_on_untracked_monitor {
                debug!("Ignoring window event on untracked monitor");
                return;
            }

            let mut event_history = focus_events.lock().await;

            let window_event = WindowEvent {
                class: window_event_data.class,
                monitor,
                address: window_event_data.address.to_string(),
                time: now_time,
            };

            if let Some(WindowEvent {
                address,
                time,
                monitor: _,
                class: _,
            }) = event_history.add(window_event)
            {
                info!("Registered window event with id {address} at {time}");
            }
        })
    })
}

#[allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::too_many_lines
)]
pub async fn run(
    command: DaemonCommand,
    HyprEventHistory { focus_events }: HyprEventHistory,
) -> anyhow::Result<()> {
    let mut event_listener = AsyncEventListener::new();

    match command {
        DaemonCommand::Focus(DaemonArgs {
            monitors: requested_monitors,
            ..
        }) => {
            if let Some(focus_events) = focus_events {
                let requested_monitors: Arc<Vec<i128>> = Arc::new(
                    requested_monitors
                        .as_ref()
                        .map(|monitors| monitors.iter().map(|monitor| monitor.id).collect())
                        .unwrap_or_default(),
                );
                event_listener
                    .add_window_closed_handler(window_closed_handler(focus_events.clone()));

                event_listener.add_active_window_changed_handler(active_window_changed_handler(
                    focus_events.clone(),
                    requested_monitors.clone(),
                ));

                event_listener.add_window_moved_handler(window_moved_handler(
                    focus_events.clone(),
                    requested_monitors.clone(),
                ));
            }
        }
    }

    event_listener.start_listener_async().await?;
    Ok(())
}
