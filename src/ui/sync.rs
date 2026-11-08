//! Keeping a window and the hub in step: taking in what changed, publishing this window's edits,
//! the poll.

use std::rc::Rc;

use gpui_kit::Context;

use super::{Efa, Loading, Role, actions, error_dialog, hub};
use crate::store::Saved;

/// The settings the stats are calculated with, besides the fits and characters: when these
/// stay the same, a change from another window needs no recalculation.
fn calc_settings(saved: &Saved) -> (bool, f64, bool, bool) {
    let preferences = &saved.settings.preferences;
    (
        saved.settings.factor_reload,
        preferences.engine.default_spool,
        preferences.general.global_character,
        preferences.general.global_damage_pattern,
    )
}

impl Efa {
    /// Take in what changed in the hub: data finishing loading, new prices, and fits other
    /// windows changed.
    pub(super) fn pull(&mut self, cx: &mut Context<Self>) {
        let hub = self.hub.read(cx);
        // What changed: anything at all redraws, what the stats come from recalculates too.
        let (mut redraw, mut recalculate) = (false, false);
        if matches!(self.loading, Loading::Pending) {
            match &hub.loading {
                hub::HubLoading::Ready(loaded) => {
                    self.loading = Loading::Ready(Rc::clone(loaded));
                    recalculate = true;
                }
                hub::HubLoading::Failed(e) => {
                    self.loading = Loading::Failed(e.clone());
                    redraw = true;
                }
                hub::HubLoading::Pending => {}
            }
        }
        if !Rc::ptr_eq(&self.prices, &hub.prices) {
            self.prices = Rc::clone(&hub.prices);
            redraw = true;
        }
        if self.palette != hub.palette {
            self.palette = hub.palette;
            redraw = true;
        }
        if !Rc::ptr_eq(&self.characters, &hub.characters) {
            self.characters = Rc::clone(&hub.characters);
            recalculate = true;
        }
        if hub.engine_generation != self.engine_synced {
            self.engine_synced = hub.engine_generation;
            recalculate = true;
        }
        if hub.generation != self.synced {
            let calc_before = calc_settings(&self.saved);
            let fits_before = self.saved.fits.clone();
            let deleted: Vec<usize> = hub
                .deletions
                .iter()
                .filter(|(generation, _)| *generation > self.synced)
                .map(|&(_, index)| index)
                .collect();
            let (open, current) = (
                self.saved.settings.open.clone(),
                self.saved.settings.current,
            );
            // Edits not published yet survive a settings change from the preferences window.
            let unpublished =
                (self.unsaved && deleted.is_empty()).then(|| std::mem::take(&mut self.saved.fits));
            self.saved = hub.saved.clone();
            self.saved.settings.open = open;
            self.saved.settings.current = current;
            if let Some(fits) = unpublished {
                self.saved.fits = fits;
            }
            for index in deleted {
                hub::shift_tabs(
                    &mut self.saved.settings.open,
                    &mut self.saved.settings.current,
                    index,
                );
                self.history.forget(index);
            }
            self.synced = hub.generation;
            self.inputs_show = None;
            redraw = true;
            // A preferences change leaves every fit shared as it was.
            let same_fits = fits_before.len() == self.saved.fits.len()
                && fits_before
                    .iter()
                    .zip(&self.saved.fits)
                    .all(|(a, b)| Rc::ptr_eq(a, b));
            recalculate |= !same_fits || calc_settings(&self.saved) != calc_before;
        }
        let inbox = (self.role == Role::Main && self.loaded().is_some() && !hub.inbox.is_empty())
            .then(|| self.hub.update(cx, |hub, _| hub.take_inbox()));
        if recalculate {
            self.recompute();
        }
        if redraw || recalculate {
            cx.notify();
        }
        if let Some(inbox) = inbox {
            self.take_inbox(inbox, cx);
        }
    }

    /// Do what other windows left in the hub's inbox: imports become tabs, edits are made with
    /// undo like any other, and requested fits open.
    fn take_inbox(&mut self, inbox: hub::HubInbox, cx: &mut Context<Self>) {
        let hub::HubInbox {
            import,
            fit_imports,
            mutation_edits,
            wizard_edits,
            open_requests,
        } = inbox;
        if let Some(text) = import {
            self.import(&text);
        }
        for imported in fit_imports {
            self.add_imported(imported);
        }
        self.changed(cx);
        self.apply_mutation_edits(mutation_edits, cx);
        self.apply_wizard_edits(wizard_edits, cx);
        let count = self.saved.fits.len();
        for index in open_requests.into_iter().filter(|&i| i < count) {
            self.open_fit(index, cx);
        }
    }

    /// Hand this window's fits to the hub, which writes them out and updates the other windows.
    pub(super) fn publish(&mut self, cx: &mut Context<Self>) {
        let saved = self.saved.clone();
        let deleted = std::mem::take(&mut self.pending_deletions);
        let main = self.role == Role::Main;
        self.hub
            .update(cx, |hub, cx| hub.publish(&saved, main, &deleted, cx));
        self.synced = self.hub.read(cx).generation;
    }

    /// Background work only the window can do: prices and error reports in the main window,
    /// and handing its changes to the hub.
    pub(super) fn poll(&mut self, cx: &mut Context<Self>) {
        // One window fetching prices is enough; the hub hands them to the rest. The same goes
        // for showing errors.
        if self.role == Role::Main {
            self.refresh_prices(cx);
            let errors = crate::errors::take();
            if !errors.is_empty() {
                cx.defer(move |cx| error_dialog::open(&errors, cx));
            }
        }
        if std::mem::take(&mut self.unsaved) {
            self.publish(cx);
        }
    }

    /// Set rolls from stats windows on whichever fits they belong to, one undo step each.
    fn apply_mutation_edits(&mut self, edits: Vec<hub::MutationEdit>, cx: &mut Context<Self>) {
        for edit in edits {
            self.edit_fit(
                edit.fit,
                false,
                |fit, env| {
                    let Some(mutation) = fit.mutation_mut(edit.item) else {
                        return actions::Outcome::skipped();
                    };
                    for (attr, value) in edit.values {
                        mutation.set(&env.loaded.catalog, attr, value);
                    }
                    actions::Outcome::default()
                },
                cx,
            );
        }
    }

    /// Make the fitting wizard's changes on the fits they belong to, one undo step each.
    /// Modules the fitting rules refuse are left out.
    fn apply_wizard_edits(&mut self, edits: Vec<hub::WizardEdit>, cx: &mut Context<Self>) {
        for edit in edits {
            let Some(index) = self.saved.fit_index(edit.uid) else {
                continue;
            };
            self.edit_fit(
                index,
                false,
                |fit, env| {
                    let (engine, pilot) = (&env.loaded.engine, env.pilot);
                    crate::wizard::apply(
                        fit,
                        &crate::wizard::Change {
                            removed: edit.change.removed,
                            placements: Vec::new(),
                        },
                    );
                    let mut outcome = actions::Outcome {
                        structural: true,
                        ..actions::Outcome::default()
                    };
                    let mut base = engine.validate(fit, pilot);
                    for placement in edit.change.placements {
                        let mut next = fit.clone();
                        crate::wizard::place(&mut next, &placement);
                        let after = engine.validate(&next, pilot);
                        if let Some(problem) = base.refusal(&after, next.ignore_restrictions) {
                            outcome.error = Some(problem.describe(|t| env.loaded.catalog.name(t)));
                            continue;
                        }
                        *fit = next;
                        base = after;
                    }
                    outcome
                },
                cx,
            );
        }
    }
}
