//! Drone stacks and what a fit does with them: adding, launching, splitting off, merging and
//! copying stacks.

use serde::{Deserialize, Serialize};

use super::{Fit, MAX_DRONES, MAX_LAUNCHED, Mutation, mutate, revert};
use crate::data::{Catalog, TypeId};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DroneStack {
    pub type_id: TypeId,
    pub count: u32,
    pub active: u32,
    /// What a mutaplasmid made of these drones; `type_id` is then the mutated drone type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mutation: Option<Mutation>,
}

impl DroneStack {
    /// A stack of `count` drones, at most [`MAX_DRONES`], `active` of them launched.
    pub fn new(type_id: TypeId, count: u32, active: u32) -> Self {
        let count = count.min(MAX_DRONES);
        Self {
            type_id,
            count,
            active: active.min(count),
            mutation: None,
        }
    }

    /// The drone before any mutation.
    pub fn base_type(&self) -> TypeId {
        self.mutation.as_ref().map_or(self.type_id, |m| m.base)
    }

    pub fn mutate(&mut self, catalog: &Catalog, mutaplasmid: TypeId) -> bool {
        mutate(catalog, &mut self.type_id, &mut self.mutation, mutaplasmid)
    }

    pub fn revert(&mut self) -> bool {
        revert(&mut self.type_id, &mut self.mutation)
    }
}

impl Fit {
    /// Add a stack of its own, launching as many of it as there is room for in space.
    pub fn add_drone_stack(&mut self, mut stack: DroneStack) {
        stack.active = MAX_LAUNCHED
            .saturating_sub(self.launched())
            .min(stack.count);
        self.drones.push(stack);
    }

    /// Add `count` drones of a type, launching as many as there is room for in space.
    pub fn add_drones(&mut self, type_id: TypeId, count: u32) {
        let launch = MAX_LAUNCHED.saturating_sub(self.launched()).min(count);
        match self.drones.iter_mut().find(|d| d.type_id == type_id) {
            Some(stack) => {
                stack.count = stack.count.saturating_add(count).min(MAX_DRONES);
                stack.active = stack.active.saturating_add(launch).min(stack.count);
            }
            None => self.drones.push(DroneStack::new(type_id, count, launch)),
        }
    }

    pub fn launched(&self) -> u32 {
        self.drones.iter().map(|d| d.active).sum()
    }

    /// pyfa's drone state toggle: if the clicked stack has drones out, recall every stack in
    /// `stacks`, otherwise launch all of them in full. Stacks are all-or-nothing, so a partly
    /// launched stack counts as launched.
    pub fn toggle_drones(&mut self, clicked: usize, stacks: &[usize]) {
        let Some(launch) = self.drones.get(clicked).map(|d| d.active == 0) else {
            return;
        };
        for &i in stacks.iter().chain([&clicked]) {
            if let Some(stack) = self.drones.get_mut(i) {
                stack.active = if launch { stack.count } else { 0 };
            }
        }
    }

    /// Take one drone out of a stack, dropping the stack once it is empty.
    pub fn remove_drone(&mut self, i: usize) {
        let Some(stack) = self.drones.get_mut(i) else {
            return;
        };
        stack.count = stack.count.saturating_sub(1);
        stack.active = stack.active.min(stack.count);
        if stack.count == 0 {
            self.drones.remove(i);
        }
    }

    /// Fold stack `from` into stack `to`. Only stacks of the same drone merge; `to` keeps its
    /// launched count. Returns whether anything changed.
    pub fn merge_drones(&mut self, from: usize, to: usize) -> bool {
        let (Some(src), Some(dst)) = (self.drones.get(from), self.drones.get(to)) else {
            return false;
        };
        // Mutated drones only merge with drones rolled the same.
        if from == to || src.type_id != dst.type_id || src.mutation != dst.mutation {
            return false;
        }
        let moved = src.count;
        let stack = &mut self.drones[to];
        stack.count = stack.count.saturating_add(moved).min(MAX_DRONES);
        self.drones.remove(from);
        true
    }

    /// Add a copy of a stack as a stack of its own, launched state included.
    pub fn clone_drones(&mut self, i: usize) {
        if let Some(stack) = self.drones.get(i).cloned() {
            self.drones.push(stack);
        }
    }
}

#[cfg(test)]
mod drone_stack_tests {
    use super::*;

    fn stack(type_id: TypeId, count: u32, active: u32) -> DroneStack {
        DroneStack::new(type_id, count, active)
    }

    fn fit_with(drones: Vec<DroneStack>) -> Fit {
        Fit {
            drones,
            ..Fit::default()
        }
    }

    fn counts(fit: &Fit) -> Vec<(TypeId, u32, u32)> {
        fit.drones
            .iter()
            .map(|d| (d.type_id, d.count, d.active))
            .collect()
    }

    #[test]
    fn toggle_follows_the_clicked_stack() {
        let mut fit = fit_with(vec![stack(1, 5, 2), stack(2, 3, 0)]);
        // The clicked stack is out, so everything targeted is recalled.
        fit.toggle_drones(0, &[0, 1]);
        assert_eq!(counts(&fit), [(1, 5, 0), (2, 3, 0)]);
        // Now it is in, so everything targeted launches in full.
        fit.toggle_drones(1, &[0, 1]);
        assert_eq!(counts(&fit), [(1, 5, 5), (2, 3, 3)]);
    }

    #[test]
    fn toggle_includes_the_clicked_stack_outside_the_selection() {
        let mut fit = fit_with(vec![stack(1, 2, 0), stack(2, 2, 0)]);
        fit.toggle_drones(1, &[0]);
        assert_eq!(counts(&fit), [(1, 2, 2), (2, 2, 2)]);
    }

    #[test]
    fn removing_one_drone_shrinks_then_drops_the_stack() {
        let mut fit = fit_with(vec![stack(1, 2, 2), stack(2, 1, 0)]);
        fit.remove_drone(0);
        assert_eq!(counts(&fit), [(1, 1, 1), (2, 1, 0)]);
        fit.remove_drone(1);
        assert_eq!(counts(&fit), [(1, 1, 1)]);
        fit.remove_drone(7);
        assert_eq!(counts(&fit), [(1, 1, 1)]);
    }

    #[test]
    fn merge_only_joins_the_same_drone() {
        let mut fit = fit_with(vec![stack(1, 2, 2), stack(2, 3, 0), stack(1, 4, 0)]);
        assert!(!fit.merge_drones(0, 1), "different drones");
        assert!(!fit.merge_drones(0, 0), "same stack");
        assert!(fit.merge_drones(2, 0));
        assert_eq!(counts(&fit), [(1, 6, 2), (2, 3, 0)]);
    }

    #[test]
    fn merge_into_a_later_stack_keeps_indices_straight() {
        let mut fit = fit_with(vec![stack(1, 2, 0), stack(2, 1, 0), stack(1, 3, 1)]);
        assert!(fit.merge_drones(0, 2));
        assert_eq!(counts(&fit), [(2, 1, 0), (1, 5, 1)]);
    }

    #[test]
    fn clone_adds_a_separate_stack() {
        let mut fit = fit_with(vec![stack(1, 3, 1)]);
        fit.clone_drones(0);
        assert_eq!(counts(&fit), [(1, 3, 1), (1, 3, 1)]);
    }
}
