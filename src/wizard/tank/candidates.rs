//! What a tank search tries: the modules and rigs for the layer and goal, in the variations the
//! search tiers switch on, and the room the fit leaves for them.

use std::collections::HashSet;

use super::{Candidate, Choices, Goal, Problem, RigCandidate, Role, Room, idle_guns};
use crate::wizard::{HullSize, Layer, Setup};
use crate::{
    data::{Catalog, Slot, TypeId},
    dogma::{attr, group},
    engine::{Engine, Pilot},
    fit::{Fit, Module},
    prices::Prices,
};

/// Repairers or boosters of one kind a tank takes at most, unless more are asked for.
const MAX_REPAIRERS: usize = 3;

/// Damage-type specific modules of one kind a tank takes at most.
const MAX_SPECIFIC: usize = 2;

const DAMAGE_TYPES: [&str; 4] = ["EM", "Thermal", "Kinetic", "Explosive"];

/// Whether a module repairs its layer: a repairer or booster, ancillary or not.
fn is_repairer(catalog: &Catalog, id: TypeId) -> bool {
    catalog.get(id).is_some_and(|t| {
        matches!(
            t.group_id,
            group::ARMOR_REPAIRER
                | group::SHIELD_BOOSTER
                | group::ANCILLARY_ARMOR_REPAIRER
                | group::ANCILLARY_SHIELD_BOOSTER
        )
    })
}

impl Setup<'_> {
    /// The room the choices' tank has on `fit`: the fit with the tank being redone taken off,
    /// and the positions it leaves free. No engine needed, so it is cheap to work out often.
    pub fn tank_room(&self, fit: &Fit, choices: &Choices) -> Room {
        let slot = choices.layer.slot();
        // What counts as the fitted tank: anything of a group the search fits, repairers
        // included whatever the goal, but not a cap booster, which may be there for anything.
        let module_groups: HashSet<i32> = self
            .tank_candidates(choices, true, &[0, 1, 2, 3])
            .iter()
            .filter_map(|c| self.catalog.get(c.module.type_id))
            .map(|t| t.group_id)
            .filter(|&g| g != group::CAPACITOR_BOOSTER)
            .collect();
        let rig_groups: HashSet<i32> = self
            .rig_candidates(choices, true, &[0, 1, 2, 3])
            .iter()
            .filter(|r| !r.fitting)
            .filter_map(|r| self.catalog.get(r.type_id))
            .map(|t| t.group_id)
            .collect();
        let tank = |id: TypeId, groups: &HashSet<i32>| {
            self.catalog
                .get(id)
                .is_some_and(|t| groups.contains(&t.group_id))
        };
        let mut next = fit.clone();
        let mut removed = Vec::new();
        if choices.refit {
            for (i, at) in next.rack_mut(slot).iter_mut().enumerate() {
                if at.as_ref().is_some_and(|m| tank(m.type_id, &module_groups)) {
                    *at = None;
                    removed.push((slot, i));
                }
            }
            for (i, at) in next.rigs.iter_mut().enumerate() {
                if at.is_some_and(|r| tank(r, &rig_groups)) {
                    *at = None;
                    removed.push((Slot::Rig, i));
                }
            }
        }
        let positions = (0..self.facts.total(slot))
            .filter(|&i| next.rack(slot).get(i).is_none_or(Option::is_none))
            .collect();
        let rig_positions = (0..self.facts.total(Slot::Rig))
            .filter(|&i| next.rigs.get(i).is_none_or(Option::is_none))
            .collect();
        Room {
            fit: next,
            removed,
            positions,
            rig_positions,
        }
    }

    /// The search for the choices' tank on `fit`. Its positions are empty when the layer and
    /// the rigs have no room.
    pub fn tank_problem(
        &self,
        engine: &Engine,
        pilot: &Pilot,
        fit: &Fit,
        choices: &Choices,
        prices: &Prices,
    ) -> Problem {
        let layer = choices.layer;
        let slot = layer.slot();
        let repair = choices.goal == Goal::Repair;
        let Room {
            fit: mut next,
            removed,
            positions,
            rig_positions,
        } = self.tank_room(fit, choices);
        idle_guns(&mut next);
        // Damage-specific modules for every damage type the pattern brings, the most exposed
        // first; the score against the pattern decides whether a kinetic or thermal hardener
        // beats an omni one.
        let empty = engine.probe_tank(&next, pilot);
        let resists = match layer {
            Layer::Shield => empty.shield.resists,
            Layer::Armor => empty.armor.resists,
        };
        let mut order = [0, 1, 2, 3];
        order.sort_by(|&a, &b| {
            let hole = |i: usize| empty.incoming[i] * (1.0 - resists[i]);
            hole(b).total_cmp(&hole(a))
        });
        let exposed: Vec<usize> = order
            .into_iter()
            .filter(|&i| empty.incoming[i] > 0.0)
            .collect();
        // Repairers left on count: the search need not add one, though it may, and a chosen
        // number of them counts these too.
        let fitted_repairers = next
            .rack(slot)
            .iter()
            .flatten()
            .filter(|m| is_repairer(self.catalog, m.type_id))
            .count();
        let repairers = choices
            .repairers()
            .map(|n| n.saturating_sub(fitted_repairers));
        let affordable = |id: TypeId| choices.budget.allows(prices.get(id));
        let mut candidates = self.tank_candidates(choices, repair, &exposed);
        candidates.retain(|c| affordable(c.module.type_id));
        let mut rigs = if rig_positions.is_empty() {
            Vec::new()
        } else {
            self.rig_candidates(choices, repair, &exposed)
        };
        rigs.retain(|r| affordable(r.type_id));
        let known = candidates
            .iter()
            .map(|c| c.module.type_id)
            .chain(rigs.iter().map(|r| r.type_id))
            .filter_map(|id| Some((id, prices.get(id)?)))
            .collect();
        Problem {
            candidates,
            rigs,
            prices: known,
            max_total: choices.budget.total.map(|max| max as f64),
            fit: next,
            pilot: pilot.clone(),
            layer,
            goal: choices.goal,
            positions,
            rig_positions,
            removed,
            need_repair: repair && fitted_repairers == 0 && repairers.is_none_or(|n| n > 0),
            repairers,
        }
    }

    /// The modules worth trying for the layer, repairers first when `repair`; `exposed` are
    /// the damage types to try specific resist modules for.
    fn tank_candidates(
        &self,
        choices: &Choices,
        repair: bool,
        exposed: &[usize],
    ) -> Vec<Candidate> {
        let max_repairers = MAX_REPAIRERS.max(choices.repairers().unwrap_or(0));
        let all = usize::MAX;
        let mut wanted: Vec<(String, Role, usize)> = Vec::new();
        let mut want = |name: String, role: Role, max: usize| wanted.push((name, role, max));
        match choices.layer {
            Layer::Armor => {
                if repair {
                    want(self.repairer_name(), Role::Repair, max_repairers);
                    want(self.ancillary_name(Layer::Armor), Role::Repair, 1);
                }
                want(self.plate_name().into(), Role::Buffer, all);
                want(
                    "Multispectrum Energized Membrane I".into(),
                    Role::Resist,
                    all,
                );
                want("Multispectrum Coating I".into(), Role::Resist, all);
                for &i in exposed {
                    let kind = DAMAGE_TYPES[i];
                    want(
                        format!("{kind} Energized Membrane I"),
                        Role::Resist,
                        MAX_SPECIFIC,
                    );
                    want(format!("{kind} Coating I"), Role::Resist, MAX_SPECIFIC);
                    want(
                        format!("{kind} Armor Hardener I"),
                        Role::Resist,
                        MAX_SPECIFIC,
                    );
                }
                want("Reactive Armor Hardener".into(), Role::Resist, 1);
                want("Damage Control I".into(), Role::Resist, 1);
            }
            Layer::Shield => {
                if repair {
                    want(self.booster_name().into(), Role::Repair, max_repairers);
                    want(self.ancillary_name(Layer::Shield), Role::Repair, 1);
                    want(
                        "Shield Boost Amplifier I".into(),
                        Role::Support,
                        MAX_SPECIFIC,
                    );
                }
                want(self.extender_name().into(), Role::Buffer, all);
                want("Multispectrum Shield Hardener I".into(), Role::Resist, all);
                for &i in exposed {
                    let kind = DAMAGE_TYPES[i];
                    want(
                        format!("{kind} Shield Hardener I"),
                        Role::Resist,
                        MAX_SPECIFIC,
                    );
                    want(
                        format!("{kind} Shield Amplifier I"),
                        Role::Resist,
                        MAX_SPECIFIC,
                    );
                }
                if repair {
                    want(self.cap_booster_name(), Role::Support, 1);
                }
            }
        }
        wanted
            .into_iter()
            .filter_map(|(name, role, max)| Some((self.base(&name)?, role, max)))
            .flat_map(|(base, role, max)| {
                self.search_variations(base, false, choices.search_tiers)
                    .into_iter()
                    .map(move |type_id| (base, type_id, role, max))
            })
            .map(|(base, type_id, role, max)| {
                let mut module = Module::new(self.catalog, type_id);
                module.charge = self.charge_for(type_id);
                Candidate {
                    base,
                    module,
                    role,
                    max,
                }
            })
            .collect()
    }

    /// The rigs worth trying for the layer, repair rigs when `repair`.
    fn rig_candidates(
        &self,
        choices: &Choices,
        repair: bool,
        exposed: &[usize],
    ) -> Vec<RigCandidate> {
        let mut names: Vec<String> = Vec::new();
        match choices.layer {
            Layer::Armor => {
                names.push("Trimark Armor Pump I".into());
                if repair {
                    names.push("Auxiliary Nano Pump I".into());
                    names.push("Nanobot Accelerator I".into());
                }
                for &i in exposed {
                    names.push(format!("{} Armor Reinforcer I", DAMAGE_TYPES[i]));
                }
            }
            Layer::Shield => {
                names.push("Core Defense Field Extender I".into());
                if repair {
                    names.push("Core Defense Operational Solidifier I".into());
                }
                for &i in exposed {
                    names.push(format!("{} Shield Reinforcer I", DAMAGE_TYPES[i]));
                }
            }
        }
        // Rigs that make room rather than tank: for hulls short of powergrid or CPU.
        let fitting = [
            "Ancillary Current Router I",
            "Processor Overclocking Unit I",
        ];
        names.extend(fitting.iter().map(|&n| n.to_owned()));
        let size = self.facts.size.label();
        names
            .into_iter()
            .filter_map(|name| {
                let base = self.base(&format!("{size} {name}"))?;
                Some((base, fitting.contains(&name.as_str())))
            })
            .flat_map(|(base, fitting)| {
                self.search_variations(base, true, choices.search_tiers)
                    .into_iter()
                    .map(move |type_id| (base, type_id, fitting))
            })
            .filter(|&(_, type_id, _)| {
                self.attr(type_id, attr::UPGRADE_COST) <= self.facts.calibration
            })
            .map(|(base, type_id, fitting)| RigCandidate {
                base,
                type_id,
                fitting,
            })
            .collect()
    }

    /// The plate a hull this size takes.
    fn plate_name(&self) -> &'static str {
        match self.facts.size {
            HullSize::Small => "400mm Steel Plates I",
            HullSize::Medium if !self.facts.large() => "800mm Steel Plates I",
            HullSize::Medium | HullSize::Large => "1600mm Steel Plates I",
            HullSize::Capital => "25000mm Steel Plates I",
        }
    }

    fn extender_name(&self) -> &'static str {
        match self.facts.size {
            HullSize::Small => "Medium Shield Extender I",
            HullSize::Medium | HullSize::Large => "Large Shield Extender I",
            HullSize::Capital => "Capital Shield Extender I",
        }
    }

    fn repairer_name(&self) -> String {
        format!("{} Armor Repairer I", self.facts.size.label())
    }

    fn booster_name(&self) -> &'static str {
        match self.facts.size {
            HullSize::Small => "Small Shield Booster I",
            HullSize::Medium if !self.facts.large() => "Medium Shield Booster I",
            HullSize::Medium => "Large Shield Booster I",
            HullSize::Large => "X-Large Shield Booster I",
            HullSize::Capital => "Capital Shield Booster I",
        }
    }

    fn cap_booster_name(&self) -> String {
        let size = match self.facts.size {
            HullSize::Small => "Small",
            HullSize::Medium => "Medium",
            HullSize::Large => "Heavy",
            HullSize::Capital => "Capital",
        };
        format!("{size} Capacitor Booster I")
    }

    /// The ancillary repairer or booster for the layer.
    fn ancillary_name(&self, layer: Layer) -> String {
        match layer {
            Layer::Armor => format!("{} Ancillary Armor Repairer", self.facts.size.label()),
            Layer::Shield => {
                let size = match self.facts.size {
                    HullSize::Small => "Small",
                    HullSize::Medium if !self.facts.large() => "Medium",
                    HullSize::Medium => "Large",
                    HullSize::Large => "X-Large",
                    HullSize::Capital => "Capital",
                };
                format!("{size} Ancillary Shield Booster")
            }
        }
    }
}
