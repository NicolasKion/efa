//! The fit as the user sees it. This is the source of truth; the engine rebuilds its own state from
//! it whenever it changes.

use std::fmt::Write;

use serde::{Deserialize, Serialize};

use crate::data::{Catalog, Kind, Slot, TypeId};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module {
    pub type_id: TypeId,
    pub state: ModuleState,
    pub charge: Option<TypeId>,
    /// Spool cycles to assume, for modules that spool up. None means fully spooled.
    #[serde(default)]
    pub spool: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DroneStack {
    pub type_id: TypeId,
    pub count: u32,
    pub active: u32,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
}

/// Drones a character can have in space at once, without skills or ship bonuses changing it.
const MAX_LAUNCHED: u32 = 5;

impl Fit {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            ..Self::default()
        }
    }

    pub fn rack(&self, slot: Slot) -> &Vec<Option<Module>> {
        match slot {
            Slot::High => &self.high,
            Slot::Mid => &self.mid,
            _ => &self.low,
        }
    }

    pub fn rack_mut(&mut self, slot: Slot) -> &mut Vec<Option<Module>> {
        match slot {
            Slot::High => &mut self.high,
            Slot::Mid => &mut self.mid,
            _ => &mut self.low,
        }
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
            Kind::Module(slot @ (Slot::High | Slot::Mid | Slot::Low)) => {
                let state = if t.activatable {
                    ModuleState::Active
                } else {
                    ModuleState::Online
                };
                place_first_free(
                    self.rack_mut(slot),
                    Module {
                        type_id,
                        state,
                        charge: None,
                        spool: None,
                    },
                );
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
            Kind::Drone | Kind::Fighter => {
                let launch = self.launched() < MAX_LAUNCHED;
                match self.drones.iter_mut().find(|d| d.type_id == type_id) {
                    Some(stack) => {
                        stack.count += 1;
                        stack.active += u32::from(launch);
                    }
                    None => self.drones.push(DroneStack {
                        type_id,
                        count: 1,
                        active: u32::from(launch),
                    }),
                }
            }
            Kind::Implant => self.implants.push(type_id),
            Kind::Booster => {
                self.add_booster(catalog, Booster::new(type_id));
            }
            Kind::Mode => self.mode = Some(type_id),
            Kind::Skill | Kind::Other => return false,
        }
        true
    }

    /// Put `count` of an item into the cargo hold, on top of any stack already there.
    pub fn add_cargo(&mut self, type_id: TypeId, count: u32) {
        match self.cargo.iter_mut().find(|(t, _)| *t == type_id) {
            Some((_, n)) => *n += count,
            None => self.cargo.push((type_id, count)),
        }
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
                stack.count += count;
                stack.active += launch;
            }
            None => self.drones.push(DroneStack {
                type_id,
                count,
                active: launch,
            }),
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

    /// High, mid and low modules, in that order.
    pub fn modules(&self) -> impl Iterator<Item = &Module> {
        self.high.iter().chain(&self.mid).chain(&self.low).flatten()
    }

    pub fn launched(&self) -> u32 {
        self.drones.iter().map(|d| d.active).sum()
    }

    pub fn to_eft(&self, catalog: &Catalog) -> String {
        let mut out = String::new();
        let ship = self
            .ship
            .map_or_else(|| "No ship".to_owned(), |s| catalog.name(s));
        let _ = writeln!(out, "[{ship}, {}]", self.name);

        let module_line = |empty: &'static str, m: &Option<Module>| {
            let Some(m) = m else {
                return format!("[Empty {empty} slot]");
            };
            let mut line = catalog.name(m.type_id);
            if let Some(charge) = m.charge {
                let _ = write!(line, ", {}", catalog.name(charge));
            }
            if m.state == ModuleState::Offline {
                line.push_str(" /OFFLINE");
            }
            line
        };
        let mut sections: Vec<Vec<String>> = vec![
            self.low.iter().map(|m| module_line("Low", m)).collect(),
            self.mid.iter().map(|m| module_line("Med", m)).collect(),
            self.high.iter().map(|m| module_line("High", m)).collect(),
            self.rigs
                .iter()
                .map(|r| r.map_or_else(|| "[Empty Rig slot]".to_owned(), |r| catalog.name(r)))
                .collect(),
            self.subsystems
                .iter()
                .chain(&self.mode)
                .map(|&s| catalog.name(s))
                .collect(),
            self.drones
                .iter()
                .map(|d| format!("{} x{}", catalog.name(d.type_id), d.count))
                .collect(),
            self.implants.iter().map(|&i| catalog.name(i)).collect(),
            self.boosters
                .iter()
                .map(|b| catalog.name(b.type_id))
                .collect(),
            self.cargo
                .iter()
                .map(|&(t, n)| format!("{} x{n}", catalog.name(t)))
                .collect(),
        ];
        sections.retain(|s| !s.is_empty());
        for (n, section) in sections.into_iter().enumerate() {
            if n > 0 {
                out.push('\n');
            }
            for line in section {
                out.push_str(&line);
                out.push('\n');
            }
        }
        out
    }

    /// Parse an EFT block. Lines are placed by what the item is rather than which section it sits
    /// in, so slightly mangled pastes still come through. Unknown names are returned alongside.
    pub fn from_eft(catalog: &Catalog, text: &str) -> Result<(Self, Vec<String>), String> {
        let mut lines = text.lines().map(str::trim).skip_while(|l| l.is_empty());
        let header = lines.next().ok_or("nothing to import")?;
        let header = header
            .strip_prefix('[')
            .and_then(|h| h.strip_suffix(']'))
            .ok_or("an EFT fit starts with [Ship, Name]")?;
        let (ship_name, fit_name) = header.split_once(',').unwrap_or((header, ""));
        let ship = catalog
            .find(ship_name)
            .filter(|t| t.kind == Kind::Ship)
            .ok_or_else(|| format!("unknown ship \"{}\"", ship_name.trim()))?;

        let mut fit = Fit::new(fit_name.trim());
        fit.ship = Some(ship.id);
        fit.mode = catalog.modes_for(ship.id).first().copied();
        let mut unknown = Vec::new();
        let mut in_cargo_section = false;
        let mut seen_stacks = false;

        for line in lines {
            if line.is_empty() {
                // Charges after the drone section are cargo, not ammo for anything.
                in_cargo_section = seen_stacks;
                continue;
            }
            if let Some(empty) = line
                .strip_prefix("[Empty ")
                .and_then(|l| l.strip_suffix(" slot]"))
            {
                match empty {
                    "High" => fit.high.push(None),
                    "Med" => fit.mid.push(None),
                    "Low" => fit.low.push(None),
                    "Rig" => fit.rigs.push(None),
                    _ => {}
                }
                continue;
            }
            let (line, offline) = match line.strip_suffix("/OFFLINE") {
                Some(rest) => (rest.trim(), true),
                None => (line, false),
            };
            let (name, count) = split_count(line);
            let (name, charge) = match count {
                Some(_) => (name, None),
                None => match name.split_once(", ") {
                    Some((module, charge)) => (module, Some(charge)),
                    None => (name, None),
                },
            };
            let Some(t) = catalog.find(name) else {
                unknown.push(name.to_owned());
                continue;
            };
            match (t.kind, count) {
                (Kind::Drone | Kind::Fighter, Some(n)) => {
                    seen_stacks = true;
                    fit.drones.push(DroneStack {
                        type_id: t.id,
                        count: n,
                        active: 0,
                    });
                }
                (_, Some(n)) => {
                    seen_stacks = true;
                    fit.cargo.push((t.id, n));
                }
                (Kind::Charge, None) if in_cargo_section => fit.cargo.push((t.id, 1)),
                (Kind::Module(slot @ (Slot::High | Slot::Mid | Slot::Low)), None) => {
                    let state = match (offline, t.activatable) {
                        (true, _) => ModuleState::Offline,
                        (false, true) => ModuleState::Active,
                        (false, false) => ModuleState::Online,
                    };
                    let charge = charge.and_then(|name| {
                        let found = catalog.find(name).map(|c| c.id);
                        if found.is_none() {
                            unknown.push(name.to_owned());
                        }
                        found
                    });
                    fit.rack_mut(slot).push(Some(Module {
                        type_id: t.id,
                        state,
                        charge,
                        spool: None,
                    }));
                }
                // EFT lists rigs in position, empty slots included.
                (Kind::Module(Slot::Rig), None) => fit.rigs.push(Some(t.id)),
                _ => {
                    fit.add(catalog, t.id);
                }
            }
        }
        // Launch drones in the order they were listed, up to what can be in space.
        let mut room = MAX_LAUNCHED;
        for stack in &mut fit.drones {
            stack.active = stack.count.min(room);
            room -= stack.active;
        }
        Ok((fit, unknown))
    }
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

/// "Hobgoblin II x5" -> ("Hobgoblin II", Some(5))
pub fn split_count(line: &str) -> (&str, Option<u32>) {
    if let Some((name, count)) = line.rsplit_once(" x")
        && let Ok(count) = count.trim().parse()
    {
        return (name.trim(), Some(count));
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
