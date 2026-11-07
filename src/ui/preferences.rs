//! pyfa's preferences dialog (gui/preferenceDialog.py, gui/builtinPreferenceViews/) as a window
//! of its own. Every change goes straight to the hub, which saves it and hands it to every fit
//! window, the way pyfa applies a preference as soon as it is clicked.
//!
//! Left out on purpose: language and theme (gyfa follows the system and Omarchy's theme),
//! global character and damage pattern, compact skills tooltip, gauge animation, Omega icons,
//! market shortcuts and market view modes, reactivation timers, update checks, ESI and SSO,
//! database paths and logging.

use gpui_kit::component::{
    ActiveTheme, Disableable, Sizable,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    h_flex,
    input::{Input, InputEvent, InputState},
    radio::Radio,
    v_flex,
};
use gpui_kit::{
    AnyElement, AppContext, ClickEvent, Context, Entity, FontWeight, InteractiveElement,
    IntoElement, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    Subscription, Window, WindowBounds, WindowOptions, div, prelude::FluentBuilder, px, size,
};

use super::{Gyfa, Hub, port::html_path};
use crate::{
    preferences::{AdditionsLabels, Preferences, ProxyMode},
    prices::{PriceSource, PriceSystem},
    store::{PriceTotals, Saved},
};

/// A checkbox: id, label, whether it is on, and how to set it.
type Toggle = (&'static str, &'static str, bool, fn(&mut Saved, bool));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    General,
    Engine,
    Stats,
    ContextMenus,
    Market,
    Network,
    Html,
}

impl Page {
    const ALL: [Self; 7] = [
        Self::General,
        Self::Engine,
        Self::Stats,
        Self::ContextMenus,
        Self::Market,
        Self::Network,
        Self::Html,
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
            window_bounds: Some(WindowBounds::centered(size(px(760.), px(560.)), cx)),
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

    /// A checkbox bound to one setting, with pyfa's wording and an optional note under it.
    fn check(
        id: &'static str,
        label: &'static str,
        note: Option<&'static str>,
        checked: bool,
        apply: fn(&mut Saved, bool),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        v_flex()
            .gap_0p5()
            .child(
                Checkbox::new(id)
                    .label(label)
                    .checked(checked)
                    .on_click(cx.listener(move |this, value: &bool, _, cx| {
                        let value = *value;
                        this.set(cx, |s| apply(s, value));
                    })),
            )
            .when_some(note, |col, note| {
                col.child(div().pl_6().text_xs().text_color(muted).child(note))
            })
            .into_any_element()
    }

    /// A group of radio buttons choosing one of `options`.
    fn choice<T: Copy + PartialEq + 'static>(
        id: &'static str,
        title: &'static str,
        options: &[(T, &'static str)],
        current: T,
        apply: fn(&mut Saved, T),
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let radios: Vec<AnyElement> = options
            .iter()
            .enumerate()
            .map(|(n, &(value, label))| {
                Radio::new(SharedString::from(format!("{id}-{n}")))
                    .label(label)
                    .checked(current == value)
                    .on_click(cx.listener(move |this, _: &bool, _, cx| {
                        this.set(cx, |s| apply(s, value));
                    }))
                    .into_any_element()
            })
            .collect();
        v_flex()
            .gap_1()
            .child(subheading(title))
            .child(v_flex().pl_2().gap_1().children(radios))
            .into_any_element()
    }

    fn general(p: &Preferences, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let g = &p.general;
        let labels: Vec<(AdditionsLabels, &str)> = AdditionsLabels::ALL
            .iter()
            .map(|&l| (l, l.label()))
            .collect();
        vec![
            Self::check(
                "character-implants",
                "Use character implants by default for new fits",
                Some("Applies to new and imported fits that have no implants of their own."),
                g.character_implants,
                |s, v| s.preferences.general.character_implants = v,
                cx,
            ),
            Self::check(
                "color-slots",
                "Color fitting view by slot",
                None,
                g.color_by_slot,
                |s, v| s.preferences.general.color_by_slot = v,
                cx,
            ),
            Self::check(
                "reopen-fits",
                "Reopen previous fits on startup",
                None,
                g.reopen_fits,
                |s, v| s.preferences.general.reopen_fits = v,
                cx,
            ),
            Self::check(
                "tab-tooltips",
                "Show fitting tab tooltips",
                None,
                g.tab_tooltips,
                |s, v| s.preferences.general.tab_tooltips = v,
                cx,
            ),
            Self::check(
                "ship-browser-tooltips",
                "Show ship browser tooltip",
                None,
                g.ship_browser_tooltips,
                |s, v| s.preferences.general.ship_browser_tooltips = v,
                cx,
            ),
            Self::check(
                "omega-icons",
                "Show Omega icons for hulls and modules",
                None,
                g.omega_icons,
                |s, v| s.preferences.general.omega_icons = v,
                cx,
            ),
            Self::check(
                "open-new-tab",
                "Open fittings in a new page by default",
                Some("Otherwise the fittings browser opens a fit in place of the current tab."),
                g.open_fit_in_new_tab,
                |s, v| s.preferences.general.open_fit_in_new_tab = v,
                cx,
            ),
            Self::check(
                "ammo-all",
                "Change charge in all modules of the same type",
                Some("Ctrl or Alt while picking a charge does the opposite."),
                g.ammo_change_all,
                |s, v| s.preferences.general.ammo_change_all = v,
                cx,
            ),
            Self::check(
                "expanded-mutants",
                "Include more information in names of mutated items",
                Some("Use short mutaplasmid name and base item name instead of actual item name."),
                g.expanded_mutant_names,
                |s, v| s.preferences.general.expanded_mutant_names = v,
                cx,
            ),
            Self::choice(
                "additions-labels",
                "Extra info in Additions panel tab names",
                &labels,
                g.additions_labels,
                |s, v| s.preferences.general.additions_labels = v,
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
            Self::check(
                "factor-reload",
                "Factor in reload time when calculating capacitor usage, damage, and tank.",
                None,
                factor_reload,
                |s, v| s.factor_reload = v,
                cx,
            ),
            Self::check(
                "strict-skill-levels",
                "Enforce strict skill level requirements",
                Some(
                    "Lowering a skill resets the skills that need it higher. Setting Drones from V to IV resets Heavy Drone Operation, which needs Drones V.",
                ),
                prefs.engine.strict_skill_levels,
                |s, v| s.preferences.engine.strict_skill_levels = v,
                cx,
            ),
            labelled(
                "Global Default Spoolup Percentage:",
                Input::new(&self.fields.spool).small().w(px(80.)),
                Some("For modules that spool up and have no spoolup set of their own."),
                cx,
            ),
        ]
    }

    fn stats(p: &Preferences, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut out = vec![
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child("Which sections the stats pane shows. Changes apply right away.")
                .into_any_element(),
        ];
        for (key, label, on) in p.stats.entries() {
            out.push(
                Checkbox::new(SharedString::from(format!("stats-{key}")))
                    .label(label.replace("&&", "&"))
                    .checked(on)
                    .on_click(cx.listener(move |this, value: &bool, _, cx| {
                        let value = *value;
                        this.set(cx, |s| s.preferences.stats.set(key, value));
                    }))
                    .into_any_element(),
            );
        }
        out
    }

    fn context_menus(p: &Preferences, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let c = &p.context_menus;
        let mut out = vec![
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child("Hide menu entries you do not use.")
                .into_any_element(),
        ];
        let entries: [Toggle; 8] = [
            (
                "cm-ammo",
                "Set as Damage Pattern",
                c.ammo_pattern,
                |s, v| {
                    s.preferences.context_menus.ammo_pattern = v;
                },
            ),
            ("cm-skills", "Change Skills", c.change_skills, |s, v| {
                s.preferences.context_menus.change_skills = v;
            }),
            ("cm-variations", "Variations", c.variations, |s, v| {
                s.preferences.context_menus.variations = v;
            }),
            ("cm-project", "Project onto Fit", c.project, |s, v| {
                s.preferences.context_menus.project = v;
            }),
            ("cm-fill", "Fill with module", c.module_fill, |s, v| {
                s.preferences.context_menus.module_fill = v;
            }),
            ("cm-spool", "Spoolup", c.spoolup, |s, v| {
                s.preferences.context_menus.spoolup = v;
            }),
            (
                "cm-copy-paste",
                "Additions Panel Copy/Paste",
                c.additions_copy_paste,
                |s, v| {
                    s.preferences.context_menus.additions_copy_paste = v;
                },
            ),
            ("cm-cargo-fill", "Fill cargo with", c.cargo_fill, |s, v| {
                s.preferences.context_menus.cargo_fill = v;
            }),
        ];
        for (id, label, on, apply) in entries {
            out.push(Self::check(id, label, None, on, apply, cx));
        }
        out
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
        let mut out = vec![
            labelled(
                "Market Search Delay (ms):",
                Input::new(&self.fields.search_delay).small().w(px(80.)),
                None,
                cx,
            ),
            Self::choice(
                "price-source",
                "Default Market Prices",
                &sources,
                m.price_source,
                |s, v| s.preferences.market.price_source = v,
                cx,
            ),
        ];
        if m.price_source.uses_system() {
            out.push(Self::choice(
                "price-system",
                "Market",
                &systems,
                m.price_system,
                |s, v| s.preferences.market.price_system = v,
                cx,
            ));
        }
        out.push(subheading("Total Price Includes"));
        out.push(Self::check(
            "total-drones",
            "Drones",
            None,
            totals.drones,
            |s, v| s.price_totals.drones = v,
            cx,
        ));
        out.push(Self::check(
            "total-cargo",
            "Cargo",
            None,
            totals.cargo,
            |s, v| s.price_totals.cargo = v,
            cx,
        ));
        out.push(Self::check(
            "total-character",
            "Implants & Boosters",
            None,
            totals.character,
            |s, v| s.price_totals.character = v,
            cx,
        ));
        out
    }

    fn network(&self, p: &Preferences, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let n = &p.network;
        let modes: Vec<(ProxyMode, &str)> =
            ProxyMode::ALL.iter().map(|&m| (m, m.label())).collect();
        let manual = n.proxy.mode == ProxyMode::Manual;
        vec![
            Self::check(
                "network",
                "Enable Network",
                Some("Off stops price updates. Item icons still download when missing."),
                n.enabled,
                |s, v| s.preferences.network.enabled = v,
                cx,
            ),
            div()
                .pl_4()
                .child(
                    Checkbox::new("network-prices")
                        .label("Pricing updates")
                        .checked(n.prices)
                        .disabled(!n.enabled)
                        .on_click(cx.listener(|this, value: &bool, _, cx| {
                            let value = *value;
                            this.set(cx, |s| s.preferences.network.prices = value);
                        })),
                )
                .into_any_element(),
            Self::choice(
                "proxy-mode",
                "Proxy settings",
                &modes,
                n.proxy.mode,
                |s, v| s.preferences.network.proxy.mode = v,
                cx,
            ),
            v_flex()
                .pl_2()
                .gap_1()
                .when(!manual, |col| col.opacity(0.5))
                .child(labelled(
                    "Addr:",
                    Input::new(&self.fields.proxy_address)
                        .small()
                        .w(px(220.))
                        .disabled(!manual),
                    None,
                    cx,
                ))
                .child(labelled(
                    "Username:",
                    Input::new(&self.fields.proxy_user)
                        .small()
                        .w(px(220.))
                        .disabled(!manual),
                    None,
                    cx,
                ))
                .child(labelled(
                    "Password:",
                    Input::new(&self.fields.proxy_password)
                        .small()
                        .w(px(220.))
                        .disabled(!manual),
                    None,
                    cx,
                ))
                .into_any_element(),
        ]
    }

    fn html(&self, p: &Preferences, cx: &mut Context<Self>) -> Vec<AnyElement> {
        vec![
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(
                    "File > Export All Fittings to HTML writes every fit to this file, with links \
                     that open them in a browser fitting tool.",
                )
                .into_any_element(),
            labelled(
                "Export path:",
                Input::new(&self.fields.html_path).small().w(px(380.)),
                Some("Leave empty for the file shown in grey."),
                cx,
            ),
            Self::check(
                "html-minimal",
                "Enable minimal format",
                Some("Only the links, without styling or the EFT text."),
                p.html.minimal,
                |s, v| s.preferences.html.minimal = v,
                cx,
            ),
        ]
    }
}

fn subheading(text: &'static str) -> AnyElement {
    div()
        .pt_2()
        .font_weight(FontWeight::SEMIBOLD)
        .child(text)
        .into_any_element()
}

fn labelled(
    label: &'static str,
    field: impl IntoElement,
    note: Option<&'static str>,
    cx: &Context<PreferencesWindow>,
) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    v_flex()
        .gap_0p5()
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(div().min_w(px(110.)).child(label))
                .child(field),
        )
        .when_some(note, |col, note| {
            col.child(div().text_xs().text_color(muted).child(note))
        })
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
        };
        let theme = cx.theme();
        let pages = Page::ALL.into_iter().map(|page| {
            let selected = page == self.page;
            div()
                .id(page.title())
                .px_3()
                .py_1()
                .rounded(px(4.))
                .when(selected, |d| {
                    d.bg(theme.secondary).text_color(theme.foreground)
                })
                .when(!selected, |d| {
                    d.text_color(theme.muted_foreground)
                        .hover(|s| s.bg(theme.secondary.opacity(0.5)))
                })
                .child(page.title())
                .on_click(cx.listener(move |this, _: &ClickEvent, _, cx| {
                    this.page = page;
                    cx.notify();
                }))
        });
        let close = Button::new("preferences-close")
            .small()
            .primary()
            .label("Close")
            .on_click(|_, window, _| window.remove_window());
        h_flex()
            .size_full()
            .items_start()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_sm()
            .child(
                v_flex()
                    .w(px(180.))
                    .h_full()
                    .p_2()
                    .gap_0p5()
                    .border_r_1()
                    .border_color(theme.border)
                    .bg(theme.sidebar)
                    .children(pages),
            )
            .child(
                v_flex()
                    .flex_1()
                    .h_full()
                    .min_w_0()
                    .child(
                        div()
                            .px_4()
                            .pt_3()
                            .pb_2()
                            .text_lg()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(self.page.title()),
                    )
                    .child(
                        v_flex()
                            .id("preferences-page")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .px_4()
                            .pb_4()
                            .gap_2()
                            .children(body),
                    )
                    .child(
                        h_flex()
                            .justify_end()
                            .p_3()
                            .border_t_1()
                            .border_color(theme.border)
                            .child(close),
                    ),
            )
    }
}
