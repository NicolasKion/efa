//! What every window shares: the loaded data, the saved fits and settings, and prices.
//!
//! Windows keep a working copy of the fits so the views can borrow them directly. A window
//! publishes its copy here when it changes, and every other window pulls the new one in. Each
//! window keeps its own tabs; only the main window's are remembered between runs.

use std::rc::Rc;

use gpui_kit::{App, AppContext, Context, Entity, Global, SharedString, Task, WeakEntity, Window};

use super::{Loaded, load};
use crate::{
    character::Characters, engine::Overridden, fit::Fit, overrides::Overrides,
    preferences::Preferences, prices::Prices, store::Saved,
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

/// A fitting wizard change for a saved fit, by the fit's identity.
#[derive(Debug, Clone)]
pub struct WizardEdit {
    pub uid: u64,
    pub change: crate::wizard::Change,
}

/// New rolled values for a mutated module or drone stack of a saved fit.
#[derive(Debug, Clone)]
pub struct MutationEdit {
    pub fit: usize,
    pub item: crate::engine::FitItem,
    /// Attribute values by attribute ID.
    pub values: Vec<(i32, f64)>,
}

/// What other windows hand the main window to do, which it takes as one batch.
#[derive(Default)]
pub struct HubInbox {
    /// EFT text from the command line, to import once data is loaded.
    pub import: Option<String>,
    /// Fits other windows (Browse ESI Fittings) want added as new tabs.
    pub fit_imports: Vec<crate::port::Imported>,
    /// Rolls the stats window set on mutated items, to apply with undo.
    pub mutation_edits: Vec<MutationEdit>,
    /// Changes the fitting wizard window applies, to make with undo.
    pub wizard_edits: Vec<WizardEdit>,
    /// Fits other windows (the graphs) asked to open as tabs.
    pub open_requests: Vec<usize>,
}

impl HubInbox {
    pub fn is_empty(&self) -> bool {
        self.import.is_none()
            && self.fit_imports.is_empty()
            && self.mutation_edits.is_empty()
            && self.wizard_edits.is_empty()
            && self.open_requests.is_empty()
    }
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
    /// Bumped when the engine's data changes under the fits, such as for attribute overrides,
    /// so windows recalculate though no fit changed.
    pub engine_generation: u64,
    /// A revision per fit, in step with `saved.fits`, bumped whenever that fit changes, so a
    /// window can tell a fit it shows changed without comparing it.
    revisions: Vec<u64>,
    /// The last revision handed out.
    last_revision: u64,
    /// Fits deleted so far, with the generation each happened in, so windows that were behind
    /// can shift their tabs before taking the new fits.
    pub deletions: Vec<(u64, usize)>,
    pub prices: Rc<Prices>,
    /// Work other windows leave for the main window.
    pub inbox: HubInbox,
    /// The main window's stats pane shows raw HP rather than EHP; the graphs follow it.
    pub raw_hp: bool,
    /// The characters. Replaced, never changed in place, so a window can tell it has the
    /// latest by pointer.
    pub characters: Rc<Characters>,
    /// Characters logged in with EVE SSO, and a login under way.
    pub esi: super::esi::EsiState,
    /// The engine data being rebuilt for changed attribute overrides.
    overrides_task: Option<Task<()>>,
    /// Update checks and installs, shown by the main window and the preferences.
    pub updates: Entity<super::updates::Updates>,
    /// The database the fits, characters and logins are written to. None when it could not be
    /// opened, which was reported; changes are then not saved.
    store: Option<crate::db::Store>,
}

impl Hub {
    pub fn new(import: Option<String>, cx: &mut Context<Self>) -> Self {
        let (store, contents) = crate::db::Store::open_default();
        let mut saved = contents.saved;
        let overrides = saved.settings.overrides.active();
        let proxy = saved.settings.preferences.network.proxy.clone();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = cx
                .background_spawn(async move { load(&overrides, &proxy) })
                .await;
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
        if !saved.settings.preferences.general.reopen_fits {
            saved.settings.open.clear();
            saved.settings.current = None;
        }
        let revisions: Vec<u64> = (1..=saved.fits.len() as u64).collect();
        cx.set_global(CurrentPreferences(saved.settings.preferences.clone()));
        Self {
            loading: HubLoading::Pending,
            saved,
            generation: 0,
            engine_generation: 0,
            last_revision: revisions.len() as u64,
            revisions,
            deletions: Vec::new(),
            prices: Rc::new(Prices::default()),
            inbox: HubInbox {
                import,
                ..HubInbox::default()
            },
            raw_hp: false,
            characters: Rc::new(contents.characters),
            esi: super::esi::EsiState::new(contents.accounts),
            overrides_task: None,
            updates: cx.new(super::updates::Updates::new),
            store,
        }
    }

    /// Write the fits and settings, only what changed.
    fn write_saved(&mut self) -> Result<(), String> {
        match &mut self.store {
            Some(store) => store.save_saved(&self.saved),
            None => Ok(()),
        }
    }

    /// Write the ESI logins, only what changed.
    pub(super) fn write_accounts(
        &mut self,
        accounts: &crate::esi::accounts::Accounts,
    ) -> Result<(), String> {
        match &mut self.store {
            Some(store) => store.save_accounts(accounts),
            None => Ok(()),
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
            if let Some(store) = &mut self.store
                && let Err(e) = store.save_characters(&characters)
            {
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
            (saved.settings.open.clone(), saved.settings.current)
        } else {
            let mut open = std::mem::take(&mut self.saved.settings.open);
            let mut current = self.saved.settings.current;
            for &index in deleted {
                shift_tabs(&mut open, &mut current, index);
            }
            (open, current)
        };
        // Only the overrides window changes overrides, through the hub, so a window's copy may
        // be behind.
        let overrides = std::mem::take(&mut self.saved.settings.overrides);
        let before = std::mem::replace(&mut self.saved, saved.clone());
        self.saved.settings.open = open;
        self.saved.settings.current = current;
        self.saved.settings.overrides = overrides;
        self.note_revisions(&before.fits, deleted);
        if let Err(e) = self.write_saved() {
            crate::errors::report("Saving fits failed", e);
        }
        self.sync_preferences(cx);
        cx.notify();
    }

    /// Change settings from outside a fit window, such as the preferences window, and pass them
    /// on to every window.
    pub fn update_saved(&mut self, edit: impl FnOnce(&mut Saved), cx: &mut Context<Self>) {
        let before = self.saved.fits.clone();
        edit(&mut self.saved);
        self.note_revisions(&before, &[]);
        self.generation += 1;
        if let Err(e) = self.write_saved() {
            crate::errors::report("Saving settings failed", e);
        }
        self.sync_preferences(cx);
        cx.notify();
    }

    /// Change the attribute overrides or switch them, save, and recalculate every window with
    /// them once the engine's data is rebuilt.
    pub fn edit_overrides(&mut self, edit: impl FnOnce(&mut Overrides), cx: &mut Context<Self>) {
        let before = self.saved.settings.overrides.clone();
        edit(&mut self.saved.settings.overrides);
        if self.saved.settings.overrides == before {
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
        let values = self.saved.settings.overrides.active();
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
                    Ok(data) if values == hub.saved.settings.overrides.active() => {
                        loaded.engine.set_overrides(data);
                        hub.generation += 1;
                        hub.engine_generation += 1;
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
            .is_none_or(|p| p.0 != self.saved.settings.preferences);
        if changed {
            cx.set_global(CurrentPreferences(self.saved.settings.preferences.clone()));
        }
    }

    /// Fit `index`'s revision, which changes whenever the fit does. Zero for no such fit.
    pub fn fit_revision(&self, index: usize) -> u64 {
        self.revisions.get(index).copied().unwrap_or(0)
    }

    /// Everything other windows left for the main window, leaving the inbox empty.
    pub fn take_inbox(&mut self) -> HubInbox {
        std::mem::take(&mut self.inbox)
    }

    /// Bring the revisions in line with the fits after they changed from `before`, with
    /// `deleted` taken out of it in order: a fit that is still the same shared fit keeps its
    /// revision, any other gets a new one.
    fn note_revisions(&mut self, before: &[Rc<Fit>], deleted: &[usize]) {
        let mut before = before.to_vec();
        let mut revisions = std::mem::take(&mut self.revisions);
        revisions.resize(before.len(), 0);
        for &index in deleted {
            if index < before.len() {
                before.remove(index);
                revisions.remove(index);
            }
        }
        let mut last = self.last_revision;
        self.revisions = self
            .saved
            .fits
            .iter()
            .enumerate()
            .map(|(i, fit)| match before.get(i) {
                Some(old) if Rc::ptr_eq(old, fit) => revisions[i],
                _ => {
                    last += 1;
                    last
                }
            })
            .collect();
        self.last_revision = last;
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
