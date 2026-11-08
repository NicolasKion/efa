//! EFT import and export against the real data. Skipped when no dump is present.

use crate::common::catalog;
use efa::{fit::Fit, port::eft};

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

#[test]
fn eft_round_trips() {
    let Some(catalog) = catalog() else { return };
    let (fit, unknown) = Fit::from_eft(catalog, VEXOR).expect("the fit parses");
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

    let export = |fit: &Fit| eft::export(fit, catalog, &eft::Options::default(), None);
    let exported = export(&fit);
    let (again, unknown) = Fit::from_eft(catalog, &exported).expect("the export parses");
    assert!(
        unknown.is_empty(),
        "unknown items after export: {unknown:?}"
    );
    assert_eq!(export(&again), exported, "export is stable");
    // The same lines in the same order; pyfa's layout spaces sections further apart and writes
    // "/offline" in lower case.
    let lines = |text: &str| -> Vec<String> {
        text.lines()
            .filter(|l| !l.trim().is_empty())
            .map(str::to_lowercase)
            .collect()
    };
    assert_eq!(lines(&exported), lines(VEXOR), "export matches the input");
}
