//! One fitted item looked at closely, for the item stats window: every attribute before and after
//! bonuses, every modification with where it comes from, and which effects run.
//!
//! The fit is built the way [`Engine::compute_with`] builds it, but every item's ID is kept, so a
//! modification can be traced back to the module, drone or skill it came from.

use std::collections::HashMap;

use rc::{
    AddMode, AttrId, Item, ItemCommon, ItemId, ItemTypeId, MinionState, ModRack, Op, SolOwned,
};

use super::{Engine, Pilot, add_fit_drone, add_fit_module, pane};
use crate::{
    data::{Slot, TypeId},
    fit::Fit,
};

/// An item of a fit, by where it sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FitItem {
    Ship,
    /// A tactical destroyer's mode.
    Mode,
    Module(Slot, usize),
    /// The charge loaded in a module.
    Charge(Slot, usize),
    Rig(usize),
    Subsystem(usize),
    /// A drone stack; all its drones are the same, so the first stands for them.
    Drone(usize),
    /// Implants and boosters take one per slot, so the type names them.
    Implant(TypeId),
    Booster(TypeId),
    /// A fighter squadron, by its place in the fit's list.
    Fighter(usize),
}

impl FitItem {
    /// The type at this place in `fit`, if anything is there.
    /// `character_implants` are the pilot's, for fits that use them.
    pub fn type_in(self, fit: &Fit, character_implants: &[TypeId]) -> Option<TypeId> {
        match self {
            Self::Ship => fit.ship,
            Self::Mode => fit.mode,
            Self::Module(slot, i) => fit.rack(slot).get(i)?.as_ref().map(|m| m.type_id),
            Self::Charge(slot, i) => fit.rack(slot).get(i)?.as_ref()?.charge,
            Self::Rig(i) => *fit.rigs.get(i)?,
            Self::Subsystem(i) => fit.subsystems.get(i).copied(),
            Self::Drone(i) => fit.drones.get(i).map(|d| d.type_id),
            Self::Implant(t) => fit
                .applied_implants(character_implants)
                .any(|(i, _)| i == t)
                .then_some(t),
            Self::Booster(t) => fit.boosters.iter().any(|b| b.type_id == t).then_some(t),
            Self::Fighter(i) => fit.fighters.get(i).map(|f| f.type_id),
        }
    }
}

/// What kind of item a modification comes from, in the order pyfa lists them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SourceKind {
    Ship,
    Mode,
    /// Modules, rigs, subsystems and charges: pyfa counts them all as modules.
    Module,
    Drone,
    Fighter,
    Implant,
    Booster,
    Skill,
    /// The character, and effects of the environment.
    Other,
}

/// The item a modification comes from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Source {
    /// Refine's ID, unique per item: two copies of a module are two sources.
    pub item_id: u32,
    pub type_id: TypeId,
    pub kind: SourceKind,
    /// Where it sits in the fit, so its own stats can be opened.
    pub item: Option<FitItem>,
    /// It belongs to another fit and is projected onto this one.
    pub projected: bool,
}

/// How a modification combines with the value, in pyfa's terms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModOp {
    /// Sets the value before anything else applies.
    Assign,
    Add,
    Multiply,
    /// Sets the value and overrides everything else.
    Force,
    /// Keeps the value at or above the amount.
    Min,
    /// Keeps the value at or below the amount.
    Max,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Modifier {
    pub op: ModOp,
    /// What is added, or what the value is multiplied by or set to, after resistances and range
    /// but before the stacking penalty, as pyfa shows it.
    pub amount: f64,
    /// It counts towards a stacking penalty.
    pub penalized: bool,
    /// The target's resistance weakened it.
    pub resisted: bool,
    pub source: Source,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttrValue {
    pub id: i32,
    pub base: f64,
    pub modified: f64,
}

/// The item's attributes and what changes them.
#[derive(Debug, Clone, Default)]
pub struct Inspection {
    pub type_id: TypeId,
    pub attrs: Vec<AttrValue>,
    /// Modifications per attribute, by attribute ID.
    pub modifiers: Vec<(i32, Vec<Modifier>)>,
    /// Dogma effects with whether each one runs right now.
    pub effects: Vec<(i32, bool)>,
}

impl Inspection {
    pub fn attr(&self, id: i32) -> Option<&AttrValue> {
        self.attrs.iter().find(|a| a.id == id)
    }
}

impl Engine {
    /// The item at `item` in `fit`, after all bonuses. None when nothing is there.
    pub fn inspect(&self, fit: &Fit, pilot: &Pilot, item: FitItem) -> Option<Inspection> {
        let type_id = item.type_in(fit, &pilot.implants)?;
        let mut sol = SolOwned::new(&self.src);
        let (fit_id, places) = Self::build(&mut sol, fit, pilot)?;
        let target = places
            .iter()
            .find(|&(_, &place)| place == item)
            .map(|(&id, _)| id)?;

        let mut it = sol.get_item(&target).ok()?;
        let attrs: Vec<AttrValue> = it
            .iter_attrs()
            .ok()?
            .filter_map(|(attr, values)| match attr {
                AttrId::Eve(id) => Some(AttrValue {
                    id: id.into_i32(),
                    base: values.base.into_f64(),
                    modified: values.modified.into_f64(),
                }),
                AttrId::Custom(_) => None,
            })
            .collect();
        let effects = it.iter_effects().map_or_default(|effects| {
            effects
                .filter_map(|(effect, info)| match effect {
                    rc::EffectId::Dogma(id) => Some((id.into_i32(), info.running)),
                    _ => None,
                })
                .collect()
        });
        let raw: Vec<(i32, Vec<rc::Modification>)> = it.iter_modifiers().map_or_default(|mods| {
            mods.filter_map(|(attr, mods)| match attr {
                AttrId::Eve(id) => Some((id.into_i32(), mods.collect())),
                AttrId::Custom(_) => None,
            })
            .collect()
        });

        let mut sources: HashMap<ItemId, Option<Source>> = HashMap::new();
        let mut modifiers = Vec::with_capacity(raw.len());
        for (attr, mods) in raw {
            let mut list = Vec::new();
            for m in mods {
                let (op, amount) = op_and_amount(&m);
                let penalized = m.stacking_mult.is_some();
                let resisted = m.resist_mult.is_some_and(|r| r.into_f64() < 1.0);
                for affector in &m.affectors {
                    let source = *sources.entry(affector.item_id).or_insert_with(|| {
                        source(
                            &mut sol,
                            affector.item_id,
                            fit_id,
                            places.get(&affector.item_id),
                        )
                    });
                    if let Some(source) = source {
                        list.push(Modifier {
                            op,
                            amount,
                            penalized,
                            resisted,
                            source,
                        });
                    }
                }
            }
            if !list.is_empty() {
                modifiers.push((attr, list));
            }
        }
        Some(Inspection {
            type_id,
            attrs,
            modifiers,
            effects,
        })
    }

    /// Put `fit` into `sol` the way [`Engine::compute_for`] does, keeping where each item went.
    fn build(
        sol: &mut SolOwned,
        fit: &Fit,
        pilot: &Pilot,
    ) -> Option<(rc::FitId, HashMap<ItemId, FitItem>)> {
        let mut places = HashMap::new();
        sol.set_sec_zone(super::sec_zone(fit.system_security));
        let mut f = sol.add_fit();
        pilot.board(&mut f);
        let ship = fit.ship?;
        let ship_id = f
            .set_ship(ItemTypeId::from_i32(ship), None, None)
            .get_item_id();
        places.insert(ship_id, FitItem::Ship);
        for (slot, rack, modules) in [
            (Slot::High, ModRack::High, &fit.high),
            (Slot::Mid, ModRack::Mid, &fit.mid),
            (Slot::Low, ModRack::Low, &fit.low),
        ] {
            for (i, m) in modules.iter().enumerate() {
                let Some(m) = m else { continue };
                let Ok(mut module) = add_fit_module(
                    &mut f,
                    rack,
                    AddMode::Replace(rc::RackPos::from_usize_clamped(i)),
                    m,
                ) else {
                    continue;
                };
                places.insert(module.get_item_id(), FitItem::Module(slot, i));
                if let Some(charge) = module.get_charge() {
                    places.insert(charge.get_item_id(), FitItem::Charge(slot, i));
                }
            }
        }
        if let Some(mode) = fit.mode {
            let id = f.set_stance(ItemTypeId::from_i32(mode)).get_item_id();
            places.insert(id, FitItem::Mode);
        }
        f.set_sec_status(rc::FitSecStatus::from_f64_clamped(
            fit.sec_status.unwrap_or(pilot.sec_status),
        ));
        for (i, rig) in fit.rigs.iter().enumerate() {
            if let Some(rig) = rig {
                let id = f.add_rig(ItemTypeId::from_i32(*rig)).get_item_id();
                places.insert(id, FitItem::Rig(i));
            }
        }
        for (i, id) in super::add_services(&mut f, fit) {
            places.insert(id, FitItem::Module(Slot::Service, i));
        }
        for (i, &subsystem) in fit.subsystems.iter().enumerate() {
            let id = f
                .add_subsystem(ItemTypeId::from_i32(subsystem))
                .get_item_id();
            places.insert(id, FitItem::Subsystem(i));
        }
        for (n, stack) in fit.drones.iter().enumerate() {
            for i in 0..stack.count {
                let state = if i < stack.active {
                    MinionState::Engaging
                } else {
                    MinionState::InBay
                };
                let id = add_fit_drone(&mut f, stack, state).get_item_id();
                places.insert(id, FitItem::Drone(n));
            }
        }
        for (implant, active) in fit.applied_implants(&pilot.implants) {
            let mut item = f.add_implant(ItemTypeId::from_i32(implant));
            if !active {
                item.set_state(false);
            }
            places.insert(item.get_item_id(), FitItem::Implant(implant));
        }
        for booster in &fit.boosters {
            let mut item = f.add_booster(ItemTypeId::from_i32(booster.type_id));
            item.set_state(booster.active);
            for &effect in &booster.side_effects {
                let id = rc::EffectId::Dogma(rc::DogmaEffectId::from_i32(effect));
                if let Ok(mut side_effect) = item.get_side_effect(&id) {
                    side_effect.set_state(true);
                }
            }
            places.insert(item.get_item_id(), FitItem::Booster(booster.type_id));
        }
        for (i, id) in super::surroundings::add_fighters(&mut f, &fit.fighters)
            .into_iter()
            .enumerate()
        {
            places.insert(id, FitItem::Fighter(i));
        }
        let incoming = pane::dps_profile(pane::incoming_mix(&mut f, &fit.damage_profile));
        let rah = super::rah_incoming(&mut f, fit, incoming);
        f.set_rah_incoming_dps(Some(rah));
        Some((f.get_fit_id(), places))
    }
}

/// Describe the item behind an affector. None for items gone from the solar system.
fn source(
    sol: &mut SolOwned,
    id: ItemId,
    fit_id: rc::FitId,
    place: Option<&FitItem>,
) -> Option<Source> {
    let item = sol.get_item(&id).ok()?;
    let kind = match &item {
        Item::Ship(_) => SourceKind::Ship,
        Item::Stance(_) => SourceKind::Mode,
        Item::Module(_) | Item::Rig(_) | Item::Subsystem(_) | Item::Charge(_) => SourceKind::Module,
        Item::Drone(_) => SourceKind::Drone,
        Item::Fighter(_) | Item::Autocharge(_) => SourceKind::Fighter,
        Item::Implant(_) => SourceKind::Implant,
        Item::Booster(_) => SourceKind::Booster,
        Item::Skill(_) => SourceKind::Skill,
        _ => SourceKind::Other,
    };
    let projected = item.get_fit_id().is_some_and(|f| f != fit_id);
    Some(Source {
        item_id: item_number(id),
        type_id: item.get_type_id().into_i32(),
        kind,
        item: place.copied(),
        projected,
    })
}

/// A number for an item ID that is stable within one inspection.
fn item_number(id: ItemId) -> u32 {
    id.to_string().parse().unwrap_or_default()
}

/// pyfa shows percentages, divisions and multiplications all as a multiplier, and subtractions as
/// negative additions.
fn op_and_amount(m: &rc::Modification) -> (ModOp, f64) {
    let initial = m.initial_str.into_f64();
    let applied = m.applied_str.into_f64();
    let scale = m.range_mult.map_or(1.0, rc::PValue::into_f64)
        * m.resist_mult.map_or(1.0, rc::UnitInterval::into_f64);
    let mult = |factor: f64| 1.0 + (factor - 1.0) * scale;
    match m.op {
        Op::BaseAssign | Op::PreAssign => (ModOp::Assign, applied),
        Op::PostAssign => (ModOp::Force, applied),
        Op::Add | Op::ExtraAdd => (ModOp::Add, applied),
        Op::Sub => (ModOp::Add, -applied),
        Op::PreMul | Op::PostMul | Op::ExtraMul => (ModOp::Multiply, mult(initial)),
        Op::PreDiv | Op::PostDiv => (
            ModOp::Multiply,
            if initial == 0.0 {
                1.0
            } else {
                mult(1.0 / initial)
            },
        ),
        Op::PostPerc => (ModOp::Multiply, 1.0 + initial / 100.0 * scale),
        Op::MinLimit => (ModOp::Min, applied),
        Op::MaxLimit => (ModOp::Max, applied),
    }
}
