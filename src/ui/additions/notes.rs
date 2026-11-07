//! The Notes tab: free text kept with the fit.

use std::rc::Rc;

use gpui_kit::{
    AnyElement, Context, IntoElement, ParentElement, SharedString, Styled, div, relative,
};

use crate::fit::Fit;
use crate::ui::{Efa, Loaded};

/// Longest notes excerpt pyfa puts in a fitting browser tooltip, ellipsis included.
const TOOLTIP_NOTES_MAX: usize = 200;
const ELLIPSIS: &str = "...";

impl Efa {
    pub(in crate::ui) fn notes_tab(
        &mut self,
        _loaded: &Rc<Loaded>,
        _cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .p_2()
            .flex_1()
            .min_h_0()
            .child(self.notes_field().h(relative(1.)))
            .into_any_element()
    }

    /// Keep what was typed in the current fit. The fit always holds the latest text, so switching
    /// fits loses nothing, but the file is written by the poll rather than on every keystroke,
    /// the way pyfa holds its database write back while typing goes on.
    pub(in crate::ui) fn edit_notes(&mut self, text: String, cx: &mut Context<Self>) {
        let Some(i) = self.saved.current else { return };
        self.saved.fits[i].notes = text;
        self.saved.fits[i].modified = crate::fit::now();
        self.unsaved = true;
        // The tab title counts characters.
        cx.notify();
    }

    /// Hover text for a fit in the fittings browser, carrying its notes as pyfa's does. None
    /// without notes; the browser adds the ship's traits below when it shows them.
    pub(in crate::ui) fn fitting_tooltip(fit: &Fit, ship_name: &str) -> Option<SharedString> {
        tooltip(ship_name, &fit.notes).map(SharedString::from)
    }
}

/// The Notes tab title: the character count in brackets once there is any text, as pyfa shows it
/// with its default "quantity" tab labels.
pub(super) fn tab_title(fit: &Fit) -> String {
    match fit.notes.chars().count() {
        0 => "Notes".to_owned(),
        n => format!("Notes ({})", short_count(n)),
    }
}

/// A count the way pyfa's `formatAmount(n, 2, 0, 3)` writes it: exact below a thousand, then
/// thousands to two significant digits. pyfa caps the suffix at k, so a million is "1000k".
fn short_count(n: usize) -> String {
    if n < 1000 {
        return n.to_string();
    }
    #[expect(
        clippy::cast_precision_loss,
        reason = "note lengths are far below where f64 loses integers"
    )]
    let thousands = n as f64 / 1000.;
    let digits = if thousands < 10. {
        format!("{thousands:.1}")
    } else {
        format!("{thousands:.0}")
    };
    let digits = digits.strip_suffix(".0").unwrap_or(&digits);
    format!("{digits}k")
}

/// pyfa's fit tooltip: the ship, then the notes between rules, cut to 200 characters.
fn tooltip(ship_name: &str, notes: &str) -> Option<String> {
    if notes.is_empty() {
        return None;
    }
    let rule = "─".repeat(20);
    let excerpt = if notes.chars().count() > TOOLTIP_NOTES_MAX {
        let keep: String = notes
            .chars()
            .take(TOOLTIP_NOTES_MAX - ELLIPSIS.len())
            .collect();
        format!("{keep}{ELLIPSIS}")
    } else {
        notes.to_owned()
    };
    Some(format!("{ship_name}\n{rule}\nNotes: {excerpt}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with_notes(notes: &str) -> Fit {
        Fit {
            notes: notes.to_owned(),
            ..Fit::new("Test")
        }
    }

    #[test]
    fn short_count_matches_pyfa() {
        let cases = [
            (1, "1"),
            (999, "999"),
            (1000, "1k"),
            (1234, "1.2k"),
            (1999, "2k"),
            (9960, "10k"),
            (15_000, "15k"),
            (123_456, "123k"),
            (999_999, "1000k"),
        ];
        for (n, want) in cases {
            assert_eq!(short_count(n), want, "count {n}");
        }
    }

    #[test]
    fn tab_title_counts_characters() {
        assert_eq!(tab_title(&with_notes("")), "Notes");
        assert_eq!(tab_title(&with_notes("Fit for äbyssal")), "Notes (15)");
        assert_eq!(tab_title(&with_notes(&"x".repeat(2500))), "Notes (2.5k)");
    }

    #[test]
    fn tooltip_only_with_notes() {
        assert_eq!(tooltip("Rifter", ""), None);
        let tip = tooltip("Rifter", "Kite at 20km").expect("notes give a tooltip");
        assert_eq!(
            tip,
            format!("Rifter\n{}\nNotes: Kite at 20km", "─".repeat(20))
        );
    }

    #[test]
    fn tooltip_cuts_long_notes_like_pyfa() {
        let exact = "é".repeat(TOOLTIP_NOTES_MAX);
        let tip = tooltip("Rifter", &exact).expect("notes give a tooltip");
        assert!(tip.ends_with(&exact), "200 characters stay whole");

        let long = "é".repeat(TOOLTIP_NOTES_MAX + 1);
        let tip = tooltip("Rifter", &long).expect("notes give a tooltip");
        let excerpt = tip.rsplit("Notes: ").next().expect("notes line");
        assert_eq!(excerpt, format!("{}...", "é".repeat(197)));
    }
}
