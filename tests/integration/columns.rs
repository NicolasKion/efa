//! The fitting view's engine numbers against the real data: what the Miscellanea and Heat columns
//! read, the reactive armor hardener's own pattern, and structures with services. Skipped when
//! no dump is present.

use std::path::Path;

use efa::{
    data::{Catalog, Kind, Slot},
    engine::{Engine, FitItem, Stats, misc_attr},
    fit::{DamageProfile, Fit, ModuleState, RahPattern},
};

const DATA: &str = "data/eve";

fn setup() -> Option<(Catalog, Engine)> {
    if !Path::new(DATA).join("phobos/metadata.json").exists() {
        eprintln!("no EVE data in {DATA}, skipping");
        return None;
    }
    let catalog = Catalog::load(Path::new(DATA)).expect("the dump loads");
    let engine = Engine::load(
        Path::new(DATA),
        Path::new("data/cache"),
        catalog.skills.clone(),
    )
    .expect("the engine loads");
    Some((catalog, engine))
}

fn fit(catalog: &Catalog, eft: &str) -> Fit {
    let (fit, unknown) = Fit::from_eft(catalog, eft).expect("the fit parses");
    assert!(unknown.is_empty(), "unknown items: {unknown:?}");
    fit
}

const VEXOR: &str = "[Vexor, Columns]
Reactive Armor Hardener
Damage Control II

Stasis Webifier II
Medium Shield Extender II

Heavy Neutron Blaster II, Void M
Heavy Neutron Blaster II, Void M
Small Remote Armor Repairer II
Miner II
";

fn misc(stats: &Stats, slot: Slot, i: usize) -> &efa::engine::ModuleMisc {
    stats
        .module_misc
        .get(&FitItem::Module(slot, i))
        .unwrap_or_else(|| panic!("misc data for {slot:?} {i}"))
}

#[test]
fn misc_column_reads_module_numbers() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let stats = engine.compute(&fit(&catalog, VEXOR));
    // A web's speed factor, whatever state it is in.
    assert_eq!(
        misc(&stats, Slot::Mid, 0).attr(misc_attr::SPEED_FACTOR),
        Some(-60.0)
    );
    // Turrets track; remote repairers and miners work out a rate even while passive.
    assert!(
        misc(&stats, Slot::High, 0)
            .attr(misc_attr::TRACKING_SPEED)
            .is_some()
    );
    assert!(misc(&stats, Slot::High, 2).reps[2] > 0.0);
    assert!(misc(&stats, Slot::High, 3).mining.0 > 0.0);
    // Void loaded: the charge is read too, here its missing explosion radius stays out.
    assert!(
        misc(&stats, Slot::High, 0)
            .charge_attr(misc_attr::AOE_CLOUD_SIZE)
            .is_none_or(|v| v == 0.0)
    );
}

#[test]
fn charge_column_reads_charge_count_and_cycles() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let stats = engine.compute(&fit(&catalog, VEXOR));
    // A blaster fires one charge a cycle, so its charges and cycles agree.
    let blaster = stats.modules[&(Slot::High, 0)];
    let charges = blaster.charges.expect("the blaster holds charges");
    assert!(charges > 0);
    assert_eq!(blaster.charged_cycles, Some(charges));
    // Nothing loaded, nothing counted.
    assert_eq!(stats.modules[&(Slot::High, 2)].charges, None);
}

#[test]
fn overheated_modules_get_a_burnout_time() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let mut fit = fit(&catalog, VEXOR);
    let stats = engine.compute(&fit);
    assert_eq!(stats.modules[&(Slot::High, 0)].burnout, None);
    for m in fit.high.iter_mut().take(2).flatten() {
        m.state = ModuleState::Overload;
    }
    let stats = engine.compute(&fit);
    let burnout = stats.modules[&(Slot::High, 0)].burnout.expect("burns out");
    assert!(burnout > 0.0 && burnout < 3600.0, "{burnout}");
    assert!(!stats.modules[&(Slot::High, 0)].burnout_from_neighbours);
    // The modules in the rack that run cold burn too, from their neighbours' heat, and later.
    let cold: Vec<_> = (2..fit.high.len())
        .filter(|&i| fit.high[i].is_some())
        .map(|i| stats.modules[&(Slot::High, i)])
        .collect();
    assert!(!cold.is_empty(), "the Vexor has a cold high slot module");
    for m in cold {
        assert!(m.burnout_from_neighbours);
        assert!(m.burnout.expect("burns from its neighbours") > burnout);
    }
}

/// The hardener's armor resists, EM to explosive, as fractions resisted.
fn rah_resists(stats: &Stats) -> [f64; 4] {
    let m = misc(stats, Slot::Low, 0);
    [
        misc_attr::ARMOR_EM_RESONANCE,
        misc_attr::ARMOR_THERMAL_RESONANCE,
        misc_attr::ARMOR_KINETIC_RESONANCE,
        misc_attr::ARMOR_EXPLOSIVE_RESONANCE,
    ]
    .map(|a| 1.0 - m.attr(a).expect("the hardener has armor resonances"))
}

#[test]
fn reactive_hardener_follows_its_own_pattern() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let mut fit = fit(&catalog, VEXOR);
    let set = |fit: &mut Fit, rah: Option<RahPattern>| {
        fit.low[0].as_mut().expect("the hardener").rah = rah;
    };

    set(&mut fit, Some(RahPattern::DoNotAdapt));
    let still = rah_resists(&engine.compute(&fit));
    for resist in still {
        assert!((resist - 0.15).abs() < 1e-6, "{still:?}");
    }

    let em = DamageProfile::Pattern {
        name: "EM".to_owned(),
        amounts: [1.0, 0.0, 0.0, 0.0],
    };
    set(&mut fit, Some(RahPattern::Pattern(em)));
    let shifted = rah_resists(&engine.compute(&fit));
    assert!(shifted[0] > 0.15, "{shifted:?}");
    assert!(shifted[0] > shifted[2], "{shifted:?}");
}

#[test]
fn structures_fit_services() {
    let Some((catalog, engine)) = setup() else {
        return;
    };
    let astrahus = catalog.find("Astrahus").expect("Astrahus");
    assert_eq!(astrahus.kind, Kind::Ship);
    assert!(astrahus.is_structure());
    let plant = catalog
        .find("Standup Manufacturing Plant I")
        .expect("the plant");
    assert_eq!(plant.kind, Kind::Module(Slot::Service));

    let mut fit = Fit::new("test");
    assert!(fit.add(&catalog, astrahus.id));
    assert!(fit.add(&catalog, plant.id));
    assert_eq!(fit.services.iter().flatten().count(), 1);
    let stats = engine.compute(&fit);
    assert!(
        stats.service.total.is_some_and(|n| n >= 1),
        "{:?}",
        stats.service
    );
    assert_eq!(stats.service.used, 1);

    // Services go in and out of EFT text in their own section.
    let text = fit.to_eft(&catalog);
    assert!(text.contains("Standup Manufacturing Plant I"), "{text}");
    let (back, unknown) = Fit::from_eft(&catalog, &text).expect("parses");
    assert!(unknown.is_empty(), "{unknown:?}");
    assert_eq!(back.services.iter().flatten().count(), 1);
}
