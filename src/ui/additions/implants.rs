//! The Implants tab: the fit's implants.

use std::rc::Rc;

use gpui_kit::{AnyElement, Context};

use crate::ui::{AddKind, Gyfa, Loaded};

impl Gyfa {
    pub(in crate::ui) fn implants_tab(
        &mut self,
        loaded: &Rc<Loaded>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.item_list(loaded, AddKind::Implants, Vec::new(), cx)
    }
}
