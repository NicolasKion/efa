//! The Cargo tab: what the fit carries in its hold, against the hold's size.

use std::rc::Rc;

use gpui_kit::component::ActiveTheme;
use gpui_kit::{AnyElement, Context, IntoElement, ParentElement, Styled, div};

use crate::ui::{AddKind, Gyfa, Loaded, widgets::short};

impl Gyfa {
    pub(in crate::ui) fn cargo_tab(
        &mut self,
        loaded: &Rc<Loaded>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let used = self.fit().map_or(0.0, |f| f.cargo_volume(&loaded.catalog));
        let capacity = self
            .stats
            .as_ref()
            .and_then(|s| s.cargo_capacity)
            .unwrap_or(0.0);
        let theme = cx.theme();
        let over = used > capacity + 1e-6;
        let summary = div()
            .px_2()
            .py_1()
            .text_xs()
            .text_color(if over {
                theme.danger
            } else {
                theme.muted_foreground
            })
            .child(format!("{} / {} m³", short(used), short(capacity)))
            .into_any_element();
        self.item_list(loaded, AddKind::Cargo, vec![summary], cx)
    }
}
