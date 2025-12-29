use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::sync::Arc;

use chrono::NaiveDateTime;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

use crate::event_history::EventHistory;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SortedDistinctVec<T>(Vec<T>);

impl<T: Ord + PartialOrd> SortedDistinctVec<T> {
    #[must_use]
    pub fn new(mut input: Vec<T>) -> Self {
        input.sort();
        input.dedup();
        Self(input)
    }

    #[must_use]
    pub fn get(&self) -> &[T] {
        &self.0
    }
}

impl<T> SortedDistinctVec<T> {
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.0.iter()
    }
}

impl<T> IntoIterator for SortedDistinctVec<T> {
    type Item = T;
    type IntoIter = std::vec::IntoIter<T>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<'a, T> IntoIterator for &'a SortedDistinctVec<T> {
    type Item = &'a T;
    type IntoIter = std::slice::Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub type SharedEventHistory<T> = Arc<Mutex<EventHistory<T>>>;

// Events
pub trait EventItem {
    type ID: Eq + PartialEq + Hash + Clone + Display + Debug;
    fn get_id(&self) -> &Self::ID;
}

pub struct WindowEvent {
    pub address: String,
    pub monitor: Option<String>,
    pub time: NaiveDateTime,
}

#[derive(Clone)]
pub struct FocusEvents {
    pub focus_events: SharedEventHistory<WindowEvent>,
    pub requested_monitors: &'static SortedDistinctVec<String>,
}

#[derive(Clone)]
pub enum HyprEvents {
    Focus(FocusEvents),
}

impl EventItem for WindowEvent {
    type ID = String;

    fn get_id(&self) -> &Self::ID {
        &self.address
    }
}

#[cfg(test)]
mod tests {
    use super::SortedDistinctVec;

    #[test]
    fn iterates_in_sorted_unique_order_by_value() {
        let sorted = SortedDistinctVec::new(vec![3, 1, 2, 2, 1]);

        let collected: Vec<_> = sorted.into_iter().collect();

        assert_eq!(collected, vec![1, 2, 3]);
    }

    #[test]
    fn iterates_by_reference() {
        let sorted = SortedDistinctVec::new(vec![
            "delta".to_string(),
            "alpha".to_string(),
            "beta".to_string(),
            "alpha".to_string(),
        ]);

        let collected: Vec<_> = (&sorted).into_iter().cloned().collect();

        assert_eq!(
            collected,
            vec!["alpha".to_string(), "beta".to_string(), "delta".to_string()]
        );
    }
}
