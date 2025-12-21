use std::collections::HashSet;
use std::num::NonZeroUsize;
use std::str::FromStr;

use log::{debug, info};

use crate::types::HasId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HistorySize(NonZeroUsize);

impl HistorySize {
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

pub struct EventHistory<T: HasId> {
    max_size: HistorySize,
    /// Moveable cursor in the event history. Can never exceed head, but can 'detatch' from head
    /// when tracking back through the event history.
    cursor: usize,
    /// The position of the latest event history. If an event occurs while `cursor` is
    /// detatched, the new event will be placed ahead `cursor`, and head will track this
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
    events: Vec<T>,
    ignored_events: HashSet<T::ID>,
}

impl<T: HasId> EventHistory<T> {
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

    fn next_idx(&self, current: usize) -> usize {
        if current == self.max_size.get() - 1 {
            0
        } else {
            current + 1
        }
    }
    fn prev_idx(&self, current: usize) -> usize {
        if current == 0 {
            self.max_size.get() - 1
        } else {
            current - 1
        }
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
        if self.cursor == self.head {
            // Already at the latest event
            info!("Forward move ignored; cursor already at head {}", self.head);
            return None;
        }

        let new_cursor_position = self.next_idx(self.cursor);
        self.cursor = new_cursor_position;
        let current_event: &T = &self.events[new_cursor_position];
        self.ignored_events.insert(current_event.get_id().clone());
        debug!(
            "Forward invoked; cursor moved to {new_cursor_position} with id {}; {} inserted into ignore set",
            current_event.get_id(),
            current_event.get_id()
        );
        Some(current_event)
    }

    pub fn backward(&mut self) -> Option<&T> {
        if self.cursor == self.history_start {
            info!(
                "Backward move ignored; cursor at history_start {}",
                self.history_start
            );
            return None;
        }

        let new_cursor_position = self.prev_idx(self.cursor);
        if self.in_valid_range(new_cursor_position) {
            self.cursor = new_cursor_position;
            let current_event: &T = &self.events[new_cursor_position];
            self.ignored_events.insert(current_event.get_id().clone());
            debug!(
                "Backward invoked; cursor moved to {new_cursor_position} with id {}; {} inserted into ignore set",
                current_event.get_id(),
                current_event.get_id()
            );
            Some(current_event)
        } else {
            info!("Backward move blocked; idx {new_cursor_position} outside valid range");
            None
        }
    }

    pub fn add(&mut self, item: T) -> Option<&T> {
        if self.ignored_events.contains(item.get_id()) {
            debug!(
                "Ignoring event with {}; present in ignore set: {:?}",
                item.get_id(),
                self.ignored_events
            );
            self.ignored_events.remove(item.get_id());
            return None;
        }

        let is_duplicate_item = self
            .events
            .get(self.cursor)
            .is_some_and(|current_item| item.get_id() == current_item.get_id());

        if is_duplicate_item {
            info!(
                "Add skipped; duplicate item at cursor {} (head {})",
                self.cursor, self.head
            );
            return None;
        }

        // When the cursor is detatched and history is not static we want the
        // history to start from the previous head posiition to prevent reads into 'dead' regions
        // of the buffer when cursor tracks backwards.
        let cursor_is_detached = self.head != self.cursor;
        if cursor_is_detached && !self.static_history {
            let new_start = self.next_idx(self.head);
            info!(
                "History truncated after detached cursor; history_start: {} -> {}",
                self.history_start, new_start
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

        // When a new item is added, regardless of detatched head we'll want cursor to be
        // incremented by 1, and for head to track with it. If the cursor is detached, the head
        // will now track back to cursor. Otherwise, they'll both just move forward by one.
        self.cursor = insert_idx;
        self.head = self.cursor;

        // This catches the case where history is and is not static. In either case, if head
        // catchus up with history start, history will no longer be static, history start should
        // track just ahead of head.
        let buffer_full = self.events.len() == self.max_size.get();
        if buffer_full && self.head == self.history_start {
            self.static_history = false;
            let new_start = self.next_idx(self.history_start);
            debug!(
                "History buffer full; advancing history_start {} -> {}",
                self.history_start, new_start
            );
            self.history_start = self.next_idx(self.history_start);
        }

        let result = if insert_idx == self.events.len() {
            self.events.push(item);
            Some(&self.events[self.events.len() - 1])
        } else {
            self.events[insert_idx] = item;
            Some(&self.events[insert_idx])
        };

        if let Some(result) = result {
            info!(
                "Event added at idx {}; id={}; cursor={}, head={}, history_start={}, static_history={}",
                insert_idx,
                result.get_id(),
                self.cursor,
                self.head,
                self.history_start,
                self.static_history
            );
        }

        result
    }
}

#[cfg(test)]
mod test {
    use std::collections::HashSet;

    use crate::event_history::{EventHistory, HasId, HistorySize};

    impl HasId for i16 {
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
}
