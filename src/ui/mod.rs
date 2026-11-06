//! The main window, laid out like pyfa: browsers on the left, the fit in the middle with its
//! additions underneath, stats on the right.

mod actions;
mod additions;
mod columns;
mod fitting;
mod implant_sets;
mod side;
mod stats;
mod widgets;

use std::{
    collections::HashSet,
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

use gpui_kit::component::{
    ActiveTheme, IconName, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    input::{Input, InputEvent, InputState, Textarea, TextareaState},
    menu::DropdownMenu,
    resizable::{h_resizable, resizable_panel, v_resizable},
    v_flex,
};
use gpui_kit::{
    App, AppContext, ClickEvent, ClipboardItem, Context, Entity, FocusHandle, InteractiveElement,
    IntoElement, ParentElement, Render, ScrollHandle, SharedString, StatefulInteractiveElement,
    Styled, Subscription, Task, WeakEntity, Window, div, prelude::FluentBuilder, px,
};

pub use actions::{Action, AddKind, RowRef, Scope};

use crate::{
    data::{Catalog, Kind, TypeId},
    engine::{Engine, Stats},
    fit::Fit,
    icons::{Icon, Icons},
    store::Saved,
    theme::{self, Palette},
};

/// How often background work is checked: icons that finished downloading, theme switches.
const POLL: Duration = Duration::from_millis(250);

pub struct Loaded {
    pub catalog: Catalog,
    pub engine: Engine,
    pub icons: Icons,
}

enum Loading {
    Pending,
    Ready(Rc<Loaded>),
    Failed(SharedString),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserTab {
    Market,
    Fittings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdditionsTab {
    Drones,
    Implants,
    Boosters,
    Cargo,
    Notes,
}

pub struct Gyfa {
    loading: Loading,
    saved: Saved,
    stats: Option<Stats>,
    browser_tab: BrowserTab,
    additions_tab: AdditionsTab,
    search: Entity<InputState>,
    search_text: String,
    name_input: Entity<InputState>,
    notes_input: Entity<TextareaState>,
    /// The fit whose name and notes the inputs currently show.
    inputs_show: Option<usize>,
    expanded_groups: HashSet<i32>,
    /// Item to scroll the market to and highlight, after a market group jump.
    market_highlight: Option<TypeId>,
    market_scroll_pending: bool,
    market_scroll: ScrollHandle,
    /// Ship whose fits the fittings browser is narrowed to.
    fittings_ship: Option<TypeId>,
    selection: Vec<RowRef>,
    /// Where a Shift-click selection range starts.
    anchor: Option<RowRef>,
    /// Changes not written yet; the poll saves them, so dragging does not write every frame.
    unsaved: bool,
    /// The fitting view's column widths for the frame being drawn.
    column_layout: columns::Layout,
    /// Selected rows of the open additions tab.
    additions_selection: Vec<usize>,
    additions_anchor: Option<usize>,
    /// Keyboard focus of the additions list, for Delete, Ctrl+A and Escape.
    additions_focus: FocusHandle,
    /// Takes the drones pane's keys: Delete, Escape, Ctrl+A.
    drones_focus: FocusHandle,
    collapsed_sections: HashSet<&'static str>,
    status: Option<SharedString>,
    palette: Palette,
    theme_generation: Option<String>,
    _subscriptions: Vec<Subscription>,
    _tasks: Vec<Task<()>>,
}

impl Gyfa {
    pub fn new(
        data_dir: PathBuf,
        import: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let palette = Palette::current().unwrap_or_default();
        widgets::apply_palette(&palette, cx);

        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search items"));
        let name_input = cx.new(|cx| InputState::new(window, cx).placeholder("Fit name"));
        let notes_input = cx.new(|cx| TextareaState::new(window, cx).rows(6).placeholder("Notes"));
        let subscriptions = vec![
            cx.subscribe(&search, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.search_text = input.read(cx).value().to_string();
                    cx.notify();
                }
            }),
            cx.subscribe(&name_input, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change)
                    && let Some(i) = this.saved.current
                {
                    this.saved.fits[i].name = input.read(cx).value().to_string();
                    this.save();
                    cx.notify();
                }
            }),
            cx.subscribe(&notes_input, |this, input, event: &InputEvent, cx| {
                if matches!(event, InputEvent::Change) {
                    this.edit_notes(input.read(cx).value().to_string(), cx);
                }
            }),
        ];

        let loader = cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = cx.background_spawn(async move { load(&data_dir) }).await;
            let _ = this.update(cx, |this, cx| {
                this.loading = match result {
                    Ok(loaded) => Loading::Ready(Rc::new(loaded)),
                    Err(e) => Loading::Failed(e.into()),
                };
                if let Some(text) = import {
                    this.import(&text);
                }
                this.recompute();
                cx.notify();
            });
        });
        let poller = cx.spawn(async move |this: WeakEntity<Self>, cx| {
            loop {
                cx.background_executor().timer(POLL).await;
                if this.update(cx, Gyfa::poll).is_err() {
                    break;
                }
            }
        });

        Self {
            loading: Loading::Pending,
            saved: Saved::load(),
            stats: None,
            browser_tab: BrowserTab::Market,
            additions_tab: AdditionsTab::Drones,
            search,
            search_text: String::new(),
            name_input,
            notes_input,
            inputs_show: None,
            expanded_groups: HashSet::new(),
            market_highlight: None,
            market_scroll_pending: false,
            market_scroll: ScrollHandle::new(),
            fittings_ship: None,
            selection: Vec::new(),
            anchor: None,
            unsaved: false,
            column_layout: columns::Layout::default(),
            additions_selection: Vec::new(),
            additions_anchor: None,
            additions_focus: cx.focus_handle(),
            drones_focus: cx.focus_handle(),
            collapsed_sections: HashSet::new(),
            status: None,
            palette,
            theme_generation: theme::generation(),
            _subscriptions: subscriptions,
            _tasks: vec![loader, poller],
        }
    }

    fn loaded(&self) -> Option<Rc<Loaded>> {
        match &self.loading {
            Loading::Ready(loaded) => Some(Rc::clone(loaded)),
            Loading::Pending | Loading::Failed(_) => None,
        }
    }

    /// Redraw for downloaded icons, restyle for a theme switch.
    fn poll(&mut self, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.unsaved) {
            self.save();
        }
        if self.loaded().is_some_and(|l| l.icons.take_updated()) {
            cx.notify();
        }
        let generation = theme::generation();
        if generation != self.theme_generation {
            self.theme_generation = generation;
            if let Some(palette) = Palette::current() {
                self.palette = palette;
                widgets::apply_palette(&palette, cx);
            }
        }
    }

    fn fit(&self) -> Option<&Fit> {
        self.saved.current.map(|i| &self.saved.fits[i])
    }

    fn recompute(&mut self) {
        let Some(loaded) = self.loaded() else { return };
        self.stats = self.fit().map(|fit| loaded.engine.compute(fit));
    }

    fn save(&self) {
        if let Err(e) = self.saved.save() {
            eprintln!("saving fits: {e}");
        }
    }

    /// Recalculate, save and redraw after the fits changed.
    fn changed(&mut self, cx: &mut Context<Self>) {
        self.recompute();
        self.save();
        cx.notify();
    }

    fn open_fit(&mut self, index: usize, cx: &mut Context<Self>) {
        self.clear_selection();
        if !self.saved.open.contains(&index) {
            self.saved.open.push(index);
        }
        self.saved.current = Some(index);
        self.changed(cx);
    }

    fn add_fit(&mut self, fit: Fit, cx: &mut Context<Self>) {
        self.saved.fits.push(fit);
        self.open_fit(self.saved.fits.len() - 1, cx);
    }

    fn close_fit(&mut self, index: usize, cx: &mut Context<Self>) {
        self.clear_selection();
        self.saved.open.retain(|&i| i != index);
        if self.saved.current == Some(index) {
            self.saved.current = self.saved.open.last().copied();
        }
        self.changed(cx);
    }

    fn delete_fit(&mut self, index: usize, cx: &mut Context<Self>) {
        self.clear_selection();
        self.saved.open.retain(|&i| i != index);
        if self.saved.current == Some(index) {
            self.saved.current = self.saved.open.last().copied();
        }
        self.saved.fits.remove(index);
        let shift = |i: usize| if i > index { i - 1 } else { i };
        self.saved.open = self.saved.open.iter().map(|&i| shift(i)).collect();
        self.saved.current = self.saved.current.map(shift);
        self.inputs_show = None;
        self.changed(cx);
    }

    /// Import EFT text as a new open fit. Problems end up in the status line.
    fn import(&mut self, text: &str) {
        let Some(loaded) = self.loaded() else { return };
        match Fit::from_eft(&loaded.catalog, text) {
            Ok((fit, unknown)) => {
                self.status = (!unknown.is_empty())
                    .then(|| format!("Skipped unknown items: {}", unknown.join(", ")).into());
                self.saved.fits.push(fit);
                let index = self.saved.fits.len() - 1;
                self.saved.open.push(index);
                self.saved.current = Some(index);
            }
            Err(e) => self.status = Some(format!("Import failed: {e}").into()),
        }
    }

    /// Add an item from the market the way double-clicking it in pyfa does.
    fn add_item(&mut self, id: TypeId, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded() else { return };
        let catalog = &loaded.catalog;
        let is_ship = catalog.get(id).is_some_and(|t| t.kind == Kind::Ship);
        // A ship starts a new fit rather than swapping the hull under an existing one.
        if is_ship {
            let mut fit = Fit::new(catalog.name(id));
            fit.ship = Some(id);
            fit.mode = catalog.modes_for(id).first().copied();
            self.status = None;
            self.add_fit(fit, cx);
            return;
        }
        let Some(i) = self.saved.current else {
            self.status = Some("Pick a ship first".into());
            cx.notify();
            return;
        };
        let added = self.saved.fits[i].add(catalog, id);
        // pyfa brings the Boosters tab up even when the booster was already there.
        if catalog.get(id).is_some_and(|t| t.kind == Kind::Booster)
            && self.additions_tab != AdditionsTab::Boosters
        {
            self.additions_tab = AdditionsTab::Boosters;
            self.additions_selection.clear();
            self.additions_anchor = None;
        }
        if catalog.get(id).is_some_and(|t| t.kind == Kind::Implant)
            && self.additions_tab != AdditionsTab::Implants
        {
            self.additions_tab = AdditionsTab::Implants;
            self.clear_selection();
        }
        // pyfa brings the drones pane forward when a drone goes in.
        if added
            && self.additions_tab != AdditionsTab::Drones
            && catalog.get(id).is_some_and(|t| t.kind == Kind::Drone)
        {
            self.additions_tab = AdditionsTab::Drones;
            self.additions_selection.clear();
            self.additions_anchor = None;
        }
        self.status =
            (!added).then(|| format!("Nothing on this fit takes {}", catalog.name(id)).into());
        self.changed(cx);
    }

    fn open_import_dialog(window: &mut Window, cx: &mut Context<Self>) {
        let text = cx.new(|cx| {
            TextareaState::new(window, cx)
                .rows(16)
                .placeholder("[Vexor, My Vexor]\nDrone Damage Amplifier II\n…")
        });
        let this = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let (text, this) = (text.clone(), this.clone());
            dialog.title("Import EFT").w(px(560.)).child(
                v_flex()
                    .gap_3()
                    .child(Textarea::new(&text).h(px(320.)))
                    .child(
                        h_flex().justify_end().child(
                            Button::new("import-eft")
                                .primary()
                                .label("Import")
                                .on_click(move |_, window, cx| {
                                    let value = text.read(cx).value().to_string();
                                    let _ = this.update(cx, |this, cx| {
                                        this.import(&value);
                                        this.changed(cx);
                                    });
                                    window.close_dialog(cx);
                                }),
                        ),
                    ),
            )
        });
    }

    /// Point the name and notes inputs at the current fit when it changes.
    fn sync_inputs(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.inputs_show == self.saved.current {
            return;
        }
        self.inputs_show = self.saved.current;
        let (name, notes) = self
            .fit()
            .map_or_default(|f| (f.name.clone(), f.notes.clone()));
        self.name_input
            .update(cx, |input, cx| input.set_value(name, window, cx));
        self.notes_input
            .update(cx, |input, cx| input.set_value(notes, window, cx));
    }

    fn menu_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let this = cx.entity().downgrade();
        let has_fit = self.saved.current.is_some();
        h_flex()
            .gap_1()
            .px_2()
            .py_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().title_bar)
            .child(
                Button::new("menu-file")
                    .ghost()
                    .small()
                    .label("File")
                    .dropdown_menu({
                        let this = this.clone();
                        move |menu, _, _| {
                            let (import, copy) = (this.clone(), this.clone());
                            menu.item(widgets::entry(IconName::FileText, "Import EFT…").on_click(
                                move |_, window, cx| {
                                    let _ = import.update(cx, |_, cx| {
                                        Self::open_import_dialog(window, cx);
                                    });
                                },
                            ))
                            .item(
                                widgets::entry(IconName::Copy, "Copy as EFT")
                                    .disabled(!has_fit)
                                    .on_click(move |_, _, cx| {
                                        let _ = copy.update(cx, Gyfa::copy_eft);
                                    }),
                            )
                            .separator()
                            .item(
                                widgets::entry(IconName::WindowClose, "Quit")
                                    .on_click(|_, _, cx| cx.quit()),
                            )
                        }
                    }),
            )
            .child(
                Button::new("menu-fit")
                    .ghost()
                    .small()
                    .label("Fit")
                    .dropdown_menu({
                        let this = this.clone();
                        move |menu, _, _| {
                            let (new, duplicate, delete) =
                                (this.clone(), this.clone(), this.clone());
                            menu.item(widgets::entry(IconName::Plus, "New fit").on_click(
                                move |_, _, cx| {
                                    let _ = new.update(cx, |this, cx| {
                                        this.add_fit(Fit::new("New fit"), cx);
                                    });
                                },
                            ))
                            .item(
                                widgets::entry(IconName::Copy, "Duplicate")
                                    .disabled(!has_fit)
                                    .on_click(move |_, _, cx| {
                                        let _ = duplicate.update(cx, Gyfa::duplicate_fit);
                                    }),
                            )
                            .item(
                                widgets::entry(IconName::Delete, "Delete")
                                    .disabled(!has_fit)
                                    .on_click(move |_, _, cx| {
                                        let _ = delete.update(cx, |this, cx| {
                                            if let Some(current) = this.saved.current {
                                                this.delete_fit(current, cx);
                                            }
                                        });
                                    }),
                            )
                        }
                    }),
            )
            .child(
                Button::new("menu-character")
                    .ghost()
                    .small()
                    .label("Character")
                    .dropdown_menu(move |menu, _, _| {
                        let editor = this.clone();
                        menu.item(
                            widgets::entry(IconName::User, "All 5 (the only character for now)")
                                .disabled(true),
                        )
                        .separator()
                        .item(
                            widgets::entry(IconName::Settings2, "Implant Set Editor…").on_click(
                                move |_, window, cx| {
                                    Self::app_update(&editor, cx, |g, cx| {
                                        g.open_implant_set_editor(None, window, cx);
                                    });
                                },
                            ),
                        )
                    }),
            )
            .when_some(self.status.clone(), |bar, status| {
                bar.child(
                    div()
                        .ml_4()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(status),
                )
            })
    }

    fn copy_eft(&mut self, cx: &mut Context<Self>) {
        let (Some(loaded), Some(fit)) = (self.loaded(), self.fit()) else {
            return;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(fit.to_eft(&loaded.catalog)));
        self.status = Some("Copied fit to clipboard".into());
        cx.notify();
    }

    fn duplicate_fit(&mut self, cx: &mut Context<Self>) {
        let Some(mut copy) = self.fit().cloned() else {
            return;
        };
        copy.name = format!("{} copy", copy.name);
        self.add_fit(copy, cx);
    }

    fn fit_tabs(&self, loaded: &Loaded, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        h_flex()
            .flex_wrap()
            .gap_1()
            .p_1()
            .bg(theme.tab_bar)
            .border_b_1()
            .border_color(theme.border)
            .children(self.saved.open.iter().map(|&i| {
                let fit = &self.saved.fits[i];
                let selected = self.saved.current == Some(i);
                let label = match fit.ship {
                    Some(ship) => format!("{}: {}", loaded.catalog.name(ship), fit.name),
                    None => fit.name.clone(),
                };
                h_flex()
                    .id(SharedString::from(format!("tab-{i}")))
                    .gap_1()
                    .px_2()
                    .py_0p5()
                    .rounded(px(4.))
                    .bg(if selected {
                        theme.tab_active
                    } else {
                        theme.tab
                    })
                    .text_color(if selected {
                        theme.tab_active_foreground
                    } else {
                        theme.tab_foreground
                    })
                    .border_1()
                    .border_color(if selected { theme.ring } else { theme.border })
                    .when_some(fit.ship, |tab, ship| {
                        tab.child(widgets::icon_box(
                            &loaded.catalog,
                            &loaded.icons,
                            Icon::Type(ship),
                            16.,
                        ))
                    })
                    .child(div().text_sm().child(label))
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.saved.current = Some(i);
                        this.clear_selection();
                        this.changed(cx);
                    }))
                    .child(
                        div()
                            .id(SharedString::from(format!("tab-close-{i}")))
                            .px_1()
                            .rounded(px(3.))
                            .hover(|s| s.bg(cx.theme().secondary_hover))
                            .child("×")
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                cx.stop_propagation();
                                this.close_fit(i, cx);
                            })),
                    )
            }))
            .child(
                Button::new("new-fit")
                    .ghost()
                    .xsmall()
                    .label("+")
                    .tooltip("New fit")
                    .on_click(cx.listener(|this, _, _, cx| this.add_fit(Fit::new("New fit"), cx))),
            )
    }
}

impl Gyfa {
    /// Show an item in the market browser: clear the search, open its groups, scroll to it.
    pub(super) fn jump_to_market(
        &mut self,
        id: TypeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(loaded) = self.loaded() else { return };
        self.browser_tab = BrowserTab::Market;
        self.search_text.clear();
        self.search
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.expanded_groups.extend(loaded.catalog.market_path(id));
        self.market_highlight = Some(id);
        self.market_scroll_pending = true;
        cx.notify();
    }

    /// Show the fittings browser narrowed to one ship's fits.
    pub(super) fn jump_to_fittings(&mut self, ship: TypeId, cx: &mut Context<Self>) {
        self.browser_tab = BrowserTab::Fittings;
        self.fittings_ship = Some(ship);
        cx.notify();
    }
}

fn load(data_dir: &Path) -> Result<Loaded, String> {
    if !data_dir.join("phobos/metadata.json").exists() {
        return Err(format!(
            "No EVE data in {}.\nRun scripts/fetch-data.sh, or point GYFA_DATA at a deimos dump.",
            data_dir.display()
        ));
    }
    let cache_dir = data_dir.parent().unwrap_or(data_dir).join("cache");
    let catalog = Catalog::load_cached(data_dir, &cache_dir)?;
    let engine = Engine::load(data_dir, &cache_dir, catalog.skills.clone())?;
    let icons = Icons::new(data_dir, &cache_dir);
    Ok(Loaded {
        catalog,
        engine,
        icons,
    })
}

impl Render for Gyfa {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let root = v_flex()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .text_sm();
        let loaded = match &self.loading {
            Loading::Ready(loaded) => Rc::clone(loaded),
            Loading::Pending => {
                return root
                    .items_center()
                    .justify_center()
                    .child("Loading EVE data…")
                    .into_any_element();
            }
            Loading::Failed(e) => {
                return root
                    .items_center()
                    .justify_center()
                    .text_color(cx.theme().danger)
                    .child(e.clone())
                    .into_any_element();
            }
        };
        self.sync_inputs(window, cx);

        let center = v_flex()
            .size_full()
            .child(self.fit_tabs(&loaded, cx))
            .child(if self.fit().is_some() {
                v_resizable("center")
                    .child(resizable_panel().child(self.fitting_view(&loaded, window, cx)))
                    .child(
                        resizable_panel()
                            .size(px(200.))
                            .size_range(px(80.)..px(600.))
                            .child(self.additions(&loaded, window, cx)),
                    )
                    .into_any_element()
            } else {
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(cx.theme().muted_foreground)
                    .child("Double-click a ship in the market to start a fit, or import one from the File menu.")
                    .into_any_element()
            });

        root.child(self.menu_bar(cx))
            .child(
                h_resizable("main")
                    .child(
                        resizable_panel()
                            .size(px(280.))
                            .size_range(px(200.)..px(520.))
                            .child(self.browser(&loaded, cx)),
                    )
                    .child(resizable_panel().child(center))
                    .child(
                        resizable_panel()
                            .size(px(340.))
                            .size_range(px(260.)..px(560.))
                            .child(self.stats_panel(&loaded, cx)),
                    ),
            )
            .into_any_element()
    }
}

/// Inputs that the views embed, exposed for them.
impl Gyfa {
    fn search_input(&self) -> Input {
        Input::new(&self.search).small()
    }

    fn name_field(&self) -> Input {
        Input::new(&self.name_input).small()
    }

    fn notes_field(&self) -> Textarea {
        Textarea::new(&self.notes_input)
    }

    fn weak(cx: &Context<Self>) -> WeakEntity<Self> {
        cx.entity().downgrade()
    }

    fn app_update(
        weak: &WeakEntity<Self>,
        cx: &mut App,
        f: impl FnOnce(&mut Self, &mut Context<Self>),
    ) {
        let _ = weak.update(cx, f);
    }
}
