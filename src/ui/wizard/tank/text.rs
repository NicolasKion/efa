//! The goals' numbers, units and changes as text, and the sentences around them.

use gpui_kit::component::ActiveTheme;
use gpui_kit::{App, Hsla, SharedString};

use crate::{
    data::Slot,
    theme::Palette,
    ui::widgets,
    wizard::tank::{Goal, Limit},
};

/// A goal's number in a results column.
pub(super) fn goal_number(goal: Goal, value: f64) -> String {
    match goal {
        Goal::Ehp => widgets::short(value),
        Goal::Resists => format!("{value:.2}x"),
        Goal::Repair => format!("{value:.0}"),
    }
}

/// A goal's column title, which names its unit.
pub(super) fn goal_unit(goal: Goal) -> &'static str {
    match goal {
        Goal::Ehp => "EHP",
        Goal::Resists => "Resists",
        Goal::Repair => "EHP/s",
    }
}

/// How a goal's number moves from `now` to `after`, green up and red down.
pub(super) fn delta(
    goal: Goal,
    now: f64,
    after: f64,
    palette: Palette,
    cx: &App,
) -> (String, Hsla) {
    let change = after - now;
    let margin = 1e-6 * after.abs().max(1.0);
    if change.abs() <= margin {
        return ("same".to_owned(), cx.theme().muted_foreground);
    }
    let text = match goal {
        Goal::Resists => format!("{change:+.2}x"),
        Goal::Ehp => format!(
            "{}{}",
            if change >= 0.0 { "+" } else { "-" },
            widgets::short(change.abs())
        ),
        Goal::Repair => format!("{change:+.0}"),
    };
    let color = if change > 0.0 {
        palette.green
    } else {
        palette.red
    };
    (text, widgets::color(color))
}

/// What a goal aims for, in a sentence.
pub(super) fn goal_text(goal: Goal) -> &'static str {
    match goal {
        Goal::Ehp => "The most effective HP over all layers, however it gets there.",
        Goal::Resists => {
            "The highest resist multiplier on the layer, so every repair lands hardest; EHP \
             decides between equals."
        }
        Goal::Repair => {
            "The most effective HP repaired a second: the repair the capacitor keeps up, times \
             the resists."
        }
    }
}

pub(super) fn goal_text_short(goal: Goal) -> &'static str {
    match goal {
        Goal::Ehp => "for max EHP",
        Goal::Resists => "for max resists",
        Goal::Repair => "for max repair",
    }
}

/// A slot as a word in a sentence: "mid", "low".
pub(super) fn slot_word(slot: Slot) -> &'static str {
    match slot {
        Slot::High => "high",
        Slot::Mid => "mid",
        Slot::Low => "low",
        _ => "rig",
    }
}

/// Why the search's better tanks did not make it.
pub(super) fn limit_text(limit: Limit) -> SharedString {
    let rest = match limit {
        Limit::Cpu | Limit::Powergrid => "better modules did not fit.",
        Limit::Calibration => "better rigs did not fit.",
        Limit::Budget => {
            return "The budget is the limit: better tanks cost more. Raise it to see them.".into();
        }
    };
    format!("{} is the limit: {rest}", limit.label()).into()
}
