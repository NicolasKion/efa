//! pyfa's preferences dialog (gui/preferenceDialog.py, gui/builtinPreferenceViews/) as a window
//! of its own. Every change goes straight to the hub, which saves it and hands it to every fit
//! window, the way pyfa applies a preference as soon as it is clicked.
//!
//! Left out on purpose: language and theme (gyfa follows the system and Omarchy's theme),
//! global character and damage pattern, compact skills tooltip, gauge animation, Omega icons,
//! market shortcuts and market view modes, reactivation timers, update checks, pyfa's own SSO
//! server mode (gyfa always logs in through a local callback),
//! database paths and logging.

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon as UiIcon, Sizable,
    button::Button,
    h_flex,
    input::{Input, InputEvent, InputState},
    switch::Switch,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext, ClickEvent, Context, Entity, FontWeight, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Window, WindowBounds, WindowOptions, div, prelude::FluentBuilder, px, size,
};

use super::{Gyfa, Hub, port::html_path};
use crate::{
    preferences::{AdditionsLabels, Preferences, ProxyMode},
    prices::{PriceSource, PriceSystem},
    store::{PriceTotals, Saved},
};

/// A switch: id, title, description, whether it is on, and how to set it.
type Toggle = (
    &'static str,
    &'static str,
    &'static str,
    bool,
    fn(&mut Saved, bool),
);

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
}

impl Page {
    const ALL: [Self; 8] = [
        Self::General,
        Self::Engine,
        Self::Stats,
        Self::ContextMenus,
        Self::Market,
        Self::Network,
        Self::Html,
        Self::Esi,
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

impl Gyfa {
    /// pyfa's Global > Preferences.
    pub(super) fn open_preferences(&mut self, cx: &mut Context<Self>) {
        let hub = self.hub.clone();
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(880.), px(640.)), cx)),
            app_id: Some("gyfa".into()),
            ..Default::default()
        };
        let opened = gpui_kit::open_window(options, cx, move |window, cx| {
            window.set_window_title("gyfa: Preferences");
            cx.new(|cx| PreferencesWindow::new(hub, window, cx))
        });
        if let Err(e) = opened {
            self.status = Some(format!("Could not open a window: {e}").into());
            cx.notify();
        }
    }
}

impl PreferencesWindow {
    fn new(hub: Entity<Hub>, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let prefs = hub.read(cx).saved.preferences.clone();
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
            esi_client_id: field(prefs.esi.client_id.clone(), "Client ID", false),
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
        let subscriptions = vec![
            cx.observe(&hub, |_, _, cx| cx.notify()),
            on_change(&fields.spool, cx, |s, v| {
                if let Ok(percent) = v.trim().trim_end_matches('%').parse::<f64>() {
                    s.preferences.engine.default_spool = (percent / 100.0).clamp(0.0, 1.0);
                }
            }),
            on_change(&fields.search_delay, cx, |s, v| {
                if let Ok(ms) = v.trim().parse::<u64>() {
                    s.preferences.market.search_delay_ms = ms.min(10_000);
                }
            }),
            on_change(&fields.proxy_address, cx, |s, v| {
                v.trim()
                    .clone_into(&mut s.preferences.network.proxy.address);
            }),
            on_change(&fields.proxy_user, cx, |s, v| {
                v.clone_into(&mut s.preferences.network.proxy.username);
            }),
            on_change(&fields.proxy_password, cx, |s, v| {
                v.clone_into(&mut s.preferences.network.proxy.password);
            }),
            on_change(&fields.html_path, cx, |s, v| {
                v.trim().clone_into(&mut s.preferences.html.path);
            }),
            on_change(&fields.esi_client_id, cx, |s, v| {
                v.trim().clone_into(&mut s.preferences.esi.client_id);
            }),
            on_change(&fields.esi_port, cx, |s, v| {
                if let Ok(port) = v.trim().parse::<u16>()
                    && port > 0
                {
                    s.preferences.esi.port = port;
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

    /// A switch bound to one setting.
    fn toggle(
        id: &'static str,
        title: &'static str,
        description: &'static str,
        checked: bool,
        apply: fn(&mut Saved, bool),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        Self::toggle_under(id, title, description, checked, None, apply, cx)
    }

    /// A switch that depends on another setting: indented under it, and off limits while
    /// `enabled` is false. None for a switch that stands on its own.
    fn toggle_under(
        id: &'static str,
        title: &'static str,
        description: &'static str,
        checked: bool,
        enabled: Option<bool>,
        apply: fn(&mut Saved, bool),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let disabled = enabled == Some(false);
        let control = Switch::new(id)
            .checked(checked)
            .disabled(disabled)
            .on_click(cx.listener(move |this, value: &bool, _, cx| {
                let value = *value;
                this.set(cx, |s| apply(s, value));
            }))
            .into_any_element();
        row(title, description, control, enabled.is_some(), disabled, cx)
    }

    /// One of a few values, as a segmented control under the setting's text.
    fn choice<T: Copy + PartialEq + 'static>(
        id: &'static str,
        title: &'static str,
        description: &'static str,
        options: &[(T, &'static str)],
        current: T,
        apply: fn(&mut Saved, T),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let segments: Vec<AnyElement> = options
            .iter()
            .enumerate()
            .map(|(n, &(value, label))| {
                let selected = current == value;
                div()
                    .id(SharedString::from(format!("{id}-{n}")))
                    .px_3()
                    .py_1()
                    .rounded(px(5.))
                    .text_xs()
                    .when(selected, |d| {
                        d.bg(theme.primary)
                            .text_color(theme.primary_foreground)
                            .font_weight(FontWeight::MEDIUM)
                    })
                    .when(!selected, |d| {
                        d.text_color(theme.muted_foreground)
                            .hover(|s| s.bg(theme.secondary).text_color(theme.foreground))
                    })
                    .child(label)
                    .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                        this.set(cx, |s| apply(s, value));
                    }))
                    .into_any_element()
            })
            .collect();
        let control = h_flex()
            .flex_wrap()
            .gap_0p5()
            .p_0p5()
            .rounded(px(7.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.background)
            .children(segments);
        v_flex()
            .gap_2()
            .px_4()
            .py_3()
            .child(text(title, description, false, cx))
            .child(h_flex().child(control))
            .into_any_element()
    }

    fn general(p: &Preferences, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let g = &p.general;
        let labels: Vec<(AdditionsLabels, &str)> = AdditionsLabels::ALL
            .iter()
            .map(|&l| (l, l.label()))
            .collect();
        vec![
            section(
                "Fitting view",
                vec![
                    Self::toggle(
                        "color-slots",
                        "Color by slot",
                        "Tint high, mid, low and rig rows so the racks are easy to tell apart.",
                        g.color_by_slot,
                        |s, v| s.preferences.general.color_by_slot = v,
                        cx,
                    ),
                    Self::toggle(
                        "rack-slots",
                        "Separate racks",
                        "Leave a gap between the racks.",
                        g.rack_slots,
                        |s, v| s.preferences.general.rack_slots = v,
                        cx,
                    ),
                    Self::toggle_under(
                        "rack-labels",
                        "Show rack labels",
                        "A header band over each rack with its slot count.",
                        g.rack_labels,
                        Some(g.rack_slots),
                        |s, v| s.preferences.general.rack_labels = v,
                        cx,
                    ),
                ],
                cx,
            ),
            section(
                "Fits and tabs",
                vec![
                    Self::toggle(
                        "reopen-fits",
                        "Reopen fits on startup",
                        "Bring back the tabs that were open when gyfa was closed.",
                        g.reopen_fits,
                        |s, v| s.preferences.general.reopen_fits = v,
                        cx,
                    ),
                    Self::toggle(
                        "open-new-tab",
                        "Open fits in a new tab",
                        "Otherwise the fittings browser opens a fit in place of the current tab.",
                        g.open_fit_in_new_tab,
                        |s, v| s.preferences.general.open_fit_in_new_tab = v,
                        cx,
                    ),
                    Self::toggle(
                        "tab-tooltips",
                        "Tab tooltips",
                        "Hovering a fit tab shows the ship and fit name.",
                        g.tab_tooltips,
                        |s, v| s.preferences.general.tab_tooltips = v,
                        cx,
                    ),
                    Self::choice(
                        "additions-labels",
                        "Counts in additions tabs",
                        "What the Drones, Implants and other tab names show in brackets.",
                        &labels,
                        g.additions_labels,
                        |s, v| s.preferences.general.additions_labels = v,
                        cx,
                    ),
                ],
                cx,
            ),
            section(
                "Fitting",
                vec![
                    Self::toggle(
                        "character-implants",
                        "Character implants for new fits",
                        "New and imported fits without implants of their own use the character's.",
                        g.character_implants,
                        |s, v| s.preferences.general.character_implants = v,
                        cx,
                    ),
                    Self::toggle(
                        "ammo-all",
                        "Change charges in every module of a type",
                        "Picking a charge loads it into all modules of that type. Ctrl or Alt does \
                         the opposite.",
                        g.ammo_change_all,
                        |s, v| s.preferences.general.ammo_change_all = v,
                        cx,
                    ),
                    Self::toggle(
                        "implants-in-cargo",
                        "Implants and boosters take cargo space",
                        "The fit's own implants and boosters count against the cargo hold, as \
                         they travel there. The hold warns when overfull and Fill Cargo leaves \
                         room for them.",
                        g.implants_in_cargo,
                        |s, v| s.preferences.general.implants_in_cargo = v,
                        cx,
                    ),
                    Self::toggle(
                        "expanded-mutants",
                        "Descriptive names for mutated items",
                        "Name them after the mutaplasmid and base item, like \"Unstable 50MN \
                         Microwarpdrive II\", instead of the abyssal type.",
                        g.expanded_mutant_names,
                        |s, v| s.preferences.general.expanded_mutant_names = v,
                        cx,
                    ),
                ],
                cx,
            ),
            section(
                "Browsers",
                vec![
                    Self::toggle(
                        "ship-browser-tooltips",
                        "Ship tooltips",
                        "Hovering a ship or fit in the fittings browser shows the ship's traits.",
                        g.ship_browser_tooltips,
                        |s, v| s.preferences.general.ship_browser_tooltips = v,
                        cx,
                    ),
                    Self::toggle(
                        "omega-icons",
                        "Omega markers",
                        "Mark ships and modules an Alpha clone cannot use.",
                        g.omega_icons,
                        |s, v| s.preferences.general.omega_icons = v,
                        cx,
                    ),
                ],
                cx,
            ),
        ]
    }

    fn engine(
        &self,
        prefs: &Preferences,
        factor_reload: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        vec![
            section(
                "Calculations",
                vec![
                    Self::toggle(
                        "factor-reload",
                        "Factor in reload time",
                        "Count the time spent reloading in capacitor use, damage and tank.",
                        factor_reload,
                        |s, v| s.factor_reload = v,
                        cx,
                    ),
                    row(
                        "Default spool-up",
                        "Percent spooled for modules that spool up and have no setting of their \
                         own.",
                        Input::new(&self.fields.spool)
                            .small()
                            .suffix("%")
                            .w(px(90.))
                            .into_any_element(),
                        false,
                        false,
                        cx,
                    ),
                ],
                cx,
            ),
            section(
                "Skills",
                vec![Self::toggle(
                    "strict-skill-levels",
                    "Strict skill requirements",
                    "Lowering a skill also lowers the skills that need it. Setting Drones to IV \
                     resets Heavy Drone Operation, which needs Drones V.",
                    prefs.engine.strict_skill_levels,
                    |s, v| s.preferences.engine.strict_skill_levels = v,
                    cx,
                )],
                cx,
            ),
        ]
    }

    fn stats(p: &Preferences, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let rows = p
            .stats
            .entries()
            .into_iter()
            .map(|(key, label, on)| {
                let description = match key {
                    "Resources" => "Hardpoints, CPU, powergrid, calibration and the bays.",
                    "Resistances" => "Resist profile and effective HP per layer.",
                    "Recharge" => "Passive regen and repair rates.",
                    "Firepower" => "Weapon and drone DPS and volley.",
                    "Outgoing" => "Remote repairs and capacitor transfer.",
                    "Capacitor" => "Capacity, stability and how long it lasts.",
                    "Targeting" => "Targeting, speed, signature and cargo.",
                    "Bombing" => "Bombs needed to kill the fit.",
                    "Price" => "What the fit costs.",
                    _ => "",
                };
                let control = Switch::new(SharedString::from(format!("stats-{key}")))
                    .checked(on)
                    .on_click(cx.listener(move |this, value: &bool, _, cx| {
                        let value = *value;
                        this.set(cx, |s| s.preferences.stats.set(key, value));
                    }))
                    .into_any_element();
                row(
                    label.replace("&&", "&"),
                    description,
                    control,
                    false,
                    false,
                    cx,
                )
            })
            .collect();
        vec![section("Sections", rows, cx)]
    }

    fn context_menus(p: &Preferences, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let c = &p.context_menus;
        let entries: [Toggle; 8] = [
            (
                "cm-ammo",
                "Set as Damage Pattern",
                "On charges: use the charge's damage as the fit's damage pattern.",
                c.ammo_pattern,
                |s, v| s.preferences.context_menus.ammo_pattern = v,
            ),
            (
                "cm-skills",
                "Change Skills",
                "On items: set the skills that affect them right from the menu.",
                c.change_skills,
                |s, v| s.preferences.context_menus.change_skills = v,
            ),
            (
                "cm-variations",
                "Variations",
                "On modules and drones: swap to a meta, faction or officer version.",
                c.variations,
                |s, v| s.preferences.context_menus.variations = v,
            ),
            (
                "cm-project",
                "Project onto Fit",
                "On market items: add them to the fit's projected effects.",
                c.project,
                |s, v| s.preferences.context_menus.project = v,
            ),
            (
                "cm-fill",
                "Fill with module",
                "On modules: fill every free slot of the rack with copies.",
                c.module_fill,
                |s, v| s.preferences.context_menus.module_fill = v,
            ),
            (
                "cm-spool",
                "Spool-up",
                "On spooling modules: pick how far they are spooled.",
                c.spoolup,
                |s, v| s.preferences.context_menus.spoolup = v,
            ),
            (
                "cm-copy-paste",
                "Copy and paste",
                "In the additions tabs: copy items as text and paste them back.",
                c.additions_copy_paste,
                |s, v| s.preferences.context_menus.additions_copy_paste = v,
            ),
            (
                "cm-cargo-fill",
                "Fill cargo with",
                "In the cargo tab: fill the remaining space with an item.",
                c.cargo_fill,
                |s, v| s.preferences.context_menus.cargo_fill = v,
            ),
        ];
        let rows = entries
            .into_iter()
            .map(|(id, title, description, on, apply)| {
                Self::toggle(id, title, description, on, apply, cx)
            })
            .collect();
        vec![section("Menu entries", rows, cx)]
    }

    fn market(
        &self,
        p: &Preferences,
        totals: PriceTotals,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let m = &p.market;
        let sources: Vec<(PriceSource, &str)> =
            PriceSource::ALL.iter().map(|&s| (s, s.label())).collect();
        let systems: Vec<(PriceSystem, &str)> =
            PriceSystem::ALL.iter().map(|&s| (s, s.label())).collect();
        let mut prices = vec![Self::choice(
            "price-source",
            "Price source",
            "Where item prices come from.",
            &sources,
            m.price_source,
            |s, v| s.preferences.market.price_source = v,
            cx,
        )];
        if m.price_source.uses_system() {
            prices.push(Self::choice(
                "price-system",
                "Trade hub",
                "Which market's prices to use.",
                &systems,
                m.price_system,
                |s, v| s.preferences.market.price_system = v,
                cx,
            ));
        }
        vec![
            section(
                "Search",
                vec![row(
                    "Search delay",
                    "How long to wait after the last keystroke before searching, in \
                     milliseconds.",
                    Input::new(&self.fields.search_delay)
                        .small()
                        .suffix("ms")
                        .w(px(100.))
                        .into_any_element(),
                    false,
                    false,
                    cx,
                )],
                cx,
            ),
            section("Prices", prices, cx),
            section(
                "Total price includes",
                vec![
                    Self::toggle(
                        "total-drones",
                        "Drones",
                        "Add the drone bay to the fit's total.",
                        totals.drones,
                        |s, v| s.price_totals.drones = v,
                        cx,
                    ),
                    Self::toggle(
                        "total-cargo",
                        "Cargo",
                        "Add the cargo hold to the fit's total.",
                        totals.cargo,
                        |s, v| s.price_totals.cargo = v,
                        cx,
                    ),
                    Self::toggle(
                        "total-character",
                        "Implants and boosters",
                        "Add what the pilot has plugged in to the fit's total.",
                        totals.character,
                        |s, v| s.price_totals.character = v,
                        cx,
                    ),
                ],
                cx,
            ),
        ]
    }

    fn network(&self, p: &Preferences, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let n = &p.network;
        let modes: Vec<(ProxyMode, &str)> =
            ProxyMode::ALL.iter().map(|&m| (m, m.label())).collect();
        let manual = n.proxy.mode == ProxyMode::Manual;
        let field = |title: &'static str, input: &Entity<InputState>, cx: &Context<Self>| {
            row(
                title,
                "",
                Input::new(input)
                    .small()
                    .w(px(240.))
                    .disabled(!manual)
                    .into_any_element(),
                true,
                !manual,
                cx,
            )
        };
        vec![
            section(
                "Connection",
                vec![
                    Self::toggle(
                        "network",
                        "Enable network",
                        "Off stops price updates and ESI. Item icons still download when missing.",
                        n.enabled,
                        |s, v| s.preferences.network.enabled = v,
                        cx,
                    ),
                    Self::toggle_under(
                        "network-prices",
                        "Price updates",
                        "Refresh prices in the background.",
                        n.prices,
                        Some(n.enabled),
                        |s, v| s.preferences.network.prices = v,
                        cx,
                    ),
                ],
                cx,
            ),
            section(
                "Proxy",
                vec![
                    Self::choice(
                        "proxy-mode",
                        "Proxy",
                        "How gyfa reaches the internet.",
                        &modes,
                        n.proxy.mode,
                        |s, v| s.preferences.network.proxy.mode = v,
                        cx,
                    ),
                    field("Address", &self.fields.proxy_address, cx),
                    field("Username", &self.fields.proxy_user, cx),
                    field("Password", &self.fields.proxy_password, cx),
                ],
                cx,
            ),
        ]
    }

    fn html(&self, p: &Preferences, cx: &mut Context<Self>) -> Vec<AnyElement> {
        vec![section(
            "Export",
            vec![
                row(
                    "Export file",
                    "File > Export All Fittings to HTML writes every fit here. Leave it empty for \
                     the file shown in grey.",
                    Input::new(&self.fields.html_path)
                        .small()
                        .w(px(300.))
                        .into_any_element(),
                    false,
                    false,
                    cx,
                ),
                Self::toggle(
                    "html-minimal",
                    "Minimal format",
                    "Only the links, without styling or the EFT text.",
                    p.html.minimal,
                    |s, v| s.preferences.html.minimal = v,
                    cx,
                ),
            ],
            cx,
        )]
    }
}

impl PreferencesWindow {
    fn esi(&self, p: &Preferences, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let url = crate::esi::callback_url(p.esi.port);
        let logged_in = self.hub.read(cx).esi.accounts.accounts.len();
        let theme = cx.theme();
        let mono = |text: String| {
            div()
                .font_family("monospace")
                .text_xs()
                .child(text)
                .into_any_element()
        };
        let callback = h_flex()
            .gap_2()
            .child(mono(url.clone()))
            .child(
                gpui_kit::component::clipboard::Clipboard::new("esi-callback-copy")
                    .value(url)
                    .tooltip("Copy the callback URL"),
            )
            .into_any_element();
        let register = v_flex()
            .gap_2()
            .px_4()
            .py_3()
            .child(div().text_xs().text_color(theme.muted_foreground).child(
                "Register an app at developers.eveonline.com with this callback URL and \
                         these scopes, then paste its Client ID above:",
            ))
            .child(
                v_flex()
                    .gap_0p5()
                    .children(crate::esi::SCOPES.map(|s| mono(s.to_owned()))),
            )
            .into_any_element();
        let hub = self.hub.clone();
        let manage = Button::new("esi-manage")
            .small()
            .label("Manage…")
            .on_click(move |_, _, cx| super::esi::open_characters_window(hub.clone(), cx))
            .into_any_element();
        vec![
            section(
                "Application",
                vec![
                    row(
                        "Client ID",
                        "The ID of your application at developers.eveonline.com.",
                        Input::new(&self.fields.esi_client_id)
                            .small()
                            .w(px(300.))
                            .into_any_element(),
                        false,
                        false,
                        cx,
                    ),
                    row(
                        "Callback port",
                        "The local port EVE sends the browser back to after logging in.",
                        Input::new(&self.fields.esi_port)
                            .small()
                            .w(px(90.))
                            .into_any_element(),
                        false,
                        false,
                        cx,
                    ),
                    row("Callback URL", "", callback, false, false, cx),
                    register,
                ],
                cx,
            ),
            section(
                "Characters",
                vec![row(
                    match logged_in {
                        0 => "No characters logged in".to_owned(),
                        1 => "1 character logged in".to_owned(),
                        n => format!("{n} characters logged in"),
                    },
                    "Add or remove them in Character > Manage ESI Characters.",
                    manage,
                    false,
                    false,
                    cx,
                )],
                cx,
            ),
        ]
    }
}

/// A setting's title with its description under it.
fn text(
    title: impl Into<SharedString>,
    description: &'static str,
    disabled: bool,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    v_flex()
        .flex_1()
        .min_w_0()
        .gap_0p5()
        .when(disabled, |d| d.opacity(0.5))
        .child(div().font_weight(FontWeight::MEDIUM).child(title.into()))
        .when(!description.is_empty(), |d| {
            d.child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(description),
            )
        })
        .into_any_element()
}

/// A setting: its text on the left, its control on the right. `nested` indents it under the
/// setting before it.
fn row(
    title: impl Into<SharedString>,
    description: &'static str,
    control: AnyElement,
    nested: bool,
    disabled: bool,
    cx: &App,
) -> AnyElement {
    h_flex()
        .gap_6()
        .px_4()
        .py_3()
        .when(nested, Styled::pl_10)
        .child(text(title, description, disabled, cx))
        .child(div().flex_none().child(control))
        .into_any_element()
}

/// A titled card of settings, divided by hairlines.
pub(super) fn section(title: &'static str, rows: Vec<AnyElement>, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let mut card = v_flex()
        .rounded(px(8.))
        .border_1()
        .border_color(theme.border)
        .bg(theme.sidebar)
        .overflow_hidden();
    for (n, row) in rows.into_iter().enumerate() {
        if n > 0 {
            card = card.child(div().h_px().mx_4().bg(theme.border));
        }
        card = card.child(row);
    }
    v_flex()
        .gap_2()
        .child(
            div()
                .px_1()
                .text_xs()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.muted_foreground)
                .child(title.to_uppercase()),
        )
        .child(card)
        .into_any_element()
}

impl Render for PreferencesWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let (prefs, factor_reload, totals) = {
            let saved = &self.hub.read(cx).saved;
            (
                saved.preferences.clone(),
                saved.factor_reload,
                saved.price_totals,
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
        };
        let theme = cx.theme();
        let pages = Page::ALL.into_iter().map(|page| {
            let selected = page == self.page;
            h_flex()
                .id(page.title())
                .gap_2p5()
                .px_2p5()
                .py_1p5()
                .rounded(px(6.))
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
                .child(page.title())
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.page = page;
                    cx.notify();
                }))
        });
        let header = h_flex()
            .gap_3()
            .px_8()
            .pt_6()
            .pb_4()
            .child(
                div()
                    .flex()
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
        h_flex()
            .size_full()
            .items_start()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_sm()
            .child(
                v_flex()
                    .w(px(200.))
                    .h_full()
                    .px_2()
                    .py_4()
                    .gap_0p5()
                    .border_r_1()
                    .border_color(theme.border)
                    .bg(theme.sidebar)
                    .child(
                        div()
                            .px_2p5()
                            .pb_3()
                            .text_xs()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.muted_foreground)
                            .child("PREFERENCES"),
                    )
                    .children(pages),
            )
            .child(
                v_flex().flex_1().h_full().min_w_0().child(header).child(
                    v_flex()
                        .id("preferences-page")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .px_8()
                        .pb_8()
                        .gap_6()
                        .children(body),
                ),
            )
    }
}
