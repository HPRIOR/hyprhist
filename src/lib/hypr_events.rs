use chrono::Local;
use hyprland::{data::Clients, event_listener::AsyncEventListener, shared::HyprData};
use log::{error, info};

use crate::types::{HyprEventHistory, WindowEvent};

#[allow(clippy::missing_errors_doc)]
pub async fn listen(HyprEventHistory { focus_events }: HyprEventHistory) -> anyhow::Result<()> {
    let mut event_listener = AsyncEventListener::new();

    if let Some(focus_events) = focus_events {
        event_listener.add_active_window_changed_handler(move |id| {
            let focus_events = focus_events.clone();
            Box::pin(async move {
                let Some(window_event_data) = id else {
                    return;
                };

                let monitor = match Clients::get_async().await {
                    Ok(clients) => clients
                        .iter()
                        .find(|c| c.address == window_event_data.address)
                        .and_then(|c| c.monitor),
                    Err(err) => {
                        error!("Failed to fetch clients while recording focus event: {err}");
                        None
                    }
                };

                let mut window_event_history = focus_events.lock().await;
                let add_window_event_result = window_event_history.add(WindowEvent {
                    class: window_event_data.class,
                    monitor,
                    address: window_event_data.address.to_string(),
                    time: Local::now().naive_local(),
                });

                if let Some(WindowEvent {
                    address,
                    time,
                    monitor: _,
                    class: _,
                }) = add_window_event_result
                {
                    info!("Registered window event with id {address} at {time}");
                } else {
                    error!("Failed to register event!");
                }
            })
        });
    }

    event_listener.start_listener_async().await?;

    Ok(())
}
