//! The graph window's options, remembered between runs like pyfa's graph settings.

use serde::{Deserialize, Serialize};

use super::{
    ammo::{AmmoQuality, AmmoStyle},
    defs::Graph,
    plot::{DroneMode, Options},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent toggles, each a menu checkbox in pyfa"
)]
pub struct GraphSettings {
    pub ignore_resists: bool,
    pub apply_projected: bool,
    pub ignore_lock_range: bool,
    pub ignore_dcr: bool,
    pub drone_mode: DroneMode,
    /// The Application Profile graph keeps its own two, as in pyfa.
    pub ammo_ignore_resists: bool,
    pub ammo_apply_projected: bool,
    pub ammo_style: AmmoStyle,
    pub ammo_quality: AmmoQuality,
    pub show_legend: bool,
    pub show_y0: bool,
}

impl Default for GraphSettings {
    /// pyfa's defaults (service/settings.py `GraphSettings`).
    fn default() -> Self {
        Self {
            ignore_resists: true,
            apply_projected: true,
            ignore_lock_range: true,
            ignore_dcr: false,
            drone_mode: DroneMode::Auto,
            ammo_ignore_resists: true,
            ammo_apply_projected: true,
            ammo_style: AmmoStyle::Color,
            ammo_quality: AmmoQuality::Navy,
            show_legend: true,
            show_y0: true,
        }
    }
}

impl GraphSettings {
    /// The options a graph's lines are computed with. Shield graphs count EHP as the stats pane
    /// shows it.
    pub fn options(&self, graph: Graph, effective_hp: bool) -> Options {
        let ammo = graph == Graph::ApplicationProfile;
        Options {
            ignore_resists: if ammo {
                self.ammo_ignore_resists
            } else {
                self.ignore_resists
            },
            apply_projected: if ammo {
                self.ammo_apply_projected
            } else {
                self.apply_projected
            },
            ignore_lock_range: self.ignore_lock_range,
            ignore_dcr: self.ignore_dcr,
            drone_mode: self.drone_mode,
            effective_hp,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_saves_get_pyfa_defaults() {
        let settings: GraphSettings =
            serde_json::from_str(r#"{"ignore_dcr": true}"#).expect("partial settings parse");
        assert!(settings.ignore_dcr);
        assert!(settings.apply_projected);
        let options = settings.options(Graph::ApplicationProfile, false);
        assert!(options.ignore_resists && !options.effective_hp);
    }
}
