//! The Projected tab, pyfa's projected pane: fits, modules, drones and fighters projecting onto
//! the active fit, and environmental effects, with pyfa's state clicks, ranges, amounts, menus
//! and drops.

mod menu;
mod model;

use std::rc::Rc;

use gpui_kit::component::{ActiveTheme, h_flex, menu::ContextMenuExt, v_flex};
use gpui_kit::{
    AnyElement, ClickEvent, Context, InteractiveElement, IntoElement, KeyDownEvent, Modifiers,
    MouseButton, MouseDownEvent, ParentElement, SharedString, StatefulInteractiveElement, Styled,
    div, prelude::FluentBuilder, px,
};

use super::{FitDrag, ROW_HEIGHT, linked::Link};
use crate::ui::widgets::{DropHighlight, empty_line};
use crate::{
    data::{Catalog, Kind, TypeId},
    engine::FighterInfo,
    fit::{ModuleState, projected::Projected},
    icons::Icon,
    ui::{
        Efa, Loaded, RowRef,
        columns::{ICON_W, STATE_W},
        drag::{DragSource, Dragged},
        widgets::{color, icon, icon_box},
    },
};
use menu::{projected_item_menu, projected_view_menu};
use model::{Proposal, Reach, Row, env_name, range_text, rows};

const RANGE_W: f32 = 70.;
const HEADER_ICON: f32 = 18.;

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
            self.set_error(format!("{} cannot be projected", t.name));
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
        let mut content: Vec<AnyElement> = Vec::with_capacity(rows.len() + 1);
        if rows.is_empty() {
            content.push(
                empty_line(
                    "Drag an item or fit, or use right-click menu for wormhole effects",
                    cx,
                )
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
                    .drop_highlight::<Dragged>()
                    .on_drop(cx.listener(|g, dragged: &Dragged, _, cx| {
                        g.drop_on_projected(dragged, cx);
                    }))
                    .drop_highlight::<FitDrag>()
                    .on_drop(cx.listener(|g, drag: &FitDrag, _, cx| {
                        g.link(Link::Projected, &[drag.index], cx);
                    }))
                    .context_menu({
                        let (this, loaded) = (cx.weak_entity(), Rc::clone(loaded));
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
                    if g.additions_selection.right_click(n) {
                        cx.notify();
                    }
                }),
            )
            .context_menu({
                let (this, loaded) = (cx.weak_entity(), Rc::clone(loaded));
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
        let count = self.fit().map_or(0, |f| f.projected.len());
        self.list_keys(
            event,
            count,
            true,
            |g, places, cx| {
                let Some(loaded) = g.loaded() else { return };
                let rows = g.projected_rows(&loaded);
                let targets: Vec<Row> = places
                    .iter()
                    .filter_map(|&n| rows.get(n).copied())
                    .collect();
                g.remove_projected(&targets, true, cx);
            },
            cx,
        );
    }
}
