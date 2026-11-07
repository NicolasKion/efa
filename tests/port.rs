//! Every import and export format against real fits and the real data. Skipped when no dump is
//! present.

use std::path::Path;

use gyfa::{
    data::Catalog,
    fit::{Fit, ModuleState},
    port::{self, dna, eft, esi, html, multibuy, xml},
};

const DATA: &str = "data/eve";

const VEXOR: &str = "[Vexor, Drone Vexor]
Drone Damage Amplifier II
Drone Damage Amplifier II
Medium Armor Repairer II
Multispectrum Energized Membrane II
Damage Control II /OFFLINE

50MN Microwarpdrive II
Medium Cap Battery II
Warp Scrambler II

Heavy Neutron Blaster II, Void M
Heavy Neutron Blaster II, Void M
Heavy Neutron Blaster II, Void M
Heavy Neutron Blaster II, Void M

Medium Auxiliary Nano Pump I
Medium Capacitor Control Circuit I
Medium Capacitor Control Circuit I

Hammerhead II x5
Hobgoblin II x5

Void M x2000
";

const TENGU: &str = "[Tengu, Missile Tengu]
Ballistic Control System II
Ballistic Control System II
Damage Control II

Large Shield Extender II
Large Shield Extender II
Multispectrum Shield Hardener II
50MN Microwarpdrive II
Warp Disruptor II

Heavy Assault Missile Launcher II, Scourge Rage Heavy Assault Missile
Heavy Assault Missile Launcher II, Scourge Rage Heavy Assault Missile
[Empty High slot]
Heavy Assault Missile Launcher II, Scourge Rage Heavy Assault Missile

Medium Core Defense Field Extender I
Medium Core Defense Field Extender I
Medium Core Defense Field Extender I

Tengu Core - Electronic Efficiency Gate
Tengu Defensive - Supplemental Screening
Tengu Offensive - Accelerated Ejection Bay
Tengu Propulsion - Interdiction Nullifier

High-grade Snake Alpha
Strong Blue Pill Booster

Scourge Rage Heavy Assault Missile x1000
Nanite Repair Paste x50
";

fn catalog() -> Option<Catalog> {
    if !Path::new(DATA).join("phobos/metadata.json").exists() {
        eprintln!("no EVE data in {DATA}, skipping");
        return None;
    }
    Some(Catalog::load_cached(Path::new(DATA), Path::new("data/cache")).expect("the dump loads"))
}

fn fit(catalog: &Catalog, text: &str) -> Fit {
    let (fit, unknown) = Fit::from_eft(catalog, text).expect("the fit parses");
    assert!(unknown.is_empty(), "unknown items: {unknown:?}");
    fit
}

fn one(imported: port::Imported) -> Fit {
    assert!(
        imported.unknown.is_empty(),
        "unknown items: {:?}",
        imported.unknown
    );
    assert_eq!(imported.fits.len(), 1, "one fit expected");
    imported.fits.into_iter().next().expect("one fit")
}

/// Fitted types by rack, ignoring positions and states, for formats that keep neither.
fn fitted(fit: &Fit) -> Vec<Vec<i32>> {
    let sorted = |mut v: Vec<i32>| {
        v.sort_unstable();
        v
    };
    vec![
        sorted(fit.high.iter().flatten().map(|m| m.type_id).collect()),
        sorted(fit.mid.iter().flatten().map(|m| m.type_id).collect()),
        sorted(fit.low.iter().flatten().map(|m| m.type_id).collect()),
        sorted(fit.rigs.iter().flatten().copied().collect()),
        sorted(fit.subsystems.clone()),
        sorted(fit.drones.iter().map(|d| d.type_id).collect()),
    ]
}

#[test]
fn eft_in_pyfa_layout_round_trips() {
    let Some(catalog) = catalog() else { return };
    for text in [VEXOR, TENGU] {
        let original = fit(&catalog, text);
        let options = eft::Options::default();
        let exported = eft::export(&original, &catalog, &options, None);
        let again = one(port::import(&catalog, &exported, None).expect("the export imports"));
        assert_eq!(
            eft::export(&again, &catalog, &options, None),
            exported,
            "export is stable"
        );
        assert_eq!(again.implants, original.implants);
        assert_eq!(again.boosters, original.boosters);
        assert_eq!(again.cargo.len(), original.cargo.len());
        assert_eq!(fitted(&again), fitted(&original));
    }
}

#[test]
fn eft_keeps_offline_state_and_empty_positions() {
    let Some(catalog) = catalog() else { return };
    let original = fit(&catalog, VEXOR);
    let exported = eft::export(&original, &catalog, &eft::Options::default(), None);
    assert!(
        exported.contains("Damage Control II /offline"),
        "{exported}"
    );
    let again = one(port::import(&catalog, &exported, None).expect("the export imports"));
    let dc = again.low[4]
        .as_ref()
        .expect("the damage control is in place");
    assert_eq!(dc.state, ModuleState::Offline);

    let tengu = fit(&catalog, TENGU);
    assert!(
        tengu.high[2].is_none(),
        "the empty high slot stays a position"
    );
    let exported = eft::export(&tengu, &catalog, &eft::Options::default(), None);
    let again = one(port::import(&catalog, &exported, None).expect("the export imports"));
    assert!(again.high[2].is_none());
}

#[test]
fn eft_options_leave_things_out() {
    let Some(catalog) = catalog() else { return };
    let tengu = fit(&catalog, TENGU);
    let bare = eft::Options {
        loaded_charges: false,
        implants: false,
        boosters: false,
        cargo: false,
        mutations: false,
    };
    let exported = eft::export(&tengu, &catalog, &bare, None);
    for gone in ["Scourge", "Snake", "Blue Pill", "Nanite"] {
        assert!(!exported.contains(gone), "{gone} left in:\n{exported}");
    }
    let sizes = eft::RackSizes {
        high: 6,
        mid: 5,
        low: 3,
        rig: 3,
    };
    let padded = eft::export(&tengu, &catalog, &bare, Some(sizes));
    assert_eq!(padded.matches("[Empty High slot]").count(), 3, "{padded}");
}

#[test]
fn several_eft_fits_in_one_paste() {
    let Some(catalog) = catalog() else { return };
    let text = format!("{VEXOR}\n\n{TENGU}");
    let imported = port::import(&catalog, &text, None).expect("both import");
    assert!(imported.unknown.is_empty(), "{:?}", imported.unknown);
    let names: Vec<&str> = imported.fits.iter().map(|f| f.name.as_str()).collect();
    assert_eq!(names, ["Drone Vexor", "Missile Tengu"]);
}

#[test]
fn eft_config_files_import() {
    let Some(catalog) = catalog() else { return };
    let cfg = "[Brawler]
Drone Damage Amplifier II
Heavy Neutron Blaster II,Void M
Drones_Active=Hobgoblin II,5
Drones_Inactive=Hammerhead II,3
Implant_Active=High-grade Snake Alpha
Booster_Inactive=Strong Blue Pill Booster
Cargohold=Void M,500
Description=Close range|brawler

[Second]
Damage Control II
";
    let imported = port::import(&catalog, cfg, Some("Vexor.cfg")).expect("the config imports");
    assert!(imported.unknown.is_empty(), "{:?}", imported.unknown);
    assert_eq!(imported.fits.len(), 2);
    let brawler = &imported.fits[0];
    assert_eq!(brawler.name, "Brawler");
    assert_eq!(brawler.drones.len(), 2);
    assert_eq!((brawler.drones[0].count, brawler.drones[0].active), (5, 5));
    assert_eq!((brawler.drones[1].count, brawler.drones[1].active), (3, 0));
    assert!(brawler.high[0].as_ref().is_some_and(|m| m.charge.is_some()));
    assert!(!brawler.boosters[0].active);
    assert_eq!(brawler.cargo.len(), 1);
    assert_eq!(brawler.notes, "Close range\nbrawler");
}

#[test]
fn dna_round_trips() {
    let Some(catalog) = catalog() else { return };
    for text in [VEXOR, TENGU] {
        let original = fit(&catalog, text);
        let plain = dna::export(&original, &catalog, &dna::Options { formatting: false });
        assert!(plain.ends_with("::"), "{plain}");
        let again = one(port::import(&catalog, &plain, None).expect("the DNA imports"));
        assert_eq!(fitted(&again), fitted(&original));
        assert_eq!(again.ship, original.ship);
        // Loaded charges come back as cargo, as in pyfa.
        assert_ne!(again.cargo.len(), 0);

        let link = dna::export(&original, &catalog, &dna::Options { formatting: true });
        let again = one(port::import(&catalog, &link, None).expect("the chat link imports"));
        assert_eq!(again.name, original.name);
        assert_eq!(fitted(&again), fitted(&original));
    }
}

#[test]
fn dna_alt_form_imports() {
    let Some(catalog) = catalog() else { return };
    let vexor = catalog.find("Vexor").expect("the Vexor exists").id;
    let hob = catalog.find("Hobgoblin II").expect("the drone exists").id;
    let text = format!("DNA:{vexor}:{hob}*5");
    let fit = one(port::import(&catalog, &text, None).expect("the DNA imports"));
    assert_eq!(fit.drones[0].count, 5);
}

#[test]
fn esi_json_round_trips() {
    let Some(catalog) = catalog() else { return };
    for text in [VEXOR, TENGU] {
        let mut original = fit(&catalog, text);
        original.notes = "line one\nline two".to_owned();
        let json = esi::export(&original, &catalog).expect("the fit exports");
        let again = one(port::import(&catalog, &json, None).expect("the JSON imports"));
        assert_eq!(fitted(&again), fitted(&original));
        assert_eq!(again.notes, original.notes);
        assert_eq!(
            esi::export(&again, &catalog).expect("the fit exports again"),
            json,
            "export is stable"
        );
    }
}

#[test]
fn esi_json_with_named_flags_imports() {
    let Some(catalog) = catalog() else { return };
    let vexor = catalog.find("Vexor").expect("the Vexor exists").id;
    let mwd = catalog
        .find("50MN Microwarpdrive II")
        .expect("the MWD exists")
        .id;
    let hob = catalog.find("Hobgoblin II").expect("the drone exists").id;
    let json = format!(
        r#"{{"name": "Named", "description": "", "ship_type_id": {vexor}, "items": [
            {{"flag": "MedSlot0", "quantity": 1, "type_id": {mwd}}},
            {{"flag": "DroneBay", "quantity": 4, "type_id": {hob}}}
        ]}}"#
    );
    let fit = one(port::import(&catalog, &json, None).expect("the JSON imports"));
    assert_eq!(fit.mid.len(), 1);
    assert_eq!(fit.drones[0].count, 4);
}

#[test]
fn xml_round_trips_several_fits() {
    let Some(catalog) = catalog() else { return };
    let mut vexor = fit(&catalog, VEXOR);
    vexor.name = "Quotes \"&\" <brackets>".to_owned();
    vexor.notes = "first\nsecond".to_owned();
    let tengu = fit(&catalog, TENGU);
    let doc = xml::export(&[&vexor, &tengu], &catalog);
    let imported = port::import(&catalog, &doc, None).expect("the XML imports");
    assert!(imported.unknown.is_empty(), "{:?}", imported.unknown);
    assert_eq!(imported.fits.len(), 2);
    assert_eq!(imported.fits[0].name, vexor.name);
    assert_eq!(imported.fits[0].notes, vexor.notes);
    assert_eq!(fitted(&imported.fits[0]), fitted(&vexor));
    assert_eq!(fitted(&imported.fits[1]), fitted(&tengu));
    let refs: Vec<&Fit> = imported.fits.iter().collect();
    // Loaded charges become cargo on the way through, so the second export is the stable one.
    let again = xml::export(&refs, &catalog);
    let twice = port::import(&catalog, &again, None).expect("the XML imports again");
    let refs: Vec<&Fit> = twice.fits.iter().collect();
    assert_eq!(xml::export(&refs, &catalog), again);
}

#[test]
fn multibuy_lists_the_ship_and_totals() {
    let Some(catalog) = catalog() else { return };
    let vexor = fit(&catalog, VEXOR);
    let list = multibuy::export(&vexor, &catalog, &multibuy::Options::default());
    let lines: Vec<&str> = list.lines().collect();
    assert_eq!(lines[0], "Vexor");
    assert!(lines.contains(&"Heavy Neutron Blaster II x4"), "{list}");
    assert!(lines.contains(&"Hobgoblin II x5"), "{list}");
    // Every line is something the item paste reads back.
    let items = port::parse_items(&catalog, &list).expect("the list reads back");
    assert_eq!(items.len(), lines.len());
}

#[test]
fn html_links_every_fit() {
    let Some(catalog) = catalog() else { return };
    let fits = [fit(&catalog, VEXOR), fit(&catalog, TENGU)];
    let full = html::export(&fits, &catalog, false);
    assert_eq!(full.matches("<details>").count(), 2);
    let minimal = html::export(&fits, &catalog, true);
    assert_eq!(minimal.matches("<a ").count(), 2);
    assert!(minimal.contains("Tengu: Missile Tengu"), "{minimal}");
}

#[test]
fn junk_is_not_a_fit() {
    let Some(catalog) = catalog() else { return };
    assert!(port::import(&catalog, "hello there", None).is_err());
    assert!(port::import(&catalog, "   ", None).is_err());
}
