//! The Boosters tab: the fit's boosters.

use std::rc::Rc;

use gpui_kit::{AnyElement, Context};

use crate::ui::{AddKind, Gyfa, Loaded};

impl Gyfa {
    pub(in crate::ui) fn boosters_tab(
        &mut self,
        loaded: &Rc<Loaded>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.item_list(loaded, AddKind::Boosters, Vec::new(), cx)
    }
}
