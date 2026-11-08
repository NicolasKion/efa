//! The dump's JSON files as serde reads them, for the catalog and the item stats data alike. Maps
//! keyed by ID come in keyed by number.

use std::{fs::File, io::BufReader, path::Path};

use serde::{Deserialize, de::DeserializeOwned};

use super::TypeId;

/// requiredSkill1..6 and the level attribute that goes with each, in the game's order.
pub const REQUIRED_SKILL_ATTRS: [(i32, i32); 6] = [
    (182, 277),
    (183, 278),
    (184, 279),
    (1285, 1286),
    (1289, 1287),
    (1290, 1288),
];

/// A file of the dump in `dir`, parsed.
pub fn read<T: DeserializeOwned>(dir: &Path, file: &str) -> Result<T, String> {
    let path = dir.join(file);
    let reader = File::open(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_reader(BufReader::with_capacity(1 << 20, reader))
        .map_err(|e| format!("{}: {e}", path.display()))
}

#[derive(Debug, Default, Deserialize)]
pub struct Localized {
    #[serde(default)]
    pub en: String,
}

/// A type's dogma attributes and effects, from typedogma.json.
#[derive(Debug, Deserialize)]
pub struct TypeDogma {
    #[serde(rename = "dogmaAttributes", default)]
    pub attributes: Vec<DogmaAttr>,
    #[serde(rename = "dogmaEffects", default)]
    pub effects: Vec<DogmaEffect>,
}

impl TypeDogma {
    pub fn attr(&self, id: i32) -> Option<f64> {
        self.attributes
            .iter()
            .find(|a| a.attribute_id == id)
            .map(|a| a.value)
    }

    pub fn has_effect(&self, id: i32) -> bool {
        self.effects.iter().any(|e| e.effect_id == id)
    }
}

#[derive(Debug, Deserialize)]
pub struct DogmaAttr {
    #[serde(rename = "attributeID")]
    pub attribute_id: i32,
    pub value: f64,
}

#[derive(Debug, Deserialize)]
pub struct DogmaEffect {
    #[serde(rename = "effectID")]
    pub effect_id: i32,
    /// A number in Phobos' files, a boolean in the SDE's.
    #[serde(rename = "isDefault", default, deserialize_with = "flag")]
    pub is_default: bool,
}

/// A boolean written as `true` or as `1`.
fn flag<'de, D: serde::Deserializer<'de>>(de: D) -> Result<bool, D::Error> {
    struct Visitor;
    impl serde::de::Visitor<'_> for Visitor {
        type Value = bool;
        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("a boolean or a number")
        }
        fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<bool, E> {
            Ok(v)
        }
        fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<bool, E> {
            Ok(v != 0)
        }
        fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<bool, E> {
            Ok(v != 0)
        }
    }
    de.deserialize_any(Visitor)
}

/// dogmaattributes.json.
#[derive(Debug, Deserialize)]
pub struct Attr {
    pub name: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<Localized>,
    #[serde(rename = "unitID")]
    pub unit_id: Option<i32>,
    #[serde(rename = "iconID")]
    pub icon_id: Option<i32>,
    #[serde(default)]
    pub published: bool,
    #[serde(rename = "highIsGood", default)]
    pub high_is_good: bool,
}

/// dogmaunits.json.
#[derive(Debug, Deserialize)]
pub struct Unit {
    pub name: String,
    #[serde(rename = "displayName")]
    pub display_name: Option<Localized>,
}

/// The SDE's dogmaeffects.json, with names.
#[derive(Debug, Deserialize)]
pub struct Effect {
    pub name: String,
    #[serde(rename = "effectCategoryID", default)]
    pub effect_category_id: i32,
}

/// Phobos' dogmaeffects.json, which says which effects come by chance.
#[derive(Debug, Deserialize)]
pub struct PhbEffect {
    #[serde(rename = "effectCategory", default)]
    pub effect_category: i32,
    #[serde(rename = "fittingUsageChanceAttributeID", default)]
    pub fitting_usage_chance_attribute_id: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct Group {
    #[serde(rename = "categoryID")]
    pub category_id: i32,
    pub name: Localized,
}

#[derive(Debug, Deserialize)]
pub struct MarketGroup {
    pub name: Localized,
    #[serde(rename = "parentGroupID")]
    pub parent_group_id: Option<i32>,
    #[serde(rename = "iconID")]
    pub icon_id: Option<i32>,
}

#[derive(Debug, Deserialize)]
pub struct Icon {
    #[serde(rename = "iconFile")]
    pub icon_file: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Graphic {
    #[serde(rename = "iconFolder")]
    pub icon_folder: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MetaGroup {
    pub name: Localized,
}

/// What the catalog reads of types.json.
#[derive(Debug, Deserialize)]
pub struct Type {
    pub name: Localized,
    #[serde(rename = "groupID")]
    pub group_id: i32,
    #[serde(rename = "marketGroupID")]
    pub market_group_id: Option<i32>,
    #[serde(default)]
    pub published: bool,
    #[serde(rename = "iconID")]
    pub icon_id: Option<i32>,
    #[serde(rename = "graphicID")]
    pub graphic_id: Option<i32>,
    #[serde(rename = "variationParentTypeID")]
    pub variation_parent_type_id: Option<i32>,
    #[serde(rename = "metaGroupID")]
    pub meta_group_id: Option<i32>,
    #[serde(rename = "metaLevel")]
    pub meta_level: Option<i32>,
    pub volume: Option<f64>,
    #[serde(rename = "packagedVolume")]
    pub packaged_volume: Option<f64>,
    pub capacity: Option<f64>,
    #[serde(rename = "factionID")]
    pub faction_id: Option<i32>,
    #[serde(rename = "raceID")]
    pub race_id: Option<i32>,
}

/// What the item stats data reads of types.json: descriptions and the columns pyfa lists as
/// attributes.
#[derive(Debug, Deserialize)]
pub struct TypeDetails {
    pub description: Option<Localized>,
    #[serde(rename = "basePrice")]
    pub base_price: Option<f64>,
    #[serde(rename = "portionSize")]
    pub portion_size: Option<i32>,
    #[serde(rename = "techLevel")]
    pub tech_level: Option<i32>,
    #[serde(rename = "raceID")]
    pub race_id: Option<i32>,
    #[serde(rename = "factionID")]
    pub faction_id: Option<i32>,
    pub mass: Option<f64>,
    pub capacity: Option<f64>,
    pub volume: Option<f64>,
    pub radius: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct CloneGrade {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub skills: Vec<CloneSkill>,
}

#[derive(Debug, Deserialize)]
pub struct CloneSkill {
    #[serde(rename = "typeID")]
    pub type_id: TypeId,
    pub level: u8,
}

/// typebonus.json: a ship's or subsystem's traits.
#[derive(Debug, Deserialize)]
pub struct TypeBonus {
    #[serde(default)]
    pub types: Vec<SkillBonuses>,
    #[serde(rename = "roleBonuses", default)]
    pub role_bonuses: Vec<Bonus>,
    #[serde(rename = "miscBonuses", default)]
    pub misc_bonuses: Vec<Bonus>,
}

#[derive(Debug, Deserialize)]
pub struct SkillBonuses {
    #[serde(rename = "_key")]
    pub key: TypeId,
    #[serde(rename = "_value", default)]
    pub value: Vec<Bonus>,
}

#[derive(Debug, Deserialize)]
pub struct Bonus {
    #[serde(rename = "bonus")]
    pub amount: Option<f64>,
    #[serde(rename = "bonusText", default)]
    pub text: Localized,
    #[serde(default)]
    pub importance: i32,
    #[serde(rename = "unitID")]
    pub unit_id: Option<i32>,
}

/// masteries.json: per ship, its mastery levels, each a list of certificate IDs.
#[derive(Debug, Deserialize)]
pub struct Mastery {
    #[serde(rename = "_value")]
    pub levels: Vec<MasteryLevel>,
}

#[derive(Debug, Deserialize)]
pub struct MasteryLevel {
    #[serde(rename = "_value")]
    pub certificates: Vec<i32>,
}

#[derive(Debug, Deserialize)]
pub struct Certificate {
    pub name: Localized,
}

/// fighterabilities.json.
#[derive(Debug, Deserialize)]
pub struct FighterAbility {
    #[serde(rename = "displayName")]
    pub display_name: Option<Localized>,
}
