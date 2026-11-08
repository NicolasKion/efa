//! Fit validation in the window: red rows with a tooltip saying why, refusing modules the ship
//! cannot take, and pyfa's fitting restrictions toggle (fitCommands/gui/fitRestrictionToggle.py).

use gpui_kit::component::{
    WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    tooltip::Tooltip,
    v_flex,
};
use gpui_kit::{
    Context, Div, Hsla, ParentElement, SharedString, Stateful, StatefulInteractiveElement, Styled,
    Window, px,
};

use super::{
    Efa, Loaded,
    actions::{Outcome, RowRef},
};
use crate::{
    data::Slot,
    engine::ItemRef,
    fit::{Fit, trim_rack},
};

impl From<RowRef> for ItemRef {
    fn from(row: RowRef) -> Self {
        match row {
            RowRef::Module(slot, i) => Self::Module(slot, i),
            RowRef::Rig(i) => Self::Rig(i),
            RowRef::Subsystem(i) => Self::Subsystem(i),
        }
    }
}

/// Red text and a tooltip with the reasons, for a row of an item that breaks a rule.
pub(super) fn flag(el: Stateful<Div>, why: SharedString, danger: Hsla) -> Stateful<Div> {
    el.text_color(danger)
        .tooltip(move |window, cx| Tooltip::new(why.clone()).build(window, cx))
}

impl Efa {
    /// What is wrong with an item of the current fit, one reason per line.
    pub(super) fn problems_of(&self, loaded: &Loaded, item: ItemRef) -> Option<SharedString> {
        let stats = self.stats.as_ref()?;
        let lines: Vec<String> = stats
            .validation
            .of(item)
            .map(|p| p.describe(|t| loaded.catalog.name(t)))
            .collect();
        (!lines.is_empty()).then(|| lines.join("\n").into())
    }

    /// Ask first, as pyfa does, then switch the current fit's restrictions.
    pub(super) fn confirm_restrictions_toggle(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(fit) = self.fit() else { return };
        let message = if fit.ignore_restrictions {
            "Re-enabling fitting restrictions for this fit will also remove any illegal items \
             from the fit. Do you want to continue?"
        } else {
            "Are you sure you wish to ignore fitting restrictions for the current fit? This \
             could lead to wildly inaccurate results and possible errors."
        };
        let this = cx.weak_entity();
        window.open_dialog(cx, move |dialog, _, _| {
            let yes = this.clone();
            dialog.title("Confirm").w(px(420.)).child(
                v_flex().gap_3().child(message).child(
                    h_flex()
                        .justify_end()
                        .gap_2()
                        .child(
                            Button::new("restrictions-no")
                                .label("No")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("restrictions-yes")
                                .primary()
                                .label("Yes")
                                .on_click(move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let _ = yes.update(cx, Efa::toggle_restrictions);
                                }),
                        ),
                ),
            )
        });
    }

    /// Switch the current fit's restrictions. Turning them back on takes off whatever the ship
    /// cannot take, last first, as pyfa does; one undo step brings it all back.
    pub(super) fn toggle_restrictions(&mut self, cx: &mut Context<Self>) {
        let Some(current) = self.saved.current else {
            return;
        };
        self.edit_fit(
            current,
            false,
            |fit, env| {
                fit.ignore_restrictions = !fit.ignore_restrictions;
                if !fit.ignore_restrictions {
                    remove_illegal(fit, |fit| env.loaded.engine.validate(fit, env.pilot));
                }
                Outcome {
                    structural: true,
                    ..Outcome::default()
                }
            },
            cx,
        );
    }
}

/// Take items the ship cannot take off one at a time from the bottom, checking again after each:
/// of two modules over a group limit only the second goes.
fn remove_illegal(fit: &mut Fit, validate: impl Fn(&Fit) -> crate::engine::Validation) {
    loop {
        let validation = validate(fit);
        let last = validation
            .items
            .iter()
            .filter(|(_, p)| p.removes_on_restrict())
            .filter_map(|&(item, _)| match item {
                ItemRef::Module(slot, i) => Some(RowRef::Module(slot, i)),
                ItemRef::Rig(i) => Some(RowRef::Rig(i)),
                ItemRef::Subsystem(i) => Some(RowRef::Subsystem(i)),
                _ => None,
            })
            .max();
        let Some(row) = last else { return };
        let removed = match row {
            RowRef::Module(slot, i) => fit
                .rack_mut(slot)
                .get_mut(i)
                .and_then(Option::take)
                .is_some(),
            RowRef::Rig(i) => fit.rigs.get_mut(i).and_then(Option::take).is_some(),
            RowRef::Subsystem(i) => (i < fit.subsystems.len())
                .then(|| fit.subsystems.remove(i))
                .is_some(),
        };
        if !removed {
            return;
        }
        for slot in [Slot::High, Slot::Mid, Slot::Low] {
            trim_rack(fit.rack_mut(slot));
        }
        trim_rack(&mut fit.rigs);
    }
}
