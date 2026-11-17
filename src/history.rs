/// Undo/redo over whole-state snapshots of the project.
pub struct History<T> {
    past: Vec<T>,
    current: T,
}

impl<T: Clone> History<T> {
    pub fn new(initial: T) -> Self {
        Self { past: Vec::new(), current: initial }
    }

    pub fn current(&self) -> &T {
        &self.current
    }

    /// Records a new state as the result of an edit.
    pub fn push(&mut self, state: T) {
        self.past.push(std::mem::replace(&mut self.current, state));
    }

    pub fn undo(&mut self) {
        if let Some(previous) = self.past.pop() {
            self.current = previous;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_returns_to_the_state_before_the_last_edit() {
        let mut history = History::new("two tracks");
        history.push("offset nudged");

        history.undo();

        assert_eq!(*history.current(), "two tracks");
    }
}
