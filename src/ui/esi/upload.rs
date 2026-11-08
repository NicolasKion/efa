//! pyfa's Export fit to EVE (gui/esiFittings.py `ExportToEve`): save the fit as a fitting of a
//! logged-in character, with loaded charges, implants and boosters in the cargo if wanted.

use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme, Disableable, IconName, Sizable,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    h_flex,
    menu::{DropdownMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{
    AnyElement, Context, Entity, FontWeight, InteractiveElement, IntoElement, ParentElement,
    Render, SharedString, StatefulInteractiveElement, Styled, Subscription, Task, Window, div,
    prelude::FluentBuilder,
};

use super::{call, characters::login_status, pick};
use crate::{
    fit::Fit,
    icons::Icon,
    port::esi::{UploadOptions, upload},
    ui::{Hub, Loaded, widgets::icon_box},
};

pub struct ExportToEveWindow {
    hub: Entity<Hub>,
    loaded: Rc<Loaded>,
    fit: Fit,
    character: Option<u64>,
    busy: Option<Task<()>>,
    message: Option<(SharedString, bool)>,
    _subscriptions: Vec<Subscription>,
}

impl ExportToEveWindow {
    pub fn new(
        hub: Entity<Hub>,
        loaded: Rc<Loaded>,
        fit: Fit,
        preferred: Option<u64>,
        cx: &mut Context<Self>,
    ) -> Self {
        let subscriptions = vec![cx.observe(&hub, |w: &mut Self, hub, cx| {
            w.character = pick(&hub.read(cx).esi.accounts, w.character);
            cx.notify();
        })];
        let character = pick(&hub.read(cx).esi.accounts, preferred);
        Self {
            hub,
            loaded,
            fit,
            character,
            busy: None,
            message: None,
            _subscriptions: subscriptions,
        }
    }

    fn options(&self, cx: &Context<Self>) -> UploadOptions {
        self.hub.read(cx).saved.settings.preferences.esi.upload
    }

    fn set_option(&self, cx: &mut Context<Self>, apply: impl FnOnce(&mut UploadOptions)) {
        self.hub.update(cx, |hub, cx| {
            hub.update_saved(|s| apply(&mut s.settings.preferences.esi.upload), cx);
        });
    }

    fn export(&mut self, cx: &mut Context<Self>) {
        let Some(character) = self.character else {
            return;
        };
        let json = match upload(&self.fit, &self.loaded.catalog, self.options(cx)) {
            Ok(json) => json,
            Err(e) => {
                self.message = Some((format!("Cannot export: {e}").into(), true));
                cx.notify();
                return;
            }
        };
        self.message = Some(("Sending the fit to EVE".into(), false));
        self.busy = Some(call(
            &self.hub,
            character,
            cx,
            move |session| session.add_fitting(json),
            |w: &mut Self, result, cx| {
                w.busy = None;
                w.message = Some(match result {
                    Ok(_) => ("Saved in the game's fittings.".into(), false),
                    Err(e) => (e.into(), true),
                });
                cx.notify();
            },
        ));
        cx.notify();
    }

    fn character_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let accounts = self.hub.read(cx).esi.accounts.clone();
        let label: SharedString = self
            .character
            .and_then(|id| accounts.name(id))
            .map_or_else(|| "No ESI character".into(), |n| n.to_owned().into());
        let current = self.character;
        let choices: Vec<(u64, String)> = accounts
            .sorted()
            .into_iter()
            .map(|a| (a.character_id, a.name.clone()))
            .collect();
        let this = cx.weak_entity();
        Button::new("export-eve-character")
            .small()
            .outline()
            .icon(IconName::CircleUser)
            .label(label)
            .disabled(choices.is_empty())
            .dropdown_menu(move |menu, _, _| {
                choices.iter().fold(menu, |menu, (id, name)| {
                    let (this, id) = (this.clone(), *id);
                    menu.item(
                        PopupMenuItem::new(name.clone())
                            .checked(current == Some(id))
                            .on_click(move |_, _, cx| {
                                let _ = this.update(cx, |w, cx| {
                                    w.character = Some(id);
                                    cx.notify();
                                });
                            }),
                    )
                })
            })
            .into_any_element()
    }
}

impl Render for ExportToEveWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let options = self.options(cx);
        let picker = self.character_picker(cx);
        let busy = self.busy.is_some();
        let catalog = &self.loaded.catalog;
        let ship = self.fit.ship.unwrap_or_default();
        let checkbox = |id: &'static str,
                        label: &'static str,
                        checked: bool,
                        apply: fn(&mut UploadOptions, bool),
                        cx: &mut Context<Self>| {
            Checkbox::new(id)
                .label(label)
                .checked(checked)
                .on_click(cx.listener(move |w, value: &bool, _, cx| {
                    let value = *value;
                    w.set_option(cx, |o| apply(o, value));
                }))
        };
        let no_character = self.character.is_none();
        let theme = cx.theme();
        let message = self.message.clone();
        // What to export scrolls when the window is short; the status and buttons stay.
        let body = v_flex()
            .id("export-eve-body")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .gap_3()
            .child(
                h_flex()
                    .gap_3()
                    .child(icon_box(catalog, &self.loaded.icons, Icon::Type(ship), 40.))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .truncate()
                                    .child(self.fit.name.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .truncate()
                                    .text_color(theme.muted_foreground)
                                    .child(catalog.name(ship).clone()),
                            ),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .child(div().text_color(theme.muted_foreground).child("Save to"))
                    .child(picker),
            )
            .child(
                v_flex()
                    .gap_2()
                    .child(checkbox(
                        "export-charges",
                        "Loaded charges, into the cargo",
                        options.charges,
                        |o, v| o.charges = v,
                        cx,
                    ))
                    .child(checkbox(
                        "export-implants",
                        "Implants, into the cargo",
                        options.implants,
                        |o, v| o.implants = v,
                        cx,
                    ))
                    .child(checkbox(
                        "export-boosters",
                        "Boosters, into the cargo",
                        options.boosters,
                        |o, v| o.boosters = v,
                        cx,
                    )),
            );
        v_flex()
            .size_full()
            .p_4()
            .gap_3()
            .child(body)
            .child(login_status(&self.hub, cx))
            .when_some(message, |col, (text, error)| {
                col.child(
                    div()
                        .text_xs()
                        .text_color(if error {
                            cx.theme().danger
                        } else {
                            cx.theme().muted_foreground
                        })
                        .child(text),
                )
            })
            .when(no_character, |col| {
                col.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child("Log a character in with Character > Manage ESI Characters first."),
                )
            })
            .child(
                h_flex()
                    .flex_none()
                    .gap_2()
                    .justify_end()
                    .child(
                        Button::new("export-eve-close")
                            .small()
                            .label("Close")
                            .on_click(|_, window, _| window.remove_window()),
                    )
                    .child(
                        Button::new("export-eve-go")
                            .small()
                            .primary()
                            .icon(IconName::ArrowUp)
                            .label("Export Fit")
                            .loading(busy)
                            .disabled(busy || no_character)
                            .on_click(cx.listener(|w, _, _, cx| w.export(cx))),
                    ),
            )
    }
}
