//! What the item stats window shows beyond the catalog: every dogma attribute with its name, unit
//! and icon, each type's base attributes and effects, descriptions and traits.
//!
//! Only the stats window needs this, and descriptions alone are megabytes, so it is loaded on
//! first use instead of at start, and cached on disk like the catalog. What needs a skill comes
//! from the catalog, which knows every type's skills already.

use std::{
    collections::{HashMap, HashSet},
    path::Path,
};

use serde::{Deserialize, Serialize};

pub use crate::data::sde::REQUIRED_SKILL_ATTRS;
use crate::data::{TypeId, cache, sde};

/// Bump whenever what [`ItemData`] holds or how it is built changes.
const CACHE_FORMAT: u32 = 2;
const CACHE_FILE: &str = "item_data.bin";

/// Attributes that are columns of types.json rather than dogma attributes. pyfa adds them to
/// every item's attributes, so they show up in its lists too.
pub mod type_attr {
    pub const MASS: i32 = 4;
    pub const CAPACITY: i32 = 38;
    pub const VOLUME: i32 = 161;
    pub const RADIUS: i32 = 162;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttrInfo {
    pub id: i32,
    /// The internal name, e.g. `maxVelocity`.
    pub name: String,
    /// What the game calls it, e.g. "Maximum Velocity"; many attributes have none.
    pub display_name: Option<String>,
    pub unit: Option<i32>,
    pub icon: Option<i32>,
    pub published: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitInfo {
    /// The internal name, which decides how values are converted for display.
    pub name: String,
    /// What is printed after a value, e.g. "m/sec".
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffectInfo {
    pub name: String,
    /// Dogma effect category: passive, active, target and so on.
    pub category: i32,
}

/// One line of a trait section.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Bonus {
    /// The amount with its unit, e.g. "7.5%", missing on bonuses that are just a sentence.
    pub number: Option<String>,
    /// Text in the game's markup; it may carry links to other types.
    pub text: String,
}

/// A ship's or subsystem's bonuses, as the game lists them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Traits {
    /// Bonuses per level of a skill, by skill.
    pub skills: Vec<(TypeId, Vec<Bonus>)>,
    pub role: Vec<Bonus>,
    pub misc: Vec<Bonus>,
}

impl Traits {
    fn is_empty(&self) -> bool {
        self.skills.is_empty() && self.role.is_empty() && self.misc.is_empty()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TypeInfo {
    /// Base attribute values by attribute ID, sorted by ID.
    pub attrs: Vec<(i32, f64)>,
    /// Dogma effects, with whether each is the type's default effect.
    pub effects: Vec<(i32, bool)>,
    pub description: Option<String>,
    pub traits: Option<Traits>,
    pub base_price: Option<f64>,
    pub portion_size: Option<i32>,
    pub tech_level: Option<i32>,
    pub race_id: Option<i32>,
    pub faction_id: Option<i32>,
}

impl TypeInfo {
    pub fn attr(&self, id: i32) -> Option<f64> {
        self.attrs
            .binary_search_by_key(&id, |&(a, _)| a)
            .ok()
            .map(|i| self.attrs[i].1)
    }

    /// Skills needed to use the type, with the level each needs, in the game's order.
    pub fn required_skills(&self) -> Vec<(TypeId, u8)> {
        REQUIRED_SKILL_ATTRS
            .iter()
            .filter_map(|&(skill, level)| {
                let skill = self.attr(skill).filter(|&s| s > 0.0)? as TypeId;
                let level = self.attr(level).unwrap_or(0.0) as u8;
                Some((skill, level))
            })
            .collect()
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ItemData {
    #[serde(serialize_with = "crate::sorted::map")]
    pub attrs: HashMap<i32, AttrInfo>,
    #[serde(serialize_with = "crate::sorted::map")]
    pub units: HashMap<i32, UnitInfo>,
    #[serde(serialize_with = "crate::sorted::map")]
    pub effects: HashMap<i32, EffectInfo>,
    #[serde(serialize_with = "crate::sorted::map")]
    pub types: HashMap<TypeId, TypeInfo>,
    /// The certificates each ship's top mastery level asks for, by name, like "Large Energy
    /// Turret". They say what the ship is meant to fly with.
    #[serde(default)]
    #[serde(serialize_with = "crate::sorted::map")]
    pub masteries: HashMap<TypeId, Vec<String>>,
}

impl ItemData {
    /// From the cache when it was built from this data for these types, otherwise built from the
    /// JSON and cached. `key` names the data the catalog was built from.
    pub fn load_cached(
        dir: &Path,
        cache_dir: &Path,
        key: Option<&str>,
        wanted: &HashSet<TypeId>,
    ) -> Result<Self, String> {
        let path = cache_dir.join(CACHE_FILE);
        let key = key.map(|k| format!("{CACHE_FORMAT}:{}:{k}", wanted.len()));
        if let Some(key) = &key
            && let Some(data) = cache::read(&path, key)
        {
            return Ok(data);
        }
        let data = Self::load(dir, wanted)?;
        if let Some(key) = key {
            cache::write(&path, &key, &data);
        }
        Ok(data)
    }

    /// The data as the data pack stores it, with the format it was written in.
    pub fn to_bytes(&self) -> Result<(u32, Vec<u8>), String> {
        postcard::to_stdvec(self)
            .map(|bytes| (CACHE_FORMAT, bytes))
            .map_err(|e| format!("writing the item data: {e}"))
    }

    /// Data [`ItemData::to_bytes`] wrote.
    pub fn from_bytes(format: u32, bytes: &[u8]) -> Result<Self, String> {
        if format != CACHE_FORMAT {
            return Err(format!(
                "the item data is in format {format}, this build reads {CACHE_FORMAT}"
            ));
        }
        postcard::from_bytes(bytes).map_err(|e| format!("reading the item data: {e}"))
    }

    /// Read the dump for the types in `wanted`.
    pub fn load(dir: &Path, wanted: &HashSet<TypeId>) -> Result<Self, String> {
        let sde_attrs: HashMap<i32, sde::Attr> = sde::read(dir, "sde/dogmaattributes.json")?;
        let sde_units: HashMap<i32, sde::Unit> = sde::read(dir, "sde/dogmaunits.json")?;
        let sde_effects: HashMap<i32, sde::Effect> = sde::read(dir, "sde/dogmaeffects.json")?;
        let dogma: HashMap<TypeId, sde::TypeDogma> = sde::read(dir, "sde/typedogma.json")?;
        let bonuses: HashMap<TypeId, sde::TypeBonus> = sde::read(dir, "sde/typebonus.json")?;
        let sde_types: HashMap<TypeId, sde::TypeDetails> = sde::read(dir, "sde/types.json")?;
        let masteries = masteries(dir, wanted)?;

        let attrs = sde_attrs
            .into_iter()
            .map(|(id, a)| {
                let display_name = a.display_name.map(|d| d.en).filter(|d| !d.is_empty());
                (
                    id,
                    AttrInfo {
                        id,
                        name: a.name,
                        display_name,
                        unit: a.unit_id,
                        icon: a.icon_id,
                        published: a.published,
                    },
                )
            })
            .collect();
        let units = sde_units
            .into_iter()
            .map(|(id, u)| {
                let display_name = u.display_name.map(|d| d.en).filter(|d| !d.is_empty());
                (
                    id,
                    UnitInfo {
                        name: u.name,
                        display_name,
                    },
                )
            })
            .collect();
        let effects = sde_effects
            .into_iter()
            .map(|(id, e)| {
                (
                    id,
                    EffectInfo {
                        name: e.name,
                        category: e.effect_category_id,
                    },
                )
            })
            .collect();

        let mut types = HashMap::with_capacity(wanted.len());
        for (id, t) in sde_types {
            if !wanted.contains(&id) {
                continue;
            }
            let dogma = dogma.get(&id);
            let mut attrs: Vec<(i32, f64)> = dogma.map_or_default(|d| {
                d.attributes
                    .iter()
                    .map(|a| (a.attribute_id, a.value))
                    .collect()
            });
            for (attr, value) in [
                (type_attr::MASS, t.mass),
                (type_attr::CAPACITY, t.capacity),
                (type_attr::VOLUME, t.volume),
                (type_attr::RADIUS, t.radius),
            ] {
                if let Some(value) = value
                    && !attrs.iter().any(|&(a, _)| a == attr)
                {
                    attrs.push((attr, value));
                }
            }
            attrs.sort_by_key(|&(a, _)| a);
            let effects = dogma.map_or_default(|d| {
                d.effects
                    .iter()
                    .map(|e| (e.effect_id, e.is_default))
                    .collect()
            });
            let traits = bonuses.get(&id).map(traits_from).filter(|t| !t.is_empty());
            types.insert(
                id,
                TypeInfo {
                    attrs,
                    effects,
                    description: t.description.map(|d| d.en).filter(|d| !d.trim().is_empty()),
                    traits,
                    base_price: t.base_price,
                    portion_size: t.portion_size,
                    tech_level: t.tech_level,
                    race_id: t.race_id,
                    faction_id: t.faction_id,
                },
            );
        }
        Ok(Self {
            attrs,
            units,
            effects,
            types,
            masteries,
        })
    }

    pub fn type_info(&self, id: TypeId) -> Option<&TypeInfo> {
        self.types.get(&id)
    }

    pub fn attr(&self, id: i32) -> Option<&AttrInfo> {
        self.attrs.get(&id)
    }

    pub fn unit(&self, attr: &AttrInfo) -> Option<&UnitInfo> {
        attr.unit.and_then(|u| self.units.get(&u))
    }
}

/// How the game writes a trait's amount: 7.5%, 2x, +1.
fn bonus_number(bonus: f64, unit: Option<i32>) -> String {
    let n = if bonus.fract() == 0.0 {
        format!("{bonus:.0}")
    } else {
        format!("{bonus}")
    };
    match unit {
        Some(104) => format!("{n}x"),
        Some(105) => format!("{n}%"),
        Some(139) => format!("+{n}"),
        _ => n,
    }
}

fn bonuses_from(list: &[sde::Bonus]) -> Vec<Bonus> {
    let mut sorted: Vec<&sde::Bonus> = list.iter().collect();
    sorted.sort_by_key(|b| b.importance);
    sorted
        .into_iter()
        .map(|b| Bonus {
            number: b.amount.map(|v| bonus_number(v, b.unit_id)),
            text: b.text.en.clone(),
        })
        .collect()
}

fn traits_from(bonus: &sde::TypeBonus) -> Traits {
    Traits {
        skills: bonus
            .types
            .iter()
            .map(|s| (s.key, bonuses_from(&s.value)))
            .filter(|(_, b)| !b.is_empty())
            .collect(),
        role: bonuses_from(&bonus.role_bonuses),
        misc: bonuses_from(&bonus.misc_bonuses),
    }
}

/// The certificate names of each wanted ship's highest mastery level.
fn masteries(dir: &Path, wanted: &HashSet<TypeId>) -> Result<HashMap<TypeId, Vec<String>>, String> {
    let masteries: HashMap<TypeId, sde::Mastery> = sde::read(dir, "sde/masteries.json")?;
    let certificates: HashMap<i32, sde::Certificate> = sde::read(dir, "sde/certificates.json")?;
    Ok(masteries
        .into_iter()
        .filter_map(|(ship, mastery)| {
            if !wanted.contains(&ship) {
                return None;
            }
            let top = mastery.levels.last()?;
            let names = top
                .certificates
                .iter()
                .filter_map(|c| certificates.get(c))
                .map(|c| c.name.en.clone())
                .collect();
            Some((ship, names))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trait_amounts_carry_their_unit() {
        assert_eq!(bonus_number(7.5, Some(105)), "7.5%");
        assert_eq!(bonus_number(10.0, Some(105)), "10%");
        assert_eq!(bonus_number(2.0, Some(104)), "2x");
        assert_eq!(bonus_number(1.0, Some(139)), "+1");
    }

    #[test]
    fn required_skills_come_in_attribute_order() {
        let info = TypeInfo {
            attrs: vec![(182, 3300.0), (183, 3301.0), (277, 3.0), (278, 1.0)],
            ..TypeInfo::default()
        };
        assert_eq!(info.required_skills(), [(3300, 3), (3301, 1)]);
    }
}
