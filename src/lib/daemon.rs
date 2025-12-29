use chrono::Local;
use hyprland::{
    event_listener::{AsyncEventListener, WindowEventData, WindowMoveEvent},
    shared::Address,
};
use log::{debug, info};
use std::{future::Future, pin::Pin};

use crate::{
    hypr_utils::{WindowMonitorRequest, get_window_monitor_request},
    types::{FocusEvents, HyprEvents, SharedEventHistory, SortedDistinctVec, WindowEvent},
};

type ListenerFuture<T> =
    Box<dyn Fn(T) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync + 'static>;

fn window_closed_handler(focus_events: SharedEventHistory<WindowEvent>) -> ListenerFuture<Address> {
    Box::new(move |address: Address| {
        debug!("Window closed event occured: {address:?}");
        let focus_events = focus_events.clone();
        Box::pin(async move {
            let mut event_history = focus_events.lock().await;
            event_history.remove(&address.to_string());
        })
    })
}

fn window_moved_handler(
    focus_events: SharedEventHistory<WindowEvent>,
    requested_monitors: &'static SortedDistinctVec<String>,
) -> ListenerFuture<WindowMoveEvent> {
    Box::new(move |window_move_event: WindowMoveEvent| {
        debug!("Window move event occured: {window_move_event:?}");
        let focus_events = focus_events.clone();

        Box::pin(async move {
            match get_window_monitor_request(&window_move_event.window_address, requested_monitors)
                .await
            {
                WindowMonitorRequest::Matching { window_monitor } => {
                    let time = Local::now().naive_local();
                    let mut focus_history = focus_events.lock().await;
                    focus_history.activate(&window_move_event.window_address.to_string());
                    focus_history.add(WindowEvent {
                        address: window_move_event.window_address.to_string(),
                        monitor: Some(window_monitor),
                        time,
                    });
                }
                WindowMonitorRequest::NoMatch => {
                    let mut focus_history = focus_events.lock().await;
                    focus_history.deactivate(&window_move_event.window_address.to_string());
                }
                WindowMonitorRequest::AllRequested { window_monitor: _ } => {
                    // Active/Inactive windows aren't necessary if all monitors are tracked
                }
            }
        })
    })
}

fn active_window_changed_handler(
    focus_events: SharedEventHistory<WindowEvent>,
    requested_monitors: &'static SortedDistinctVec<String>,
) -> ListenerFuture<Option<WindowEventData>> {
    Box::new(move |maybe_window_event_data| {
        debug!("Active window event occured: {maybe_window_event_data:?}");
        let focus_events = focus_events.clone();

        Box::pin(async move {
            let now_time = Local::now().naive_local();
            let Some(window_event_data) = maybe_window_event_data else {
                return;
            };

            match get_window_monitor_request(&window_event_data.address, requested_monitors).await {
                WindowMonitorRequest::Matching {
                    window_monitor: monitor,
                }
                | WindowMonitorRequest::AllRequested {
                    window_monitor: monitor,
                } => {
                    let mut event_history = focus_events.lock().await;

                    let window_event = WindowEvent {
                        monitor: Some(monitor),
                        address: window_event_data.address.to_string(),
                        time: now_time,
                    };

                    if let Some(WindowEvent {
                        address,
                        time,
                        monitor: _,
                    }) = event_history.add(window_event)
                    {
                        info!("Registered active window event with id {address} at {time}");
                    }
                }
                WindowMonitorRequest::NoMatch => {}
            }
        })
    })
}

#[allow(clippy::missing_errors_doc)]
pub async fn run(hypr_events: HyprEvents) -> anyhow::Result<()> {
    let mut event_listener = AsyncEventListener::new();
    match hypr_events {
        HyprEvents::Focus(FocusEvents {
            focus_events,
            requested_monitors,
        }) => {
            event_listener.add_window_closed_handler(window_closed_handler(focus_events.clone()));

            event_listener.add_active_window_changed_handler(active_window_changed_handler(
                focus_events.clone(),
                requested_monitors,
            ));

            event_listener.add_window_moved_handler(window_moved_handler(
                focus_events.clone(),
                requested_monitors,
            ));
        }
    }

    info!("Starting hyprland event listener");
    event_listener.start_listener_async().await?;
    Ok(())
}
