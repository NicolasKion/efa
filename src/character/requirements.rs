//! Skills a fit needs that its character lacks, pyfa's checkRequirements and the ways
//! gui/characterSelection.py lists them.

use std::{collections::BTreeMap, fmt::Write};

use crate::{
    data::{Catalog, Kind, Slot, TypeId},
    fit::Fit,
};

use super::roman;
use crate::port::escape_markup as escape;

/// A skill the character has below what is needed, with its own missing prerequisites.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Requirement {
    pub skill: TypeId,
    pub level: u8,
    pub prerequisites: Vec<Requirement>,
}

/// An item of the fit and the skills it is missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemRequirements {
    pub item: TypeId,
    pub skills: Vec<Requirement>,
}

/// What the fit is missing, item by item. `level` is the character's level of a skill, 0 when
/// not learned; `implants` are the character's, for fits that use them. Rigs need no skill to
/// fit and fighters' bombs are never used, so neither counts, as in pyfa. An item that appears
/// more than once is listed once.
pub fn missing(
    fit: &Fit,
    catalog: &Catalog,
    implants: &[TypeId],
    level: impl Fn(TypeId) -> u8,
) -> Vec<ItemRequirements> {
    let mut items: Vec<TypeId> = Vec::new();
    let racks = [Slot::High, Slot::Mid, Slot::Low].map(|slot| fit.rack(slot));
    for module in racks.into_iter().flatten().flatten() {
        items.push(module.type_id);
        items.extend(module.charge);
    }
    items.extend(&fit.subsystems);
    let kind = |t: TypeId| catalog.get(t).map(|t| t.kind);
    let stacks = |wanted: Kind| {
        fit.drones
            .iter()
            .map(|d| d.type_id)
            .filter(move |&t| kind(t) == Some(wanted))
    };
    items.extend(stacks(Kind::Drone));
    items.extend(stacks(Kind::Fighter));
    items.extend(fit.ship);
    items.extend(fit.applied_implants(implants).map(|(t, _)| t));
    items.extend(fit.boosters.iter().map(|b| b.type_id));

    let mut seen = Vec::new();
    items
        .into_iter()
        .filter(|t| {
            let new = !seen.contains(t);
            seen.push(*t);
            new
        })
        .filter_map(|item| {
            let skills = check(catalog, item, &level);
            (!skills.is_empty()).then_some(ItemRequirements { item, skills })
        })
        .collect()
}

fn check(catalog: &Catalog, item: TypeId, level: &impl Fn(TypeId) -> u8) -> Vec<Requirement> {
    let Some(required) = catalog.required_skills.get(&item) else {
        return Vec::new();
    };
    required
        .iter()
        .filter(|&&(skill, needed)| needed > 0 && level(skill) < needed)
        .map(|&(skill, needed)| Requirement {
            skill,
            level: needed,
            prerequisites: check(catalog, skill, level),
        })
        .collect()
}

/// Every missing skill once, at the highest level anything needs it, sorted by name: pyfa's
/// condensed list, which Grant Missing Skills trains to.
pub fn condensed(missing: &[ItemRequirements], catalog: &Catalog) -> Vec<(TypeId, u8)> {
    fn walk(reqs: &[Requirement], out: &mut BTreeMap<TypeId, u8>) {
        for r in reqs {
            let level = out.entry(r.skill).or_default();
            *level = (*level).max(r.level);
            walk(&r.prerequisites, out);
        }
    }
    let mut levels = BTreeMap::new();
    for item in missing {
        walk(&item.skills, &mut levels);
    }
    by_name(levels, catalog)
}

/// [`condensed`] without skills that a deeper level of another listed skill needs anyway:
/// pyfa's "Copy Missing Skills (condensed)".
pub fn super_condensed(missing: &[ItemRequirements], catalog: &Catalog) -> Vec<(TypeId, u8)> {
    fn walk(
        reqs: &[Requirement],
        top: bool,
        all: &mut BTreeMap<TypeId, u8>,
        implied: &mut BTreeMap<TypeId, u8>,
    ) {
        for r in reqs {
            let level = all.entry(r.skill).or_default();
            *level = (*level).max(r.level);
            if !top {
                let level = implied.entry(r.skill).or_default();
                *level = (*level).max(r.level);
            }
            walk(&r.prerequisites, false, all, implied);
        }
    }
    let (mut all, mut implied) = (BTreeMap::new(), BTreeMap::new());
    for item in missing {
        walk(&item.skills, true, &mut all, &mut implied);
    }
    all.retain(|skill, level| implied.get(skill).is_none_or(|i| i < level));
    by_name(all, catalog)
}

fn by_name(levels: BTreeMap<TypeId, u8>, catalog: &Catalog) -> Vec<(TypeId, u8)> {
    let mut list: Vec<(TypeId, u8)> = levels.into_iter().collect();
    list.sort_by_cached_key(|&(skill, _)| catalog.name(skill));
    list
}

/// "Name level" lines, or "Name IV" with Roman numerals for `EVEMon`.
pub fn lines(skills: &[(TypeId, u8)], catalog: &Catalog, numerals: Numerals) -> String {
    let mut out = String::new();
    for &(skill, level) in skills {
        let level = match numerals {
            Numerals::Arabic => level.to_string(),
            Numerals::Roman => roman(level),
        };
        let _ = writeln!(out, "{} {level}", catalog.name(skill));
    }
    out
}

/// How skill levels are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Numerals {
    Arabic,
    Roman,
}

/// The missing-skills icon's tooltip. pyfa's default is the condensed list; the full one lists
/// each item with its skills indented under it.
pub fn tooltip(missing: &[ItemRequirements], catalog: &Catalog, style: TooltipStyle) -> String {
    if missing.is_empty() {
        return "All skill prerequisites have been met".to_owned();
    }
    let mut tip = String::from("Skills required:\n");
    match style {
        TooltipStyle::Condensed => {
            for (skill, level) in condensed(missing, catalog) {
                let _ = writeln!(tip, "{}: {level}", catalog.name(skill));
            }
        }
        TooltipStyle::Full => {
            for entry in plan(missing, catalog) {
                if entry.indent == 1 && entry.first_of_item {
                    let _ = writeln!(tip, "{}:", entry.item);
                }
                let pad = "    ".repeat(entry.indent);
                let _ = writeln!(tip, "{pad}{}: {}", entry.skill_name, entry.level);
            }
        }
    }
    tip.trim_end().to_owned()
}

/// How much the tooltip says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TooltipStyle {
    Condensed,
    Full,
}

/// One line of a training plan: a skill under the item that needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanEntry {
    pub item: String,
    pub first_of_item: bool,
    pub skill: TypeId,
    pub skill_name: String,
    pub level: u8,
    /// 1 for the item's own requirements, deeper for prerequisites.
    pub indent: usize,
}

/// The requirements walked depth first, the order pyfa's plans list them in.
pub fn plan(missing: &[ItemRequirements], catalog: &Catalog) -> Vec<PlanEntry> {
    fn walk(
        reqs: &[Requirement],
        item: &str,
        indent: usize,
        catalog: &Catalog,
        out: &mut Vec<PlanEntry>,
    ) {
        for r in reqs {
            let first_of_item = out.last().is_none_or(|e| e.item != item);
            out.push(PlanEntry {
                item: item.to_owned(),
                first_of_item,
                skill: r.skill,
                skill_name: catalog.name(r.skill),
                level: r.level,
                indent,
            });
            walk(&r.prerequisites, item, indent + 1, catalog, out);
        }
    }
    let mut out = Vec::new();
    for item in missing {
        walk(&item.skills, &catalog.name(item.item), 1, catalog, &mut out);
    }
    out
}

/// pyfa's "Export Skills Needed" as text.
pub fn plan_text(character: &str, plan: &[PlanEntry]) -> String {
    let rule = |c: char| c.to_string().repeat(79);
    let mut out = format!("Pyfa exported plan for \"{character}\"\n{}\n\n", rule('='));
    for entry in plan {
        if entry.first_of_item {
            let _ = writeln!(out, "{}\nSkills required for {}:", rule('-'), entry.item);
        }
        let pad = "    ".repeat(entry.indent);
        let _ = writeln!(out, "{pad}{}: {}", entry.skill_name, entry.level);
    }
    let _ = writeln!(out, "{}", rule('-'));
    out
}

/// `EVEMon`'s minimum plan revision that pyfa writes.
const EVEMON_REVISION: &str = "4081";

/// pyfa's "Export Skills Needed" as an `EVEMon` plan.
pub fn plan_xml(character: &str, plan: &[PlanEntry]) -> String {
    let mut out = String::from("<?xml version=\"1.0\" ?>\n");
    let _ = writeln!(
        out,
        "<plan name=\"Pyfa exported plan for {}\" revision=\"{EVEMON_REVISION}\">",
        escape(character)
    );
    out.push_str("  <sorting criteria=\"None\" groupByPriority=\"false\" order=\"None\"/>\n");
    let mut seen = Vec::new();
    for entry in plan {
        // `EVEMon` gets confused by the same skill and level twice.
        let key = (entry.skill, entry.level);
        if seen.contains(&key) {
            continue;
        }
        seen.push(key);
        let name = escape(&entry.skill_name);
        let _ = writeln!(
            out,
            "  <entry level=\"{}\" priority=\"3\" skill=\"{name}\" skillID=\"{}\" type=\"Prerequisite\">\n    <notes>{name}</notes>\n  </entry>",
            entry.level, entry.skill
        );
    }
    out.push_str("</plan>\n");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(skill: TypeId, level: u8, prerequisites: Vec<Requirement>) -> Requirement {
        Requirement {
            skill,
            level,
            prerequisites,
        }
    }

    #[test]
    fn super_condensed_drops_implied_levels() {
        let catalog = Catalog::default();
        // Item needs skill 2 at 3, which needs 1 at 4; the item also needs 1 at 2 directly.
        let missing = [ItemRequirements {
            item: 10,
            skills: vec![req(2, 3, vec![req(1, 4, vec![])]), req(1, 2, vec![])],
        }];
        let mut all = condensed(&missing, &catalog);
        all.sort_unstable();
        assert_eq!(all, [(1, 4), (2, 3)]);
        assert_eq!(super_condensed(&missing, &catalog), [(2, 3)]);
    }
}
