//! Mutaplasmids: which modules and drones each one takes, the abyssal type it turns them into,
//! and how far it can roll each attribute (dynamicitemattributes.json, pyfa's `mutaplasmids`
//! tables).

use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use serde::{Deserialize, Serialize};

use super::{TypeId, sde};

/// One mutaplasmid.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Mutaplasmid {
    pub id: TypeId,
    /// Each type it takes, with the abyssal type that comes out.
    #[serde(serialize_with = "crate::sorted::map")]
    pub results: HashMap<TypeId, TypeId>,
    /// Multipliers of the base value an attribute can roll between, by attribute ID.
    pub ranges: Vec<(i32, Range)>,
}

/// How far a mutaplasmid moves an attribute, as multipliers of the unmutated value.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Range {
    pub min: f64,
    pub max: f64,
}

impl Range {
    /// The multipliers as pyfa uses them, rounded to three decimals.
    fn rounded(self) -> (f64, f64) {
        let round = |v: f64| (v * 1000.0).round() / 1000.0;
        (round(self.min), round(self.max))
    }
}

impl Mutaplasmid {
    pub fn range(&self, attr: i32) -> Option<Range> {
        self.ranges
            .iter()
            .find(|&&(a, _)| a == attr)
            .map(|&(_, r)| r)
    }
}

/// A mutated attribute's name and which way is better.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutaAttr {
    /// The internal name the muta text format uses, e.g. `speedFactor`.
    pub name: String,
    pub high_is_good: bool,
}

/// Where a mutated attribute can go on one base item.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Limits {
    /// The unmutated value.
    pub base: f64,
    /// Base value times the minimum and the maximum multiplier. With a negative base the first
    /// is the larger one, as in pyfa.
    pub at_min: f64,
    pub at_max: f64,
}

impl Limits {
    pub fn low(self) -> f64 {
        self.at_min.min(self.at_max)
    }

    pub fn high(self) -> f64 {
        self.at_min.max(self.at_max)
    }

    /// pyfa's mutator validator: a zero base stays zero, everything else is kept in range.
    pub fn clamp(self, value: f64) -> f64 {
        if self.base == 0.0 {
            return 0.0;
        }
        value.clamp(self.low(), self.high())
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Mutaplasmids {
    #[serde(serialize_with = "crate::sorted::map")]
    pub by_type: HashMap<TypeId, Mutaplasmid>,
    /// Mutaplasmids that take a type, in pyfa's menu order.
    #[serde(serialize_with = "crate::sorted::map")]
    pub for_base: HashMap<TypeId, Vec<TypeId>>,
    /// Unmutated values of the attributes any of a type's mutaplasmids change, by type.
    #[serde(serialize_with = "crate::sorted::map")]
    pub base_values: HashMap<TypeId, Vec<(i32, f64)>>,
    /// Every attribute some mutaplasmid changes.
    #[serde(serialize_with = "crate::sorted::map")]
    pub attrs: HashMap<i32, MutaAttr>,
}

impl Mutaplasmids {
    pub fn get(&self, id: TypeId) -> Option<&Mutaplasmid> {
        self.by_type.get(&id)
    }

    /// Mutaplasmids that take `base`, in pyfa's menu order.
    pub fn for_base(&self, base: TypeId) -> &[TypeId] {
        self.for_base.get(&base).map_or(&[], Vec::as_slice)
    }

    pub fn base_value(&self, base: TypeId, attr: i32) -> Option<f64> {
        self.base_values
            .get(&base)?
            .iter()
            .find(|&&(a, _)| a == attr)
            .map(|&(_, v)| v)
    }

    /// Where `attr` of `base` can roll with mutaplasmid `muta`.
    pub fn limits(&self, muta: TypeId, base: TypeId, attr: i32) -> Option<Limits> {
        let (min, max) = self.get(muta)?.range(attr)?.rounded();
        let base = self.base_value(base, attr).unwrap_or(0.0);
        Some(Limits {
            base,
            at_min: base * min,
            at_max: base * max,
        })
    }

    /// The attribute named `name` that some mutaplasmid changes.
    pub fn attr_by_name(&self, name: &str) -> Option<i32> {
        self.attrs
            .iter()
            .find(|(_, a)| a.name.eq_ignore_ascii_case(name))
            .map(|(&id, _)| id)
    }

    /// The abyssal type a mutaplasmid turns `base` into.
    pub fn result(&self, muta: TypeId, base: TypeId) -> Option<TypeId> {
        self.get(muta)?.results.get(&base).copied()
    }

    /// Whether a type comes out of a mutaplasmid.
    pub fn is_result(&self, type_id: TypeId) -> bool {
        self.by_type
            .values()
            .any(|m| m.results.values().any(|&r| r == type_id))
    }
}

/// pyfa's `DynamicItem.shortName`: the grade of the mutaplasmid, "Gl." for glorified ones, and
/// for drone mutaplasmids what they improve.
pub fn short_name(name: &str) -> String {
    const GRADES: [&str; 8] = [
        "Decayed",
        "Glorified Decayed",
        "Gravid",
        "Glorified Gravid",
        "Unstable",
        "Glorified Unstable",
        "Radical",
        "Glorified Radical",
    ];
    let mut short = name.to_owned();
    for grade in GRADES {
        if short.starts_with(&format!("{grade} ")) {
            grade.clone_into(&mut short);
        }
    }
    // "(Glorified )Exigent Heavy Drone Durability Mutaplasmid": grade and what it improves.
    let words: Vec<&str> = short.split(' ').collect();
    let grade_len = if words.first() == Some(&"Glorified") {
        2
    } else {
        1
    };
    if let Some((grade, rest)) = words.split_at_checked(grade_len)
        && let [_kind, "Drone", improves, "Mutaplasmid"] = rest
    {
        short = format!("{} {improves}", grade.join(" "));
    }
    short.replace("Glorified ", "Gl. ")
}

/// pyfa's menu order: by short name without "Gl.", glorified ones after the plain grade.
fn menu_key(name: &str) -> (String, bool) {
    let short = short_name(name);
    match short.strip_prefix("Gl. ") {
        Some(rest) => (rest.to_owned(), true),
        None => (short, false),
    }
}

#[derive(Deserialize)]
struct PhbDynamicItem {
    #[serde(rename = "inputOutputMapping", default)]
    mappings: Vec<PhbMapping>,
    #[serde(rename = "attributeIDs", default)]
    attributes: HashMap<i32, PhbRange>,
}

#[derive(Deserialize)]
struct PhbMapping {
    #[serde(rename = "applicableTypes", default)]
    applicable: Vec<TypeId>,
    #[serde(rename = "resultingType")]
    result: TypeId,
}

#[derive(Deserialize)]
struct PhbRange {
    min: f64,
    max: f64,
}

/// The raw mutaplasmid table, read before the types so abyssal types can borrow attributes.
pub(super) struct Source {
    items: HashMap<TypeId, PhbDynamicItem>,
}

impl Source {
    pub(super) fn read(dir: &Path) -> Result<Self, String> {
        let items = sde::read(dir, "fsd_built/dynamicitemattributes.json")?;
        Ok(Self { items })
    }

    /// For each abyssal type, a type it is made from. The game keeps the abyssal types' own
    /// attributes nearly empty and fills them in from the base item, so the catalog reads their
    /// charges, bandwidth and the like from that type.
    pub(super) fn stand_ins(&self) -> HashMap<TypeId, TypeId> {
        let mut stand_ins: HashMap<TypeId, TypeId> = HashMap::new();
        for item in self.items.values() {
            for mapping in &item.mappings {
                if let Some(&base) = mapping.applicable.iter().min() {
                    stand_ins
                        .entry(mapping.result)
                        .and_modify(|b| *b = (*b).min(base))
                        .or_insert(base);
                }
            }
        }
        stand_ins
    }

    pub(super) fn finish(
        self,
        dir: &Path,
        names: &HashMap<TypeId, String>,
        dogma: &HashMap<TypeId, sde::TypeDogma>,
    ) -> Result<Mutaplasmids, String> {
        let mut by_type = HashMap::with_capacity(self.items.len());
        let mut for_base: HashMap<TypeId, Vec<TypeId>> = HashMap::new();
        let mut mutable: HashMap<TypeId, HashSet<i32>> = HashMap::new();
        let mut attr_ids = HashSet::new();
        for (id, item) in self.items {
            let mut ranges: Vec<(i32, Range)> = item
                .attributes
                .iter()
                .map(|(&attr, r)| {
                    (
                        attr,
                        Range {
                            min: r.min,
                            max: r.max,
                        },
                    )
                })
                .collect();
            ranges.sort_by_key(|&(a, _)| a);
            let mut results = HashMap::new();
            for mapping in &item.mappings {
                for &base in &mapping.applicable {
                    results.insert(base, mapping.result);
                    for_base.entry(base).or_default().push(id);
                    mutable
                        .entry(base)
                        .or_default()
                        .extend(ranges.iter().map(|&(a, _)| a));
                }
            }
            attr_ids.extend(ranges.iter().map(|&(a, _)| a));
            by_type.insert(
                id,
                Mutaplasmid {
                    id,
                    results,
                    ranges,
                },
            );
        }
        let name = |id: &TypeId| names.get(id).cloned().unwrap_or_default();
        for list in for_base.values_mut() {
            list.sort_by_cached_key(|id| (menu_key(&name(id)), *id));
        }
        // Like refine, the base item's own value wins; an attribute it lacks comes from the
        // abyssal type.
        let dogma_value = |type_id: TypeId, attr: i32| dogma.get(&type_id)?.attr(attr);
        let base_values = mutable
            .into_iter()
            .map(|(base, attrs)| {
                let results: Vec<TypeId> = for_base
                    .get(&base)
                    .into_iter()
                    .flatten()
                    .filter_map(|m| by_type.get(m)?.results.get(&base).copied())
                    .collect();
                let mut values: Vec<(i32, f64)> = attrs
                    .into_iter()
                    .filter_map(|attr| {
                        let value = dogma_value(base, attr)
                            .or_else(|| results.iter().find_map(|&r| dogma_value(r, attr)))?;
                        Some((attr, value))
                    })
                    .collect();
                values.sort_by_key(|&(a, _)| a);
                (base, values)
            })
            .collect();
        let attrs = sde::read::<HashMap<i32, sde::Attr>>(dir, "sde/dogmaattributes.json")?
            .into_iter()
            .filter_map(|(id, a)| {
                attr_ids.contains(&id).then_some((
                    id,
                    MutaAttr {
                        name: a.name,
                        high_is_good: a.high_is_good,
                    },
                ))
            })
            .collect();
        Ok(Mutaplasmids {
            by_type,
            for_base,
            base_values,
            attrs,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_names_follow_pyfa() {
        assert_eq!(
            short_name("Unstable Stasis Webifier Mutaplasmid"),
            "Unstable"
        );
        assert_eq!(
            short_name("Glorified Decayed Small Armor Plate Mutaplasmid"),
            "Gl. Decayed"
        );
        assert_eq!(
            short_name("Exigent Heavy Drone Durability Mutaplasmid"),
            "Exigent Durability"
        );
        assert_eq!(
            short_name("Glorified Exigent Light Drone Firepower Mutaplasmid"),
            "Gl. Exigent Firepower"
        );
        assert_eq!(
            short_name("Radical Drone Damage Amplifier Mutaplasmid"),
            "Radical"
        );
        // pyfa's pattern wants one word between grade and "Drone", so these stay whole.
        assert_eq!(
            short_name("Exigent Ice Harvesting Drone Mutaplasmid"),
            "Exigent Ice Harvesting Drone Mutaplasmid"
        );
    }

    #[test]
    fn glorified_sorts_after_its_grade() {
        let mut names = vec![
            "Glorified Decayed X Mutaplasmid",
            "Unstable X Mutaplasmid",
            "Decayed X Mutaplasmid",
        ];
        names.sort_by_key(|n| menu_key(n));
        assert_eq!(
            names,
            [
                "Decayed X Mutaplasmid",
                "Glorified Decayed X Mutaplasmid",
                "Unstable X Mutaplasmid"
            ]
        );
    }

    #[test]
    fn limits_clamp_both_ways() {
        let negative = Limits {
            base: -50.0,
            at_min: -40.0,
            at_max: -60.0,
        };
        assert_eq!(negative.clamp(-70.0), -60.0);
        assert_eq!(negative.clamp(-10.0), -40.0);
        let zero = Limits {
            base: 0.0,
            at_min: 0.0,
            at_max: 0.0,
        };
        assert_eq!(zero.clamp(3.0), 0.0);
    }
}
