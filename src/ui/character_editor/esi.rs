//! The EVE SSO tab: linking a login to the character and fetching its skills.

use gpui_kit::component::{
    ActiveTheme, Disableable, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    menu::{DropdownMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{
    AnyElement, Context, IntoElement, ParentElement, Styled, div, prelude::FluentBuilder,
};

use super::CharacterEditor;
use crate::character::CharacterId;

/// The EVE SSO tab.
impl CharacterEditor {
    /// pyfa's Get Skills: skills, implants and security status from ESI, for the linked
    /// character.
    fn get_skills(&mut self, cx: &mut Context<Self>) {
        let id = self.selected;
        let Some(esi) = self.characters.esi_character(id) else {
            return;
        };
        self.esi_message = Some(("Fetching skills".into(), false));
        self.esi_fetch = Some(crate::ui::esi::call(
            &self.hub,
            esi,
            cx,
            crate::esi::api::Session::sheet,
            move |e: &mut Self, result, cx| {
                e.esi_fetch = None;
                e.esi_message = Some(match result {
                    Ok(sheet) => {
                        e.edit(cx, |c, loaded| {
                            c.apply_esi(
                                id,
                                &sheet.skills,
                                sheet.implants.as_deref(),
                                sheet.sec_status,
                                &loaded.catalog,
                            );
                        });
                        let implants = match &sheet.implants {
                            Some(i) => format!(", {} implants", i.len()),
                            None => String::new(),
                        };
                        (
                            format!("Fetched {} skills{implants}.", sheet.skills.len()).into(),
                            false,
                        )
                    }
                    Err(error) => (error.into(), true),
                });
                cx.notify();
            },
        ));
        cx.notify();
    }

    fn link_esi(&mut self, esi: Option<u64>, cx: &mut Context<Self>) {
        let id = self.selected;
        self.esi_message = None;
        self.edit(cx, |c, _| c.link_esi(id, esi));
    }

    /// Log in with EVE. The new EVE character is linked to the character shown; a built-in one
    /// cannot be linked, so a new character named after the EVE one is made and filled in.
    fn esi_login(&mut self, cx: &mut Context<Self>) {
        let target = self.selected;
        let this = cx.weak_entity();
        self.hub.update(cx, |hub, cx| {
            hub.esi_login(cx, move |result, cx| {
                if let Ok(esi) = result {
                    let _ = this.update(cx, |e, cx| e.logged_in(target, esi, cx));
                }
            });
        });
    }

    fn logged_in(&mut self, target: CharacterId, esi: u64, cx: &mut Context<Self>) {
        if !target.is_builtin() && self.characters.get(target).is_some() {
            self.selected = target;
            self.link_esi(Some(esi), cx);
            return;
        }
        let base = self
            .hub
            .read(cx)
            .esi
            .accounts
            .name(esi)
            .unwrap_or("EVE character")
            .to_owned();
        // A name nobody has yet: the EVE name, else with a number after it.
        let name = (1..1000)
            .map(|n| {
                if n == 1 {
                    base.clone()
                } else {
                    format!("{base} {n}")
                }
            })
            .find(|name| self.characters.name_problem(name).is_none())
            .unwrap_or(base);
        let id = self.edit(cx, |c, _| {
            let id = c.create(&name);
            c.link_esi(id, Some(esi));
            id
        });
        self.selected = id;
        self.skill = None;
        self.get_skills(cx);
    }

    fn esi_picker(&self, linked: Option<u64>, cx: &mut Context<Self>) -> AnyElement {
        let accounts = self.hub.read(cx).esi.accounts.clone();
        let label = match linked {
            None => "None".to_owned(),
            Some(id) => accounts
                .name(id)
                .map_or_else(|| format!("Logged out ({id})"), str::to_owned),
        };
        let choices: Vec<(Option<u64>, String)> = std::iter::once((None, "None".to_owned()))
            .chain(
                accounts
                    .sorted()
                    .into_iter()
                    .map(|a| (Some(a.character_id), a.name.clone())),
            )
            .collect();
        let this = cx.weak_entity();
        Button::new("esi-link")
            .small()
            .outline()
            .label(label)
            .dropdown_menu(move |menu, _, _| {
                choices.iter().fold(menu, |menu, (id, name)| {
                    let (this, id) = (this.clone(), *id);
                    menu.item(
                        PopupMenuItem::new(name.clone())
                            .checked(linked == id)
                            .on_click(move |_, _, cx| {
                                let _ = this.update(cx, |e, cx| e.link_esi(id, cx));
                            }),
                    )
                })
            })
            .into_any_element()
    }

    pub(super) fn esi_tab(&self, cx: &mut Context<Self>) -> AnyElement {
        let builtin = self.selected.is_builtin();
        let linked = self.characters.esi_character(self.selected);
        let logged_in = linked.is_some_and(|id| self.hub.read(cx).esi.accounts.get(id).is_some());
        let busy = self.esi_fetch.is_some();
        let waiting = self.hub.read(cx).esi.login.is_some();
        let picker = (!builtin).then(|| self.esi_picker(linked, cx));
        let status = crate::ui::esi::login_status(&self.hub, cx).into_any_element();
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let link = v_flex()
            .gap_2()
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(div().text_color(muted).child("EVE character"))
                    .children(picker)
                    .child(div().flex_1())
                    .child(
                        Button::new("esi-get-skills")
                            .small()
                            .primary()
                            .icon(IconName::RefreshCw)
                            .label("Get Skills")
                            .loading(busy)
                            .disabled(builtin || !logged_in || busy)
                            .on_click(cx.listener(|e, _, _, cx| e.get_skills(cx))),
                    ),
            )
            .child(div().text_xs().text_color(muted).child(
                "Get Skills replaces this character's skills, implants and security status with \
                 what EVE has, and saves them.",
            ))
            .when_some(self.esi_message.clone(), |col, (text, error)| {
                col.child(
                    div()
                        .text_xs()
                        .text_color(if error { theme.danger } else { muted })
                        .child(text),
                )
            });
        let tip = div().text_color(muted).child(
            "All 0 and All 5 cannot be linked to an EVE character. Pick another character, or log \
             one in below to make a new character from it.",
        );
        v_flex()
            .flex_1()
            .gap_4()
            .p_3()
            .child(if builtin {
                tip.into_any_element()
            } else {
                link.into_any_element()
            })
            .child(div().flex_1())
            .child(div().h_px().bg(theme.border))
            .child(
                v_flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .text_color(muted)
                            .child("Don't see your EVE character in the list?"),
                    )
                    .child(
                        Button::new("esi-login")
                            .small()
                            .icon(IconName::CircleUser)
                            .label("Log In with EVE SSO")
                            .loading(waiting)
                            .disabled(waiting)
                            .on_click(cx.listener(|e, _, _, cx| e.esi_login(cx))),
                    ),
            )
            .child(status)
            .into_any_element()
    }
}
