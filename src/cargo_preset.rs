//! Cargo presets: named lists of items to put in a fit's hold in one go, such as a stack of
//! filaments or a set of scripts, with a policy for what is already there. A gyfa extra.

use serde::{Deserialize, Serialize};

use crate::data::{Catalog, TypeId};

/// What applying a preset does with items the hold already has.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CargoPolicy {
    /// Make sure there are at least as many as listed, adding only what is missing.
    #[default]
    TopUp,
    /// Always add the listed amounts, on top of what is there.
    AddOnTop,
    /// Leave exactly the listed amount of each listed item; other cargo is left alone.
    Exact,
}

impl CargoPolicy {
    pub const ALL: [Self; 3] = [Self::TopUp, Self::AddOnTop, Self::Exact];

    pub fn label(self) -> &'static str {
        match self {
            Self::TopUp => "Top up",
            Self::AddOnTop => "Add on top",
            Self::Exact => "Exact",
        }
    }

    /// One line on what applying does.
    pub fn explanation(self) -> &'static str {
        match self {
            Self::TopUp => "Adds only what is missing to reach each amount.",
            Self::AddOnTop => "Always adds each amount, on top of what is already there.",
            Self::Exact => {
                "Sets each listed item to exactly its amount, removing extras. Other cargo stays."
            }
        }
    }
}

/// A user's preset: items with amounts, one entry per item, and how to apply them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CargoPreset {
    pub name: String,
    pub items: Vec<(TypeId, u32)>,
    pub policy: CargoPolicy,
}

impl CargoPreset {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// Add `count` of an item, on top of its entry if it has one.
    pub fn add(&mut self, type_id: TypeId, count: u32) {
        if count == 0 {
            return;
        }
        match self.items.iter_mut().find(|(t, _)| *t == type_id) {
            Some((_, n)) => *n = n.saturating_add(count),
            None => self.items.push((type_id, count)),
        }
    }

    /// Set an item's amount; zero takes it off the list.
    pub fn set(&mut self, type_id: TypeId, count: u32) {
        if count == 0 {
            self.remove(type_id);
        } else if let Some((_, n)) = self.items.iter_mut().find(|(t, _)| *t == type_id) {
            *n = count;
        }
    }

    pub fn remove(&mut self, type_id: TypeId) {
        self.items.retain(|&(t, _)| t != type_id);
    }
}

/// What applying a preset changed, in units.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Applied {
    pub added: u32,
    /// Units of listed items the hold already had and that counted toward the preset.
    pub already: u32,
    pub removed: u32,
}

impl Applied {
    pub fn changed(self) -> bool {
        self.added > 0 || self.removed > 0
    }

    /// The status line: "Added 10 items; 5 were already there".
    pub fn summary(self, noun: &str) -> String {
        let units = |n: u32| {
            if n == 1 {
                format!("1 {}", singular(noun))
            } else {
                format!("{n} {noun}")
            }
        };
        let mut parts = Vec::new();
        if self.added > 0 {
            parts.push(format!("Added {}", units(self.added)));
        }
        if self.removed > 0 {
            let verb = if parts.is_empty() {
                "Removed"
            } else {
                "removed"
            };
            parts.push(format!("{verb} {}", units(self.removed)));
        }
        if parts.is_empty() {
            return if self.already > 0 {
                format!(
                    "Nothing to add; all {} were already there",
                    units(self.already)
                )
            } else {
                "Nothing to add".to_owned()
            };
        }
        if self.already > 0 {
            let were = if self.already == 1 { "was" } else { "were" };
            parts.push(format!("{} {were} already there", self.already));
        }
        parts.join("; ")
    }
}

/// What to call a preset's items in the status line: their group's name when they all share one
/// that reads as a plural ("abyssal filaments"), "items" otherwise.
pub fn noun(catalog: &Catalog, preset: &CargoPreset) -> String {
    let mut groups = preset
        .items
        .iter()
        .map(|&(t, _)| catalog.get(t).map(|t| t.group_id));
    let first = groups.next().flatten();
    let shared = first.filter(|&g| groups.all(|other| other == Some(g)));
    shared
        .and_then(|g| catalog.group_names.get(&g))
        .map(|name| name.to_lowercase())
        .filter(|name| name.ends_with('s') && !name.ends_with("ss"))
        .unwrap_or_else(|| "items".to_owned())
}

/// Volume the preset's items take, in m³.
pub fn volume(catalog: &Catalog, items: &[(TypeId, u32)]) -> f64 {
    items
        .iter()
        .map(|&(t, n)| catalog.get(t).map_or(0.0, |t| t.volume) * f64::from(n))
        .sum()
}

fn singular(noun: &str) -> &str {
    noun.strip_suffix('s').unwrap_or(noun)
}

/// Units of an item in the hold, over all its stacks.
fn held(cargo: &[(TypeId, u32)], type_id: TypeId) -> u32 {
    cargo
        .iter()
        .filter(|&&(t, _)| t == type_id)
        .fold(0, |sum, &(_, n)| sum.saturating_add(n))
}

/// Leave exactly `count` of an item in one stack, where its first stack was.
fn set_held(cargo: &mut Vec<(TypeId, u32)>, type_id: TypeId, count: u32) {
    let mut first = true;
    cargo.retain_mut(|(t, n)| {
        if *t != type_id {
            return true;
        }
        let keep = std::mem::take(&mut first);
        *n = count;
        keep && count > 0
    });
    if first && count > 0 {
        cargo.push((type_id, count));
    }
}

/// Put a preset into a hold as its policy says.
pub fn apply(preset: &CargoPreset, cargo: &mut Vec<(TypeId, u32)>) -> Applied {
    let mut applied = Applied::default();
    for &(type_id, want) in &preset.items {
        let have = held(cargo, type_id);
        let target = match preset.policy {
            CargoPolicy::TopUp => have.max(want),
            CargoPolicy::AddOnTop => have.saturating_add(want),
            CargoPolicy::Exact => want,
        };
        applied.already = applied.already.saturating_add(have.min(want));
        if target > have {
            applied.added += target - have;
        } else {
            applied.removed += have - target;
        }
        if target != have {
            set_held(cargo, type_id, target);
        }
    }
    applied
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preset(policy: CargoPolicy) -> CargoPreset {
        CargoPreset {
            name: "Filaments".into(),
            items: vec![(1, 10), (2, 3)],
            policy,
        }
    }

    #[test]
    fn top_up_adds_only_what_is_missing() {
        let mut cargo = vec![(1, 5), (9, 1), (2, 7)];
        let applied = apply(&preset(CargoPolicy::TopUp), &mut cargo);
        assert_eq!(cargo, vec![(1, 10), (9, 1), (2, 7)]);
        assert_eq!(
            applied,
            Applied {
                added: 5,
                already: 8,
                removed: 0
            }
        );
    }

    #[test]
    fn add_on_top_always_adds() {
        let mut cargo = vec![(1, 5)];
        let applied = apply(&preset(CargoPolicy::AddOnTop), &mut cargo);
        assert_eq!(cargo, vec![(1, 15), (2, 3)]);
        assert_eq!(applied.added, 13);
        assert_eq!(applied.already, 5);
    }

    #[test]
    fn exact_sets_listed_items_and_leaves_the_rest() {
        let mut cargo = vec![(1, 4), (9, 2), (2, 5), (1, 3)];
        let applied = apply(&preset(CargoPolicy::Exact), &mut cargo);
        assert_eq!(cargo, vec![(1, 10), (9, 2), (2, 3)]);
        assert_eq!(
            applied,
            Applied {
                added: 3,
                already: 10,
                removed: 2
            }
        );
    }

    #[test]
    fn applying_twice_with_top_up_changes_nothing() {
        let mut cargo = Vec::new();
        apply(&preset(CargoPolicy::TopUp), &mut cargo);
        let again = apply(&preset(CargoPolicy::TopUp), &mut cargo);
        assert!(!again.changed());
        assert_eq!(
            again.summary("items"),
            "Nothing to add; all 13 items were already there"
        );
    }

    #[test]
    fn summaries_read_naturally() {
        let applied = Applied {
            added: 10,
            already: 5,
            removed: 0,
        };
        assert_eq!(
            applied.summary("filaments"),
            "Added 10 filaments; 5 were already there"
        );
        let one = Applied {
            added: 1,
            already: 1,
            removed: 2,
        };
        assert_eq!(
            one.summary("items"),
            "Added 1 item; removed 2 items; 1 was already there"
        );
    }

    #[test]
    fn editing_items_keeps_one_entry_each() {
        let mut p = CargoPreset::new("p");
        p.add(1, 2);
        p.add(1, 3);
        p.add(2, 0);
        assert_eq!(p.items, vec![(1, 5)]);
        p.set(1, 0);
        assert_eq!(p.items, []);
    }
}
