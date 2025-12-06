use std::sync::{Arc, Mutex};

use chrono::NaiveDateTime;

use crate::event_history::EventHistory;

pub type SharedEventHistory<T> = Arc<Mutex<EventHistory<T>>>;

// Events
pub trait HasId {
    type ID: Eq + PartialEq;
    fn get_id(&self) -> &Self::ID;
}

pub struct WindowEvent {
    pub address: String,
    pub class: String,
    pub monitor: Option<i128>,
    pub time: NaiveDateTime,
}

#[derive(Clone)]
pub struct HyprEventHistory {
    pub focus_events: Option<SharedEventHistory<WindowEvent>>,
}

impl HasId for WindowEvent {
    type ID = String;

    fn get_id(&self) -> &Self::ID {
        &self.address
    }
}
