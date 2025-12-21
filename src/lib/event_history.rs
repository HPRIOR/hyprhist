use std::collections::{BTreeSet, HashMap, HashSet};
use std::mem::{self};
use std::num::NonZeroUsize;
use std::str::FromStr;

use log::{debug, info};

use crate::types::EventItem;

mod pretty_print {

    use crate::event_history::{EventHistory, EventStatus};
    use crate::types::EventItem;

    #[must_use]
    pub(super) fn pretty_print<T: EventItem>(history: &EventHistory<T>) -> String {
        if history.events.is_empty() {
            return "Events: <empty>".to_string();
        }

        let entries: Vec<String> = history
            .events
            .iter()
            .enumerate()
            .map(|(idx, status)| {
                let status_str = match status {
                    EventStatus::Active(event) => format!("Active(id={})", event.get_id()),
                    EventStatus::Inactive(event) => format!("Inactive(id={})", event.get_id()),
                    EventStatus::Deleted => "Deleted".to_string(),
                };
                let mut markers = Vec::new();
                if idx == history.head {
                    markers.push("head");
                }
                if idx == history.cursor {
                    markers.push("cursor");
                }
                if idx == history.history_start {
                    markers.push("history_start");
                }
                if markers.is_empty() {
                    format!("[{idx} {status_str}]")
                } else {
                    format!("[{idx} {status_str} {}]", markers.join(" "))
                }
            })
            .collect();

        let mut line = String::from("Events: ");
        for (idx, entry) in entries.iter().enumerate() {
            line.push_str(entry);
            if idx + 1 < entries.len() {
                line.push(' ');
            }
        }

        line
    }
}

#[derive(Debug)]
enum EventStatus<T> {
    Active(T),
    Inactive(T),
    Deleted,
}

impl<T> Default for EventStatus<T> {
    fn default() -> Self {
        Self::Deleted
    }
}

impl<T> EventStatus<T> {
    fn get_event(&self) -> Option<&T> {
        match self {
            EventStatus::Active(t) | EventStatus::Inactive(t) => Some(t),
            EventStatus::Deleted => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistorySize(NonZeroUsize);

impl HistorySize {
    #[must_use]
    pub const fn get(self) -> usize {
        self.0.get()
    }
}

impl std::fmt::Display for HistorySize {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.get())
    }
}

impl TryFrom<usize> for HistorySize {
    type Error = String;

    fn try_from(value: usize) -> Result<Self, Self::Error> {
        NonZeroUsize::new(value)
            .map(Self)
            .ok_or_else(|| "history size must be greater than zero".to_string())
    }
}

impl Default for HistorySize {
    fn default() -> Self {
        Self(NonZeroUsize::new(1000).expect("default history size is non-zero"))
    }
}

impl FromStr for HistorySize {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<NonZeroUsize>()
            .map(Self)
            .map_err(|_| "history-size must be a positive integer".to_string())
    }
}

mod traverse {
    use crate::event_history::HistorySize;

    pub fn next(current: usize, max_size: HistorySize) -> usize {
        if current == max_size.get() - 1 {
            0
        } else {
            current + 1
        }
    }

    pub fn prev(current: usize, max_size: HistorySize) -> usize {
        if current == 0 {
            max_size.get() - 1
        } else {
            current - 1
        }
    }
}

#[derive(Default, Debug)]
struct EventStatusCount {
    active: usize,
    inactive: usize,
    deleted: usize,
}

impl EventStatusCount {
    fn activate(&mut self) {
        self.active += 1;
        self.inactive -= 1;
    }

    fn deactivate(&mut self) {
        self.active -= 1;
        self.inactive += 1;
    }

    fn delete_active(&mut self) {
        self.deleted += 1;
        self.active -= 1;
    }
    fn delete_inactive(&mut self) {
        self.deleted += 1;
        self.inactive -= 1;
    }
}

#[derive(Debug)]
pub struct EventHistory<T: EventItem> {
    max_size: HistorySize,
    /// Moveable cursor in the event history. Can never exceed head, but can 'detach' from head
    /// when tracking back through the event history.
    cursor: usize,
    /// The position of the latest event history. If an event occurs while `cursor` is
    /// detached, the new event will be placed ahead `cursor`, and head will track this
    /// event; event history is truncated on when new events occur while tracking back in history.
    head: usize,
    /// The start of the history. Always 0 if the ring buffer has not overflowed. Head + 1 if head
    /// has wrapped around the `events` ring buffer. If the events ring buffer has
    /// overflowed and a new event occurs while `cursor` is detached, events between the
    /// previous `head` position and the new `head` position (just ahead of
    /// ``self.cursor``) are invalid. Moving this field to the previous position of `self.head`
    /// prevents tracking back to invalid history.
    history_start: usize,
    static_history: bool,
    // Ring buffer of events.
    events: Vec<EventStatus<T>>,
    event_idx_map: HashMap<T::ID, BTreeSet<usize>>,
    ignored_events: HashSet<T::ID>,
    event_status_count: EventStatusCount,
}

impl<T: EventItem> EventHistory<T> {
    #[must_use]
    pub fn new(max_size: HistorySize) -> Self {
        info!("Creating event history with max_size: {}", max_size.get());
        Self {
            max_size,
            cursor: 0,
            head: 0,
            history_start: 0,
            static_history: true,
            events: Vec::with_capacity(max_size.get()),
            event_idx_map: HashMap::default(),
            ignored_events: HashSet::default(),
            event_status_count: EventStatusCount::default(),
        }
    }

    pub fn bootstrap(init: T, max_size: HistorySize) -> Self {
        info!(
            "Bootstrapping event history with max_size: {}",
            max_size.get()
        );
        let mut event_history = Self::new(max_size);
        event_history.add(init);
        event_history
    }

    fn next_idx(&self, current: usize) -> usize {
        traverse::next(current, self.max_size)
    }

    fn prev_idx(&self, current: usize) -> usize {
        traverse::prev(current, self.max_size)
    }

    fn get_idx_id(&self, idx: usize) -> Option<&T::ID> {
        self.events
            .get(idx)
            .and_then(EventStatus::get_event)
            .map(T::get_id)
    }

    fn find_next<NextFn: Fn(usize, HistorySize) -> usize>(
        &self,
        current: usize,
        stop_idx: usize,
        next_fn: NextFn,
    ) -> Option<usize> {
        let initial_id = self.get_idx_id(current);
        let mut idx = current;
        loop {
            idx = next_fn(idx, self.max_size);
            if !self.in_valid_range(idx) || idx == stop_idx {
                return None;
            }
            let current_event = self.events.get(idx)?;

            if let (EventStatus::Active(current_event), Some(initial_id)) =
                (current_event, initial_id)
                && current_event.get_id() == initial_id
            {
                continue;
            }

            if let EventStatus::Active(_) = current_event {
                return Some(idx);
            }
        }
    }

    fn next_active_idx(&self, current: usize) -> Option<usize> {
        if current == self.head {
            return None;
        }

        self.find_next(current, self.next_idx(self.head), traverse::next)
    }

    fn prev_active_idx(&self, current: usize) -> Option<usize> {
        if current == self.history_start {
            return None;
        }
        self.find_next(current, self.prev_idx(self.history_start), traverse::prev)
    }

    fn in_valid_range(&self, idx: usize) -> bool {
        if self.events.is_empty() {
            return false;
        }

        if self.history_start <= self.head {
            // No overflow in the buffer. Idx must be somewhere inbetween the event history
            self.history_start <= idx && idx <= self.head
        } else {
            // Overflow, history is 'ahead' of head ihe buffer, idx must sit on either side of head
            // or history start
            idx >= self.history_start || idx <= self.head
        }
    }

    pub fn forward(&mut self) -> Option<&T> {
        let new_cursor_position = self.next_active_idx(self.cursor)?;
        self.cursor = new_cursor_position;
        let current_event: &T = self.events[new_cursor_position].get_event()?;
        self.ignored_events.insert(current_event.get_id().clone());
        debug!(
            "Forward invoked; cursor moved to {new_cursor_position} with id {}; {} inserted into ignore set.\n{}",
            current_event.get_id(),
            current_event.get_id(),
            pretty_print::pretty_print(self)
        );
        Some(current_event)
    }

    pub fn backward(&mut self) -> Option<&T> {
        let new_cursor_position = self.prev_active_idx(self.cursor)?;
        self.cursor = new_cursor_position;
        let current_event: &T = self.events[new_cursor_position].get_event()?;
        self.ignored_events.insert(current_event.get_id().clone());
        debug!(
            "Backward invoked; cursor moved to {new_cursor_position} with id {}; {} inserted into ignore set.\n{}",
            current_event.get_id(),
            current_event.get_id(),
            pretty_print::pretty_print(self)
        );
        Some(current_event)
    }

    #[allow(clippy::missing_panics_doc)]
    pub fn remove(&mut self, id: &T::ID) {
        if let Some(idxs) = self.event_idx_map.remove(id) {
            debug!("Deleting id {id}, at the following locations: {idxs:?}");
            for idx in &idxs {
                match self.events[*idx] {
                    EventStatus::Active(_) => {
                        self.event_status_count.delete_active();
                    }
                    EventStatus::Inactive(_) => {
                        self.event_status_count.delete_inactive();
                    }
                    EventStatus::Deleted => {}
                }
                let _ = mem::replace(&mut self.events[*idx], EventStatus::Deleted);
            }

            if idxs.contains(&self.cursor)
                && let Some(prev_active_idx) = self.prev_active_idx(self.cursor)
            {
                debug!("Cursor present in deleted events, moving to previous active event");
                self.cursor = prev_active_idx;
            }

            debug!("{}", pretty_print::pretty_print(self));
        }
    }

    #[allow(clippy::missing_panics_doc)]
    pub fn deactivate(&mut self, id: &T::ID) {
        if let Some(idxs) = self.event_idx_map.get(id) {
            debug!("Deactivating id {id}, at the following locations: {idxs:?}");
            for idx in idxs {
                let event = mem::replace(&mut self.events[*idx], EventStatus::Deleted);
                match event {
                    EventStatus::Active(t) => {
                        self.events[*idx] = EventStatus::Inactive(t);
                        self.event_status_count.deactivate();
                    }
                    inactive_event @ EventStatus::Inactive(_) => {
                        self.events[*idx] = inactive_event;
                    }
                    EventStatus::Deleted => panic!("Tried to deactivate a deleted event"),
                }
            }

            if idxs.contains(&self.cursor)
                && let Some(prev_active_idx) = self.prev_active_idx(self.cursor)
            {
                debug!("Cursor present in deleted events, moving to previous active event");
                self.cursor = prev_active_idx;
            }

            debug!("{}", pretty_print::pretty_print(self));
        }
    }

    #[allow(clippy::missing_panics_doc)]
    pub fn activate(&mut self, id: &T::ID) {
        if let Some(idxs) = self.event_idx_map.get(id) {
            debug!("Activating id {id}, at the following locations: {idxs:?}");
            for idx in idxs {
                let event = mem::replace(&mut self.events[*idx], EventStatus::Deleted);
                match event {
                    EventStatus::Inactive(t) => {
                        self.events[*idx] = EventStatus::Active(t);
                        self.event_status_count.activate();
                    }
                    active_event @ EventStatus::Active(_) => {
                        self.events[*idx] = active_event;
                    }
                    EventStatus::Deleted => panic!("Tried to activate a deleted event"),
                }
            }
        }

        debug!("{}", pretty_print::pretty_print(self));
    }

    #[allow(clippy::missing_panics_doc)]
    pub fn add(&mut self, item: T) -> Option<&T> {
        if self.ignored_events.contains(item.get_id()) {
            debug!(
                "Ignoring event with {}; present in ignore set: {:?}.\n{}",
                item.get_id(),
                self.ignored_events,
                pretty_print::pretty_print(self)
            );
            self.ignored_events.remove(item.get_id());
            return None;
        }

        let is_duplicate_item = match &self.events.get(self.cursor) {
            Some(EventStatus::Active(current)) => current.get_id() == item.get_id(),
            _ => false,
        };

        if is_duplicate_item {
            info!(
                "Add skipped; duplicate item at cursor {} (head {})",
                self.cursor, self.head
            );
            return None;
        }

        // When the cursor is detached and history is not static we want the
        // history to start from the previous head position to prevent reads into 'dead' regions
        // of the buffer when cursor tracks backwards.
        let cursor_is_detached = self.head != self.cursor;
        if cursor_is_detached && !self.static_history {
            let new_history_start = self.next_idx(self.head);
            info!(
                "History truncated after detached cursor; history_start: {} -> {}",
                self.history_start, new_history_start
            );
            self.history_start = self.next_idx(self.head);
        }

        let insert_idx = if self.events.is_empty() {
            0
        } else {
            // Always place the next event immediately after the current cursor position,
            // overwriting any forward history regardless of whether the buffer has filled.
            self.next_idx(self.cursor)
        };

        // When a new item is added, regardless of detached head we'll want cursor to be
        // incremented by 1, and for head to track with it. If the cursor is detached, the head
        // will now track back to cursor. Otherwise, they'll both just move forward by one.
        self.cursor = insert_idx;
        self.head = self.cursor;

        // If head catches up with history start, history will no longer be static, history start should
        // track just ahead of head.
        //
        // This condition should only be true once. If history is not static, it will always be
        // pushed just beyond head above
        let buffer_full = self.events.len() == self.max_size.get();
        if buffer_full && self.head == self.history_start {
            self.static_history = false;
            let new_history_start = self.next_idx(self.history_start);
            debug!(
                "History buffer full; advancing history_start {} -> {}.",
                self.history_start, new_history_start,
            );
            self.history_start = new_history_start;
            debug!("{}", pretty_print::pretty_print(self));
        }

        let item_id = item.get_id().clone();
        let active_item = EventStatus::Active(item);

        let prev_occupied_event_id_opt = self
            .events
            .get(insert_idx)
            .and_then(EventStatus::get_event)
            .map(T::get_id);

        if let Some(id) = prev_occupied_event_id_opt
            && let Some(idxs) = self.event_idx_map.get_mut(id)
        {
            idxs.remove(&insert_idx);
            if idxs.is_empty() {
                self.event_idx_map.remove(id);
            }
        }

        if insert_idx == self.events.len() {
            self.events.push(active_item);
        } else {
            self.events[insert_idx] = active_item;
        }

        self.event_status_count.active += 1;
        self.event_idx_map
            .entry(item_id)
            .or_default()
            .insert(insert_idx);
        debug!("Event idx map after add: {:?}", self.event_idx_map);
        self.events[insert_idx].get_event()
    }
}

#[cfg(test)]
mod test {

    use super::EventStatus;
    use crate::event_history::{EventHistory, EventItem, HistorySize};

    impl EventItem for i16 {
        type ID = i16;

        fn get_id(&self) -> &Self::ID {
            self
        }
    }

    fn new_history(size: usize) -> EventHistory<i16> {
        EventHistory::new(HistorySize::try_from(size).unwrap())
    }

    #[test]
    fn can_add_events_to_history() {
        let mut event_history: EventHistory<i16> = new_history(4);
        event_history.add(0);
        event_history.add(1);
        event_history.add(2);
        event_history.add(3);

        assert!(event_history.events.len() == 4);
    }

    #[test]
    fn duplicates_are_not_added_to_history() {
        let mut event_history: EventHistory<i16> = new_history(4);
        assert!(event_history.add(0) == Some(&0));
        assert!(event_history.add(0).is_none());

        assert!(event_history.events.len() == 1);
    }

    #[test]
    fn cursor_tracks_head_if_not_moved() {
        let mut event_history: EventHistory<i16> = new_history(4);
        event_history.add(0);
        event_history.add(1);
        event_history.add(2);
        event_history.add(3);

        assert!(event_history.cursor == 3);
        assert!(event_history.head == 3);
    }

    #[test]
    fn cursor_cannot_move_forward_when_at_head() {
        let mut event_history: EventHistory<i16> = new_history(4);
        event_history.add(0);
        event_history.add(1);
        event_history.add(2);
        event_history.add(3);

        assert!(event_history.forward().is_none());
        assert!(event_history.cursor == 3);
    }

    #[test]
    fn cursor_can_move_back_when_at_head() {
        let mut event_history: EventHistory<i16> = new_history(4);
        event_history.add(0);
        event_history.add(1);
        event_history.add(2);
        event_history.add(3);

        assert!(event_history.backward() == Some(&2));
        assert!(event_history.cursor == 2);
    }

    #[test]
    fn cursor_cannot_move_past_history_start() {
        let mut event_history: EventHistory<i16> = new_history(4);
        event_history.add(0);
        event_history.add(1);
        event_history.add(2);
        event_history.add(3);

        assert!(event_history.history_start == 0);
        assert!(event_history.backward() == Some(&2));
        assert!(event_history.backward() == Some(&1));
        assert!(event_history.backward() == Some(&0));
        assert!(event_history.backward().is_none());
        assert!(event_history.cursor == 0);
    }

    #[test]
    fn cursor_can_move_back_and_forward() {
        let mut event_history: EventHistory<i16> = new_history(4);
        event_history.add(0);
        event_history.add(1);
        event_history.add(2);
        event_history.add(3);

        assert!(event_history.history_start == 0);
        assert!(event_history.backward() == Some(&2));
        assert!(event_history.backward() == Some(&1));
        assert!(event_history.backward() == Some(&0));
        assert!(event_history.backward().is_none());
        assert!(event_history.forward() == Some(&1));
        assert!(event_history.forward() == Some(&2));
        assert!(event_history.forward() == Some(&3));
        assert!(event_history.cursor == 3);
    }

    #[test]
    fn adding_event_with_detatched_cursor_truncates_history() {
        let mut event_history: EventHistory<i16> = new_history(4);
        event_history.add(0);
        event_history.add(1);
        event_history.add(2);
        event_history.add(3);

        assert!(event_history.backward() == Some(&2));
        assert!(event_history.backward() == Some(&1));

        assert_eq!(event_history.add(4), Some(&4));
        // length of events will not have changed
        assert!(event_history.events.len() == 4);
        assert!(event_history.cursor == 2);
        assert!(event_history.head == 2);
        assert!(event_history.forward().is_none());
    }

    #[test]
    fn adding_event_beyond_capacity_wraps_head_around_buffer() {
        let mut event_history: EventHistory<i16> = new_history(4);
        event_history.add(0);
        event_history.add(1);
        event_history.add(2);
        event_history.add(3);
        event_history.add(4);

        assert!(event_history.head == 0);
        assert!(event_history.cursor == 0);
    }

    #[test]
    fn cursor_move_will_make_same_id_ignored_on_next_add() {
        let mut event_history: EventHistory<i16> = new_history(4);
        event_history.add(0);
        event_history.add(1);
        event_history.add(2);
        event_history.add(3);
        event_history.add(4);

        assert!(event_history.backward() == Some(&3));
        println!("{:?}", event_history.ignored_events);
        assert!(event_history.add(3).is_none());
        println!("{:?}", event_history.ignored_events);
    }

    #[test]
    fn cursor_can_wrap_back_around_buffer() {
        let mut event_history: EventHistory<i16> = new_history(4);
        event_history.add(0);
        event_history.add(1);
        event_history.add(2);
        event_history.add(3);
        event_history.add(4);

        assert!(event_history.backward() == Some(&3));
    }

    #[test]
    fn adding_event_beyond_capacity_makes_history_dynamic() {
        let mut event_history: EventHistory<i16> = new_history(4);
        event_history.add(0);
        event_history.add(1);
        event_history.add(2);
        event_history.add(3);
        event_history.add(4);

        assert!(!event_history.static_history);
    }

    #[test]
    fn adding_event_beyond_capacity_moves_history_beyond_head() {
        let mut event_history: EventHistory<i16> = new_history(4);
        event_history.add(0);
        event_history.add(1);
        event_history.add(2);
        event_history.add(3);
        event_history.add(4);

        assert!(event_history.head == 0);
        assert!(event_history.history_start == 1);
    }

    #[test]
    fn cursor_stops_at_dynamic_history_start() {
        let mut event_history: EventHistory<i16> = new_history(4);
        event_history.add(0);
        event_history.add(1);
        event_history.add(2);
        event_history.add(3);
        event_history.add(4);

        assert!(event_history.backward() == Some(&3));
        assert!(event_history.backward() == Some(&2));
        assert!(event_history.backward() == Some(&1));
        assert!(event_history.backward().is_none());
    }

    #[test]
    fn adding_event_with_detatched_cursor_truncates_history_around_wrapped_buffer() {
        let mut event_history: EventHistory<i16> = new_history(4);
        event_history.add(0);
        event_history.add(1);
        event_history.add(2);
        event_history.add(3);
        event_history.add(4);
        event_history.add(5); // added to idx 2

        // buffer is [4, 5, 2. 3].
        assert!(event_history.backward() == Some(&4));
        assert!(event_history.backward() == Some(&3));
        assert!(event_history.add(6) == Some(&6));
        // buffer now [6, 5, 2, 3], only 2, 3 and 6 should be reachable
        assert!(!event_history.static_history);
        assert!(event_history.history_start == 2);
        assert!(event_history.head == 0);
        assert!(event_history.backward() == Some(&3));
        assert!(event_history.backward() == Some(&2));
        assert!(event_history.backward().is_none());
    }

    #[test]
    fn deactivated_events_are_skipped_when_navigating() {
        let mut event_history: EventHistory<i16> = new_history(3);
        event_history.add(10);
        event_history.add(20);
        event_history.add(30);

        event_history.deactivate(&20);

        assert_eq!(event_history.backward(), Some(&10));
        assert_eq!(event_history.cursor, 0);
        assert_eq!(event_history.forward(), Some(&30));
        assert_eq!(event_history.cursor, 2);
    }

    #[test]
    fn activate_restores_deactivated_event_to_navigation() {
        let mut event_history: EventHistory<i16> = new_history(3);
        event_history.add(10);
        event_history.add(20);
        event_history.add(30);

        event_history.deactivate(&20);

        // Moving backward skips the inactive event
        assert_eq!(event_history.backward(), Some(&10));
        assert_eq!(event_history.cursor, 0);

        event_history.activate(&20);

        assert_eq!(event_history.forward(), Some(&20));
        assert_eq!(event_history.cursor, 1);
        assert_eq!(event_history.forward(), Some(&30));
        assert_eq!(event_history.cursor, 2);
    }

    #[test]
    fn deactivate_and_activate_updates_all_indices_for_id() {
        let mut event_history: EventHistory<i16> = new_history(4);
        event_history.add(1);
        event_history.add(2);
        event_history.add(3);

        // Move the cursor back so we can re-add id 1 at a new index
        assert_eq!(event_history.backward(), Some(&2));
        event_history.add(1);

        // Both indices for id 1 should be marked inactive
        event_history.deactivate(&1);
        assert!(matches!(event_history.events[0], EventStatus::Inactive(1)));
        assert!(matches!(event_history.events[2], EventStatus::Inactive(1)));

        // Cursor moves to the only active event, so no further backward navigation is possible
        assert_eq!(event_history.cursor, 1);
        assert_eq!(event_history.backward(), None);

        // Reactivating the id should restore all entries
        event_history.activate(&1);
        assert!(matches!(event_history.events[0], EventStatus::Active(1)));
        assert!(matches!(event_history.events[2], EventStatus::Active(1)));

        // Navigating backward from head can reach both active entries again
        event_history.cursor = event_history.head;
        assert_eq!(event_history.backward(), Some(&2));
        assert_eq!(event_history.backward(), Some(&1));
    }

    #[test]
    fn activate_restores_navigation_for_deactivated_event() {
        let mut event_history: EventHistory<i16> = new_history(4);
        event_history.add(10); // idx 0
        event_history.add(20); // idx 1
        event_history.add(30); // idx 2
        event_history.add(40); // idx 3 (head)

        event_history.deactivate(&20);
        assert_eq!(event_history.cursor, 3);
        assert_eq!(event_history.backward(), Some(&30));
        assert_eq!(event_history.backward(), Some(&10));
        assert_eq!(event_history.backward(), None);

        event_history.activate(&20);
        event_history.cursor = event_history.head;

        assert_eq!(event_history.backward(), Some(&30));
        assert_eq!(event_history.backward(), Some(&20));
        assert_eq!(event_history.backward(), Some(&10));
        assert_eq!(event_history.backward(), None);
    }

    #[test]
    fn activating_duplicate_id_restores_all_entries_to_navigation() {
        let mut event_history: EventHistory<i16> = new_history(4);
        event_history.add(1); // idx 0
        event_history.add(2); // idx 1
        event_history.add(1); // idx 2
        event_history.add(3); // idx 3 (head)

        event_history.deactivate(&1);
        assert!(matches!(event_history.events[0], EventStatus::Inactive(1)));
        assert!(matches!(event_history.events[2], EventStatus::Inactive(1)));

        event_history.cursor = event_history.head;
        assert_eq!(event_history.backward(), Some(&2));
        assert_eq!(event_history.backward(), None);

        event_history.activate(&1);
        event_history.cursor = event_history.head;

        assert_eq!(event_history.backward(), Some(&1));
        assert_eq!(event_history.backward(), Some(&2));
        assert_eq!(event_history.backward(), Some(&1));
        assert_eq!(event_history.backward(), None);
    }

    #[test]
    fn combined_deactivate_remove_then_activate_restores_navigation() {
        let mut event_history: EventHistory<i16> = new_history(5);
        event_history.add(1); // idx 0
        event_history.add(2); // idx 1
        event_history.add(1); // idx 2
        event_history.add(3); // idx 3
        event_history.add(4); // idx 4 (head)

        event_history.deactivate(&1);
        event_history.remove(&2);

        assert!(matches!(event_history.events[0], EventStatus::Inactive(1)));
        assert!(matches!(event_history.events[1], EventStatus::Deleted));
        assert!(matches!(event_history.events[2], EventStatus::Inactive(1)));
        assert!(matches!(event_history.events[3], EventStatus::Active(3)));
        assert!(matches!(event_history.events[4], EventStatus::Active(4)));
        assert_eq!(event_history.event_status_count.active, 2);
        assert_eq!(event_history.event_status_count.inactive, 2);
        assert_eq!(event_history.event_status_count.deleted, 1);

        // With only 3 and 4 active, navigation skips inactive/deleted entries.
        assert_eq!(event_history.cursor, 4);
        assert_eq!(event_history.backward(), Some(&3));
        assert_eq!(event_history.backward(), None);

        event_history.activate(&1);
        assert!(matches!(event_history.events[0], EventStatus::Active(1)));
        assert!(matches!(event_history.events[2], EventStatus::Active(1)));

        // After activation, 1 is reachable and duplicate entries are skipped when navigating.
        event_history.cursor = 3;
        assert_eq!(event_history.backward(), Some(&1));
        assert_eq!(event_history.backward(), None);

        event_history.cursor = 2;
        assert_eq!(event_history.backward(), None);
    }

    #[test]
    fn navigation_skips_duplicates_after_deactivate() {
        let mut event_history: EventHistory<i16> = new_history(4);
        event_history.add(1); // idx 0
        event_history.add(2); // idx 1
        event_history.add(1); // idx 2
        event_history.add(3); // idx 3 (head)

        // Deactivating id 2 makes the two active 1s adjacent when moving backward.
        event_history.deactivate(&2);

        assert_eq!(event_history.cursor, 3);
        assert_eq!(event_history.backward(), Some(&1));
        // Duplicate 1 at idx 0 is skipped, and we stop before wrapping to head.
        assert_eq!(event_history.backward(), None);
    }

    #[test]
    fn navigation_skips_duplicates_after_remove() {
        let mut event_history: EventHistory<i16> = new_history(4);
        event_history.add(1); // idx 0
        event_history.add(2); // idx 1
        event_history.add(1); // idx 2
        event_history.add(3); // idx 3 (head)

        // Removing id 2 makes the two active 1s adjacent when moving backward.
        event_history.remove(&2);

        assert_eq!(event_history.cursor, 3);
        assert_eq!(event_history.backward(), Some(&1));
        // Duplicate 1 at idx 0 is skipped, and we stop before wrapping to head.
        assert_eq!(event_history.backward(), None);
    }

    #[test]
    fn navigation_returns_none_when_all_inactive() {
        let mut event_history: EventHistory<i16> = new_history(3);
        event_history.add(10);
        event_history.add(20);
        event_history.add(30);

        event_history.deactivate(&10);
        event_history.deactivate(&20);
        event_history.deactivate(&30);

        assert!(matches!(event_history.events[0], EventStatus::Inactive(10)));
        assert!(matches!(event_history.events[1], EventStatus::Inactive(20)));
        assert!(matches!(event_history.events[2], EventStatus::Inactive(30)));

        assert_eq!(event_history.forward(), None);
        assert_eq!(event_history.backward(), None);
        assert_eq!(event_history.cursor, event_history.head);

        // Adding after full deactivation should still work and reset head/cursor to new entry
        assert_eq!(event_history.add(40), Some(&40));
        assert!(matches!(
            event_history.events[event_history.cursor],
            EventStatus::Active(40)
        ));
    }

    #[test]
    fn inactive_entries_are_skipped_after_wraparound() {
        let mut event_history: EventHistory<i16> = new_history(3);
        event_history.add(10);
        event_history.add(20);
        event_history.add(30);

        event_history.deactivate(&20);

        // Adding wraps the buffer and advances history_start
        assert_eq!(event_history.add(40), Some(&40));
        assert_eq!(event_history.head, 0);
        assert_eq!(event_history.history_start, 1);
        assert!(!event_history.static_history);

        // Backward should skip inactive 20 and wrap to 30
        assert_eq!(event_history.backward(), Some(&30));
        assert_eq!(event_history.cursor, 2);

        // Forward from wrapped position should return to 40, still skipping inactive 20
        assert_eq!(event_history.forward(), Some(&40));
        assert_eq!(event_history.cursor, 0);
        assert_eq!(event_history.forward(), None);
    }

    #[test]
    fn remove_marks_active_entries_as_deleted_and_skips_them() {
        let mut event_history: EventHistory<i16> = new_history(4);
        event_history.add(10); // idx 0
        event_history.add(20); // idx 1
        event_history.add(30); // idx 2
        event_history.add(40); // idx 3

        assert_eq!(event_history.event_status_count.active, 4);

        event_history.remove(&20);

        assert!(matches!(event_history.events[1], EventStatus::Deleted));
        assert_eq!(event_history.event_status_count.active, 3);
        assert_eq!(event_history.event_status_count.deleted, 1);

        // Navigating backward skips the deleted entry at idx 1.
        assert_eq!(event_history.backward(), Some(&30));
        assert_eq!(event_history.cursor, 2);
        assert_eq!(event_history.forward(), Some(&40));
        assert_eq!(event_history.cursor, 3);
    }
}
