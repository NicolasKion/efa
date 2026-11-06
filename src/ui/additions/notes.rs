//! The Notes tab: free text kept with the fit.

use std::rc::Rc;

use gpui_kit::{AnyElement, Context, IntoElement, ParentElement, Styled, div};

use crate::ui::{Gyfa, Loaded};

impl Gyfa {
    pub(in crate::ui) fn notes_tab(
        &mut self,
        _loaded: &Rc<Loaded>,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .p_2()
            .size_full()
            .child(self.notes_field())
            .into_any_element()
    }
}
