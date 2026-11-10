//! Everything around the fit that refine has to see in the same solar system: its fighters, the
//! fits boosting it from its fleet, fits and loose items projecting onto it, and environmental
//! effects.
//!
//! Loose projected modules, drones and fighters have no fit of their own in pyfa. refine needs one
//! to hold them, so they go into hidden fits flying a capsule, which carries no bonuses that could
//! change them, one per distance from the target. A fit projected more than once is added once
//! per copy, since refine has no multiplier for projections.

use rc::{
    AbilityId, CountNz, ItemCommon, ItemId, ItemTypeId, Lender, MinionState, ModRack, SolOwned,
    stats::StatSlot,
};
use std::borrow::Borrow;

use super::{
    DroneInfo, Engine, Pilot, Slots,
    build::{add_fit, add_fit_module},
    drone_info, read,
};
use crate::{
    data::Slot,
    dogma::attr,
    fit::{
        Fit, Module, ModuleState,
        fighters::Fighter,
        projected::{EnvEffect, ProjectedModule},
    },
};

/// The capsule: a ship item with nothing on it that bonuses modules or drones.
pub(super) const CAPSULE: i32 = 670;

/// Fighter abilities pyfa leaves off when a squadron has no attack to start with.
const PROPULSION_ABILITIES: [i32; 2] = [4, 13];

/// What the Fighters tab and the projected fighter rows show for a squadron.
#[derive(Debug, Clone, Default)]
pub struct FighterInfo {
    pub abilities: Vec<AbilityInfo>,
    /// Fighters in the squadron now, and at most.
    pub count: Option<(u32, u32)>,
    /// Speed, EHP and regeneration, read the same way as a drone's.
    pub stats: DroneInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbilityInfo {
    pub id: i32,
    pub active: bool,
}

impl FighterInfo {
    pub fn max_size(&self) -> Option<u32> {
        self.count.map(|(_, max)| max)
    }
}

/// Squadrons launched and tubes, per class: light, heavy, support.
#[derive(Debug, Clone, Copy, Default)]
pub struct FighterClasses {
    pub light: Slots,
    pub heavy: Slots,
    pub support: Slots,
}

/// Projected items the stats read back, in the fit's order.
#[derive(Debug, Clone, Default)]
pub(super) struct Projections {
    pub fighters: Vec<ItemId>,
    pub modules: Vec<Option<ItemId>>,
}

/// Launch the fit's own fighters, in its order.
pub(super) fn add_fighters(f: &mut rc::Fit<'_>, fighters: &[Fighter]) -> Vec<ItemId> {
    fighters
        .iter()
        .map(|fighter| add_fighter(f, fighter))
        .collect()
}

fn add_fighter(f: &mut rc::Fit<'_>, fighter: &Fighter) -> ItemId {
    let state = if fighter.active {
        MinionState::Engaging
    } else {
        MinionState::InBay
    };
    let mut item = f.add_fighter(ItemTypeId::from_i32(fighter.type_id), state, None, None);
    if let Some(amount) = fighter.amount {
        item.set_count_override(Some(CountNz::from_u32_clamped(amount)));
    }
    set_abilities(&mut item, fighter);
    item.get_item_id()
}

/// pyfa's ability defaults: the standard attack when the squadron has one, otherwise everything
/// but its propulsion; then whatever the user switched.
fn set_abilities(item: &mut rc::Fighter<'_>, fighter: &Fighter) {
    let mut found: Vec<(i32, bool)> = Vec::new();
    {
        let mut abilities = item.iter_abilities();
        while let Some(ability) = abilities.next() {
            found.push((ability.get_id().into_i32(), ability.get_state()));
        }
    }
    // refine starts with only the default effect running, which is the attack.
    let has_attack = found.iter().any(|&(_, on)| on);
    for (id, refine_default) in found {
        let default =
            has_attack && refine_default || !has_attack && !PROPULSION_ABILITIES.contains(&id);
        let wanted = fighter.ability_override(id).unwrap_or(default);
        if wanted != refine_default
            && let Ok(mut ability) = item.get_ability(&AbilityId::from_i32(id))
        {
            ability.set_state(wanted);
        }
    }
}

pub(super) fn fighter_info(sol: &mut SolOwned, id: ItemId) -> FighterInfo {
    let mut info = FighterInfo {
        stats: drone_info(sol, id),
        ..FighterInfo::default()
    };
    if let Ok(mut fighter) = sol.get_fighter(&id) {
        info.count = fighter
            .get_count()
            .map(|c| (c.current.into_u32(), c.max.into_u32()));
        let mut abilities = fighter.iter_abilities();
        while let Some(ability) = abilities.next() {
            info.abilities.push(AbilityInfo {
                id: ability.get_id().into_i32(),
                active: ability.get_state(),
            });
        }
    }
    info
}

pub(super) fn fighter_classes(sol: &mut SolOwned, fit_id: rc::FitId) -> FighterClasses {
    let Ok(mut f) = sol.get_fit(&fit_id) else {
        return FighterClasses::default();
    };
    let mut s = f.stats();
    // Structures have their own standup tubes; a ship has none of those.
    let pick = |ship: StatSlot, standup: StatSlot| {
        let standup = super::slots(standup);
        if standup.total.unwrap_or(0) > 0 {
            standup
        } else {
            super::slots(ship)
        }
    };
    FighterClasses {
        light: pick(
            s.get_stat_launched_light_fighters(),
            s.get_stat_launched_st_light_fighters(),
        ),
        heavy: pick(
            s.get_stat_launched_heavy_fighters(),
            s.get_stat_launched_st_heavy_fighters(),
        ),
        support: pick(
            s.get_stat_launched_support_fighters(),
            s.get_stat_launched_st_support_fighters(),
        ),
    }
}

/// The saved fits `fit` is boosted or projected by, which its numbers depend on.
pub fn surrounding_fits<'a, F: Borrow<Fit>>(
    fit: &'a Fit,
    fits: &'a [F],
) -> impl Iterator<Item = &'a F> + 'a {
    let uids = fit
        .command
        .iter()
        .map(|c| c.fit)
        .chain(fit.projected.fits.iter().map(|p| p.fit));
    let mut seen = Vec::new();
    uids.filter(move |&uid| {
        let new = uid != 0 && !seen.contains(&uid);
        seen.push(uid);
        new
    })
    .filter_map(|uid| fits.iter().find(|f| Borrow::<Fit>::borrow(*f).uid == uid))
}

/// The hidden fits holding loose projected items, one per distance from the target: a capsule
/// with All 5's skills, which carries no bonuses that could change what it holds.
struct Holders<'a> {
    pilot: &'a Pilot,
    target_radius: f64,
    /// By the range their items project from, as bits so they compare exactly.
    fits: Vec<(Option<u64>, rc::FitId)>,
}

impl Holders<'_> {
    /// The holder for items `range` metres from the target, added the first time it is asked
    /// for.
    fn at(&mut self, sol: &mut SolOwned, range: Option<f64>) -> rc::FitId {
        let key = range.map(f64::to_bits);
        if let Some(&(_, fit_id)) = self.fits.iter().find(|(k, _)| *k == key) {
            return fit_id;
        }
        let mut f = sol.add_fit();
        self.pilot.board(&mut f);
        let ship = f
            .set_ship(ItemTypeId::from_i32(CAPSULE), None, None)
            .get_item_id();
        let fit_id = f.get_fit_id();
        place_ship(sol, ship, range, self.target_radius);
        self.fits.push((key, fit_id));
        fit_id
    }
}

impl Engine {
    /// Add what surrounds `fit` to the solar system its own items are in. `others` are the saved
    /// fits that projected and command fits refer to.
    pub(super) fn add_surroundings(
        &self,
        sol: &mut SolOwned,
        fit_id: rc::FitId,
        ship_id: ItemId,
        fit: &Fit,
        others: &[&Fit],
    ) -> Projections {
        let find = |uid: u64| others.iter().copied().find(|f| uid != 0 && f.uid == uid);
        let target_radius = read::attr_or_zero(sol, ship_id, attr::RADIUS);

        let boosters: Vec<&Fit> = fit
            .command
            .iter()
            .filter(|c| c.active)
            .filter_map(|c| find(c.fit))
            .collect();
        // Other fits fly All 5 for now; pyfa gives each its own character.
        let mut fleet_fits: Vec<rc::FitId> = boosters
            .into_iter()
            .filter_map(|booster| add_fit(sol, booster, &self.all5))
            .map(|placed| placed.fit_id)
            .collect();
        fleet_fits.extend(super::links::add_link_booster(
            sol,
            &fit.command_links,
            &self.all5,
        ));
        if !fleet_fits.is_empty() {
            let fleet = sol.add_fleet().get_fleet_id();
            for id in fleet_fits.into_iter().chain([fit_id]) {
                if let Ok(mut f) = sol.get_fit(&id) {
                    let _ = f.set_fleet(Some(&fleet));
                }
            }
        }

        for projected in fit.projected.fits.iter().filter(|p| p.active) {
            let Some(source) = find(projected.fit) else {
                continue;
            };
            for _ in 0..projected.amount {
                let Some(placed) = add_fit(sol, source, &self.all5) else {
                    continue;
                };
                place_ship(sol, placed.ship, projected.range, target_radius);
                for &(slot, _, id) in &placed.modules {
                    if slot == Slot::Service {
                        continue;
                    }
                    if let Ok(mut module) = sol.get_module(&id) {
                        let _ = module.add_proj(&ship_id);
                    }
                }
                let engaged = placed
                    .drones
                    .iter()
                    .zip(&source.drones)
                    .flat_map(|(ids, stack)| ids.iter().take(stack.active as usize));
                for &id in engaged {
                    place_minion(sol, id, projected.range, target_radius);
                    if let Ok(mut drone) = sol.get_drone(&id) {
                        let _ = drone.add_proj(&ship_id);
                    }
                }
                for id in placed.launched_fighters(source) {
                    place_minion(sol, id, projected.range, target_radius);
                    if let Ok(mut fighter) = sol.get_fighter(&id) {
                        let _ = fighter.add_proj(&ship_id);
                    }
                }
            }
        }

        let mut holders = Holders {
            pilot: &self.all5,
            target_radius,
            fits: Vec::new(),
        };
        let modules = fit
            .projected
            .modules
            .iter()
            .map(|module| {
                let holder = holders.at(sol, module.range);
                project_module(sol, holder, module, ship_id)
            })
            .collect();

        for stack in &fit.projected.drones {
            let holder = holders.at(sol, stack.range);
            let state = if stack.active {
                MinionState::Engaging
            } else {
                MinionState::InBay
            };
            for _ in 0..stack.count {
                let Ok(mut f) = sol.get_fit(&holder) else {
                    break;
                };
                let id = f
                    .add_drone(ItemTypeId::from_i32(stack.type_id), state, None, None)
                    .get_item_id();
                if stack.active {
                    place_minion(sol, id, stack.range, target_radius);
                    if let Ok(mut drone) = sol.get_drone(&id) {
                        let _ = drone.add_proj(&ship_id);
                    }
                }
            }
        }

        let mut fighter_ids = Vec::with_capacity(fit.projected.fighters.len());
        for projected in &fit.projected.fighters {
            let holder = holders.at(sol, projected.range);
            let Ok(mut f) = sol.get_fit(&holder) else {
                continue;
            };
            let id = add_fighter(&mut f, &projected.fighter);
            if projected.fighter.active {
                place_minion(sol, id, projected.range, target_radius);
                if let Ok(mut fighter) = sol.get_fighter(&id) {
                    let _ = fighter.add_proj(&ship_id);
                }
            }
            fighter_ids.push(id);
        }

        for env in &fit.projected.env {
            add_env(sol, fit_id, ship_id, env);
        }
        Projections {
            fighters: fighter_ids,
            modules,
        }
    }
}

/// Put a loose projected module into its holder, projecting onto `target` unless it is offline.
fn project_module(
    sol: &mut SolOwned,
    holder: rc::FitId,
    module: &ProjectedModule,
    target: ItemId,
) -> Option<ItemId> {
    let mut f = sol.get_fit(&holder).ok()?;
    let fitted = Module {
        type_id: module.type_id,
        state: module.state,
        charge: module.charge,
        spool: module.spool,
        mutation: None,
        rah: None,
    };
    // A capsule has no racks to speak of; the module just needs somewhere to sit.
    let mut item = add_fit_module(&mut f, ModRack::High, rc::AddMode::Append, &fitted).ok()?;
    let id = item.get_item_id();
    if module.state != ModuleState::Offline {
        let _ = item.add_proj(&target);
    }
    Some(id)
}

/// Environmental effects act on the whole fit, except abyssal weather and clouds, which refine
/// treats as something in space projecting onto ships.
fn add_env(sol: &mut SolOwned, fit_id: rc::FitId, ship_id: ItemId, env: &EnvEffect) {
    let type_id = ItemTypeId::from_i32(env.type_id);
    let Ok(mut f) = sol.get_fit(&fit_id) else {
        return;
    };
    let mut effect = f.add_fw_effect(type_id);
    let projects = effect.iter_effects().is_ok_and(|mut effects| {
        effects.any(|(id, _)| match id {
            rc::EffectId::Dogma(id) => {
                crate::data::projection::PROJECTED_ENV_EFFECTS.contains(&id.into_i32())
            }
            _ => false,
        })
    });
    if !projects {
        effect.set_state(env.active);
        return;
    }
    effect.remove();
    let mut effect = sol.add_proj_effect(type_id);
    effect.set_state(env.active);
    let _ = effect.add_proj(&ship_id);
}

/// Where a projector `range` metres from the target's surface has its centre: refine measures
/// range surface to surface, and the target sits at the origin.
fn centre_distance(range: f64, own_radius: f64, target_radius: f64) -> rc::Coordinates {
    let x = range + own_radius + target_radius;
    rc::Coordinates::new(
        rc::Value::from_f64_clamped(x),
        rc::Value::from_f64_clamped(0.0),
        rc::Value::from_f64_clamped(0.0),
    )
}

/// Without a range everything overlaps at the origin, which refine treats as point blank.
fn place_ship(sol: &mut SolOwned, ship: ItemId, range: Option<f64>, target_radius: f64) {
    let Some(range) = range else { return };
    let own = read::attr_or_zero(sol, ship, attr::RADIUS);
    if let Ok(mut ship) = sol.get_ship(&ship) {
        ship.set_coordinates(centre_distance(range, own, target_radius));
    }
}

fn place_minion(sol: &mut SolOwned, id: ItemId, range: Option<f64>, target_radius: f64) {
    let Some(range) = range else { return };
    let own = read::attr_or_zero(sol, id, attr::RADIUS);
    let at = centre_distance(range, own, target_radius);
    if let Ok(mut drone) = sol.get_drone(&id) {
        drone.set_coordinates(at);
    } else if let Ok(mut fighter) = sol.get_fighter(&id) {
        fighter.set_coordinates(at);
    }
}
