//! Who flies a fit: the character's skills, implants and security status, and which of the skills
//! change an item.

use std::collections::{BTreeSet, HashMap};

use rc::{
    AddMode, AttrId, ItemCommon, ItemId, ItemTypeId, MinionState, ModRack, SkillLevel, SolOwned,
};

use super::{CHARACTER, Engine};
use crate::data::{Slot, TypeId};

/// Highest level a skill trains to.
pub const MAX_LEVEL: u8 = 5;

/// The character as the engine needs it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Pilot {
    /// Learned skills and their levels. Skills missing here are not learned and do nothing.
    pub skills: Vec<(TypeId, u8)>,
    /// Implants plugged into the character, for fits that use the character's implants.
    pub implants: Vec<TypeId>,
    /// Used where the fit does not set its own.
    pub sec_status: f64,
    /// An Alpha clone's highest level per skill; skills it lacks fly at 0. None for Omega.
    pub alpha_caps: Option<HashMap<TypeId, u8>>,
}

impl Pilot {
    /// Every skill at one level, like pyfa's All 0 and All 5.
    pub fn all(skills: &[TypeId], level: u8) -> Self {
        Self {
            skills: skills.iter().map(|&s| (s, level)).collect(),
            implants: Vec::new(),
            sec_status: 0.0,
            alpha_caps: None,
        }
    }

    /// The level a skill is flown at: the trained level, capped for an Alpha clone the way
    /// pyfa's `Skill.level` caps it.
    pub fn flown_level(&self, skill: TypeId, level: u8) -> u8 {
        Self::capped(self.alpha_caps.as_ref(), skill, level)
    }

    /// A skill's level under an Alpha clone's `caps`, where skills it lacks fly at 0; None caps
    /// nothing, as for Omega.
    pub fn capped(caps: Option<&HashMap<TypeId, u8>>, skill: TypeId, level: u8) -> u8 {
        caps.map_or(level, |caps| {
            level.min(caps.get(&skill).copied().unwrap_or(0))
        })
    }

    /// Put the character and its skills on a fit.
    pub(super) fn board(&self, fit: &mut rc::Fit<'_>) {
        fit.set_character(ItemTypeId::from_i32(CHARACTER));
        for &(skill, level) in &self.skills {
            let level = self.flown_level(skill, level);
            let _ = fit.add_skill(
                ItemTypeId::from_i32(skill),
                SkillLevel::from_i32_clamped(i32::from(level)),
            );
        }
    }
}

/// An item whose affecting skills are asked for, as pyfa's "Change Skills" menus see it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillTarget {
    Ship,
    Module(Slot, TypeId),
    /// A charge, loaded into a module of the given type.
    Charge {
        slot: Slot,
        module: TypeId,
        charge: TypeId,
    },
    Drone(TypeId),
    Fighter(TypeId),
}

impl Engine {
    /// Skills that modify a nonzero attribute of the target, like pyfa's skillAffectors menu.
    ///
    /// Every skill is trained to V for this, so skills the character lacks still show up: they
    /// are the ones a pilot would want to train.
    pub fn affecting_skills(&self, ship: Option<TypeId>, target: SkillTarget) -> Vec<TypeId> {
        let mut sol = self.sol();
        let mut f = sol.add_fit();
        self.all5.board(&mut f);
        let ship_id = ship.map(|ship| {
            f.set_ship(ItemTypeId::from_i32(ship), None, None)
                .get_item_id()
        });
        let target_id: Option<ItemId> = match target {
            SkillTarget::Ship => ship_id,
            SkillTarget::Module(slot, module) => add_module(&mut f, slot, module, None),
            SkillTarget::Charge {
                slot,
                module,
                charge,
            } => add_module(&mut f, slot, module, Some(charge)),
            SkillTarget::Drone(drone) => Some(
                f.add_drone(ItemTypeId::from_i32(drone), MinionState::InBay, None, None)
                    .get_item_id(),
            ),
            SkillTarget::Fighter(fighter) => Some(
                f.add_fighter(
                    ItemTypeId::from_i32(fighter),
                    MinionState::InBay,
                    None,
                    None,
                )
                .get_item_id(),
            ),
        };
        // The charge goes in with its module; ask the module for it once the fit is built.
        let target_id = match target {
            SkillTarget::Charge { .. } => target_id.and_then(|module| {
                sol.get_module(&module)
                    .ok()?
                    .get_charge()
                    .map(|c| c.get_item_id())
            }),
            _ => target_id,
        };
        let Some(target_id) = target_id else {
            return Vec::new();
        };
        affectors(&mut sol, target_id)
            .into_iter()
            .filter_map(|id| {
                let item = sol.get_item(&id).ok()?;
                matches!(item, rc::Item::Skill(_)).then(|| item.get_type_id().into_i32())
            })
            .collect::<BTreeSet<TypeId>>()
            .into_iter()
            .collect()
    }
}

/// Items behind the modifications of the target's nonzero attributes.
fn affectors(sol: &mut SolOwned, target: ItemId) -> Vec<ItemId> {
    let Ok(mut item) = sol.get_item(&target) else {
        return Vec::new();
    };
    let modified: Vec<(AttrId, Vec<ItemId>)> = match item.iter_modifiers() {
        Ok(mods) => mods
            .map(|(attr, mods)| {
                let ids = mods
                    .flat_map(|m| m.affectors.into_iter().map(|a| a.item_id))
                    .collect();
                (attr, ids)
            })
            .collect(),
        Err(_) => return Vec::new(),
    };
    // pyfa leaves out attributes that end up at zero.
    modified
        .into_iter()
        .filter(|(attr, _)| {
            item.get_attr(attr)
                .is_ok_and(|v| v.modified.into_f64() != 0.0)
        })
        .flat_map(|(_, ids)| ids)
        .collect()
}

fn add_module(
    f: &mut rc::Fit<'_>,
    slot: Slot,
    module: TypeId,
    charge: Option<TypeId>,
) -> Option<ItemId> {
    let type_id = ItemTypeId::from_i32(module);
    let rack = match slot {
        Slot::High => ModRack::High,
        Slot::Mid => ModRack::Mid,
        Slot::Low => ModRack::Low,
        Slot::Rig => return Some(f.add_rig(type_id).get_item_id()),
        Slot::Subsystem => return Some(f.add_subsystem(type_id).get_item_id()),
        Slot::Service => {
            return Some(
                f.add_service(type_id, rc::ServiceState::Online)
                    .get_item_id(),
            );
        }
    };
    let mut added = f
        .add_module(rack, AddMode::Append, type_id, rc::ModuleState::Online)
        .ok()?;
    if let Some(charge) = charge {
        added.set_charge_type_id(ItemTypeId::from_i32(charge));
    }
    Some(added.get_item_id())
}
