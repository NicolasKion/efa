//! The "{Item} Stats" entries of the main window's menus.

use gpui_kit::component::{IconName, menu::PopupMenuItem};
use gpui_kit::{ClickEvent, Context, WeakEntity, Window};

use super::{Request, Target, open};
use crate::{data::TypeId, engine::FitItem, ui::Gyfa, ui::widgets::entry};

/// What a menu entry shows the stats of.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatsOf {
    /// A type by itself: market items, cargo, skills.
    Type(TypeId),
    /// An item of the window's current fit.
    Fitted(FitItem),
    /// An item of another saved fit, by its index: a projected fit's ship.
    OtherFit(usize, FitItem),
}

impl Gyfa {
    /// Open a stats window; Shift replaces the last one.
    pub(in crate::ui) fn show_stats(
        &mut self,
        of: StatsOf,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(loaded) = self.loaded() else { return };
        let request = match of {
            StatsOf::Type(id) => Request {
                target: Target::Type(id),
                fit: None,
            },
            StatsOf::Fitted(item) => {
                let Some(fit) = self.saved.current else {
                    return;
                };
                // Handed over rather than read later: the new window is built while this one
                // is being updated.
                Request {
                    target: Target::Fitted { fit, item },
                    fit: self.fit().cloned(),
                }
            }
            StatsOf::OtherFit(fit, item) => {
                let Some(other) = self.saved.fits.get(fit) else {
                    return;
                };
                Request {
                    target: Target::Fitted { fit, item },
                    fit: Some(other.clone()),
                }
            }
        };
        let hub = self.hub.clone();
        open(&loaded, &hub, request, event.modifiers().shift, window, cx);
    }
}

/// pyfa's "{label} Stats" entry.
pub fn stats_entry(this: &WeakEntity<Gyfa>, label: &str, of: StatsOf) -> PopupMenuItem {
    let this = this.clone();
    entry(IconName::Info, format!("{label} Stats")).on_click(move |event, window, cx| {
        let _ = this.update(cx, |g, cx| g.show_stats(of, event, window, cx));
    })
}
