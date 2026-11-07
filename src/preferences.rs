//! pyfa's preferences that make sense for gyfa, with pyfa's defaults. Saved with the fits; every
//! field defaults so older saves load.

use serde::{Deserialize, Serialize};

use crate::prices::{PriceSource, PriceSystem};

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    pub general: General,
    pub engine: Engine,
    pub stats: StatsPanes,
    pub context_menus: ContextMenus,
    pub market: Market,
    pub network: Network,
    pub html: HtmlExport,
    pub esi: Esi,
}

/// pyfa's General page (gui/builtinPreferenceViews/pyfaGeneralPreferences.py).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent switches, one per checkbox"
)]
pub struct General {
    /// New and imported fits without implants of their own use the character's.
    pub character_implants: bool,
    /// Open last session's tabs on startup.
    pub reopen_fits: bool,
    /// Tint fitting view rows by rack.
    pub color_by_slot: bool,
    /// Set the fitting view's racks apart, pyfa's "Separate Racks".
    pub rack_slots: bool,
    /// Name each rack in its separator row, pyfa's "Show Rack Labels".
    pub rack_labels: bool,
    /// Show a fit tab's full name and hull as a tooltip.
    pub tab_tooltips: bool,
    /// The fittings browser opens a fit in a new tab rather than in the current one.
    pub open_fit_in_new_tab: bool,
    /// Changing a module's charge changes it in every module of the same type.
    pub ammo_change_all: bool,
    pub additions_labels: AdditionsLabels,
    /// Hovering a ship or fit in the fittings browser shows the ship's traits.
    pub ship_browser_tooltips: bool,
    /// Mark hulls and items an Alpha clone cannot use.
    pub omega_icons: bool,
    /// Name mutated items after their mutaplasmid and base item rather than the abyssal type.
    pub expanded_mutant_names: bool,
}

impl Default for General {
    fn default() -> Self {
        Self {
            character_implants: true,
            reopen_fits: true,
            color_by_slot: false,
            rack_slots: true,
            rack_labels: true,
            tab_tooltips: true,
            open_fit_in_new_tab: false,
            ammo_change_all: false,
            additions_labels: AdditionsLabels::Active,
            ship_browser_tooltips: true,
            omega_icons: false,
            expanded_mutant_names: false,
        }
    }
}

/// What the additions tab names count, pyfa's "Extra info in Additions panel tab names".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdditionsLabels {
    None,
    /// Active items: launched drones, implants and boosters switched on.
    #[default]
    Active,
    /// Every item, cargo counted by units.
    All,
}

impl AdditionsLabels {
    pub const ALL: [Self; 3] = [Self::None, Self::Active, Self::All];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::Active => "Quantity of active items",
            Self::All => "Quantity of all items",
        }
    }
}

/// pyfa's Fitting Engine page. Reload time lives in `Saved::factor_reload`, which the
/// firepower section's menu shares.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Engine {
    /// Spool-up assumed for modules without their own setting, as a fraction of full spool.
    pub default_spool: f64,
    /// Lowering a skill unlearns the skills that need it higher, pyfa's "Enforce strict skill
    /// level requirements".
    pub strict_skill_levels: bool,
}

impl Default for Engine {
    fn default() -> Self {
        Self {
            default_spool: 1.0,
            strict_skill_levels: true,
        }
    }
}

/// Which stats pane sections show, pyfa's Statistics Panel page. pyfa also has a "minimal"
/// variant per section; gyfa's sections only come in one size, so each is shown or not.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent switches, one per section"
)]
pub struct StatsPanes {
    pub resources: bool,
    pub resistances: bool,
    pub recharge: bool,
    pub firepower: bool,
    pub outgoing: bool,
    pub capacitor: bool,
    pub targeting: bool,
    pub bombing: bool,
    pub price: bool,
}

impl Default for StatsPanes {
    fn default() -> Self {
        Self {
            resources: true,
            resistances: true,
            recharge: true,
            firepower: true,
            outgoing: true,
            capacitor: true,
            targeting: true,
            bombing: false,
            price: true,
        }
    }
}

impl StatsPanes {
    /// Sections by the key the stats pane gives them, with their pyfa label.
    pub fn entries(&self) -> [(&'static str, &'static str, bool); 9] {
        [
            ("Resources", "Resources", self.resources),
            ("Resistances", "Resistances", self.resistances),
            ("Recharge", "Shield/Armor Tank", self.recharge),
            ("Firepower", "Firepower", self.firepower),
            ("Outgoing", "Remote Reps", self.outgoing),
            ("Capacitor", "Capacitor", self.capacitor),
            ("Targeting", "Targeting && Misc", self.targeting),
            ("Bombing", "Bombing", self.bombing),
            ("Price", "Price", self.price),
        ]
    }

    /// Whether the section with this key shows. Sections this does not know show.
    pub fn shows(&self, key: &str) -> bool {
        self.entries()
            .iter()
            .find(|(k, ..)| *k == key)
            .is_none_or(|&(.., on)| on)
    }

    pub fn set(&mut self, key: &str, on: bool) {
        let field = match key {
            "Resources" => &mut self.resources,
            "Resistances" => &mut self.resistances,
            "Recharge" => &mut self.recharge,
            "Firepower" => &mut self.firepower,
            "Outgoing" => &mut self.outgoing,
            "Capacitor" => &mut self.capacitor,
            "Targeting" => &mut self.targeting,
            "Bombing" => &mut self.bombing,
            "Price" => &mut self.price,
            _ => return,
        };
        *field = on;
    }
}

/// Context menu entries that can be hidden, pyfa's Context Menus page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent switches, one per menu entry"
)]
pub struct ContextMenus {
    /// "Set as Damage Pattern" on charges.
    pub ammo_pattern: bool,
    /// "Change Skills" on items.
    pub change_skills: bool,
    /// "Variations" submenus.
    pub variations: bool,
    /// "Project onto Fit".
    pub project: bool,
    /// "Fill With Module".
    pub module_fill: bool,
    /// "Spoolup" on spooling modules.
    pub spoolup: bool,
    /// Copy and paste entries of the additions panes.
    pub additions_copy_paste: bool,
    /// "Fill Cargo With".
    pub cargo_fill: bool,
}

impl Default for ContextMenus {
    fn default() -> Self {
        Self {
            ammo_pattern: true,
            change_skills: true,
            variations: true,
            project: true,
            module_fill: true,
            spoolup: true,
            additions_copy_paste: true,
            cargo_fill: true,
        }
    }
}

/// pyfa's Market & Prices page. What the price total includes lives in `Saved::price_totals`,
/// which the price section's menu shares.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Market {
    /// Wait this long after the last keystroke before searching the market.
    pub search_delay_ms: u64,
    pub price_source: PriceSource,
    pub price_system: PriceSystem,
}

impl Default for Market {
    fn default() -> Self {
        Self {
            search_delay_ms: 250,
            price_source: PriceSource::default(),
            price_system: PriceSystem::default(),
        }
    }
}

/// pyfa's Network page, without the parts for its own update check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Network {
    /// Master switch: off means no price or ESI requests at all.
    pub enabled: bool,
    pub prices: bool,
    pub proxy: Proxy,
}

impl Default for Network {
    fn default() -> Self {
        Self {
            enabled: true,
            prices: true,
            proxy: Proxy::default(),
        }
    }
}

/// How requests reach the network. The system setting is whatever the environment's
/// `HTTPS_PROXY` and friends say, which reqwest honours by itself.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Proxy {
    pub mode: ProxyMode,
    /// host:port for a manual proxy.
    pub address: String,
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProxyMode {
    None,
    #[default]
    System,
    Manual,
}

impl ProxyMode {
    pub const ALL: [Self; 3] = [Self::None, Self::System, Self::Manual];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "No proxy",
            Self::System => "System proxy settings",
            Self::Manual => "Manual proxy settings",
        }
    }
}

impl Proxy {
    /// A blocking client that goes through this proxy.
    pub fn client(&self, agent: &str) -> Result<reqwest::blocking::Client, String> {
        let mut builder = reqwest::blocking::Client::builder().user_agent(agent);
        match self.mode {
            ProxyMode::System => {}
            ProxyMode::None => builder = builder.no_proxy(),
            ProxyMode::Manual => {
                let address = self.address.trim();
                let url = if address.contains("://") {
                    address.to_owned()
                } else {
                    format!("http://{address}")
                };
                let mut proxy = reqwest::Proxy::all(&url).map_err(|e| format!("proxy: {e}"))?;
                if !self.username.is_empty() {
                    proxy = proxy.basic_auth(&self.username, &self.password);
                }
                builder = builder.proxy(proxy);
            }
        }
        builder.build().map_err(|e| e.to_string())
    }
}

/// pyfa's HTML Export page.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HtmlExport {
    /// Where "Export All Fittings to HTML" writes; empty means the data directory.
    pub path: String,
    /// Plain links only, without styling or the EFT text.
    pub minimal: bool,
}

/// EVE SSO and ESI, pyfa's ESI page. gyfa ships no client ID of its own: the user registers an
/// application at developers.eveonline.com and pastes its ID here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Esi {
    pub client_id: String,
    /// The local port EVE SSO sends the browser back to.
    pub port: u16,
    /// What Export to EVE sends along with the fit.
    pub upload: crate::port::esi::UploadOptions,
}

impl Default for Esi {
    fn default() -> Self {
        Self {
            client_id: String::new(),
            port: crate::esi::DEFAULT_PORT,
            upload: crate::port::esi::UploadOptions::default(),
        }
    }
}
