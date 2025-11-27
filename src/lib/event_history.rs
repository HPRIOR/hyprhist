trait HasId {
    type ID: Eq + PartialEq;
    fn get_id(&self) -> Self::ID;
}

struct EventHistory<T: HasId> {
    max_size: usize,
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
}

impl<T: HasId> EventHistory<T> {
    fn new(max_size: usize) -> Self {
        Self {
            max_size,
            cursor: 0,
            head: 0,
            history_start: 0,
            static_history: true,
            events: Vec::with_capacity(max_size),
        }
    }

    fn next_idx(&self, current: usize) -> usize {
        if current == self.max_size - 1 {
            0
        } else {
            current + 1
        }
    }
    fn prev_idx(&self, current: usize) -> usize {
        if current == 0 {
            self.max_size - 1
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

    pub fn next_ref(&self) -> Option<&T> {
        if self.cursor == self.head {
            // Already at the latest event
            return None;
        }

        let new_cursor_position = self.next_idx(self.cursor);
        Some(&self.events[new_cursor_position])
    }

    pub fn prev_ref(&self) -> Option<&T> {
        let new_cursor_position = self.prev_idx(self.cursor);
        // Wrapping has occured
        if self.in_valid_range(new_cursor_position) {
            Some(&self.events[new_cursor_position])
        } else {
            None
        }
    }

    pub fn add(&mut self, item: T) {
        let is_duplicate_item = self
            .events
            .get(self.cursor)
            .is_some_and(|current_item| item.get_id() == current_item.get_id());

        if is_duplicate_item {
            return;
        }

        // When the cursor is detatched and history is not static we want the
        // history to start from the previous head posiition to prevent reads into 'dead' regions
        // of the buffer when cursor tracks backwards.
        let cursor_is_detached = self.head != self.cursor;
        if cursor_is_detached && !self.static_history {
            self.history_start = self.next_idx(self.head);
        }

        let insert_idx = if self.events.len() < self.max_size {
            self.events.len()
        } else {
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
        let buffer_full = self.events.len() == self.max_size;
        if buffer_full && self.head == self.history_start {
            self.static_history = false;
            self.history_start = self.next_idx(self.history_start);
        }

        if insert_idx == self.events.len() {
            self.events.push(item);
        } else {
            self.events[insert_idx] = item;
        }
    }
}
