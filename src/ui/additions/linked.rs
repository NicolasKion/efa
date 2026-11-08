//! Pieces the Projected, Command and Fighters tabs share: editing the current fit, linking other
//! saved fits to it, a value prompt that takes any closure, and pyfa's "Add Fit..." browser.

use std::{rc::Rc, str::FromStr};

use gpui_kit::component::{
    ActiveTheme, IconName, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{
    AppContext, ClickEvent, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    Render, SharedString, StatefulInteractiveElement, Styled, Subscription, WeakEntity, Window,
    div, prelude::FluentBuilder, px,
};

use crate::{
    data::Catalog,
    fit::Fit,
    icons::Icon,
    ui::{
        Efa, Loaded,
        widgets::{entry, icon_box, icon_label},
    },
};

/// What a picked set of fits is handed to.
type OnPick = Rc<dyn Fn(&mut Efa, Vec<usize>, &mut Context<Efa>)>;

const PICKER_ROW_H: f32 = 30.;
const PICKER_H: f32 = 360.;
/// The fit picker dialog without its list: margins, title, search box and button.
const PICKER_AROUND: f32 = 220.;

/// pyfa's label for a fit in menus: "Ship: Fit".
pub(super) fn fit_label(catalog: &Catalog, fit: &Fit) -> String {
    match fit.ship {
        Some(ship) => format!("{}: {}", catalog.name(ship), fit.name),
        None => fit.name.clone(),
    }
}

impl Efa {
    /// Change the current fit and recalculate. `edit` says whether rows came or went, which
    /// clears the selection since it holds row positions.
    pub(super) fn edit_current(
        &mut self,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut Fit, &Catalog) -> bool,
    ) {
        let Some(loaded) = self.loaded() else { return };
        let Some(i) = self.saved.current else { return };
        if edit(&mut self.saved.fits[i], &loaded.catalog) {
            self.clear_selection();
        }
        self.changed(cx);
    }

    /// Hand `link` the identity of saved fit `index`, giving it one first if it has none, to
    /// record on the current fit.
    pub(super) fn link_fits(
        &mut self,
        indices: &[usize],
        cx: &mut Context<Self>,
        link: impl Fn(&mut Fit, u64),
    ) {
        let uids: Vec<u64> = indices
            .iter()
            .filter_map(|&i| self.saved.ensure_uid(i))
            .collect();
        if uids.is_empty() {
            return;
        }
        self.edit_current(cx, |fit, _| {
            for uid in uids {
                link(fit, uid);
            }
            true
        });
    }

    /// Ask for a value and hand it to `apply`. Input that does not parse leaves the prompt open.
    pub(super) fn open_input_dialog<T: FromStr + 'static>(
        title: &'static str,
        prompt: &'static str,
        initial: String,
        window: &mut Window,
        cx: &mut Context<Self>,
        apply: impl Fn(&mut Efa, T, &mut Context<Efa>) + 'static,
    ) {
        let input = cx.new(|cx| InputState::new(window, cx).default_value(initial));
        let this = cx.weak_entity();
        let apply = Rc::new(apply);
        window.open_dialog(cx, move |dialog, _, _| {
            let (input, this, apply) = (input.clone(), this.clone(), Rc::clone(&apply));
            // The OK button and Enter both submit; it closes the dialog itself when done.
            let submit: crate::ui::widgets::Submit = {
                let (input, this, apply) = (input.clone(), this.clone(), apply.clone());
                Rc::new(
                    move |_: &ClickEvent, window: &mut Window, cx: &mut gpui_kit::App| {
                        let Ok(value) = input.read(cx).value().trim().parse::<T>() else {
                            return;
                        };
                        window.close_dialog(cx);
                        Efa::app_update(&this, cx, |g, cx| apply(g, value, cx));
                    },
                )
            };
            dialog
                .on_ok({
                    let submit = Rc::clone(&submit);
                    move |e, w, cx| {
                        submit(e, w, cx);
                        false
                    }
                })
                .title(title)
                .w(px(340.))
                .child(
                    v_flex()
                        .gap_3()
                        .child(prompt)
                        .child(Input::new(&input))
                        .child(h_flex().justify_end().child(
                            Button::new("input-ok").primary().label("OK").on_click({
                                let submit = Rc::clone(&submit);
                                move |e: &ClickEvent, w: &mut Window, cx: &mut gpui_kit::App| {
                                    submit(e, w, cx);
                                }
                            }),
                        )),
                )
        });
    }

    /// pyfa's "Add Fit..." browser: saved fits, narrowed by a search, to pick one or more of.
    /// Fits whose identity is in `exclude` are left out.
    pub(super) fn open_fit_picker(
        &mut self,
        title: &'static str,
        loaded: &Rc<Loaded>,
        exclude: &[u64],
        window: &mut Window,
        cx: &mut Context<Self>,
        on_pick: impl Fn(&mut Efa, Vec<usize>, &mut Context<Efa>) + 'static,
    ) {
        // Gathered here: the picker is built while this view is being updated and cannot read
        // it.
        let mut fits: Vec<PickerFit> = self
            .saved
            .fits
            .iter()
            .enumerate()
            .filter(|(_, f)| f.uid == 0 || !exclude.contains(&f.uid))
            .map(|(index, f)| PickerFit {
                index,
                ship: f.ship,
                label: fit_label(&loaded.catalog, f),
            })
            .collect();
        fits.sort_by(|a, b| a.label.cmp(&b.label));
        let efa = cx.weak_entity();
        let loaded = Rc::clone(loaded);
        let picker = cx.new(|cx| FitPicker::new(efa, loaded, fits, Rc::new(on_pick), window, cx));
        window.open_dialog(cx, move |dialog, _, _| {
            dialog.title(title).w(px(460.)).child(picker.clone())
        });
    }
}

struct PickerFit {
    index: usize,
    ship: Option<i32>,
    label: String,
}

/// The browser's own state; the fits come in as a snapshot.
struct FitPicker {
    efa: WeakEntity<Efa>,
    loaded: Rc<Loaded>,
    fits: Vec<PickerFit>,
    picked: Vec<usize>,
    search: Entity<InputState>,
    on_pick: OnPick,
    _subscriptions: Vec<Subscription>,
}

impl FitPicker {
    fn new(
        efa: WeakEntity<Efa>,
        loaded: Rc<Loaded>,
        fits: Vec<PickerFit>,
        on_pick: OnPick,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search fits"));
        let subscriptions = vec![cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                cx.notify();
            }
        })];
        Self {
            efa,
            loaded,
            fits,
            picked: Vec::new(),
            search,
            on_pick,
            _subscriptions: subscriptions,
        }
    }

    fn finish(&mut self, indices: Vec<usize>, window: &mut Window, cx: &mut Context<Self>) {
        window.close_dialog(cx);
        if indices.is_empty() {
            return;
        }
        let on_pick = Rc::clone(&self.on_pick);
        Efa::app_update(&self.efa, cx, |g, cx| on_pick(g, indices, cx));
    }
}

impl Render for FitPicker {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let query = self.search.read(cx).value().to_lowercase();
        let words: Vec<&str> = query.split_whitespace().collect();
        let theme = cx.theme();
        let rows: Vec<_> = self
            .fits
            .iter()
            .filter(|f| {
                let label = f.label.to_lowercase();
                words.iter().all(|w| label.contains(w))
            })
            .map(|f| {
                let index = f.index;
                let picked = self.picked.contains(&index);
                h_flex()
                    .id(SharedString::from(format!("pick-fit-{index}")))
                    .gap_2()
                    .px_2()
                    .h(px(PICKER_ROW_H))
                    .rounded(px(4.))
                    .when(picked, |r| r.bg(theme.table_active))
                    .hover(|s| s.bg(theme.table_hover))
                    .when_some(f.ship, |row, ship| {
                        row.child(icon_box(
                            &self.loaded.catalog,
                            &self.loaded.icons,
                            Icon::Type(ship),
                            24.,
                        ))
                    })
                    .child(div().flex_1().min_w_0().truncate().child(f.label.clone()))
                    .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                        if event.click_count() >= 2 {
                            this.finish(vec![index], window, cx);
                            return;
                        }
                        match this.picked.iter().position(|&p| p == index) {
                            Some(at) => {
                                this.picked.remove(at);
                            }
                            None => this.picked.push(index),
                        }
                        cx.notify();
                    }))
            })
            .collect();
        let empty = rows.is_empty();
        v_flex()
            .gap_3()
            .child(Input::new(&self.search).small())
            .child(
                v_flex()
                    .id("fit-picker-list")
                    .h(crate::ui::widgets::dialog_fill_h(
                        window,
                        PICKER_H,
                        PICKER_AROUND,
                    ))
                    .overflow_y_scroll()
                    .children(rows)
                    .when(empty, |list| {
                        list.child(
                            div()
                                .p_2()
                                .text_color(theme.muted_foreground)
                                .child("No fits to add."),
                        )
                    }),
            )
            .child(
                h_flex().justify_end().gap_2().child(
                    Button::new("fit-picker-ok")
                        .primary()
                        .label("Add")
                        .on_click(cx.listener(|this, _, window, cx| {
                            let picked = std::mem::take(&mut this.picked);
                            this.finish(picked, window, cx);
                        })),
                ),
            )
    }
}

/// Which list of the current fit other fits are added to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::ui) enum Link {
    Projected,
    Command,
}

impl Link {
    fn apply(self, fit: &mut Fit, uid: u64) {
        match self {
            Self::Projected => fit.projected.add_fit(uid),
            Self::Command => {
                fit.add_command_fit(uid);
            }
        }
    }

    fn existing(self, fit: &Fit) -> Vec<u64> {
        match self {
            Self::Projected => fit.projected.fits.iter().map(|p| p.fit).collect(),
            Self::Command => fit.command.iter().map(|c| c.fit).collect(),
        }
    }

    fn title(self) -> &'static str {
        match self {
            Self::Projected => "Add Projected Fits",
            Self::Command => "Add Command Fits",
        }
    }
}

impl Efa {
    /// Add saved fits, by index, to the current fit's projected or command list.
    pub(in crate::ui) fn link(&mut self, link: Link, indices: &[usize], cx: &mut Context<Self>) {
        self.link_fits(indices, cx, |fit, uid| link.apply(fit, uid));
    }
}

/// pyfa's fitAddBrowse and fitAddCurrentlyOpen entries.
pub(super) fn add_fit_entries(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    loaded: &Rc<Loaded>,
    link: Link,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let Some(g) = this.upgrade() else { return menu };
    let g = g.read(cx);
    let Some(fit) = g.fit() else { return menu };
    let existing = link.existing(fit);
    let open: Vec<(usize, String)> = g
        .saved
        .open
        .iter()
        .filter_map(|&i| Some((i, fit_label(&loaded.catalog, g.saved.fits.get(i)?))))
        .collect();
    let mut menu = {
        let (this, loaded) = (this.clone(), Rc::clone(loaded));
        menu.item(
            entry(IconName::FolderOpen, "Add Fit...").on_click(move |_, window, cx| {
                let existing = existing.clone();
                Efa::app_update(&this, cx, |g, cx| {
                    g.open_fit_picker(
                        link.title(),
                        &loaded,
                        &existing,
                        window,
                        cx,
                        move |g, picked, cx| {
                            g.link(link, &picked, cx);
                        },
                    );
                });
            }),
        )
    };
    if !open.is_empty() {
        let this = this.clone();
        menu = menu.submenu_with_element(
            "Add Currently Open Fit",
            icon_label(IconName::Plus, "Add Currently Open Fit"),
            window,
            cx,
            move |menu, _, _| {
                open.iter().fold(menu, |menu, (index, label)| {
                    let (this, index) = (this.clone(), *index);
                    menu.item(PopupMenuItem::new(label.clone()).on_click(move |_, _, cx| {
                        Efa::app_update(&this, cx, |g, cx| g.link(link, &[index], cx));
                    }))
                })
            },
        );
    }
    menu
}

/// A saved fit dragged from the fittings browser, which the Projected and Command tabs take.
#[derive(Debug, Clone)]
pub struct FitDrag {
    pub index: usize,
    pub label: SharedString,
}

impl Render for FitDrag {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .px_2()
            .py_1()
            .rounded(px(4.))
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .border_1()
            .border_color(theme.ring)
            .shadow_md()
            .child(self.label.clone())
    }
}
