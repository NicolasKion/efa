//! Opening, adding, closing, deleting and copying fits, importing them, and adding market items.

use std::rc::Rc;

use gpui_kit::component::{
    WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Textarea, TextareaState},
    v_flex,
};
use gpui_kit::{App, AppContext, ClickEvent, Context, ParentElement, Styled, Window, px};

use super::{AdditionsTab, Efa, actions, dialogs, dialogs::Confirm, hub, widgets};
use crate::{
    data::{Catalog, Kind, TypeId},
    fit::Fit,
};

/// The import dialog's text area, and what the dialog takes around it.
const IMPORT_TEXT_H: f32 = 320.;
const IMPORT_AROUND: f32 = 180.;

impl Efa {
    pub(super) fn open_fit(&mut self, index: usize, cx: &mut Context<Self>) {
        self.clear_selection();
        if !self.saved.settings.open.contains(&index) {
            self.saved.settings.open.push(index);
        }
        self.saved.settings.current = Some(index);
        self.changed(cx);
    }

    /// A fit new to efa: on `ship` with its first mode when there is one, flown by the
    /// character picked last, with the preferences' defaults.
    pub(super) fn fresh_fit(
        &self,
        name: impl Into<String>,
        ship: Option<TypeId>,
        catalog: &Catalog,
    ) -> Fit {
        let mut fit = match ship {
            Some(ship) => crate::port::new_fit(catalog, ship, name.into()),
            None => Fit::new(name),
        };
        self.new_fit_defaults(&mut fit);
        fit
    }

    pub(super) fn add_fit(&mut self, mut fit: Fit, cx: &mut Context<Self>) {
        fit.modified = crate::fit::now();
        self.saved.push_fit(fit);
        self.open_fit(self.saved.fits.len() - 1, cx);
    }

    pub(super) fn close_fit(&mut self, index: usize, cx: &mut Context<Self>) {
        self.clear_selection();
        self.saved.settings.open.retain(|&i| i != index);
        if self.saved.settings.current == Some(index) {
            self.saved.settings.current = self.saved.settings.open.last().copied();
        }
        self.changed(cx);
    }

    pub(super) fn close_other_fits(&mut self, keep: usize, cx: &mut Context<Self>) {
        self.clear_selection();
        self.saved.settings.open.retain(|&i| i == keep);
        self.saved.settings.current = Some(keep);
        self.changed(cx);
    }

    fn delete_fit(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= self.saved.fits.len() {
            return;
        }
        self.clear_selection();
        self.saved.fits.remove(index);
        hub::shift_tabs(
            &mut self.saved.settings.open,
            &mut self.saved.settings.current,
            index,
        );
        self.history.forget(index);
        self.pending_deletions.push(index);
        self.inputs_show = None;
        self.changed(cx);
    }

    /// Delete a fit after asking, as pyfa's fit row does (fitItem.py deleteBtnCB); holding
    /// Shift deletes right away. Enter confirms like pyfa's Yes default, Escape cancels.
    pub(super) fn confirm_delete_fit(
        &mut self,
        index: usize,
        shift: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if shift {
            self.delete_fit(index, cx);
            return;
        }
        // The list may change while the question is up, so find the fit again by its identity.
        let Some(uid) = self.saved.ensure_uid(index) else {
            return;
        };
        let this = cx.weak_entity();
        let question = Confirm::new("Confirm Delete", "Do you really want to delete this fit?");
        dialogs::confirm(window, cx, question, move |_, cx| {
            let _ = this.update(cx, |g, cx| {
                if let Some(index) = g.saved.fit_index(uid) {
                    g.delete_fit(index, cx);
                }
            });
        });
    }

    /// Import fits in any format as new open fits. Problems end up in the status line.
    pub(super) fn import(&mut self, text: &str) {
        self.import_fits(text, None);
    }

    /// Add an item from the market the way double-clicking it in pyfa does.
    pub(super) fn add_item(&mut self, id: TypeId, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded() else { return };
        let catalog = &loaded.catalog;
        let kind = catalog.get(id).map(|t| t.kind);
        let name = catalog.name(id);
        // A ship starts a new fit rather than swapping the hull under an existing one.
        if kind == Some(Kind::Ship) {
            let fit = self.fresh_fit(name, Some(id), catalog);
            self.status = None;
            self.add_fit(fit, cx);
            return;
        }
        let Some(i) = self.saved.settings.current else {
            self.set_status("Pick a ship first");
            cx.notify();
            return;
        };
        // pyfa launches a new squadron only when a tube of its class is free.
        if kind == Some(Kind::Fighter) {
            self.status = None;
            self.add_fighter_squadrons(vec![crate::fit::fighters::Fighter::new(id)], cx);
            return;
        }
        // Like pyfa, a module only goes on when the ship has room for it and, with the fitting
        // restrictions on, can take it.
        let module = matches!(kind, Some(Kind::Module(_)));
        let mut added = false;
        let kept = self.edit_fit(
            i,
            module,
            |fit, _| {
                added = fit.add(catalog, id);
                actions::Outcome {
                    error: (!added).then(|| format!("Nothing on this fit takes {name}")),
                    about: Some(name.clone()),
                    ..actions::Outcome::default()
                }
            },
            cx,
        );
        if !kept {
            return;
        }
        if added {
            self.status = None;
        }
        // pyfa brings the Boosters and Implants tabs up even when the item was already there,
        // and the drones pane only when a drone went in.
        match kind {
            Some(Kind::Booster) => self.show_additions_tab(AdditionsTab::Boosters),
            Some(Kind::Implant) => self.show_additions_tab(AdditionsTab::Implants),
            Some(Kind::Drone) if added => self.show_additions_tab(AdditionsTab::Drones),
            _ => {}
        }
    }

    pub(super) fn open_import_dialog(window: &mut Window, cx: &mut Context<Self>) {
        let text = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(4)
                .placeholder("[Vexor, My Vexor]\nDrone Damage Amplifier II\n…")
        });
        let this = cx.weak_entity();
        window.open_dialog(cx, move |dialog, window, _| {
            let (text, this) = (text.clone(), this.clone());
            // The OK button and Enter both submit; it closes the dialog itself when done.
            let submit: crate::ui::widgets::Submit = {
                let (text, this) = (text.clone(), this.clone());
                Rc::new(move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    let value = text.read(cx).value().to_string();
                    let _ = this.update(cx, |this, cx| {
                        this.import(&value);
                        this.changed(cx);
                    });
                    window.close_dialog(cx);
                })
            };
            dialog
                .on_ok({
                    let submit = Rc::clone(&submit);
                    move |e, w, cx| {
                        submit(e, w, cx);
                        false
                    }
                })
                .title("Import Fits")
                .w(px(560.))
                .child(
                    v_flex()
                        .gap_3()
                        .child(Textarea::new(&text).h(widgets::dialog_fill_h(
                            window,
                            IMPORT_TEXT_H,
                            IMPORT_AROUND,
                        )))
                        .child(
                            h_flex().justify_end().child(
                                Button::new("import-eft")
                                    .primary()
                                    .label("Import")
                                    .on_click({
                                        let submit = Rc::clone(&submit);
                                        move |e: &ClickEvent, w: &mut Window, cx: &mut App| {
                                            submit(e, w, cx);
                                        }
                                    }),
                            ),
                        ),
                )
        });
    }

    pub(super) fn duplicate_fit(&mut self, cx: &mut Context<Self>) {
        if let Some(i) = self.saved.settings.current {
            self.copy_fit(i, cx);
        }
    }

    /// Add a copy of a fit named as pyfa names copies, and open it.
    pub(super) fn copy_fit(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(original) = self.saved.fits.get(index) else {
            return;
        };
        let copy = Fit {
            name: format!("{} copy", original.name),
            // A fresh identity, so links to the original stay with it.
            uid: 0,
            ..(**original).clone()
        };
        self.add_fit(copy, cx);
    }
}
