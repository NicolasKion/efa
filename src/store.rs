//! Saved fits and which of them are open, and the settings kept with them in [`crate::db`].

use std::rc::Rc;

use serde::{Deserialize, Serialize};

use crate::{
    damage_pattern::DamagePattern, fit::Fit, graphs::settings::GraphSettings,
    implant_set::ImplantSet, port::ExportSettings, preferences::Preferences,
    target_profile::TargetProfile,
};

/// The user's fits and lists, and the settings kept with them.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Saved {
    /// Shared with every window's copy: an edit copies only the fit it changes, through
    /// [`Saved::fit_mut`], so passing the fits on costs a pointer each.
    pub fits: Vec<Rc<Fit>>,
    /// The user's implant sets, by name.
    pub implant_sets: Vec<ImplantSet>,
    /// The user's damage patterns, pyfa's Damage Pattern Editor.
    pub damage_patterns: Vec<DamagePattern>,
    /// The user's target profiles, pyfa's Target Profile Editor.
    pub target_profiles: Vec<TargetProfile>,
    /// The user's cargo presets, by name.
    pub cargo_presets: Vec<crate::cargo_preset::CargoPreset>,
    /// Everything that is not a list. Stored as settings rather than tables, so writing them
    /// never looks at the fits.
    #[serde(flatten)]
    pub settings: Settings,
}

/// Window state and preferences: the settings part of [`Saved`].
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Fits open as tabs, by index into `fits`.
    pub open: Vec<usize>,
    pub current: Option<usize>,
    /// Fitting view column widths the user set by dragging, by column.
    pub columns: std::collections::BTreeMap<String, f32>,
    /// Count reload time into DPS and the capacitor simulation, pyfa's global "Factor in Reload Time".
    pub factor_reload: bool,
    /// Which groups the price total counts, pyfa's price view menu.
    pub price_totals: PriceTotals,
    /// The preferences window's settings.
    #[serde(default)]
    pub preferences: Preferences,
    /// The copy dialog's last format and options.
    #[serde(default)]
    pub export: ExportSettings,
    /// The graph window's options.
    #[serde(default)]
    pub graphs: GraphSettings,
    /// Items last added from the market, newest first: pyfa's Recently Used Items.
    #[serde(default)]
    pub recent_items: Vec<crate::data::TypeId>,
    /// Height of the market results pane, in pixels, once the user dragged its divider. It
    /// keeps that height when the window resizes; the tree above takes the change.
    #[serde(default)]
    pub market_results: Option<f32>,
    /// The fitting wizard's last bling choices, which the next fit starts from.
    #[serde(default)]
    pub wizard: crate::wizard::Bling,
    /// The fitting wizard's last tab, which it opens on next time.
    #[serde(default)]
    pub wizard_tab: crate::wizard::Tab,
    /// pyfa's attribute overrides and whether they are on.
    #[serde(default)]
    pub overrides: crate::overrides::Overrides,
}

/// What besides ship and fittings the price total includes, with pyfa's defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PriceTotals {
    pub drones: bool,
    pub cargo: bool,
    /// Implants and boosters.
    pub character: bool,
}

impl Default for PriceTotals {
    fn default() -> Self {
        Self {
            drones: true,
            cargo: true,
            character: false,
        }
    }
}

impl Saved {
    /// Fit `index` to change, copied first if another window's copy shares it.
    pub fn fit_mut(&mut self, index: usize) -> &mut Fit {
        Rc::make_mut(&mut self.fits[index])
    }

    /// Add a fit at the end.
    pub fn push_fit(&mut self, fit: Fit) {
        self.fits.push(Rc::new(fit));
    }

    /// Drop tabs of fits that are gone and split shared identities, after loading.
    pub fn tidy(&mut self) {
        let count = self.fits.len();
        let settings = &mut self.settings;
        settings.open.retain(|&i| i < count);
        settings.current = settings.current.filter(|c| settings.open.contains(c));
        self.dedupe_uids();
    }

    /// The identity of a fit, handing out a fresh one when it has none yet or shares it with an
    /// earlier fit, which a duplicated fit does.
    pub fn ensure_uid(&mut self, index: usize) -> Option<u64> {
        let uid = self.fits.get(index)?.uid;
        let taken = self.fits[..index].iter().any(|f| f.uid == uid);
        if uid != 0 && !taken {
            return Some(uid);
        }
        let fresh = self.highest_uid() + 1;
        self.fit_mut(index).uid = fresh;
        Some(fresh)
    }

    /// The highest identity in use, counting references to deleted fits, so a new fit never
    /// picks up links meant for one that is gone.
    fn highest_uid(&self) -> u64 {
        self.fits
            .iter()
            .flat_map(|f| {
                let projected = f.projected.fits.iter().map(|p| p.fit);
                let command = f.command.iter().map(|c| c.fit);
                std::iter::once(f.uid).chain(projected).chain(command)
            })
            .max()
            .unwrap_or(0)
    }

    /// The index of the fit with this identity.
    pub fn fit_index(&self, uid: u64) -> Option<usize> {
        (uid != 0)
            .then(|| self.fits.iter().position(|f| f.uid == uid))
            .flatten()
    }

    /// Drop the fit's links to fits that were deleted. Returns whether any went.
    pub fn tidy_links(&mut self, index: usize) -> bool {
        let uids: std::collections::HashSet<u64> = self.fits.iter().map(|f| f.uid).collect();
        let Some(fit) = self.fits.get(index) else {
            return false;
        };
        let dangling = fit
            .projected
            .fits
            .iter()
            .any(|p| p.fit == 0 || !uids.contains(&p.fit))
            || fit
                .command
                .iter()
                .any(|c| c.fit == 0 || !uids.contains(&c.fit));
        if !dangling {
            return false;
        }
        let fit = self.fit_mut(index);
        fit.projected
            .retain_fits(|uid| uid != 0 && uids.contains(&uid));
        fit.command.retain(|c| c.fit != 0 && uids.contains(&c.fit));
        true
    }

    /// Give fits that share an identity their own, keeping it with the first of them, so a
    /// reference made before a fit was duplicated keeps pointing at the original.
    fn dedupe_uids(&mut self) {
        let mut seen = std::collections::HashSet::new();
        let mut next = self.highest_uid();
        for fit in &mut self.fits {
            if fit.uid != 0 && !seen.insert(fit.uid) {
                next += 1;
                Rc::make_mut(fit).uid = next;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uids_are_handed_out_once_and_kept() {
        let mut saved = Saved {
            fits: vec![Rc::new(Fit::new("a")), Rc::new(Fit::new("b"))],
            ..Saved::default()
        };
        let a = saved.ensure_uid(0).expect("fit exists");
        let b = saved.ensure_uid(1).expect("fit exists");
        assert_ne!(a, b);
        assert_eq!(saved.ensure_uid(0), Some(a));
        assert_eq!(saved.fit_index(b), Some(1));
        assert_eq!(saved.fit_index(0), None);

        // A copy of a fit gets its own identity; the original keeps its.
        saved.push_fit((*saved.fits[0]).clone());
        let copy = saved.ensure_uid(2).expect("fit exists");
        assert_ne!(copy, a);
        assert_eq!(saved.fit_index(a), Some(0));
    }

    #[test]
    fn identities_of_deleted_fits_are_not_handed_out_again() {
        let mut saved = Saved {
            fits: vec![Rc::new(Fit::new("a")), Rc::new(Fit::new("b"))],
            ..Saved::default()
        };
        let b = saved.ensure_uid(1).expect("fit exists");
        saved.fit_mut(0).projected.add_fit(b);
        saved.fits.remove(1);
        saved.push_fit(Fit::new("c"));
        let c = saved.ensure_uid(1).expect("fit exists");
        assert_ne!(c, b, "the new fit would inherit the old projection");
    }

    #[test]
    fn duplicate_uids_are_split_on_load() {
        let mut saved = Saved {
            fits: vec![Rc::new(Fit::new("a")), Rc::new(Fit::new("b"))],
            ..Saved::default()
        };
        saved.fit_mut(0).uid = 4;
        saved.fit_mut(1).uid = 4;
        saved.dedupe_uids();
        assert_eq!(saved.fits[0].uid, 4);
        assert_eq!(saved.fits[1].uid, 5);
    }
}
