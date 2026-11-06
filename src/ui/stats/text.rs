//! The fit's stats as text, pyfa's "Stats" copy format (service/port/shipstats.py).

use std::fmt::Write;

use super::format::amount;
use crate::{
    engine::{Cap, Layer, Stats},
    fit::Fit,
};

const DAMAGE_NAMES: [&str; 4] = ["Em", "Th", "Kin", "Exp"];

/// `ship` is the hull's name, which pyfa puts after the fit's.
pub fn fit_stats(fit: &Fit, stats: &Stats, ship: &str) -> String {
    let sections: Vec<String> = [firepower(stats), tank(stats), reps(stats), misc(stats)]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    format!("{} ({ship})\n\n{}", fit.name, sections.join("\n"))
}

fn firepower(stats: &Stats) -> String {
    let total = stats.dps.total();
    let volley = stats.volley.total();
    if total + volley + stats.weapon_dps + stats.drone_dps == 0.0 {
        return String::new();
    }
    let f = |v: f64| amount(v, 3, 0, 0);
    format!(
        "DPS: {} (Weapon: {}, Drone: {}, Volley: {})\n",
        f(total),
        f(stats.weapon_dps),
        f(stats.drone_dps),
        f(volley)
    )
}

fn layers(stats: &Stats) -> [&Layer; 3] {
    [&stats.shield, &stats.armor, &stats.hull]
}

fn tank(stats: &Stats) -> String {
    let f = |v: f64| amount(v, 3, 0, 9);
    let ehp: f64 = layers(stats).iter().map(|l| l.ehp.unwrap_or(l.hp)).sum();
    // EHP against each damage type alone, as pyfa lists after the total.
    let per_type: Vec<String> = (0..4)
        .map(|d| {
            let total: f64 = layers(stats)
                .iter()
                .map(|l| {
                    let through = 1.0 - l.resists[d];
                    if through > 0.0 { l.hp / through } else { 0.0 }
                })
                .sum();
            format!("{}: {}", DAMAGE_NAMES[d], f(total))
        })
        .collect();
    let mut out = format!("EHP: {} ({})\n", f(ehp), per_type.join(", "));
    for (name, layer) in ["Shield", "Armor", "Hull"].iter().zip(layers(stats)) {
        let resists: Vec<String> = (0..4)
            .map(|d| format!("{}: {:.0}%", DAMAGE_NAMES[d], layer.resists[d] * 100.0))
            .collect();
        let _ = writeln!(
            out,
            "{name}: {} ({})",
            f(layer.ehp.unwrap_or(layer.hp)),
            resists.join(", ")
        );
    }
    out
}

fn reps(stats: &Stats) -> String {
    let f = |v: f64| amount(v, 3, 0, 9);
    let mult = |l: &Layer| l.mult.unwrap_or(1.0);
    let [shield, armor, hull] = layers(stats);
    let local = [
        stats.tank.shield * mult(shield),
        stats.tank.armor * mult(armor),
        stats.tank.hull * mult(hull),
    ];
    let sustained = [
        stats.sustained.shield * mult(shield),
        stats.sustained.armor * mult(armor),
        stats.sustained.hull * mult(hull),
    ];
    let regen = stats.tank.passive_shield * mult(shield);
    let mut out = String::new();
    for (name, (rep, sust)) in ["Shield", "Armor", "Hull"]
        .iter()
        .zip(local.iter().zip(sustained))
    {
        if *rep > 0.0 {
            let _ = write!(
                out,
                "Self {} repair: {} EHP/s",
                name.to_lowercase(),
                f(*rep)
            );
            if (sust - rep).abs() > 1e-9 {
                let _ = write!(out, " (Sustained: {} EHP/s)", f(sust));
            }
            out.push('\n');
        }
    }
    if out.is_empty() && regen > 0.0 {
        let _ = writeln!(out, "Shield regeneration: {} EHP/s", f(regen));
    }
    let remote = stats.outgoing.reps;
    if remote.iter().any(|&r| r > 0.0) {
        for (name, rep) in ["Shield", "Armor", "Hull"].iter().zip(remote) {
            if rep > 0.0 {
                let _ = writeln!(
                    out,
                    "Remote {} repair: {} HP/s",
                    name.to_lowercase(),
                    f(rep)
                );
            }
        }
    }
    out
}

fn misc(stats: &Stats) -> String {
    let mut out = String::new();
    if let Some(speed) = stats.speed {
        let _ = writeln!(out, "Speed: {} m/s", amount(speed, 3, 0, 0));
    }
    if let Some(sig) = stats.sig_radius {
        let _ = writeln!(out, "Signature: {} m", amount(sig, 3, 0, 9));
    }
    if let Some(cap) = stats.cap_amount {
        let _ = write!(out, "Capacitor: {} GJ", amount(cap, 3, 0, 9));
        match stats.cap {
            Some(Cap::Stable(level)) => {
                let _ = write!(out, " (Stable at {:.0}%)", level * 100.0);
            }
            Some(Cap::Lasts(secs)) => {
                let secs = secs as u64;
                let lasts = if secs <= 60 {
                    format!("{secs}s")
                } else {
                    format!("{}m{}s", secs / 60, secs % 60)
                };
                let _ = write!(out, " (Lasts {lasts})");
            }
            None => {}
        }
        out.push('\n');
    }
    if let Some(range) = stats.lock_range {
        let _ = writeln!(
            out,
            "Targeting range: {} km",
            amount(range / 1000.0, 3, 0, 0)
        );
    }
    if let Some(res) = stats.scan_res {
        let _ = writeln!(out, "Scan resolution: {res:.0} mm");
    }
    if let Some(strength) = stats.sensor_strength {
        let _ = writeln!(out, "Sensor strength: {}", amount(strength, 3, 0, 0));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_stats_still_name_the_fit() {
        let fit = Fit::new("Nothing");
        let text = fit_stats(&fit, &Stats::default(), "");
        assert!(text.starts_with("Nothing ()"), "{text}");
        assert!(!text.contains("DPS"), "no firepower section without damage");
    }
}
