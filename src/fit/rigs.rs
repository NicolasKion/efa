//! Rigs switched on and off. A rig cannot be offlined in the game; switching one off here shows
//! the fit without its bonus while it still takes its calibration.

use super::Fit;

impl Fit {
    /// Whether the rig at position `i` applies its bonuses. Empty positions count as on.
    pub fn rig_active(&self, i: usize) -> bool {
        self.rigs
            .get(i)
            .copied()
            .flatten()
            .is_none_or(|t| !self.inactive_rigs.contains(&(i, t)))
    }

    /// Switch the rig at position `i` on or off. Entries for rigs no longer where they were go
    /// at the same time, so a rig put in later starts out on.
    pub fn set_rig_active(&mut self, i: usize, active: bool) {
        let rigs = &self.rigs;
        self.inactive_rigs
            .retain(|&(j, t)| j != i && rigs.get(j) == Some(&Some(t)));
        if !active && let Some(Some(t)) = self.rigs.get(i).copied() {
            self.inactive_rigs.push((i, t));
            self.inactive_rigs.sort_unstable();
        }
    }

    /// The state click on a rig: `clicked` flips, and the other `rows` follow it.
    pub fn toggle_rigs(&mut self, clicked: usize, rows: &[usize]) {
        let on = !self.rig_active(clicked);
        for &i in rows.iter().chain(std::iter::once(&clicked)) {
            self.set_rig_active(i, on);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::fit::Fit;

    #[test]
    fn a_switched_off_rig_stays_off_only_while_it_is_there() {
        let mut fit = Fit {
            rigs: vec![Some(1), Some(2)],
            ..Fit::default()
        };
        fit.toggle_rigs(0, &[]);
        assert!(!fit.rig_active(0));
        assert!(fit.rig_active(1));
        // Another rig in its place starts out on.
        fit.rigs[0] = Some(3);
        assert!(fit.rig_active(0));
        fit.toggle_rigs(1, &[0]);
        assert!(!fit.rig_active(0) && !fit.rig_active(1));
        fit.toggle_rigs(0, &[1]);
        assert!(fit.rig_active(0) && fit.rig_active(1));
        assert_eq!(fit.inactive_rigs, []);
    }
}
