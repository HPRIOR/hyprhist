use chrono::Local;
use hyprland::{data::Clients, event_listener::AsyncEventListener, shared::HyprData};
use log::{error, info};
use std::sync::Arc;

use crate::{
    cli::{DaemonCommand, DaemonFocus},
    types::{HyprEventHistory, WindowEvent},
};

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
        DaemonCommand::Focus(DaemonFocus {
            monitors: requested_monitors,
            ..
        }) => {
            if let Some(focus_events) = focus_events {
                let requested_monitors = Arc::new(requested_monitors);

                event_listener.add_active_window_changed_handler(move |maybe_window_event_data| {
                    let focus_events = focus_events.clone();
                    let requested_monitors = Arc::clone(&requested_monitors);

                    Box::pin(async move {
                        let now_time = Local::now().naive_local();
                        let Some(window_event_data) = maybe_window_event_data else {
                            return;
                        };

                        let clients = match Clients::get_async().await {
                            Ok(clients) => clients,
                            Err(e) => {
                                error!("Failed to fetch clients: {e}");
                                return;
                            }
                        };

                        let monitor: Option<i128> = clients.iter().find_map(|c| {
                            if c.address == window_event_data.address {
                                c.monitor
                            } else {
                                None
                            }
                        });

                        if !requested_monitors.is_empty()
                            && monitor.map_or_else(|| false, |m| requested_monitors.contains(&m))
                        {
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
                            info!("Registered window event with address {address} at {time}");
                        }
                    })
                });
            }
        }
    }

    event_listener.start_listener_async().await?;
    Ok(())
}
