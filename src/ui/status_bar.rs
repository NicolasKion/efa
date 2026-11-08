//! The strip along the bottom of a fit window: the last message on the left, and on the right
//! where prices come from, which EVE data is loaded, and efa's version.

use gpui_kit::component::{ActiveTheme, Sizable, h_flex, spinner::Spinner, tooltip::Tooltip};
use gpui_kit::{
    AnyElement, App, Context, InteractiveElement, IntoElement, ParentElement, SharedString,
    StatefulInteractiveElement, Styled, div, prelude::FluentBuilder, px,
};

use super::{Efa, Loading};

const HEIGHT: f32 = 24.;

impl Efa {
    pub(super) fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border, danger) = (theme.muted_foreground, theme.border, theme.danger);
        let message = self.status.clone().map(|status| {
            let error = looks_like_error(&status);
            div()
                .id("status-message")
                .min_w_0()
                .truncate()
                .when(error, |d| d.text_color(danger))
                .child(status.clone())
                .tooltip(move |window, cx| Tooltip::new(status.clone()).build(window, cx))
        });
        h_flex()
            .id("status-bar")
            .flex_none()
            .h(px(HEIGHT))
            .px_2()
            .gap_3()
            .border_t_1()
            .border_color(border)
            .bg(theme.title_bar)
            .text_xs()
            .text_color(muted)
            .child(h_flex().flex_1().min_w_0().children(message))
            .children(self.prices_item(cx))
            .child(self.data_item(cx))
            .child(item(
                "status-version",
                div().child(format!("efa {}", crate::update::VERSION)),
                "The efa version running".into(),
                cx,
            ))
    }

    /// Where prices come from, and whether they are being fetched. Nothing when prices are
    /// switched off and there are none.
    fn prices_item(&self, cx: &App) -> Option<AnyElement> {
        if !self.prices_online() && self.prices.is_empty() {
            return None;
        }
        let market = &self.saved.preferences.market;
        let fetching = self.prices_fetch.is_some();
        let source = market.price_source;
        let label = if source.uses_system() {
            format!("{}, {}", source.label(), market.price_system.label())
        } else {
            source.label().to_owned()
        };
        let tip = if fetching {
            format!("Fetching prices from {label}")
        } else if self.prices.is_empty() {
            format!("No prices yet from {label}")
        } else if self.prices.is_stale() {
            format!("Prices from {label}, due for a refresh")
        } else {
            format!("Prices from {label}")
        };
        Some(item(
            "status-prices",
            h_flex()
                .gap_1()
                .when(fetching, |d| d.child(Spinner::new().xsmall()))
                .child(if fetching {
                    "Updating prices".to_owned()
                } else {
                    label
                }),
            tip.into(),
            cx,
        ))
    }

    /// Which EVE data is loaded, or that it is loading or failed to.
    fn data_item(&self, cx: &App) -> AnyElement {
        let danger = cx.theme().danger;
        match &self.loading {
            Loading::Pending => item(
                "status-data",
                h_flex()
                    .gap_1()
                    .child(Spinner::new().xsmall())
                    .child("Loading EVE data"),
                "Reading the EVE data".into(),
                cx,
            ),
            Loading::Failed(e) => item(
                "status-data",
                div().text_color(danger).child("EVE data failed to load"),
                e.clone(),
                cx,
            ),
            Loading::Ready(loaded) => {
                let version = loaded.source.version();
                let build = version.as_deref().and_then(eve_build);
                let label = build.map_or_else(|| "EVE data".to_owned(), |b| format!("EVE {b}"));
                let from = match &loaded.source {
                    crate::pack::Source::Pack(_) => "built into efa".to_owned(),
                    crate::pack::Source::Dump { dir, .. } => {
                        format!("read from {}", dir.display())
                    }
                };
                let tip = format!(
                    "Static data {}, {from}. Loaded in {} ms.",
                    version.as_deref().unwrap_or("of an unknown version"),
                    loaded.load_time.as_millis(),
                );
                item("status-data", div().child(label), tip.into(), cx)
            }
        }
    }
}

/// One entry on the right of the bar, with its tooltip.
fn item(id: &'static str, content: impl IntoElement, tip: SharedString, cx: &App) -> AnyElement {
    h_flex()
        .id(id)
        .flex_none()
        .pl_3()
        .border_l_1()
        .border_color(cx.theme().border)
        .child(content)
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        .into_any_element()
}

/// The game build in a deimos release name, `sde-3670638-client-3670638` → `3670638`.
fn eve_build(version: &str) -> Option<&str> {
    let build = version.strip_prefix("sde-")?.split('-').next()?;
    (!build.is_empty() && build.bytes().all(|b| b.is_ascii_digit())).then_some(build)
}

/// Messages that report a failure are shown in the error color.
fn looks_like_error(message: &str) -> bool {
    let lower = message.to_lowercase();
    ["could not", "failed", "error", "cannot", "can't"]
        .iter()
        .any(|word| lower.contains(word))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_build_comes_out_of_the_release_name() {
        assert_eq!(eve_build("sde-3670638-client-3670638"), Some("3670638"));
        assert_eq!(eve_build("something-else"), None);
    }

    #[test]
    fn failures_read_as_errors() {
        assert!(looks_like_error("Could not fetch prices: timeout"));
        assert!(looks_like_error("Import failed: no ship"));
        assert!(!looks_like_error("Saved the cargo as preset Ammo"));
    }
}
