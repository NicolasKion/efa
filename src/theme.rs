//! The Omarchy theme's palette. The active theme's `colors.toml` is read here; the UI maps it onto
//! its widgets and polls [`generation`] to notice `omarchy theme set`.
//!
//! Without Omarchy, [`Palette::default`] gives a plain dark theme.

use std::path::PathBuf;

use serde::Deserialize;

/// A color as 0xRRGGBB.
pub type Rgb = u32;

/// The theme's colors that efa uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub dark: bool,
    pub accent: Rgb,
    pub selection: Rgb,
    pub muted: Rgb,
    pub background: Rgb,
    pub dark_background: Rgb,
    pub darker_background: Rgb,
    pub lighter_background: Rgb,
    pub foreground: Rgb,
    pub dark_foreground: Rgb,
    pub red: Rgb,
    pub yellow: Rgb,
    pub orange: Rgb,
    pub green: Rgb,
    pub blue: Rgb,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            dark: true,
            accent: 0x005a_aaff,
            selection: 0x0000_5c80,
            muted: 0x006e_6e6e,
            background: 0x001b_1b1b,
            dark_background: 0x0014_1414,
            darker_background: 0x000a_0a0a,
            lighter_background: 0x0028_2828,
            foreground: 0x00c8_c8c8,
            dark_foreground: 0x008c_8c8c,
            red: 0x00e6_6464,
            yellow: 0x00d2_b428,
            orange: 0x00e6_5a1e,
            green: 0x005a_c850,
            blue: 0x0034_78be,
        }
    }
}

impl Palette {
    /// The active Omarchy theme's palette, or None when there is no Omarchy theme to follow.
    pub fn current() -> Option<Self> {
        let text = std::fs::read_to_string(theme_dir()?.join("colors.toml")).ok()?;
        Self::parse(&text).ok()
    }

    /// Read a theme's `colors.toml`.
    pub fn parse(text: &str) -> Result<Self, String> {
        let raw: RawColors = toml::from_str(text).map_err(|e| e.to_string())?;
        let color = |hex: &str| parse_hex(hex).ok_or_else(|| format!("not a color: {hex}"));
        Ok(Self {
            dark: raw.mode.as_deref() != Some("light"),
            accent: color(&raw.accent)?,
            selection: color(&raw.selection)?,
            muted: color(&raw.muted)?,
            background: color(&raw.background)?,
            dark_background: color(&raw.dark_background)?,
            darker_background: color(&raw.darker_background)?,
            lighter_background: color(&raw.lighter_background)?,
            foreground: color(&raw.foreground)?,
            dark_foreground: color(&raw.dark_foreground)?,
            red: color(&raw.red)?,
            yellow: color(&raw.yellow)?,
            // A few themes have no orange; their yellow is the nearest warm color.
            orange: color(raw.orange.as_deref().unwrap_or(&raw.yellow))?,
            green: color(&raw.green)?,
            blue: color(&raw.blue)?,
        })
    }
}

/// Changes every time Omarchy switches theme, after the new files are in place. None without
/// Omarchy.
pub fn generation() -> Option<String> {
    std::fs::read_to_string(theme_dir()?.with_file_name("theme.generation")).ok()
}

fn theme_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let dir = PathBuf::from(home).join(".local/state/omarchy/current/theme");
    dir.is_dir().then_some(dir)
}

/// "#8bcbb1" or "8bcbb1".
fn parse_hex(hex: &str) -> Option<Rgb> {
    let hex = hex.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    u32::from_str_radix(hex, 16).ok()
}

#[derive(Deserialize)]
struct RawColors {
    mode: Option<String>,
    accent: String,
    selection: String,
    muted: String,
    background: String,
    dark_background: String,
    darker_background: String,
    lighter_background: String,
    foreground: String,
    dark_foreground: String,
    red: String,
    yellow: String,
    orange: Option<String>,
    green: String,
    blue: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_an_omarchy_palette() {
        let text = r##"
            mode = "dark"
            accent = "#8bcbb1"
            selection = "#2b5147"
            muted = "#64887b"
            background = "#142620"
            dark_background = "#0f1e19"
            darker_background = "#0a1713"
            lighter_background = "#203b31"
            foreground = "#dfede5"
            dark_foreground = "#91ad9f"
            light_foreground = "#bed6c9"
            red = "#d88d91"
            yellow = "#c5cc91"
            green = "#8bcbb1"
            blue = "#96b7d0"
        "##;
        let palette = Palette::parse(text).expect("a full palette parses");
        assert!(palette.dark);
        assert_eq!(palette.background, 0x0014_2620);
        assert_eq!(
            palette.orange, palette.yellow,
            "orange falls back to yellow"
        );
    }

    #[test]
    fn rejects_bad_hex() {
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex("zzzzzz"), None);
        assert_eq!(parse_hex("ffffff"), Some(0x00ff_ffff));
    }
}
