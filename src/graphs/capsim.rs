//! pyfa's capacitor simulator (eos/capSim.py), for the capacitor graph's curve over time.
//!
//! refine has its own simulator, but it only reports how long the capacitor lasts or where it
//! settles; the graph needs the level after every activation, so this keeps pyfa's event loop.
//! It runs with pyfa's graph settings: staggered activations, reloads always counted, no
//! repeat shortcut.

use std::{cmp::Ordering, collections::BinaryHeap};

use super::snapshot::CapUser;

/// One activation waiting in the queue.
#[derive(Debug, Clone, PartialEq)]
struct Activation {
    at: f64,
    duration: f64,
    need: f64,
    shot: u32,
    clip: u32,
    reload: f64,
    injector: bool,
}

impl Eq for Activation {}

impl Ord for Activation {
    // Earliest first, out of a max-heap; ties go to the larger need like Python's heap of lists.
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .at
            .total_cmp(&self.at)
            .then_with(|| other.duration.total_cmp(&self.duration))
            .then_with(|| other.need.total_cmp(&self.need))
    }
}

impl PartialOrd for Activation {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Activation {
    /// Move to the next activation, through a reload when the clip runs out.
    fn advance(&mut self) {
        self.at += self.duration;
        self.shot += 1;
        if self.clip > 0 && self.shot.is_multiple_of(self.clip) {
            self.shot = 0;
            self.at += self.reload;
        }
    }
}

/// Capacitor level after every change, as (seconds, GJ), from `start` GJ until `t_max` seconds
/// or until the capacitor runs dry.
pub fn simulate(
    users: &[CapUser],
    capacity: f64,
    recharge_s: f64,
    start: f64,
    t_max: f64,
) -> Vec<(f64, f64)> {
    if users.is_empty() || capacity <= 0.0 {
        return Vec::new();
    }
    let tau = recharge_s * 1000.0 / 5.0;
    let t_max = t_max * 1000.0;
    let mut state = BinaryHeap::new();

    // Identical modules are grouped, then staggered across their cycle.
    let mut groups: Vec<(&CapUser, u32)> = Vec::new();
    for user in users {
        match groups.iter_mut().find(|(u, _)| *u == user) {
            Some((_, count)) => *count += 1,
            None => groups.push((user, 1)),
        }
    }
    for (user, amount) in groups {
        let mut activation = Activation {
            at: 0.0,
            duration: user.cycle_ms.trunc(),
            need: user.need,
            shot: 0,
            clip: user.clip,
            reload: user.reload_ms,
            injector: user.injector,
        };
        if user.injector {
            for _ in 0..amount {
                state.push(activation.clone());
            }
            continue;
        }
        if user.no_stagger {
            activation.need *= f64::from(amount);
        } else if user.clip == 0 {
            activation.duration = (activation.duration / f64::from(amount)).trunc();
        } else {
            let gap = (activation.duration * f64::from(user.clip) + activation.reload)
                / f64::from(amount * user.clip);
            for i in 1..amount {
                state.push(Activation {
                    at: f64::from(i) * gap,
                    ..activation.clone()
                });
            }
        }
        state.push(activation);
    }

    let mut changes: Vec<(f64, f64)> = Vec::new();
    // Several changes at one instant keep the last level, as pyfa's dict keyed by time does.
    let mut record = |t: f64, cap: f64| match changes.last_mut() {
        Some(last) if last.0 >= t => last.1 = cap,
        _ => changes.push((t, cap)),
    };
    let mut waiting: Vec<Activation> = Vec::new();
    let (mut cap, mut t_last) = (start, 0.0);
    while let Some(mut activation) = state.pop() {
        let now = activation.at;
        if now >= t_max {
            break;
        }
        if now > t_last {
            cap = (1.0 + ((cap / capacity).sqrt() - 1.0) * ((t_last - now) / tau).exp()).powi(2)
                * capacity;
        }
        t_last = now;

        // An injector that would overflow waits until the capacitor has room.
        if activation.injector && cap - activation.need > capacity {
            waiting.push(activation);
            continue;
        }
        if activation.need > cap && cap < capacity {
            while activation.need > cap && cap < capacity {
                let wanted = (activation.need - cap).min(capacity - cap);
                let Some(best) = pick_injector(&waiting, |inj| -inj.need >= wanted, true)
                    .or_else(|| pick_injector(&waiting, |_| true, false))
                else {
                    break;
                };
                let mut injector = waiting.swap_remove(best);
                cap = (cap - injector.need).min(capacity);
                record(now / 1000.0, cap);
                injector.at = now;
                injector.advance();
                state.push(injector);
            }
        }
        cap = (cap - activation.need).min(capacity);
        record(now / 1000.0, cap);
        if cap < 0.0 {
            break;
        }
        while cap < capacity {
            let room = capacity - cap;
            let Some(best) = pick_injector(&waiting, |inj| -inj.need <= room, false) else {
                break;
            };
            let mut injector = waiting.swap_remove(best);
            cap = (cap - injector.need).min(capacity);
            record(now / 1000.0, cap);
            injector.at = now;
            injector.advance();
            state.push(injector);
        }
        activation.advance();
        state.push(activation);
    }
    changes.into_iter().map(|(t, c)| (t, c.max(0.0))).collect()
}

/// The waiting injector passing `fits`, the smallest when `smallest`, else the largest.
fn pick_injector(
    waiting: &[Activation],
    fits: impl Fn(&Activation) -> bool,
    smallest: bool,
) -> Option<usize> {
    let candidates = waiting.iter().enumerate().filter(|(_, a)| fits(a));
    let by_size =
        |a: &(usize, &Activation), b: &(usize, &Activation)| (-a.1.need).total_cmp(&-b.1.need);
    if smallest {
        candidates.min_by(by_size)
    } else {
        candidates.max_by(by_size)
    }
    .map(|(i, _)| i)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user(cycle_ms: f64, need: f64) -> CapUser {
        CapUser {
            cycle_ms,
            need,
            clip: 0,
            reload_ms: 0.0,
            no_stagger: false,
            injector: false,
        }
    }

    #[test]
    fn single_drain_between_regens() {
        // 1000 GJ, 100 s recharge (tau 20 s), 100 GJ every 10 s from full.
        let changes = simulate(&[user(10_000.0, 100.0)], 1000.0, 100.0, 1000.0, 25.0);
        assert_eq!(changes[0], (0.0, 900.0));
        let regen = (1.0 + ((0.9_f64).sqrt() - 1.0) * (-0.5_f64).exp()).powi(2) * 1000.0;
        assert!(
            (changes[1].1 - (regen - 100.0)).abs() < 1e-9,
            "{:?}",
            changes[1]
        );
        assert_eq!(changes.len(), 3);
    }

    #[test]
    fn two_identical_modules_stagger() {
        let changes = simulate(
            &[user(10_000.0, 100.0), user(10_000.0, 100.0)],
            1000.0,
            100.0,
            1000.0,
            12.0,
        );
        let times: Vec<f64> = changes.iter().map(|c| c.0).collect();
        assert_eq!(times, vec![0.0, 5.0, 10.0]);
    }

    #[test]
    fn stops_when_dry() {
        let changes = simulate(&[user(1000.0, 400.0)], 1000.0, 1000.0, 1000.0, 3600.0);
        assert!(changes.len() < 5, "{changes:?}");
        assert_eq!(changes.last().map(|c| c.1), Some(0.0));
    }
}
