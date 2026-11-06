//! What the item stats window shows beyond the catalog: every dogma attribute with its name, unit
//! and icon, each type's base attributes and effects, descriptions and traits.
//!
//! Only the stats window needs this, and descriptions alone are megabytes, so it is loaded on
//! first use instead of at start, and cached on disk like the catalog.

use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::BufReader,
    path::Path,
};

use serde::{Deserialize, Serialize, de::DeserializeOwned};

use crate::data::TypeId;

/// Bump whenever what [`ItemData`] holds or how it is built changes.
const CACHE_FORMAT: u32 = 1;
const CACHE_FILE: &str = "item_data.bin";

/// Attributes that are columns of types.json rather than dogma attributes. pyfa adds them to
/// every item's attributes, so they show up in its lists too.
pub mod type_attr {
    pub const MASS: i32 = 4;
    pub const CAPACITY: i32 = 38;
    pub const VOLUME: i32 = 161;
    pub const RADIUS: i32 = 162;
}

/// The skill and level attribute pairs a type lists its prerequisites in, in the game's order.
pub const REQUIRED_SKILL_ATTRS: [(i32, i32); 6] = [
    (182, 277),
    (183, 278),
    (184, 279),
    (1285, 1286),
    (1289, 1287),
    (1290, 1288),
];

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
    pub attrs: HashMap<i32, AttrInfo>,
    pub units: HashMap<i32, UnitInfo>,
    pub effects: HashMap<i32, EffectInfo>,
    pub types: HashMap<TypeId, TypeInfo>,
    /// Types needing a skill, with the level they need, keyed by the skill.
    #[serde(skip)]
    pub required_for: HashMap<TypeId, Vec<(TypeId, u8)>>,
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
            && let Some(mut data) = read_cache(&path, key)
        {
            data.link();
            return Ok(data);
        }
        let mut data = Self::load(dir, wanted)?;
        if let Some(key) = key {
            write_cache(&path, &key, &data);
        }
        data.link();
        Ok(data)
    }

    /// Read the dump for the types in `wanted`.
    pub fn load(dir: &Path, wanted: &HashSet<TypeId>) -> Result<Self, String> {
        let sde_attrs: HashMap<String, SdeAttr> = read(dir, "sde/dogmaattributes.json")?;
        let sde_units: HashMap<String, SdeUnit> = read(dir, "sde/dogmaunits.json")?;
        let sde_effects: HashMap<String, SdeEffect> = read(dir, "sde/dogmaeffects.json")?;
        let dogma: HashMap<String, TypeDogma> = read(dir, "sde/typedogma.json")?;
        let bonuses: HashMap<String, SdeTypeBonus> = read(dir, "sde/typebonus.json")?;
        let sde_types: HashMap<String, SdeType> = read(dir, "sde/types.json")?;

        let attrs = sde_attrs
            .into_iter()
            .filter_map(|(id, a)| {
                let id: i32 = id.parse().ok()?;
                let display_name = a.display_name.map(|d| d.en).filter(|d| !d.is_empty());
                Some((
                    id,
                    AttrInfo {
                        id,
                        name: a.name,
                        display_name,
                        unit: a.unit_id,
                        icon: a.icon_id,
                        published: a.published,
                    },
                ))
            })
            .collect();
        let units = sde_units
            .into_iter()
            .filter_map(|(id, u)| {
                let display_name = u.display_name.map(|d| d.en).filter(|d| !d.is_empty());
                Some((
                    id.parse().ok()?,
                    UnitInfo {
                        name: u.name,
                        display_name,
                    },
                ))
            })
            .collect();
        let effects = sde_effects
            .into_iter()
            .filter_map(|(id, e)| {
                Some((
                    id.parse().ok()?,
                    EffectInfo {
                        name: e.name,
                        category: e.effect_category_id,
                    },
                ))
            })
            .collect();

        let mut types = HashMap::with_capacity(wanted.len());
        for (id, t) in sde_types {
            let Ok(id) = id.parse::<TypeId>() else {
                continue;
            };
            if !wanted.contains(&id) {
                continue;
            }
            let key = id.to_string();
            let dogma = dogma.get(&key);
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
            let traits = bonuses.get(&key).map(traits_from).filter(|t| !t.is_empty());
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
            required_for: HashMap::new(),
        })
    }

    /// Fill in what is derived rather than stored.
    fn link(&mut self) {
        let mut required_for: HashMap<TypeId, Vec<(TypeId, u8)>> = HashMap::new();
        for (&id, info) in &self.types {
            for (skill, level) in info.required_skills() {
                required_for.entry(skill).or_default().push((id, level));
            }
        }
        self.required_for = required_for;
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

fn bonuses_from(list: &[SdeBonus]) -> Vec<Bonus> {
    let mut sorted: Vec<&SdeBonus> = list.iter().collect();
    sorted.sort_by_key(|b| b.importance);
    sorted
        .into_iter()
        .map(|b| Bonus {
            number: b.bonus.map(|v| bonus_number(v, b.unit_id)),
            text: b.bonus_text.en.clone(),
        })
        .collect()
}

fn traits_from(bonus: &SdeTypeBonus) -> Traits {
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

fn read_cache(path: &Path, key: &str) -> Option<ItemData> {
    let bytes = std::fs::read(path).ok()?;
    let cache: CacheFile = postcard::from_bytes(&bytes).ok()?;
    (cache.key == key).then_some(cache.data)
}

/// Best effort: without a cache the next use just builds the data again.
fn write_cache(path: &Path, key: &str, data: &ItemData) {
    let Ok(bytes) = postcard::to_stdvec(&CacheFileRef { key, data }) else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    // Per process, so two instances writing at once never interleave in one file.
    let tmp = path.with_extension(format!("bin.{}.tmp", std::process::id()));
    if std::fs::write(&tmp, bytes).is_ok() {
        let _ = std::fs::rename(&tmp, path);
    }
}

#[derive(Deserialize)]
struct CacheFile {
    key: String,
    data: ItemData,
}

/// [`CacheFile`] for writing without copying; postcard only cares about field order.
#[derive(Serialize)]
struct CacheFileRef<'a> {
    key: &'a str,
    data: &'a ItemData,
}

fn read<T: DeserializeOwned>(dir: &Path, file: &str) -> Result<T, String> {
    let path = dir.join(file);
    let reader = File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_reader(BufReader::with_capacity(1 << 20, reader))
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[derive(Deserialize, Default)]
struct Localized {
    #[serde(default)]
    en: String,
}

#[derive(Deserialize)]
struct SdeAttr {
    name: String,
    #[serde(rename = "displayName")]
    display_name: Option<Localized>,
    #[serde(rename = "unitID")]
    unit_id: Option<i32>,
    #[serde(rename = "iconID")]
    icon_id: Option<i32>,
    #[serde(default)]
    published: bool,
}

#[derive(Deserialize)]
struct SdeUnit {
    name: String,
    #[serde(rename = "displayName")]
    display_name: Option<Localized>,
}

#[derive(Deserialize)]
struct SdeEffect {
    name: String,
    #[serde(rename = "effectCategoryID", default)]
    effect_category_id: i32,
}

#[derive(Deserialize)]
struct TypeDogma {
    #[serde(rename = "dogmaAttributes", default)]
    attributes: Vec<DogmaAttr>,
    #[serde(rename = "dogmaEffects", default)]
    effects: Vec<DogmaEffect>,
}

#[derive(Deserialize)]
struct DogmaAttr {
    #[serde(rename = "attributeID")]
    attribute_id: i32,
    value: f64,
}

#[derive(Deserialize)]
struct DogmaEffect {
    #[serde(rename = "effectID")]
    effect_id: i32,
    #[serde(rename = "isDefault", default)]
    is_default: bool,
}

#[derive(Deserialize)]
struct SdeTypeBonus {
    #[serde(default)]
    types: Vec<SkillBonuses>,
    #[serde(rename = "roleBonuses", default)]
    role_bonuses: Vec<SdeBonus>,
    #[serde(rename = "miscBonuses", default)]
    misc_bonuses: Vec<SdeBonus>,
}

#[derive(Deserialize)]
struct SkillBonuses {
    #[serde(rename = "_key")]
    key: TypeId,
    #[serde(rename = "_value", default)]
    value: Vec<SdeBonus>,
}

#[derive(Deserialize)]
struct SdeBonus {
    bonus: Option<f64>,
    #[serde(rename = "bonusText", default)]
    bonus_text: Localized,
    #[serde(default)]
    importance: i32,
    #[serde(rename = "unitID")]
    unit_id: Option<i32>,
}

#[derive(Deserialize)]
struct SdeType {
    description: Option<Localized>,
    #[serde(rename = "basePrice")]
    base_price: Option<f64>,
    #[serde(rename = "portionSize")]
    portion_size: Option<i32>,
    #[serde(rename = "techLevel")]
    tech_level: Option<i32>,
    #[serde(rename = "raceID")]
    race_id: Option<i32>,
    #[serde(rename = "factionID")]
    faction_id: Option<i32>,
    mass: Option<f64>,
    capacity: Option<f64>,
    volume: Option<f64>,
    radius: Option<f64>,
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
