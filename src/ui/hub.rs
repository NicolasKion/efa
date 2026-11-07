//! What every window shares: the loaded data, the saved fits and settings, and prices.
//!
//! Windows keep a working copy of the fits so the views can borrow them directly. A window
//! publishes its copy here when it changes, and every other window pulls the new one in. Each
//! window keeps its own tabs; only the main window's are remembered between runs.

use std::rc::Rc;

use gpui_kit::{
    AnyWindowHandle, App, AppContext, Context, Entity, Global, SharedString, Task, WeakEntity,
    Window,
};

use super::{Loaded, character_editor::CharacterEditor, load};
use crate::{
    character::Characters, engine::Overridden, overrides::Overrides, preferences::Preferences,
    prices::Prices, store::Saved,
};

/// The current preferences for code that has no window to ask, such as menu builders. The hub
/// keeps it in step with its saved state.
struct CurrentPreferences(Preferences);

impl Global for CurrentPreferences {}

/// The preferences in effect, or pyfa's defaults before the hub exists.
pub fn preferences(cx: &App) -> Preferences {
    cx.try_global::<CurrentPreferences>()
        .map_or_default(|p| p.0.clone())
}

/// Which context menu entries show in a menu opening now: the Context Menus preferences, or all
/// of them while Ctrl is held, as in pyfa (gui/contextMenu.py).
pub fn shown_menus(window: &Window, cx: &App) -> crate::preferences::ContextMenus {
    if window.modifiers().control {
        crate::preferences::ContextMenus::default()
    } else {
        preferences(cx).context_menus
    }
}

/// New rolled values for a mutated module or drone stack of a saved fit.
#[derive(Debug, Clone)]
pub struct MutationEdit {
    pub fit: usize,
    pub item: crate::engine::FitItem,
    /// Attribute values by attribute ID.
    pub values: Vec<(i32, f64)>,
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
    /// Rolls the stats window set on mutated items, for the main window to apply with undo.
    pub mutation_edits: Vec<MutationEdit>,
    /// The main window's stats pane shows raw HP rather than EHP; the graphs follow it.
    pub raw_hp: bool,
    /// The characters. Replaced, never changed in place, so a window can tell it has the
    /// latest by pointer.
    pub characters: Rc<Characters>,
    /// The open character editor, which is brought forward instead of opening a second one.
    pub character_editor: Option<(AnyWindowHandle, WeakEntity<CharacterEditor>)>,
    /// Characters logged in with EVE SSO, and a login under way.
    pub esi: super::esi::EsiState,
    /// Fits other windows (Browse ESI Fittings) want the main window to add as new tabs.
    pub fit_imports: Vec<crate::port::Imported>,
    /// The engine data being rebuilt for changed attribute overrides.
    overrides_task: Option<Task<()>>,
    /// Update checks and installs, shown by the main window and the preferences.
    pub updates: Entity<super::updates::Updates>,
}

impl Hub {
    pub fn new(import: Option<String>, cx: &mut Context<Self>) -> Self {
        let mut saved = Saved::load();
        let overrides = saved.overrides.active();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = cx.background_spawn(async move { load(&overrides) }).await;
            let _ = this.update(cx, |hub, cx| {
                hub.loading = match result {
                    Ok(loaded) => {
                        // Last session's prices show right away; a refresh follows if stale.
                        hub.prices = Rc::new(Prices::cached(&loaded.cache_dir));
                        HubLoading::Ready(Rc::new(loaded))
                    }
                    Err(e) => HubLoading::Failed(e.into()),
                };
                // Overrides changed while the data was loading.
                hub.refresh_overrides(cx);
                cx.notify();
            });
        })
        .detach();
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
            mutation_edits: Vec::new(),
            raw_hp: false,
            characters: Rc::new(Characters::load()),
            character_editor: None,
            esi: super::esi::EsiState::load(),
            fit_imports: Vec::new(),
            overrides_task: None,
            updates: cx.new(super::updates::Updates::new),
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
                crate::errors::report("Saving characters failed", e);
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
        // Only the overrides window changes overrides, through the hub, so a window's copy may
        // be behind.
        self.saved = Saved {
            open,
            current,
            overrides: std::mem::take(&mut self.saved.overrides),
            ..saved.clone()
        };
        if let Err(e) = self.saved.save() {
            crate::errors::report("Saving fits failed", e);
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
            crate::errors::report("Saving settings failed", e);
        }
        self.sync_preferences(cx);
        cx.notify();
    }

    /// Change the attribute overrides or switch them, save, and recalculate every window with
    /// them once the engine's data is rebuilt.
    pub fn edit_overrides(&mut self, edit: impl FnOnce(&mut Overrides), cx: &mut Context<Self>) {
        let before = self.saved.overrides.clone();
        edit(&mut self.saved.overrides);
        if self.saved.overrides == before {
            return;
        }
        self.update_saved(|_| {}, cx);
        self.refresh_overrides(cx);
    }

    /// Rebuild the engine's data away from the UI thread if the overrides in effect changed, and
    /// bump the generation when it lands so every window recalculates.
    fn refresh_overrides(&mut self, cx: &mut Context<Self>) {
        let HubLoading::Ready(loaded) = &self.loading else {
            return;
        };
        let values = self.saved.overrides.active();
        if loaded.engine.overrides() == values {
            self.overrides_task = None;
            return;
        }
        let source = loaded.engine.source().clone();
        // A newer change replaces a rebuild still running.
        self.overrides_task = Some(cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let built = cx
                .background_spawn(async move {
                    let data = Overridden::load(&source, &values);
                    (values, data)
                })
                .await;
            let _ = this.update(cx, |hub, cx| {
                hub.overrides_task = None;
                let (values, data) = built;
                let HubLoading::Ready(loaded) = &hub.loading else {
                    return;
                };
                match data {
                    Ok(data) if values == hub.saved.overrides.active() => {
                        loaded.engine.set_overrides(data);
                        hub.generation += 1;
                        cx.notify();
                    }
                    Ok(_) => hub.refresh_overrides(cx),
                    Err(e) => eprintln!("attribute overrides: {e}"),
                }
            });
        }));
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
