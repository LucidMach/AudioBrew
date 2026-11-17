/// Undo/redo over whole-state snapshots of the project.
///
/// A drag is a *gesture*: `begin_gesture`, many live `update`s, then `commit` (one undo step)
/// or `abort` (back to where it started). Single edits use `edit`.
pub struct History<T> {
    past: Vec<T>,
    current: T,
    future: Vec<T>,
    /// The state before the open gesture, if one is in progress.
    gesture_base: Option<T>,
    max_depth: usize,
}

impl<T: Clone> History<T> {
    pub fn new(initial: T) -> Self {
        Self::with_limit(initial, usize::MAX)
    }

    /// Like `new`, but keeps at most `max_depth` undo steps, dropping the oldest.
    pub fn with_limit(initial: T, max_depth: usize) -> Self {
        Self {
            past: Vec::new(),
            current: initial,
            future: Vec::new(),
            gesture_base: None,
            max_depth,
        }
    }

    pub fn current(&self) -> &T {
        &self.current
    }

    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// Records a new state as the result of an edit.
    pub fn push(&mut self, state: T) {
        let previous = std::mem::replace(&mut self.current, state);
        self.record(previous);
    }

    /// Starts a drag. A second `begin_gesture` while one is open keeps the original start.
    pub fn begin_gesture(&mut self) {
        if self.gesture_base.is_none() {
            self.gesture_base = Some(self.current.clone());
        }
    }

    /// Sets the live value during a drag without adding an undo step.
    /// Outside a gesture it starts one.
    pub fn update(&mut self, state: T) {
        self.begin_gesture();
        self.current = state;
    }

    /// Cancels the open drag, restoring the state it started from.
    pub fn abort(&mut self) {
        if let Some(base) = self.gesture_base.take() {
            self.current = base;
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.future.pop() {
            let previous = std::mem::replace(&mut self.current, next);
            self.past.push(previous);
            self.trim();
        }
    }

    /// Undoes the last step. During a drag it only cancels the drag.
    pub fn undo(&mut self) {
        if self.gesture_base.is_some() {
            self.abort();
        } else if let Some(previous) = self.past.pop() {
            self.future
                .push(std::mem::replace(&mut self.current, previous));
        }
    }

    fn record(&mut self, previous: T) {
        self.past.push(previous);
        self.future.clear();
        self.trim();
    }

    fn trim(&mut self) {
        if self.past.len() > self.max_depth {
            let excess = self.past.len() - self.max_depth;
            self.past.drain(..excess);
        }
    }
}

impl<T: Clone + PartialEq> History<T> {
    /// Like `push`, but ignores a state equal to the current one.
    pub fn edit(&mut self, state: T) {
        if state != self.current {
            self.push(state);
        }
    }

    /// Ends the open drag as a single undo step, unless it ended where it began.
    pub fn commit(&mut self) {
        if let Some(base) = self.gesture_base.take()
            && base != self.current
        {
            self.record(base);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redo_reapplies_an_undone_edit() {
        let mut history = History::new("two tracks");
        history.push("offset nudged");
        history.undo();

        history.redo();

        assert_eq!(*history.current(), "offset nudged");
    }

    #[test]
    fn a_new_edit_after_undo_discards_the_redo_branch() {
        let mut history = History::new("two tracks");
        history.push("offset nudged");
        history.undo();
        history.push("band muted");

        history.redo();

        assert_eq!(*history.current(), "band muted");
    }

    #[test]
    fn undo_returns_to_the_state_before_the_last_edit() {
        let mut history = History::new("two tracks");
        history.push("offset nudged");

        history.undo();

        assert_eq!(*history.current(), "two tracks");
    }

    #[test]
    fn a_whole_drag_is_one_undo_step() {
        let mut h = History::new(0);
        h.begin_gesture();
        for v in 1..=100 {
            h.update(v);
        }
        h.commit();

        assert_eq!(*h.current(), 100);
        h.undo();
        assert_eq!(*h.current(), 0);
        assert!(!h.can_undo());
        h.redo();
        assert_eq!(*h.current(), 100);
    }

    #[test]
    fn the_live_value_is_visible_while_the_drag_is_in_progress() {
        let mut h = History::new(0);
        h.begin_gesture();
        h.update(7);
        assert_eq!(*h.current(), 7);
    }

    #[test]
    fn aborting_a_drag_restores_the_start_and_records_nothing() {
        let mut h = History::new(5);
        h.begin_gesture();
        h.update(9);
        h.abort();

        assert_eq!(*h.current(), 5);
        assert!(!h.can_undo());
        assert!(!h.can_redo());
    }

    #[test]
    fn a_drag_that_ends_where_it_started_records_nothing() {
        let mut h = History::new(5);
        h.begin_gesture();
        h.update(9);
        h.update(5);
        h.commit();

        assert!(!h.can_undo());
    }

    #[test]
    fn committing_a_drag_discards_the_redo_branch() {
        let mut h = History::new(0);
        h.edit(1);
        h.undo();
        assert!(h.can_redo());

        h.begin_gesture();
        h.update(2);
        h.commit();

        assert!(!h.can_redo());
    }

    #[test]
    fn beginning_a_second_gesture_keeps_the_original_start() {
        let mut h = History::new(0);
        h.begin_gesture();
        h.update(3);
        h.begin_gesture();
        h.update(4);
        h.commit();

        h.undo();
        assert_eq!(*h.current(), 0);
    }

    #[test]
    fn undo_during_a_drag_only_cancels_the_drag() {
        let mut h = History::new(0);
        h.edit(1);
        h.begin_gesture();
        h.update(2);
        h.undo();

        assert_eq!(*h.current(), 1);
        assert!(h.can_undo());
        h.undo();
        assert_eq!(*h.current(), 0);
    }

    #[test]
    fn committing_or_aborting_with_no_gesture_does_nothing() {
        let mut h = History::new(1);
        h.commit();
        h.abort();
        assert_eq!(*h.current(), 1);
        assert!(!h.can_undo());
    }

    #[test]
    fn an_edit_to_the_same_value_is_not_an_undo_step() {
        let mut h = History::new(1);
        h.edit(1);
        assert!(!h.can_undo());
        h.edit(2);
        assert!(h.can_undo());
    }

    #[test]
    fn undo_and_redo_on_a_fresh_history_change_nothing() {
        let mut h = History::new(1);
        h.undo();
        h.redo();
        assert_eq!(*h.current(), 1);
        assert!(!h.can_undo() && !h.can_redo());
    }

    #[test]
    fn fifty_undos_then_fifty_redos_return_to_the_same_state() {
        let mut h = History::new(0);
        for v in 1..=50 {
            h.edit(v);
        }
        for _ in 0..50 {
            h.undo();
        }
        assert_eq!(*h.current(), 0);
        for _ in 0..50 {
            h.redo();
        }
        assert_eq!(*h.current(), 50);
    }

    #[test]
    fn history_depth_is_capped_by_dropping_the_oldest_steps() {
        let mut h = History::with_limit(0, 3);
        for v in 1..=5 {
            h.edit(v);
        }
        for _ in 0..10 {
            h.undo();
        }
        assert_eq!(*h.current(), 2); // 5 -> 4 -> 3 -> 2, the older states are gone
    }
}
