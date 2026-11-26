//! What the universal search finds for a query, and what picking a result does.

use std::cmp::Reverse;

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::{ClickEvent, Context, Modifiers, SharedString, Window};

use super::commands::{COMMANDS, CommandDef, CommandRun, SETTINGS, WITH_SHIP, WITHOUT_SHIP};
use super::{Act, Entry, Glyph, Group, Tag};
use crate::{
    character::CharacterId,
    data::{ItemType, Kind, Slot, TypeId},
    finder::{Query, Scope},
    ui::{
        Action, AdditionsTab, Efa, Loaded, RowRef,
        additions::{Link, goes_to_cargo},
        item_stats::StatsOf,
        preferences::Page,
        widgets::count,
    },
};

/// Results a group shows at most once a scope is picked.
const SCOPED_LIMIT: usize = 40;
/// Market matches looked at before the best are kept.
const ITEM_POOL: usize = 400;
/// The rank of an item the market search found through its jargon, "hml" for a launcher.
const JARGON: u32 = 300;
/// What an item gains by being something the current fit can use.
const USABLE: u32 = 40;
/// Charges "Add 1000 to cargo" adds, the market menu's batch.
const AMMO_BATCH: u32 = 1000;

/// What picking a result or one of its actions does.
#[derive(Debug, Clone, Copy)]
pub enum Run {
    Command(CommandRun),
    Preferences(Page),
    /// Switch a preference, by its place in [`SETTINGS`].
    Toggle(usize),
    /// What a double-click in the market does with the item.
    Use(TypeId),
    /// Load a charge into every module that takes it.
    Load(TypeId),
    FillRack(Slot, TypeId),
    Drones(TypeId, u32),
    Cargo(TypeId, u32),
    FillCargo(TypeId),
    DamagePattern(TypeId),
    Project(TypeId),
    Stats(TypeId),
    ShowInMarket(TypeId),
    NewFit(TypeId),
    ShipFits(TypeId),
    OpenFit(usize),
    OpenFitInTab(usize),
    CopyFit(usize),
    LinkFit(Link, usize),
    RenameFit(usize),
    DeleteFit(usize),
    Character(CharacterId),
}

impl Efa {
    /// The results for the search as it stands: its query, scope, and the result whose actions
    /// are listed in place of results.
    pub(super) fn find(&self) -> Vec<Group> {
        let finder = &self.finder;
        let query = Query::parse(&finder.query);
        if let Some((entry, _)) = &finder.focused {
            let entries = entry
                .acts
                .iter()
                .filter(|act| query.is_empty() || query.score(&act.label).is_some())
                .map(Entry::of_act)
                .collect();
            return vec![Group {
                title: None,
                entries,
            }];
        }
        let loaded = self.loaded();
        let scope = finder.scope;
        if query.is_empty() {
            let mut groups = self.suggestions(scope, loaded.as_deref());
            groups.retain(|g| !g.entries.is_empty());
            return groups;
        }
        let cap = |all: usize| {
            if scope == Scope::All {
                all
            } else {
                SCOPED_LIMIT
            }
        };
        let mut groups: Vec<(u32, Group)> = Vec::new();
        let mut push = |title: &'static str, mut found: Vec<(u32, Entry)>, limit: usize| {
            // What can be picked comes first, best match first; ties keep their order.
            found.sort_by_key(|(score, entry)| (entry.disabled, Reverse(*score)));
            found.truncate(limit);
            if found.is_empty() {
                return;
            }
            let best = found
                .iter()
                .filter(|(_, entry)| !entry.disabled)
                .map(|(score, _)| *score)
                .max()
                .unwrap_or(0);
            groups.push((
                best,
                Group {
                    title: Some(title.into()),
                    entries: found.into_iter().map(|(_, entry)| entry).collect(),
                },
            ));
        };
        if scope.shows(Scope::Actions) {
            push("Actions", self.find_commands(&query), cap(6));
            push("Characters", self.find_characters(&query), cap(3));
        }
        if let Some(loaded) = &loaded {
            if scope.shows(Scope::Items) {
                push("Items", self.find_items(loaded, &query), cap(8));
            }
            if scope.shows(Scope::Ships) {
                push("Ships", self.find_ships(loaded, &query), cap(4));
            }
            if scope.shows(Scope::Fits) {
                push("Fits", self.find_fits(loaded, &query), cap(5));
            }
        }
        if scope.shows(Scope::Settings) {
            push("Settings", self.find_settings(&query), cap(5));
        }
        // The group with the best match leads, so Enter takes the best match overall.
        groups.sort_by_key(|(best, _)| Reverse(*best));
        groups.into_iter().map(|(_, group)| group).collect()
    }

    /// What shows before anything is typed: what is likely wanted next, and what was used last.
    fn suggestions(&self, scope: Scope, loaded: Option<&Loaded>) -> Vec<Group> {
        let group = |title: &'static str, entries: Vec<Entry>| Group {
            title: Some(title.into()),
            entries,
        };
        let flags = self.menu_flags();
        let recent_fits = |limit: usize| -> Vec<Entry> {
            let Some(loaded) = loaded else {
                return Vec::new();
            };
            crate::ship_browser::recent_fits(&self.saved.fits)
                .into_iter()
                .take(limit)
                .filter_map(|i| self.fit_entry(loaded, i))
                .collect()
        };
        let recent_items = |limit: usize| -> Vec<Entry> {
            let Some(loaded) = loaded else {
                return Vec::new();
            };
            self.saved
                .settings
                .recent_items
                .iter()
                .filter_map(|&id| loaded.catalog.get(id))
                .take(limit)
                .map(|t| self.item_entry(loaded, t, None))
                .collect()
        };
        match scope {
            Scope::All => {
                let labels = if flags.has_ship {
                    WITH_SHIP
                } else {
                    WITHOUT_SHIP
                };
                let suggested = labels
                    .iter()
                    .filter_map(|label| COMMANDS.iter().find(|c| c.label == *label))
                    .map(|def| command_entry(def, flags))
                    .collect();
                vec![
                    group("Suggested", suggested),
                    group("Recent fits", recent_fits(5)),
                    group("Recently used", recent_items(6)),
                ]
            }
            Scope::Actions => vec![group(
                "Actions",
                COMMANDS
                    .iter()
                    .map(|def| command_entry(def, flags))
                    .collect(),
            )],
            Scope::Items => vec![group("Recently used", recent_items(SCOPED_LIMIT))],
            Scope::Ships => {
                let Some(loaded) = loaded else {
                    return Vec::new();
                };
                let counts = crate::ship_browser::fit_counts(&self.saved.fits);
                let mut ships: Vec<TypeId> = Vec::new();
                for i in crate::ship_browser::recent_fits(&self.saved.fits) {
                    if let Some(ship) = self.saved.fits[i].ship
                        && !ships.contains(&ship)
                    {
                        ships.push(ship);
                    }
                }
                let entries = ships
                    .into_iter()
                    .take(12)
                    .filter_map(|ship| loaded.catalog.get(ship))
                    .map(|t| ship_entry(t, counts.get(&t.id).copied().unwrap_or(0)))
                    .collect();
                vec![group("Recently fitted", entries)]
            }
            Scope::Fits => vec![group("Recent fits", recent_fits(SCOPED_LIMIT))],
            Scope::Settings => vec![
                group(
                    "Settings",
                    (0..SETTINGS.len()).map(|i| self.setting_entry(i)).collect(),
                ),
                group(
                    "Preferences",
                    Page::ALL.iter().map(|&page| page_entry(page)).collect(),
                ),
            ],
        }
    }

    fn find_commands(&self, query: &Query) -> Vec<(u32, Entry)> {
        let flags = self.menu_flags();
        COMMANDS
            .iter()
            .filter_map(|def| {
                let keywords = format!("{} {}", def.menu, def.keywords);
                let score = query.score_with(def.label, &keywords)?;
                Some((score, command_entry(def, flags)))
            })
            .collect()
    }

    /// The characters, to fly the current fit with.
    fn find_characters(&self, query: &Query) -> Vec<(u32, Entry)> {
        if self.fit().is_none() {
            return Vec::new();
        }
        let current = self.fit_character();
        self.characters
            .ordered()
            .into_iter()
            .filter_map(|id| {
                let name = self.characters.name(id);
                let score = query.score_with(&name, "character pilot fly skills")?;
                let entry = Entry {
                    glyph: Glyph::Icon(AssetIcon::CircleUser),
                    label: name.into(),
                    detail: Some("Character".into()),
                    tag: if id == current {
                        Tag::Text("Flying".into())
                    } else {
                        Tag::None
                    },
                    acts: vec![Act::new(
                        "Fly this fit",
                        AssetIcon::CircleUser,
                        Run::Character(id),
                    )],
                    disabled: false,
                };
                Some((score, entry))
            })
            .collect()
    }

    /// Market items the way the market search finds them, jargon included, with what the
    /// current fit can use ahead of what it cannot.
    fn find_items(&self, loaded: &Loaded, query: &Query) -> Vec<(u32, Entry)> {
        let catalog = &loaded.catalog;
        let room = self.ship_room(catalog);
        let mut found: Vec<(u32, &ItemType)> =
            crate::market::search(catalog, &query.text, ITEM_POOL)
                .into_iter()
                .filter_map(|id| catalog.get(id))
                .filter(|t| t.kind != Kind::Ship)
                .map(|t| {
                    let usable = room.as_ref().is_some_and(|room| room.can_use(t));
                    let score = query.score(&t.name).unwrap_or(JARGON);
                    (score + if usable { USABLE } else { 0 }, t)
                })
                .collect();
        // Stable, so equal matches stay in market order.
        found.sort_by_key(|(score, _)| Reverse(*score));
        found.truncate(SCOPED_LIMIT);
        found
            .into_iter()
            .map(|(score, t)| (score, self.item_entry(loaded, t, query.amount)))
            .collect()
    }

    fn find_ships(&self, loaded: &Loaded, query: &Query) -> Vec<(u32, Entry)> {
        let catalog = &loaded.catalog;
        let counts = crate::ship_browser::fit_counts(&self.saved.fits);
        catalog
            .ship_groups
            .iter()
            .flat_map(|(_, ships)| ships)
            .filter_map(|&id| catalog.get(id))
            .filter_map(|t| {
                let score = query.score(&t.name)?;
                let fits = counts.get(&t.id).copied().unwrap_or(0);
                Some((score, ship_entry(t, fits)))
            })
            .collect()
    }

    /// Saved fits by their name, or by their ship's.
    fn find_fits(&self, loaded: &Loaded, query: &Query) -> Vec<(u32, Entry)> {
        self.saved
            .fits
            .iter()
            .enumerate()
            .filter_map(|(i, fit)| {
                let ship = loaded.catalog.name(fit.ship?);
                let score = query.score_with(&fit.name, &ship)?;
                Some((score, self.fit_entry(loaded, i)?))
            })
            .collect()
    }

    fn find_settings(&self, query: &Query) -> Vec<(u32, Entry)> {
        let settings = SETTINGS.iter().enumerate().filter_map(|(i, def)| {
            let keywords = format!("{} {}", def.hint, def.page.title());
            let score = query.score_with(def.label, &keywords)?;
            Some((score, self.setting_entry(i)))
        });
        let pages = Page::ALL.iter().filter_map(|&page| {
            let entry = page_entry(page);
            let keywords = format!("{} settings options", page.summary());
            let score = query.score_with(&entry.label, &keywords)?;
            Some((score, entry))
        });
        settings.chain(pages).collect()
    }

    fn setting_entry(&self, index: usize) -> Entry {
        let def = &SETTINGS[index];
        let on = (def.get)(&self.saved);
        let (icon, verb) = if on {
            (AssetIcon::ToggleRight, "Turn off")
        } else {
            (AssetIcon::ToggleLeft, "Turn on")
        };
        Entry {
            glyph: Glyph::Icon(icon),
            label: def.label.into(),
            detail: Some(def.page.title().into()),
            tag: Tag::Switch(on),
            acts: vec![
                Act::new(verb, icon, Run::Toggle(index)).stay(),
                Act::new(
                    format!("Open the {} preferences", def.page.title()),
                    AssetIcon::Settings,
                    Run::Preferences(def.page),
                ),
            ],
            disabled: false,
        }
    }

    /// A market item with what can be done with it on the current fit. The first action is
    /// what Enter does: an amount typed with the query comes before everything else.
    fn item_entry(&self, loaded: &Loaded, t: &ItemType, amount: Option<u32>) -> Entry {
        let catalog = &loaded.catalog;
        let id = t.id;
        let ship = self
            .fit()
            .and_then(|f| f.ship)
            .and_then(|ship| catalog.get(ship));
        let has_ship = ship.is_some();
        let structure = ship.is_some_and(ItemType::is_structure);
        // pyfa offers structures only charges for their cargo.
        let cargo = has_ship && (!structure || t.kind == Kind::Charge);
        let to_cargo = |n: u32| {
            let label = if n == 1 {
                "Add to cargo".to_owned()
            } else {
                format!("Add {n} to cargo")
            };
            Act::new(label, AssetIcon::Package, Run::Cargo(id, n))
        };
        let to_bay = |n: u32| {
            Act::new(
                format!("Add {n} to the drone bay"),
                AssetIcon::Plus,
                Run::Drones(id, n),
            )
        };
        let mut acts = Vec::new();
        if let Some(n) = amount {
            if t.kind == Kind::Drone && has_ship {
                acts.push(to_bay(n));
            } else if cargo {
                acts.push(to_cargo(n));
            }
        }
        if has_ship {
            match t.kind {
                Kind::Module(slot) => {
                    acts.push(Act::new("Fit", AssetIcon::Plus, Run::Use(id)));
                    if slot != Slot::Subsystem {
                        acts.push(Act::new(
                            "Fill the rack",
                            AssetIcon::Layers,
                            Run::FillRack(slot, id),
                        ));
                    }
                }
                Kind::Charge => {
                    let takers = self.modules_loading(loaded, t).len();
                    if takers > 0 {
                        acts.push(Act::new(
                            format!("Load into {}", count(takers, "module", "modules")),
                            AssetIcon::Plus,
                            Run::Load(id),
                        ));
                    }
                }
                Kind::Drone => match self.drone_stack_size(loaded, id) {
                    0 => acts.push(Act::new("Add to fit", AssetIcon::Plus, Run::Use(id))),
                    n if amount != Some(n) => acts.push(to_bay(n)),
                    _ => {}
                },
                Kind::Fighter | Kind::Implant | Kind::Booster | Kind::Mode => {
                    acts.push(Act::new("Add to fit", AssetIcon::Plus, Run::Use(id)));
                }
                Kind::Ship | Kind::Skill | Kind::Cargo | Kind::Other => {}
            }
        }
        if cargo {
            if amount != Some(1) {
                acts.push(to_cargo(1));
            }
            if t.kind == Kind::Charge && amount != Some(AMMO_BATCH) {
                acts.push(to_cargo(AMMO_BATCH));
            }
            if goes_to_cargo(t.kind) {
                acts.push(Act::new(
                    "Fill the cargo hold",
                    AssetIcon::PackagePlus,
                    Run::FillCargo(id),
                ));
            }
        }
        if has_ship && t.damage.iter().any(|&d| d > 0.0) {
            acts.push(Act::new(
                "Use as damage pattern",
                AssetIcon::Target,
                Run::DamagePattern(id),
            ));
        }
        if has_ship && !structure && catalog.projection.projectable.contains(&id) {
            acts.push(Act::new(
                "Project onto the fit",
                AssetIcon::Globe,
                Run::Project(id),
            ));
        }
        acts.push(Act::new("Show info", AssetIcon::Info, Run::Stats(id)));
        let group = t.market_group.and_then(|g| catalog.market_groups.get(&g));
        if group.is_some() {
            acts.push(Act::new(
                "Show in the market",
                AssetIcon::Store,
                Run::ShowInMarket(id),
            ));
        }
        Entry {
            glyph: Glyph::Type(id),
            label: t.name.clone().into(),
            detail: group.map(|g| g.name.clone().into()),
            tag: Tag::Text(t.category_label().into()),
            acts,
            disabled: false,
        }
    }

    /// The fitted modules that can load a charge.
    fn modules_loading(&self, loaded: &Loaded, charge: &ItemType) -> Vec<RowRef> {
        let Some(fit) = self.fit() else {
            return Vec::new();
        };
        [Slot::High, Slot::Mid, Slot::Low]
            .into_iter()
            .flat_map(|slot| {
                fit.rack(slot)
                    .iter()
                    .enumerate()
                    .filter(|(_, module)| {
                        module
                            .as_ref()
                            .and_then(|m| loaded.catalog.get(m.type_id))
                            .is_some_and(|m| charge.fits_in(m))
                    })
                    .map(move |(i, _)| RowRef::Module(slot, i))
            })
            .collect()
    }

    /// A saved fit, when it still is one and has a ship.
    fn fit_entry(&self, loaded: &Loaded, index: usize) -> Option<Entry> {
        let fit = self.saved.fits.get(index)?;
        let ship = fit.ship?;
        let current = self.saved.settings.current;
        let mut acts = vec![
            Act::new("Open", AssetIcon::FolderOpen, Run::OpenFit(index)),
            Act::new(
                "Open in a new tab",
                AssetIcon::Plus,
                Run::OpenFitInTab(index),
            ),
            Act::new("Duplicate", AssetIcon::Copy, Run::CopyFit(index)),
        ];
        let on_a_ship = self.fit().is_some_and(|f| f.ship.is_some());
        if on_a_ship && current != Some(index) {
            acts.push(Act::new(
                "Project onto the current fit",
                AssetIcon::Globe,
                Run::LinkFit(Link::Projected, index),
            ));
            acts.push(Act::new(
                "Add as command booster",
                AssetIcon::Network,
                Run::LinkFit(Link::Command, index),
            ));
        }
        acts.push(Act::new("Rename", AssetIcon::Pencil, Run::RenameFit(index)));
        acts.push(Act::new("Delete", AssetIcon::Trash, Run::DeleteFit(index)));
        let label: SharedString = if fit.name.is_empty() {
            "Unnamed fit".into()
        } else {
            fit.name.clone().into()
        };
        let tag = if current == Some(index) {
            "Current"
        } else if self.saved.settings.open.contains(&index) {
            "Open"
        } else {
            "Fit"
        };
        Some(Entry {
            glyph: Glyph::Type(ship),
            label,
            detail: Some(loaded.catalog.name(ship).into()),
            tag: Tag::Text(tag.into()),
            acts,
            disabled: false,
        })
    }

    /// Do what a result or one of its actions stands for.
    pub(super) fn finder_run(&mut self, run: Run, window: &mut Window, cx: &mut Context<Self>) {
        match run {
            Run::Command(CommandRun::Action(make)) => {
                // Once this update is over: the action's handler updates this window again.
                let root = self.root_focus.clone();
                window.defer(cx, move |window, cx| {
                    root.dispatch_action(make().as_ref(), window, cx);
                });
            }
            Run::Command(CommandRun::Url(url)) => cx.open_url(url),
            Run::Command(CommandRun::AdditionsTab(tab)) => {
                self.show_additions_tab(tab);
                cx.notify();
            }
            Run::Preferences(page) => self.open_preferences_page(page, cx),
            Run::Toggle(index) => self.toggle_setting(index, cx),
            Run::Use(id) => {
                self.used_from_market(id);
                self.market_double_click(id, Modifiers::default(), cx);
            }
            Run::Load(id) => {
                let Some(loaded) = self.loaded() else { return };
                let Some(charge) = loaded.catalog.get(id) else {
                    return;
                };
                let rows = self.modules_loading(&loaded, charge);
                self.used_from_market(id);
                self.apply(Action::SetCharge(rows, Some(id)), cx);
            }
            Run::FillRack(slot, id) => {
                self.used_from_market(id);
                self.apply(Action::FillRack(slot, id), cx);
            }
            Run::Drones(id, amount) => {
                self.used_from_market(id);
                self.show_additions_tab(AdditionsTab::Drones);
                self.apply(Action::AddDrones(id, amount), cx);
            }
            Run::Cargo(id, amount) => {
                self.used_from_market(id);
                self.add_to_cargo(id, amount, cx);
            }
            Run::FillCargo(id) => {
                self.used_from_market(id);
                self.fill_cargo_with(id, cx);
            }
            Run::DamagePattern(id) => {
                let Some(loaded) = self.loaded() else { return };
                let Some(t) = loaded.catalog.get(id) else {
                    return;
                };
                let profile = crate::fit::DamageProfile::Pattern {
                    name: crate::damage_pattern::SELECTED_AMMO.to_owned(),
                    amounts: t.damage,
                };
                self.apply(Action::SetDamageProfile(profile), cx);
            }
            Run::Project(id) => {
                self.used_from_market(id);
                self.project_item(id, cx);
                self.show_additions_tab(AdditionsTab::Projected);
                cx.notify();
            }
            Run::Stats(id) => {
                self.show_stats(StatsOf::Type(id), &ClickEvent::default(), window, cx);
            }
            Run::ShowInMarket(id) => self.jump_to_market(id, window, cx),
            Run::NewFit(ship) => self.new_fit_for_ship(ship, cx),
            Run::ShipFits(ship) => self.jump_to_fittings(ship, cx),
            Run::OpenFit(index) if index < self.saved.fits.len() => {
                self.open_from_browser(index, cx);
            }
            Run::OpenFitInTab(index) if index < self.saved.fits.len() => self.open_fit(index, cx),
            Run::CopyFit(index) => self.copy_fit(index, cx),
            Run::LinkFit(link, index) if index < self.saved.fits.len() => {
                self.link(link, &[index], cx);
            }
            Run::RenameFit(index) if index < self.saved.fits.len() => {
                self.open_fit(index, cx);
                // After the search closed and handed the focus back, which the name field
                // then takes.
                let this = cx.weak_entity();
                window.defer(cx, move |window, cx| {
                    let _ = this.update(cx, |g, cx| g.start_rename(window, cx));
                });
            }
            Run::DeleteFit(index) if index < self.saved.fits.len() => {
                self.confirm_delete_fit(index, false, window, cx);
            }
            Run::Character(id) => self.set_fit_character(id, cx),
            // The fit went while the search was open.
            Run::OpenFit(_)
            | Run::OpenFitInTab(_)
            | Run::LinkFit(..)
            | Run::RenameFit(_)
            | Run::DeleteFit(_) => {}
        }
    }

    /// Switch a preference the way the preferences window does, through the hub, so every
    /// window gets it.
    fn toggle_setting(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(def) = SETTINGS.get(index) else {
            return;
        };
        let on = !(def.get)(&self.saved);
        // Here too, without waiting for the hub to hand it back: the results show the switch.
        (def.set)(&mut self.saved, on);
        self.hub
            .update(cx, |hub, cx| hub.update_saved(|s| (def.set)(s, on), cx));
        self.recompute();
        self.set_status(format!("{}: {}", def.label, if on { "on" } else { "off" }));
        cx.notify();
    }
}

fn command_entry(def: &CommandDef, flags: crate::ui::menu::Flags) -> Entry {
    let (tag, verb) = match def.run {
        CommandRun::Action(make) => (Tag::Shortcut(make), "Run"),
        CommandRun::Url(_) => (Tag::None, "Open in the browser"),
        CommandRun::AdditionsTab(_) => (Tag::None, "Show"),
    };
    Entry {
        glyph: Glyph::Icon(def.icon),
        label: def.label.into(),
        detail: Some(def.menu.into()),
        tag,
        acts: vec![Act::new(
            verb,
            AssetIcon::CornerDownLeft,
            Run::Command(def.run),
        )],
        disabled: !(def.enabled)(flags),
    }
}

fn page_entry(page: Page) -> Entry {
    Entry {
        glyph: Glyph::Icon(page.icon()),
        label: format!("Preferences: {}", page.title()).into(),
        detail: Some(page.summary().into()),
        tag: Tag::None,
        acts: vec![Act::new(
            "Open",
            AssetIcon::Settings,
            Run::Preferences(page),
        )],
        disabled: false,
    }
}

fn ship_entry(t: &ItemType, fits: usize) -> Entry {
    let mut acts = vec![Act::new("New fit", AssetIcon::Plus, Run::NewFit(t.id))];
    if fits > 0 {
        acts.push(Act::new(
            "Show its fits",
            AssetIcon::ListTree,
            Run::ShipFits(t.id),
        ));
    }
    acts.push(Act::new("Show info", AssetIcon::Info, Run::Stats(t.id)));
    Entry {
        glyph: Glyph::Type(t.id),
        label: t.name.clone().into(),
        detail: (fits > 0).then(|| count(fits, "fit", "fits").into()),
        tag: Tag::Text(t.category_label().into()),
        acts,
        disabled: false,
    }
}
