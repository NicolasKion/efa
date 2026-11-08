//! The Database page: deleting damage patterns, target profiles or prices after asking.

use gpui_kit::component::{ActiveTheme, Sizable};
use gpui_kit::{AnyElement, App, Context, IntoElement, ParentElement, Styled, Window, div, px};

use super::PreferencesWindow;
use super::controls::{row, section};
use crate::ui::dialogs::{Confirm, confirm};

/// What the Database page can delete.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wipe {
    DamagePatterns,
    TargetProfiles,
    Prices,
}

impl Wipe {
    fn button(self) -> &'static str {
        match self {
            Self::DamagePatterns => "Delete All Damage Pattern Profiles",
            Self::TargetProfiles => "Delete All Target Profiles",
            Self::Prices => "Delete All Prices",
        }
    }

    /// pyfa's question before going ahead.
    fn question(self) -> &'static str {
        match self {
            Self::DamagePatterns => {
                "This is a destructive action that will delete all damage pattern profiles. \
                 Are you sure you want to do this?"
            }
            Self::TargetProfiles => {
                "This is a destructive action that will delete all target profiles. Are you \
                 sure you want to do this?"
            }
            Self::Prices => {
                "This is a destructive action that will delete all cached prices. Are you sure \
                 you want to do this?"
            }
        }
    }
}

impl PreferencesWindow {
    /// pyfa's Database page: where things are kept, and the three deletes.
    pub(super) fn database(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let cache_dir = match &self.hub.read(cx).loading {
            crate::ui::hub::HubLoading::Ready(loaded) => Some(loaded.cache_dir.clone()),
            _ => None,
        };
        let shown = |path: Option<std::path::PathBuf>| {
            path.map_or_else(|| "unknown".to_owned(), |p| p.display().to_string())
        };
        let path_row = |title: &'static str, path: String, cx: &App| {
            row(
                title,
                "",
                div()
                    .text_xs()
                    .max_w(px(360.))
                    .text_color(cx.theme().muted_foreground)
                    .child(path)
                    .into_any_element(),
                false,
                false,
                cx,
            )
        };
        let wipes = [Wipe::DamagePatterns, Wipe::TargetProfiles, Wipe::Prices]
            .into_iter()
            .map(|wipe| {
                let description = match wipe {
                    Wipe::DamagePatterns => "Your own damage patterns. The built-in ones stay.",
                    Wipe::TargetProfiles => "Your own target profiles. The built-in ones stay.",
                    Wipe::Prices => "Every price fetched so far. They are fetched again.",
                };
                let control = gpui_kit::component::button::Button::new(wipe.button())
                    .small()
                    .label(wipe.button())
                    .on_click(cx.listener(move |_, _, window, cx| {
                        Self::confirm_wipe(wipe, window, cx);
                    }))
                    .into_any_element();
                row(wipe.button(), description, control, false, false, cx)
            })
            .collect();
        vec![
            section(
                "Locations",
                vec![
                    path_row(
                        "Fits, characters and settings",
                        shown(crate::db::path()),
                        cx,
                    ),
                    path_row("Cache", shown(cache_dir), cx),
                ],
                cx,
            ),
            section("Start over", wipes, cx),
        ]
    }

    fn confirm_wipe(wipe: Wipe, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.weak_entity();
        let question = Confirm {
            danger: true,
            ..Confirm::new("Confirm", wipe.question())
        };
        confirm(window, cx, question, move |_, cx| {
            let _ = this.update(cx, |this, cx| this.wipe(wipe, cx));
        });
    }

    fn wipe(&self, wipe: Wipe, cx: &mut Context<Self>) {
        match wipe {
            Wipe::DamagePatterns => self.set(cx, |s| s.damage_patterns.clear()),
            Wipe::TargetProfiles => self.set(cx, |s| s.target_profiles.clear()),
            Wipe::Prices => {
                let cache_dir = match &self.hub.read(cx).loading {
                    crate::ui::hub::HubLoading::Ready(loaded) => Some(loaded.cache_dir.clone()),
                    _ => None,
                };
                if let Some(dir) = cache_dir
                    && let Err(e) = crate::prices::Prices::delete_cached(&dir)
                {
                    crate::errors::report("Deleting the prices failed", e);
                }
                self.hub.update(cx, |hub, cx| {
                    hub.set_prices(std::rc::Rc::new(crate::prices::Prices::default()), cx);
                });
            }
        }
    }
}
