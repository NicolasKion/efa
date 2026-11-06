//! EFT import and export against the real data. Skipped when no dump is present.

use std::path::Path;

use gyfa::{data::Catalog, fit::Fit};

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

fn catalog() -> Option<Catalog> {
    if !Path::new(DATA).join("phobos/metadata.json").exists() {
        eprintln!("no EVE data in {DATA}, skipping");
        return None;
    }
    Some(Catalog::load_cached(Path::new(DATA), Path::new("data/cache")).expect("the dump loads"))
}

#[test]
fn eft_round_trips() {
    let Some(catalog) = catalog() else { return };
    let (fit, unknown) = Fit::from_eft(&catalog, VEXOR).expect("the fit parses");
    assert!(unknown.is_empty(), "unknown items: {unknown:?}");
    assert_eq!(
        (fit.high.len(), fit.mid.len(), fit.low.len(), fit.rigs.len()),
        (4, 3, 5, 3)
    );
    assert_eq!(fit.drones.len(), 2);
    assert_eq!(fit.cargo.len(), 1);
    // Only five drones fit in space, and they come from the first stack listed.
    assert_eq!(fit.launched(), 5);
    assert_eq!(fit.drones[0].active, 5);

    let exported = fit.to_eft(&catalog);
    let (again, unknown) = Fit::from_eft(&catalog, &exported).expect("the export parses");
    assert!(
        unknown.is_empty(),
        "unknown items after export: {unknown:?}"
    );
    assert_eq!(again.to_eft(&catalog), exported, "export is stable");
    assert_eq!(exported.trim(), VEXOR.trim(), "export matches the input");
}
