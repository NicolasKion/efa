//! The preference pages, General through ESI, each a list of sections.

use gpui_kit::component::{
    ActiveTheme, Disableable, Sizable,
    button::Button,
    h_flex,
    input::{Input, InputState},
    switch::Switch,
    v_flex,
};
use gpui_kit::{
    AnyElement, Context, Entity, FontWeight, IntoElement, ParentElement, SharedString, Styled, div,
    px,
};

use super::PreferencesWindow;
use super::controls::{Toggle, row, section};
use crate::{
    preferences::{AdditionsLabels, Preferences, ProxyMode},
    prices::{PriceSource, PriceSystem},
    store::PriceTotals,
};

impl PreferencesWindow {
    pub(super) fn general(p: &Preferences, cx: &mut Context<Self>) -> Vec<AnyElement> {
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
                        |s, v| s.settings.preferences.general.color_by_slot = v,
                        cx,
                    ),
                    Self::toggle(
                        "rack-slots",
                        "Separate racks",
                        "Leave a gap between the racks.",
                        g.rack_slots,
                        |s, v| s.settings.preferences.general.rack_slots = v,
                        cx,
                    ),
                    Self::toggle_under(
                        "rack-labels",
                        "Show rack labels",
                        "A header band over each rack with its slot count.",
                        g.rack_labels,
                        Some(g.rack_slots),
                        |s, v| s.settings.preferences.general.rack_labels = v,
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
                        "Bring back the tabs that were open when efa was closed.",
                        g.reopen_fits,
                        |s, v| s.settings.preferences.general.reopen_fits = v,
                        cx,
                    ),
                    Self::toggle(
                        "open-new-tab",
                        "Open fits in a new tab",
                        "Otherwise the fittings browser opens a fit in place of the current tab.",
                        g.open_fit_in_new_tab,
                        |s, v| s.settings.preferences.general.open_fit_in_new_tab = v,
                        cx,
                    ),
                    Self::toggle(
                        "tab-tooltips",
                        "Tab tooltips",
                        "Hovering a fit tab shows the ship and fit name.",
                        g.tab_tooltips,
                        |s, v| s.settings.preferences.general.tab_tooltips = v,
                        cx,
                    ),
                    Self::choice(
                        "additions-labels",
                        "Counts in additions tabs",
                        "What the Drones, Implants and other tab names show in brackets.",
                        &labels,
                        g.additions_labels,
                        |s, v| s.settings.preferences.general.additions_labels = v,
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
                        |s, v| s.settings.preferences.general.character_implants = v,
                        cx,
                    ),
                    Self::toggle(
                        "ammo-all",
                        "Change charges in every module of a type",
                        "Picking a charge loads it into all modules of that type. Ctrl or Alt does \
                         the opposite.",
                        g.ammo_change_all,
                        |s, v| s.settings.preferences.general.ammo_change_all = v,
                        cx,
                    ),
                    Self::toggle(
                        "global-character",
                        "Use global character",
                        "Every fit flies the character picked last, instead of each fit keeping \
                         its own.",
                        g.global_character,
                        |s, v| s.settings.preferences.general.global_character = v,
                        cx,
                    ),
                    Self::toggle(
                        "global-damage-pattern",
                        "Use global damage pattern",
                        "Every fit takes the damage pattern picked last, instead of each fit \
                         keeping its own.",
                        g.global_damage_pattern,
                        |s, v| s.settings.preferences.general.global_damage_pattern = v,
                        cx,
                    ),
                    Self::toggle(
                        "compact-skills",
                        "Compact skills needed tooltip",
                        "List each missing skill once. Off lists the skills under each item \
                         that needs them.",
                        g.compact_skills,
                        |s, v| s.settings.preferences.general.compact_skills = v,
                        cx,
                    ),
                    Self::toggle(
                        "implants-in-cargo",
                        "Implants and boosters take cargo space",
                        "The fit's own implants and boosters count against the cargo hold, as \
                         they travel there. The hold warns when overfull and Fill Cargo leaves \
                         room for them.",
                        g.implants_in_cargo,
                        |s, v| s.settings.preferences.general.implants_in_cargo = v,
                        cx,
                    ),
                    Self::toggle(
                        "expanded-mutants",
                        "Descriptive names for mutated items",
                        "Name them after the mutaplasmid and base item, like \"Unstable 50MN \
                         Microwarpdrive II\", instead of the abyssal type.",
                        g.expanded_mutant_names,
                        |s, v| s.settings.preferences.general.expanded_mutant_names = v,
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
                        |s, v| s.settings.preferences.general.ship_browser_tooltips = v,
                        cx,
                    ),
                    Self::toggle(
                        "omega-icons",
                        "Omega markers",
                        "Mark ships and modules an Alpha clone cannot use.",
                        g.omega_icons,
                        |s, v| s.settings.preferences.general.omega_icons = v,
                        cx,
                    ),
                ],
                cx,
            ),
        ]
        .into_iter()
        .chain(cfg!(target_os = "linux").then(|| {
            section(
                "Window",
                vec![Self::toggle(
                    "window-controls",
                    "Window buttons in the header",
                    "Draw minimize, maximize and close into efa's top bar, as on Windows. Meant \
                     for desktops that draw no title bars of their own.",
                    g.window_controls,
                    |s, v| s.settings.preferences.general.window_controls = v,
                    cx,
                )],
                cx,
            )
        }))
        .collect()
    }

    pub(super) fn engine(
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
                        |s, v| s.settings.factor_reload = v,
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
                    |s, v| s.settings.preferences.engine.strict_skill_levels = v,
                    cx,
                )],
                cx,
            ),
        ]
    }

    pub(super) fn stats(p: &Preferences, cx: &mut Context<Self>) -> Vec<AnyElement> {
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
                        this.set(cx, |s| s.settings.preferences.stats.set(key, value));
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

    pub(super) fn context_menus(p: &Preferences, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let c = &p.context_menus;
        let entries: [Toggle; 8] = [
            (
                "cm-ammo",
                "Set as Damage Pattern",
                "On charges: use the charge's damage as the fit's damage pattern.",
                c.ammo_pattern,
                |s, v| s.settings.preferences.context_menus.ammo_pattern = v,
            ),
            (
                "cm-skills",
                "Change Skills",
                "On items: set the skills that affect them right from the menu.",
                c.change_skills,
                |s, v| s.settings.preferences.context_menus.change_skills = v,
            ),
            (
                "cm-variations",
                "Variations",
                "On modules and drones: swap to a meta, faction or officer version.",
                c.variations,
                |s, v| s.settings.preferences.context_menus.variations = v,
            ),
            (
                "cm-project",
                "Project onto Fit",
                "On market items: add them to the fit's projected effects.",
                c.project,
                |s, v| s.settings.preferences.context_menus.project = v,
            ),
            (
                "cm-fill",
                "Fill with module",
                "On modules: fill every free slot of the rack with copies.",
                c.module_fill,
                |s, v| s.settings.preferences.context_menus.module_fill = v,
            ),
            (
                "cm-spool",
                "Spool-up",
                "On spooling modules: pick how far they are spooled.",
                c.spoolup,
                |s, v| s.settings.preferences.context_menus.spoolup = v,
            ),
            (
                "cm-copy-paste",
                "Copy and paste",
                "In the additions tabs: copy items as text and paste them back.",
                c.additions_copy_paste,
                |s, v| s.settings.preferences.context_menus.additions_copy_paste = v,
            ),
            (
                "cm-cargo-fill",
                "Fill cargo with",
                "In the cargo tab: fill the remaining space with an item.",
                c.cargo_fill,
                |s, v| s.settings.preferences.context_menus.cargo_fill = v,
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

    pub(super) fn market(
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
            "Where item prices come from. Items it has no price for are asked of the others.",
            &sources,
            m.price_source,
            |s, v| s.settings.preferences.market.price_source = v,
            cx,
        )];
        if m.price_source.uses_system() {
            prices.push(Self::choice(
                "price-system",
                "Trade hub",
                "Which market's prices to use.",
                &systems,
                m.price_system,
                |s, v| s.settings.preferences.market.price_system = v,
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
                        |s, v| s.settings.price_totals.drones = v,
                        cx,
                    ),
                    Self::toggle(
                        "total-cargo",
                        "Cargo",
                        "Add the cargo hold to the fit's total.",
                        totals.cargo,
                        |s, v| s.settings.price_totals.cargo = v,
                        cx,
                    ),
                    Self::toggle(
                        "total-character",
                        "Implants and boosters",
                        "Add what the pilot has plugged in to the fit's total.",
                        totals.character,
                        |s, v| s.settings.price_totals.character = v,
                        cx,
                    ),
                ],
                cx,
            ),
        ]
    }

    pub(super) fn network(&self, p: &Preferences, cx: &mut Context<Self>) -> Vec<AnyElement> {
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
                        |s, v| s.settings.preferences.network.enabled = v,
                        cx,
                    ),
                    Self::toggle_under(
                        "network-prices",
                        "Price updates",
                        "Refresh prices in the background.",
                        n.prices,
                        Some(n.enabled),
                        |s, v| s.settings.preferences.network.prices = v,
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
                        "How efa reaches the internet.",
                        &modes,
                        n.proxy.mode,
                        |s, v| s.settings.preferences.network.proxy.mode = v,
                        cx,
                    ),
                    field("Address", &self.fields.proxy_address, cx),
                    field("Username", &self.fields.proxy_user, cx),
                    field("Password", &self.fields.proxy_password, cx),
                ],
                cx,
            ),
            section("Updates", self.update_rows(n, cx), cx),
        ]
    }

    /// The update check switch, and checking by hand with what the last check found.
    fn update_rows(
        &self,
        n: &crate::preferences::Network,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let updates = self.hub.read(cx).updates.clone();
        let (checking, result) = {
            let u = updates.read(cx);
            (u.checking, u.last_result.clone())
        };
        let switch = Switch::new("check-updates")
            .checked(n.check_updates)
            .disabled(!n.enabled)
            .on_click(cx.listener(|this, value: &bool, _, cx| {
                let value = *value;
                this.set(cx, |s| s.settings.preferences.network.check_updates = value);
            }))
            .into_any_element();
        let check = Button::new("check-updates-now")
            .small()
            .label(if checking { "Checking..." } else { "Check Now" })
            .disabled(checking || !n.enabled)
            .on_click(move |_, _, cx| updates.update(cx, |u, cx| u.check(true, cx)))
            .into_any_element();
        let found = result.unwrap_or_else(|| "Not checked yet.".into());
        vec![
            row(
                "Check for updates",
                "Look for a new efa at startup and every hour.",
                switch,
                false,
                !n.enabled,
                cx,
            ),
            h_flex()
                .gap_6()
                .px_4()
                .py_2()
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .gap_0p5()
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .child(format!("efa {}", crate::update::VERSION)),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(cx.theme().muted_foreground)
                                .child(found),
                        ),
                )
                .child(div().flex_none().child(check))
                .into_any_element(),
        ]
    }

    pub(super) fn html(&self, p: &Preferences, cx: &mut Context<Self>) -> Vec<AnyElement> {
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
                    |s, v| s.settings.preferences.html.minimal = v,
                    cx,
                ),
            ],
            cx,
        )]
    }
}

impl PreferencesWindow {
    pub(super) fn esi(&self, p: &Preferences, cx: &mut Context<Self>) -> Vec<AnyElement> {
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
            .py_2()
            .child(div().text_xs().text_color(theme.muted_foreground).child(
                "Only needed for an application of your own: register it at \
                 developers.eveonline.com with this callback URL and these scopes, then paste its \
                 Client ID above.",
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
            .on_click(move |_, _, cx| {
                crate::ui::esi::open_characters_window(hub.clone(), cx).detach();
            })
            .into_any_element();
        vec![
            section(
                "Application",
                vec![
                    row(
                        "Client ID",
                        "Leave empty to use efa's own application. Fill in only to log in \
                         through an application of your own.",
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
                        "The local port EVE sends the browser back to after logging in. efa's \
                         own application is registered for 6461.",
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
