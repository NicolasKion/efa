//! Line colors, lightness and dash styles, pyfa's graphs/style.py on the Omarchy palette.

use gpui_kit::component::Theme;
use gpui_kit::{Hsla, Pixels, px};

/// A fit's line color. pyfa's eight hues; the ones the theme has come from the theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineColor {
    Red,
    Green,
    Blue,
    Orange,
    Magenta,
    Cyan,
    Purple,
    Yellow,
}

impl LineColor {
    pub const ALL: [Self; 8] = [
        Self::Red,
        Self::Green,
        Self::Blue,
        Self::Orange,
        Self::Magenta,
        Self::Cyan,
        Self::Purple,
        Self::Yellow,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Red => "Red",
            Self::Green => "Green",
            Self::Blue => "Blue",
            Self::Orange => "Orange",
            Self::Magenta => "Magenta",
            Self::Cyan => "Cyan",
            Self::Purple => "Purple",
            Self::Yellow => "Yellow",
        }
    }

    /// The theme's own red, green, blue and yellow (the Omarchy palette's, mapped in
    /// `widgets::apply_palette`); pyfa's hues for the rest.
    pub fn color(self, theme: &Theme) -> Hsla {
        let hue = |degrees: f32| gpui_kit::hsla(degrees / 360.0, 0.85, 0.6, 1.0);
        match self {
            Self::Red => theme.red,
            Self::Green => theme.green,
            Self::Blue => theme.blue,
            Self::Yellow => theme.yellow,
            Self::Orange => hue(30.0),
            Self::Magenta => hue(300.0),
            Self::Cyan => hue(180.0),
            Self::Purple => hue(275.0),
        }
    }

    /// The color used least among `taken`, earliest first, as pyfa picks for a new fit.
    pub fn least_used(taken: impl Iterator<Item = Self>) -> Self {
        let taken: Vec<Self> = taken.collect();
        let uses = |c: Self| taken.iter().filter(|&&t| t == c).count();
        let least = Self::ALL.iter().map(|&c| uses(c)).min().unwrap_or(0);
        Self::ALL
            .into_iter()
            .find(|&c| uses(c) == least)
            .unwrap_or(Self::Red)
    }
}

/// A target's shade of its fit's color.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lightness {
    Normal,
    Dark,
    Bright,
}

impl Lightness {
    pub const ALL: [Self; 3] = [Self::Normal, Self::Dark, Self::Bright];

    pub fn label(self) -> &'static str {
        match self {
            Self::Normal => "Normal",
            Self::Dark => "Dark",
            Self::Bright => "Bright",
        }
    }

    pub fn apply(self, color: Hsla) -> Hsla {
        match self {
            Self::Normal => color,
            Self::Dark => Hsla {
                s: color.s * 0.5,
                l: color.l * 0.7,
                ..color
            },
            Self::Bright => Hsla {
                s: color.s * 0.5,
                l: color.l + (1.0 - color.l) * 0.5,
                ..color
            },
        }
    }
}

/// A target's dash pattern.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineStyle {
    Solid,
    Dashed,
    Dotted,
    DashDotted,
}

impl LineStyle {
    pub const ALL: [Self; 4] = [Self::Solid, Self::Dashed, Self::Dotted, Self::DashDotted];

    pub fn label(self) -> &'static str {
        match self {
            Self::Solid => "Solid",
            Self::Dashed => "Dashed",
            Self::Dotted => "Dotted",
            Self::DashDotted => "Dash-dotted",
        }
    }

    /// Dash lengths for a line `width` wide; matplotlib's patterns are in line widths.
    pub fn dashes(self, width: f32) -> Vec<Pixels> {
        let pattern: &[f32] = match self {
            Self::Solid => &[],
            Self::Dashed => &[5.0, 1.0],
            Self::Dotted => &[1.0, 1.0],
            Self::DashDotted => &[3.0, 1.0, 1.0, 1.0],
        };
        pattern.iter().map(|&v| px(v * width * 1.6)).collect()
    }
}

/// The style used least among `taken`, earliest first.
pub fn least_used<T: Copy + PartialEq>(all: &[T], taken: &[T]) -> T {
    let uses = |c: T| taken.iter().filter(|&&t| t == c).count();
    let least = all.iter().map(|&c| uses(c)).min().unwrap_or(0);
    all.iter()
        .copied()
        .find(|&c| uses(c) == least)
        .unwrap_or(all[0])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_fits_take_the_least_used_color() {
        let taken = [LineColor::Red, LineColor::Green, LineColor::Red];
        assert_eq!(LineColor::least_used(taken.into_iter()), LineColor::Blue);
        assert_eq!(
            least_used(&LineStyle::ALL, &[LineStyle::Solid]),
            LineStyle::Dashed
        );
    }
}
