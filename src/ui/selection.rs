//! Picking rows in a list the way desktop list views do, and the keys every list takes.
//!
//! A plain click picks one row, Ctrl adds or removes one, Shift takes the run from the last
//! clicked row (the anchor) to the clicked one, in the order the list shows them. A right-click
//! on a row outside the selection makes it the selection, so a menu acts on what it was opened
//! on. Delete, Ctrl+A and Escape are read with [`list_key`].

use gpui_kit::{KeyDownEvent, Modifiers};

/// The rows picked in a list, by whatever identifies a row there, and the row a Shift-click
/// range starts from.
#[derive(Debug, Clone)]
pub struct ListSelection<T> {
    items: Vec<T>,
    anchor: Option<T>,
}

impl<T> Default for ListSelection<T> {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            anchor: None,
        }
    }
}

impl<T: Copy + Eq> ListSelection<T> {
    /// The picked rows, in the order they were picked.
    pub fn items(&self) -> &[T] {
        &self.items
    }

    pub fn contains(&self, item: &T) -> bool {
        self.items.contains(item)
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// A click on `item` with `modifiers`, where `order` is every row as the list shows them,
    /// for Shift's range. Shift without an anchor, or with rows `order` lacks, clicks plainly.
    pub fn click(&mut self, item: T, modifiers: Modifiers, order: &[T]) {
        if modifiers.shift
            && let Some(anchor) = self.anchor
            && let (Some(a), Some(b)) = (
                order.iter().position(|&o| o == anchor),
                order.iter().position(|&o| o == item),
            )
        {
            self.items = order[a.min(b)..=a.max(b)].to_vec();
            return;
        }
        if modifiers.control {
            match self.items.iter().position(|&s| s == item) {
                Some(at) => {
                    self.items.remove(at);
                }
                None => self.items.push(item),
            }
        } else {
            self.items = vec![item];
        }
        self.anchor = Some(item);
    }

    /// A right-click on `item`: outside the selection it becomes the selection. Returns whether
    /// that changed anything.
    pub fn right_click(&mut self, item: T) -> bool {
        if self.contains(&item) {
            return false;
        }
        self.items = vec![item];
        self.anchor = Some(item);
        true
    }

    /// What an action on `clicked` applies to: the selection when it holds the clicked row,
    /// the clicked row alone otherwise.
    pub fn targets(&self, clicked: T) -> Vec<T> {
        if self.contains(&clicked) {
            self.items.clone()
        } else {
            vec![clicked]
        }
    }

    /// Ctrl+A: every row in `order`.
    pub fn select_all(&mut self, order: impl IntoIterator<Item = T>) {
        self.items = order.into_iter().collect();
    }

    pub fn clear(&mut self) {
        self.items.clear();
        self.anchor = None;
    }

    /// The picked rows, leaving nothing picked, for an action that removes them.
    pub fn take(&mut self) -> Vec<T> {
        self.anchor = None;
        std::mem::take(&mut self.items)
    }

    /// Keep only the rows `keep` accepts, such as those still listed.
    pub fn retain(&mut self, mut keep: impl FnMut(&T) -> bool) {
        self.items.retain(|item| keep(item));
        if self.anchor.is_some_and(|a| !keep(&a)) {
            self.anchor = None;
        }
    }
}

impl ListSelection<usize> {
    /// A click on the row at position `n`, in a list whose rows are known by their position,
    /// so Shift's range is every position in between.
    pub fn click_position(&mut self, n: usize, modifiers: Modifiers) {
        let order: Vec<usize> = (0..=n.max(self.anchor.unwrap_or(0))).collect();
        self.click(n, modifiers, &order);
    }
}

/// A key every list reacts to, pyfa's list keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKey {
    /// Delete, no modifiers: remove the selection.
    Delete,
    /// Ctrl+A: select every row.
    SelectAll,
    /// Escape, no modifiers: clear the selection.
    Clear,
}

/// Which list key `event` is, if any.
pub fn list_key(event: &KeyDownEvent) -> Option<ListKey> {
    let keystroke = &event.keystroke;
    let m = keystroke.modifiers;
    let plain = !(m.control || m.alt || m.shift || m.platform);
    match keystroke.key.as_str() {
        "delete" if plain => Some(ListKey::Delete),
        "a" if m.control && !m.alt && !m.shift && !m.platform => Some(ListKey::SelectAll),
        "escape" if plain => Some(ListKey::Clear),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(control: bool, shift: bool) -> Modifiers {
        Modifiers {
            control,
            shift,
            ..Modifiers::default()
        }
    }

    #[test]
    fn clicks_pick_toggle_and_extend_in_list_order() {
        let order = [5, 3, 9, 1];
        let mut s = ListSelection::default();
        s.click(3, with(false, false), &order);
        assert_eq!(s.items(), &[3]);
        s.click(1, with(false, true), &order);
        assert_eq!(s.items(), &[3, 9, 1]);
        s.click(9, with(true, false), &order);
        assert_eq!(s.items(), &[3, 1]);
        // The Ctrl-click moved the anchor.
        s.click(5, with(false, true), &order);
        assert_eq!(s.items(), &[5, 3, 9]);
    }

    #[test]
    fn a_right_click_outside_the_selection_replaces_it() {
        let mut s = ListSelection::default();
        s.select_all([1, 2, 3]);
        assert!(!s.right_click(2));
        assert_eq!(s.targets(2), vec![1, 2, 3]);
        assert!(s.right_click(7));
        assert_eq!(s.items(), &[7]);
        assert_eq!(s.targets(1), vec![1]);
    }
}
