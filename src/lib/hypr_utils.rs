use chrono::Local;
use hyprland::{
    data::{Client, Clients, Monitors},
    shared::{Address, HyprData, HyprDataActiveOptional},
};
use log::error;

use crate::types::{SortedDistinctVec, WindowEvent};

pub async fn current_focused_window_event() -> Option<WindowEvent> {
    let time = Local::now().naive_local();
    let (active_client, monitors) =
        match tokio::try_join!(Client::get_active_async(), Monitors::get_async()) {
            Ok((client, monitors)) => (client, monitors),
            Err(e) => {
                error!("Failed to query hyprland monitors and clients: {e}");
                return None;
            }
        };

    let active_client = active_client?;

    Some(WindowEvent {
        monitor: active_client.monitor.and_then(|client_monitor| {
            monitors.into_iter().find_map(|m| {
                if m.id == client_monitor {
                    Some(m.name)
                } else {
                    None
                }
            })
        }),
        address: active_client.address.to_string(),
        time,
    })
}

async fn get_window_monitor(address: &Address) -> Option<String> {
    let (clients, monitors) = match tokio::try_join!(Clients::get_async(), Monitors::get_async()) {
        Ok((clients, monitors)) => (clients, monitors),
        Err(e) => {
            error!("Failed to query hyprland monitors and clients: {e}");
            return None;
        }
    };

    let monitor_at_address = clients.iter().find_map(|c| {
        if c.address == *address {
            c.monitor
        } else {
            None
        }
    })?;

    monitors.into_iter().find_map(|m| {
        if m.id == monitor_at_address {
            Some(m.name)
        } else {
            None
        }
    })
}

pub enum WindowMonitorRequest {
    Matching { window_monitor: String },
    NoMatch,
    AllRequested { window_monitor: String },
}

pub async fn get_window_monitor_request(
    address: &Address,
    requested_monitors: &'static SortedDistinctVec<String>,
) -> WindowMonitorRequest {
    match get_window_monitor(address).await {
        Some(monitor) => {
            if requested_monitors.get().is_empty() {
                return WindowMonitorRequest::AllRequested {
                    window_monitor: monitor,
                };
            }
            if requested_monitors.get().contains(&monitor) {
                WindowMonitorRequest::Matching {
                    window_monitor: monitor,
                }
            } else {
                WindowMonitorRequest::NoMatch
            }
        }
        None => WindowMonitorRequest::NoMatch,
    }
}
