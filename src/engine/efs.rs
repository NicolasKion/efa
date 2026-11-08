//! pyfa's Eve Fleet Simulator export (service/port/efs.py): the fit's numbers, weapons, repairs
//! and projections as JSON, in the layout EFS reads.

use std::collections::BTreeMap;

use rc::{
    AddMode, AttrId, EveAttrId, ItemCommon, ItemId, ItemTypeId, MinionState, ModRack, SolOwned,
    stats::{ItemStatsCommon, StatCritOptions, StatDmg, StatItemChargeOptions},
};
use serde_json::{Map, Value, json};

use super::{BuildOptions, CalcOptions, Engine, Pilot, Stats, pane};
use crate::{
    data::{Catalog, Hardpoint, Kind, Slot, TypeId},
    fit::{Fit, ModuleState},
};

/// The EFS format version pyfa writes.
const VERSION: f64 = 0.06;
/// The capsule, the hull without bonuses the weapon multipliers are measured against.
const CAPSULE: TypeId = 670;
/// pyfa's Tech II meta group, which the weapon test set is taken from.
const TECH_II: i32 = 2;
/// Fighter shield HP to EHP, pyfa's rough average fighter resist.
const FIGHTER_EHP: f64 = 0.8875;

/// Dogma attribute IDs by pyfa's names.
mod attr {
    pub const HI_SLOTS: i32 = 14;
    pub const MED_SLOTS: i32 = 13;
    pub const LOW_SLOTS: i32 = 12;
    pub const RIG_SLOTS: i32 = 1137;
    pub const TURRET_SLOTS: i32 = 102;
    pub const LAUNCHER_SLOTS: i32 = 101;
    pub const POWER_OUTPUT: i32 = 11;
    pub const CPU_OUTPUT: i32 = 48;
    pub const RIG_SIZE: i32 = 1547;
    pub const DRONE_BANDWIDTH: i32 = 1271;
    pub const SIGNATURE_RADIUS: i32 = 552;
    pub const SCAN_RESOLUTION: i32 = 564;
    pub const CAPACITOR_CAPACITY: i32 = 482;
    pub const RECHARGE_RATE: i32 = 55;
    pub const MASS: i32 = 4;
    pub const SHIELD_RECHARGE_RATE: i32 = 479;
    pub const AGILITY: i32 = 70;
    pub const ENERGY_WARFARE_RESISTANCE: i32 = 2045;
    /// EM, thermal, kinetic, explosive resonances of hull, armor and shield.
    pub const HULL_RESONANCE: [i32; 4] = [113, 110, 109, 111];
    pub const ARMOR_RESONANCE: [i32; 4] = [267, 270, 269, 268];
    pub const SHIELD_RESONANCE: [i32; 4] = [271, 274, 273, 272];
    pub const SHIELD_CAPACITY: i32 = 263;
    pub const MAX_VELOCITY: i32 = 37;
    pub const DURATION: i32 = 73;
    pub const RATE_OF_FIRE: i32 = 51;
    pub const CAPACITOR_NEED: i32 = 6;
    pub const ARMOR_DAMAGE_AMOUNT: i32 = 84;
    pub const SHIELD_BONUS: i32 = 68;
    pub const RELOAD_TIME: i32 = 1795;
    pub const CHARGED_ARMOR_DAMAGE_MULTIPLIER: i32 = 1886;
    pub const MAX_RANGE: i32 = 54;
    pub const FALLOFF: i32 = 158;
    pub const FALLOFF_EFFECTIVENESS: i32 = 2044;
    pub const SPEED_FACTOR: i32 = 20;
    pub const POWER_TRANSFER_AMOUNT: i32 = 90;
    pub const NEUT_SIG_RESOLUTION: i32 = 2451;
    pub const ENERGY_NEUTRALIZER_AMOUNT: i32 = 97;
    pub const ACTIVATION_BLOCKED_STRENGTH: i32 = 1350;
    pub const WARP_SCRAMBLE_STRENGTH: i32 = 105;
    pub const WARP_SCRAMBLE_RANGE: i32 = 103;
    pub const SIGNATURE_RADIUS_BONUS: i32 = 554;
    pub const ECM_BURST_RANGE: i32 = 142;
    pub const TRACKING_SPEED: i32 = 160;
    pub const OPTIMAL_SIG_RADIUS: i32 = 620;
    pub const DAMAGE_MULTIPLIER: i32 = 64;
    pub const DAMAGE_MULTIPLIER_BONUS_MAX: i32 = 2734;
    pub const DAMAGE_MULTIPLIER_BONUS_PER_CYCLE: i32 = 2733;
    pub const EMP_FIELD_RANGE: i32 = 99;
    pub const EXPLOSION_DELAY: i32 = 281;
    pub const AOE_DAMAGE_REDUCTION_FACTOR: i32 = 1353;
    pub const AOE_CLOUD_SIZE: i32 = 654;
    pub const AOE_VELOCITY: i32 = 653;
    /// EM, thermal, kinetic, explosive damage of a charge or drone.
    pub const DAMAGE: [i32; 4] = [114, 118, 117, 116];
}

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
            ("energyNeutralizerAmount", attr::ENERGY_NEUTRALIZER_AMOUNT),
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
    pub fits: &'a [Fit],
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
        let mut item = self.sol.get_item(&id).ok()?;
        item.get_attr(&AttrId::Eve(EveAttrId::from_i32(attr)))
            .ok()
            .map(|v| v.modified.into_f64())
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

fn total(d: &rc::stats::StatDmgEntry) -> f64 {
    (d.em + d.thermal + d.kinetic + d.explosive).into_f64()
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
    fn build_for_efs(&self, fit: &Fit, fits: &[Fit], pilot: &Pilot) -> Option<Built> {
        let options = BuildOptions {
            surroundings: Some(fits),
            default_spool: Some(1.0),
        };
        let built = self.build(fit, pilot, options)?;
        Some(Built {
            ship: built.fit.ship,
            fit_id: built.fit.fit_id,
            drones: built.fit.first_drones().collect(),
            modules: built.fit.modules,
            fighters: built.fit.fighters,
            sol: built.sol,
        })
    }

    /// The fit as pyfa's EFS export writes it. None without a ship.
    pub fn efs(&self, fit: &Fit, cx: &EfsContext<'_>) -> Option<Value> {
        let catalog = cx.catalog;
        let ship = fit.ship?;
        let ship_type = catalog.get(ship)?;
        let options = CalcOptions {
            factor_reload: false,
            default_spool: Some(1.0),
        };
        let stats = self.compute_among(fit, cx.fits, cx.pilot, options);
        let mut built = self.build_for_efs(fit, cx.fits, cx.pilot)?;
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
            let mut volley =
                |kinds| total(&s.get_stat_dmg(kinds, at, StatCritOptions::default()).volley);
            (
                volley(pane::weapons()),
                volley(pane::minions()),
                volley(rc::stats::StatDmgItemKinds::default()),
            )
        };

        let prop = self.prop_data(fit, cx, catalog, &stats);
        let multipliers = self.weapon_multipliers(fit, cx.pilot, catalog);
        let turret_slots = built.ship(attr::TURRET_SLOTS);
        let launcher_slots = built.ship(attr::LAUNCHER_SLOTS);
        let bandwidth = built.ship(attr::DRONE_BANDWIDTH);
        let round2 = |v: f64| (v * 100.0).round() / 100.0;
        let mut effective_turrets = round2(turret_slots * multipliers[0]);
        let mut effective_launchers = round2(launcher_slots * multipliers[1]);
        // A Tech II siege module, assumed for dreadnoughts.
        if ship_group == "Dreadnought" {
            effective_turrets *= 9.4;
            effective_launchers *= 15.0;
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
            Value::Array(self.weapons(fit, &mut built, catalog, &stats)),
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
        put(
            "repairs",
            Value::Array(self.repairs(fit, cx, &mut built, catalog)),
        );
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

    /// Speed and signature with the microwarpdrive off, pyfa's `getPropData`.
    fn prop_data(
        &self,
        fit: &Fit,
        cx: &EfsContext<'_>,
        catalog: &Catalog,
        stats: &Stats,
    ) -> PropData {
        let mwd = [Slot::High, Slot::Mid, Slot::Low]
            .into_iter()
            .find_map(|slot| {
                fit.rack(slot).iter().enumerate().find_map(|(i, m)| {
                    let m = m.as_ref()?;
                    let t = catalog.get(m.type_id)?;
                    let prop = catalog.group_names.get(&t.group_id).map(String::as_str)
                        == Some("Propulsion Module");
                    let blooms = self.base_attr(m.type_id, attr::SIGNATURE_RADIUS_BONUS);
                    (prop && blooms.is_some_and(|v| v != 0.0) && m.state >= ModuleState::Active)
                        .then_some((slot, i))
                })
            });
        let Some((slot, i)) = mwd else {
            return PropData {
                using_mwd: false,
                unproped_speed: stats.speed,
                unproped_sig: stats.sig_radius,
            };
        };
        let mut off = fit.clone();
        if let Some(Some(m)) = off.rack_mut(slot).get_mut(i) {
            m.state = ModuleState::Online;
        }
        let unproped = self.compute_among(&off, cx.fits, cx.pilot, CalcOptions::default());
        PropData {
            using_mwd: true,
            unproped_speed: unproped.speed,
            unproped_sig: unproped.sig_radius,
        }
    }

    /// An attribute of a type before anything modifies it.
    fn base_attr(&self, type_id: TypeId, attr: i32) -> Option<f64> {
        let mut sol = self.sol();
        let mut f = sol.add_fit();
        let id = f
            .add_module(
                ModRack::High,
                AddMode::Append,
                ItemTypeId::from_i32(type_id),
                rc::ModuleState::Offline,
            )
            .ok()?
            .get_item_id();
        let mut item = sol.get_item(&id).ok()?;
        item.get_attr(&AttrId::Eve(EveAttrId::from_i32(attr)))
            .ok()
            .map(|v| v.base.into_f64())
    }

    /// Weapons grouped by type and charge, then drones and fighters, pyfa's
    /// `getWeaponSystemData`.
    fn weapons(
        &self,
        fit: &Fit,
        built: &mut Built,
        catalog: &Catalog,
        stats: &Stats,
    ) -> Vec<Value> {
        let _ = self;
        let mut groups: BTreeMap<WeaponKey, (Slot, usize, ItemId, u32)> = BTreeMap::new();
        let mut order = Vec::new();
        for &(slot, i, id) in &built.modules.clone() {
            let Some(Some(module)) = fit.rack(slot).get(i) else {
                continue;
            };
            let dps = built.dmg(id).map_or(0.0, |d| total(&d.dps));
            if dps <= 0.0 {
                continue;
            }
            let key = (module.type_id, module.charge);
            groups
                .entry(key)
                .and_modify(|g| g.3 += 1)
                .or_insert_with(|| {
                    order.push(key);
                    (slot, i, id, 1)
                });
        }
        let mut weapons = Vec::new();
        for key in order {
            let (slot, i, id, n) = groups[&key];
            let n_f = f64::from(n);
            let (module, charge) = key;
            let Some(t) = catalog.get(module) else {
                continue;
            };
            let group = catalog
                .group_names
                .get(&t.group_id)
                .map_or("", String::as_str);
            let name = match charge {
                Some(c) => format!("{}, {}", t.name, catalog.name(c)),
                None => t.name.clone(),
            };
            let info = stats.modules.get(&(slot, i));
            let optimal = info.and_then(|m| m.optimal);
            let mut kind = "None";
            let (mut tracking, mut optimal_sig) = (0.0, 0.0);
            let (mut velocity, mut delay, mut reduction, mut radius, mut aoe_velocity) =
                (0.0, 0.0, 0.0, 0.0, 0.0);
            let mut field_range = 0.0;
            if t.hardpoint == Hardpoint::Turret {
                tracking = built.attr0(id, attr::TRACKING_SPEED);
                optimal_sig = built.attr0(id, attr::OPTIMAL_SIG_RADIUS);
                kind = "Turret";
            } else if t.hardpoint == Hardpoint::Launcher || t.name.contains("Bomb Launcher") {
                if let Some(c) = built.charge(id) {
                    velocity = built.attr0(c, attr::MAX_VELOCITY);
                    delay = built.attr0(c, attr::EXPLOSION_DELAY);
                    reduction = built.attr0(c, attr::AOE_DAMAGE_REDUCTION_FACTOR);
                    radius = built.attr0(c, attr::AOE_CLOUD_SIZE);
                    aoe_velocity = built.attr0(c, attr::AOE_VELOCITY);
                }
                kind = "Missile";
            } else if group == "Super Weapon" && optimal.is_some() {
                radius = built.attr0(id, attr::SIGNATURE_RADIUS);
                kind = "Missile";
            } else if t.hardpoint == Hardpoint::None {
                field_range = built.attr0(id, attr::EMP_FIELD_RANGE);
                kind = "SmartBomb";
            }
            let max_range = if group == "Super Weapon" && optimal.is_none() {
                Some(300_000.0)
            } else {
                optimal
            };
            let dmg = built.dmg(id);
            let dps = dmg.as_ref().map_or(0.0, |d| total(&d.dps));
            let volley = dmg.as_ref().map_or(0.0, |d| total(&d.volley));
            let spread = dmg.as_ref().map_or_else(
                || json!({"em": 0, "therm": 0, "kin": 0, "exp": 0, "pure": 0}),
                |d| {
                    let v = |x: rc::PValue| round6(x.into_f64() * n_f);
                    json!({
                        "em": v(d.dps.em), "therm": v(d.dps.thermal), "kin": v(d.dps.kinetic),
                        "exp": v(d.dps.explosive), "pure": 0,
                    })
                },
            );
            let (charges, shots) = built
                .sol
                .get_module(&id)
                .ok()
                .map_or((None, None), |mut m| {
                    (
                        m.get_charge_count().map(rc::Count::into_u32),
                        m.get_charged_cycle_count().map(rc::Count::into_u32),
                    )
                });
            let cycle = built
                .attr(id, attr::RATE_OF_FIRE)
                .filter(|&v| v > 0.0)
                .or_else(|| built.attr(id, attr::DURATION));
            weapons.push(json!({
                "dps": round6(dps * n_f),
                "capUse": round6(info.and_then(|m| m.cap_per_sec).unwrap_or(0.0) * n_f),
                "falloff": num(built.attr(id, attr::FALLOFF)),
                "type": kind,
                "name": name,
                "optimal": num(max_range),
                "numCharges": charges.unwrap_or(0),
                "numShots": shots.unwrap_or(0),
                "reloadTime": round6(built.attr0(id, attr::RELOAD_TIME)),
                "cycleTime": num(cycle),
                "volley": round6(volley * n_f),
                "tracking": round6(tracking),
                "maxVelocity": round6(velocity),
                "explosionDelay": round6(delay),
                "damageReductionFactor": round6(reduction),
                "explosionRadius": round6(radius),
                "explosionVelocity": round6(aoe_velocity),
                "aoeFieldRange": round6(field_range),
                "damageMultiplierBonusMax": num(built.attr(id, attr::DAMAGE_MULTIPLIER_BONUS_MAX)),
                "damageMultiplierBonusPerCycle": num(built.attr(id, attr::DAMAGE_MULTIPLIER_BONUS_PER_CYCLE)),
                "dps_spread": spread,
                "optimalSigRadius": round6(optimal_sig),
            }));
        }
        for (stack, id) in fit.drones.iter().zip(built.drones.clone()) {
            let Some(id) = id.filter(|_| stack.active > 0) else {
                continue;
            };
            let active = f64::from(stack.active);
            let dmg = built.dmg(id);
            let dps = dmg.as_ref().map_or(0.0, |d| total(&d.dps)) * active;
            if dps <= 0.0 {
                continue;
            }
            let volley = dmg.as_ref().map_or(0.0, |d| total(&d.volley)) * active;
            let sig = built.attr0(id, attr::OPTIMAL_SIG_RADIUS);
            // Drones use the old tracking formula; pyfa scales it to match turrets.
            let tracking = if sig > 0.0 {
                built.attr0(id, attr::TRACKING_SPEED) / (sig / 40_000.0)
            } else {
                0.0
            };
            weapons.push(json!({
                "dps": round6(dps),
                "cycleTime": num(built.attr(id, attr::RATE_OF_FIRE)),
                "type": "Drone",
                "optimal": num(built.attr(id, attr::MAX_RANGE)),
                "name": catalog.name(stack.type_id),
                "falloff": num(built.attr(id, attr::FALLOFF)),
                "maxSpeed": round6(built.attr0(id, attr::MAX_VELOCITY)),
                "tracking": round6(tracking),
                "volley": round6(volley),
            }));
        }
        for ((fighter, id), info) in fit
            .fighters
            .iter()
            .zip(built.fighters.clone())
            .zip(&stats.fighters)
        {
            let amount = info.count.map_or(0, |(now, _)| now);
            if amount == 0 {
                continue;
            }
            let dmg = built.dmg(id);
            let dps = dmg.as_ref().map_or(0.0, |d| total(&d.dps));
            if dps <= 0.0 {
                continue;
            }
            let abilities: Vec<Value> = [FIGHTER_ATTACK, FIGHTER_MISSILES]
                .iter()
                .filter_map(|ability| fighter_ability(built, id, ability, amount))
                .collect();
            weapons.push(json!({
                "dps": round6(dps),
                "type": "Fighter",
                "name": catalog.name(fighter.type_id),
                "maxSpeed": round6(built.attr0(id, attr::MAX_VELOCITY)),
                "abilities": abilities,
                "ehp": round6(built.attr0(id, attr::SHIELD_CAPACITY) / FIGHTER_EHP * f64::from(amount)),
                "volley": round6(dmg.as_ref().map_or(0.0, |d| total(&d.volley))),
                "signatureRadius": round6(built.attr0(id, attr::SIGNATURE_RADIUS)),
            }));
        }
        weapons
    }

    /// Local repairers and capacitor boosters, pyfa's `getRepairData`.
    fn repairs(
        &self,
        fit: &Fit,
        cx: &EfsContext<'_>,
        built: &mut Built,
        catalog: &Catalog,
    ) -> Vec<Value> {
        const GROUPS: [&str; 6] = [
            "Shield Booster",
            "Armor Repair Unit",
            "Ancillary Shield Booster",
            "Ancillary Armor Repairer",
            "Hull Repair Unit",
            "Capacitor Booster",
        ];
        let mut repairs = Vec::new();
        for (slot, i, id, group) in modules_in_groups(fit, built, catalog, &GROUPS) {
            let mut stats = Map::new();
            let mut put = |key: &str, value: Value| {
                stats.insert(key.to_owned(), value);
            };
            put("duration", num(built.attr(id, attr::DURATION)));
            put("capacitorNeed", num(built.attr(id, attr::CAPACITOR_NEED)));
            let shots = built
                .sol
                .get_module(&id)
                .ok()
                .and_then(|mut m| m.get_charged_cycle_count())
                .map(rc::Count::into_u32);
            match group.as_str() {
                "Armor Repair Unit" | "Ancillary Armor Repairer" => {
                    put("type", json!("Armor Repairer"));
                    put(
                        "armorDamageAmount",
                        num(built.attr(id, attr::ARMOR_DAMAGE_AMOUNT)),
                    );
                    if group == "Ancillary Armor Repairer" {
                        put("numShots", json!(shots.unwrap_or(0)));
                        put("reloadTime", num(built.attr(id, attr::RELOAD_TIME)));
                        put(
                            "chargedArmorDamageMultiplier",
                            num(built.attr(id, attr::CHARGED_ARMOR_DAMAGE_MULTIPLIER)),
                        );
                    }
                }
                "Shield Booster" | "Ancillary Shield Booster" => {
                    put("type", json!("Shield Booster"));
                    put("shieldBonus", num(built.attr(id, attr::SHIELD_BONUS)));
                    if group == "Ancillary Shield Booster" {
                        put("numShots", json!(shots.unwrap_or(0)));
                        put("reloadTime", num(built.attr(id, attr::RELOAD_TIME)));
                        // What it needs once its cap boosters run out.
                        if fit
                            .rack(slot)
                            .get(i)
                            .and_then(|m| m.as_ref()?.charge)
                            .is_some()
                        {
                            let mut empty = fit.clone();
                            if let Some(Some(m)) = empty.rack_mut(slot).get_mut(i) {
                                m.charge = None;
                            }
                            let need =
                                self.build_for_efs(&empty, cx.fits, cx.pilot)
                                    .and_then(|mut b| {
                                        let id = b
                                            .modules
                                            .iter()
                                            .find(|&&(s, j, _)| s == slot && j == i)?
                                            .2;
                                        b.attr(id, attr::CAPACITOR_NEED)
                                    });
                            put("unloadedCapacitorNeed", num(need));
                        }
                    }
                }
                "Capacitor Booster" => {
                    put("type", json!("Capacitor Booster"));
                    put("numShots", json!(shots.unwrap_or(0)));
                    put("reloadTime", num(built.attr(id, attr::RELOAD_TIME)));
                }
                _ => {}
            }
            repairs.push(Value::Object(stats));
        }
        repairs
    }

    /// How much more damage the hull's bonuses give turrets, launchers and drones, pyfa's
    /// `getWeaponBonusMultipliers`: Tech II weapons on this hull against the same on a capsule,
    /// the biggest gain of each kind.
    fn weapon_multipliers(&self, fit: &Fit, pilot: &Pilot, catalog: &Catalog) -> [f64; 3] {
        let test = test_set(catalog);
        let bare = Fit {
            ship: Some(CAPSULE),
            ..Fit::default()
        };
        let pre = self.measure(&bare, pilot, &test);
        let post = self.measure(fit, pilot, &test);
        let mut best = [0.0_f64; 3];
        for ((kind, before), (_, after)) in pre.iter().zip(&post) {
            if *before > 0.0 && after.is_finite() {
                best[*kind] = best[*kind].max(after / before);
            }
        }
        best.map(|ratio| {
            if ratio > 0.0 {
                (ratio * 1e6).round() / 1e6
            } else {
                1.0
            }
        })
    }

    /// Damage per second factor of each test weapon on the fit's hull, mode and subsystems:
    /// (kind, factor) with kinds 0 turret, 1 launcher, 2 drone.
    fn measure(&self, fit: &Fit, pilot: &Pilot, test: &[TestWeapon]) -> Vec<(usize, f64)> {
        let mut sol = self.sol();
        let mut f = sol.add_fit();
        pilot.board(&mut f);
        if let Some(ship) = fit.ship {
            f.set_ship(ItemTypeId::from_i32(ship), None, None);
        }
        if let Some(mode) = fit.mode {
            f.set_stance(ItemTypeId::from_i32(mode));
        }
        for &sub in &fit.subsystems {
            f.add_subsystem(ItemTypeId::from_i32(sub));
        }
        let mut ids = Vec::with_capacity(test.len());
        for weapon in test {
            let id = match *weapon {
                TestWeapon::Turret(t) | TestWeapon::Launcher(t, _) => {
                    let Ok(mut m) = f.add_module(
                        ModRack::High,
                        AddMode::Append,
                        ItemTypeId::from_i32(t),
                        rc::ModuleState::Active,
                    ) else {
                        ids.push(None);
                        continue;
                    };
                    if let TestWeapon::Launcher(_, charge) = *weapon {
                        m.set_charge_type_id(ItemTypeId::from_i32(charge));
                    }
                    m.get_item_id()
                }
                TestWeapon::Drone(t) => f
                    .add_drone(ItemTypeId::from_i32(t), MinionState::Engaging, None, None)
                    .get_item_id(),
            };
            ids.push(Some(id));
        }
        let mut charges = Vec::new();
        for (weapon, id) in test.iter().zip(&ids) {
            if let (TestWeapon::Launcher(..), Some(id)) = (weapon, id) {
                charges.push(
                    sol.get_module(id)
                        .ok()
                        .and_then(|mut m| m.get_charge().map(|c| c.get_item_id())),
                );
            } else {
                charges.push(None);
            }
        }
        let mut read = |id: ItemId, attr: i32| {
            sol.get_item(&id)
                .ok()
                .and_then(|mut item| item.get_attr(&AttrId::Eve(EveAttrId::from_i32(attr))).ok())
                .map_or(0.0, |v| v.modified.into_f64())
        };
        test.iter()
            .zip(ids)
            .zip(charges)
            .map(|((weapon, id), charge)| {
                let Some(id) = id else {
                    return (weapon.kind(), 0.0);
                };
                let sum = |read: &mut dyn FnMut(ItemId, i32) -> f64, id: ItemId| {
                    attr::DAMAGE.iter().map(|&a| read(id, a)).sum::<f64>()
                };
                let factor = match weapon {
                    TestWeapon::Turret(_) => {
                        let rof = read(id, attr::RATE_OF_FIRE);
                        if rof > 0.0 {
                            read(id, attr::DAMAGE_MULTIPLIER) / rof
                        } else {
                            0.0
                        }
                    }
                    TestWeapon::Launcher(..) => {
                        let rof = read(id, attr::RATE_OF_FIRE);
                        match charge {
                            Some(c) if rof > 0.0 => sum(&mut read, c) / rof,
                            _ => 0.0,
                        }
                    }
                    TestWeapon::Drone(_) => sum(&mut read, id) * read(id, attr::DAMAGE_MULTIPLIER),
                };
                (weapon.kind(), factor)
            })
            .collect()
    }
}

/// A weapon and the charge it has loaded, which pyfa groups identical weapons by.
type WeaponKey = (TypeId, Option<TypeId>);

struct PropData {
    using_mwd: bool,
    unproped_speed: Option<f64>,
    unproped_sig: Option<f64>,
}

/// A weapon of pyfa's test set.
#[derive(Debug, Clone, Copy)]
enum TestWeapon {
    Turret(TypeId),
    /// A launcher with one of the charges it takes.
    Launcher(TypeId, TypeId),
    Drone(TypeId),
}

impl TestWeapon {
    fn kind(self) -> usize {
        match self {
            Self::Turret(_) => 0,
            Self::Launcher(..) => 1,
            Self::Drone(_) => 2,
        }
    }
}

/// pyfa's `getTestSet`: Tech II turrets, one per charge group and size; Tech II launchers with
/// one charge of each family they take, for the damage-type bonuses; Tech II drones.
fn test_set(catalog: &Catalog) -> Vec<TestWeapon> {
    let mut t2: Vec<&crate::data::ItemType> = catalog
        .types
        .values()
        .filter(|t| t.published && t.meta_group == Some(TECH_II))
        .collect();
    t2.sort_by_key(|t| t.id);
    let mut set = Vec::new();
    let mut turret_kinds = Vec::new();
    for t in &t2 {
        match (t.kind, t.hardpoint) {
            (Kind::Module(_), Hardpoint::Turret) if !t.mining && !t.charge_groups.is_empty() => {
                let kind = (t.charge_groups[0], t.charge_size.map(f64::to_bits));
                if !turret_kinds.contains(&kind) {
                    turret_kinds.push(kind);
                    set.push(TestWeapon::Turret(t.id));
                }
            }
            (Kind::Module(_), Hardpoint::Launcher) => {
                let Some(&group) = t.charge_groups.first() else {
                    continue;
                };
                let mut charges: Vec<&crate::data::ItemType> = catalog
                    .types
                    .values()
                    .filter(|c| c.group_id == group && c.published && c.kind == Kind::Charge)
                    .collect();
                charges.sort_by_key(|c| c.id);
                let mut families = Vec::new();
                for c in charges {
                    let family = c.variation_parent.unwrap_or(c.id);
                    if !families.contains(&family) {
                        families.push(family);
                        set.push(TestWeapon::Launcher(t.id, c.id));
                    }
                }
            }
            (Kind::Drone, _) => set.push(TestWeapon::Drone(t.id)),
            _ => {}
        }
    }
    set
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

/// One of a fighter's damaging abilities: attribute IDs from its base name.
struct FighterAbility {
    name: &'static str,
    damage: [i32; 4],
    multiplier: i32,
    reduction_factor: i32,
    reduction_sensitivity: i32,
    explosion_radius: i32,
    explosion_velocity: i32,
    range: i32,
    duration: i32,
}

/// pyfa calls the attack missile `RegularAttack` and the missiles `MissileAttack`.
const FIGHTER_ATTACK: FighterAbility = FighterAbility {
    name: "RegularAttack",
    damage: [2227, 2228, 2230, 2229],
    multiplier: 2226,
    reduction_factor: 2231,
    reduction_sensitivity: 2232,
    explosion_radius: 2234,
    explosion_velocity: 2235,
    range: 2236,
    duration: 2233,
};

const FIGHTER_MISSILES: FighterAbility = FighterAbility {
    name: "MissileAttack",
    damage: [2131, 2132, 2134, 2133],
    multiplier: 2130,
    reduction_factor: 2127,
    reduction_sensitivity: 2128,
    explosion_radius: 2125,
    explosion_velocity: 2126,
    range: 2149,
    duration: 2182,
};

fn fighter_ability(
    built: &mut Built,
    id: ItemId,
    ability: &FighterAbility,
    amount: u32,
) -> Option<Value> {
    // pyfa checks the type has the ability's EM damage at all.
    built.attr(id, ability.damage[0])?;
    let factor = built.attr0(id, ability.reduction_factor);
    let sensitivity = built.attr0(id, ability.reduction_sensitivity);
    let reduction = if factor > 0.0 && sensitivity > 0.0 && (sensitivity - 1.0).abs() > f64::EPSILON
    {
        factor.ln() / sensitivity.ln()
    } else {
        0.0
    };
    let base: f64 = ability.damage.iter().map(|&a| built.attr0(id, a)).sum();
    let damage = base * built.attr0(id, ability.multiplier);
    Some(json!({
        "name": ability.name,
        "volley": round6(damage * f64::from(amount)),
        "explosionRadius": round6(built.attr0(id, ability.explosion_radius)),
        "explosionVelocity": round6(built.attr0(id, ability.explosion_velocity)),
        "optimal": round6(built.attr0(id, ability.range)),
        "damageReductionFactor": round6(reduction),
        "rof": round6(built.attr0(id, ability.duration)),
    }))
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
