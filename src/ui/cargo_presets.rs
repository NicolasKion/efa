//! The Cargo Presets editor, a window of its own: presets on the left, the selected one in the
//! middle and the market to add from on the right. Also the name prompt behind "Save Cargo as
//! Preset". A gyfa extra.

use std::{collections::HashMap, rc::Rc};

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon as UiIcon, Selectable, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState},
    menu::{DropdownMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{
    AnyElement, AnyWindowHandle, App, AppContext, ClickEvent, Context, Entity, FocusHandle,
    Focusable, FontWeight, Global, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    Render, SharedString, StatefulInteractiveElement, Styled, Subscription, WeakEntity, Window,
    WindowBounds, WindowOptions, div, prelude::FluentBuilder, px, size,
};

use super::{
    Action, AdditionsTab, Gyfa, Loaded,
    additions::amount,
    editor_kit::{self, ItemTree, ListEntry, caption, empty_state, icon_button},
    fitting::{DragPreview, DragSource, Dragged},
    widgets::icon_box,
};
use crate::{
    cargo_preset::{self, CargoPolicy, CargoPreset},
    data::{ItemType, Kind, TypeId},
    icons::Icon,
};

/// The editor window's starting size.
const EDITOR_W: f32 = 1100.;
const EDITOR_H: f32 = 700.;
const LIST_W: f32 = 230.;
/// The preset list's width in a narrow window.
const LIST_NARROW_W: f32 = 180.;
const TREE_W: f32 = 340.;
/// Below this window width the market folds away behind a button.
const NARROW_W: f32 = 900.;
const QUANTITY_W: f32 = 84.;
const VOLUME_W: f32 = 110.;

const NAME_MISSING: &str = "Enter a name for the preset.";
const NAME_TAKEN: &str = "A preset with this name already exists.";

/// The open editor, which is brought forward instead of opening a second one.
struct OpenEditor(Option<(AnyWindowHandle, WeakEntity<CargoPresetEditor>)>);

impl Global for OpenEditor {}

/// What the item tree offers: anything a hold can carry.
fn carried(t: &ItemType) -> bool {
    !matches!(t.kind, Kind::Ship | Kind::Skill | Kind::Mode | Kind::Other)
}

/// What is wrong with a new name, given the names in use.
fn name_problem<'a>(name: &str, mut taken: impl Iterator<Item = &'a str>) -> Option<&'static str> {
    if name.is_empty() {
        Some(NAME_MISSING)
    } else if taken.any(|t| t == name) {
        Some(NAME_TAKEN)
    } else {
        None
    }
}

/// "3 items" for a list's detail.
fn item_count(n: usize) -> String {
    if n == 1 {
        "1 item".to_owned()
    } else {
        format!("{n} items")
    }
}

impl Gyfa {
    /// Change the saved presets, keeping them sorted by name, and write them out.
    fn edit_cargo_presets(
        &mut self,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut Vec<CargoPreset>),
    ) {
        edit(&mut self.saved.cargo_presets);
        self.saved
            .cargo_presets
            .sort_by_key(|p| p.name.to_lowercase());
        self.save();
        cx.notify();
    }

    /// Put a saved preset into the current fit's hold, as one undo step, and show the Cargo tab.
    pub(super) fn apply_cargo_preset(&mut self, name: &str, cx: &mut Context<Self>) {
        let Some(preset) = self
            .saved
            .cargo_presets
            .iter()
            .find(|p| p.name == name)
            .cloned()
        else {
            return;
        };
        if self.fit().is_none() {
            self.status = Some("Open a fit first".into());
            cx.notify();
            return;
        }
        if self.additions_tab != AdditionsTab::Cargo {
            self.additions_tab = AdditionsTab::Cargo;
            self.additions_selection.clear();
            self.additions_anchor = None;
        }
        self.apply(Action::ApplyCargoPreset(preset), cx);
    }

    /// Open the preset editor, showing the named preset if there is one.
    pub(super) fn open_cargo_preset_editor(
        &mut self,
        select: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let Some(loaded) = self.loaded() else { return };
        let gyfa = cx.entity();
        let select = select.or_else(|| self.saved.cargo_presets.first().map(|p| p.name.clone()));
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(EDITOR_W), px(EDITOR_H)), cx)),
            app_id: Some("gyfa".into()),
            ..Default::default()
        };
        // Opened once this update is over: the new window draws right away, and drawing reads
        // this view, which gpui does not allow while it is being updated.
        cx.defer(move |cx| {
            let existing = cx
                .try_global::<OpenEditor>()
                .and_then(|open| open.0.clone());
            if let Some((handle, editor)) = existing {
                let shown = handle.update(cx, |_, window, cx| {
                    if let Some(name) = select.clone() {
                        let _ = editor.update(cx, |e, cx| e.select(Some(name), window, cx));
                    }
                    window.activate_window();
                });
                if shown.is_ok() {
                    return;
                }
            }
            let weak = gyfa.downgrade();
            let opened = gpui_kit::open_window(options, cx, move |window, cx| {
                window.set_window_title("gyfa: Cargo Presets");
                cx.new(|cx| CargoPresetEditor::new(&gyfa, loaded, select, window, cx))
            });
            match opened {
                Ok((handle, editor)) => {
                    cx.set_global(OpenEditor(Some((handle, editor.downgrade()))));
                }
                Err(e) => Gyfa::app_update(&weak, cx, |g, cx| {
                    g.status = Some(format!("Could not open the cargo preset editor: {e}").into());
                    cx.notify();
                }),
            }
        });
    }

    /// Ask for a name and save the current fit's cargo under it.
    pub(super) fn save_cargo_preset_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(fit) = self.fit() else { return };
        let mut items: Vec<(TypeId, u32)> = Vec::new();
        for &(t, n) in &fit.cargo {
            match items.iter_mut().find(|(i, _)| *i == t) {
                Some((_, count)) => *count = count.saturating_add(n),
                None => items.push((t, n)),
            }
        }
        let this = cx.entity().downgrade();
        let check = {
            let this = this.clone();
            move |name: &str, cx: &App| {
                let saved = this.upgrade()?;
                let presets = &saved.read(cx).saved.cargo_presets;
                name_problem(name, presets.iter().map(|p| p.name.as_str()))
            }
        };
        editor_kit::prompt_name(
            window,
            cx,
            "Save Cargo as Preset".to_owned(),
            "Enter a name for the new cargo preset:".to_owned(),
            String::new(),
            check,
            move |name, _, cx| {
                let preset = CargoPreset {
                    name: name.clone(),
                    items: items.clone(),
                    policy: CargoPolicy::default(),
                };
                Gyfa::app_update(&this, cx, |g, cx| {
                    g.edit_cargo_presets(cx, |presets| presets.push(preset));
                    g.status = Some(format!("Saved the cargo as preset {name}").into());
                });
            },
        );
    }
}

/// The editor's own state; the presets themselves live in [`Gyfa`]'s saved data.
pub struct CargoPresetEditor {
    gyfa: WeakEntity<Gyfa>,
    loaded: Rc<Loaded>,
    /// The preset being edited, by name; names are unique.
    selected: Option<String>,
    /// The item row last clicked in the selected preset, which Delete removes.
    picked: Option<TypeId>,
    list_search: Entity<InputState>,
    tree: ItemTree,
    /// Quantity fields of the selected preset's items.
    quantities: HashMap<TypeId, (Entity<InputState>, Subscription)>,
    focus: FocusHandle,
    notice: Option<SharedString>,
    /// The market column is open in a narrow window, where it is folded away by default.
    market_open: bool,
    _subscriptions: Vec<Subscription>,
}

impl CargoPresetEditor {
    fn new(
        gyfa: &Entity<Gyfa>,
        loaded: Rc<Loaded>,
        selected: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let list_search = cx.new(|cx| InputState::new(window, cx).placeholder("Search presets"));
        let tree = ItemTree::new(&loaded.catalog, carried, "Search items", window, cx);
        let redraw =
            |_: &mut Self, _: Entity<InputState>, event: &InputEvent, cx: &mut Context<Self>| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            };
        let subscriptions = vec![
            cx.observe(gyfa, |_, _, cx| cx.notify()),
            cx.subscribe(&list_search, redraw),
            cx.subscribe(&tree.search, redraw),
        ];
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        Self {
            gyfa: gyfa.downgrade(),
            loaded,
            selected,
            picked: None,
            list_search,
            tree,
            quantities: HashMap::new(),
            focus,
            notice: None,
            market_open: false,
            _subscriptions: subscriptions,
        }
    }

    fn presets<'a>(&self, cx: &'a App) -> &'a [CargoPreset] {
        self.gyfa
            .upgrade()
            .map_or(&[], |g| g.read(cx).saved.cargo_presets.as_slice())
    }

    fn current(&self, cx: &App) -> Option<CargoPreset> {
        let name = self.selected.as_ref()?;
        self.presets(cx).iter().find(|p| &p.name == name).cloned()
    }

    fn edit(&self, cx: &mut Context<Self>, edit: impl FnOnce(&mut Vec<CargoPreset>)) {
        Gyfa::app_update(&self.gyfa, cx, |g, cx| g.edit_cargo_presets(cx, edit));
    }

    /// Change the selected preset.
    fn edit_current(&self, cx: &mut Context<Self>, edit: impl FnOnce(&mut CargoPreset)) {
        let Some(name) = self.selected.clone() else {
            return;
        };
        self.edit(cx, |presets| {
            if let Some(p) = presets.iter_mut().find(|p| p.name == name) {
                edit(p);
            }
        });
    }

    fn select(&mut self, name: Option<String>, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected != name {
            self.quantities.clear();
        }
        self.picked = None;
        self.selected = name;
        self.notice = None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    fn taken_problem(&self, name: &str, cx: &App) -> Option<&'static str> {
        name_problem(name, self.presets(cx).iter().map(|p| p.name.as_str()))
    }

    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.ask_name(Naming::New, window, cx);
    }

    /// Ask for a name for a new, copied or renamed preset.
    fn ask_name(&mut self, naming: Naming, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.selected.clone();
        if naming != Naming::New && current.is_none() {
            return;
        }
        let current = current.unwrap_or_default();
        let (title, message, default) = match naming {
            Naming::New => (
                "New Cargo Preset",
                "Enter a name for the new preset:",
                String::new(),
            ),
            Naming::Duplicate => (
                "Duplicate Cargo Preset",
                "Enter a name for the copy:",
                format!("{current} copy"),
            ),
            Naming::Rename => ("Rename Cargo Preset", "Enter a new name:", current.clone()),
        };
        let editor = cx.entity().downgrade();
        let check = {
            let (editor, current) = (editor.clone(), current.clone());
            move |name: &str, cx: &App| {
                if naming == Naming::Rename && name == current {
                    return None;
                }
                editor.upgrade()?.read(cx).taken_problem(name, cx)
            }
        };
        editor_kit::prompt_name(
            window,
            cx,
            title.to_owned(),
            message.to_owned(),
            default,
            check,
            move |name, window, cx| {
                let _ = editor.update(cx, |e, cx| e.named(naming, name, window, cx));
            },
        );
    }

    fn named(&mut self, naming: Naming, name: String, window: &mut Window, cx: &mut Context<Self>) {
        let current = self.selected.clone().unwrap_or_default();
        let new = name.clone();
        self.edit(cx, |presets| match naming {
            Naming::New => presets.push(CargoPreset::new(new)),
            Naming::Duplicate => {
                if let Some(p) = presets.iter().find(|p| p.name == current) {
                    let copy = CargoPreset {
                        name: new,
                        ..p.clone()
                    };
                    presets.push(copy);
                }
            }
            Naming::Rename => {
                if let Some(p) = presets.iter_mut().find(|p| p.name == current) {
                    p.name = new;
                }
            }
        });
        if naming == Naming::Rename {
            // Keep the quantity fields: the items did not change.
            self.selected = Some(name);
            cx.notify();
        } else {
            self.select(Some(name), window, cx);
        }
    }

    fn confirm_delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(name) = self.selected.clone() else {
            return;
        };
        let editor = cx.entity().downgrade();
        editor_kit::confirm_delete(
            window,
            cx,
            format!("Delete the cargo preset {name}?"),
            move |window, cx| {
                let _ = editor.update(cx, |e, cx| e.delete(window, cx));
            },
        );
    }

    fn delete(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(old) = self.selected.clone() else {
            return;
        };
        let at = self.presets(cx).iter().position(|p| p.name == old);
        self.edit(cx, |presets| presets.retain(|p| p.name != old));
        let presets = self.presets(cx);
        let next = at
            .and_then(|at| presets.get(at).or_else(|| presets.last()))
            .map(|p| p.name.clone());
        self.select(next, window, cx);
    }

    fn add_item(&mut self, type_id: TypeId, count: u32, cx: &mut Context<Self>) {
        if self.selected.is_none() {
            self.notice = Some("Pick or create a preset first.".into());
            cx.notify();
            return;
        }
        self.edit_current(cx, |p| p.add(type_id, count));
    }

    fn remove_item(&mut self, type_id: TypeId, cx: &mut Context<Self>) {
        self.quantities.remove(&type_id);
        if self.picked == Some(type_id) {
            self.picked = None;
        }
        self.edit_current(cx, |p| p.remove(type_id));
    }

    /// Take the typed quantity, or put the stored one back if it does not read as a number.
    fn commit_quantity(&mut self, type_id: TypeId, window: &mut Window, cx: &mut Context<Self>) {
        let Some((input, _)) = self.quantities.get(&type_id) else {
            return;
        };
        let input = input.clone();
        let typed = input.read(cx).value().trim().parse::<u32>();
        let stored = self.current(cx).and_then(|p| {
            p.items
                .iter()
                .find(|&&(t, _)| t == type_id)
                .map(|&(_, n)| n)
        });
        match (typed, stored) {
            (Ok(n), Some(old)) if n != old => {
                if n == 0 {
                    self.quantities.remove(&type_id);
                }
                self.edit_current(cx, |p| p.set(type_id, n));
            }
            (_, Some(old)) => {
                input.update(cx, |i, cx| i.set_value(old.to_string(), window, cx));
            }
            (_, None) => {}
        }
    }

    /// Add every stack of the current fit's cargo to the preset.
    fn add_fit_cargo(&mut self, cx: &mut Context<Self>) {
        let cargo = self
            .gyfa
            .upgrade()
            .and_then(|g| g.read(cx).fit().map(|f| f.cargo.clone()));
        let Some(cargo) = cargo else {
            self.notice = Some("Open a fit to take its cargo.".into());
            cx.notify();
            return;
        };
        if cargo.is_empty() {
            self.notice = Some("The current fit's cargo is empty.".into());
            cx.notify();
            return;
        }
        let units: u32 = cargo.iter().fold(0, |sum, &(_, n)| sum.saturating_add(n));
        self.edit_current(cx, |p| {
            for &(t, n) in &cargo {
                p.add(t, n);
            }
        });
        self.notice = Some(format!("Added {units} units from the fit's cargo.").into());
        cx.notify();
    }

    fn apply_to_fit(&mut self, cx: &mut Context<Self>) {
        let Some(name) = self.selected.clone() else {
            return;
        };
        Gyfa::app_update(&self.gyfa, cx, |g, cx| g.apply_cargo_preset(&name, cx));
        self.notice = Some("Applied to the current fit; see its status line.".into());
        cx.notify();
    }

    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let m = keystroke.modifiers;
        let plain = !(m.control || m.alt || m.shift || m.platform);
        let ctrl = m.control && !m.alt && !m.shift;
        match keystroke.key.as_str() {
            // Only when no text field has the keys, so Delete still edits text.
            "delete" if plain && self.focus.is_focused(window) => match self.picked {
                Some(t) => self.remove_item(t, cx),
                None => self.confirm_delete(window, cx),
            },
            "d" if ctrl => self.ask_name(Naming::Duplicate, window, cx),
            "f2" if plain => self.ask_name(Naming::Rename, window, cx),
            "f" if ctrl => self
                .tree
                .search
                .update(cx, |input, cx| input.focus(window, cx)),
            _ => return,
        }
        cx.stop_propagation();
    }

    fn list_column(
        &self,
        presets: &[CargoPreset],
        narrow: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let query = self.list_search.read(cx).value().to_string();
        let entries: Vec<ListEntry> = presets
            .iter()
            .map(|p| ListEntry {
                name: p.name.clone(),
                detail: item_count(p.items.len()),
            })
            .collect();
        let rows = editor_kit::name_list(
            entries,
            &query,
            self.selected.as_deref(),
            cx,
            |e: &mut Self, name, window, cx| e.select(Some(name), window, cx),
            None,
        );
        let has = self.selected.is_some();
        let empty = if presets.is_empty() {
            Some(empty_state(
                AssetIcon::Package,
                "No presets yet",
                "Click New, or save a fit's cargo from the Cargo tab's menu.",
                cx,
            ))
        } else if rows.is_empty() {
            Some(empty_state(
                AssetIcon::SearchX,
                "No presets match",
                "Clear the search to see them all.",
                cx,
            ))
        } else {
            None
        };
        let theme = cx.theme();
        v_flex()
            .w(px(if narrow { LIST_NARROW_W } else { LIST_W }))
            .flex_none()
            .h_full()
            .gap_2()
            .p_3()
            .border_r_1()
            .border_color(theme.border)
            .bg(theme.sidebar)
            .child(caption("Cargo presets", cx))
            .child(Input::new(&self.list_search).small().cleanable(true))
            .child(
                v_flex()
                    .id("preset-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .gap_0p5()
                    .children(empty)
                    .children(rows),
            )
            .child(
                h_flex()
                    .gap_1()
                    .child(
                        icon_button("preset-new", AssetIcon::Plus, "New preset")
                            .on_click(cx.listener(|e, _, window, cx| e.create(window, cx))),
                    )
                    .child(
                        icon_button("preset-duplicate", AssetIcon::Copy, "Duplicate (Ctrl+D)")
                            .disabled(!has)
                            .on_click(cx.listener(|e, _, window, cx| {
                                e.ask_name(Naming::Duplicate, window, cx);
                            })),
                    )
                    .child(
                        icon_button("preset-rename", AssetIcon::Pencil, "Rename (F2)")
                            .disabled(!has)
                            .on_click(cx.listener(|e, _, window, cx| {
                                e.ask_name(Naming::Rename, window, cx);
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        icon_button("preset-delete", AssetIcon::Trash, "Delete (Delete)")
                            .disabled(!has)
                            .on_click(cx.listener(|e, _, window, cx| e.confirm_delete(window, cx))),
                    ),
            )
            .into_any_element()
    }

    /// The quantity field of an item, made on first use and kept in step with the preset while
    /// it is not being typed in.
    fn quantity_input(
        &mut self,
        type_id: TypeId,
        count: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<InputState> {
        if let Some((input, _)) = self.quantities.get(&type_id) {
            let input = input.clone();
            let typing = input.read(cx).focus_handle(cx).is_focused(window);
            if !typing && input.read(cx).value() != count.to_string().as_str() {
                input.update(cx, |i, cx| i.set_value(count.to_string(), window, cx));
            }
            return input;
        }
        let input = cx.new(|cx| InputState::new(window, cx).default_value(count.to_string()));
        let subscription = cx.subscribe_in(
            &input,
            window,
            move |e: &mut Self, _, event: &InputEvent, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                    e.commit_quantity(type_id, window, cx);
                }
            },
        );
        self.quantities
            .insert(type_id, (input.clone(), subscription));
        input
    }

    fn item_row(
        &mut self,
        n: usize,
        type_id: TypeId,
        count: u32,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input = self.quantity_input(type_id, count, window, cx);
        let loaded = Rc::clone(&self.loaded);
        let catalog = &loaded.catalog;
        let unit = catalog.get(type_id).map_or(0.0, |t| t.volume);
        let picked = self.picked == Some(type_id);
        let theme = cx.theme();
        let row = h_flex()
            .id(SharedString::from(format!("preset-item-{type_id}")))
            .gap_3()
            .h(px(editor_kit::ROW_HEIGHT + 2.))
            .flex_none()
            .px_2()
            .rounded(px(4.))
            .when(n % 2 == 1 && !picked, |r| r.bg(theme.table_even))
            .when(picked, |r| r.bg(theme.table_active))
            .hover(|s| s.bg(theme.table_hover))
            // Dropped on its own row, a market item adds to the count like anywhere else.
            .drag_over::<Dragged>(|style, dragged, _, cx| {
                if matches!(dragged.source, DragSource::Market) {
                    style.bg(cx.theme().drop_target)
                } else {
                    style
                }
            })
            .child(icon_box(catalog, &loaded.icons, Icon::Type(type_id), 24.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(catalog.name(type_id)),
            )
            .child(
                div()
                    .w(px(QUANTITY_W))
                    .flex_none()
                    .child(Input::new(&input).small()),
            )
            .child(
                div()
                    .w(px(VOLUME_W))
                    .flex_none()
                    .text_right()
                    .text_color(theme.muted_foreground)
                    .child(format!("{} m³", amount(unit * f64::from(count)))),
            )
            .child(
                Button::new(SharedString::from(format!("preset-remove-{type_id}")))
                    .ghost()
                    .xsmall()
                    .icon(UiIcon::new(AssetIcon::X))
                    .tooltip("Remove from preset")
                    .on_click(cx.listener(move |e, _, _, cx| e.remove_item(type_id, cx))),
            )
            .on_click(cx.listener(move |e, _: &ClickEvent, window, cx| {
                e.picked = Some(type_id);
                window.focus(&e.focus, cx);
                cx.notify();
            }));
        match Dragged::of(&loaded, DragSource::Listed, Some(type_id)) {
            Some(dragged) => row
                .on_drag(dragged, |dragged, _, _, cx| {
                    cx.new(|_| DragPreview(dragged.clone()))
                })
                .into_any_element(),
            None => row.into_any_element(),
        }
    }

    fn preset_column(
        &mut self,
        preset: Option<&CargoPreset>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(preset) = preset else {
            return v_flex()
                .flex_1()
                .min_w_0()
                .justify_center()
                .child(empty_state(
                    AssetIcon::Package,
                    "No preset selected",
                    "Pick a preset on the left, or click New to make one.",
                    cx,
                ))
                .into_any_element();
        };
        let catalog = &self.loaded.catalog;
        let mut items = preset.items.clone();
        items.sort_by_cached_key(|&(t, _)| catalog.name(t));
        let volume = cargo_preset::volume(catalog, &preset.items);
        let units: u32 = items.iter().fold(0, |sum, &(_, n)| sum.saturating_add(n));
        let rows: Vec<AnyElement> = items
            .iter()
            .enumerate()
            .map(|(n, &(t, count))| self.item_row(n, t, count, window, cx))
            .collect();
        let has_fit = self
            .gyfa
            .upgrade()
            .is_some_and(|g| g.read(cx).fit().is_some());
        let narrow = window.viewport_size().width < px(NARROW_W);
        let theme = cx.theme();
        let policy = preset.policy;
        // Everything but applying sits behind the gear: the column is for the items.
        let settings = Button::new("preset-settings")
            .ghost()
            .small()
            .icon(UiIcon::new(AssetIcon::Settings2))
            .tooltip("When applied, and adding the fit's cargo")
            .dropdown_menu({
                let this = cx.entity().downgrade();
                move |menu, _, _| {
                    let mut menu = menu.label("When applied");
                    for value in CargoPolicy::ALL {
                        let this = this.clone();
                        menu = menu.item(
                            PopupMenuItem::new(format!(
                                "{}: {}",
                                value.label(),
                                value.explanation()
                            ))
                            .checked(value == policy)
                            .on_click(move |_, _, cx| {
                                let _ = this.update(cx, |e, cx| {
                                    e.edit_current(cx, |p| p.policy = value);
                                });
                            }),
                        );
                    }
                    let this = this.clone();
                    menu.separator().item(
                        PopupMenuItem::new("Add from fit's cargo")
                            .disabled(!has_fit)
                            .on_click(move |_, _, cx| {
                                let _ = this.update(cx, CargoPresetEditor::add_fit_cargo);
                            }),
                    )
                }
            });
        let market = narrow.then(|| {
            Button::new("preset-market")
                .small()
                .ghost()
                .icon(UiIcon::new(AssetIcon::Store))
                .when(self.market_open, |b| b.selected(true))
                .tooltip("Show or hide the market")
                .on_click(cx.listener(|e, _, _, cx| {
                    e.market_open = !e.market_open;
                    cx.notify();
                }))
        });
        let header = h_flex()
            .gap_2()
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .truncate()
                            .child(preset.name.clone()),
                    )
                    .child(
                        h_flex()
                            .gap_1p5()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(
                                div()
                                    .id("preset-policy")
                                    .px_1p5()
                                    .rounded(px(4.))
                                    .bg(theme.secondary)
                                    .text_color(theme.foreground)
                                    .child(policy.label())
                                    .tooltip(move |window, cx| {
                                        gpui_kit::component::tooltip::Tooltip::new(
                                            policy.explanation(),
                                        )
                                        .build(window, cx)
                                    }),
                            )
                            .child(div().truncate().child(format!(
                                "{}, {units} units, {} m³",
                                item_count(items.len()),
                                amount(volume)
                            ))),
                    ),
            )
            .children(market)
            .child(settings)
            .child(
                Button::new("preset-apply")
                    .small()
                    .primary()
                    .icon(UiIcon::new(AssetIcon::PackageCheck))
                    .label("Apply")
                    .disabled(!has_fit || preset.items.is_empty())
                    .tooltip("Put this preset into the current fit's cargo")
                    .on_click(cx.listener(|e, _, _, cx| e.apply_to_fit(cx))),
            );
        let list = if rows.is_empty() {
            vec![
                empty_state(
                    AssetIcon::PackagePlus,
                    "This preset is empty",
                    "Drag items here from the market on the right, double-click them there, \
                     or add the current fit's cargo.",
                    cx,
                )
                .into_any_element(),
            ]
        } else {
            rows
        };
        let notice = self.notice.clone();
        v_flex()
            .id("preset-column")
            .flex_1()
            .min_w_0()
            .h_full()
            .gap_2()
            .p_3()
            .drag_over::<Dragged>(|style, dragged, _, cx| {
                if matches!(dragged.source, DragSource::Market) {
                    style.bg(cx.theme().drop_target)
                } else {
                    style
                }
            })
            .on_drop(cx.listener(|e, dragged: &Dragged, _, cx| {
                if matches!(dragged.source, DragSource::Market) {
                    for t in dragged.types() {
                        e.add_item(t, 1, cx);
                    }
                }
            }))
            .child(header)
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_3()
                            .px_2()
                            .child(div().flex_1().child(caption("Items", cx)))
                            .child(div().w(px(QUANTITY_W)).child(caption("Quantity", cx)))
                            .child(
                                div()
                                    .w(px(VOLUME_W))
                                    .flex()
                                    .justify_end()
                                    .child(caption("Volume", cx)),
                            )
                            .child(div().w(px(22.))),
                    )
                    .child(
                        v_flex()
                            .id("preset-items")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .children(list),
                    ),
            )
            .when_some(notice, |col, notice| {
                col.child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(notice),
                )
            })
            .into_any_element()
    }
}

/// Which name prompt is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Naming {
    New,
    Duplicate,
    Rename,
}

impl Render for CargoPresetEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let presets = self.presets(cx).to_vec();
        // Another window may have removed the preset this one was showing.
        if self
            .selected
            .as_ref()
            .is_some_and(|name| !presets.iter().any(|p| &p.name == name))
        {
            self.selected = None;
            self.quantities.clear();
        }
        let preset = self
            .selected
            .as_ref()
            .and_then(|name| presets.iter().find(|p| &p.name == name));
        if let Some(preset) = preset {
            self.quantities
                .retain(|t, _| preset.items.iter().any(|&(i, _)| i == *t));
        }
        let loaded = Rc::clone(&self.loaded);
        let narrow = window.viewport_size().width < px(NARROW_W);
        let list = self.list_column(&presets, narrow, cx);
        let middle = self.preset_column(preset, window, cx);
        let tree = self.tree.render(
            &loaded,
            cx,
            |e: &mut Self| &mut e.tree,
            |e: &mut Self, t, _, cx| e.add_item(t, 1, cx),
        );
        let show_market = !narrow || self.market_open;
        let theme = cx.theme();
        h_flex()
            .size_full()
            .track_focus(&self.focus)
            .key_context("CargoPresets")
            .on_key_down(
                cx.listener(|e, event: &KeyDownEvent, window, cx| e.key(event, window, cx)),
            )
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_sm()
            .child(list)
            .child(middle)
            .when(show_market, |row| row.child(
                v_flex()
                    .w(px(TREE_W))
                    .flex_none()
                    .h_full()
                    .gap_2()
                    .p_3()
                    .border_l_1()
                    .border_color(theme.border)
                    .bg(theme.sidebar)
                    // A preset's item dragged back here comes off the preset.
                    .id("preset-market")
                    .drag_over::<Dragged>(|style, dragged, _, cx| {
                        if matches!(dragged.source, DragSource::Listed) {
                            style.bg(cx.theme().drop_target)
                        } else {
                            style
                        }
                    })
                    .on_drop(cx.listener(|e, dragged: &Dragged, _, cx| {
                        if matches!(dragged.source, DragSource::Listed) {
                            e.remove_item(dragged.type_id, cx);
                        }
                    }))
                    .child(caption("Market", cx))
                    .child(div().flex_1().min_h_0().child(tree))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("Drag an item onto the preset, or double-click it. Drag one back here to remove it."),
                    ),
            ))
    }
}

/// The Cargo tab's preset entries: apply one, or save the cargo as a new one.
pub(super) fn menu_entries(
    menu: gpui_kit::component::menu::PopupMenu,
    this: &WeakEntity<Gyfa>,
    window: &mut Window,
    cx: &mut Context<gpui_kit::component::menu::PopupMenu>,
) -> gpui_kit::component::menu::PopupMenu {
    use super::widgets::{entry, icon_label};
    use gpui_kit::component::IconName;
    let Some(gyfa) = this.upgrade() else {
        return menu;
    };
    let g = gyfa.read(cx);
    let names: Vec<String> = g
        .saved
        .cargo_presets
        .iter()
        .map(|p| p.name.clone())
        .collect();
    let has_cargo = g.fit().is_some_and(|f| !f.cargo.is_empty());
    let mut menu = menu;
    if !names.is_empty() {
        let this = this.clone();
        menu = menu.submenu_with_element(
            "Apply Preset",
            icon_label(IconName::Inbox, "Apply Preset"),
            window,
            cx,
            move |menu, _, _| {
                names.iter().fold(menu, |menu, name| {
                    let (this, name) = (this.clone(), name.clone());
                    menu.item(
                        entry(IconName::Inbox, name.clone()).on_click(move |_, _, cx| {
                            Gyfa::app_update(&this, cx, |g, cx| g.apply_cargo_preset(&name, cx));
                        }),
                    )
                })
            },
        );
    }
    if has_cargo {
        let this = this.clone();
        menu = menu.item(entry(IconName::Plus, "Save Cargo as Preset…").on_click(
            move |_, window, cx| {
                Gyfa::app_update(&this, cx, |g, cx| g.save_cargo_preset_dialog(window, cx));
            },
        ));
    }
    let this = this.clone();
    menu.item(
        entry(IconName::Settings2, "Edit Cargo Presets…").on_click(move |_, _, cx| {
            Gyfa::app_update(&this, cx, |g, cx| g.open_cargo_preset_editor(None, cx));
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_must_be_new_and_not_blank() {
        let taken = ["Filaments", "Scripts"];
        assert_eq!(name_problem("", taken.into_iter()), Some(NAME_MISSING));
        assert_eq!(name_problem("Scripts", taken.into_iter()), Some(NAME_TAKEN));
        assert_eq!(name_problem("Fuel", taken.into_iter()), None);
    }
}
