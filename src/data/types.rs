//! The item facts the catalog holds: types with their kind, slot and hardpoint, market groups and
//! Alpha clone grades.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use super::{TypeId, category};
use crate::ship_browser::Race;

/// Which kind of hardpoint a module takes, if any.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Hardpoint {
    None,
    Turret,
    Launcher,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub enum Slot {
    High,
    Mid,
    Low,
    Rig,
    Subsystem,
    /// An Upwell structure's service modules.
    Service,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kind {
    Ship,
    Module(Slot),
    Charge,
    Drone,
    Fighter,
    Implant,
    Booster,
    Skill,
    /// A tactical destroyer's mode.
    Mode,
    /// Something a ship only carries: commodities, materials, deployables, containers.
    Cargo,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent facts read from the data, not a state machine"
)]
pub struct ItemType {
    pub id: TypeId,
    pub name: String,
    /// The name in lower case, for filters that match a piece of it.
    pub lower_name: String,
    /// Where the item comes in market order; see [`super::order::rank`].
    pub market_rank: u32,
    pub group_id: i32,
    pub category_id: i32,
    pub market_group: Option<i32>,
    pub published: bool,
    pub kind: Kind,
    pub activatable: bool,
    pub overloadable: bool,
    pub charge_groups: Vec<i32>,
    pub charge_size: Option<f64>,
    /// Client path of the icon art, a `res:/` path on CCP's CDN.
    pub icon: Option<String>,
    /// The type this one is a variation of (the T1 base), if any.
    pub variation_parent: Option<TypeId>,
    pub meta_group: Option<i32>,
    pub meta_level: i32,
    /// Packaged volume where there is one, in m³.
    pub volume: f64,
    pub hardpoint: Hardpoint,
    /// EM, thermal, kinetic, explosive damage, for charges.
    pub damage: [f64; 4],
    pub range_multiplier: Option<f64>,
    pub falloff_multiplier: Option<f64>,
    /// Unmodified optimal and falloff, for sorting a turret's charges by range.
    pub optimal: Option<f64>,
    pub falloff: Option<f64>,
    pub mining: bool,
    /// Whether this module spools up over cycles.
    pub spools: bool,
    /// Bandwidth a drone takes while in space, in Mbit/s.
    pub drone_bandwidth: f64,
    /// Unmodified signature radius, which a bomb's explosion is sized by.
    pub signature_radius: Option<f64>,
    /// Slot a booster goes into; a fit holds one booster per slot.
    pub booster_slot: Option<u32>,
    /// Chance of a booster's first side effect, missing on boosters without one.
    pub booster_chance: Option<f64>,
    /// Effects that may come with a booster, by effect ID: the ones with a usage chance.
    pub side_effects: Vec<i32>,
    /// Which implant slot an implant plugs into; a character takes one implant per slot.
    pub implant_slot: Option<u8>,
    /// What the item holds, in m³: charges for a module, contents for a container.
    pub capacity: f64,
    /// Who builds a ship, for the fittings browser's race filter.
    pub race: Option<Race>,
    /// Needs a skill level beyond what an Alpha clone can train.
    pub omega: bool,
    /// Rig size of a rig, or of the rigs a ship takes.
    pub rig_size: Option<f64>,
    /// Ships and ship groups the item is limited to; empty lists mean no limit.
    pub fits_ships: Vec<TypeId>,
    pub fits_groups: Vec<i32>,
}

impl ItemType {
    /// Whether this is an Upwell structure rather than a ship.
    pub fn is_structure(&self) -> bool {
        self.category_id == category::STRUCTURE
    }

    /// The word pyfa puts into menu labels for this kind of item.
    pub fn category_label(&self) -> &'static str {
        match self.kind {
            Kind::Ship if self.is_structure() => "Citadel",
            Kind::Ship => "Ship",
            Kind::Module(Slot::Subsystem) => "Subsystem",
            Kind::Module(Slot::Service) => "Service",
            Kind::Module(_) => "Module",
            Kind::Charge => "Charge",
            Kind::Drone => "Drone",
            Kind::Fighter => "Fighter",
            Kind::Implant => "Implant",
            Kind::Booster => "Booster",
            Kind::Skill => "Skill",
            Kind::Mode => "Tactical Mode",
            Kind::Cargo => match self.category_id {
                category::COMMODITY => "Commodity",
                category::MATERIAL => "Material",
                category::DEPLOYABLE => "Deployable",
                _ => "Celestial",
            },
            Kind::Other => "Item",
        }
    }

    /// Whether this charge fits in a module.
    pub fn fits_in(&self, module: &ItemType) -> bool {
        self.kind == Kind::Charge
            && module.charge_groups.contains(&self.group_id)
            && match module.charge_size {
                Some(size) => self.charge_size == Some(size),
                None => true,
            }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketGroup {
    pub id: i32,
    pub name: String,
    pub parent: Option<i32>,
    pub children: Vec<i32>,
    pub types: Vec<TypeId>,
    pub icon_id: Option<i32>,
}

/// An Alpha clone grade: the skills it can use and the highest level of each.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AlphaClone {
    pub id: i32,
    pub name: String,
    #[serde(serialize_with = "crate::sorted::map")]
    pub caps: HashMap<TypeId, u8>,
}
