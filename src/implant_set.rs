//! Implant sets: named groups of implants to put on a fit in one go. The user saves their own;
//! pirate sets and the like are worked out from the data the way pyfa's database build does.

use std::{collections::BTreeMap, fmt::Write};

use serde::{Deserialize, Serialize};

use crate::{
    data::{Catalog, TypeId},
    fit::{implant_slot, plug_implant},
};

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
