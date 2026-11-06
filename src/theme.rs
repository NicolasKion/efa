//! Follows the Omarchy theme. The active theme's `colors.toml` is mapped onto egui's visuals, and
//! a watcher reapplies it whenever `omarchy theme set` swaps the theme.
//!
//! Without Omarchy the app keeps egui's own dark theme.

use std::{
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
    time::Duration,
};

use eframe::egui::{self, Color32, Stroke};
use serde::Deserialize;

/// How often the watcher looks for a theme change. Switching themes is rare and a one second lag
/// is not noticeable next to the rest of the desktop restyling.
const POLL: Duration = Duration::from_secs(1);

/// The colors gyfa draws with, beyond what egui's visuals carry.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub accent: Color32,
    pub selection: Color32,
    pub muted: Color32,
    pub background: Color32,
    pub dark_background: Color32,
    pub darker_background: Color32,
    pub lighter_background: Color32,
    pub foreground: Color32,
    pub dark_foreground: Color32,
    pub red: Color32,
    pub yellow: Color32,
    pub orange: Color32,
    pub green: Color32,
}

impl Default for Palette {
    /// egui's dark theme, so the rest of the app can always ask for a palette.
    fn default() -> Self {
        Self {
            dark: true,
            accent: Color32::from_rgb(90, 170, 255),
            selection: Color32::from_rgb(0, 92, 128),
            muted: Color32::from_gray(110),
            background: Color32::from_gray(27),
            dark_background: Color32::from_gray(20),
            darker_background: Color32::from_gray(10),
            lighter_background: Color32::from_gray(40),
            foreground: Color32::from_gray(200),
            dark_foreground: Color32::from_gray(140),
            red: Color32::from_rgb(230, 100, 100),
            yellow: Color32::from_rgb(210, 180, 40),
            orange: Color32::from_rgb(230, 90, 30),
            green: Color32::from_rgb(90, 200, 80),
        }
    }
}

impl Palette {
    /// The active Omarchy theme's palette, or None when there is no Omarchy theme to follow.
    pub fn current() -> Option<Self> {
        let path = theme_dir()?.join("colors.toml");
        let text = std::fs::read_to_string(path).ok()?;
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
        })
    }

    /// Restyle the whole app with this palette.
    pub fn apply(&self, ctx: &egui::Context) {
        let mut v = if self.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        v.override_text_color = Some(self.foreground);
        v.weak_text_color = Some(self.dark_foreground);
        v.hyperlink_color = self.accent;
        v.panel_fill = self.background;
        v.window_fill = self.dark_background;
        v.extreme_bg_color = self.darker_background;
        v.faint_bg_color = self.lighter_background.gamma_multiply(0.5);
        v.code_bg_color = self.darker_background;
        v.warn_fg_color = self.yellow;
        v.error_fg_color = self.red;
        v.window_stroke = Stroke::new(1.0, self.lighter_background);
        v.selection.bg_fill = self.selection;
        v.selection.stroke = Stroke::new(1.0, self.accent);

        let w = &mut v.widgets;
        w.noninteractive.bg_fill = self.background;
        w.noninteractive.weak_bg_fill = self.background;
        w.noninteractive.bg_stroke = Stroke::new(1.0, self.lighter_background);
        w.noninteractive.fg_stroke = Stroke::new(1.0, self.foreground);
        for (state, fill) in [
            (&mut w.inactive, self.lighter_background),
            (&mut w.hovered, self.selection),
            (&mut w.active, self.selection),
            (&mut w.open, self.lighter_background),
        ] {
            state.bg_fill = fill;
            state.weak_bg_fill = fill;
            state.fg_stroke = Stroke::new(1.0, self.foreground);
        }
        w.hovered.bg_stroke = Stroke::new(1.0, self.accent);
        w.active.bg_stroke = Stroke::new(1.0, self.accent);
        ctx.set_visuals(v);
    }
}

/// Watch for theme switches, sending the new palette each time and waking the UI so it repaints.
/// The receiver never yields anything when there is no Omarchy theme.
pub fn watch(ctx: egui::Context) -> Receiver<Palette> {
    let (tx, rx) = mpsc::channel();
    let Some(dir) = theme_dir() else { return rx };
    // Omarchy bumps this on every `theme set`, after the new files are in place.
    let generation_file = dir.with_file_name("theme.generation");
    std::thread::spawn(move || {
        let mut seen = generation(&generation_file);
        loop {
            std::thread::sleep(POLL);
            let now = generation(&generation_file);
            if now == seen {
                continue;
            }
            seen = now;
            if let Some(palette) = Palette::current() {
                if tx.send(palette).is_err() {
                    return;
                }
                ctx.request_repaint();
            }
        }
    });
    rx
}

fn generation(path: &Path) -> Option<String> {
    std::fs::read_to_string(path).ok()
}

fn theme_dir() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    let dir = PathBuf::from(home).join(".local/state/omarchy/current/theme");
    dir.is_dir().then_some(dir)
}

/// "#8bcbb1" or "8bcbb1".
fn parse_hex(hex: &str) -> Option<Color32> {
    let hex = hex.trim().trim_start_matches('#');
    if hex.len() != 6 {
        return None;
    }
    let channel = |i: usize| u8::from_str_radix(hex.get(i..i + 2)?, 16).ok();
    Some(Color32::from_rgb(channel(0)?, channel(2)?, channel(4)?))
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
        "##;
        let palette = Palette::parse(text).expect("a full palette parses");
        assert!(palette.dark);
        assert_eq!(palette.background, Color32::from_rgb(0x14, 0x26, 0x20));
        assert_eq!(
            palette.orange, palette.yellow,
            "orange falls back to yellow"
        );
    }

    #[test]
    fn rejects_bad_hex() {
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex("zzzzzz"), None);
        assert_eq!(parse_hex("ffffff"), Some(Color32::WHITE));
    }
}
