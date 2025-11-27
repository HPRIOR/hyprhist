use chrono::NaiveDateTime;
use hyprland::event_listener::{AsyncEventListener, WindowEventData};
use hyprland::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
struct WindowEvent {
    address: String,
    time: NaiveDateTime,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> hyprland::Result<()> {
    // Create a event listener
    let mut event_listener = AsyncEventListener::new();

    #[allow(deprecated)]
    event_listener.add_active_window_changed_handler(async_closure! {
        |id| {
            if let Some(WindowEventData { class, title, address }) = id {
                // queue.clone().push_back(address.clone());
            }
        }
    });

    event_listener.start_listener_async().await?;
    Ok(())
}
