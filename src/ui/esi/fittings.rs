//! pyfa's Browse EVE Fittings (gui/esiFittings.py `EveFittings`): the fittings a character keeps
//! in the game, with their contents, to import into efa or delete from the game.

use std::{collections::BTreeSet, rc::Rc};

use gpui_kit::component::{
    ActiveTheme, Disableable, IconName, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{
    AnyElement, AppContext, ClickEvent, Context, Entity, FontWeight, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Task, Window, div, prelude::FluentBuilder, px, relative,
};

use super::{call, characters::login_status, pick};
use crate::{
    icons::Icon,
    port::{
        Imported,
        esi::{Fitting, to_fit},
    },
    ui::{
        Hub, Loaded,
        dialogs::{Confirm, confirm},
        widgets::{count, icon_box},
    },
};

/// Where an item of a fitting sits, for the contents pane, in pyfa's order.
fn section(flag: u32) -> (u8, &'static str) {
    match flag {
        27..=34 => (0, "High slots"),
        19..=26 => (1, "Mid slots"),
        11..=18 => (2, "Low slots"),
        92..=99 => (3, "Rigs"),
        125..=132 => (4, "Subsystems"),
        87 => (5, "Drones"),
        158 => (6, "Fighters"),
        _ => (7, "Cargo"),
    }
}

pub struct EsiFittingsWindow {
    hub: Entity<Hub>,
    loaded: Rc<Loaded>,
    character: Option<u64>,
    /// The fittings as last fetched, and for whom.
    fittings: Vec<Fitting>,
    fetched_for: Option<u64>,
    selected: BTreeSet<u64>,
    search: Entity<InputState>,
    busy: Option<Task<()>>,
    message: Option<(SharedString, bool)>,
    _subscriptions: Vec<Subscription>,
}

impl EsiFittingsWindow {
    pub fn new(
        hub: Entity<Hub>,
        loaded: Rc<Loaded>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search fittings"));
        let subscriptions = vec![
            cx.observe(&hub, |w: &mut Self, _, cx| w.sync_character(cx)),
            cx.subscribe(&search, |_, _, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            }),
        ];
        let character = pick(&hub.read(cx).esi.accounts, None);
        let mut this = Self {
            hub,
            loaded,
            character,
            fittings: Vec::new(),
            fetched_for: None,
            selected: BTreeSet::new(),
            search,
            busy: None,
            message: None,
            _subscriptions: subscriptions,
        };
        this.fetch(cx);
        this
    }

    /// Follow logins and removals: pick a character when there was none, drop a removed one.
    fn sync_character(&mut self, cx: &mut Context<Self>) {
        let picked = pick(&self.hub.read(cx).esi.accounts, self.character);
        if picked != self.character {
            self.character = picked;
            self.fittings.clear();
            self.fetched_for = None;
            self.selected.clear();
            self.fetch(cx);
        }
        cx.notify();
    }

    fn choose(&mut self, character: u64, cx: &mut Context<Self>) {
        if self.character != Some(character) {
            self.character = Some(character);
            self.fittings.clear();
            self.selected.clear();
            self.fetch(cx);
        }
    }

    fn fetch(&mut self, cx: &mut Context<Self>) {
        let Some(character) = self.character else {
            return;
        };
        self.message = Some(("Fetching fittings".into(), false));
        self.busy = Some(call(
            &self.hub,
            character,
            cx,
            crate::esi::api::Session::fittings,
            move |w: &mut Self, result, cx| {
                w.busy = None;
                match result {
                    Ok(fittings) => {
                        w.message =
                            Some((count(fittings.len(), "fitting", "fittings").into(), false));
                        w.fittings = fittings;
                        w.fetched_for = Some(character);
                        w.selected
                            .retain(|id| w.fittings.iter().any(|f| f.fitting_id == Some(*id)));
                    }
                    Err(e) => w.message = Some((e.into(), true)),
                }
                cx.notify();
            },
        ));
        cx.notify();
    }

    /// The fittings the search lets through, by ship name, then name.
    fn listed(&self, cx: &Context<Self>) -> Vec<&Fitting> {
        let catalog = &self.loaded.catalog;
        let query = self.search.read(cx).value().trim().to_lowercase();
        let mut listed: Vec<&Fitting> = self
            .fittings
            .iter()
            .filter(|f| {
                query.is_empty()
                    || f.name.to_lowercase().contains(&query)
                    || catalog.name(f.ship_type_id).to_lowercase().contains(&query)
            })
            .collect();
        listed.sort_by_cached_key(|f| (catalog.name(f.ship_type_id).clone(), f.name.clone()));
        listed
    }

    fn click(&mut self, id: u64, toggle: bool, cx: &mut Context<Self>) {
        if toggle {
            if !self.selected.remove(&id) {
                self.selected.insert(id);
            }
        } else {
            self.selected = BTreeSet::from([id]);
        }
        cx.notify();
    }

    fn import(&mut self, cx: &mut Context<Self>) {
        let catalog = &self.loaded.catalog;
        let mut all = Imported::default();
        let mut errors = Vec::new();
        for f in self
            .fittings
            .iter()
            .filter(|f| f.fitting_id.is_some_and(|id| self.selected.contains(&id)))
        {
            match to_fit(catalog, f) {
                Ok(imported) => {
                    all.fits.extend(imported.fits);
                    all.unknown.extend(imported.unknown);
                }
                Err(e) => errors.push(format!("{}: {e}", f.name)),
            }
        }
        let fits = all.fits.len();
        if fits > 0 {
            self.hub.update(cx, |hub, cx| hub.import_fits(all, cx));
        }
        self.message = Some(if errors.is_empty() {
            (
                format!("Imported {} into efa", count(fits, "fit", "fits")).into(),
                false,
            )
        } else {
            (errors.join("; ").into(), true)
        });
        cx.notify();
    }

    /// Ask, then delete `ids` from the game.
    fn confirm_delete(&mut self, ids: Vec<u64>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(character) = self.character else {
            return;
        };
        let catalog = &self.loaded.catalog;
        let question = match ids.as_slice() {
            [id] => {
                let Some(f) = self.fittings.iter().find(|f| f.fitting_id == Some(*id)) else {
                    return;
                };
                format!(
                    "Do you really want to delete {} ({}) from EVE?",
                    f.name,
                    catalog.name(f.ship_type_id)
                )
            }
            ids => format!(
                "Do you really want to delete {} fittings from EVE? This cannot be undone.",
                ids.len()
            ),
        };
        let this = cx.weak_entity();
        confirm(window, cx, Confirm::delete(question), move |_, cx| {
            let ids = ids.clone();
            let _ = this.update(cx, |w, cx| w.delete(character, ids, cx));
        });
    }

    fn delete(&mut self, character: u64, ids: Vec<u64>, cx: &mut Context<Self>) {
        self.message = Some(("Deleting".into(), false));
        self.busy = Some(call(
            &self.hub,
            character,
            cx,
            move |session| {
                let mut deleted = Vec::new();
                for id in ids {
                    match session.delete_fitting(id) {
                        Ok(()) => deleted.push(id),
                        Err(e) => return Ok((deleted, Some(e))),
                    }
                }
                Ok((deleted, None))
            },
            move |w: &mut Self, result: Result<(Vec<u64>, Option<String>), String>, cx| {
                w.busy = None;
                match result {
                    Ok((deleted, error)) => {
                        w.fittings
                            .retain(|f| f.fitting_id.is_none_or(|id| !deleted.contains(&id)));
                        for id in &deleted {
                            w.selected.remove(id);
                        }
                        w.message = Some(match error {
                            Some(e) => (e.into(), true),
                            None => (
                                format!(
                                    "Deleted {} from EVE",
                                    count(deleted.len(), "fitting", "fittings")
                                )
                                .into(),
                                false,
                            ),
                        });
                    }
                    Err(e) => w.message = Some((e.into(), true)),
                }
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
        Button::new("esi-fittings-character")
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
                                let _ = this.update(cx, |w, cx| w.choose(id, cx));
                            }),
                    )
                })
            })
            .into_any_element()
    }

    fn fitting_row(&self, f: &Fitting, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let catalog = &self.loaded.catalog;
        let id = f.fitting_id.unwrap_or_default();
        let selected = self.selected.contains(&id);
        h_flex()
            .id(SharedString::from(format!("esi-fit-{id}")))
            .gap_2()
            .px_2()
            .py_1()
            .rounded(px(4.))
            .when(selected, |r| r.bg(theme.table_active))
            .hover(|s| s.bg(theme.table_hover))
            .child(icon_box(
                catalog,
                &self.loaded.icons,
                Icon::Type(f.ship_type_id),
                32.,
            ))
            .child(
                v_flex()
                    .min_w_0()
                    .child(div().truncate().child(f.name.clone()))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .truncate()
                            .child(catalog.name(f.ship_type_id).clone()),
                    ),
            )
            .on_click(cx.listener(move |w, event: &ClickEvent, _, cx| {
                let toggle = event.modifiers().control || event.modifiers().platform;
                w.click(id, toggle, cx);
            }))
            .into_any_element()
    }

    /// The contents of the fitting picked last, by section.
    fn contents(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let catalog = &self.loaded.catalog;
        let shown = self
            .selected
            .iter()
            .next_back()
            .and_then(|id| self.fittings.iter().find(|f| f.fitting_id == Some(*id)));
        let Some(f) = shown else {
            return div()
                .p_4()
                .text_color(theme.muted_foreground)
                .child("Pick a fitting to see what is in it.")
                .into_any_element();
        };
        let mut items = f.items.clone();
        items.sort_by_key(|i| (section(i.flag), i.flag));
        let mut body = v_flex().gap_0p5();
        let mut last = None;
        for item in &items {
            let (order, title) = section(item.flag);
            if last != Some(order) {
                last = Some(order);
                body = body.child(
                    div()
                        .pt_2()
                        .text_xs()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .child(title.to_uppercase()),
                );
            }
            let name = if catalog.get(item.type_id).is_some() {
                catalog.name(item.type_id).clone()
            } else {
                format!("Unknown item #{}", item.type_id)
            };
            body = body.child(
                h_flex()
                    .gap_2()
                    .child(icon_box(
                        catalog,
                        &self.loaded.icons,
                        Icon::Type(item.type_id),
                        20.,
                    ))
                    .child(div().flex_1().truncate().child(name))
                    .when(item.quantity > 1, |r| {
                        r.child(
                            div()
                                .text_color(theme.muted_foreground)
                                .child(format!("x{}", item.quantity)),
                        )
                    }),
            );
        }
        v_flex()
            .gap_2()
            .child(
                h_flex()
                    .gap_3()
                    .child(icon_box(
                        catalog,
                        &self.loaded.icons,
                        Icon::Type(f.ship_type_id),
                        48.,
                    ))
                    .child(
                        v_flex()
                            .min_w_0()
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .truncate()
                                    .child(f.name.clone()),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(catalog.name(f.ship_type_id).clone()),
                            ),
                    ),
            )
            .when(!f.description.is_empty(), |d| {
                d.child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(f.description.clone()),
                )
            })
            .child(body)
            .into_any_element()
    }
}

/// The fittings list's width beside the contents.
const LIST_W: f32 = 380.;
/// Below this window width the contents go under the list.
const STACK_BELOW: f32 = 640.;
/// The search box keeps this much room, and goes to a line of its own when it cannot.
const SEARCH_MIN: f32 = 160.;

impl Render for EsiFittingsWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // A narrow window puts the fitting's contents under the list instead of beside it.
        let narrow = window.viewport_size().width < px(STACK_BELOW);
        let busy = self.busy.is_some();
        let has_character = self.character.is_some();
        let listed: Vec<Fitting> = self.listed(cx).into_iter().cloned().collect();
        let rows: Vec<AnyElement> = listed.iter().map(|f| self.fitting_row(f, cx)).collect();
        let contents = self.contents(cx);
        let picker = self.character_picker(cx);
        let selected: Vec<u64> = self.selected.iter().copied().collect();
        let all: Vec<u64> = self.fittings.iter().filter_map(|f| f.fitting_id).collect();
        let theme = cx.theme();
        let empty_text = if !has_character {
            "Log a character in with Manage ESI Characters (Character menu) to see its fittings."
        } else if self.fetched_for.is_none() {
            ""
        } else if self.fittings.is_empty() {
            "This character has no fittings saved in the game."
        } else {
            "No fitting matches the search."
        };
        let message = self.message.clone();
        v_flex()
            .size_full()
            .p_3()
            .gap_2()
            .child(
                h_flex()
                    .flex_wrap()
                    .gap_2()
                    .child(picker)
                    .child(
                        Button::new("esi-fetch")
                            .small()
                            .icon(IconName::RefreshCw)
                            .label("Fetch Fittings")
                            .loading(busy)
                            .disabled(busy || !has_character)
                            .on_click(cx.listener(|w, _, _, cx| w.fetch(cx))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(SEARCH_MIN))
                            .child(Input::new(&self.search).small()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .when(narrow, Styled::flex_col)
                    .flex_1()
                    .min_h_0()
                    .gap_2()
                    .child(
                        v_flex()
                            .id("esi-fittings-list")
                            .map(|list| {
                                if narrow {
                                    list.w_full().flex_1().min_h_0()
                                } else {
                                    list.flex_shrink_0()
                                        .w(px(LIST_W))
                                        .max_w(relative(0.5))
                                        .h_full()
                                }
                            })
                            .overflow_y_scroll()
                            .p_1()
                            .border_1()
                            .border_color(theme.border)
                            .rounded(px(6.))
                            .children(rows)
                            .when(listed.is_empty() && !empty_text.is_empty(), |list| {
                                list.child(
                                    div()
                                        .p_3()
                                        .text_color(theme.muted_foreground)
                                        .child(empty_text),
                                )
                            }),
                    )
                    .child(
                        v_flex()
                            .id("esi-fitting-contents")
                            .flex_1()
                            .min_w_0()
                            .min_h_0()
                            .when(!narrow, Styled::h_full)
                            .overflow_y_scroll()
                            .p_3()
                            .border_1()
                            .border_color(theme.border)
                            .rounded(px(6.))
                            .bg(theme.sidebar)
                            .child(contents),
                    ),
            )
            .child(login_status(&self.hub, cx))
            .child(
                h_flex()
                    .flex_wrap()
                    .justify_end()
                    .gap_2()
                    .when_some(message, |row, (text, error)| {
                        row.child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_xs()
                                .text_color(if error {
                                    cx.theme().danger
                                } else {
                                    cx.theme().muted_foreground
                                })
                                .child(text),
                        )
                    })
                    .child(
                        Button::new("esi-import")
                            .small()
                            .primary()
                            .icon(IconName::Plus)
                            .label(match selected.len() {
                                0 | 1 => "Import to efa".to_owned(),
                                n => format!("Import {n} to efa"),
                            })
                            .disabled(selected.is_empty())
                            .on_click(cx.listener(|w, _, _, cx| w.import(cx))),
                    )
                    .child(
                        Button::new("esi-delete")
                            .small()
                            .icon(IconName::Delete)
                            .label("Delete from EVE")
                            .disabled(busy || selected.is_empty())
                            .on_click(cx.listener(move |w, _, window, cx| {
                                w.confirm_delete(selected.clone(), window, cx);
                            })),
                    )
                    .child(
                        Button::new("esi-delete-all")
                            .small()
                            .danger()
                            .label("Delete All from EVE")
                            .disabled(busy || all.is_empty())
                            .on_click(cx.listener(move |w, _, window, cx| {
                                w.confirm_delete(all.clone(), window, cx);
                            })),
                    ),
            )
    }
}
