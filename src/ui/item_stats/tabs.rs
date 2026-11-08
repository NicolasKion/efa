//! The stats window's tabs: attributes, requirements, dependents, effects, affected by and
//! properties, and the trees they show.

use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme, Selectable, Sizable,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use gpui_kit::{
    AnyElement, App, ClickEvent, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, prelude::FluentBuilder, px,
};

use super::tree::Node;
use super::{ItemStats, ROW_H, Tab, Target, TreeId, VALUE_W};
use super::{affected, attributes, format};
use crate::ui::widgets::{empty_line, table_head};
use crate::{
    data::TypeId,
    engine::FitItem,
    fit::Fit,
    icons::Icon,
    item_data::{ItemData, TypeInfo},
    text::roman,
};

impl ItemStats {
    /// The item's own attributes, merged with its base item's when mutated, and their values.
    pub(super) fn attribute_values(
        &self,
        data: &ItemData,
    ) -> (Option<TypeInfo>, Vec<attributes::Value>) {
        let merged = self
            .mutated_info(data)
            .or_else(|| data.type_info(self.type_id).cloned());
        let values = self.values(merged.as_ref());
        (merged, values)
    }

    fn attribute_nodes(&self, data: &ItemData) -> Vec<Node> {
        let catalog = &self.loaded.catalog;
        let (info, values) = self.attribute_values(data);
        let overridden = if self.fitted() {
            self.loaded.engine.overridden_attrs(self.type_id)
        } else {
            Vec::new()
        };
        let describe = attributes::Describe {
            data,
            catalog,
            own: info.as_ref(),
            fitted: self.fitted(),
            overridden: &overridden,
            raw: self.raw,
        };
        let category = catalog.get(self.type_id).map_or(0, |t| t.category_id);
        attributes::tree(&describe, category, &values)
    }

    /// The tree `tab` shows, built once for what it depends on.
    fn tree(&mut self, tab: Tab, data: &ItemData) -> Rc<[Node]> {
        let key = (tab, self.raw, self.affected_view);
        if let Some((cached, nodes)) = &self.tree_cache
            && *cached == key
        {
            return Rc::clone(nodes);
        }
        let catalog = &self.loaded.catalog;
        let nodes: Rc<[Node]> = match tab {
            Tab::Attributes => self.attribute_nodes(data),
            Tab::Requirements => requirements(catalog, data, self.type_id),
            Tab::Dependents => dependents(catalog, self.type_id),
            Tab::AffectedBy => self.inspection.as_ref().map_or_default(|inspection| {
                let names = affected::Names { catalog, data };
                affected::tree(&names, inspection, self.affected_view, self.fit_index())
            }),
            _ => Vec::new(),
        }
        .into();
        self.tree_cache = Some((key, Rc::clone(&nodes)));
        nodes
    }

    pub(super) fn attributes_tab(&mut self, data: &ItemData, cx: &mut Context<Self>) -> AnyElement {
        let nodes = self.tree(Tab::Attributes, data);
        let head = table_head(cx)
            .child(div().flex_1().child("Attribute"))
            .child(div().flex_none().w(px(VALUE_W)).child("Current Value"))
            .when(self.fitted(), |h| {
                h.child(div().flex_none().w(px(VALUE_W)).child("Base Value"))
            });
        v_flex()
            .flex_1()
            .min_h_0()
            .child(head)
            .child(self.tree_view(TreeId::Attributes, &nodes, cx))
            .child(
                Self::button_row(cx)
                    .child(
                        Button::new("raw")
                            .small()
                            .ghost()
                            .label("View Raw Data")
                            .selected(self.raw)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.raw = !this.raw;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("export")
                            .small()
                            .ghost()
                            .label("Export Item Stats")
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| this.export(cx))),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn requirements_tab(
        &mut self,
        data: &ItemData,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let nodes = self.tree(Tab::Requirements, data);
        if nodes.is_empty() {
            return Self::empty("No skills needed.", cx);
        }
        self.tree_view(TreeId::Requirements, &nodes, cx)
            .into_any_element()
    }

    pub(super) fn dependents_tab(&mut self, data: &ItemData, cx: &mut Context<Self>) -> AnyElement {
        let nodes = self.tree(Tab::Dependents, data);
        if nodes.is_empty() {
            return Self::empty("Nothing needs this skill.", cx);
        }
        self.tree_view(TreeId::Dependents, &nodes, cx)
            .into_any_element()
    }

    pub(super) fn effects_tab(&self, data: &ItemData, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let info = data.type_info(self.type_id);
        let mut effects: Vec<(String, String, String)> = info
            .map_or(&[][..], |i| i.effects.as_slice())
            .iter()
            .map(|&(id, _)| {
                let effect = data.effects.get(&id);
                let name = effect.map_or_else(|| id.to_string(), |e| e.name.clone());
                let active = self
                    .inspection
                    .as_ref()
                    .and_then(|i| i.effects.iter().find(|&&(e, _)| e == id))
                    .map_or("", |&(_, running)| if running { "Yes" } else { "No" });
                let kind = effect.map_or("", |e| effect_type(e.category));
                (name, active.to_owned(), kind.to_owned())
            })
            .collect();
        effects.sort();
        let cell = |w: Option<f32>, text: String| match w {
            Some(w) => div().flex_none().w(px(w)).truncate().child(text),
            None => div().flex_1().min_w_0().truncate().child(text),
        };
        v_flex()
            .id("effects")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .child(
                table_head(cx)
                    .child(cell(None, "Name".into()))
                    .child(cell(Some(60.), "Active".into()))
                    .child(cell(Some(120.), "Type".into())),
            )
            .children(
                effects
                    .into_iter()
                    .enumerate()
                    .map(|(n, (name, active, kind))| {
                        h_flex()
                            .h(px(ROW_H))
                            .px_1()
                            .when(n % 2 == 1, |r| r.bg(theme.table_even))
                            .child(cell(None, name))
                            .child(cell(Some(60.), active))
                            .child(cell(Some(120.), kind))
                    }),
            )
            .into_any_element()
    }

    pub(super) fn affected_tab(&mut self, data: &ItemData, cx: &mut Context<Self>) -> AnyElement {
        if self.inspection.is_none() {
            return Self::empty("Not part of a fit.", cx);
        }
        let nodes = self.tree(Tab::AffectedBy, data);
        let view = self.affected_view;
        let expanded = self.affected_open.default_open;
        v_flex()
            .flex_1()
            .min_h_0()
            .child(if nodes.is_empty() {
                Self::empty("Nothing changes this item.", cx)
            } else {
                self.tree_view(TreeId::AffectedBy, &nodes, cx)
                    .into_any_element()
            })
            .child(
                Self::button_row(cx)
                    .child(
                        Button::new("expand")
                            .small()
                            .ghost()
                            .label("Expand All")
                            .selected(expanded)
                            .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                                this.affected_open.set_all(!expanded);
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("names")
                            .small()
                            .ghost()
                            .label("Toggle Names")
                            .selected(view.real_names)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.affected_view.real_names = !this.affected_view.real_names;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("view")
                            .small()
                            .ghost()
                            .label("Toggle View")
                            .selected(view.by_attribute)
                            .on_click(cx.listener(|this, _: &ClickEvent, _, cx| {
                                this.affected_view.by_attribute = !this.affected_view.by_attribute;
                                // The tree is rebuilt, so it starts out as Expand All says.
                                let open = this.affected_open.default_open;
                                this.affected_open.set_all(open);
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    pub(super) fn properties_tab(&self, data: &ItemData, cx: &mut Context<Self>) -> AnyElement {
        let rows = properties(
            &self.loaded.catalog,
            data,
            self.type_id,
            self.fit.as_ref(),
            self.target,
        );
        let theme = cx.theme();
        let count = rows.len();
        v_flex()
            .flex_1()
            .min_h_0()
            .child(
                v_flex()
                    .id("properties")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(rows.into_iter().enumerate().map(|(n, (name, value))| {
                        h_flex()
                            .h(px(ROW_H))
                            .px_1()
                            .gap_2()
                            .when(n % 2 == 1, |r| r.bg(theme.table_even))
                            .child(div().flex_none().w(px(160.)).truncate().child(name))
                            .child(div().flex_1().min_w_0().truncate().child(value))
                    })),
            )
            .child(Self::button_row(cx).child(format!("{count} attributes.")))
            .into_any_element()
    }

    pub(super) fn empty(text: &'static str, cx: &App) -> AnyElement {
        empty_line(text, cx).into_any_element()
    }
}

/// pyfa's requirements tree: each skill with what it needs in turn. A skill's own needs are
/// spelled out once; later mentions stand alone.
pub(super) fn requirements(
    catalog: &crate::data::Catalog,
    data: &ItemData,
    type_id: TypeId,
) -> Vec<Node> {
    fn walk(
        catalog: &crate::data::Catalog,
        data: &ItemData,
        type_id: TypeId,
        path: &str,
        seen: &mut Vec<TypeId>,
    ) -> Vec<Node> {
        let needs = data
            .type_info(type_id)
            .map_or_default(TypeInfo::required_skills);
        needs
            .into_iter()
            .map(|(skill, level)| {
                let key = format!("{path}/{skill}");
                let mut node = Node::new(
                    key.clone(),
                    format!("{}  {}", catalog.name(skill), roman(level)),
                )
                .icon(Some(Icon::Type(skill)));
                if !seen.contains(&skill) {
                    node.children = walk(catalog, data, skill, &key, seen);
                    seen.push(skill);
                }
                node
            })
            .collect()
    }
    walk(catalog, data, type_id, "", &mut Vec::new())
}

/// pyfa's dependents tree: what a skill unlocks, by the level it needs.
pub(super) fn dependents(catalog: &crate::data::Catalog, skill: TypeId) -> Vec<Node> {
    let users = catalog.required_for(skill);
    let mut by_level: std::collections::BTreeMap<u8, Vec<TypeId>> =
        std::collections::BTreeMap::new();
    for &(id, level) in users {
        if catalog.get(id).is_some_and(|t| t.published) {
            by_level.entry(level).or_default().push(id);
        }
    }
    by_level
        .into_iter()
        .map(|(level, mut ids)| {
            ids.sort_by_key(|&id| catalog.name(id));
            let mut node = Node::new(format!("l{level}"), format!("Level {}", roman(level)))
                .icon(Some(Icon::Type(skill)));
            node.children = ids
                .into_iter()
                .map(|id| {
                    Node::new(format!("l{level}/{id}"), catalog.name(id)).icon(Some(Icon::Type(id)))
                })
                .collect();
            node
        })
        .collect()
}

/// What pyfa calls an effect's type, from its dogma category.
fn effect_type(category: i32) -> &'static str {
    match category {
        0 => "passive",
        1 => "active",
        2 => "active projected",
        3 => "area",
        4 => "online",
        5 => "overheat",
        6 => "dungeon",
        7 => "system",
        _ => "",
    }
}

/// The type's raw facts, sorted by name, like pyfa's debug Properties tab.
fn properties(
    catalog: &crate::data::Catalog,
    data: &ItemData,
    type_id: TypeId,
    fit: Option<&Fit>,
    target: Target,
) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = vec![("Id".into(), type_id.to_string())];
    let named = |id: Option<i32>, names: &std::collections::HashMap<i32, String>| {
        id.map(|i| {
            names
                .get(&i)
                .map_or_else(|| i.to_string(), |n| format!("{n} ({i})"))
        })
    };
    if let Some(t) = catalog.get(type_id) {
        rows.push(("Name".into(), t.name.clone()));
        rows.push((
            "Category".into(),
            format!("{} ({})", t.category_label(), t.category_id),
        ));
        if let Some(group) = named(Some(t.group_id), &catalog.group_names) {
            rows.push(("Group".into(), group));
        }
        if let Some(market) = t.market_group {
            let name = catalog
                .market_groups
                .get(&market)
                .map_or_else(|| market.to_string(), |g| format!("{} ({market})", g.name));
            rows.push(("Market Group".into(), name));
        }
        if let Some(meta) = named(t.meta_group, &catalog.meta_group_names) {
            rows.push(("Meta Group".into(), meta));
        }
        rows.push(("Meta Level".into(), t.meta_level.to_string()));
        rows.push(("Published".into(), t.published.to_string()));
        rows.push(("Volume".into(), format::raw(t.volume)));
        rows.push(("Capacity".into(), format::raw(t.capacity)));
        if let Some(parent) = t.variation_parent {
            rows.push((
                "Variation Parent".into(),
                format!("{} ({parent})", catalog.name(parent)),
            ));
        }
        if let Some(icon) = &t.icon {
            rows.push(("Icon".into(), icon.clone()));
        }
    }
    if let Some(info) = data.type_info(type_id) {
        let mut push = |name: &str, value: Option<String>| {
            if let Some(value) = value {
                rows.push((name.to_owned(), value));
            }
        };
        push("Base Price", info.base_price.map(format::raw));
        push("Portion Size", info.portion_size.map(|v| v.to_string()));
        push("Tech Level", info.tech_level.map(|v| v.to_string()));
        push("Race Id", info.race_id.map(|v| v.to_string()));
        push("Faction Id", info.faction_id.map(|v| v.to_string()));
        push("Effects", Some(info.effects.len().to_string()));
        push("Attributes", Some(info.attrs.len().to_string()));
    }
    if let (Target::Fitted { item, .. }, Some(fit)) = (target, fit) {
        rows.push(("Fit".into(), fit.name.clone()));
        rows.push(("Position".into(), format!("{item:?}")));
        if let FitItem::Module(slot, i) = item
            && let Some(Some(m)) = fit.rack(slot).get(i)
        {
            rows.push(("State".into(), m.state.label().to_owned()));
            if let Some(charge) = m.charge {
                rows.push(("Charge".into(), catalog.name(charge)));
            }
        }
        if let FitItem::Drone(i) = item
            && let Some(stack) = fit.drones.get(i)
        {
            rows.push(("Amount".into(), stack.count.to_string()));
            rows.push(("Amount Active".into(), stack.active.to_string()));
        }
    }
    rows.sort();
    rows
}
