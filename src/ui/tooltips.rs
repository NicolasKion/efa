//! Hover texts shared by the market, the fittings browser and the editors.

use std::rc::Rc;

use gpui_kit::component::{tooltip::Tooltip, v_flex};
use gpui_kit::{AnyView, App, ParentElement, Styled, Window, div, px};

use crate::data::TypeId;
use crate::ui::{Loaded, item_data, item_stats, stats::format};

/// Widest a hover text gets before it wraps.
const TOOLTIP_W: f32 = 380.;
/// Longest description excerpt a market tooltip shows.
const DESCRIPTION_MAX: usize = 400;

/// Hover text as lines in a box that wraps.
pub(super) fn text_tooltip(lines: Vec<String>, window: &mut Window, cx: &mut App) -> AnyView {
    Tooltip::element(move |_, _| {
        v_flex()
            .max_w(px(TOOLTIP_W))
            .children(lines.iter().map(|line| {
                if line.is_empty() {
                    div().h(px(8.))
                } else {
                    div().child(line.clone())
                }
            }))
    })
    .build(window, cx)
}

/// A market item's hover text: name, Omega, price and the start of its description.
pub(super) fn item_tooltip(
    loaded: &Rc<Loaded>,
    id: TypeId,
    price: Option<f64>,
    window: &mut Window,
    cx: &mut App,
) -> AnyView {
    let mut lines = vec![loaded.catalog.name(id)];
    if loaded.catalog.get(id).is_some_and(|t| t.omega) {
        lines.push("Omega clones only".to_owned());
    }
    if let Some(price) = price {
        lines.push(format!("Price: {}", format::currency(price)));
    }
    let description =
        item_data::hover_data(loaded, cx).and_then(|data| item_stats::description_text(&data, id));
    if let Some(description) = description {
        lines.push(String::new());
        let mut excerpt: String = description.chars().take(DESCRIPTION_MAX).collect();
        if excerpt.len() < description.len() {
            excerpt.push_str("...");
        }
        lines.extend(excerpt.lines().map(str::to_owned));
    }
    text_tooltip(lines, window, cx)
}

/// What a fittings browser tooltip starts with before the traits.
pub(super) enum TipHead {
    /// Nothing: a ship row shows only the traits.
    Traits,
    /// A fit row: the ship, or the ship with the notes when there are any.
    Fit(Option<String>),
}

/// pyfa's ship and fit hover text: for a fit the ship and its notes, then the ship's traits.
pub(super) fn ship_tooltip(
    loaded: &Rc<Loaded>,
    ship: TypeId,
    head: TipHead,
    window: &mut Window,
    cx: &mut App,
) -> AnyView {
    let rule = "─".repeat(20);
    let traits = item_data::hover_data(loaded, cx)
        .and_then(|data| item_stats::traits_text(loaded, &data, ship));
    let mut text = match head {
        TipHead::Fit(Some(notes)) => notes,
        TipHead::Fit(None) => loaded.catalog.name(ship),
        TipHead::Traits => String::new(),
    };
    if let Some(traits) = traits {
        if !text.is_empty() {
            text.push('\n');
            text.push_str(&rule);
            text.push('\n');
        }
        text.push_str(&traits);
    }
    if text.is_empty() {
        text = loaded.catalog.name(ship);
    }
    text_tooltip(text.lines().map(str::to_owned).collect(), window, cx)
}
