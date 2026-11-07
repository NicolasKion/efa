//! What every window shares: the loaded data, the saved fits and settings, and prices.
//!
//! Windows keep a working copy of the fits so the views can borrow them directly. A window
//! publishes its copy here when it changes, and every other window pulls the new one in. Each
//! window keeps its own tabs; only the main window's are remembered between runs.

use std::{path::PathBuf, rc::Rc};

use gpui_kit::{AnyWindowHandle, App, AppContext, Context, Global, SharedString, WeakEntity};

use super::{Loaded, character_editor::CharacterEditor, load};
use crate::{character::Characters, preferences::Preferences, prices::Prices, store::Saved};

/// The current preferences for code that has no window to ask, such as menu builders. The hub
/// keeps it in step with its saved state.
struct CurrentPreferences(Preferences);

impl Global for CurrentPreferences {}

/// The preferences in effect, or pyfa's defaults before the hub exists.
pub fn preferences(cx: &App) -> Preferences {
    cx.try_global::<CurrentPreferences>()
        .map_or_default(|p| p.0.clone())
}

pub enum HubLoading {
    Pending,
    Ready(Rc<Loaded>),
    Failed(SharedString),
}

pub struct Hub {
    pub loading: HubLoading,
    /// The authoritative fits and settings. Its tabs are the main window's.
    pub saved: Saved,
    /// Bumped on every publish, so windows can tell they are behind.
    pub generation: u64,
    /// Fits deleted so far, with the generation each happened in, so windows that were behind
    /// can shift their tabs before taking the new fits.
    pub deletions: Vec<(u64, usize)>,
    pub prices: Rc<Prices>,
    /// EFT text from the command line, for the main window to import once data is loaded.
    pub import: Option<String>,
    /// Fits other windows (the graphs) asked the main window to open as tabs.
    pub open_requests: Vec<usize>,
    /// The main window's stats pane shows raw HP rather than EHP; the graphs follow it.
    pub raw_hp: bool,
    /// The characters. Replaced, never changed in place, so a window can tell it has the
    /// latest by pointer.
    pub characters: Rc<Characters>,
    /// The open character editor, which is brought forward instead of opening a second one.
    pub character_editor: Option<(AnyWindowHandle, WeakEntity<CharacterEditor>)>,
}

impl Hub {
    pub fn new(data_dir: PathBuf, import: Option<String>, cx: &mut Context<Self>) -> Self {
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = cx.background_spawn(async move { load(&data_dir) }).await;
            let _ = this.update(cx, |hub, cx| {
                hub.loading = match result {
                    Ok(loaded) => {
                        // Last session's prices show right away; a refresh follows if stale.
                        hub.prices = Rc::new(Prices::cached(&loaded.cache_dir));
                        HubLoading::Ready(Rc::new(loaded))
                    }
                    Err(e) => HubLoading::Failed(e.into()),
                };
                cx.notify();
            });
        })
        .detach();
        let mut saved = Saved::load();
        if !saved.preferences.general.reopen_fits {
            saved.open.clear();
            saved.current = None;
        }
        cx.set_global(CurrentPreferences(saved.preferences.clone()));
        Self {
            loading: HubLoading::Pending,
            saved,
            generation: 0,
            deletions: Vec::new(),
            prices: Rc::new(Prices::default()),
            import,
            open_requests: Vec::new(),
            raw_hp: false,
            characters: Rc::new(Characters::load()),
            character_editor: None,
        }
    }

    /// Change the characters, write them out if that changed anything, and tell every window.
    pub fn edit_characters<R>(
        &mut self,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut Characters) -> R,
    ) -> R {
        let mut characters = (*self.characters).clone();
        let result = edit(&mut characters);
        if characters != *self.characters {
            if let Err(e) = characters.save() {
                eprintln!("saving characters: {e}");
            }
            self.characters = Rc::new(characters);
            cx.notify();
        }
        result
    }

    /// Take a window's fits and settings as the new truth, write them out, and tell every
    /// window. `deleted` are fits the window removed since it last published, in order.
    pub fn publish(
        &mut self,
        saved: &Saved,
        main: bool,
        deleted: &[usize],
        cx: &mut Context<Self>,
    ) {
        self.generation += 1;
        self.deletions
            .extend(deleted.iter().map(|&index| (self.generation, index)));
        let (open, current) = if main {
            (saved.open.clone(), saved.current)
        } else {
            let mut open = std::mem::take(&mut self.saved.open);
            let mut current = self.saved.current;
            for &index in deleted {
                shift_tabs(&mut open, &mut current, index);
            }
            (open, current)
        };
        self.saved = Saved {
            open,
            current,
            ..saved.clone()
        };
        if let Err(e) = self.saved.save() {
            eprintln!("saving fits: {e}");
        }
        self.sync_preferences(cx);
        cx.notify();
    }

    /// Change settings from outside a fit window, such as the preferences window, and pass them
    /// on to every window.
    pub fn update_saved(&mut self, edit: impl FnOnce(&mut Saved), cx: &mut Context<Self>) {
        edit(&mut self.saved);
        self.generation += 1;
        if let Err(e) = self.saved.save() {
            eprintln!("saving settings: {e}");
        }
        self.sync_preferences(cx);
        cx.notify();
    }

    fn sync_preferences(&self, cx: &mut Context<Self>) {
        let changed = cx
            .try_global::<CurrentPreferences>()
            .is_none_or(|p| p.0 != self.saved.preferences);
        if changed {
            cx.set_global(CurrentPreferences(self.saved.preferences.clone()));
        }
    }

    pub fn set_prices(&mut self, prices: Rc<Prices>, cx: &mut Context<Self>) {
        self.prices = prices;
        cx.notify();
    }
}

/// Drop the tab of a deleted fit and move the ones after it down, as their fits did.
pub fn shift_tabs(open: &mut Vec<usize>, current: &mut Option<usize>, deleted: usize) {
    open.retain(|&i| i != deleted);
    if *current == Some(deleted) {
        *current = open.last().copied();
    }
    let shift = |i: usize| if i > deleted { i - 1 } else { i };
    *open = open.iter().map(|&i| shift(i)).collect();
    *current = current.map(shift);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deleting_a_fit_shifts_later_tabs() {
        let (mut open, mut current) = (vec![0, 2, 3], Some(3));
        shift_tabs(&mut open, &mut current, 1);
        assert_eq!((open, current), (vec![0, 1, 2], Some(2)));
    }

    #[test]
    fn deleting_the_current_fit_moves_to_the_last_tab() {
        let (mut open, mut current) = (vec![0, 1, 2], Some(1));
        shift_tabs(&mut open, &mut current, 1);
        assert_eq!((open, current), (vec![0, 1], Some(1)));
    }
}
