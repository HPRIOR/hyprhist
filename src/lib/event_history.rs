use std::collections::{HashSet, VecDeque};
use std::mem::{self};
use std::num::NonZeroUsize;
use std::str::FromStr;

use log::{debug, info};

use crate::types::EventItem;

#[derive(Debug)]
enum EventStatus<T> {
    Active(T),
    Inactive(T),
    Deleted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CursorRealignment {
    PreviousActive(usize),
    NextActive(usize),
    LastInactive(usize),
    ResetToStart(usize),
}

impl<T: Clone> Default for EventStatus<T> {
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

#[derive(Debug)]
pub struct EventHistory<T: EventItem> {
    max_size: HistorySize,
    cursor: usize,
    events: VecDeque<EventStatus<T>>,
    ignored_events: HashSet<T::ID>,
}

impl<T: EventItem> EventHistory<T> {
    #[must_use]
    pub fn new(max_size: HistorySize) -> Self {
        info!("Creating event history with max_size: {}", max_size.get());
        Self {
            max_size,
            cursor: 0,
            events: VecDeque::new(),
            ignored_events: HashSet::default(),
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

    fn next_active_idx(&self, current: usize, current_id: Option<&T::ID>) -> Option<usize> {
        let mut idx = current + 1;
        while let Some(event) = self.events.get(idx) {
            if let EventStatus::Active(event) = event {
                match current_id {
                    Some(id) if id != event.get_id() => {
                        return Some(idx);
                    }
                    _ => {}
                }
            }
            idx += 1;
        }
        None
    }

    fn prev_active_idx(&self, current: usize, current_id: Option<&T::ID>) -> Option<usize> {
        let mut idx = current;
        while idx > 0 {
            idx -= 1;
            if let Some(EventStatus::Active(event)) = self.events.get(idx) {
                match current_id {
                    Some(id) if id != event.get_id() => {
                        return Some(idx);
                    }
                    _ => {}
                }
            }
        }
        None
    }

    fn update_matching_events<F>(
        &mut self,
        id: &T::ID,
        mut on_match: F,
    ) -> Option<CursorRealignment>
    where
        F: FnMut(EventStatus<T>) -> Option<EventStatus<T>>,
    {
        let mut changed_at_cursor = false;
        let mut id_at_updated_cursor = None;

        for (idx, event) in &mut self.events.iter_mut().enumerate() {
            let id_matching = event
                .get_event()
                .map_or_else(|| false, |e| e.get_id() == id);
            if id_matching {
                let previous_event = mem::replace(event, EventStatus::Deleted);

                if idx == self.cursor {
                    changed_at_cursor = true;
                    id_at_updated_cursor = previous_event.get_event().map(T::get_id).cloned();
                }

                let new_status = on_match(previous_event);

                if let Some(new_status) = new_status {
                    *event = new_status;
                }
            }
        }

        if !changed_at_cursor {
            return None;
        }

        if let Some(prev_active_idx) =
            self.prev_active_idx(self.cursor, id_at_updated_cursor.as_ref())
        {
            self.cursor = prev_active_idx;
            return Some(CursorRealignment::PreviousActive(prev_active_idx));
        }

        if let Some(next_active_idx) =
            self.next_active_idx(self.cursor, id_at_updated_cursor.as_ref())
        {
            self.cursor = next_active_idx;
            return Some(CursorRealignment::NextActive(next_active_idx));
        }

        if let Some((last_inactive_idx, _)) = self
            .events
            .iter()
            .enumerate()
            .rev()
            .find(|(_, status)| matches!(status, EventStatus::Inactive(_)))
        {
            self.cursor = last_inactive_idx;
            return Some(CursorRealignment::LastInactive(last_inactive_idx));
        }

        self.cursor = 0;
        Some(CursorRealignment::ResetToStart(0))
    }

    #[allow(clippy::missing_panics_doc)]
    pub fn current_event(&mut self) -> &T {
        let current_event = self.events[self.cursor].get_event().unwrap();
        self.ignored_events.insert(current_event.get_id().clone());
        current_event
    }

    pub fn forward(&mut self) -> Option<&T> {
        let current_id = self
            .events
            .get(self.cursor)
            .and_then(EventStatus::get_event)
            .map(T::get_id);

        let new_cursor_position = self.next_active_idx(self.cursor, current_id)?;
        self.cursor = new_cursor_position;
        let current_event: &T = self.events[new_cursor_position].get_event()?;
        self.ignored_events.insert(current_event.get_id().clone());
        debug!(
            "Forward invoked; cursor moved to {new_cursor_position} with id {}; {} inserted into ignore set.",
            current_event.get_id(),
            current_event.get_id(),
        );
        Some(current_event)
    }

    pub fn backward(&mut self) -> Option<&T> {
        let current_id = self
            .events
            .get(self.cursor)
            .and_then(EventStatus::get_event)
            .map(T::get_id);

        let new_cursor_position = self.prev_active_idx(self.cursor, current_id)?;

        self.cursor = new_cursor_position;
        let current_event: &T = self.events[new_cursor_position].get_event()?;
        self.ignored_events.insert(current_event.get_id().clone());
        debug!(
            "Backward invoked; cursor moved to {new_cursor_position} with id {}; {} inserted into ignore set.",
            current_event.get_id(),
            current_event.get_id(),
        );
        Some(current_event)
    }

    pub fn remove(&mut self, id: &T::ID) {
        info!("Removing event with id {id}");
        if let Some(realignment) = self.update_matching_events(id, |_| None) {
            match realignment {
                CursorRealignment::PreviousActive(idx) => {
                    debug!(
                        "Cursor present in deleted events, moving to previous active event at position {idx}"
                    );
                }
                CursorRealignment::NextActive(idx) => {
                    debug!(
                        "Cursor present in deleted events, moving to next active event at position {idx}"
                    );
                }
                CursorRealignment::LastInactive(idx) => {
                    debug!(
                        "Cursor present in deleted events, moving to last inactive event at position {idx}"
                    );
                }
                CursorRealignment::ResetToStart(idx) => {
                    debug!(
                        "Cursor present in deleted events, no active or inactive entries found; moving to {idx}"
                    );
                }
            }
        }
    }

    pub fn deactivate(&mut self, id: &T::ID) {
        info!("Deactivating event with id {id}");
        if let Some(realignment) =
            self.update_matching_events(id, |previous_event| match previous_event {
                EventStatus::Active(t) => Some(EventStatus::Inactive(t)),
                _ => None,
            })
        {
            match realignment {
                CursorRealignment::PreviousActive(idx) => {
                    debug!(
                        "Cursor present in deactivated events, moving to previous active event at poistion {idx}"
                    );
                }
                CursorRealignment::NextActive(idx) => {
                    debug!(
                        "Cursor present in deactivated events, moving to next active event at poistion {idx}"
                    );
                }
                CursorRealignment::LastInactive(idx) => {
                    debug!(
                        "Cursor present in deactivated events, moving to last inactive event at position {idx}"
                    );
                }
                CursorRealignment::ResetToStart(idx) => {
                    debug!(
                        "Cursor present in deactivated events, no active or inactive entries found; moving to {idx}"
                    );
                }
            }
        }
    }

    pub fn activate(&mut self, id: &T::ID) {
        info!("Activating event with id {id}");
        for event in &mut self.events {
            let event_matches_id = event
                .get_event()
                .map_or_else(|| false, |e| e.get_id() == id);
            if event_matches_id {
                let previous_event = mem::replace(event, EventStatus::Deleted);
                if let EventStatus::Inactive(t) | EventStatus::Active(t) = previous_event {
                    *event = EventStatus::Active(t);
                }
            }
        }
    }

    pub fn add(&mut self, item: T) -> Option<&T> {
        if self.ignored_events.contains(item.get_id()) {
            debug!(
                "Ignoring event with {}; present in ignore set: {:?}.",
                item.get_id(),
                self.ignored_events,
            );
            self.ignored_events.remove(item.get_id());
            return None;
        }

        let is_duplicate_item = match &self.events.get(self.cursor) {
            Some(EventStatus::Active(current)) => current.get_id() == item.get_id(),
            _ => false,
        };

        if is_duplicate_item {
            info!("Add skipped; duplicate item at cursor {}", self.cursor);
            return None;
        }

        let active_item = EventStatus::Active(item);

        if self.events.is_empty() {
            self.events.push_back(active_item);
            self.cursor = 0;
            return self.events.back().and_then(EventStatus::get_event);
        }

        let buffer_full = self.events.len() >= self.max_size.get();

        if buffer_full {
            self.events.pop_front();
            if self.cursor > 0 {
                self.cursor -= 1;
            }
        }

        let cursor_detached = !self.events.is_empty() && self.cursor + 1 != self.events.len();

        if cursor_detached {
            let mut idx = self.events.len() - 1;
            while idx > self.cursor {
                self.events.pop_back();
                idx -= 1;
            }
        }

        self.events.push_back(active_item);
        self.cursor = self.events.len() - 1;

        self.events[self.events.len() - 1].get_event()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{HashSet, VecDeque};

    use super::{EventHistory, EventItem, EventStatus, HistorySize};

    impl EventItem for i32 {
        type ID = i32;

        fn get_id(&self) -> &Self::ID {
            self
        }
    }

    fn new_history(size: usize) -> EventHistory<i32> {
        EventHistory::new(HistorySize::try_from(size).expect("size must be non-zero"))
    }

    fn manual_history(events: Vec<EventStatus<i32>>, cursor: usize) -> EventHistory<i32> {
        EventHistory {
            max_size: HistorySize::try_from(events.len().max(1)).expect("size must be non-zero"),
            cursor,
            events: VecDeque::from(events),
            ignored_events: HashSet::new(),
        }
    }

    #[test]
    fn adds_under_max_size() {
        let mut history = new_history(3);
        history.add(1);
        history.add(2);

        assert_eq!(history.events.len(), 2);
        assert_eq!(history.cursor, 1);
        assert!(matches!(history.events[0], EventStatus::Active(1)));
        assert!(matches!(history.events[1], EventStatus::Active(2)));
    }

    #[test]
    fn adds_beyond_max_size_evict_oldest() {
        let mut history = new_history(3);
        history.add(1);
        history.add(2);
        history.add(3);

        history.add(4);

        assert_eq!(history.events.len(), 3);
        assert!(matches!(history.events[0], EventStatus::Active(2)));
        assert!(matches!(history.events[1], EventStatus::Active(3)));
        assert!(matches!(history.events[2], EventStatus::Active(4)));
        assert_eq!(history.cursor, 2);
    }

    #[test]
    fn append_at_head_moves_cursor_to_new_event() {
        let mut history = new_history(4);
        history.add(1);
        history.add(2);
        assert_eq!(history.cursor, 1);

        let added = history.add(3);

        assert!(matches!(added, Some(&3)));
        assert_eq!(history.cursor, 2);
        assert!(matches!(history.events[2], EventStatus::Active(3)));
    }

    #[test]
    fn detached_cursor_truncates_and_appends() {
        let mut history = new_history(5);
        history.add(1);
        history.add(2);
        history.add(3);
        history.add(4);

        history.cursor = 2; // detach cursor from head
        let added = history.add(6);

        assert!(matches!(added, Some(&6)));
        assert_eq!(history.events.len(), 4);
        assert!(matches!(history.events[0], EventStatus::Active(1)));
        assert!(matches!(history.events[1], EventStatus::Active(2)));
        assert!(matches!(history.events[2], EventStatus::Active(3)));
        assert!(matches!(history.events[3], EventStatus::Active(6)));
        assert_eq!(history.cursor, 3);
    }

    #[test]
    fn contiguous_duplicates_are_ignored() {
        let mut history = new_history(3);
        assert!(history.add(1).is_some());
        assert!(history.add(1).is_none());

        assert_eq!(history.events.len(), 1);
        assert!(matches!(history.events[0], EventStatus::Active(1)));
    }

    #[test]
    fn ignored_events_are_skipped_and_removed_from_set() {
        let mut history = new_history(3);
        history.ignored_events.insert(5);

        assert!(history.add(5).is_none());
        assert!(!history.ignored_events.contains(&5));
        assert!(history.events.is_empty());
    }

    #[test]
    fn forward_moves_cursor_when_not_at_head() {
        let mut history = new_history(4);
        history.add(1);
        history.add(2);
        history.add(3);
        history.cursor = 1;

        let result = history.forward();

        assert!(matches!(result, Some(&3)));
        assert_eq!(history.cursor, 2);
    }

    #[test]
    fn forward_does_not_move_at_head() {
        let mut history = new_history(3);
        history.add(1);
        history.add(2);
        history.add(3);
        history.cursor = history.events.len() - 1;

        let result = history.forward();

        assert!(result.is_none());
        assert_eq!(history.cursor, history.events.len() - 1);
    }

    #[test]
    fn forward_skips_inactive_and_deleted_events() {
        let mut history = manual_history(
            vec![
                EventStatus::Active(1),
                EventStatus::Inactive(2),
                EventStatus::Active(3),
            ],
            0,
        );

        let result = history.forward();

        assert!(matches!(result, Some(&3)));
        assert_eq!(history.cursor, 2);

        let mut history_deleted = manual_history(
            vec![
                EventStatus::Active(1),
                EventStatus::Deleted,
                EventStatus::Active(3),
            ],
            0,
        );
        let result_deleted = history_deleted.forward();

        assert!(matches!(result_deleted, Some(&3)));
        assert_eq!(history_deleted.cursor, 2);
    }

    #[test]
    fn forward_stops_when_only_inactive_ahead() {
        let mut history = manual_history(vec![EventStatus::Active(1), EventStatus::Inactive(2)], 0);

        let result = history.forward();

        assert!(result.is_none());
        assert_eq!(history.cursor, 0);
    }

    #[test]
    fn forward_stops_when_only_deleted_ahead() {
        let mut history = manual_history(vec![EventStatus::Active(1), EventStatus::Deleted], 0);

        let result = history.forward();

        assert!(result.is_none());
        assert_eq!(history.cursor, 0);
    }

    #[test]
    fn forward_adds_new_cursor_item_to_ignore_set() {
        let mut history = new_history(3);
        history.add(1);
        history.add(2);
        history.add(3);
        history.cursor = 1;
        history.ignored_events.clear();

        let result = history.forward();

        assert!(matches!(result, Some(&3)));
        assert!(history.ignored_events.contains(&3));
    }

    #[test]
    fn forward_skips_duplicate_active_entries() {
        let mut history = manual_history(
            vec![
                EventStatus::Active(1),
                EventStatus::Active(2),
                EventStatus::Active(2),
                EventStatus::Active(3),
            ],
            1,
        );

        let result = history.forward();

        assert!(matches!(result, Some(&3)));
        assert_eq!(history.cursor, 3);
    }

    #[test]
    fn backward_moves_cursor_when_not_at_start() {
        let mut history = new_history(4);
        history.add(1);
        history.add(2);
        history.add(3);
        history.cursor = 2;

        let result = history.backward();

        assert!(matches!(result, Some(&2)));
        assert_eq!(history.cursor, 1);
    }

    #[test]
    fn backward_does_not_move_at_start() {
        let mut history = new_history(2);
        history.add(1);
        history.cursor = 0;

        let result = history.backward();

        assert!(result.is_none());
        assert_eq!(history.cursor, 0);
    }

    #[test]
    fn backward_skips_inactive_and_deleted_events() {
        let mut history = manual_history(
            vec![
                EventStatus::Active(1),
                EventStatus::Inactive(2),
                EventStatus::Active(3),
            ],
            2,
        );

        let result = history.backward();

        assert!(matches!(result, Some(&1)));
        assert_eq!(history.cursor, 0);

        let mut history_deleted = manual_history(
            vec![
                EventStatus::Active(1),
                EventStatus::Deleted,
                EventStatus::Active(3),
            ],
            2,
        );
        let result_deleted = history_deleted.backward();

        assert!(matches!(result_deleted, Some(&1)));
        assert_eq!(history_deleted.cursor, 0);
    }

    #[test]
    fn backward_stops_when_only_inactive_behind() {
        let mut history = manual_history(
            vec![
                EventStatus::Inactive(1),
                EventStatus::Inactive(2),
                EventStatus::Active(3),
            ],
            2,
        );

        let result = history.backward();

        assert!(result.is_none());
        assert_eq!(history.cursor, 2);
    }

    #[test]
    fn backward_stops_when_only_deleted_behind() {
        let mut history = manual_history(
            vec![
                EventStatus::Deleted,
                EventStatus::Deleted,
                EventStatus::Active(3),
            ],
            2,
        );

        let result = history.backward();

        assert!(result.is_none());
        assert_eq!(history.cursor, 2);
    }

    #[test]
    fn backward_adds_new_cursor_item_to_ignore_set() {
        let mut history = new_history(3);
        history.add(1);
        history.add(2);
        history.add(3);
        history.cursor = 2;
        history.ignored_events.clear();

        let result = history.backward();

        assert!(matches!(result, Some(&2)));
        assert!(history.ignored_events.contains(&2));
    }

    #[test]
    fn backward_skips_duplicate_active_entries() {
        let mut history = manual_history(
            vec![
                EventStatus::Active(1),
                EventStatus::Active(2),
                EventStatus::Active(2),
                EventStatus::Active(3),
            ],
            2,
        );

        let result = history.backward();

        assert!(matches!(result, Some(&1)));
        assert_eq!(history.cursor, 0);
    }

    #[test]
    fn remove_deletes_single_event() {
        let mut history = manual_history(
            vec![
                EventStatus::Active(1),
                EventStatus::Active(2),
                EventStatus::Active(3),
            ],
            0,
        );

        history.remove(&2);

        assert!(matches!(history.events[0], EventStatus::Active(1)));
        assert!(matches!(history.events[1], EventStatus::Deleted));
        assert!(matches!(history.events[2], EventStatus::Active(3)));
    }

    #[test]
    fn remove_deletes_multiple_events() {
        let mut history = manual_history(
            vec![
                EventStatus::Active(1),
                EventStatus::Active(2),
                EventStatus::Active(3),
                EventStatus::Active(2),
            ],
            0,
        );

        history.remove(&2);

        assert!(matches!(history.events[0], EventStatus::Active(1)));
        assert!(matches!(history.events[1], EventStatus::Deleted));
        assert!(matches!(history.events[2], EventStatus::Active(3)));
        assert!(matches!(history.events[3], EventStatus::Deleted));
    }

    #[test]
    fn remove_does_not_move_cursor_when_not_on_removed_idx() {
        let mut history = manual_history(
            vec![
                EventStatus::Active(1),
                EventStatus::Active(2),
                EventStatus::Active(3),
            ],
            0,
        );

        history.remove(&2);

        assert_eq!(history.cursor, 0);
    }

    #[test]
    fn remove_moves_cursor_to_previous_active_when_on_removed_idx() {
        let mut history = manual_history(
            vec![
                EventStatus::Active(1),
                EventStatus::Active(2),
                EventStatus::Active(3),
            ],
            1,
        );

        history.remove(&2);

        assert_eq!(history.cursor, 0);
    }

    #[test]
    fn remove_moves_cursor_to_next_active_when_no_previous_active() {
        let mut history = manual_history(vec![EventStatus::Active(1), EventStatus::Active(2)], 0);

        history.remove(&1);

        assert_eq!(history.cursor, 1);
    }

    #[test]
    fn remove_moves_cursor_to_last_inactive_when_no_active_exists() {
        let mut history = manual_history(
            vec![
                EventStatus::Active(1),
                EventStatus::Inactive(2),
                EventStatus::Inactive(3),
            ],
            0,
        );

        history.remove(&1);

        assert_eq!(history.cursor, 2);
    }

    #[test]
    fn remove_moves_cursor_to_zero_when_no_active_or_inactive_remain() {
        let mut history = manual_history(
            vec![
                EventStatus::Active(1),
                EventStatus::Deleted,
                EventStatus::Deleted,
            ],
            0,
        );

        history.remove(&1);

        assert_eq!(history.cursor, 0);
    }

    #[test]
    fn deactivate_deactivates_single_event() {
        let mut history = manual_history(
            vec![
                EventStatus::Active(1),
                EventStatus::Active(2),
                EventStatus::Active(3),
            ],
            0,
        );

        history.deactivate(&2);

        assert!(matches!(history.events[0], EventStatus::Active(1)));
        assert!(matches!(history.events[1], EventStatus::Inactive(2)));
        assert!(matches!(history.events[2], EventStatus::Active(3)));
    }

    #[test]
    fn deactivate_deactivates_multiple_events() {
        let mut history = manual_history(
            vec![
                EventStatus::Active(1),
                EventStatus::Active(2),
                EventStatus::Active(3),
                EventStatus::Active(2),
            ],
            0,
        );

        history.deactivate(&2);

        assert!(matches!(history.events[0], EventStatus::Active(1)));
        assert!(matches!(history.events[1], EventStatus::Inactive(2)));
        assert!(matches!(history.events[2], EventStatus::Active(3)));
        assert!(matches!(history.events[3], EventStatus::Inactive(2)));
    }

    #[test]
    fn deactivate_does_not_move_cursor_when_not_on_deactivated_idx() {
        let mut history = manual_history(
            vec![
                EventStatus::Active(1),
                EventStatus::Active(2),
                EventStatus::Active(3),
            ],
            0,
        );

        history.deactivate(&2);

        assert_eq!(history.cursor, 0);
    }

    #[test]
    fn deactivate_moves_cursor_to_previous_active_when_on_deactivated_idx() {
        let mut history = manual_history(
            vec![
                EventStatus::Active(1),
                EventStatus::Active(2),
                EventStatus::Active(3),
            ],
            1,
        );

        history.deactivate(&2);

        assert_eq!(history.cursor, 0);
    }

    #[test]
    fn deactivate_moves_cursor_to_next_active_when_no_previous_active() {
        let mut history = manual_history(vec![EventStatus::Active(1), EventStatus::Active(2)], 0);

        history.deactivate(&1);

        assert_eq!(history.cursor, 1);
    }

    #[test]
    fn deactivate_moves_cursor_to_last_inactive_when_no_active_exists() {
        let mut history = manual_history(
            vec![
                EventStatus::Active(1),
                EventStatus::Inactive(2),
                EventStatus::Deleted,
            ],
            0,
        );

        history.deactivate(&1);

        assert_eq!(history.cursor, 1);
    }

    #[test]
    fn deactivate_moves_cursor_to_zero_when_no_active_or_inactive_remain() {
        let mut history = manual_history(vec![EventStatus::Active(1)], 0);

        history.deactivate(&1);

        assert_eq!(history.cursor, 0);
    }

    #[test]
    fn activate_activates_single_event() {
        let mut history = manual_history(
            vec![
                EventStatus::Inactive(1),
                EventStatus::Active(2),
                EventStatus::Active(3),
            ],
            1,
        );

        let _ = history.activate(&1);

        assert!(matches!(history.events[0], EventStatus::Active(1)));
        assert!(matches!(history.events[1], EventStatus::Active(2)));
        assert!(matches!(history.events[2], EventStatus::Active(3)));
    }

    #[test]
    fn activate_activates_multiple_events() {
        let mut history = manual_history(
            vec![
                EventStatus::Inactive(1),
                EventStatus::Active(2),
                EventStatus::Inactive(1),
                EventStatus::Deleted,
                EventStatus::Inactive(3),
            ],
            1,
        );

        let _ = history.activate(&1);
        let _ = history.activate(&3);

        assert!(matches!(history.events[0], EventStatus::Active(1)));
        assert!(matches!(history.events[1], EventStatus::Active(2)));
        assert!(matches!(history.events[2], EventStatus::Active(1)));
        assert!(matches!(history.events[3], EventStatus::Deleted));
        assert!(matches!(history.events[4], EventStatus::Active(3)));
    }
}
