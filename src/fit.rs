//! The fit as the user sees it. This is the source of truth; the engine rebuilds its own state from
//! it whenever it changes.

pub mod cargo;
pub mod fighters;
pub mod mutation;
pub mod projected;

use serde::{Deserialize, Serialize};

pub use mutation::Mutation;

use crate::{
    character::CharacterId,
    data::{Catalog, Kind, Slot, TypeId},
};

/// Ordered from off to most active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum ModuleState {
    Offline,
    Online,
    Active,
    Overload,
}

impl ModuleState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Offline => "Offline",
            Self::Online => "Online",
            Self::Active => "Active",
            Self::Overload => "Overheated",
        }
    }

    /// Whether the module cycles: active or overheated.
    pub fn is_running(self) -> bool {
        matches!(self, Self::Active | Self::Overload)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Module {
    pub type_id: TypeId,
    pub state: ModuleState,
    pub charge: Option<TypeId>,
    /// Spool cycles to assume, for modules that spool up. None means fully spooled.
    #[serde(default)]
    pub spool: Option<u32>,
    /// What a mutaplasmid made of it; `type_id` is then the abyssal type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mutation: Option<Mutation>,
    /// What a reactive armor hardener adapts to; None takes the fit's damage pattern.
    #[serde(default)]
    pub rah: Option<RahPattern>,
}

impl Module {
    /// A module as pyfa adds one: active if it can be, online otherwise.
    pub fn new(catalog: &Catalog, type_id: TypeId) -> Self {
        let activatable = catalog.get(type_id).is_some_and(|t| t.activatable);
        Self {
            type_id,
            state: if activatable {
                ModuleState::Active
            } else {
                ModuleState::Online
            },
            charge: None,
            spool: None,
            mutation: None,
            rah: None,
        }
    }

    /// The type before any mutation, which is what the market sells and EFT names.
    pub fn base_type(&self) -> TypeId {
        self.mutation.as_ref().map_or(self.type_id, |m| m.base)
    }

    /// Apply a mutaplasmid, pyfa's "Apply Mutaplasmid". Returns whether it took.
    pub fn mutate(&mut self, catalog: &Catalog, mutaplasmid: TypeId) -> bool {
        mutate(catalog, &mut self.type_id, &mut self.mutation, mutaplasmid)
    }

    /// Back to the base item, pyfa's "Revert to". Returns whether it was mutated.
    pub fn revert(&mut self) -> bool {
        revert(&mut self.type_id, &mut self.mutation)
    }
}

/// pyfa's per-module "RAH Damage Pattern": a pattern of its own, or no adapting at all.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum RahPattern {
    /// The hardener keeps its starting resists.
    DoNotAdapt,
    Pattern(DamageProfile),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DroneStack {
    pub type_id: TypeId,
    pub count: u32,
    pub active: u32,
    /// What a mutaplasmid made of these drones; `type_id` is then the mutated drone type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mutation: Option<Mutation>,
}

/// The most of one item a cargo stack holds, and the most any amount read from text counts
/// for: far beyond what any hold carries, and small enough that sums of stacks stay sane.
pub const MAX_STACK: u32 = 1_000_000;
/// The most drones one stack holds, and fighters one squadron. The engine adds every drone as
/// an item of its own, so a pasted "x4000000000" must not reach it.
pub const MAX_DRONES: u32 = 1_000;

impl DroneStack {
    /// A stack of `count` drones, at most [`MAX_DRONES`], `active` of them launched.
    pub fn new(type_id: TypeId, count: u32, active: u32) -> Self {
        let count = count.min(MAX_DRONES);
        Self {
            type_id,
            count,
            active: active.min(count),
            mutation: None,
        }
    }

    /// The drone before any mutation.
    pub fn base_type(&self) -> TypeId {
        self.mutation.as_ref().map_or(self.type_id, |m| m.base)
    }

    pub fn mutate(&mut self, catalog: &Catalog, mutaplasmid: TypeId) -> bool {
        mutate(catalog, &mut self.type_id, &mut self.mutation, mutaplasmid)
    }

    pub fn revert(&mut self) -> bool {
        revert(&mut self.type_id, &mut self.mutation)
    }
}

fn mutate(
    catalog: &Catalog,
    type_id: &mut TypeId,
    mutation: &mut Option<Mutation>,
    mutaplasmid: TypeId,
) -> bool {
    if mutation.is_some() {
        return false;
    }
    let Some((result, new)) = Mutation::new(catalog, *type_id, mutaplasmid) else {
        return false;
    };
    *type_id = result;
    *mutation = Some(new);
    true
}

fn revert(type_id: &mut TypeId, mutation: &mut Option<Mutation>) -> bool {
    match mutation.take() {
        Some(m) => {
            *type_id = m.base;
            true
        }
        None => false,
    }
}

/// A booster and the switches pyfa keeps for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "BoosterSave")]
pub struct Booster {
    pub type_id: TypeId,
    pub active: bool,
    /// Side effects switched on, by effect ID. pyfa starts every booster with none.
    pub side_effects: Vec<i32>,
}

impl Booster {
    pub fn new(type_id: TypeId) -> Self {
        Self {
            type_id,
            active: true,
            side_effects: Vec::new(),
        }
    }
}

/// Saves from before boosters had switches hold bare type IDs.
#[derive(Deserialize)]
#[serde(untagged)]
enum BoosterSave {
    Bare(TypeId),
    Full {
        type_id: TypeId,
        #[serde(default = "active_default")]
        active: bool,
        #[serde(default)]
        side_effects: Vec<i32>,
    },
}

fn active_default() -> bool {
    true
}

impl From<BoosterSave> for Booster {
    fn from(save: BoosterSave) -> Self {
        match save {
            BoosterSave::Bare(type_id) => Self::new(type_id),
            BoosterSave::Full {
                type_id,
                active,
                side_effects,
            } => Self {
                type_id,
                active,
                side_effects,
            },
        }
    }
}

/// Whose implants a fit is calculated with: its own, or the ones plugged into the character.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImplantSource {
    #[default]
    Fit,
    Character,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Fit {
    pub name: String,
    pub ship: Option<TypeId>,
    /// Racks by position; None is an empty slot. Position matters for heat, which spreads to
    /// neighbouring slots, so modules keep their place when others are removed.
    pub high: Vec<Option<Module>>,
    pub mid: Vec<Option<Module>>,
    pub low: Vec<Option<Module>>,
    pub rigs: Vec<Option<TypeId>>,
    pub subsystems: Vec<TypeId>,
    /// An Upwell structure's service modules, by position like the racks.
    #[serde(default)]
    pub services: Vec<Option<Module>>,
    pub drones: Vec<DroneStack>,
    pub implants: Vec<TypeId>,
    /// One per booster slot, in slot order.
    pub boosters: Vec<Booster>,
    pub cargo: Vec<(TypeId, u32)>,
    #[serde(default)]
    pub notes: String,
    /// A tactical destroyer's mode.
    #[serde(default)]
    pub mode: Option<TypeId>,
    /// Pilot security status; None uses the character's.
    #[serde(default)]
    pub sec_status: Option<f64>,
    /// Where a structure is anchored, which some structure bonuses depend on.
    #[serde(default)]
    pub system_security: SystemSecurity,
    /// Incoming damage the tank numbers assume.
    #[serde(default)]
    pub damage_profile: DamageProfile,
    /// Implants switched off: they stay on the fit but do nothing, like pyfa's unchecked ones.
    #[serde(default)]
    pub inactive_implants: Vec<TypeId>,
    #[serde(default)]
    pub implant_source: ImplantSource,
    /// What the firepower view applies damage to; none shows raw damage.
    #[serde(default)]
    pub target_profile: Option<crate::target_profile::TargetProfile>,
    /// Who flies the fit; fits from before characters existed fly All 5.
    #[serde(default)]
    pub character: CharacterId,
    /// Identity other fits reference this one by when projecting it or taking its boosts. Zero
    /// until something needs it; see [`crate::store::Saved::ensure_uid`].
    #[serde(default)]
    pub uid: u64,
    #[serde(default)]
    pub fighters: Vec<fighters::Fighter>,
    #[serde(default)]
    pub projected: projected::Projected,
    /// Fleet boosters, pyfa's command fits.
    #[serde(default)]
    pub command: Vec<projected::CommandFit>,
    /// pyfa's "Disable Fitting Restrictions": anything goes on, and what breaks the rules shows
    /// red.
    #[serde(default)]
    pub ignore_restrictions: bool,
    /// Racks whose modules of one type act as one: shown as a single row, edited together.
    #[serde(default)]
    pub grouped: std::collections::BTreeSet<Slot>,
    /// When the fit was created or last changed, in seconds since the Unix epoch; zero for fits
    /// saved before efa kept it. The fittings browser's recent fits go by it.
    #[serde(default)]
    pub modified: u64,
}

/// The time to stamp a changed fit with.
pub use crate::net::now;

/// The kind of system a structure sits in, pyfa's "Citadel System Security". pyfa assumes null
/// security until told otherwise.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SystemSecurity {
    HighSec,
    LowSec,
    #[default]
    NullSec,
    WSpace,
}

impl SystemSecurity {
    pub const ALL: [Self; 4] = [Self::HighSec, Self::LowSec, Self::NullSec, Self::WSpace];

    pub fn label(self) -> &'static str {
        match self {
            Self::HighSec => "High Security",
            Self::LowSec => "Low Security",
            Self::NullSec => "Null Security",
            Self::WSpace => "W-Space",
        }
    }
}

/// Incoming damage that effective HP, effective repair rates and the reactive armor hardener
/// assume, like pyfa's per-fit damage pattern.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub enum DamageProfile {
    #[default]
    Uniform,
    /// The mix the fit's own weapons deal with the charges loaded now.
    SelectedAmmo,
    /// A named EM, thermal, kinetic, explosive mix; only the ratios matter.
    Pattern { name: String, amounts: [f64; 4] },
}

/// Drones a character can have in space at once, without skills or ship bonuses changing it.
pub(crate) const MAX_LAUNCHED: u32 = 5;

impl Fit {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    /// Positions of the module at `slot`/`i` and, in a grouped rack, every other module of its
    /// type and rolls. Empty when nothing is there.
    pub fn group(&self, slot: Slot, i: usize) -> Vec<usize> {
        let rack = self.rack(slot);
        let Some(Some(m)) = rack.get(i) else {
            return Vec::new();
        };
        if !self.grouped.contains(&slot) {
            return vec![i];
        }
        rack.iter()
            .enumerate()
            // Mutated modules only group with ones rolled the same.
            .filter(|(_, o)| {
                o.as_ref()
                    .is_some_and(|o| o.type_id == m.type_id && o.mutation == m.mutation)
            })
            .map(|(j, _)| j)
            .collect()
    }

    /// The modules of a rack. Rigs and subsystems are not modules; they read as the low rack.
    pub fn rack(&self, slot: Slot) -> &Vec<Option<Module>> {
        match slot {
            Slot::High => &self.high,
            Slot::Mid => &self.mid,
            Slot::Service => &self.services,
            Slot::Low | Slot::Rig | Slot::Subsystem => &self.low,
        }
    }

    pub fn rack_mut(&mut self, slot: Slot) -> &mut Vec<Option<Module>> {
        match slot {
            Slot::High => &mut self.high,
            Slot::Mid => &mut self.mid,
            Slot::Service => &mut self.services,
            Slot::Low | Slot::Rig | Slot::Subsystem => &mut self.low,
        }
    }

    /// Every rack that holds modules, with its slot.
    pub fn module_racks(&self) -> [(Slot, &Vec<Option<Module>>); 4] {
        [
            (Slot::High, &self.high),
            (Slot::Mid, &self.mid),
            (Slot::Low, &self.low),
            (Slot::Service, &self.services),
        ]
    }

    /// Add whatever this type is to the fit, the way double-clicking it in pyfa does. Returns false
    /// if it is not something a fit can hold.
    pub fn add(&mut self, catalog: &Catalog, type_id: TypeId) -> bool {
        let Some(t) = catalog.get(type_id) else {
            return false;
        };
        match t.kind {
            Kind::Ship => {
                if self.name.is_empty()
                    || self
                        .ship
                        .and_then(|s| catalog.get(s))
                        .is_some_and(|s| s.name == self.name)
                {
                    self.name.clone_from(&t.name);
                }
                self.ship = Some(type_id);
                self.mode = catalog.modes_for(type_id).first().copied();
            }
            Kind::Module(slot @ (Slot::High | Slot::Mid | Slot::Low | Slot::Service)) => {
                place_first_free(self.rack_mut(slot), Module::new(catalog, type_id));
            }
            Kind::Module(Slot::Rig) => place_first_free(&mut self.rigs, type_id),
            Kind::Module(Slot::Subsystem) => self.subsystems.push(type_id),
            Kind::Charge => {
                let mut loaded = false;
                for module in self
                    .high
                    .iter_mut()
                    .chain(&mut self.mid)
                    .chain(&mut self.low)
                    .flatten()
                {
                    if catalog.get(module.type_id).is_some_and(|m| t.fits_in(m)) {
                        module.charge = Some(type_id);
                        loaded = true;
                    }
                }
                if !loaded {
                    return false;
                }
            }
            Kind::Fighter => self.fighters.push(fighters::Fighter::new(type_id)),
            Kind::Drone => {
                let launch = self.launched() < MAX_LAUNCHED;
                match self.drones.iter_mut().find(|d| d.type_id == type_id) {
                    Some(stack) => {
                        stack.count += 1;
                        stack.active += u32::from(launch);
                    }
                    None => self
                        .drones
                        .push(DroneStack::new(type_id, 1, u32::from(launch))),
                }
            }
            Kind::Implant => {
                self.add_implant(catalog, type_id);
            }
            Kind::Booster => {
                self.add_booster(catalog, Booster::new(type_id));
            }
            Kind::Mode => self.mode = Some(type_id),
            Kind::Cargo => self.add_cargo(type_id, 1),
            Kind::Skill | Kind::Other => return false,
        }
        true
    }

    /// Add a mutated module or drone, as pyfa does with a pasted mutated item: a module goes into
    /// the first free position of its rack, a drone into a stack of its own. Returns false if it
    /// is neither.
    pub fn add_mutated(&mut self, catalog: &Catalog, type_id: TypeId, mutation: Mutation) -> bool {
        match catalog.get(type_id).map(|t| t.kind) {
            Some(Kind::Module(slot @ (Slot::High | Slot::Mid | Slot::Low))) => {
                let module = Module {
                    mutation: Some(mutation),
                    ..Module::new(catalog, type_id)
                };
                place_first_free(self.rack_mut(slot), module);
                true
            }
            Some(Kind::Drone) => {
                self.add_drone_stack(DroneStack {
                    mutation: Some(mutation),
                    ..DroneStack::new(type_id, 1, 0)
                });
                true
            }
            _ => false,
        }
    }

    /// Add a stack of its own, launching as many of it as there is room for in space.
    pub fn add_drone_stack(&mut self, mut stack: DroneStack) {
        stack.active = MAX_LAUNCHED
            .saturating_sub(self.launched())
            .min(stack.count);
        self.drones.push(stack);
    }

    /// Put a module into the first free position of its rack.
    pub fn place_module(&mut self, slot: Slot, module: Module) {
        place_first_free(self.rack_mut(slot), module);
    }

    /// Put `count` of an item into the cargo hold, on top of any stack already there.
    pub fn add_cargo(&mut self, type_id: TypeId, count: u32) {
        let count = count.min(MAX_STACK);
        match self.cargo.iter_mut().find(|(t, _)| *t == type_id) {
            Some((_, n)) => *n = n.saturating_add(count).min(MAX_STACK),
            None => self.cargo.push((type_id, count)),
        }
    }

    /// Volume the hold carries, in m³: the cargo, plus the fit's own implants and boosters when
    /// `with_character` is set, for fits whose pilot carries them along.
    pub fn hold_volume(&self, catalog: &Catalog, with_character: bool) -> f64 {
        let volume = |t: TypeId| catalog.get(t).map_or(0.0, |t| t.volume);
        let character: f64 = if with_character {
            let implants = (self.implant_source == ImplantSource::Fit)
                .then_some(&self.implants)
                .into_iter()
                .flatten()
                .copied();
            implants
                .chain(self.boosters.iter().map(|b| b.type_id))
                .map(volume)
                .sum()
        } else {
            0.0
        };
        self.cargo_volume(catalog) + character
    }

    /// Volume the cargo takes, in m³.
    pub fn cargo_volume(&self, catalog: &Catalog) -> f64 {
        self.cargo
            .iter()
            .map(|&(t, n)| catalog.get(t).map_or(0.0, |t| t.volume) * f64::from(n))
            .sum()
    }

    /// Add `count` drones of a type, launching as many as there is room for in space.
    pub fn add_drones(&mut self, type_id: TypeId, count: u32) {
        let launch = MAX_LAUNCHED.saturating_sub(self.launched()).min(count);
        match self.drones.iter_mut().find(|d| d.type_id == type_id) {
            Some(stack) => {
                stack.count = stack.count.saturating_add(count).min(MAX_DRONES);
                stack.active = stack.active.saturating_add(launch).min(stack.count);
            }
            None => self.drones.push(DroneStack::new(type_id, count, launch)),
        }
    }

    /// Add a booster the way pyfa does: one already on the fit is left alone, and one sharing its
    /// slot is replaced. Returns whether the fit changed.
    pub fn add_booster(&mut self, catalog: &Catalog, booster: Booster) -> bool {
        if self.boosters.iter().any(|b| b.type_id == booster.type_id) {
            return false;
        }
        let slot = |id: TypeId| catalog.get(id).and_then(|t| t.booster_slot);
        let wanted = slot(booster.type_id);
        if wanted.is_some() {
            self.boosters.retain(|b| slot(b.type_id) != wanted);
        }
        self.boosters.push(booster);
        // pyfa lists boosters by slot; keeping them stored that way makes rows and indices agree.
        self.boosters.sort_by_key(|b| slot(b.type_id).unwrap_or(0));
        true
    }

    /// Swap a booster for a variation, keeping whether it is on and those of its side effects the
    /// new one has too.
    pub fn swap_booster(&mut self, catalog: &Catalog, index: usize, type_id: TypeId) -> bool {
        let Some(old) = self.boosters.get(index) else {
            return false;
        };
        if self.boosters.iter().any(|b| b.type_id == type_id) {
            return false;
        }
        let effects = catalog
            .get(type_id)
            .map_or(&[][..], |t| t.side_effects.as_slice());
        let new = Booster {
            type_id,
            active: old.active,
            side_effects: old
                .side_effects
                .iter()
                .copied()
                .filter(|e| effects.contains(e))
                .collect(),
        };
        self.boosters.remove(index);
        self.add_booster(catalog, new)
    }

    /// Plug in an implant the way pyfa does: the fit switches to its own implants, an implant
    /// already in that slot makes way, and the list stays in slot order. Returns false when
    /// nothing changed.
    pub fn add_implant(&mut self, catalog: &Catalog, type_id: TypeId) -> bool {
        let switched = self.implant_source != ImplantSource::Fit;
        self.implant_source = ImplantSource::Fit;
        let plugged = plug_implant(&mut self.implants, catalog, type_id);
        self.inactive_implants.retain(|t| self.implants.contains(t));
        switched || plugged
    }

    pub fn remove_implant(&mut self, i: usize) {
        if i < self.implants.len() {
            let gone = self.implants.remove(i);
            self.inactive_implants.retain(|&t| t != gone);
        }
    }

    /// Swap an implant for another type, keeping whether it is switched on.
    pub fn swap_implant(&mut self, catalog: &Catalog, i: usize, type_id: TypeId) {
        let Some(&old) = self.implants.get(i) else {
            return;
        };
        if old == type_id {
            return;
        }
        let active = self.implant_active(old);
        self.remove_implant(i);
        self.add_implant(catalog, type_id);
        if !active && self.implants.contains(&type_id) {
            self.inactive_implants.push(type_id);
        }
    }

    pub fn implant_active(&self, type_id: TypeId) -> bool {
        !self.inactive_implants.contains(&type_id)
    }

    /// pyfa's state toggle: if the clicked implant is on, switch every target off, otherwise
    /// switch them all on.
    pub fn toggle_implants(&mut self, clicked: usize, rows: &[usize]) {
        let Some(&main) = self.implants.get(clicked) else {
            return;
        };
        let turn_on = !self.implant_active(main);
        let targets: Vec<TypeId> = rows
            .iter()
            .chain([&clicked])
            .filter_map(|&r| self.implants.get(r).copied())
            .collect();
        self.inactive_implants.retain(|t| !targets.contains(t));
        if !turn_on {
            self.inactive_implants.extend(targets);
            self.inactive_implants.sort_unstable();
            self.inactive_implants.dedup();
        }
    }

    /// Put the implants in slot order, which older saves did not keep. Returns whether any moved.
    pub fn sort_implants(&mut self, catalog: &Catalog) -> bool {
        if self.implants_in_slot_order(catalog) {
            return false;
        }
        let slot = |&t: &TypeId| implant_slot(catalog, t);
        self.implants.sort_by_key(slot);
        true
    }

    /// Whether the implants are in slot order, as the Implants tab lists them.
    pub fn implants_in_slot_order(&self, catalog: &Catalog) -> bool {
        self.implants
            .is_sorted_by_key(|&t: &TypeId| implant_slot(catalog, t))
    }

    /// Implants the fit is calculated with, as (type, switched on): its own, or `character`'s
    /// when the fit uses the character's implants.
    pub fn applied_implants<'a>(
        &'a self,
        character: &'a [TypeId],
    ) -> impl Iterator<Item = (TypeId, bool)> + 'a {
        let (implants, own) = match self.implant_source {
            ImplantSource::Fit => (self.implants.as_slice(), true),
            ImplantSource::Character => (character, false),
        };
        implants
            .iter()
            .map(move |&t| (t, !own || self.implant_active(t)))
    }

    /// The mutation of the module or drone stack at `item`, if it is mutated.
    pub fn mutation(&self, item: crate::engine::FitItem) -> Option<&Mutation> {
        use crate::engine::FitItem;
        match item {
            FitItem::Module(slot, i) => self.rack(slot).get(i)?.as_ref()?.mutation.as_ref(),
            FitItem::Drone(i) => self.drones.get(i)?.mutation.as_ref(),
            _ => None,
        }
    }

    pub fn mutation_mut(&mut self, item: crate::engine::FitItem) -> Option<&mut Mutation> {
        use crate::engine::FitItem;
        match item {
            FitItem::Module(slot, i) => self.rack_mut(slot).get_mut(i)?.as_mut()?.mutation.as_mut(),
            FitItem::Drone(i) => self.drones.get_mut(i)?.mutation.as_mut(),
            _ => None,
        }
    }

    /// High, mid and low modules, then services, in that order.
    pub fn modules(&self) -> impl Iterator<Item = &Module> {
        self.high
            .iter()
            .chain(&self.mid)
            .chain(&self.low)
            .chain(&self.services)
            .flatten()
    }

    /// The modules in the high, mid and low racks, to change.
    pub fn modules_mut(&mut self) -> impl Iterator<Item = &mut Module> {
        [&mut self.high, &mut self.mid, &mut self.low]
            .into_iter()
            .flatten()
            .flatten()
    }

    pub fn launched(&self) -> u32 {
        self.drones.iter().map(|d| d.active).sum()
    }

    /// pyfa's drone state toggle: if the clicked stack has drones out, recall every stack in
    /// `stacks`, otherwise launch all of them in full. Stacks are all-or-nothing, so a partly
    /// launched stack counts as launched.
    pub fn toggle_drones(&mut self, clicked: usize, stacks: &[usize]) {
        let Some(launch) = self.drones.get(clicked).map(|d| d.active == 0) else {
            return;
        };
        for &i in stacks.iter().chain([&clicked]) {
            if let Some(stack) = self.drones.get_mut(i) {
                stack.active = if launch { stack.count } else { 0 };
            }
        }
    }

    /// Take one drone out of a stack, dropping the stack once it is empty.
    pub fn remove_drone(&mut self, i: usize) {
        let Some(stack) = self.drones.get_mut(i) else {
            return;
        };
        stack.count = stack.count.saturating_sub(1);
        stack.active = stack.active.min(stack.count);
        if stack.count == 0 {
            self.drones.remove(i);
        }
    }

    /// Fold stack `from` into stack `to`. Only stacks of the same drone merge; `to` keeps its
    /// launched count. Returns whether anything changed.
    pub fn merge_drones(&mut self, from: usize, to: usize) -> bool {
        let (Some(src), Some(dst)) = (self.drones.get(from), self.drones.get(to)) else {
            return false;
        };
        // Mutated drones only merge with drones rolled the same.
        if from == to || src.type_id != dst.type_id || src.mutation != dst.mutation {
            return false;
        }
        let moved = src.count;
        let stack = &mut self.drones[to];
        stack.count = stack.count.saturating_add(moved).min(MAX_DRONES);
        self.drones.remove(from);
        true
    }

    /// Add a copy of a stack as a stack of its own, launched state included.
    pub fn clone_drones(&mut self, i: usize) {
        if let Some(stack) = self.drones.get(i).cloned() {
            self.drones.push(stack);
        }
    }

    /// Parse an EFT block; see [`crate::port::eft::parse`].
    pub fn from_eft(catalog: &Catalog, text: &str) -> Result<(Self, Vec<String>), String> {
        crate::port::eft::parse(catalog, text)
    }

    /// Every item on the fit with where it sits, the ship first, then modules each followed by
    /// its charge (high, mid, low, services), rigs, subsystems, drones, fighters, implants,
    /// boosters and cargo. The tactical mode is no item anyone buys and is left out.
    pub fn items(&self) -> impl Iterator<Item = Item> + '_ {
        let item = |type_id, place, amount| Item {
            type_id,
            place,
            mutated: false,
            amount,
        };
        let modules = self.modules().flat_map(move |m| {
            let module = Item {
                mutated: m.mutation.is_some(),
                ..item(m.type_id, Place::Module, Some(1))
            };
            let charge = m
                .charge
                .map(|c| item(c, Place::Charge { module: m.type_id }, None));
            std::iter::once(module).chain(charge)
        });
        let drones = self.drones.iter().map(move |d| Item {
            mutated: d.mutation.is_some(),
            ..item(d.type_id, Place::Drone, Some(d.count))
        });
        self.ship
            .map(|s| item(s, Place::Ship, Some(1)))
            .into_iter()
            .chain(modules)
            .chain(
                self.rigs
                    .iter()
                    .flatten()
                    .map(move |&r| item(r, Place::Rig, Some(1))),
            )
            .chain(
                self.subsystems
                    .iter()
                    .map(move |&s| item(s, Place::Subsystem, Some(1))),
            )
            .chain(drones)
            .chain(
                self.fighters
                    .iter()
                    .map(move |f| item(f.type_id, Place::Fighter, f.amount)),
            )
            .chain(
                self.implants
                    .iter()
                    .map(move |&i| item(i, Place::Implant, Some(1))),
            )
            .chain(
                self.boosters
                    .iter()
                    .map(move |b| item(b.type_id, Place::Booster, Some(1))),
            )
            .chain(
                self.cargo
                    .iter()
                    .map(move |&(t, n)| item(t, Place::Cargo, Some(n))),
            )
    }
}

/// Where an item sits on a fit, for [`Fit::items`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Ship,
    Module,
    /// Loaded into a module of this type.
    Charge {
        module: TypeId,
    },
    Rig,
    Subsystem,
    Drone,
    Fighter,
    Implant,
    Booster,
    Cargo,
}

/// One item on a fit, as [`Fit::items`] walks them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Item {
    pub type_id: TypeId,
    pub place: Place,
    /// A mutated module or drone stack.
    pub mutated: bool,
    /// How many, when the fit alone says.
    amount: Option<u32>,
}

impl Item {
    /// How many there are: a stack's size, as many charges as the module holds, or the full
    /// squadron for a fighter kept at full size.
    pub fn count(&self, catalog: &Catalog) -> u32 {
        match (self.amount, self.place) {
            (Some(n), _) => n,
            (None, Place::Charge { module }) => {
                crate::port::charge_count(catalog, module, self.type_id)
            }
            (None, Place::Fighter) => catalog.projection.squadron_size(self.type_id).unwrap_or(1),
            (None, _) => 1,
        }
    }
}

/// The slot an implant takes; implants without one sort last.
pub fn implant_slot(catalog: &Catalog, type_id: TypeId) -> u8 {
    catalog
        .get(type_id)
        .and_then(|t| t.implant_slot)
        .unwrap_or(u8::MAX)
}

/// Add an implant to a slot-ordered list, replacing whatever is in its slot. Returns false when
/// it is already there or is not an implant.
pub fn plug_implant(implants: &mut Vec<TypeId>, catalog: &Catalog, type_id: TypeId) -> bool {
    let Some(slot) = catalog.get(type_id).and_then(|t| t.implant_slot) else {
        return false;
    };
    if implants.contains(&type_id) {
        return false;
    }
    implants.retain(|&t| implant_slot(catalog, t) != slot);
    let at = implants.partition_point(|&t| implant_slot(catalog, t) <= slot);
    implants.insert(at, type_id);
    true
}

/// Put an item into the first empty position, or after the last one.
fn place_first_free<T>(rack: &mut Vec<Option<T>>, item: T) {
    match rack.iter_mut().find(|slot| slot.is_none()) {
        Some(slot) => *slot = Some(item),
        None => rack.push(Some(item)),
    }
}

/// Drop trailing empty positions; they mean nothing past the last module.
pub fn trim_rack<T>(rack: &mut Vec<Option<T>>) {
    while rack.last().is_some_and(Option::is_none) {
        rack.pop();
    }
}

/// "Hobgoblin II x5" -> ("Hobgoblin II", Some(5)). Amounts above [`MAX_STACK`] count as that.
pub fn split_count(line: &str) -> (&str, Option<u32>) {
    if let Some((name, count)) = line.rsplit_once(" x")
        && let Ok(count) = count.trim().parse::<u32>()
    {
        return (name.trim(), Some(count.min(MAX_STACK)));
    }
    (line, None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boosters_from_old_saves_load() {
        let old: Vec<Booster> = serde_json::from_str("[10155]").expect("old boosters parse");
        assert_eq!(old, [Booster::new(10155)]);

        let booster = Booster {
            type_id: 10155,
            active: false,
            side_effects: vec![2737],
        };
        let json = serde_json::to_string(&booster).expect("booster serializes");
        let again: Booster = serde_json::from_str(&json).expect("booster parses");
        assert_eq!(again, booster);
    }
}

#[cfg(test)]
mod drone_stack_tests {
    use super::*;

    fn stack(type_id: TypeId, count: u32, active: u32) -> DroneStack {
        DroneStack::new(type_id, count, active)
    }

    fn fit_with(drones: Vec<DroneStack>) -> Fit {
        Fit {
            drones,
            ..Fit::default()
        }
    }

    fn counts(fit: &Fit) -> Vec<(TypeId, u32, u32)> {
        fit.drones
            .iter()
            .map(|d| (d.type_id, d.count, d.active))
            .collect()
    }

    #[test]
    fn toggle_follows_the_clicked_stack() {
        let mut fit = fit_with(vec![stack(1, 5, 2), stack(2, 3, 0)]);
        // The clicked stack is out, so everything targeted is recalled.
        fit.toggle_drones(0, &[0, 1]);
        assert_eq!(counts(&fit), [(1, 5, 0), (2, 3, 0)]);
        // Now it is in, so everything targeted launches in full.
        fit.toggle_drones(1, &[0, 1]);
        assert_eq!(counts(&fit), [(1, 5, 5), (2, 3, 3)]);
    }

    #[test]
    fn toggle_includes_the_clicked_stack_outside_the_selection() {
        let mut fit = fit_with(vec![stack(1, 2, 0), stack(2, 2, 0)]);
        fit.toggle_drones(1, &[0]);
        assert_eq!(counts(&fit), [(1, 2, 2), (2, 2, 2)]);
    }

    #[test]
    fn removing_one_drone_shrinks_then_drops_the_stack() {
        let mut fit = fit_with(vec![stack(1, 2, 2), stack(2, 1, 0)]);
        fit.remove_drone(0);
        assert_eq!(counts(&fit), [(1, 1, 1), (2, 1, 0)]);
        fit.remove_drone(1);
        assert_eq!(counts(&fit), [(1, 1, 1)]);
        fit.remove_drone(7);
        assert_eq!(counts(&fit), [(1, 1, 1)]);
    }

    #[test]
    fn merge_only_joins_the_same_drone() {
        let mut fit = fit_with(vec![stack(1, 2, 2), stack(2, 3, 0), stack(1, 4, 0)]);
        assert!(!fit.merge_drones(0, 1), "different drones");
        assert!(!fit.merge_drones(0, 0), "same stack");
        assert!(fit.merge_drones(2, 0));
        assert_eq!(counts(&fit), [(1, 6, 2), (2, 3, 0)]);
    }

    #[test]
    fn merge_into_a_later_stack_keeps_indices_straight() {
        let mut fit = fit_with(vec![stack(1, 2, 0), stack(2, 1, 0), stack(1, 3, 1)]);
        assert!(fit.merge_drones(0, 2));
        assert_eq!(counts(&fit), [(2, 1, 0), (1, 5, 1)]);
    }

    #[test]
    fn clone_adds_a_separate_stack() {
        let mut fit = fit_with(vec![stack(1, 3, 1)]);
        fit.clone_drones(0);
        assert_eq!(counts(&fit), [(1, 3, 1), (1, 3, 1)]);
    }
}
