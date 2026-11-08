//! pyfa's Eve Fleet Simulator export (service/port/efs.py): the fit's numbers, weapons, repairs
//! and projections as JSON, in the layout EFS reads.

mod multipliers;
mod repairs;
mod weapons;

use rc::{
    ItemCommon, ItemId, SolOwned,
    stats::{ItemStatsCommon, StatCritOptions, StatDmg, StatItemChargeOptions},
};
use serde_json::{Map, Value, json};

use super::{BuildOptions, CalcOptions, Damage, Engine, Pilot, Stats, pane, read};
use crate::{
    data::{Catalog, Slot, TypeId},
    dogma::attr,
    fit::{Fit, ModuleState},
};

use repairs::repairs;
use weapons::weapons;

/// The EFS format version pyfa writes.
const VERSION: f64 = 0.06;
/// What pyfa assumes a Tech II siege module does for a dreadnought's turrets and launchers.
const SIEGE_TURRET_MULTIPLIER: f64 = 9.4;
const SIEGE_LAUNCHER_MULTIPLIER: f64 = 15.0;

/// pyfa's module attributes per projection kind, by attribute name and ID.
const PROJECTION_ATTRS: &[(&str, &[(&str, i32)])] = &[
    ("Stasis Web", &[("speedFactor", attr::SPEED_FACTOR)]),
    (
        "Weapon Disruptor",
        &[
            ("trackingSpeedBonus", 767),
            ("maxRangeBonus", 351),
            ("falloffBonus", 349),
            ("aoeCloudSizeBonus", 848),
            ("aoeVelocityBonus", 847),
            ("missileVelocityBonus", 547),
            ("explosionDelayBonus", 596),
        ],
    ),
    (
        "Energy Nosferatu",
        &[
            ("powerTransferAmount", attr::POWER_TRANSFER_AMOUNT),
            (
                "energyNeutralizerSignatureResolution",
                attr::NEUT_SIG_RESOLUTION,
            ),
        ],
    ),
    (
        "Energy Neutralizer",
        &[
            (
                "energyNeutralizerSignatureResolution",
                attr::NEUT_SIG_RESOLUTION,
            ),
            ("entityCapacitorLevelModifierSmall", 1895),
            ("entityCapacitorLevelModifierMedium", 1896),
            ("entityCapacitorLevelModifierLarge", 1897),
            ("energyNeutralizerAmount", attr::NEUT_AMOUNT),
        ],
    ),
    (
        "Remote Shield Booster",
        &[("shieldBonus", attr::SHIELD_BONUS)],
    ),
    (
        "Remote Armor Repairer",
        &[("armorDamageAmount", attr::ARMOR_DAMAGE_AMOUNT)],
    ),
    (
        "Remote Capacitor Transmitter",
        &[("powerTransferAmount", attr::POWER_TRANSFER_AMOUNT)],
    ),
    (
        "Warp Scrambler",
        &[
            (
                "activationBlockedStrenght",
                attr::ACTIVATION_BLOCKED_STRENGTH,
            ),
            ("warpScrambleStrength", attr::WARP_SCRAMBLE_STRENGTH),
        ],
    ),
    (
        "Target Painter",
        &[("signatureRadiusBonus", attr::SIGNATURE_RADIUS_BONUS)],
    ),
    (
        "Sensor Dampener",
        &[("maxTargetRangeBonus", 309), ("scanResolutionBonus", 566)],
    ),
    (
        "ECM",
        &[
            ("scanGravimetricStrengthBonus", 238),
            ("scanMagnetometricStrengthBonus", 240),
            ("scanRadarStrengthBonus", 241),
            ("scanLadarStrengthBonus", 239),
        ],
    ),
    (
        "Burst Jammer",
        &[
            ("scanGravimetricStrengthBonus", 238),
            ("scanMagnetometricStrengthBonus", 240),
            ("scanRadarStrengthBonus", 241),
            ("scanLadarStrengthBonus", 239),
        ],
    ),
    ("Micro Jump Drive", &[("moduleReactivationDelay", 669)]),
];

/// The module groups pyfa exports as projections, and the kind each one counts as.
fn projection_kind(group: &str) -> Option<&'static str> {
    Some(match group {
        "Stasis Web" | "Stasis Grappler" => "Stasis Web",
        "Weapon Disruptor" => "Weapon Disruptor",
        "Energy Nosferatu" => "Energy Nosferatu",
        "Energy Neutralizer" => "Energy Neutralizer",
        "Remote Shield Booster" | "Ancillary Remote Shield Booster" => "Remote Shield Booster",
        "Remote Armor Repairer"
        | "Ancillary Remote Armor Repairer"
        | "Mutadaptive Remote Armor Repairer" => "Remote Armor Repairer",
        "Remote Capacitor Transmitter" => "Remote Capacitor Transmitter",
        "Warp Scrambler" | "Warp Disrupt Field Generator" => "Warp Scrambler",
        "Target Painter" => "Target Painter",
        "Sensor Dampener" => "Sensor Dampener",
        "ECM" => "ECM",
        "Burst Jammer" => "Burst Jammer",
        "Micro Jump Drive" => "Micro Jump Drive",
        // pyfa takes these in too but has nothing more to say about them.
        "Remote Hull Repairer"
        | "Burst Projectors"
        | "Armor Resistance Shift Hardener"
        | "Ship Modifiers"
        | "Titan Phenomena Generator"
        | "Non-Repeating Hardeners" => "",
        _ => return None,
    })
}

/// pyfa's ship size buckets, by group name.
const SHIP_SIZES: &[(&str, &[&str])] = &[
    (
        "Frigate",
        &[
            "Frigate",
            "Shuttle",
            "Corvette",
            "Assault Frigate",
            "Covert Ops",
            "Interceptor",
            "Stealth Bomber",
            "Electronic Attack Ship",
            "Expedition Frigate",
            "Logistics Frigate",
        ],
    ),
    (
        "Destroyer",
        &[
            "Destroyer",
            "Interdictor",
            "Tactical Destroyer",
            "Command Destroyer",
        ],
    ),
    (
        "Cruiser",
        &[
            "Cruiser",
            "Heavy Assault Cruiser",
            "Logistics",
            "Force Recon Ship",
            "Heavy Interdiction Cruiser",
            "Combat Recon Ship",
            "Strategic Cruiser",
        ],
    ),
    (
        "Battlecruiser",
        &[
            "Combat Battlecruiser",
            "Command Ship",
            "Attack Battlecruiser",
        ],
    ),
    (
        "Battleship",
        &["Battleship", "Elite Battleship", "Black Ops", "Marauder"],
    ),
    (
        "Capital",
        &[
            "Titan",
            "Dreadnought",
            "Freighter",
            "Carrier",
            "Supercarrier",
            "Capital Industrial Ship",
            "Jump Freighter",
            "Force Auxiliary",
        ],
    ),
    (
        "Industrial",
        &[
            "Hauler",
            "Deep Space Transport",
            "Blockade Runner",
            "Mining Barge",
            "Exhumer",
            "Industrial Command Ship",
        ],
    ),
    ("Misc", &["Capsule", "Prototype Exploration Ship"]),
];

/// What the export needs besides the fit.
pub struct EfsContext<'a> {
    pub catalog: &'a Catalog,
    /// Saved fits, where projected and command fits are looked up.
    pub fits: &'a [std::rc::Rc<Fit>],
    pub pilot: &'a Pilot,
    /// The character's name; All 5 is left out, as pyfa does.
    pub character: Option<&'a str>,
    /// Written as `pyfaVersion`, which EFS shows as the exporter.
    pub exporter: &'a str,
}

/// A refine solar system with the fit built in, for reading attributes back.
struct Built {
    sol: SolOwned,
    fit_id: rc::FitId,
    ship: ItemId,
    modules: Vec<(Slot, usize, ItemId)>,
    drones: Vec<Option<ItemId>>,
    fighters: Vec<ItemId>,
}

impl Built {
    fn attr(&mut self, id: ItemId, attr: i32) -> Option<f64> {
        read::attr(&mut self.sol, id, attr)
    }

    /// An attribute before anything modifies it.
    fn base_attr(&mut self, id: ItemId, attr: i32) -> Option<f64> {
        read::base_attr(&mut self.sol, id, attr)
    }

    fn attr0(&mut self, id: ItemId, attr: i32) -> f64 {
        self.attr(id, attr).unwrap_or(0.0)
    }

    fn ship(&mut self, attr: i32) -> f64 {
        self.attr0(self.ship, attr)
    }

    fn charge(&mut self, module: ItemId) -> Option<ItemId> {
        let mut module = self.sol.get_module(&module).ok()?;
        module.get_charge().map(|c| c.get_item_id())
    }

    /// Damage at full spool, which pyfa exports for consistency.
    fn dmg(&mut self, id: ItemId) -> Option<StatDmg> {
        let mut stats = self.sol.stats();
        let mut item = stats.get_item(&id).ok()?;
        item.get_stat_dmg(
            pane::burst_at(1.0),
            StatCritOptions::default(),
            StatItemChargeOptions::Include,
        )
        .ok()
    }
}

/// pyfa rounds every number to six places so the output only changes when the fit does.
fn round6(value: f64) -> Value {
    let rounded = (value * 1e6).round() / 1e6;
    serde_json::Number::from_f64(rounded).map_or(Value::Null, Value::Number)
}

fn num(value: Option<f64>) -> Value {
    value.map_or(Value::Null, round6)
}

impl Engine {
    /// The fit as pyfa's EFS export writes it. None without a ship.
    pub fn efs(&self, fit: &Fit, cx: &EfsContext<'_>) -> Option<Value> {
        let catalog = cx.catalog;
        let ship = fit.ship?;
        let ship_type = catalog.get(ship)?;
        let options = CalcOptions {
            factor_reload: false,
            default_spool: Some(1.0),
        };
        let settings = BuildOptions {
            default_spool: options.default_spool,
            ..BuildOptions::among(cx.fits)
        };
        let mut whole = self.build(fit, cx.pilot, settings)?;
        let stats = super::stats_of(&mut whole, fit, options);
        let mut built = Built {
            ship: whole.fit.ship,
            fit_id: whole.fit.fit_id,
            drones: whole.fit.first_drones().collect(),
            modules: whole.fit.modules,
            fighters: whole.fit.fighters,
            sol: whole.sol,
        };
        let group_name = |t: TypeId| {
            catalog
                .get(t)
                .and_then(|t| catalog.group_names.get(&t.group_id))
                .map_or("", String::as_str)
        };
        let ship_group = group_name(ship);

        let (weapon_volley, drone_volley, total_volley) = {
            let mut f = built.sol.get_fit(&built.fit_id).ok()?;
            let mut s = f.stats();
            let at = pane::burst_at(1.0);
            let mut volley = |kinds| {
                Damage::from(&s.get_stat_dmg(kinds, at, StatCritOptions::default()).volley).total()
            };
            (
                volley(pane::weapons()),
                volley(pane::minions()),
                volley(rc::stats::StatDmgItemKinds::default()),
            )
        };

        let prop = prop_data(fit, &mut built, catalog, &stats);
        let multipliers = self.weapon_multipliers(fit, cx.pilot, catalog);
        let turret_slots = built.ship(attr::TURRET_SLOTS);
        let launcher_slots = built.ship(attr::LAUNCHER_SLOTS);
        let bandwidth = built.ship(attr::DRONE_BANDWIDTH);
        let round2 = |v: f64| (v * 100.0).round() / 100.0;
        let mut effective_turrets = round2(turret_slots * multipliers[0]);
        let mut effective_launchers = round2(launcher_slots * multipliers[1]);
        if ship_group == "Dreadnought" {
            effective_turrets *= SIEGE_TURRET_MULTIPLIER;
            effective_launchers *= SIEGE_LAUNCHER_MULTIPLIER;
        }
        let resonance = |built: &mut Built, ids: [i32; 4]| {
            let [em, therm, kin, exp] = ids.map(|id| built.ship(id));
            json!({ "exp": round6(exp), "kin": round6(kin), "therm": round6(therm), "em": round6(em) })
        };
        let resonances = json!({
            "hull": resonance(&mut built, attr::HULL_RESONANCE),
            "armor": resonance(&mut built, attr::ARMOR_RESONANCE),
            "shield": resonance(&mut built, attr::SHIELD_RESONANCE),
        });
        let ship_size = SHIP_SIZES
            .iter()
            .find(|(_, groups)| groups.contains(&ship_group))
            .map_or_else(
                || format!("ShipSize not found for groupID: {}", ship_type.group_id),
                |(name, _)| (*name).to_owned(),
            );
        let (names, type_ids) = module_info(fit, catalog, &stats, cx);
        let layers = |pick: fn(&super::Layer) -> Option<f64>| {
            json!({
                "shield": num(pick(&stats.shield)),
                "armor": num(pick(&stats.armor)),
                "hull": num(pick(&stats.hull)),
            })
        };

        let mut data = Map::new();
        let mut put = |key: &str, value: Value| {
            data.insert(key.to_owned(), value);
        };
        let ship_name = catalog.name(ship);
        put("name", json!(format!("{ship_name}: {}", fit.name)));
        put("ehp", layers(|l| l.ehp));
        put("droneDPS", round6(stats.drone_dps));
        put("droneVolley", round6(drone_volley));
        put("hp", layers(|l| Some(l.hp)));
        put("maxTargets", json!(stats.locks.unwrap_or(0)));
        put("maxSpeed", num(stats.speed));
        put("weaponVolley", round6(weapon_volley));
        put("totalVolley", round6(total_volley));
        put("maxTargetRange", num(stats.lock_range));
        put("scanStrength", num(stats.sensor_strength));
        put("weaponDPS", round6(stats.firepower.weapon_dps.full));
        put("alignTime", num(stats.align_time));
        put(
            "signatureRadius",
            round6(built.ship(attr::SIGNATURE_RADIUS)),
        );
        put(
            "weapons",
            Value::Array(weapons(fit, &mut built, catalog, &stats)),
        );
        put("scanRes", round6(built.ship(attr::SCAN_RESOLUTION)));
        put("capUsed", round6(stats.cap_detail.used));
        put("capRecharge", round6(stats.cap_detail.recharge));
        put(
            "capacitorCapacity",
            round6(built.ship(attr::CAPACITOR_CAPACITY)),
        );
        put("rechargeRate", round6(built.ship(attr::RECHARGE_RATE)));
        put("rigSlots", round6(built.ship(attr::RIG_SLOTS)));
        put("lowSlots", round6(built.ship(attr::LOW_SLOTS)));
        put("midSlots", round6(built.ship(attr::MED_SLOTS)));
        put("highSlots", round6(built.ship(attr::HI_SLOTS)));
        put("turretSlots", round6(turret_slots));
        put("launcherSlots", round6(launcher_slots));
        put("powerOutput", round6(built.ship(attr::POWER_OUTPUT)));
        put("cpuOutput", round6(built.ship(attr::CPU_OUTPUT)));
        put("rigSize", num(built.attr(built.ship, attr::RIG_SIZE)));
        put("effectiveTurrets", round6(effective_turrets));
        put("effectiveLaunchers", round6(effective_launchers));
        put(
            "effectiveDroneBandwidth",
            round6(round2(bandwidth * multipliers[2])),
        );
        put("resonance", resonances);
        put("typeID", json!(ship));
        put("groupID", json!(ship_type.group_id));
        put("shipSize", json!(ship_size));
        put("droneControlRange", num(stats.misc.drone_control_range));
        put("mass", round6(built.ship(attr::MASS)));
        put(
            "shieldrechargetime",
            round6(built.ship(attr::SHIELD_RECHARGE_RATE)),
        );
        put("shipinertia", round6(built.ship(attr::AGILITY)));
        put(
            "energyWarfareResistance",
            num(built.attr(built.ship, attr::ENERGY_WARFARE_RESISTANCE)),
        );
        put("unpropedSpeed", num(prop.unproped_speed));
        put("unpropedSig", num(prop.unproped_sig));
        put("usingMWD", json!(prop.using_mwd));
        put("mwdPropSpeed", num(stats.speed));
        put(
            "projections",
            Value::Array(projections(fit, &mut built, catalog, &stats)),
        );
        put("repairs", Value::Array(repairs(fit, &mut built, catalog)));
        put("modTypeIDs", json!(type_ids));
        put("moduleNames", json!(names));
        put(
            "cargoItemIDs",
            json!(fit.cargo.iter().map(|&(t, _)| t).collect::<Vec<_>>()),
        );
        put("pyfaVersion", json!(cx.exporter));
        put("efsExportVersion", json!(VERSION));
        Some(Value::Object(data))
    }
}

/// Speed and signature with the microwarpdrive off, pyfa's `getPropData`.
fn prop_data(fit: &Fit, built: &mut Built, catalog: &Catalog, stats: &Stats) -> PropData {
    let mwd = built.modules.clone().into_iter().find(|&(slot, i, id)| {
        let Some(Some(m)) = fit.rack(slot).get(i) else {
            return false;
        };
        let prop = catalog
            .get(m.type_id)
            .and_then(|t| catalog.group_names.get(&t.group_id))
            .is_some_and(|group| group == "Propulsion Module");
        prop && m.state >= ModuleState::Active
            && built
                .base_attr(id, attr::SIGNATURE_RADIUS_BONUS)
                .is_some_and(|v| v != 0.0)
    });
    let Some((_, _, id)) = mwd else {
        return PropData {
            using_mwd: false,
            unproped_speed: stats.speed,
            unproped_sig: stats.sig_radius,
        };
    };
    let mut tx = built.sol.begin_tx();
    if let Ok(mut module) = tx.get_module(&id) {
        module.set_state(rc::ModuleState::Online);
    }
    let (unproped_speed, unproped_sig) = tx.get_fit(&built.fit_id).ok().map_or_default(|mut f| {
        let mut s = f.stats();
        (
            s.get_stat_speed().ok().map(rc::PValue::into_f64),
            s.get_stat_sig_radius().ok().map(rc::PValue::into_f64),
        )
    });
    PropData {
        using_mwd: true,
        unproped_speed,
        unproped_sig,
    }
}

struct PropData {
    using_mwd: bool,
    unproped_speed: Option<f64>,
    unproped_sig: Option<f64>,
}

/// The modules of the given groups, pyfa's `getModsInGroups`: by group, then type.
fn modules_in_groups(
    fit: &Fit,
    built: &Built,
    catalog: &Catalog,
    groups: &[&str],
) -> Vec<(Slot, usize, ItemId, String)> {
    let mut found: Vec<(i32, TypeId, Slot, usize, ItemId, String)> = built
        .modules
        .iter()
        .filter_map(|&(slot, i, id)| {
            let module = fit.rack(slot).get(i)?.as_ref()?;
            let t = catalog.get(module.type_id)?;
            let group = catalog.group_names.get(&t.group_id)?;
            groups
                .contains(&group.as_str())
                .then(|| (t.group_id, t.id, slot, i, id, group.clone()))
        })
        .collect();
    found.sort_by_key(|&(group, type_id, ..)| (group, type_id));
    found
        .into_iter()
        .map(|(_, _, slot, i, id, group)| (slot, i, id, group))
        .collect()
}

/// Projected effects the fit can apply, pyfa's `getOutgoingProjectionData`.
fn projections(fit: &Fit, built: &mut Built, catalog: &Catalog, stats: &Stats) -> Vec<Value> {
    let groups: Vec<&str> = catalog
        .group_names
        .values()
        .map(String::as_str)
        .filter(|g| projection_kind(g).is_some())
        .collect();
    let mut out = Vec::new();
    for (slot, i, id, group) in modules_in_groups(fit, built, catalog, &groups) {
        let kind = projection_kind(&group).unwrap_or_default();
        let mut stats_map = Map::new();
        if !kind.is_empty() {
            stats_map.insert("type".to_owned(), json!(kind));
        }
        if let Some((_, attrs)) = PROJECTION_ATTRS.iter().find(|(k, _)| *k == kind) {
            for &(name, attr_id) in *attrs {
                stats_map.insert(name.to_owned(), num(built.attr(id, attr_id)));
            }
        }
        let mut default_range = 0.0;
        if group == "Warp Disrupt Field Generator" {
            default_range = built.attr0(id, attr::WARP_SCRAMBLE_RANGE);
            // Disruption scripts: EFS tells scrams from points only by this.
            if default_range >= 30_000.0 {
                stats_map.insert("activationBlockedStrenght".to_owned(), json!(0));
            }
        } else if group == "Burst Jammer" {
            default_range = built.attr0(id, attr::ECM_BURST_RANGE);
        }
        let optimal = if group == "Burst Jammer" {
            stats.modules.get(&(slot, i)).and_then(|m| m.optimal)
        } else {
            built.attr(id, attr::MAX_RANGE).or(Some(default_range))
        };
        stats_map.insert("optimal".to_owned(), num(optimal));
        stats_map.insert(
            "falloff".to_owned(),
            num(built.attr(id, attr::FALLOFF_EFFECTIVENESS).or(Some(0.0))),
        );
        stats_map.insert("duration".to_owned(), num(built.attr(id, attr::DURATION)));
        stats_map.insert(
            "capacitorNeed".to_owned(),
            num(built.attr(id, attr::CAPACITOR_NEED)),
        );
        out.push(Value::Object(stats_map));
    }
    out
}

/// The fit as a list of names with headers, and the type IDs in it, pyfa's `getModuleInfo`.
fn module_info(
    fit: &Fit,
    catalog: &Catalog,
    stats: &Stats,
    cx: &EfsContext<'_>,
) -> (Vec<String>, Vec<TypeId>) {
    let mut names: Vec<String> = Vec::new();
    let mut ids: Vec<TypeId> = Vec::new();
    let rack = |header: &str, slot: Slot, names: &mut Vec<String>, ids: &mut Vec<TypeId>| {
        if !names.is_empty() {
            names.push(String::new());
        }
        names.push(header.to_owned());
        let modules = fit.rack(slot);
        let total = stats
            .slots(slot)
            .total
            .map_or(modules.len(), |t| t as usize)
            .max(modules.len());
        for i in 0..total {
            match modules.get(i).and_then(Option::as_ref) {
                Some(m) => {
                    ids.push(m.type_id);
                    match m.charge {
                        Some(c) => {
                            ids.push(c);
                            names.push(format!(
                                "{}:  {}",
                                catalog.name(m.type_id),
                                catalog.name(c)
                            ));
                        }
                        None => names.push(catalog.name(m.type_id)),
                    }
                }
                None => names.push("Empty Slot".to_owned()),
            }
        }
    };
    rack("High Slots:", Slot::High, &mut names, &mut ids);
    rack("Med Slots:", Slot::Mid, &mut names, &mut ids);
    rack("Low Slots:", Slot::Low, &mut names, &mut ids);
    names.extend([String::new(), "Rig Slots:".to_owned()]);
    let rig_total = stats
        .rig
        .total
        .map_or(fit.rigs.len(), |t| t as usize)
        .max(fit.rigs.len());
    for i in 0..rig_total {
        match fit.rigs.get(i).copied().flatten() {
            Some(rig) => {
                ids.push(rig);
                names.push(catalog.name(rig));
            }
            None => names.push("Empty Slot".to_owned()),
        }
    }
    let mut section = |header: &str, entries: Vec<(TypeId, String)>| {
        if entries.is_empty() {
            return;
        }
        names.extend([String::new(), header.to_owned()]);
        for (id, name) in entries {
            ids.push(id);
            names.push(name);
        }
    };
    section(
        "Subsystems:",
        fit.subsystems
            .iter()
            .map(|&s| (s, catalog.name(s)))
            .collect(),
    );
    section(
        "Drones:",
        fit.drones
            .iter()
            .filter(|d| d.active > 0)
            .map(|d| {
                (
                    d.type_id,
                    format!("{} x{}", catalog.name(d.type_id), d.count),
                )
            })
            .collect(),
    );
    section(
        "Fighters:",
        fit.fighters
            .iter()
            .zip(&stats.fighters)
            .filter_map(|(f, info)| {
                let amount = info.count.map_or(0, |(now, _)| now);
                (amount > 0).then(|| (f.type_id, format!("{} x{amount}", catalog.name(f.type_id))))
            })
            .collect(),
    );
    section(
        "Implants:",
        fit.applied_implants(&cx.pilot.implants)
            .map(|(t, _)| (t, catalog.name(t)))
            .collect(),
    );
    section(
        "Boosters:",
        fit.boosters
            .iter()
            .map(|b| (b.type_id, catalog.name(b.type_id)))
            .collect(),
    );
    section(
        "Command Fits:",
        fit.command
            .iter()
            .filter_map(|c| {
                let other = cx.fits.iter().find(|f| c.fit != 0 && f.uid == c.fit)?;
                Some((other.ship?, other.name.clone()))
            })
            .collect(),
    );
    section(
        "Projected Modules:",
        fit.projected
            .modules
            .iter()
            .map(|m| m.type_id)
            .chain(fit.projected.env.iter().map(|e| e.type_id))
            .map(|t| (t, catalog.name(t)))
            .collect(),
    );
    if let Some(character) = cx.character {
        names.extend([String::new(), "Character:".to_owned(), character.to_owned()]);
    }
    (names, ids)
}
