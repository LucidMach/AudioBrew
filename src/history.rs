/// Undo/redo over whole-state snapshots of the project.
pub struct History<T> {
    past: Vec<T>,
    current: T,
    future: Vec<T>,
}

impl<T: Clone> History<T> {
    pub fn new(initial: T) -> Self {
        Self { past: Vec::new(), current: initial, future: Vec::new() }
    }

    pub fn current(&self) -> &T {
        &self.current
    }

    /// Records a new state as the result of an edit.
    pub fn push(&mut self, state: T) {
        self.past.push(std::mem::replace(&mut self.current, state));
        self.future.clear();
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.future.pop() {
            self.past.push(std::mem::replace(&mut self.current, next));
        }
    }

    pub fn undo(&mut self) {
        if let Some(previous) = self.past.pop() {
            self.future.push(std::mem::replace(&mut self.current, previous));
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
}
