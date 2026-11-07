//! What is wrong with a fit, from refine's validation, in gyfa's terms: which item breaks which
//! rule, the way pyfa colors rows red and refuses modules that cannot go on the ship.

use std::collections::HashMap;

use rc::{
    ItemId, OptionExt,
    val::{ValOptions, ValResultFit, ValSlotCountFail},
};

use super::{Engine, Pilot};
use crate::{
    data::{Slot, TypeId},
    fit::Fit,
};

/// Where on the fit a problem sits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemRef {
    Ship,
    Mode,
    /// A module and the charge loaded into it.
    Module(Slot, usize),
    Rig(usize),
    Subsystem(usize),
    /// A drone stack, by its index in the fit.
    Drone(usize),
    Fighter(usize),
    Implant(TypeId),
    Booster(usize),
}

/// A skill an item needs and the level the pilot has it at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MissingSkill {
    pub skill: TypeId,
    pub have: Option<u8>,
    pub need: u8,
}

/// Something the fit uses more of than the ship has. These are fit-wide; the stats pane shows
/// them, so they are not tied to rows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Budget {
    Cpu,
    Powergrid,
    Calibration,
    DroneBandwidth,
    DroneBay,
    LaunchedDrones,
    FighterBay,
    FighterTubes,
    LightFighters,
    HeavyFighters,
    SupportFighters,
}

/// One broken rule.
#[derive(Debug, Clone, PartialEq)]
pub enum Problem {
    /// More modules in the rack than the ship has slots; only the ones past the end.
    SlotCount(Slot),
    Turrets,
    Launchers,
    /// The item names the ship types or groups it goes on, and this ship is not one of them.
    ShipLimit,
    MaxGroupFitted(u32),
    MaxGroupOnline(u32),
    MaxGroupActive(u32),
    MaxTypeFitted(u32),
    RigSize,
    /// Two subsystems for the same slot.
    SubsystemSlot,
    /// Two implants or boosters for the same slot.
    SlotTaken,
    CapitalModule,
    /// A structure module on a ship, or the other way round.
    HullKind,
    /// The module cannot be in the state it is in, e.g. active when it has nothing to activate.
    State,
    /// Overheating needs Thermodynamics at this level.
    OverloadSkill(u8),
    Skills(Vec<MissingSkill>),
    /// The loaded charge does not fit the module.
    Charge,
    DroneGroup,
    /// A drone or fighter the ship cannot launch at all.
    Unlaunchable,
    SquadSize(u32),
    /// The module needs more capacitor per cycle than the ship holds.
    UnusableCap,
    /// A mode that does not belong to the ship.
    Mode,
    NotLoaded,
    Over {
        budget: Budget,
        used: f64,
        max: f64,
    },
}

impl Problem {
    /// Whether pyfa's fitting restrictions refuse to add an item that causes this.
    pub fn is_restriction(&self) -> bool {
        matches!(
            self,
            Self::SlotCount(_)
                | Self::Turrets
                | Self::Launchers
                | Self::ShipLimit
                | Self::MaxGroupFitted(_)
                | Self::MaxTypeFitted(_)
                | Self::RigSize
                | Self::SubsystemSlot
                | Self::CapitalModule
                | Self::HullKind
        )
    }

    /// Whether turning the restrictions back on takes the item off. pyfa leaves hardpoints out of
    /// that check.
    pub fn removes_on_restrict(&self) -> bool {
        self.is_restriction() && !matches!(self, Self::Turrets | Self::Launchers)
    }

    /// What to tell the user; `name` gives a skill's name.
    pub fn describe(&self, name: impl Fn(TypeId) -> String) -> String {
        match self {
            Self::SlotCount(slot) => format!("The ship has no {} slot for this", slot_word(*slot)),
            Self::Turrets => "Not enough turret hardpoints".into(),
            Self::Launchers => "Not enough launcher hardpoints".into(),
            Self::ShipLimit => "Cannot be fitted to this ship".into(),
            Self::MaxGroupFitted(n) => format!("Only {n} of this group can be fitted"),
            Self::MaxGroupOnline(n) => format!("Only {n} of this group can be online"),
            Self::MaxGroupActive(n) => format!("Only {n} of this group can be active"),
            Self::MaxTypeFitted(n) => format!("Only {n} of this type can be fitted"),
            Self::RigSize => "Wrong rig size for this ship".into(),
            Self::SubsystemSlot => "Another subsystem takes this slot".into(),
            Self::SlotTaken => "Another item takes this slot".into(),
            Self::CapitalModule => "Capital modules need a capital ship".into(),
            Self::HullKind => "Does not go on this kind of hull".into(),
            Self::State => "Cannot be in this state".into(),
            Self::OverloadSkill(level) => {
                format!("Overheating needs Thermodynamics {}", roman(*level))
            }
            Self::Skills(missing) => {
                let list: Vec<String> = missing
                    .iter()
                    .map(|m| format!("{} {}", name(m.skill), roman(m.need)))
                    .collect();
                format!("Missing skills: {}", list.join(", "))
            }
            Self::Charge => "The loaded charge does not fit".into(),
            Self::DroneGroup => "The ship cannot use this kind of drone".into(),
            Self::Unlaunchable => "The ship cannot launch this".into(),
            Self::SquadSize(max) => format!("More than {max} in the squadron"),
            Self::UnusableCap => "Needs more capacitor than the ship holds".into(),
            Self::Mode => "Not a mode of this ship".into(),
            Self::NotLoaded => "Not in the game data".into(),
            Self::Over { budget, used, max } => {
                format!("{} over: {used:.1} of {max:.1}", budget_word(*budget))
            }
        }
    }
}

fn slot_word(slot: Slot) -> &'static str {
    match slot {
        Slot::High => "high",
        Slot::Mid => "med",
        Slot::Low => "low",
        Slot::Rig => "rig",
        Slot::Subsystem => "subsystem",
    }
}

fn budget_word(budget: Budget) -> &'static str {
    match budget {
        Budget::Cpu => "CPU",
        Budget::Powergrid => "Powergrid",
        Budget::Calibration => "Calibration",
        Budget::DroneBandwidth => "Drone bandwidth",
        Budget::DroneBay => "Drone bay",
        Budget::LaunchedDrones => "Launched drones",
        Budget::FighterBay => "Fighter bay",
        Budget::FighterTubes => "Fighter tubes",
        Budget::LightFighters => "Light fighters",
        Budget::HeavyFighters => "Heavy fighters",
        Budget::SupportFighters => "Support fighters",
    }
}

fn roman(level: u8) -> &'static str {
    ["0", "I", "II", "III", "IV", "V"]
        .get(usize::from(level))
        .copied()
        .unwrap_or("?")
}

/// Everything wrong with a fit.
#[derive(Debug, Clone, Default)]
pub struct Validation {
    /// Problems tied to an item, in no particular order.
    pub items: Vec<(ItemRef, Problem)>,
    /// Fit-wide overuse: CPU, powergrid and the like.
    pub fit: Vec<Problem>,
}

impl Validation {
    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && self.fit.is_empty()
    }

    /// The problems of one item.
    pub fn of(&self, item: ItemRef) -> impl Iterator<Item = &Problem> {
        self.items
            .iter()
            .filter(move |(i, _)| *i == item)
            .map(|(_, p)| p)
    }

    /// Item problems the fitting restrictions forbid.
    pub fn restrictions(&self) -> impl Iterator<Item = &(ItemRef, Problem)> {
        self.items.iter().filter(|(_, p)| p.is_restriction())
    }

    /// Why the change to `after` is refused, if it is: the first rule it breaks that this
    /// validation does not. Rules the fit already broke do not count, so a fit that came in
    /// broken can still be worked on. Without restrictions only the slot count is enforced, as
    /// pyfa always does.
    pub fn refusal<'a>(&self, after: &'a Self, ignore_restrictions: bool) -> Option<&'a Problem> {
        after
            .items
            .iter()
            .filter(|(_, p)| {
                if ignore_restrictions {
                    matches!(p, Problem::SlotCount(_))
                } else {
                    p.is_restriction()
                }
            })
            .find(|entry| !self.items.contains(entry))
            .map(|(_, p)| p)
    }

    fn push(&mut self, item: Option<ItemRef>, problem: Problem) {
        if let Some(item) = item
            && !self.items.iter().any(|(i, p)| *i == item && *p == problem)
        {
            self.items.push((item, problem));
        }
    }
}

/// Which gyfa item each refine item stands for.
pub(super) type Refs = HashMap<ItemId, ItemRef>;

/// Everything refine checks, less what needs a target, a security zone or another fit.
fn options() -> ValOptions {
    let off = || OptionExt::Disabled;
    ValOptions::new(true)
        .with_activation_blocked(off())
        .with_effect_stopper(off())
        .with_cloaking_blocked(off())
        .with_projectee_filter(off())
        .with_assist_immunity(off())
        .with_offense_immunity(off())
        .with_resist_immunity(off())
        .with_sec_zone_fitted(off())
        .with_sec_zone_online(off())
        .with_sec_zone_active(off())
        .with_sec_zone_unonlineable(off())
        .with_sec_zone_unactivable(off())
        .with_sec_zone_effect(off())
}

/// Run refine's validation on a fit built into a solar system.
pub(super) fn validate(f: &mut rc::Fit<'_>, refs: &Refs) -> Validation {
    let result = f.validate_verbose(&options());
    convert(&result, refs)
}

#[expect(
    clippy::too_many_lines,
    reason = "one arm per refine validation, flat is easiest to check against refine's list"
)]
fn convert(r: &ValResultFit, refs: &Refs) -> Validation {
    let mut v = Validation::default();
    let at = |id: &ItemId| refs.get(id).copied();

    let slots = |v: &mut Validation, fail: Option<&ValSlotCountFail>, problem: Problem| {
        for id in fail.iter().flat_map(|f| &f.users) {
            v.push(at(id), problem.clone());
        }
    };
    slots(
        &mut v,
        r.high_slot_count.as_ref(),
        Problem::SlotCount(Slot::High),
    );
    slots(
        &mut v,
        r.mid_slot_count.as_ref(),
        Problem::SlotCount(Slot::Mid),
    );
    slots(
        &mut v,
        r.low_slot_count.as_ref(),
        Problem::SlotCount(Slot::Low),
    );
    slots(
        &mut v,
        r.rig_slot_count.as_ref(),
        Problem::SlotCount(Slot::Rig),
    );
    slots(
        &mut v,
        r.subsystem_slot_count.as_ref(),
        Problem::SlotCount(Slot::Subsystem),
    );
    slots(&mut v, r.turret_slot_count.as_ref(), Problem::Turrets);
    slots(&mut v, r.launcher_slot_count.as_ref(), Problem::Launchers);
    for fail in [
        &r.unlaunchable_drone_slot,
        &r.unlaunchable_fighter,
        &r.unlaunchable_light_fighter,
        &r.unlaunchable_heavy_fighter,
        &r.unlaunchable_support_fighter,
        &r.unlaunchable_st_light_fighter,
        &r.unlaunchable_st_heavy_fighter,
        &r.unlaunchable_st_support_fighter,
    ] {
        for id in fail.iter().flat_map(|f| &f.users) {
            v.push(at(id), Problem::Unlaunchable);
        }
    }
    for id in r.unlaunchable_drone_bandwidth.iter().flat_map(|f| &f.users) {
        v.push(at(&id.item_id), Problem::Unlaunchable);
    }

    if let Some(fail) = &r.ship_limit {
        for item in &fail.items {
            v.push(at(&item.item_id), Problem::ShipLimit);
        }
    }
    for (fail, problem) in [
        (
            &r.max_group_fitted,
            Problem::MaxGroupFitted as fn(u32) -> Problem,
        ),
        (&r.max_group_online, Problem::MaxGroupOnline),
        (&r.max_group_active, Problem::MaxGroupActive),
    ] {
        for group in fail.iter().flat_map(|f| &f.groups) {
            for item in &group.items {
                v.push(at(&item.item_id), problem(item.limit.into_u32()));
            }
        }
    }
    for t in r.max_type_fitted.iter().flat_map(|f| &f.item_types) {
        for item in &t.items {
            v.push(
                at(&item.item_id),
                Problem::MaxTypeFitted(item.limit.into_u32()),
            );
        }
    }
    for rig in r.rig_size.iter().flat_map(|f| &f.rig_sizes) {
        v.push(at(&rig.rig_id), Problem::RigSize);
    }
    for slot in r.subsystem_slot_index.iter().flat_map(|f| &f.slot_users) {
        for id in &slot.item_ids {
            v.push(at(id), Problem::SubsystemSlot);
        }
    }
    for fail in [&r.implant_slot_index, &r.booster_slot_index] {
        for slot in fail.iter().flat_map(|f| &f.slot_users) {
            for id in &slot.item_ids {
                v.push(at(id), Problem::SlotTaken);
            }
        }
    }
    for m in r.capital_module.iter().flat_map(|f| &f.module_volumes) {
        v.push(at(&m.module_id), Problem::CapitalModule);
    }
    for item in r.item_vs_ship_kind.iter().flat_map(|f| &f.items) {
        v.push(at(&item.item_id), Problem::HullKind);
    }
    for m in r.module_state.iter().flat_map(|f| &f.modules) {
        v.push(at(&m.module_id), Problem::State);
    }
    for m in r.overload_skill.iter().flat_map(|f| &f.module_reqs) {
        v.push(
            at(&m.item_id),
            Problem::OverloadSkill(m.level_req.into_u8()),
        );
    }
    for item in r.skill_reqs.iter().flat_map(|f| &f.items) {
        let missing = item
            .missing_skills
            .iter()
            .map(|s| MissingSkill {
                skill: s.skill_type_id.into_i32(),
                have: s.current_lvl.map(rc::SkillLevel::into_u8),
                need: s.required_lvl.into_u8(),
            })
            .collect();
        v.push(at(&item.item_id), Problem::Skills(missing));
    }
    for c in r.charge_group.iter().flat_map(|f| &f.charges) {
        v.push(at(&c.parent_item_id), Problem::Charge);
    }
    for c in r.charge_parent_group.iter().flat_map(|f| &f.charges) {
        v.push(at(&c.parent_item_id), Problem::Charge);
    }
    for c in r.charge_size.iter().flat_map(|f| &f.charges) {
        v.push(at(&c.parent_item_id), Problem::Charge);
    }
    for c in r.charge_volume.iter().flat_map(|f| &f.charges) {
        v.push(at(&c.parent_item_id), Problem::Charge);
    }
    for d in r.drone_group.iter().flat_map(|f| &f.drone_groups) {
        v.push(at(&d.drone_id), Problem::DroneGroup);
    }
    for f in r.fighter_squad_size.iter().flat_map(|f| &f.fighters) {
        v.push(at(&f.fighter_id), Problem::SquadSize(f.max_size.into_u32()));
    }
    for item in r.unusable_cap.iter().flat_map(|f| &f.items) {
        v.push(at(&item.item_id), Problem::UnusableCap);
    }
    if let Some(fail) = &r.ship_stance {
        v.push(at(&fail.stance_item_id), Problem::Mode);
    }
    for id in r.not_loaded_item.iter().flat_map(|f| &f.item_ids) {
        v.push(at(id), Problem::NotLoaded);
    }
    for item in r.item_kind.iter().flat_map(|f| &f.item_kinds) {
        v.push(at(&item.item_id), Problem::HullKind);
    }

    let resources = [
        (&r.cpu, Budget::Cpu),
        (&r.powergrid, Budget::Powergrid),
        (&r.calibration, Budget::Calibration),
        (&r.drone_bandwidth, Budget::DroneBandwidth),
        (&r.drone_bay_volume, Budget::DroneBay),
        (&r.fighter_bay_volume, Budget::FighterBay),
    ];
    for (fail, budget) in resources {
        if let Some(fail) = fail {
            v.fit.push(Problem::Over {
                budget,
                used: fail.used.into_f64(),
                max: fail.max.map_or(0.0, rc::Value::into_f64),
            });
        }
    }
    let counts = [
        (&r.launched_drone_count, Budget::LaunchedDrones),
        (&r.launched_fighter_count, Budget::FighterTubes),
        (&r.launched_light_fighter_count, Budget::LightFighters),
        (&r.launched_heavy_fighter_count, Budget::HeavyFighters),
        (&r.launched_support_fighter_count, Budget::SupportFighters),
        (&r.launched_st_light_fighter_count, Budget::LightFighters),
        (&r.launched_st_heavy_fighter_count, Budget::HeavyFighters),
        (
            &r.launched_st_support_fighter_count,
            Budget::SupportFighters,
        ),
    ];
    for (fail, budget) in counts {
        if let Some(fail) = fail {
            v.fit.push(Problem::Over {
                budget,
                used: f64::from(fail.used.into_u32()),
                max: f64::from(fail.max.map_or(0, rc::Count::into_u32)),
            });
        }
    }
    v
}

impl Engine {
    /// What is wrong with the fit flown by `pilot`, on its own: no fleet, projections or
    /// environment. Cheaper than [`Engine::compute_among`], which carries the same result in
    /// [`super::Stats::validation`]; this is for trying out a change before keeping it.
    pub fn validate(&self, fit: &Fit, pilot: &Pilot) -> Validation {
        let mut sol = rc::SolOwned::new(&self.src);
        let mut f = sol.add_fit();
        let Some(local) = super::build_local(&mut f, fit, pilot, &mut Vec::new()) else {
            return Validation::default();
        };
        validate(&mut f, &local.refs)
    }
}
