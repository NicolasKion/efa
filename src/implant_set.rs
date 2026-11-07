//! Implant sets: named groups of implants to put on a fit in one go. The user saves their own;
//! pirate sets and the like are worked out from the data the way pyfa's database build does.

use std::{collections::BTreeMap, fmt::Write, ops::RangeInclusive};

use serde::{Deserialize, Serialize};

use crate::{
    data::{Catalog, Kind, TypeId},
    fit::{implant_slot, plug_implant},
};

/// The slots a character has for implants.
pub const SLOTS: RangeInclusive<u8> = 1..=10;

/// Implant groups whose members can form sets: cyberimplants and the Genolution ones.
const SET_GROUPS: [i32; 2] = [300, 1730];
const GRADES: [&str; 3] = ["low-grade", "mid-grade", "high-grade"];
const SET_MEMBERS: [&str; 6] = ["alpha", "beta", "gamma", "delta", "epsilon", "omega"];
/// Genolution implants form a set without grades; pyfa files them under this name.
const GENOLUTION: &str = "Genolution";
const GENOLUTION_PREFIX: &str = "Genolution Core Augmentation CA-";

/// A user's saved set, one implant per slot, in slot order.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImplantSet {
    pub name: String,
    pub implants: Vec<TypeId>,
}

impl ImplantSet {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            implants: Vec::new(),
        }
    }

    /// Add an implant, replacing the one in its slot. Returns false if it is already in the set.
    pub fn add(&mut self, catalog: &Catalog, type_id: TypeId) -> bool {
        plug_implant(&mut self.implants, catalog, type_id)
    }

    /// The implant in a slot, if there is one.
    pub fn in_slot(&self, catalog: &Catalog, slot: u8) -> Option<TypeId> {
        self.implants
            .iter()
            .copied()
            .find(|&t| implant_slot(catalog, t) == slot)
    }

    /// Take an implant out. Returns false if it was not in the set.
    pub fn remove(&mut self, type_id: TypeId) -> bool {
        let before = self.implants.len();
        self.implants.retain(|&t| t != type_id);
        self.implants.len() != before
    }

    /// Put each implant into its own slot, a later one replacing an earlier one in the same
    /// slot. Returns how many went in.
    pub fn add_all(&mut self, catalog: &Catalog, implants: &[TypeId]) -> usize {
        implants.iter().filter(|&&t| self.add(catalog, t)).count()
    }

    /// Add an implant only if its slot is free, the way pyfa appends to a set on import.
    fn add_if_free(&mut self, catalog: &Catalog, type_id: TypeId) {
        let slot = implant_slot(catalog, type_id);
        if !self
            .implants
            .iter()
            .any(|&t| implant_slot(catalog, t) == slot)
        {
            self.add(catalog, type_id);
        }
    }
}

/// One grade of a precalculated set: its name, if the set has grades, and its implants.
pub type Grade = (Option<String>, Vec<TypeId>);

/// A set worked out from the data, such as the Amulet set, in one or more grades.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrecalcSet {
    pub name: String,
    /// Low, mid then high grade; a set without grades has one entry named None.
    pub grades: Vec<Grade>,
}

/// The sets pyfa precalculates: every "<Grade> <Set> <Alpha..Omega>" family of published
/// cyberimplants with at least two members, plus the Genolution set. Sorted by name.
pub fn precalculated(catalog: &Catalog) -> Vec<PrecalcSet> {
    let mut found: BTreeMap<(String, Option<String>), Vec<TypeId>> = BTreeMap::new();
    for t in catalog.types.values() {
        if !t.published || !SET_GROUPS.contains(&t.group_id) {
            continue;
        }
        if let Some((grade, set)) = set_membership(&t.name) {
            found.entry((set, grade)).or_default().push(t.id);
        }
    }
    let mut sets: BTreeMap<String, Vec<Grade>> = BTreeMap::new();
    for ((set, grade), mut implants) in found {
        if implants.len() < 2 {
            continue;
        }
        implants.sort_unstable();
        sets.entry(set).or_default().push((grade, implants));
    }
    sets.into_iter()
        .map(|(name, mut grades)| {
            grades.sort_by_key(|(grade, _)| {
                let rank = grade.as_deref().and_then(|g| {
                    GRADES
                        .iter()
                        .position(|known| g.eq_ignore_ascii_case(known))
                });
                (rank.unwrap_or(GRADES.len()), grade.clone())
            });
            PrecalcSet { name, grades }
        })
        .collect()
}

/// The (grade, set) an implant's name puts it in, if any.
fn set_membership(name: &str) -> Option<(Option<String>, String)> {
    if let Some(rest) = name.strip_prefix(GENOLUTION_PREFIX) {
        return rest
            .starts_with(|c: char| c.is_ascii_digit())
            .then(|| (None, GENOLUTION.to_owned()));
    }
    let mut words = name.split(' ');
    let (grade, set, member) = (words.next()?, words.next()?, words.next()?);
    let is_grade = GRADES.iter().any(|g| grade.eq_ignore_ascii_case(g));
    let is_word = !set.is_empty() && set.chars().all(|c| c.is_alphanumeric() || c == '_');
    let lower = member.to_lowercase();
    let is_member = SET_MEMBERS.iter().any(|m| lower.starts_with(m));
    (is_grade && is_word && is_member).then(|| (Some(grade.to_owned()), set.to_owned()))
}

/// `base` if no set is named that, otherwise `base 2`, `base 3` and so on.
pub fn free_name(sets: &[ImplantSet], base: &str) -> String {
    let taken = |name: &str| sets.iter().any(|s| s.name == name);
    if !taken(base) {
        return base.to_owned();
    }
    // Of these many numbers at least one is free.
    (2..=sets.len() + 2)
        .map(|n| format!("{base} {n}"))
        .find(|name| !taken(name))
        .unwrap_or_else(|| base.to_owned())
}

/// The name a duplicate of the set `name` gets: "<name> copy", numbered if that is taken.
pub fn copy_name(sets: &[ImplantSet], name: &str) -> String {
    free_name(sets, &format!("{name} copy"))
}

/// Implants of one kind within a slot, such as the slot's armor hardwirings or the pieces of
/// the Snake set that go in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImplantGroup {
    pub name: String,
    pub implants: Vec<TypeId>,
}

/// The implants that go in one slot, in groups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotImplants {
    pub slot: u8,
    pub groups: Vec<ImplantGroup>,
}

/// Every published implant on the market, by slot and then by kind: the market group it sits
/// in (attribute enhancers, armor implants and so on), with the pieces of a set like Snake or
/// Genolution in a group of their own after those. Slots without implants are left out.
pub fn by_slot(catalog: &Catalog) -> Vec<SlotImplants> {
    // Keyed by slot, whether the group is a set, and the group's name.
    let mut found: BTreeMap<(u8, bool, String), Vec<TypeId>> = BTreeMap::new();
    for t in catalog.types.values() {
        let Some(slot) = t.implant_slot.filter(|s| SLOTS.contains(s)) else {
            continue;
        };
        let Some(group) = t
            .market_group
            .filter(|_| t.published && t.kind == Kind::Implant)
        else {
            continue;
        };
        let key = match set_membership(&t.name) {
            Some((_, set)) => (slot, true, format!("{set} set")),
            None => (slot, false, market_group_name(catalog, group)),
        };
        found.entry(key).or_default().push(t.id);
    }
    let mut slots: Vec<SlotImplants> = Vec::new();
    for ((slot, is_set, name), mut implants) in found {
        if is_set {
            implants.sort_by_cached_key(|&t| {
                let name = catalog.name(t);
                (grade_rank(&name), name)
            });
        } else {
            crate::market::sort_items(&catalog.types, &mut implants);
        }
        let group = ImplantGroup { name, implants };
        match slots.last_mut() {
            Some(last) if last.slot == slot => last.groups.push(group),
            _ => slots.push(SlotImplants {
                slot,
                groups: vec![group],
            }),
        }
    }
    slots
}

/// What a market group is called in the slot tree. The leaf groups are named "Implant Slot
/// 06" and the like, which the tree already says, so those take their parent's name.
fn market_group_name(catalog: &Catalog, group: i32) -> String {
    let Some(g) = catalog.market_groups.get(&group) else {
        return "Other".to_owned();
    };
    if g.name.starts_with("Implant Slot")
        && let Some(parent) = g.parent.and_then(|p| catalog.market_groups.get(&p))
    {
        return parent.name.clone();
    }
    g.name.clone()
}

/// Low, mid then high grade, then anything without a grade.
fn grade_rank(name: &str) -> usize {
    let first = name.split(' ').next().unwrap_or_default();
    GRADES
        .iter()
        .position(|g| first.eq_ignore_ascii_case(g))
        .unwrap_or(GRADES.len())
}

/// What a grade of a set is called, such as "High-grade Snake", or just "Genolution".
pub fn grade_name(set: &PrecalcSet, grade: &Grade) -> String {
    match &grade.0 {
        Some(grade) => format!("{grade} {}", set.name),
        None => set.name.clone(),
    }
}

/// The effect an implant is bought for, in one line, from its description as plain text:
/// "+4 bonus to Perception, 0.5% bonus to maximum velocity". Set pieces give their primary and
/// secondary effects; the set bonus is only given when it is all the implant does, as on Omega
/// pieces. None when the description names no effect.
pub fn main_bonus(description: &str) -> Option<String> {
    const EFFECT_PREFIXES: [&str; 3] =
        ["primary effect:", "secondary effects:", "secondary effect:"];
    let mut effects: Vec<String> = Vec::new();
    let mut set_effect: Option<String> = None;
    for line in description.lines().map(str::trim) {
        let lower = line.to_lowercase();
        let has_number = line.chars().any(|c| c.is_ascii_digit());
        let is_effect = ["%", "bonus", "reduction", "increase"]
            .iter()
            .any(|word| lower.contains(word));
        if !has_number || !is_effect || lower.starts_with("note") || lower.starts_with("slot ") {
            continue;
        }
        let text = EFFECT_PREFIXES
            .iter()
            .find_map(|p| lower.starts_with(p).then(|| line.get(p.len()..)))
            .flatten()
            .unwrap_or(line)
            .trim()
            .trim_end_matches('.')
            .to_owned();
        if lower.contains("set effect") || lower.contains("strength of all") {
            set_effect.get_or_insert(text);
        } else {
            effects.push(text);
        }
    }
    if effects.is_empty() {
        set_effect
    } else {
        Some(effects.join(", "))
    }
}

/// The sets in pyfa's clipboard format: a commented header, then "[Name]" and one implant per
/// line for each set.
pub fn export(catalog: &Catalog, sets: &[ImplantSet]) -> String {
    let mut out = String::from(
        "# Exported from pyfa\n#\n# Values are in following format:\n# [Implant Set name]\n\
         # [Implant name]\n# [Implant name]\n# ...\n\n",
    );
    let mut sorted: Vec<&ImplantSet> = sets.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    for set in sorted {
        let _ = writeln!(out, "[{}]", set.name);
        for &implant in &set.implants {
            out.push_str(&catalog.name(implant));
            out.push('\n');
        }
        out.push('\n');
    }
    out.trim().to_owned()
}

/// What an import found: the sets, and how many lines could not be used.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Imported {
    pub sets: Vec<ImplantSet>,
    pub errors: usize,
}

/// Read sets in the export format. Unknown names, non-implants and implants before the first
/// set header count as errors.
pub fn import(catalog: &Catalog, text: &str) -> Imported {
    let mut imported = Imported::default();
    for line in text.lines().map(str::trim) {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            imported.sets.push(ImplantSet::new(name));
            continue;
        }
        let target = imported.sets.last_mut().zip(catalog.find(line));
        match target {
            Some((set, t)) if t.implant_slot.is_some() => set.add_if_free(catalog, t.id),
            _ => imported.errors += 1,
        }
    }
    imported
}

/// Add imported sets to the saved ones. A set whose name is taken adds its implants to the
/// existing set, into slots that are still free, as pyfa does.
pub fn merge(saved: &mut Vec<ImplantSet>, imported: Vec<ImplantSet>, catalog: &Catalog) {
    for set in imported {
        match saved.iter_mut().find(|s| s.name == set.name) {
            Some(existing) => {
                for implant in set.implants {
                    existing.add_if_free(catalog, implant);
                }
            }
            None => saved.push(set),
        }
    }
    saved.sort_by(|a, b| a.name.cmp(&b.name));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graded_set_names_are_recognised() {
        assert_eq!(
            set_membership("High-grade Amulet Alpha"),
            Some((Some("High-grade".to_owned()), "Amulet".to_owned()))
        );
        assert_eq!(
            set_membership("low-grade Snake Omega"),
            Some((Some("low-grade".to_owned()), "Snake".to_owned()))
        );
        assert_eq!(
            set_membership("Genolution Core Augmentation CA-3"),
            Some((None, GENOLUTION.to_owned()))
        );
    }

    #[test]
    fn names_for_new_and_copied_sets_are_free() {
        let sets = vec![
            ImplantSet::new("Snake"),
            ImplantSet::new("Snake copy"),
            ImplantSet::new("Snake copy 2"),
        ];
        assert_eq!(copy_name(&sets, "Snake"), "Snake copy 3");
        assert_eq!(copy_name(&sets, "Amulet"), "Amulet copy");
        assert_eq!(free_name(&sets, "New Set"), "New Set");
        assert_eq!(free_name(&sets, "Snake"), "Snake 2");
    }

    #[test]
    fn main_bonus_reads_the_effect_lines() {
        let snake = "This ocular filter has been modified by Serpentis scientists.\n\n\
                     Primary Effect: +4 bonus to Perception\n\n\
                     Secondary Effect: 0.5% bonus to maximum velocity\n\n\
                     Set Effect: 15% bonus to the strength of all Snake implant secondary effects";
        assert_eq!(
            main_bonus(snake).as_deref(),
            Some("+4 bonus to Perception, 0.5% bonus to maximum velocity")
        );
        let hardwiring = "A neural Interface upgrade, made in YC 110.\n\n\
                          1% bonus to repair system repair amount.\n\n\
                          Note: This implant has no effect on capital sized modules.";
        assert_eq!(
            main_bonus(hardwiring).as_deref(),
            Some("1% bonus to repair system repair amount")
        );
        let omega = "This implant does nothing in and of itself.\n\n\
                     25% bonus to the strength of all Amulet implant secondary effects.";
        assert_eq!(
            main_bonus(omega).as_deref(),
            Some("25% bonus to the strength of all Amulet implant secondary effects")
        );
        assert_eq!(main_bonus("Just a story about YC 120."), None);
    }

    #[test]
    fn other_implants_are_not_set_members() {
        for name in [
            "Ocular Filter - Basic",
            "High-grade Amulet",
            "Mid-grade Amulet Zeta",
            "Genolution Core Augmentation CA-X",
            "Superior-grade Amulet Alpha",
        ] {
            assert_eq!(set_membership(name), None, "{name}");
        }
    }
}
