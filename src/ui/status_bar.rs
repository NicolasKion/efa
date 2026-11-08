//! The strip along the bottom of a fit window, from the left: a light for whether everything is
//! in order, the EVE data loaded, where prices come from, efa's version, and the last message.

use std::time::Instant;

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{ActiveTheme, Icon, Sizable, h_flex, spinner::Spinner};
use gpui_kit::{
    AnyElement, App, Context, Hsla, InteractiveElement, IntoElement, ParentElement, SharedString,
    Styled, div, px,
};

use super::widgets::TipExt;
use super::{Efa, Loading};

const HEIGHT: f32 = 24.;

/// How things stand, worst first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Health {
    Failed,
    Busy,
    Degraded,
    Ok,
}

impl Health {
    fn label(self) -> &'static str {
        match self {
            Self::Failed => "Error",
            Self::Busy => "Working",
            Self::Degraded => "Limited",
            Self::Ok => "Ready",
        }
    }

    fn color(self, cx: &App) -> Hsla {
        let theme = cx.theme();
        match self {
            Self::Failed => theme.danger,
            Self::Busy => theme.info,
            Self::Degraded => theme.warning,
            Self::Ok => theme.success,
        }
    }
}

/// The status line's message, and whether it reports a failure, which shows in the error color.
#[derive(Debug, Clone)]
pub struct Status {
    pub text: SharedString,
    pub error: bool,
}

impl Efa {
    /// Show a message in the status line.
    pub(super) fn set_status(&mut self, text: impl Into<SharedString>) {
        self.status = Some(Status {
            text: text.into(),
            error: false,
        });
    }

    /// Show a failure in the status line, in the error color.
    pub(super) fn set_error(&mut self, text: impl Into<SharedString>) {
        self.status = Some(Status {
            text: text.into(),
            error: true,
        });
    }

    /// Show a result in the status line: the message, or the failure in the error color.
    pub(super) fn set_outcome(&mut self, outcome: Result<SharedString, SharedString>) {
        match outcome {
            Ok(text) => self.set_status(text),
            Err(text) => self.set_error(text),
        }
    }

    pub(super) fn status_bar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (muted, border) = (theme.muted_foreground, theme.border);
        h_flex()
            .id("status-bar")
            .flex_none()
            .h(px(HEIGHT))
            .px_2()
            .border_t_1()
            .border_color(border)
            .bg(theme.title_bar)
            .text_xs()
            .text_color(muted)
            .child(self.health_item(cx))
            .child(self.data_item(cx))
            .children(self.prices_item(cx))
            .child(item(
                "status-version",
                None,
                div().child(format!("efa {}", crate::update::VERSION)),
                "The efa version running".into(),
                cx,
            ))
            .children(self.message_item(cx))
    }

    /// Each check that feeds the light: what it found, and how bad that is.
    fn checks(&self) -> Vec<(Health, String)> {
        let mut checks = vec![match &self.loading {
            Loading::Pending => (Health::Busy, "Loading the EVE data".to_owned()),
            Loading::Failed(_) => (
                Health::Failed,
                "The EVE data could not be loaded".to_owned(),
            ),
            Loading::Ready(_) => (Health::Ok, "EVE data loaded".to_owned()),
        }];
        if self.prices_online() || !self.prices.is_empty() {
            let failing = self.prices_retry.is_some_and(|at| Instant::now() < at);
            checks.push(if self.prices_fetch.is_some() {
                (Health::Busy, "Fetching prices".to_owned())
            } else if failing {
                (
                    Health::Degraded,
                    "Prices could not be fetched, trying again shortly".to_owned(),
                )
            } else if self.prices.is_empty() {
                (Health::Ok, "No prices needed yet".to_owned())
            } else {
                (Health::Ok, "Prices loaded".to_owned())
            });
        } else {
            checks.push((Health::Ok, "Price fetching is off".to_owned()));
        }
        checks
    }

    /// The light and its word, with every check in the tooltip.
    fn health_item(&self, cx: &App) -> AnyElement {
        let checks = self.checks();
        let health = checks.iter().map(|(h, _)| *h).min().unwrap_or(Health::Ok);
        let color = health.color(cx);
        let tip: SharedString = checks
            .iter()
            .map(|(h, text)| {
                let mark = if *h == Health::Ok { "✓" } else { "•" };
                format!("{mark} {text}")
            })
            .collect::<Vec<_>>()
            .join("\n")
            .into();
        h_flex()
            .id("status-health")
            .flex_none()
            .gap_1p5()
            .pr_3()
            .child(if health == Health::Busy {
                Spinner::new().xsmall().color(color).into_any_element()
            } else {
                div()
                    .size(px(7.))
                    .rounded_full()
                    .bg(color)
                    .into_any_element()
            })
            .child(
                div()
                    .text_color(cx.theme().foreground)
                    .child(health.label()),
            )
            .tip(tip)
            .into_any_element()
    }

    /// Which EVE data is loaded.
    fn data_item(&self, cx: &App) -> AnyElement {
        let (label, tip): (String, SharedString) = match &self.loading {
            Loading::Pending => ("Loading EVE data".into(), "Reading the EVE data".into()),
            Loading::Failed(e) => ("No EVE data".into(), e.clone()),
            Loading::Ready(loaded) => {
                let version = loaded.source.version();
                let build = version.as_deref().and_then(eve_build);
                let from = match &loaded.source {
                    crate::pack::Source::Pack(_) => "built into efa".to_owned(),
                    crate::pack::Source::Dump { dir, .. } => {
                        format!("read from {}", dir.display())
                    }
                };
                (
                    build.map_or_else(|| "EVE data".to_owned(), |b| format!("EVE {b}")),
                    format!(
                        "Static data {}, {from}. Loaded in {} ms.",
                        version.as_deref().unwrap_or("of an unknown version"),
                        loaded.load_time.as_millis(),
                    )
                    .into(),
                )
            }
        };
        item(
            "status-data",
            Some(AssetIcon::Database),
            div().child(label),
            tip,
            cx,
        )
    }

    /// Where prices come from. Nothing when prices are switched off and there are none.
    fn prices_item(&self, cx: &App) -> Option<AnyElement> {
        if !self.prices_online() && self.prices.is_empty() {
            return None;
        }
        let market = &self.saved.preferences.market;
        let source = market.price_source;
        let label = if source.uses_system() {
            format!("{}, {}", source.label(), market.price_system.label())
        } else {
            source.label().to_owned()
        };
        let tip = if self.prices_fetch.is_some() {
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
            Some(AssetIcon::Coins),
            div().child(label),
            tip.into(),
            cx,
        ))
    }

    /// The last message, filling what is left; failures in the error color.
    fn message_item(&self, cx: &App) -> Option<AnyElement> {
        let Status {
            text: status,
            error,
        } = self.status.clone()?;
        let color = if error {
            cx.theme().danger
        } else {
            cx.theme().muted_foreground
        };
        Some(
            h_flex()
                .id("status-message")
                .flex_1()
                .min_w_0()
                .gap_1p5()
                .pl_3()
                .border_l_1()
                .border_color(cx.theme().border)
                .text_color(color)
                .child(
                    Icon::new(if error {
                        AssetIcon::TriangleAlert
                    } else {
                        AssetIcon::Info
                    })
                    .xsmall()
                    .flex_none(),
                )
                .child(div().min_w_0().truncate().child(status.clone()))
                .tip(status)
                .into_any_element(),
        )
    }
}

/// One entry of the bar, set off from the one before, with its tooltip.
fn item(
    id: &'static str,
    icon: Option<AssetIcon>,
    content: impl IntoElement,
    tip: SharedString,
    cx: &App,
) -> AnyElement {
    h_flex()
        .id(id)
        .flex_none()
        .gap_1p5()
        .px_3()
        .border_l_1()
        .border_color(cx.theme().border)
        .children(icon.map(|icon| Icon::new(icon).xsmall()))
        .child(content)
        .tip(tip)
        .into_any_element()
}

/// The game build in a deimos release name, `sde-3670638-client-3670638` → `3670638`.
fn eve_build(version: &str) -> Option<&str> {
    let build = version.strip_prefix("sde-")?.split('-').next()?;
    (!build.is_empty() && build.bytes().all(|b| b.is_ascii_digit())).then_some(build)
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
    fn the_worst_check_sets_the_light() {
        let worst = [Health::Ok, Health::Degraded, Health::Busy]
            .into_iter()
            .min();
        assert_eq!(worst, Some(Health::Busy));
    }
}
