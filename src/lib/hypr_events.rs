use chrono::Local;
use hyprland::event_listener::AsyncEventListener;
use log::{error, info};

use crate::types::{HyprEventHistory, WindowEvent};

#[allow(clippy::missing_errors_doc)]
pub async fn listen(HyprEventHistory { focus_events }: HyprEventHistory) -> anyhow::Result<()> {
    let mut event_listener = AsyncEventListener::new();

    if let Some(focus_events) = focus_events {
        event_listener.add_active_window_changed_handler(move |id| {
            let focus_events = focus_events.clone();
            Box::pin(async move {
                if let Some(window_event_data) = id
                    && let Ok(mut weh) = focus_events.lock()
                {
                    let add_window_event_result = weh.add(WindowEvent {
                        class: window_event_data.class,
                        monitor: None,
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
                        info!("Registered window event with address {address} at {time}");
                    }
                } else {
                    error!("Failed to register event!");
                }
            })
        });
    }

    event_listener.start_listener_async().await?;

    Ok(())
}
