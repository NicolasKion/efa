//! The Affected By tab: what changes the item's attributes, listed by the item doing it or by
//! the attribute changed (gui/builtinItemStatsViews/itemAffectedBy.py).

use std::{
    collections::{BTreeMap, HashSet},
    fmt::Write as _,
};

use crate::{
    data::Catalog,
    engine::{Inspection, ModOp, Modifier, Source, SourceKind},
    icons::Icon,
    item_data::ItemData,
};

use super::{Target, tree::Node};

/// Icon pyfa shows for attributes without one of their own.
const UNKNOWN_ICON: i32 = 0;

/// How the tree is laid out and labelled.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct View {
    /// Attributes at the top with what changes them below, instead of the other way round.
    pub by_attribute: bool,
    /// Internal attribute names instead of the game's.
    pub real_names: bool,
}

/// The word pyfa uses for a kind of item, as in "{Module} Stats".
pub fn kind_label(kind: SourceKind) -> &'static str {
    match kind {
        SourceKind::Ship => "Ship",
        SourceKind::Mode => "Mode",
        SourceKind::Module => "Module",
        SourceKind::Drone => "Drone",
        SourceKind::Fighter => "Fighter",
        SourceKind::Implant => "Implant",
        SourceKind::Booster => "Booster",
        SourceKind::Skill => "Skill",
        SourceKind::Other => "Item",
    }
}

/// pyfa's operator text: `s` for stacking penalized, `r` for resisted, then the operator.
fn operator(m: &Modifier) -> String {
    let symbol = match m.op {
        ModOp::Assign => "=",
        ModOp::Add => "+",
        ModOp::Multiply => "*",
        ModOp::Force => "\u{2263}",
        ModOp::Min => "min",
        ModOp::Max => "max",
    };
    let mut prefix = String::new();
    if m.penalized {
        prefix.push('s');
    }
    if m.resisted {
        prefix.push('r');
    }
    format!("{prefix}{symbol}")
}

/// The change as pyfa prints it: "* 1.10 (penalized)", "- 5.00".
fn change(op: &str, amount: f64) -> String {
    let mut note = String::new();
    let op = if op.contains('*') {
        if op.contains('s') {
            note.push_str("(penalized)");
        }
        if op.contains('r') {
            note.push_str("(resisted)");
        }
        "*"
    } else {
        op
    };
    let (op, amount) = if op == "+" && amount < 0.0 {
        ("-", -amount)
    } else {
        (op, amount)
    };
    format!("{op} {amount:.2} {note}")
}

/// Everything needed to name things in the tree.
pub struct Names<'a> {
    pub catalog: &'a Catalog,
    pub data: &'a ItemData,
}

impl Names<'_> {
    fn attr_icon(&self, attr: i32) -> Option<Icon> {
        match self.data.attr(attr) {
            Some(info) => info.icon.map(Icon::Id),
            None => Some(Icon::Id(UNKNOWN_ICON)),
        }
    }

    fn attr_name(&self, attr: i32) -> String {
        self.data
            .attr(attr)
            .map_or_else(|| attr.to_string(), |a| a.name.clone())
    }

    /// The game's name, falling back to the internal one.
    fn attr_display(&self, attr: i32) -> String {
        self.data
            .attr(attr)
            .and_then(|a| a.display_name.clone())
            .unwrap_or_else(|| self.attr_name(attr))
    }

    fn link(source: &Source, fit: Option<usize>) -> (String, Target) {
        let target = match (source.kind, source.item, fit) {
            // pyfa opens skills as plain items, and so does anything without a place in the fit.
            (SourceKind::Skill, _, _) | (_, None, _) | (_, _, None) => Target::Type(source.type_id),
            (_, Some(item), Some(fit)) => Target::Fitted { fit, item },
        };
        (kind_label(source.kind).to_owned(), target)
    }
}

/// Modifications of attributes the item actually has changed, skipping unchanged ones as pyfa
/// does.
#[expect(
    clippy::float_cmp,
    reason = "pyfa hides exactly the attributes that come out unchanged"
)]
fn changed(inspection: &Inspection) -> impl Iterator<Item = (i32, &[Modifier])> {
    inspection.modifiers.iter().filter_map(|(attr, mods)| {
        let value = inspection.attr(*attr)?;
        (value.modified != value.base).then_some((*attr, mods.as_slice()))
    })
}

fn source_name(names: &Names<'_>, source: &Source) -> String {
    names.catalog.name(source.type_id)
}

/// The tree for `view`. `fit` is the fit the item belongs to, for opening other items' stats.
pub fn tree(
    names: &Names<'_>,
    inspection: &Inspection,
    view: View,
    fit: Option<usize>,
) -> Vec<Node> {
    if view.by_attribute {
        by_attribute(names, inspection, view, fit)
    } else {
        by_item(names, inspection, view, fit)
    }
}

/// One entry per affecting item, with the attributes it changes below.
fn by_item(
    names: &Names<'_>,
    inspection: &Inspection,
    view: View,
    fit: Option<usize>,
) -> Vec<Node> {
    struct Entry {
        kind: SourceKind,
        sources: Vec<Source>,
        changes: Vec<(i32, String, f64)>,
        type_id: i32,
        projected: bool,
    }
    // Keyed by name, so two copies of a module show as one line with a count.
    let mut entries: BTreeMap<String, Entry> = BTreeMap::new();
    for (attr, mods) in changed(inspection) {
        for m in mods {
            let name = source_name(names, &m.source);
            let entry = entries.entry(name).or_insert_with(|| Entry {
                kind: m.source.kind,
                sources: Vec::new(),
                changes: Vec::new(),
                type_id: m.source.type_id,
                projected: m.source.projected,
            });
            if !entry.sources.iter().any(|s| s.item_id == m.source.item_id) {
                entry.sources.push(m.source);
            }
            let op = operator(m);
            // With several copies, each one's identical change is listed once.
            #[expect(
                clippy::float_cmp,
                reason = "only exact duplicates of a change are left out, as in pyfa"
            )]
            let same = |c: &(i32, String, f64)| c.0 == attr && c.1 == op && c.2 == m.amount;
            if entry.sources.len() > 1 && entry.changes.iter().any(same) {
                continue;
            }
            entry.changes.push((attr, op, m.amount));
        }
    }
    let mut order: Vec<(String, Entry)> = entries.into_iter().collect();
    order.sort_by(|(a, x), (b, y)| x.kind.cmp(&y.kind).then_with(|| a.cmp(b)));
    order
        .into_iter()
        .map(|(name, entry)| {
            let mut label = name.clone();
            if entry.sources.len() > 1 {
                let _ = write!(label, " x {}", entry.sources.len());
            }
            if entry.projected {
                label.push_str(" (projected)");
            }
            let mut changes = entry.changes;
            changes.sort_by_key(|(attr, _, _)| names.attr_name(*attr));
            let children = changes
                .iter()
                .map(|(attr, op, amount)| {
                    let attr_label = if view.real_names {
                        names.attr_name(*attr)
                    } else {
                        names.attr_display(*attr)
                    };
                    Node::new(
                        format!("m:{name}/{attr}/{op}/{amount}"),
                        format!("{attr_label} {}", change(op, *amount)),
                    )
                    .icon(names.attr_icon(*attr))
                })
                .collect();
            let icon = Some(Icon::Type(entry.type_id));
            let link = entry.sources.last().map(|s| Names::link(s, fit));
            Node {
                key: format!("m:{name}"),
                icon,
                label,
                cells: Vec::new(),
                children,
                link,
            }
        })
        .collect()
}

/// One entry per changed attribute, with what changes it below.
fn by_attribute(
    names: &Names<'_>,
    inspection: &Inspection,
    view: View,
    fit: Option<usize>,
) -> Vec<Node> {
    let mut attrs: Vec<(i32, &[Modifier])> = changed(inspection).collect();
    attrs.sort_by_key(|(attr, _)| names.attr_display(*attr));
    let mut seen = HashSet::new();
    attrs
        .into_iter()
        .filter(|(attr, _)| seen.insert(*attr))
        .map(|(attr, mods)| {
            let label = if view.real_names {
                names.attr_name(attr)
            } else {
                names.attr_display(attr)
            };
            let mut mods: Vec<&Modifier> = mods.iter().collect();
            mods.sort_by_key(|m| m.source.kind);
            let children = mods
                .iter()
                .enumerate()
                .map(|(n, m)| {
                    let mut name = source_name(names, &m.source);
                    if m.source.projected {
                        name.push_str(" (projected)");
                    }
                    Node {
                        key: format!("a:{attr}/{n}"),
                        icon: Some(Icon::Type(m.source.type_id)),
                        label: format!("{name} {}", change(&operator(m), m.amount)),
                        cells: Vec::new(),
                        children: Vec::new(),
                        link: Some(Names::link(&m.source, fit)),
                    }
                })
                .collect();
            Node {
                key: format!("a:{attr}"),
                icon: names.attr_icon(attr),
                label,
                cells: Vec::new(),
                children,
                link: None,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn changes_read_like_pyfa() {
        assert_eq!(change("s*", 1.1), "* 1.10 (penalized)");
        assert_eq!(change("sr*", 0.5), "* 0.50 (penalized)(resisted)");
        assert_eq!(change("+", -5.0), "- 5.00 ");
        assert_eq!(change("=", 3.0), "= 3.00 ");
    }
}
