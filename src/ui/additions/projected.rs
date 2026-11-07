//! The Projected tab, pyfa's projected pane: fits, modules, drones and fighters projecting onto
//! the active fit, and environmental effects, with pyfa's state clicks, ranges, amounts, menus
//! and drops.

use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme, IconName, h_flex,
    menu::{ContextMenuExt, PopupMenu, PopupMenuItem},
    v_flex,
};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, InteractiveElement, IntoElement, KeyDownEvent, Modifiers,
    MouseButton, MouseDownEvent, ParentElement, SharedString, StatefulInteractiveElement, Styled,
    WeakEntity, Window, div, prelude::FluentBuilder, px,
};

use super::{
    FitDrag, MENU_MAX_H, ROW_HEIGHT,
    drones::format_amount,
    linked::{Link, add_fit_entries},
};
use crate::{
    data::{Catalog, Kind, TypeId, projection::EnvMenu},
    engine::{FighterInfo, FitItem, Stats},
    fit::{
        Fit, ModuleState,
        projected::{MAX_FIT_COPIES, Projected},
    },
    icons::Icon,
    store::Saved,
    ui::{
        Efa, Loaded, RowRef,
        columns::{ICON_W, STATE_W},
        fitting::{ChargeMenu, DragSource, Dragged, compatible_charges, structure_charges},
        item_stats::{StatsOf, stats_entry},
        widgets::{color, entry, icon, icon_box, icon_label, item_entry},
    },
};

const RANGE_W: f32 = 70.;
const HEADER_ICON: f32 = 18.;

/// pyfa's drone order for projected drones, by market group.
const DRONE_ORDER: [&str; 12] = [
    "Light Scout Drones",
    "Light Hybrid Drones",
    "Medium Scout Drones",
    "Medium Hybrid Drones",
    "Heavy Attack Drones",
    "Heavy Hybrid Drones",
    "Sentry Drones",
    "Combat Utility Drones",
    "Electronic Warfare Drones",
    "Logistic Drones",
    "Mining Drones",
    "Salvage Drones",
];

/// A row of the pane and the entry it shows, by index into its list on the fit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Fit { entry: usize, fit: usize },
    Module(usize),
    Env(usize),
    Drone(usize),
    Fighter(usize),
}

/// What a state click proposes for everything it touches, as pyfa's
/// `GuiChangeProjectedItemStatesCommand` works it out from the clicked row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Proposal {
    Active,
    Inactive,
    Overheat,
}

/// Which of the clicked row's peers an action takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// The selection, or the clicked row when it is not selected.
    Selection,
    /// Every module or squadron of the clicked one's type.
    Similar,
}

impl Reach {
    fn from_modifiers(modifiers: Modifiers) -> Self {
        if modifiers.alt || modifiers.control {
            Self::Similar
        } else {
            Self::Selection
        }
    }
}

/// The pane's rows in pyfa's order: fits by name, then modules and environmental effects by
/// name, drones by class, fighters by name. Fits that were deleted are left out.
fn rows(catalog: &Catalog, saved: &Saved, fit: &Fit) -> Vec<Row> {
    let projected = &fit.projected;
    let mut fits: Vec<Row> = projected
        .fits
        .iter()
        .enumerate()
        .filter_map(|(entry, p)| {
            Some(Row::Fit {
                entry,
                fit: saved.fit_index(p.fit)?,
            })
        })
        .collect();
    fits.sort_by_cached_key(|r| match *r {
        Row::Fit { fit, .. } => saved.fits[fit].name.clone(),
        _ => String::new(),
    });
    let mut modules: Vec<(String, Row)> = projected
        .modules
        .iter()
        .enumerate()
        .map(|(i, m)| (catalog.name(m.type_id), Row::Module(i)))
        .chain(
            projected
                .env
                .iter()
                .enumerate()
                .map(|(i, e)| (env_name(catalog, e.type_id), Row::Env(i))),
        )
        .collect();
    modules.sort_by(|a, b| a.0.cmp(&b.0));
    let mut drones: Vec<usize> = (0..projected.drones.len()).collect();
    drones.sort_by_cached_key(|&i| {
        let t = projected.drones[i].type_id;
        let group = catalog
            .get(t)
            .and_then(|t| t.market_group)
            .and_then(|g| catalog.market_groups.get(&g))
            .map(|g| g.name.as_str());
        let rank = group
            .and_then(|g| DRONE_ORDER.iter().position(|&o| o == g))
            .unwrap_or(DRONE_ORDER.len());
        (rank, catalog.name(t))
    });
    let mut fighters: Vec<usize> = (0..projected.fighters.len()).collect();
    fighters.sort_by_cached_key(|&i| catalog.name(projected.fighters[i].fighter.type_id));
    fits.into_iter()
        .chain(modules.into_iter().map(|(_, r)| r))
        .chain(drones.into_iter().map(Row::Drone))
        .chain(fighters.into_iter().map(Row::Fighter))
        .collect()
}

fn env_name(catalog: &Catalog, type_id: TypeId) -> String {
    catalog
        .projection
        .env_name(type_id)
        .map_or_else(|| catalog.name(type_id), str::to_owned)
}

/// pyfa's projection range text: "10 km", "500 m", nothing without a range.
fn range_text(range: Option<f64>) -> String {
    match range {
        None => String::new(),
        Some(m) if m >= 1000.0 => format!("{} km", format_amount(m / 1000.0, 0, false)),
        Some(m) => format!("{} m", format_amount(m, 0, false)),
    }
}

/// What the range prompt takes: kilometres, or nothing for no range. Like pyfa, anything but
/// digits and the point is ignored.
struct RangeInput(Option<f64>);

impl std::str::FromStr for RangeInput {
    type Err = std::num::ParseFloatError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let clean: String = text
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        if clean.is_empty() {
            return Ok(Self(None));
        }
        Ok(Self(Some(clean.parse::<f64>()? * 1000.0)))
    }
}

impl Projected {
    fn range(&self, row: Row) -> Option<f64> {
        match row {
            Row::Fit { entry, .. } => self.fits.get(entry)?.range,
            Row::Module(i) => self.modules.get(i)?.range,
            Row::Drone(i) => self.drones.get(i)?.range,
            Row::Fighter(i) => self.fighters.get(i)?.range,
            Row::Env(_) => None,
        }
    }

    fn set_range(&mut self, row: Row, range: Option<f64>) {
        let slot = match row {
            Row::Fit { entry, .. } => self.fits.get_mut(entry).map(|p| &mut p.range),
            Row::Module(i) => self.modules.get_mut(i).map(|m| &mut m.range),
            Row::Drone(i) => self.drones.get_mut(i).map(|d| &mut d.range),
            Row::Fighter(i) => self.fighters.get_mut(i).map(|f| &mut f.range),
            Row::Env(_) => None,
        };
        if let Some(slot) = slot {
            *slot = range;
        }
    }

    fn is_active(&self, row: Row) -> bool {
        match row {
            Row::Fit { entry, .. } => self.fits.get(entry).is_some_and(|p| p.active),
            Row::Module(i) => self
                .modules
                .get(i)
                .is_some_and(|m| m.state != ModuleState::Offline),
            Row::Env(i) => self.env.get(i).is_some_and(|e| e.active),
            Row::Drone(i) => self.drones.get(i).is_some_and(|d| d.active),
            Row::Fighter(i) => self.fighters.get(i).is_some_and(|f| f.fighter.active),
        }
    }

    /// The left-click proposal for a clicked row: off when it is on, on otherwise. A module only
    /// counts as on while active, as pyfa's projected state map has it.
    fn proposal(&self, row: Row) -> Proposal {
        let on = match row {
            Row::Module(i) => self
                .modules
                .get(i)
                .is_some_and(|m| m.state == ModuleState::Active),
            _ => self.is_active(row),
        };
        if on {
            Proposal::Inactive
        } else {
            Proposal::Active
        }
    }

    fn apply_proposal(&mut self, catalog: &Catalog, rows: &[Row], proposal: Proposal) {
        let on = proposal != Proposal::Inactive;
        for &row in rows {
            match row {
                Row::Fit { entry, .. } => {
                    if let Some(p) = self.fits.get_mut(entry) {
                        p.active = on;
                    }
                }
                Row::Module(i) => {
                    let Some(m) = self.modules.get_mut(i) else {
                        continue;
                    };
                    let t = catalog.get(m.type_id);
                    let activatable = t.is_some_and(|t| t.activatable);
                    let overloadable = t.is_some_and(|t| t.overloadable);
                    m.state = match proposal {
                        Proposal::Inactive => ModuleState::Offline,
                        Proposal::Overheat if overloadable => ModuleState::Overload,
                        _ if activatable => ModuleState::Active,
                        _ => ModuleState::Online,
                    };
                }
                Row::Env(i) => {
                    if let Some(e) = self.env.get_mut(i) {
                        e.active = on;
                    }
                }
                Row::Drone(i) => {
                    if let Some(d) = self.drones.get_mut(i) {
                        d.active = on;
                    }
                }
                Row::Fighter(i) => {
                    if let Some(f) = self.fighters.get_mut(i) {
                        f.fighter.active = on;
                    }
                }
            }
        }
    }

    /// Take rows away: `whole` drops them, otherwise a fit loses one copy and a drone stack one
    /// drone, the way pyfa's double-click does.
    fn remove(&mut self, rows: &[Row], whole: bool) {
        let mut fits = Vec::new();
        let mut modules = Vec::new();
        let mut env = Vec::new();
        let mut drones = Vec::new();
        let mut fighters = Vec::new();
        for &row in rows {
            match row {
                Row::Fit { entry, .. } => fits.push(entry),
                Row::Module(i) => modules.push(i),
                Row::Env(i) => env.push(i),
                Row::Drone(i) => drones.push(i),
                Row::Fighter(i) => fighters.push(i),
            }
        }
        if !whole {
            for &i in &fits {
                if let Some(p) = self.fits.get_mut(i) {
                    p.amount = p.amount.saturating_sub(1);
                }
            }
            for &i in &drones {
                if let Some(d) = self.drones.get_mut(i) {
                    d.count = d.count.saturating_sub(1);
                }
            }
        }
        let drop = |len: usize, wanted: &[usize], emptied: &dyn Fn(usize) -> bool| {
            (0..len)
                .map(|i| (whole && wanted.contains(&i)) || emptied(i))
                .collect::<Vec<bool>>()
        };
        let gone_fits = drop(self.fits.len(), &fits, &|i| self.fits[i].amount == 0);
        let gone_drones = drop(self.drones.len(), &drones, &|i| self.drones[i].count == 0);
        let mut n = 0;
        self.fits.retain(|_| {
            n += 1;
            !gone_fits[n - 1]
        });
        let mut n = 0;
        self.drones.retain(|_| {
            n += 1;
            !gone_drones[n - 1]
        });
        retain_indices(&mut self.modules, &modules);
        retain_indices(&mut self.env, &env);
        retain_indices(&mut self.fighters, &fighters);
    }
}

/// Drop the entries at `indices`.
fn retain_indices<T>(list: &mut Vec<T>, indices: &[usize]) {
    let mut n = 0;
    list.retain(|_| {
        n += 1;
        !indices.contains(&(n - 1))
    });
}

impl Efa {
    fn projected_rows(&self, loaded: &Loaded) -> Vec<Row> {
        self.fit()
            .map_or_default(|fit| rows(&loaded.catalog, &self.saved, fit))
    }

    /// The rows an action on display row `n` reaches.
    fn projected_targets(&self, loaded: &Loaded, n: usize, reach: Reach) -> Vec<Row> {
        let rows = self.projected_rows(loaded);
        let Some(&clicked) = rows.get(n) else {
            return Vec::new();
        };
        let Some(fit) = self.fit() else {
            return Vec::new();
        };
        let projected = &fit.projected;
        match (reach, clicked) {
            (Reach::Similar, Row::Module(i)) => {
                let t = projected.modules[i].type_id;
                (0..projected.modules.len())
                    .filter(|&j| projected.modules[j].type_id == t)
                    .map(Row::Module)
                    .collect()
            }
            (Reach::Similar, Row::Fighter(i)) => {
                let t = projected.fighters[i].fighter.type_id;
                (0..projected.fighters.len())
                    .filter(|&j| projected.fighters[j].fighter.type_id == t)
                    .map(Row::Fighter)
                    .collect()
            }
            _ => self
                .addition_targets(n)
                .into_iter()
                .filter_map(|r| rows.get(r).copied())
                .collect(),
        }
    }

    fn edit_projected(
        &mut self,
        cx: &mut Context<Self>,
        edit: impl FnOnce(&mut Projected, &Catalog) -> bool,
    ) {
        self.edit_current(cx, |fit, catalog| edit(&mut fit.projected, catalog));
    }

    /// A state column click: left proposes the opposite of the clicked row, right overheats
    /// modules.
    fn projected_state_click(
        &mut self,
        n: usize,
        right: bool,
        modifiers: Modifiers,
        cx: &mut Context<Self>,
    ) {
        let Some(loaded) = self.loaded() else { return };
        let rows = self.projected_rows(&loaded);
        let (Some(&clicked), Some(fit)) = (rows.get(n), self.fit()) else {
            return;
        };
        let proposal = match (right, clicked) {
            (true, Row::Module(_)) => Proposal::Overheat,
            (true, _) => return,
            (false, row) => fit.projected.proposal(row),
        };
        // pyfa takes every similar module or squadron with Alt.
        let reach = if modifiers.alt {
            Reach::Similar
        } else {
            Reach::Selection
        };
        let targets = self.projected_targets(&loaded, n, reach);
        self.edit_projected(cx, |projected, catalog| {
            projected.apply_proposal(catalog, &targets, proposal);
            false
        });
    }

    fn remove_projected(&mut self, targets: &[Row], whole: bool, cx: &mut Context<Self>) {
        self.edit_projected(cx, |projected, _| {
            projected.remove(targets, whole);
            true
        });
    }

    /// A market item projected the way pyfa's drops and "Project onto Fit" do; things that do
    /// not reach other ships are refused.
    pub(in crate::ui) fn project_item(&mut self, type_id: TypeId, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded() else { return };
        let catalog = &loaded.catalog;
        let Some(t) = catalog.get(type_id) else {
            return;
        };
        if !catalog.projection.projectable.contains(&type_id) {
            self.status = Some(format!("{} cannot be projected", t.name).into());
            cx.notify();
            return;
        }
        let (kind, activatable) = (t.kind, t.activatable);
        self.edit_projected(cx, |projected, _| {
            match kind {
                Kind::Module(_) => projected.add_module(type_id, activatable),
                Kind::Drone => projected.add_drones(type_id, 1),
                Kind::Fighter => projected.add_fighter(type_id),
                _ => return false,
            }
            true
        });
    }

    fn drop_on_projected(&mut self, dragged: &Dragged, cx: &mut Context<Self>) {
        match dragged.source {
            DragSource::Market => {
                for id in dragged.types() {
                    self.project_item(id, cx);
                }
            }
            DragSource::Row(RowRef::Module(..)) => {
                self.project_item(dragged.type_id, cx);
            }
            _ => {}
        }
    }

    pub(in crate::ui) fn projected_tab(
        &mut self,
        loaded: &Rc<Loaded>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(fit) = self.fit() else {
            return div().into_any_element();
        };
        let rows = rows(&loaded.catalog, &self.saved, fit);
        let theme = cx.theme();
        let mut content: Vec<AnyElement> = Vec::with_capacity(rows.len() + 1);
        if rows.is_empty() {
            content.push(
                div()
                    .p_2()
                    .text_color(theme.muted_foreground)
                    .child("Drag an item or fit, or use right-click menu for wormhole effects")
                    .into_any_element(),
            );
        }
        for (n, &row) in rows.iter().enumerate() {
            content.push(self.projected_row(loaded, n, row, cx));
        }
        let theme = cx.theme();
        v_flex()
            .flex_1()
            .min_h_0()
            .bg(theme.background)
            .child(Self::projected_header(loaded, cx))
            .child(
                v_flex()
                    .id("projected-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_focus(&self.additions_focus)
                    .on_key_down(cx.listener(|g, event: &KeyDownEvent, _, cx| {
                        g.projected_key(event, cx);
                    }))
                    .children(content)
                    .drag_over::<Dragged>(|style, _, _, cx| style.bg(cx.theme().drop_target))
                    .on_drop(cx.listener(|g, dragged: &Dragged, _, cx| {
                        g.drop_on_projected(dragged, cx);
                    }))
                    .drag_over::<FitDrag>(|style, _, _, cx| style.bg(cx.theme().drop_target))
                    .on_drop(cx.listener(|g, drag: &FitDrag, _, cx| {
                        g.link(Link::Projected, &[drag.index], cx);
                    }))
                    .context_menu({
                        let (this, loaded) = (Self::weak(cx), Rc::clone(loaded));
                        move |menu, window, cx| {
                            projected_view_menu(menu, &this, &loaded, window, cx)
                        }
                    }),
            )
            .into_any_element()
    }

    fn projected_header(loaded: &Loaded, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        h_flex()
            .h(px(24.))
            .px_1()
            .gap_1()
            .flex_none()
            .bg(theme.table_head)
            .text_color(theme.table_head_foreground)
            .child(div().w(px(STATE_W + ICON_W * 2.)).flex_none())
            .child(div().flex_1().min_w_0().child("Name"))
            .child(div().flex_1().min_w_0().child("Charge"))
            .child(div().w(px(RANGE_W)).flex_none().child(icon_box(
                &loaded.catalog,
                &loaded.icons,
                Icon::Id(icon::RANGE),
                HEADER_ICON,
            )))
            .into_any_element()
    }

    #[expect(clippy::too_many_lines, reason = "one row of five kinds of item")]
    fn projected_row(
        &self,
        loaded: &Rc<Loaded>,
        n: usize,
        row: Row,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let catalog = &loaded.catalog;
        let theme = cx.theme();
        let Some(fit) = self.fit() else {
            return div().into_any_element();
        };
        let projected = &fit.projected;
        let stats = self.stats.as_ref();
        let (icon_id, charge, name): (Option<Icon>, Option<TypeId>, String) = match row {
            Row::Fit { entry, fit: index } => {
                let target = &self.saved.fits[index];
                let amount = projected.fits.get(entry).map_or(0, |p| p.amount);
                let ship = target.ship.map_or_default(|s| catalog.name(s));
                (
                    target.ship.map(Icon::Type),
                    None,
                    format!("{amount}x {} ({ship})", target.name),
                )
            }
            Row::Module(i) => {
                let m = &projected.modules[i];
                (
                    Some(Icon::Type(m.type_id)),
                    m.charge,
                    catalog.name(m.type_id),
                )
            }
            Row::Env(i) => {
                let t = projected.env[i].type_id;
                let icon = catalog
                    .projection
                    .env_types
                    .get(&t)
                    .and_then(|e| e.icon_id)
                    .map(Icon::Id);
                (icon, None, env_name(catalog, t))
            }
            Row::Drone(i) => {
                let d = &projected.drones[i];
                (
                    Some(Icon::Type(d.type_id)),
                    None,
                    format!("{}x {}", d.count, catalog.name(d.type_id)),
                )
            }
            Row::Fighter(i) => {
                let f = &projected.fighters[i].fighter;
                let max = stats
                    .and_then(|s| s.projected_fighters.get(i))
                    .and_then(FighterInfo::max_size)
                    .or_else(|| catalog.projection.squadron_size(f.type_id));
                let amount = f.amount.or(max).unwrap_or(0);
                let name = match max {
                    Some(max) => format!("{amount}/{max} {}", catalog.name(f.type_id)),
                    None => catalog.name(f.type_id),
                };
                (Some(Icon::Type(f.type_id)), None, name)
            }
        };
        let state_mark = match row {
            Row::Module(i) => match projected.modules[i].state {
                ModuleState::Overload => ("⚡", self.palette.red),
                ModuleState::Active => ("✓", self.palette.green),
                ModuleState::Online => ("●", self.palette.muted),
                ModuleState::Offline => ("○", self.palette.muted),
            },
            _ if projected.is_active(row) => ("✓", self.palette.green),
            _ => ("○", self.palette.muted),
        };
        let selected = self.additions_selection.contains(&n);
        let range = range_text(projected.range(row));
        h_flex()
            .id(SharedString::from(format!("projected-{n}")))
            .h(px(ROW_HEIGHT))
            .px_1()
            .gap_1()
            .when(n % 2 == 1, |r| r.bg(theme.table_even))
            .when(selected, |r| r.bg(theme.table_active))
            .hover(|s| s.bg(theme.table_hover))
            .child(
                div()
                    .id(SharedString::from(format!("projected-state-{n}")))
                    .w(px(STATE_W))
                    .flex_none()
                    .text_color(color(state_mark.1))
                    .child(state_mark.0)
                    .on_click(cx.listener(move |g, event: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        g.projected_state_click(n, false, event.modifiers(), cx);
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |g, event: &MouseDownEvent, _, cx| {
                            // The state column opens no menu in pyfa; it overheats instead.
                            cx.stop_propagation();
                            g.projected_state_click(n, true, event.modifiers, cx);
                        }),
                    ),
            )
            .child(
                div()
                    .w(px(ICON_W))
                    .flex_none()
                    .children(charge.map(|c| icon_box(catalog, &loaded.icons, Icon::Type(c), 22.))),
            )
            .child(
                div()
                    .w(px(ICON_W))
                    .flex_none()
                    .children(icon_id.map(|i| icon_box(catalog, &loaded.icons, i, 22.))),
            )
            .child(div().flex_1().min_w_0().truncate().child(name))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .children(charge.map(|c| catalog.name(c))),
            )
            .child(div().w(px(RANGE_W)).flex_none().truncate().child(range))
            .on_click(cx.listener(move |g, event: &ClickEvent, window, cx| {
                window.focus(&g.additions_focus, cx);
                if event.click_count() < 2 {
                    g.select_addition(n, event.modifiers(), cx);
                    return;
                }
                let Some(loaded) = g.loaded() else { return };
                let alt = event.modifiers().alt;
                // pyfa: one copy or one drone at a time, Alt for all of it; modules and
                // fighters go whole, Alt taking the similar ones too.
                match row {
                    Row::Module(_) | Row::Fighter(_) => {
                        let reach = if alt {
                            Reach::Similar
                        } else {
                            Reach::Selection
                        };
                        let targets = if alt {
                            g.projected_targets(&loaded, n, reach)
                        } else {
                            vec![row]
                        };
                        g.remove_projected(&targets, true, cx);
                    }
                    Row::Env(_) => g.remove_projected(&[row], true, cx),
                    Row::Fit { .. } | Row::Drone(_) => g.remove_projected(&[row], alt, cx),
                }
            }))
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |g, _: &MouseDownEvent, _, cx| {
                    if !g.additions_selection.contains(&n) {
                        g.additions_selection = vec![n];
                        g.additions_anchor = Some(n);
                        cx.notify();
                    }
                }),
            )
            .context_menu({
                let (this, loaded) = (Self::weak(cx), Rc::clone(loaded));
                move |menu, window, cx| {
                    let menu = projected_item_menu(menu, &this, &loaded, n, row, window, cx);
                    projected_view_menu(menu.separator(), &this, &loaded, window, cx)
                }
            })
            .into_any_element()
    }

    /// pyfa's keys: Escape clears the selection, Ctrl+A selects everything, Delete removes the
    /// selected rows whole.
    fn projected_key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let plain = !keystroke.modifiers.modified();
        match keystroke.key.as_str() {
            "escape" if plain => {
                self.additions_selection.clear();
                self.additions_anchor = None;
                cx.notify();
            }
            "a" if keystroke.modifiers == Modifiers::control() => {
                let count = self.fit().map_or(0, |f| f.projected.len());
                self.additions_selection = (0..count).collect();
                cx.notify();
            }
            "delete" if plain && !self.additions_selection.is_empty() => {
                let Some(loaded) = self.loaded() else { return };
                let rows = self.projected_rows(&loaded);
                let targets: Vec<Row> = self
                    .additions_selection
                    .iter()
                    .filter_map(|&n| rows.get(n).copied())
                    .collect();
                self.remove_projected(&targets, true, cx);
            }
            _ => return,
        }
        cx.stop_propagation();
    }
}

/// Run `edit` on the targets of display row `n`, the reach picked by the modifiers held when the
/// entry is clicked.
fn on_targets<F>(
    this: &WeakEntity<Efa>,
    n: usize,
    default: Reach,
    edit: F,
) -> impl Fn(&ClickEvent, &mut Window, &mut App) + 'static + use<F>
where
    F: Fn(&mut Efa, Vec<Row>, &mut Context<Efa>) + 'static,
{
    let this = this.clone();
    move |_, window, cx| {
        let reach = match Reach::from_modifiers(window.modifiers()) {
            Reach::Similar => Reach::Similar,
            Reach::Selection => default,
        };
        Efa::app_update(&this, cx, |g, cx| {
            let Some(loaded) = g.loaded() else { return };
            let targets = g.projected_targets(&loaded, n, reach);
            edit(g, targets, cx);
        });
    }
}

/// pyfa's projectedModule, projectedDrone, projectedFighter and projectedFit entries.
#[expect(clippy::too_many_lines, reason = "four menus in one")]
fn projected_item_menu(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    loaded: &Rc<Loaded>,
    n: usize,
    row: Row,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let catalog = &loaded.catalog;
    let Some((projected, current, spool_max, fighter_info)) = this.upgrade().and_then(|g| {
        let g = g.read(cx);
        let fit = g.fit()?;
        let stats: Option<&Stats> = g.stats.as_ref();
        let spool = match row {
            Row::Module(i) => stats.and_then(|s| s.projected_spool.get(i).copied().flatten()),
            _ => None,
        };
        let info = match row {
            Row::Fighter(i) => stats.and_then(|s| s.projected_fighters.get(i).cloned()),
            _ => None,
        };
        Some((fit.projected.clone(), g.saved.current, spool, info))
    }) else {
        return menu;
    };
    let mut menu = menu;
    let type_id = match row {
        Row::Fit { .. } => None,
        Row::Module(i) => Some(projected.modules[i].type_id),
        Row::Env(i) => Some(projected.env[i].type_id),
        Row::Drone(i) => Some(projected.drones[i].type_id),
        Row::Fighter(i) => Some(projected.fighters[i].fighter.type_id),
    };

    if let Row::Fit { fit: index, .. } = row
        && current != Some(index)
    {
        let this = this.clone();
        menu = menu.item(
            entry(IconName::ExternalLink, "Open Fit in New Tab").on_click(move |_, _, cx| {
                Efa::app_update(&this, cx, |g, cx| g.open_fit(index, cx));
            }),
        );
    }
    if let Row::Module(i) = row {
        let module = &projected.modules[i];
        let charges = compatible_charges(catalog, module.type_id);
        if let Some(t) = catalog.get(module.type_id)
            && !charges.is_empty()
        {
            let structure = structure_charges(catalog, t, &charges);
            menu = charge_submenu(menu, this, loaded, n, structure, module.charge, window, cx);
        }
        if let Some(max) = spool_max {
            menu = spool_submenu(menu, this, n, module.spool, max, window, cx);
        }
    }
    if let Row::Fighter(i) = row
        && let Some(info) = fighter_info.filter(|info| !info.abilities.is_empty())
    {
        let names: Vec<String> = info
            .abilities
            .iter()
            .map(|a| catalog.projection.ability_name(a.id))
            .collect();
        let this = this.clone();
        menu = menu.submenu_with_element(
            "Abilities",
            icon_label(IconName::Settings2, "Abilities"),
            window,
            cx,
            move |menu, _, _| {
                info.abilities
                    .iter()
                    .zip(&names)
                    .fold(menu, |menu, (ability, name)| {
                        let id = ability.id;
                        let main_on = ability.active;
                        menu.item(
                            PopupMenuItem::new(name.clone())
                                .checked(ability.active)
                                .on_click(on_targets(
                                    &this,
                                    n,
                                    Reach::Selection,
                                    move |g, targets, cx| {
                                        let infos =
                                            g.stats.as_ref().map(|s| s.projected_fighters.clone());
                                        g.edit_projected(cx, |projected, _| {
                                            for target in
                                                targets.into_iter().chain([Row::Fighter(i)])
                                            {
                                                let Row::Fighter(j) = target else { continue };
                                                let has = infos
                                                    .as_ref()
                                                    .and_then(|s| s.get(j))
                                                    .is_some_and(|f| {
                                                        f.abilities.iter().any(|a| a.id == id)
                                                    });
                                                if has
                                                    && let Some(p) = projected.fighters.get_mut(j)
                                                {
                                                    p.fighter.set_ability(id, !main_on);
                                                }
                                            }
                                            false
                                        });
                                    },
                                )),
                        )
                    })
            },
        );
    }
    let stats_of = match row {
        Row::Fit { fit, .. } => Some(StatsOf::OtherFit(fit, FitItem::Ship)),
        _ => type_id.map(StatsOf::Type),
    };
    if let Some(of) = stats_of {
        menu = menu.item(stats_entry(this, "Projected Item", of));
    }
    if let Some(type_id) = type_id
        && catalog
            .get(type_id)
            .and_then(|t| t.market_group)
            .is_some_and(|g| catalog.market_groups.contains_key(&g))
    {
        let this = this.clone();
        menu = menu.item(
            entry(IconName::Search, "Projected Item Market Group").on_click(
                move |_, window, cx| {
                    Efa::app_update(&this, cx, |g, cx| g.jump_to_market(type_id, window, cx));
                },
            ),
        );
    }
    let remove_label = match row {
        Row::Drone(_) | Row::Fit { .. } => "Remove Projected Item Stack",
        _ => "Remove Projected Item",
    };
    menu = menu.item(entry(IconName::Delete, remove_label).on_click(on_targets(
        this,
        n,
        Reach::Selection,
        |g, targets, cx| g.remove_projected(&targets, true, cx),
    )));
    let amount = match row {
        Row::Fit { entry, .. } => Some(projected.fits[entry].amount),
        Row::Drone(i) => Some(projected.drones[i].count),
        Row::Fighter(i) => {
            let f = &projected.fighters[i].fighter;
            f.amount
                .or_else(|| catalog.projection.squadron_size(f.type_id))
        }
        Row::Module(_) | Row::Env(_) => None,
    };
    if let Some(amount) = amount {
        let this = this.clone();
        let is_fit = matches!(row, Row::Fit { .. });
        let prompt = if is_fit {
            "New Amount (0-20):"
        } else {
            "New Amount:"
        };
        menu = menu.item(
            entry(IconName::Settings2, "Change Projected Item Quantity").on_click(
                move |_, window, cx| {
                    Efa::app_update(&this, cx, |_, cx| {
                        Efa::open_input_dialog(
                            "Change Amount",
                            prompt,
                            amount.to_string(),
                            window,
                            cx,
                            move |g, amount: u32, cx| set_amount(g, row, amount, cx),
                        );
                    });
                },
            ),
        );
    }
    if !matches!(row, Row::Env(_)) {
        let current_km = projected
            .range(row)
            .map_or_default(|m| format_amount(m / 1000.0, 0, false));
        let this = this.clone();
        menu = menu.item(
            entry(IconName::Map, "Change Projected Item Range").on_click(move |_, window, cx| {
                let similar = window.modifiers().alt || window.modifiers().control;
                let current_km = current_km.clone();
                Efa::app_update(&this, cx, |_, cx| {
                    Efa::open_input_dialog(
                        "Change Projection Range",
                        "New Range, km:",
                        current_km,
                        window,
                        cx,
                        move |g, range: RangeInput, cx| {
                            let Some(loaded) = g.loaded() else { return };
                            let reach = if similar {
                                Reach::Similar
                            } else {
                                Reach::Selection
                            };
                            let targets = g.projected_targets(&loaded, n, reach);
                            g.edit_projected(cx, |projected, _| {
                                for target in targets {
                                    projected.set_range(target, range.0);
                                }
                                false
                            });
                        },
                    );
                });
            }),
        );
    }
    if let Some(type_id) = type_id.filter(|_| !matches!(row, Row::Env(_))) {
        let variations = catalog.variations_of(type_id);
        if variations.len() > 1 {
            let entries: Vec<(TypeId, String)> =
                variations.iter().map(|&v| (v, catalog.name(v))).collect();
            let (this, loaded) = (this.clone(), Rc::clone(loaded));
            menu = menu.submenu_with_element(
                "Variations",
                icon_label(IconName::Replace, "Variations"),
                window,
                cx,
                move |menu, _, _| {
                    entries.iter().fold(
                        menu.scrollable(true).max_h(px(MENU_MAX_H)),
                        |menu, (v, name)| {
                            let v = *v;
                            menu.item(
                                item_entry(&loaded, v, name.clone())
                                    .checked(v == type_id)
                                    .on_click(on_targets(
                                        &this,
                                        n,
                                        Reach::Selection,
                                        move |g, targets, cx| {
                                            g.edit_projected(cx, |projected, _| {
                                                swap_type(projected, &targets, type_id, v);
                                                false
                                            });
                                        },
                                    )),
                            )
                        },
                    )
                },
            );
        }
    }
    // pyfa's projectedCharge entries follow the module's own.
    if let Row::Module(i) = row
        && let Some(charge) = projected.modules[i].charge
    {
        menu = menu.item(stats_entry(this, "Charge", StatsOf::Type(charge)));
    }
    // pyfa's projectedCharge entries follow the module's own.
    if let Row::Module(i) = row
        && let Some(charge) = projected.modules[i].charge
        && catalog
            .get(charge)
            .and_then(|t| t.market_group)
            .is_some_and(|g| catalog.market_groups.contains_key(&g))
    {
        let this = this.clone();
        menu = menu.item(entry(IconName::Search, "Charge Market Group").on_click(
            move |_, window, cx| {
                Efa::app_update(&this, cx, |g, cx| g.jump_to_market(charge, window, cx));
            },
        ));
    }
    if let Row::Module(i) = row {
        let module = projected.modules[i].type_id;
        if !compatible_charges(catalog, module).is_empty() {
            let this = this.clone();
            menu = menu.item(
                entry(IconName::BatteryCharging, "Show Compatible Charges").on_click(
                    move |_, window, cx| {
                        Efa::app_update(&this, cx, |g, cx| g.show_charges_for(module, window, cx));
                    },
                ),
            );
        }
    }
    menu
}

/// Swap targets of the same type as the clicked one for a variation.
fn swap_type(projected: &mut Projected, targets: &[Row], from: TypeId, to: TypeId) {
    for &target in targets {
        let t = match target {
            Row::Module(i) => projected.modules.get_mut(i).map(|m| &mut m.type_id),
            Row::Drone(i) => projected.drones.get_mut(i).map(|d| &mut d.type_id),
            Row::Fighter(i) => projected
                .fighters
                .get_mut(i)
                .map(|f| &mut f.fighter.type_id),
            Row::Fit { .. } | Row::Env(_) => None,
        };
        if let Some(t) = t.filter(|t| **t == from) {
            *t = to;
        }
    }
    // A swapped module may not take its charge any more; the engine just ignores it then.
}

/// pyfa's amount changes: fits between 1 and 20 (0 removes), drones and fighters at least one
/// (0 removes).
fn set_amount(g: &mut Efa, row: Row, amount: u32, cx: &mut Context<Efa>) {
    let max = match row {
        Row::Fighter(i) => g
            .stats
            .as_ref()
            .and_then(|s| s.projected_fighters.get(i))
            .and_then(FighterInfo::max_size),
        _ => None,
    };
    g.edit_projected(cx, |projected, catalog| {
        if amount == 0 {
            projected.remove(&[row], true);
            return true;
        }
        match row {
            Row::Fit { entry, .. } => {
                if let Some(p) = projected.fits.get_mut(entry) {
                    p.amount = amount.min(MAX_FIT_COPIES);
                }
            }
            Row::Drone(i) => {
                if let Some(d) = projected.drones.get_mut(i) {
                    d.count = amount;
                }
            }
            Row::Fighter(i) => {
                if let Some(p) = projected.fighters.get_mut(i) {
                    let max = max.or_else(|| catalog.projection.squadron_size(p.fighter.type_id));
                    p.fighter.set_amount(amount, max);
                }
            }
            Row::Module(_) | Row::Env(_) => {}
        }
        false
    });
}

/// The projected module's Charge menu, laid out like the fitting view's.
#[expect(
    clippy::too_many_arguments,
    reason = "the submenu needs the row's whole context"
)]
fn charge_submenu(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    loaded: &Rc<Loaded>,
    n: usize,
    structure: ChargeMenu,
    current: Option<TypeId>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let (this, loaded) = (this.clone(), Rc::clone(loaded));
    let set = move |this: &WeakEntity<Efa>, charge: Option<TypeId>| {
        on_targets(this, n, Reach::Similar, move |g, targets, cx| {
            g.edit_projected(cx, |projected, catalog| {
                for target in &targets {
                    let Row::Module(index) = *target else {
                        continue;
                    };
                    let Some(module) = projected.modules.get_mut(index) else {
                        continue;
                    };
                    let fits = charge.is_none_or(|charge| {
                        let item = catalog.get(charge);
                        let holder = catalog.get(module.type_id);
                        item.zip(holder).is_some_and(|(c, m)| c.fits_in(m))
                    });
                    if fits {
                        module.charge = charge;
                    }
                }
                false
            });
        })
    };
    let charge_entry = move |this: &WeakEntity<Efa>, loaded: &Rc<Loaded>, id: TypeId| {
        item_entry(loaded, id, loaded.catalog.name(id))
            .checked(current == Some(id))
            .on_click(set(this, Some(id)))
    };
    menu.submenu_with_element(
        "Charge",
        icon_label(IconName::BatteryCharging, "Charge"),
        window,
        cx,
        move |menu, window, cx| {
            let catalog = &loaded.catalog;
            let mut menu = menu.scrollable(true).max_h(px(MENU_MAX_H));
            let ladder = |menu: PopupMenu,
                          title: String,
                          charges: Vec<TypeId>,
                          window: &mut Window,
                          cx: &mut Context<PopupMenu>| {
                let (this, loaded) = (this.clone(), Rc::clone(&loaded));
                menu.submenu(title, window, cx, move |menu, _, _| {
                    charges
                        .iter()
                        .fold(menu.label("─ Less Damage ─"), |menu, &c| {
                            menu.item(charge_entry(&this, &loaded, c))
                        })
                        .label("─ More Damage ─")
                })
            };
            match &structure {
                ChargeMenu::Turret(families) => {
                    menu = menu.label("─ Long Range ─");
                    for family in families {
                        menu = match family.as_slice() {
                            [only] => menu.item(charge_entry(&this, &loaded, *only)),
                            _ => ladder(menu, catalog.name(family[0]), family.clone(), window, cx),
                        };
                    }
                    menu = menu.label("─ Short Range ─");
                }
                ChargeMenu::Missile(by_type) => {
                    for (label, charges) in by_type {
                        menu = ladder(menu, (*label).to_owned(), charges.clone(), window, cx);
                    }
                }
                ChargeMenu::Flat(list) => {
                    for &c in list {
                        menu = menu.item(charge_entry(&this, &loaded, c));
                    }
                }
            }
            menu.item(entry(IconName::CircleX, "Empty").on_click(set(&this, None)))
        },
    )
}

fn spool_submenu(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    n: usize,
    current: Option<u32>,
    max: u32,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let this = this.clone();
    let set = move |this: &WeakEntity<Efa>, cycles: Option<u32>| {
        on_targets(this, n, Reach::Selection, move |g, targets, cx| {
            g.edit_projected(cx, |projected, _| {
                for target in &targets {
                    if let Row::Module(i) = *target
                        && let Some(m) = projected.modules.get_mut(i)
                    {
                        m.spool = cycles;
                    }
                }
                false
            });
        })
    };
    menu.submenu_with_element(
        "Spoolup Cycles",
        icon_label(IconName::RefreshCw, "Spoolup Cycles"),
        window,
        cx,
        move |menu, _, _| {
            let menu = (0..=max).fold(
                menu.scrollable(true).max_h(px(MENU_MAX_H)),
                |menu, cycles| {
                    let label = if cycles == max {
                        format!("{cycles} (default)")
                    } else {
                        cycles.to_string()
                    };
                    menu.item(
                        PopupMenuItem::new(label)
                            .checked(current.unwrap_or(max) == cycles)
                            .on_click(set(&this, Some(cycles))),
                    )
                },
            );
            menu.separator()
                .item(entry(IconName::Undo2, "Reset").on_click(set(&this, None)))
        },
    )
}

/// pyfa's projected entries for the whole pane: add fits, and environmental effects.
fn projected_view_menu(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    loaded: &Rc<Loaded>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let menu = add_fit_entries(menu, this, loaded, Link::Projected, window, cx);
    let env = Rc::new(loaded.catalog.projection.env_menu.clone());
    let this = this.clone();
    menu.submenu_with_element(
        "Add Environmental Effect",
        icon_label(IconName::Globe, "Add Environmental Effect"),
        window,
        cx,
        move |menu, window, cx| env_level(menu, &this, &env, window, cx),
    )
}

/// One level of the environmental effects tree: its submenus, then its effects.
fn env_level(
    menu: PopupMenu,
    this: &WeakEntity<Efa>,
    level: &Rc<EnvMenu>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let mut menu = menu.scrollable(true).max_h(px(MENU_MAX_H));
    for (name, group) in &level.groups {
        let (this, group) = (this.clone(), Rc::new(group.clone()));
        menu = menu.submenu(name.clone(), window, cx, move |menu, window, cx| {
            env_level(menu, &this, &group, window, cx)
        });
    }
    for item in &level.items {
        let (this, type_id) = (this.clone(), item.type_id);
        menu = menu.item(
            PopupMenuItem::new(item.label.clone()).on_click(move |_, _, cx| {
                Efa::app_update(&this, cx, |g, cx| {
                    g.edit_projected(cx, |projected, _| {
                        projected.add_env(type_id);
                        true
                    });
                });
            }),
        );
    }
    menu
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fit::projected::{EnvEffect, ProjectedDrone, ProjectedFit};

    #[test]
    fn ranges_read_in_km() {
        assert_eq!(range_text(None), "");
        assert_eq!(range_text(Some(500.0)), "500 m");
        assert_eq!(range_text(Some(12_500.0)), "12.5 km");
        let parsed: RangeInput = "12.5 km".parse().expect("parses");
        assert_eq!(parsed.0, Some(12_500.0));
        let empty: RangeInput = "".parse().expect("parses");
        assert_eq!(empty.0, None);
    }

    #[test]
    fn double_click_takes_one_copy_or_one_drone() {
        let mut projected = Projected {
            fits: vec![ProjectedFit {
                fit: 1,
                amount: 2,
                active: true,
                range: None,
            }],
            drones: vec![ProjectedDrone {
                type_id: 5,
                count: 1,
                active: true,
                range: None,
            }],
            env: vec![EnvEffect {
                type_id: 9,
                active: true,
            }],
            ..Projected::default()
        };
        projected.remove(&[Row::Fit { entry: 0, fit: 0 }, Row::Drone(0)], false);
        assert_eq!(projected.fits[0].amount, 1);
        assert!(projected.drones.is_empty(), "the last drone took its stack");
        projected.remove(&[Row::Fit { entry: 0, fit: 0 }, Row::Env(0)], true);
        assert!(projected.is_empty());
    }
}
