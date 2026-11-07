//! Mutaplasmids against the real data: what the catalog knows, what refine makes of the rolls,
//! and pyfa's mutated item text. Skipped when no dump is present.

use std::path::Path;

use gyfa::{
    data::Catalog,
    engine::{Engine, FitItem},
    fit::{Fit, Module, Mutation},
    port::{eft, muta},
};

const DATA: &str = "data/eve";

const MWD: i32 = 12076;
const UNSTABLE_MWD: i32 = 47297;
const ABYSSAL_MWD: i32 = 47408;
const HOBGOBLIN: i32 = 2456;
const FIREPOWER: i32 = 60461;
const SPEED_FACTOR: i32 = 20;
const CAP_NEED: i32 = 6;
const DAMAGE_MULTIPLIER: i32 = 64;

const VEXOR: &str = "[Vexor, Muta Vexor]
Drone Damage Amplifier II

50MN Microwarpdrive II [1]
Stasis Webifier II

Heavy Neutron Blaster II, Void M


Hobgoblin II x5 [2]
Hammerhead II x2


[1] 50MN Microwarpdrive II
  Unstable 50MN Microwarpdrive Mutaplasmid
  capacitorNeed 180.0, cpu 60.0, power 200.0, signatureRadiusBonus 550.0, speedFactor 550.0
[2] Hobgoblin II
  Exigent Light Drone Firepower Mutaplasmid
  damageMultiplier 2.5
";

fn catalog() -> Option<Catalog> {
    if !Path::new(DATA).join("phobos/metadata.json").exists() {
        eprintln!("no EVE data in {DATA}, skipping");
        return None;
    }
    Some(Catalog::load(Path::new(DATA)).expect("the dump loads"))
}

fn engine(catalog: &Catalog) -> Engine {
    Engine::load(
        Path::new(DATA),
        Path::new("data/cache"),
        catalog.skills.clone(),
    )
    .expect("the engine loads")
}

#[test]
fn catalog_knows_mutaplasmids() {
    let Some(catalog) = catalog() else { return };
    let mutas = &catalog.mutaplasmids;
    let names: Vec<String> = mutas
        .for_base(MWD)
        .iter()
        .map(|&m| gyfa::data::muta::short_name(&catalog.name(m)))
        .collect();
    assert_eq!(
        names,
        [
            "Decayed",
            "Gl. Decayed",
            "Gravid",
            "Gl. Gravid",
            "Unstable",
            "Gl. Unstable"
        ]
    );
    assert_eq!(mutas.result(UNSTABLE_MWD, MWD), Some(ABYSSAL_MWD));
    let limits = mutas
        .limits(UNSTABLE_MWD, MWD, SPEED_FACTOR)
        .expect("speed factor rolls");
    assert_eq!(limits.base, 510.0);
    assert!((limits.low() - 459.0).abs() < 1e-9 && (limits.high() - 561.0).abs() < 1e-9);
    assert_eq!(mutas.attrs[&SPEED_FACTOR].name, "speedFactor");
    // The abyssal type reads what it lacks from a base item: it is still a mid slot module.
    let abyssal = catalog.get(ABYSSAL_MWD).expect("abyssal MWD");
    assert_eq!(
        abyssal.kind,
        gyfa::data::Kind::Module(gyfa::data::Slot::Mid)
    );
    assert!(abyssal.activatable);
}

#[test]
fn mutating_starts_at_base_values_and_clamps() {
    let Some(catalog) = catalog() else { return };
    let mut module = Module::new(&catalog, MWD);
    assert!(module.mutate(&catalog, UNSTABLE_MWD));
    assert_eq!(module.type_id, ABYSSAL_MWD);
    assert_eq!(module.base_type(), MWD);
    let mutation = module.mutation.as_mut().expect("mutated");
    assert_eq!(mutation.attrs[&SPEED_FACTOR], 510.0);
    assert_eq!(mutation.set(&catalog, SPEED_FACTOR, 9999.0), Some(561.0));
    assert_eq!(
        mutation.full_name(&catalog, ABYSSAL_MWD),
        "Unstable 50MN Microwarpdrive II"
    );
    assert!(!module.mutate(&catalog, UNSTABLE_MWD), "already mutated");
    assert!(module.revert());
    assert_eq!(module.type_id, MWD);
    assert!(module.mutation.is_none());
}

#[test]
fn engine_applies_rolls() {
    let Some(catalog) = catalog() else { return };
    let engine = engine(&catalog);
    let (plain, unknown) = Fit::from_eft(
        &catalog,
        "[Vexor, Plain]\n50MN Microwarpdrive II\n\nHobgoblin II x5",
    )
    .expect("parses");
    assert_eq!(unknown, Vec::<String>::new());
    let mut rolled = plain.clone();
    let module = rolled.mid[0].as_mut().expect("MWD");
    assert!(module.mutate(&catalog, UNSTABLE_MWD));
    let mutation = module.mutation.as_mut().expect("mutated");
    mutation.set(&catalog, SPEED_FACTOR, 561.0);
    mutation.set(&catalog, CAP_NEED, 96.0);
    assert!(rolled.drones[0].mutate(&catalog, FIREPOWER));
    rolled.drones[0].mutation.as_mut().expect("mutated").set(
        &catalog,
        DAMAGE_MULTIPLIER,
        1.92 * 1.2,
    );

    let pilot = engine.all5().clone();
    let inspect = |fit: &Fit, item, attr| {
        engine
            .inspect(fit, &pilot, item)
            .and_then(|i| i.attr(attr).copied())
            .expect("attribute is there")
    };
    let mwd = FitItem::Module(gyfa::data::Slot::Mid, 0);
    assert_eq!(inspect(&plain, mwd, SPEED_FACTOR).base, 510.0);
    assert!((inspect(&rolled, mwd, SPEED_FACTOR).base - 561.0).abs() < 1e-6);
    assert!((inspect(&rolled, mwd, CAP_NEED).base - 96.0).abs() < 1e-6);
    let drone = FitItem::Drone(0);
    let plain_damage = inspect(&plain, drone, DAMAGE_MULTIPLIER).modified;
    let rolled_damage = inspect(&rolled, drone, DAMAGE_MULTIPLIER).modified;
    assert!(
        (rolled_damage / plain_damage - 1.2).abs() < 1e-6,
        "{plain_damage} -> {rolled_damage}"
    );

    let (plain, rolled) = (engine.compute(&plain), engine.compute(&rolled));
    assert!(rolled.drone_dps > plain.drone_dps * 1.19);
}

#[test]
fn eft_reads_and_writes_mutations() {
    let Some(catalog) = catalog() else { return };
    let (fit, unknown) = Fit::from_eft(&catalog, VEXOR).expect("the fit parses");
    assert!(unknown.is_empty(), "unknown items: {unknown:?}");
    let mwd = fit.mid[0].as_ref().expect("MWD");
    assert_eq!(mwd.type_id, ABYSSAL_MWD);
    let mutation = mwd.mutation.as_ref().expect("mutated");
    assert_eq!(mutation.attrs[&SPEED_FACTOR], 550.0);
    assert_eq!(mutation.attrs[&CAP_NEED], 180.0);
    assert!(fit.mid[1].as_ref().is_some_and(|m| m.mutation.is_none()));
    assert_eq!(fit.drones.len(), 2);
    let hobgoblins = &fit.drones[0];
    assert_eq!((hobgoblins.base_type(), hobgoblins.count), (HOBGOBLIN, 5));
    // 2.5 is out of the firepower mutaplasmid's range and is pulled back to 1.2 times 1.92.
    let damage = hobgoblins.mutation.as_ref().expect("mutated").attrs[&DAMAGE_MULTIPLIER];
    assert!((damage - 2.304).abs() < 1e-9, "{damage}");

    let exported = eft::export(&fit, &catalog, &eft::Options::default(), None);
    assert!(
        exported.contains("50MN Microwarpdrive II [1]\n"),
        "{exported}"
    );
    assert!(exported.contains("Hobgoblin II x5 [2]\n"), "{exported}");
    assert!(
        exported.contains(
            "\n\n\n[1] 50MN Microwarpdrive II\n  Unstable 50MN Microwarpdrive Mutaplasmid\n  \
             capacitorNeed 180.0, cpu 60.0, power 200.0, signatureRadiusBonus 550.0, \
             speedFactor 550.0\n[2] Hobgoblin II\n  Exigent Light Drone Firepower Mutaplasmid\n  \
             armorHP 108.0, damageMultiplier 2.304, "
        ),
        "{exported}"
    );
    let (again, unknown) = Fit::from_eft(&catalog, &exported).expect("the export parses");
    assert!(unknown.is_empty(), "{unknown:?}");
    assert_eq!(
        again.mid[0].as_ref().and_then(|m| m.mutation.clone()),
        Some(mutation.clone())
    );
    assert_eq!(again.drones[0].mutation, fit.drones[0].mutation);

    let plain = eft::Options {
        mutations: false,
        ..eft::Options::default()
    };
    let exported = eft::export(&fit, &catalog, &plain, None);
    assert!(!exported.contains("[1]"), "{exported}");
}

#[test]
fn muta_text_round_trips() {
    let Some(catalog) = catalog() else { return };
    let (_, mutation) = Mutation::with_values(&catalog, MWD, UNSTABLE_MWD, [(SPEED_FACTOR, 530.5)])
        .expect("mutates");
    let text = muta::render(&catalog, &mutation, "", "");
    assert_eq!(
        text,
        "50MN Microwarpdrive II\nUnstable 50MN Microwarpdrive Mutaplasmid\n\
         capacitorNeed 160.0, cpu 50.0, power 165.0, signatureRadiusBonus 475.0, \
         speedFactor 530.5"
    );
    let parsed = muta::parse_single(&catalog, &text).expect("one mutated item");
    let (result, again) = parsed.mutation(&catalog).expect("mutates again");
    assert_eq!(result, ABYSSAL_MWD);
    assert_eq!(again, mutation);
    // A plain module name is not a mutated item.
    assert!(muta::parse_single(&catalog, "50MN Microwarpdrive II").is_none());
}
