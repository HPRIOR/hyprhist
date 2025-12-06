use chrono::Local;
use hyprland::{
    data::{Clients, Monitor, Monitors},
    event_listener::{self, AsyncEventListener},
    shared::HyprData,
};
use log::{error, info};
use std::sync::{Arc, PoisonError, RwLock};

use crate::{
    cli::{DaemonCommand, DaemonFocus},
    types::{HyprEventHistory, WindowEvent},
};

type SharedActiveMonitors = Arc<RwLock<Vec<Monitor>>>;

async fn get_active_monitors() -> anyhow::Result<Vec<Monitor>> {
    Ok(Monitors::get_async()
        .await?
        .into_iter()
        .filter(|m| !m.disabled)
        .collect())
}

// The simplest thing to do is query hyprland for the active monitors each time instead of using
// the event arguments to track added/removed. The monitor removed listener presents a String for
// the monitor name, so I'd have to query hyprland regardless in this case. In the case of mutex
// poisening for add, I'd want to recover by querying hyprland.
// fn listen_for_monitor_changes(
//     event_listener: &mut AsyncEventListener,
//     shared_active_monitors: &SharedActiveMonitors,
// ) {
//     let shared_active_monitors_for_remove = Arc::clone(shared_active_monitors);
//     event_listener.add_monitor_removed_handler(move |_| {
//         let shared_active_monitors = Arc::clone(&shared_active_monitors_for_remove);
//         Box::pin(async move {
//             let new_active_monitors = get_active_monitors()
//                 .await
//                 .expect("Error retrieving monitors from hyprland");
//             let mut old_active_monitors = shared_active_monitors
//                 .write()
//                 .unwrap_or_else(PoisonError::into_inner);
//             *old_active_monitors = new_active_monitors;
//         })
//     });
//
//     let shared_active_monitors_for_add = Arc::clone(shared_active_monitors);
//     event_listener.add_monitor_added_handler(move |_| {
//         let shared_active_monitors = Arc::clone(&shared_active_monitors_for_add);
//         Box::pin(async move {
//             let new_active_monitors = get_active_monitors()
//                 .await
//                 .expect("Error retrieving monitors from hyprland");
//             let mut old_active_monitors = shared_active_monitors
//                 .write()
//                 .unwrap_or_else(PoisonError::into_inner);
//             *old_active_monitors = new_active_monitors;
//         })
//     });
// }

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

    // let shared_active_monitors: SharedActiveMonitors =
    //     Arc::new(RwLock::new(get_active_monitors().await?));

    // listen_for_monitor_changes(&mut event_listener, &shared_active_monitors.clone()); // this may
    // no tbe necessary

    match command {
        DaemonCommand::Focus(DaemonFocus {
            monitors: requested_monitors,
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

                        let mut event_history =
                            focus_events.lock().unwrap_or_else(PoisonError::into_inner);

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
