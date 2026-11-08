//! Undo and redo for fit edits, one history per fit like pyfa's per-fit command processors
//! (gui/fitCommands). Each step keeps the whole fit as it was, which for a fit is small and
//! makes every action undoable without writing an inverse for it.

use std::collections::HashMap;

use gpui_kit::Context;

use super::Efa;
use crate::fit::Fit;

/// Steps kept per fit; the oldest go first.
const LIMIT: usize = 100;

#[derive(Debug, Default)]
pub struct History {
    fits: HashMap<usize, Steps>,
}

#[derive(Debug, Default)]
struct Steps {
    undo: Vec<Fit>,
    redo: Vec<Fit>,
}

impl History {
    /// Remember `before` as the state to go back to from fit `index`'s current one, when the
    /// edit changed anything, which it returns. A new edit drops what could be redone, as in
    /// pyfa.
    pub fn record(&mut self, index: usize, before: Fit, after: &Fit) -> bool {
        if same(&before, after) {
            return false;
        }
        let steps = self.fits.entry(index).or_default();
        steps.undo.push(before);
        if steps.undo.len() > LIMIT {
            steps.undo.remove(0);
        }
        steps.redo.clear();
        true
    }

    pub fn can_undo(&self, index: usize) -> bool {
        self.fits.get(&index).is_some_and(|s| !s.undo.is_empty())
    }

    pub fn can_redo(&self, index: usize) -> bool {
        self.fits.get(&index).is_some_and(|s| !s.redo.is_empty())
    }

    /// The fit as it was before the last edit, with `current` kept for redo.
    pub fn undo(&mut self, index: usize, current: Fit) -> Option<Fit> {
        let steps = self.fits.get_mut(&index)?;
        let previous = steps.undo.pop()?;
        steps.redo.push(current);
        Some(previous)
    }

    /// The fit as it was before the last undo, with `current` kept for undo.
    pub fn redo(&mut self, index: usize, current: Fit) -> Option<Fit> {
        let steps = self.fits.get_mut(&index)?;
        let next = steps.redo.pop()?;
        steps.undo.push(current);
        Some(next)
    }

    /// Forget a deleted fit and move the histories of the fits after it down, as their fits did.
    pub fn forget(&mut self, deleted: usize) {
        self.fits = std::mem::take(&mut self.fits)
            .into_iter()
            .filter(|&(i, _)| i != deleted)
            .map(|(i, steps)| (if i > deleted { i - 1 } else { i }, steps))
            .collect();
    }
}

impl Efa {
    pub(super) fn can_undo(&self) -> bool {
        self.saved.current.is_some_and(|i| self.history.can_undo(i))
    }

    pub(super) fn can_redo(&self) -> bool {
        self.saved.current.is_some_and(|i| self.history.can_redo(i))
    }

    pub(super) fn undo(&mut self, cx: &mut Context<Self>) {
        let Some(i) = self.saved.current else { return };
        let current = self.saved.fits[i].clone();
        if let Some(previous) = self.history.undo(i, current) {
            self.restore(i, previous, cx);
        } else {
            self.set_status("Nothing to undo");
            cx.notify();
        }
    }

    pub(super) fn redo(&mut self, cx: &mut Context<Self>) {
        let Some(i) = self.saved.current else { return };
        let current = self.saved.fits[i].clone();
        if let Some(next) = self.history.redo(i, current) {
            self.restore(i, next, cx);
        } else {
            self.set_status("Nothing to redo");
            cx.notify();
        }
    }

    fn restore(&mut self, i: usize, fit: Fit, cx: &mut Context<Self>) {
        self.saved.fits[i] = fit;
        self.clear_selection();
        self.inputs_show = None;
        self.status = None;
        self.changed(cx);
    }
}

/// Fits have no equality of their own; their saved form is what matters anyway.
fn same(a: &Fit, b: &Fit) -> bool {
    serde_json::to_string(a).ok() == serde_json::to_string(b).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn named(name: &str) -> Fit {
        Fit::new(name)
    }

    #[test]
    fn undo_and_redo_walk_the_steps() {
        let mut history = History::default();
        history.record(0, named("a"), &named("b"));
        history.record(0, named("b"), &named("c"));
        let back = history.undo(0, named("c")).expect("one step back");
        assert_eq!(back.name, "b");
        let back = history.undo(0, back).expect("two steps back");
        assert_eq!(back.name, "a");
        assert!(!history.can_undo(0));
        let forward = history.redo(0, back).expect("one step forward");
        assert_eq!(forward.name, "b");
        assert!(history.can_redo(0));
    }

    #[test]
    fn unchanged_edits_and_new_edits_after_undo() {
        let mut history = History::default();
        history.record(0, named("a"), &named("a"));
        assert!(!history.can_undo(0), "nothing changed");
        history.record(0, named("a"), &named("b"));
        let back = history.undo(0, named("b")).expect("a step back");
        history.record(0, back, &named("x"));
        assert!(!history.can_redo(0), "a new edit drops the redo steps");
    }

    #[test]
    fn deleting_a_fit_shifts_histories() {
        let mut history = History::default();
        history.record(0, named("a"), &named("b"));
        history.record(2, named("c"), &named("d"));
        history.forget(0);
        assert!(history.can_undo(1));
        assert!(!history.can_undo(0));
        assert!(!history.can_undo(2));
    }

    #[test]
    fn keeps_a_bounded_number_of_steps() {
        let mut history = History::default();
        for n in 0..=LIMIT {
            history.record(0, named(&n.to_string()), &named("next"));
        }
        let mut current = named("next");
        let mut steps = 0;
        while history.can_undo(0) {
            current = history.undo(0, current).expect("a step to undo");
            steps += 1;
        }
        assert_eq!(steps, LIMIT);
        assert_eq!(current.name, "1", "the oldest step went");
    }
}
