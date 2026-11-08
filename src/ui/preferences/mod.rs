//! pyfa's preferences dialog (gui/preferenceDialog.py, gui/builtinPreferenceViews/) as a window
//! of its own. Every change goes straight to the hub, which saves it and hands it to every fit
//! window, the way pyfa applies a preference as soon as it is clicked.
//!
//! Left out on purpose: language and theme (efa follows the system and Omarchy's theme), gauge
//! animation, market shortcuts and market view modes, reactivation timers, pyfa's own SSO server
//! mode (efa always logs in through a local callback), and logging.

mod controls;
mod database;
mod pages;

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Icon as UiIcon, h_flex,
    input::{InputEvent, InputState},
    v_flex,
};
use gpui_kit::{
    AppContext, Context, Entity, FontWeight, InteractiveElement, IntoElement, ParentElement,
    Render, StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::FluentBuilder,
    px,
};

use super::{
    Efa, Hub,
    port::html_path,
    widgets::{TipExt, parse_decimal},
};
use crate::store::Saved;

pub(super) use controls::section;

/// Below this width the page list shows only its icons and the page sits closer to the edges.
const NARROW_W: f32 = 720.;
/// Below this height the page's title scrolls with its settings.
const SHORT_H: f32 = 520.;
/// The page list's width, and with icons only.
const PAGES_W: f32 = 200.;
const PAGES_NARROW_W: f32 = 48.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    General,
    Engine,
    Stats,
    ContextMenus,
    Market,
    Network,
    Html,
    Esi,
    Database,
}

impl Page {
    const ALL: [Self; 9] = [
        Self::General,
        Self::Engine,
        Self::Stats,
        Self::ContextMenus,
        Self::Market,
        Self::Network,
        Self::Html,
        Self::Esi,
        Self::Database,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Engine => "Fitting Engine",
            Self::Stats => "Statistics Panel",
            Self::ContextMenus => "Context Menus",
            Self::Market => "Market & Prices",
            Self::Network => "Network",
            Self::Html => "HTML Export",
            Self::Esi => "ESI",
            Self::Database => "Database",
        }
    }

    fn icon(self) -> AssetIcon {
        match self {
            Self::General => AssetIcon::Settings,
            Self::Engine => AssetIcon::Cpu,
            Self::Stats => AssetIcon::ChartColumn,
            Self::ContextMenus => AssetIcon::MousePointerClick,
            Self::Market => AssetIcon::Coins,
            Self::Network => AssetIcon::Globe,
            Self::Html => AssetIcon::FileCode,
            Self::Esi => AssetIcon::KeyRound,
            Self::Database => AssetIcon::Database,
        }
    }

    fn summary(self) -> &'static str {
        match self {
            Self::General => "How the fitting view, tabs and browsers look and behave.",
            Self::Engine => "How the fitting engine works out the numbers.",
            Self::Stats => "Which sections the stats pane shows.",
            Self::ContextMenus => "Hide menu entries you do not use.",
            Self::Market => "Market search and where prices come from.",
            Self::Network => "Internet access and proxy settings.",
            Self::Html => "Exporting every fit to one HTML page.",
            Self::Esi => "Logging in with EVE for skills and in-game fittings.",
            Self::Database => "Where efa keeps its files, and starting over.",
        }
    }
}

/// Text fields, kept so typing is not interrupted by redraws.
struct Fields {
    spool: Entity<InputState>,
    search_delay: Entity<InputState>,
    proxy_address: Entity<InputState>,
    proxy_user: Entity<InputState>,
    proxy_password: Entity<InputState>,
    html_path: Entity<InputState>,
    esi_client_id: Entity<InputState>,
    esi_port: Entity<InputState>,
}

pub struct PreferencesWindow {
    hub: Entity<Hub>,
    page: Page,
    fields: Fields,
    _subscriptions: Vec<Subscription>,
}

impl Efa {
    /// pyfa's Global > Preferences.
    pub(super) fn open_preferences(&mut self, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        let opening = crate::ui::chrome::open_single(
            "Preferences",
            (880., 640.),
            cx,
            |_: &mut PreferencesWindow, _, _| {},
            move |window, cx| PreferencesWindow::new(hub, window, cx),
        );
        Self::report_open(opening, cx);
    }
}

impl PreferencesWindow {
    fn new(hub: Entity<Hub>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let prefs = hub.read(cx).saved.settings.preferences.clone();
        let mut field = |value: String, placeholder: &str, masked: bool| {
            let placeholder = placeholder.to_owned();
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .masked(masked)
                    .default_value(value)
            })
        };
        let proxy = &prefs.network.proxy;
        let fields = Fields {
            spool: field(
                format!("{}", (prefs.engine.default_spool * 100.0).round()),
                "100",
                false,
            ),
            search_delay: field(prefs.market.search_delay_ms.to_string(), "250", false),
            proxy_address: field(proxy.address.clone(), "host:port", false),
            proxy_user: field(proxy.username.clone(), "Username", false),
            proxy_password: field(proxy.password.clone(), "Password", true),
            html_path: field(
                prefs.html.path.clone(),
                &html_path("").display().to_string(),
                false,
            ),
            esi_client_id: field(prefs.esi.client_id.clone(), crate::esi::CLIENT_ID, false),
            esi_port: field(
                prefs.esi.port.to_string(),
                &crate::esi::DEFAULT_PORT.to_string(),
                false,
            ),
        };
        let on_change =
            |input: &Entity<InputState>, cx: &mut Context<Self>, apply: fn(&mut Saved, &str)| {
                cx.subscribe(input, move |this, input, event: &InputEvent, cx| {
                    if matches!(event, InputEvent::Change) {
                        let value = input.read(cx).value().to_string();
                        this.set(cx, |s| apply(s, &value));
                    }
                })
            };
        let updates = hub.read(cx).updates.clone();
        let subscriptions = vec![
            cx.observe(&hub, |_, _, cx| cx.notify()),
            cx.observe(&updates, |_, _, cx| cx.notify()),
            on_change(&fields.spool, cx, |s, v| {
                if let Some(percent) = parse_decimal::<f64>(v.trim().trim_end_matches('%')) {
                    s.settings.preferences.engine.default_spool = (percent / 100.0).clamp(0.0, 1.0);
                }
            }),
            on_change(&fields.search_delay, cx, |s, v| {
                if let Some(ms) = parse_decimal::<u64>(v) {
                    s.settings.preferences.market.search_delay_ms = ms.min(10_000);
                }
            }),
            on_change(&fields.proxy_address, cx, |s, v| {
                v.trim()
                    .clone_into(&mut s.settings.preferences.network.proxy.address);
            }),
            on_change(&fields.proxy_user, cx, |s, v| {
                v.clone_into(&mut s.settings.preferences.network.proxy.username);
            }),
            on_change(&fields.proxy_password, cx, |s, v| {
                v.clone_into(&mut s.settings.preferences.network.proxy.password);
            }),
            on_change(&fields.html_path, cx, |s, v| {
                v.trim().clone_into(&mut s.settings.preferences.html.path);
            }),
            on_change(&fields.esi_client_id, cx, |s, v| {
                v.trim()
                    .clone_into(&mut s.settings.preferences.esi.client_id);
            }),
            on_change(&fields.esi_port, cx, |s, v| {
                if let Some(port) = parse_decimal::<u16>(v)
                    && port > 0
                {
                    s.settings.preferences.esi.port = port;
                }
            }),
        ];
        Self {
            hub,
            page: Page::General,
            fields,
            _subscriptions: subscriptions,
        }
    }

    fn set(&self, cx: &mut Context<Self>, edit: impl FnOnce(&mut Saved)) {
        self.hub.update(cx, |hub, cx| hub.update_saved(edit, cx));
    }
}

impl Render for PreferencesWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        let narrow = viewport.width < px(NARROW_W);
        // A short window scrolls the page's title away with its settings.
        let short = viewport.height < px(SHORT_H);
        let (prefs, factor_reload, totals) = {
            let saved = &self.hub.read(cx).saved;
            (
                saved.settings.preferences.clone(),
                saved.settings.factor_reload,
                saved.settings.price_totals,
            )
        };
        let body = match self.page {
            Page::General => Self::general(&prefs, cx),
            Page::Engine => self.engine(&prefs, factor_reload, cx),
            Page::Stats => Self::stats(&prefs, cx),
            Page::ContextMenus => Self::context_menus(&prefs, cx),
            Page::Market => self.market(&prefs, totals, cx),
            Page::Network => self.network(&prefs, cx),
            Page::Html => self.html(&prefs, cx),
            Page::Esi => self.esi(&prefs, cx),
            Page::Database => self.database(cx),
        };
        let theme = cx.theme();
        let pages = Page::ALL.into_iter().map(|page| {
            let selected = page == self.page;
            h_flex()
                .id(page.title())
                .flex_none()
                .gap_2p5()
                .px_2p5()
                .py_1p5()
                .rounded(px(6.))
                .when(narrow, gpui_kit::Styled::justify_center)
                .when(selected, |d| {
                    d.bg(theme.secondary)
                        .text_color(theme.foreground)
                        .font_weight(FontWeight::MEDIUM)
                })
                .when(!selected, |d| {
                    d.text_color(theme.muted_foreground).hover(|s| {
                        s.bg(theme.secondary.opacity(0.5))
                            .text_color(theme.foreground)
                    })
                })
                .child(
                    UiIcon::new(page.icon())
                        .size(px(16.))
                        .text_color(if selected {
                            theme.primary
                        } else {
                            theme.muted_foreground
                        }),
                )
                .when(!narrow, |d| d.child(page.title()))
                .when(narrow, |d| d.tip(page.title()))
                .on_mouse_down(
                    gpui_kit::MouseButton::Left,
                    cx.listener(move |this, _: &gpui_kit::MouseDownEvent, _, cx| {
                        this.page = page;
                        cx.notify();
                    }),
                )
        });
        let header = h_flex()
            .flex_none()
            .gap_3()
            .when(narrow, |d| d.px_4().pt_4().pb_3())
            .when(!narrow, |d| d.px_8().pt_6().pb_4())
            .child(
                div()
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .size(px(36.))
                    .rounded(px(8.))
                    .bg(theme.primary.opacity(0.15))
                    .child(
                        UiIcon::new(self.page.icon())
                            .size(px(18.))
                            .text_color(theme.primary),
                    ),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .child(
                        div()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.page.title()),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(self.page.summary()),
                    ),
            );
        let (scrolled_header, fixed_header) = if short {
            (Some(header), None)
        } else {
            (None, Some(header))
        };
        h_flex()
            .size_full()
            .items_start()
            .child(
                v_flex()
                    .id("preferences-pages")
                    .flex_none()
                    .w(px(if narrow { PAGES_NARROW_W } else { PAGES_W }))
                    .h_full()
                    .overflow_y_scroll()
                    .px_2()
                    .when(narrow, gpui_kit::Styled::py_2)
                    .when(!narrow, gpui_kit::Styled::py_4)
                    .gap_0p5()
                    .border_r_1()
                    .border_color(theme.border)
                    .bg(theme.sidebar)
                    .when(!narrow, |d| {
                        d.child(
                            div()
                                .px_2p5()
                                .pb_3()
                                .text_xs()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.muted_foreground)
                                .child("PREFERENCES"),
                        )
                    })
                    .children(pages),
            )
            .child(
                v_flex()
                    .flex_1()
                    .h_full()
                    .min_w_0()
                    .children(fixed_header)
                    .child(
                        v_flex()
                            .id("preferences-page")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .children(scrolled_header)
                            .child(v_flex().flex_none().pb_4().gap_3().children(body)),
                    ),
            )
    }
}
