//! pyfa's SSO Character Management (gui/esiFittings.py `SsoCharacterMgmt`): the characters
//! logged in with EVE SSO, with Add and Remove.

use gpui_kit::component::{
    ActiveTheme, Disableable, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use gpui_kit::{
    App, ClickEvent, Context, Entity, FontWeight, InteractiveElement, IntoElement, ParentElement,
    Render, SharedString, StatefulInteractiveElement, Styled, Subscription, Window, div,
    prelude::FluentBuilder, px,
};

use crate::ui::Hub;

pub struct EsiCharactersWindow {
    hub: Entity<Hub>,
    selected: Option<u64>,
    _subscriptions: Vec<Subscription>,
}

impl EsiCharactersWindow {
    pub fn new(hub: Entity<Hub>, _window: &mut Window, cx: &mut Context<Self>) -> Self {
        let subscriptions = vec![cx.observe(&hub, |_, _, cx| cx.notify())];
        Self {
            hub,
            selected: None,
            _subscriptions: subscriptions,
        }
    }

    fn add(&mut self, cx: &mut Context<Self>) {
        let this = cx.weak_entity();
        self.hub.update(cx, |hub, cx| {
            hub.esi_login(cx, move |result, cx| {
                if let Ok(id) = result {
                    let _ = this.update(cx, |w, cx| {
                        w.selected = Some(id);
                        cx.notify();
                    });
                }
            });
        });
    }

    fn remove(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.selected.take() else {
            return;
        };
        self.hub.update(cx, |hub, cx| hub.esi_remove(id, cx));
    }
}

/// The login status line with a Cancel button while a login is waiting, shared by the windows
/// that can log a character in.
pub(in crate::ui) fn login_status<V: 'static>(
    hub: &Entity<Hub>,
    cx: &mut Context<V>,
) -> impl IntoElement {
    let (waiting, status) = {
        let esi = &hub.read(cx).esi;
        (esi.login.is_some(), esi.status.clone())
    };
    let hub = hub.clone();
    h_flex()
        .gap_2()
        .min_h(px(24.))
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(div().flex_1().min_w_0().children(status))
        .when(waiting, |row| {
            row.child(
                Button::new("esi-login-cancel")
                    .small()
                    .ghost()
                    .label("Cancel")
                    .on_click(move |_: &ClickEvent, _: &mut Window, cx: &mut App| {
                        hub.update(cx, |hub, cx| {
                            hub.esi_cancel_login();
                            cx.notify();
                        });
                    }),
            )
        })
}

impl Render for EsiCharactersWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (accounts, waiting) = {
            let esi = &self.hub.read(cx).esi;
            (esi.accounts.clone(), esi.login.is_some())
        };
        if self.selected.is_some_and(|id| accounts.get(id).is_none()) {
            self.selected = None;
        }
        let theme = cx.theme();
        let rows: Vec<_> = accounts
            .sorted()
            .into_iter()
            .map(|account| {
                let id = account.character_id;
                let selected = self.selected == Some(id);
                let missing = account.missing_scopes();
                let note: SharedString = if missing.is_empty() {
                    format!("Character ID {id}").into()
                } else {
                    format!("Missing: {}", missing.join(", ")).into()
                };
                h_flex()
                    .id(SharedString::from(format!("esi-char-{id}")))
                    .gap_3()
                    .px_3()
                    .py_2()
                    .rounded(px(6.))
                    .when(selected, |r| r.bg(theme.table_active))
                    .hover(|s| s.bg(theme.table_hover))
                    .child(
                        gpui_kit::component::Icon::new(IconName::CircleUser)
                            .size(px(20.))
                            .text_color(theme.muted_foreground),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .truncate()
                                    .child(account.name.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .when(missing.is_empty(), |d| {
                                        d.text_color(theme.muted_foreground)
                                    })
                                    .when(!missing.is_empty(), |d| d.text_color(theme.danger))
                                    .child(note),
                            ),
                    )
                    .on_click(cx.listener(move |w, _: &ClickEvent, _, cx| {
                        w.selected = Some(id);
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect();
        let empty = rows.is_empty();
        v_flex()
            .size_full()
            .p_4()
            .gap_3()
            .child(
                div().text_xs().text_color(theme.muted_foreground).child(
                    "Characters efa may read skills and fittings for, and save fittings to.",
                ),
            )
            .child(
                v_flex()
                    .id("esi-characters")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p_1()
                    .border_1()
                    .border_color(theme.border)
                    .rounded(px(6.))
                    .children(rows)
                    .when(empty, |list| {
                        list.child(
                            div()
                                .p_4()
                                .text_color(theme.muted_foreground)
                                .child("No characters logged in yet."),
                        )
                    }),
            )
            .child(login_status(&self.hub, cx))
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(
                        Button::new("esi-add")
                            .small()
                            .primary()
                            .icon(IconName::Plus)
                            .label("Add Character")
                            .loading(waiting)
                            .disabled(waiting)
                            .on_click(cx.listener(|w, _, _, cx| w.add(cx))),
                    )
                    .child(
                        Button::new("esi-remove")
                            .small()
                            .icon(IconName::Delete)
                            .label("Remove Character")
                            .disabled(self.selected.is_none())
                            .on_click(cx.listener(|w, _, _, cx| w.remove(cx))),
                    )
                    .child(div().flex_1())
                    .child(
                        Button::new("esi-close")
                            .small()
                            .label("Close")
                            .on_click(|_, window, _| window.remove_window()),
                    ),
            )
    }
}
