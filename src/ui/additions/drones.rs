//! The Drones tab: the fit's drone stacks.

use std::rc::Rc;

use gpui_kit::{AnyElement, Context};

use crate::ui::{AddKind, Gyfa, Loaded};

impl Gyfa {
    pub(in crate::ui) fn drones_tab(
        &mut self,
        loaded: &Rc<Loaded>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.item_list(loaded, AddKind::Drones, Vec::new(), cx)
    }
}
