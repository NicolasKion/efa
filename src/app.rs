//! The main window, laid out like pyfa: browsers on the left, the fit in the middle with its
//! additions underneath, stats on the right.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
};

use eframe::egui::{self, Color32, RichText, Sense, vec2};
use egui_extras::{Column, TableBuilder, TableRow};

use crate::{
    data::{Catalog, Kind, Slot, TypeId},
    engine::{Cap, Engine, Layer, ModuleInfo, Resource, Slots, Stats},
    fit::{Fit, Module, ModuleState},
    icons::{Icon, Icons},
    theme::{self, Palette},
};

const STORAGE_KEY: &str = "gyfa";
const ROW_HEIGHT: f32 = 24.0;

/// Icon IDs the game uses for these attributes, which label the matching stats.
mod icon {
    pub const CPU: i32 = 1405;
    pub const POWERGRID: i32 = 70;
    pub const CALIBRATION: i32 = 2887;
    pub const TURRET: i32 = 387;
    pub const LAUNCHER: i32 = 168;
    pub const DRONE_BANDWIDTH: i32 = 2987;
    pub const DRONE_BAY: i32 = 1084;
    pub const SHIELD: i32 = 1384;
    pub const ARMOR: i32 = 1383;
    pub const HULL: i32 = 67;
    pub const EM: i32 = 1396;
    pub const THERMAL: i32 = 1394;
    pub const KINETIC: i32 = 1393;
    pub const EXPLOSIVE: i32 = 1395;
    pub const CAPACITOR: i32 = 1668;
    pub const RECHARGE: i32 = 1392;
    pub const RANGE: i32 = 1391;
    pub const DAMAGE: i32 = 1432;
    pub const HIGH: i32 = 293;
    pub const MID: i32 = 294;
    pub const LOW: i32 = 295;
    pub const RIG: i32 = 3266;
    pub const SUBSYSTEM: i32 = 3756;
}

/// EM, thermal, kinetic, explosive.
const DAMAGE_COLORS: [Color32; 4] = [
    Color32::from_rgb(52, 120, 190),
    Color32::from_rgb(180, 50, 45),
    Color32::from_rgb(120, 120, 120),
    Color32::from_rgb(190, 130, 30),
];

struct Loaded {
    catalog: Catalog,
    engine: Engine,
    icons: Icons,
}

enum Loading {
    Pending(Receiver<Result<Loaded, String>>),
    Ready(Box<Loaded>),
    Failed(String),
}

#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct Saved {
    fits: Vec<Fit>,
    /// Fits open as tabs, by index into `fits`.
    open: Vec<usize>,
    current: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserTab {
    Market,
    Fittings,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AdditionsTab {
    Drones,
    Implants,
    Boosters,
    Cargo,
    Notes,
}

/// A change a row asked for, applied after drawing so the fit is not borrowed twice.
#[derive(Debug, Clone, Copy)]
enum Action {
    CycleState(Slot, usize),
    SetState(Slot, usize, ModuleState),
    SetCharge(Slot, usize, Option<TypeId>),
    RemoveModule(Slot, usize),
    RemoveRig(usize),
    RemoveSubsystem(usize),
    SetDronesActive(usize, u32),
    RemoveDrones(usize),
    RemoveImplant(usize),
    RemoveBooster(usize),
    RemoveCargo(usize),
}

/// Everything read-only a frame needs.
#[derive(Clone, Copy)]
struct Cx<'frame> {
    catalog: &'frame Catalog,
    icons: &'frame Icons,
    palette: Palette,
}

impl Cx<'_> {
    /// Draw an icon, or reserve its space while it is still being fetched.
    fn image(&self, ui: &mut egui::Ui, icon: Icon, size: f32) -> egui::Response {
        match self.icons.get(self.catalog, icon) {
            Some(uri) => ui.add(egui::Image::new(uri).fit_to_exact_size(vec2(size, size))),
            None => ui.allocate_exact_size(vec2(size, size), Sense::hover()).1,
        }
    }
}

pub struct App {
    loading: Loading,
    saved: Saved,
    stats: Option<Stats>,
    dirty: bool,
    browser_tab: BrowserTab,
    additions_tab: AdditionsTab,
    search: String,
    import: Option<ImportDialog>,
    /// Published charges that fit each module type, filled as modules show up.
    charges_for: HashMap<TypeId, Vec<TypeId>>,
    status: Option<String>,
    /// EFT text from the command line, imported once the data is loaded.
    pending_import: Option<String>,
    palette: Palette,
    theme_changes: Receiver<Palette>,
}

#[derive(Debug, Default)]
struct ImportDialog {
    text: String,
    error: Option<String>,
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        data_dir: PathBuf,
        import: Option<String>,
    ) -> Self {
        egui_extras::install_image_loaders(&cc.egui_ctx);
        let palette = Palette::current().unwrap_or_default();
        palette.apply(&cc.egui_ctx);
        let mut saved: Saved = cc
            .storage
            .and_then(|s| eframe::get_value(s, STORAGE_KEY))
            .unwrap_or_default();
        saved.open.retain(|&i| i < saved.fits.len());
        saved.current = saved.current.filter(|c| saved.open.contains(c));

        let (tx, rx) = mpsc::channel();
        let ctx = cc.egui_ctx.clone();
        std::thread::spawn(move || {
            let result = load(&data_dir, &ctx);
            let _ = tx.send(result);
            ctx.request_repaint();
        });

        Self {
            loading: Loading::Pending(rx),
            saved,
            stats: None,
            dirty: true,
            browser_tab: BrowserTab::Market,
            additions_tab: AdditionsTab::Drones,
            search: String::new(),
            import: None,
            charges_for: HashMap::new(),
            status: None,
            pending_import: import,
            palette,
            theme_changes: theme::watch(cc.egui_ctx.clone()),
        }
    }

    fn fit(&self) -> Option<&Fit> {
        self.saved.current.map(|i| &self.saved.fits[i])
    }

    fn fit_mut(&mut self) -> Option<&mut Fit> {
        self.dirty = true;
        self.saved.current.map(|i| &mut self.saved.fits[i])
    }

    fn open_fit(&mut self, index: usize) {
        if !self.saved.open.contains(&index) {
            self.saved.open.push(index);
        }
        self.saved.current = Some(index);
        self.dirty = true;
    }

    fn add_fit(&mut self, fit: Fit) {
        self.saved.fits.push(fit);
        self.open_fit(self.saved.fits.len() - 1);
    }

    fn close_fit(&mut self, index: usize) {
        self.saved.open.retain(|&i| i != index);
        if self.saved.current == Some(index) {
            self.saved.current = self.saved.open.last().copied();
        }
        self.dirty = true;
    }

    fn delete_fit(&mut self, index: usize) {
        self.close_fit(index);
        self.saved.fits.remove(index);
        let shift = |i: usize| if i > index { i - 1 } else { i };
        self.saved.open = self.saved.open.iter().map(|&i| shift(i)).collect();
        self.saved.current = self.saved.current.map(shift);
        self.dirty = true;
    }

    fn import(&mut self, catalog: &Catalog, text: &str) -> Result<(), String> {
        let (fit, unknown) = Fit::from_eft(catalog, text)?;
        self.status =
            (!unknown.is_empty()).then(|| format!("Skipped unknown items: {}", unknown.join(", ")));
        self.add_fit(fit);
        Ok(())
    }
}

fn load(data_dir: &Path, ctx: &egui::Context) -> Result<Loaded, String> {
    if !data_dir.join("phobos/metadata.json").exists() {
        return Err(format!(
            "No EVE data in {}.\nRun scripts/fetch-data.sh, or point GYFA_DATA at a deimos dump.",
            data_dir.display()
        ));
    }
    let catalog = Catalog::load(data_dir)?;
    let cache_dir = data_dir.parent().unwrap_or(data_dir).join("cache");
    let engine = Engine::load(data_dir, &cache_dir, catalog.skills.clone())?;
    let icons = Icons::new(data_dir, &cache_dir, ctx);
    Ok(Loaded {
        catalog,
        engine,
        icons,
    })
}

impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, STORAGE_KEY, &self.saved);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        while let Ok(palette) = self.theme_changes.try_recv() {
            palette.apply(ui.ctx());
            self.palette = palette;
        }
        if let Loading::Pending(rx) = &self.loading
            && let Ok(result) = rx.try_recv()
        {
            self.loading = match result {
                Ok(loaded) => Loading::Ready(Box::new(loaded)),
                Err(e) => Loading::Failed(e),
            };
        }
        // Taken out of self for the frame so the panels can borrow self mutably alongside it.
        let loaded = match std::mem::replace(&mut self.loading, Loading::Failed(String::new())) {
            Loading::Ready(loaded) => loaded,
            other => {
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.centered_and_justified(|ui| match &other {
                        Loading::Failed(e) => {
                            ui.label(RichText::new(e).color(self.palette.red));
                        }
                        Loading::Pending(_) | Loading::Ready(_) => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label("Loading EVE data…");
                            });
                        }
                    });
                });
                self.loading = other;
                return;
            }
        };
        let cx = Cx {
            catalog: &loaded.catalog,
            icons: &loaded.icons,
            palette: self.palette,
        };
        self.frame(ui, cx, &loaded.engine);
        self.loading = Loading::Ready(loaded);
    }
}

impl App {
    fn frame(&mut self, ui: &mut egui::Ui, cx: Cx<'_>, engine: &Engine) {
        if let Some(text) = self.pending_import.take()
            && let Err(e) = self.import(cx.catalog, &text)
        {
            self.status = Some(format!("Import failed: {e}"));
        }
        if self.dirty {
            self.stats = self.fit().map(|fit| engine.compute(fit));
            self.dirty = false;
        }

        self.menu_bar(ui, cx);
        self.import_dialog(ui.ctx(), cx.catalog);

        egui::Panel::left("browser")
            .default_size(280.0)
            .show(ui, |ui| self.browser(ui, cx));
        egui::Panel::right("stats")
            .default_size(330.0)
            .show(ui, |ui| self.stats_panel(ui, cx));
        egui::CentralPanel::default().show(ui, |ui| {
            self.fit_tabs(ui, cx);
            if self.fit().is_none() {
                ui.centered_and_justified(|ui| {
                    ui.weak("Double-click a ship in the market to start a fit, or import one from the File menu.")
                });
                return;
            }
            egui::Panel::bottom("additions")
                .resizable(true)
                .default_size(180.0)
                .show(ui, |ui| self.additions(ui, cx));
            egui::CentralPanel::default().show(ui, |ui| self.fitting_view(ui, cx));
        });
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui, cx: Cx<'_>) {
        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Import EFT…").clicked() {
                        self.import = Some(ImportDialog::default());
                    }
                    let copy =
                        ui.add_enabled(self.fit().is_some(), egui::Button::new("Copy as EFT"));
                    if copy.clicked()
                        && let Some(fit) = self.fit()
                    {
                        ui.ctx().copy_text(fit.to_eft(cx.catalog));
                        self.status = Some("Copied fit to clipboard".into());
                    }
                    ui.separator();
                    if ui.button("Quit").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
                ui.menu_button("Fit", |ui| {
                    if ui.button("New fit").clicked() {
                        self.add_fit(Fit::new("New fit"));
                    }
                    if let Some(current) = self.saved.current {
                        if ui.button("Duplicate").clicked() {
                            let mut copy = self.saved.fits[current].clone();
                            copy.name = format!("{} copy", copy.name);
                            self.add_fit(copy);
                        }
                        if ui.button("Delete").clicked() {
                            self.delete_fit(current);
                        }
                    }
                });
                ui.menu_button("Character", |ui| {
                    ui.add_enabled(
                        false,
                        egui::Button::new("All 5 (the only character for now)"),
                    );
                });
                if let Some(status) = &self.status {
                    ui.separator();
                    ui.label(status);
                }
            });
        });
    }

    fn import_dialog(&mut self, ctx: &egui::Context, catalog: &Catalog) {
        let Some(dialog) = &mut self.import else {
            return;
        };
        let mut open = true;
        let mut submitted = false;
        egui::Window::new("Import EFT")
            .open(&mut open)
            .default_width(500.0)
            .show(ctx, |ui| {
                ui.label("Paste a fit in EFT format:");
                egui::ScrollArea::vertical()
                    .max_height(400.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut dialog.text)
                                .code_editor()
                                .desired_rows(16)
                                .desired_width(f32::INFINITY),
                        );
                    });
                if let Some(e) = &dialog.error {
                    ui.colored_label(ui.visuals().error_fg_color, e);
                }
                submitted = ui.button("Import").clicked();
            });
        if submitted {
            let text = std::mem::take(&mut dialog.text);
            if let Err(e) = self.import(catalog, &text) {
                // Put the text back so the user can fix it.
                if let Some(dialog) = &mut self.import {
                    dialog.text = text;
                    dialog.error = Some(e);
                }
                return;
            }
            self.import = None;
        } else if !open {
            self.import = None;
        }
    }

    // Left: market and fittings browsers

    fn browser(&mut self, ui: &mut egui::Ui, cx: Cx<'_>) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.browser_tab, BrowserTab::Market, "Market");
            ui.selectable_value(&mut self.browser_tab, BrowserTab::Fittings, "Fittings");
        });
        ui.separator();
        match self.browser_tab {
            BrowserTab::Market => self.market(ui, cx),
            BrowserTab::Fittings => self.fittings(ui, cx),
        }
    }

    fn market(&mut self, ui: &mut egui::Ui, cx: Cx<'_>) {
        ui.add(
            egui::TextEdit::singleline(&mut self.search)
                .hint_text("Search items")
                .desired_width(f32::INFINITY),
        );
        ui.add_space(4.0);
        let mut add = None;
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                if self.search.trim().len() >= 2 {
                    for id in cx.catalog.search(&self.search, 300) {
                        if item_row(ui, cx, id) {
                            add = Some(id);
                        }
                    }
                } else {
                    for &root in &cx.catalog.market_roots {
                        market_group(ui, cx, root, &mut add);
                    }
                }
            });
        if let Some(id) = add {
            self.add_item(cx.catalog, id);
        }
    }

    fn add_item(&mut self, catalog: &Catalog, id: TypeId) {
        let is_ship = catalog.get(id).is_some_and(|t| t.kind == Kind::Ship);
        let has_ship = self.fit().is_some_and(|f| f.ship.is_some());
        // A ship starts a new fit rather than swapping the hull under an existing one.
        if is_ship && (self.fit().is_none() || has_ship) {
            let mut fit = Fit::new(catalog.name(id));
            fit.ship = Some(id);
            self.add_fit(fit);
            self.status = None;
            return;
        }
        let Some(fit) = self.fit_mut() else {
            self.status = Some("Pick a ship first".into());
            return;
        };
        let added = fit.add(catalog, id);
        self.status = (!added).then(|| format!("Nothing on this fit takes {}", catalog.name(id)));
    }

    fn fittings(&mut self, ui: &mut egui::Ui, cx: Cx<'_>) {
        let mut by_ship: Vec<(String, usize)> = self
            .saved
            .fits
            .iter()
            .enumerate()
            .map(|(i, f)| (f.ship.map_or_else(String::new, |s| cx.catalog.name(s)), i))
            .collect();
        by_ship.sort();
        let mut open = None;
        let mut delete = None;
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                if by_ship.is_empty() {
                    ui.weak("No fits yet.");
                }
                for (ship_name, i) in by_ship {
                    let fit = &self.saved.fits[i];
                    let row = ui
                        .horizontal(|ui| {
                            match fit.ship {
                                Some(ship) => cx.image(ui, Icon::Type(ship), 32.0),
                                None => ui.allocate_exact_size(vec2(32.0, 32.0), Sense::hover()).1,
                            };
                            ui.vertical(|ui| {
                                ui.label(&fit.name);
                                ui.weak(ship_name);
                            });
                        })
                        .response
                        .interact(Sense::click());
                    if row.clicked() {
                        open = Some(i);
                    }
                    row.context_menu(|ui| {
                        if ui.button("Open").clicked() {
                            open = Some(i);
                        }
                        if ui.button("Delete").clicked() {
                            delete = Some(i);
                        }
                    });
                }
            });
        if let Some(i) = open {
            self.open_fit(i);
        }
        if let Some(i) = delete {
            self.delete_fit(i);
        }
    }

    // Middle: tabs, fitting view, additions

    fn fit_tabs(&mut self, ui: &mut egui::Ui, cx: Cx<'_>) {
        let mut select = None;
        let mut close = None;
        let mut new = false;
        ui.horizontal_wrapped(|ui| {
            for &i in &self.saved.open {
                let fit = &self.saved.fits[i];
                let selected = self.saved.current == Some(i);
                let fill = if selected {
                    ui.visuals().selection.bg_fill
                } else {
                    ui.visuals().faint_bg_color
                };
                egui::Frame::new()
                    .fill(fill)
                    .corner_radius(4.0)
                    .inner_margin(egui::Margin::symmetric(6, 2))
                    .show(ui, |ui| {
                        if let Some(ship) = fit.ship {
                            cx.image(ui, Icon::Type(ship), 16.0);
                        }
                        let label = match fit.ship {
                            Some(ship) => format!("{}: {}", cx.catalog.name(ship), fit.name),
                            None => fit.name.clone(),
                        };
                        if ui
                            .add(egui::Label::new(label).sense(Sense::click()))
                            .clicked()
                        {
                            select = Some(i);
                        }
                        if ui.small_button("×").on_hover_text("Close tab").clicked() {
                            close = Some(i);
                        }
                    });
            }
            new = ui.button("+").on_hover_text("New fit").clicked();
        });
        if let Some(i) = select {
            self.saved.current = Some(i);
            self.dirty = true;
        }
        if let Some(i) = close {
            self.close_fit(i);
        }
        if new {
            self.add_fit(Fit::new("New fit"));
        }
        ui.separator();
    }

    fn fitting_view(&mut self, ui: &mut egui::Ui, cx: Cx<'_>) {
        let Some(current) = self.saved.current else {
            return;
        };
        let Some(ship) = self.saved.fits[current].ship else {
            ui.weak("Double-click a ship in the market to put it under this fit.");
            return;
        };

        ui.horizontal(|ui| {
            cx.image(ui, Icon::Type(ship), 32.0);
            ui.heading(cx.catalog.name(ship));
            ui.add(
                egui::TextEdit::singleline(&mut self.saved.fits[current].name).desired_width(240.0),
            );
        });
        ui.add_space(4.0);

        let fit = &self.saved.fits[current];
        for m in fit.high.iter().chain(&fit.mid).chain(&fit.low) {
            self.charges_for
                .entry(m.type_id)
                .or_insert_with(|| compatible_charges(cx.catalog, m.type_id));
        }
        let charges_for = &self.charges_for;
        let no_stats = Stats::default();
        let stats = self.stats.as_ref().unwrap_or(&no_stats);
        let rows = fitting_rows(fit, stats);

        let mut actions = Vec::new();
        TableBuilder::new(ui)
            .striped(true)
            .sense(Sense::click())
            .cell_layout(egui::Layout::left_to_right(egui::Align::Center))
            .column(Column::exact(22.0))
            .column(Column::exact(26.0))
            .column(Column::remainder().at_least(140.0).clip(true))
            .column(Column::exact(52.0))
            .column(Column::exact(52.0))
            .column(Column::exact(64.0))
            .column(Column::exact(104.0))
            .column(Column::initial(170.0).at_least(60.0).clip(true))
            .header(22.0, |mut header| {
                header.col(|_| {});
                header.col(|_| {});
                header.col(|ui| {
                    ui.strong("Name");
                });
                for (icon_id, tip) in [
                    (icon::POWERGRID, "Powergrid"),
                    (icon::CPU, "CPU"),
                    (icon::CAPACITOR, "Capacitor use"),
                    (icon::RANGE, "Optimal + falloff"),
                ] {
                    header.col(|ui| {
                        cx.image(ui, Icon::Id(icon_id), 18.0).on_hover_text(tip);
                    });
                }
                header.col(|ui| {
                    ui.strong("Charge");
                });
            })
            .body(|mut body| {
                for &kind in &rows {
                    body.row(ROW_HEIGHT, |mut row| match kind {
                        Row::Header {
                            label,
                            icon_id,
                            total,
                        } => {
                            row.col(|_| {});
                            row.col(|ui| {
                                cx.image(ui, Icon::Id(icon_id), 18.0);
                            });
                            row.col(|ui| {
                                ui.strong(format!("— {total} {label} —"));
                            });
                            blank_cells(&mut row, 5);
                        }
                        Row::Module(slot, i) => {
                            let m = &fit.rack(slot)[i];
                            let info = stats.modules.get(&(slot, i)).copied().unwrap_or_default();
                            module_cells(&mut row, cx, (slot, i), m, info, &mut actions);
                            let response = row.response();
                            if response.double_clicked() {
                                actions.push(Action::RemoveModule(slot, i));
                            }
                            let charges =
                                charges_for.get(&m.type_id).map_or(&[][..], Vec::as_slice);
                            module_menu(&response, cx, (slot, i), m, charges, &mut actions);
                        }
                        Row::Empty(slot) => {
                            blank_cells(&mut row, 2);
                            row.col(|ui| {
                                ui.weak(format!("[Empty {} slot]", slot_name(slot)));
                            });
                            blank_cells(&mut row, 5);
                        }
                        Row::Rig(i) | Row::Subsystem(i) => {
                            let (id, remove) = match kind {
                                Row::Rig(_) => (fit.rigs[i], Action::RemoveRig(i)),
                                _ => (fit.subsystems[i], Action::RemoveSubsystem(i)),
                            };
                            row.col(|_| {});
                            row.col(|ui| {
                                cx.image(ui, Icon::Type(id), 22.0);
                            });
                            row.col(|ui| {
                                ui.label(cx.catalog.name(id));
                            });
                            blank_cells(&mut row, 5);
                            removable(&row.response(), remove, &mut actions);
                        }
                    });
                }
            });

        for p in &stats.problems {
            ui.colored_label(cx.palette.red, p);
        }
        for action in actions {
            self.apply(cx.catalog, action);
        }
    }

    fn additions(&mut self, ui: &mut egui::Ui, cx: Cx<'_>) {
        let Some(current) = self.saved.current else {
            return;
        };
        let fit = &self.saved.fits[current];
        ui.horizontal(|ui| {
            for (tab, label, count) in [
                (AdditionsTab::Drones, "Drones", fit.drones.len()),
                (AdditionsTab::Implants, "Implants", fit.implants.len()),
                (AdditionsTab::Boosters, "Boosters", fit.boosters.len()),
                (AdditionsTab::Cargo, "Cargo", fit.cargo.len()),
                (AdditionsTab::Notes, "Notes", 0),
            ] {
                let text = if count > 0 {
                    format!("{label} ({count})")
                } else {
                    label.to_owned()
                };
                ui.selectable_value(&mut self.additions_tab, tab, text);
            }
        });
        ui.separator();

        if self.additions_tab == AdditionsTab::Notes {
            // Notes do not affect any stat, so editing them in place needs no recalculation.
            ui.add(
                egui::TextEdit::multiline(&mut self.saved.fits[current].notes)
                    .desired_width(f32::INFINITY)
                    .desired_rows(8),
            );
            return;
        }

        let mut actions = Vec::new();
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| match self.additions_tab {
                AdditionsTab::Drones => {
                    if fit.drones.is_empty() {
                        ui.weak("Double-click drones in the market to add them.");
                    }
                    for (i, d) in fit.drones.iter().enumerate() {
                        let row = ui.horizontal(|ui| {
                            let mut active = d.active;
                            let launched = ui
                                .add(
                                    egui::DragValue::new(&mut active)
                                        .range(0..=d.count)
                                        .suffix(format!(" / {}", d.count)),
                                )
                                .on_hover_text("Drones in space");
                            if launched.changed() {
                                actions.push(Action::SetDronesActive(i, active));
                            }
                            cx.image(ui, Icon::Type(d.type_id), 22.0);
                            ui.add(
                                egui::Label::new(cx.catalog.name(d.type_id)).sense(Sense::click()),
                            )
                        });
                        removable(&row.inner, Action::RemoveDrones(i), &mut actions);
                    }
                }
                AdditionsTab::Implants => {
                    for (i, &id) in fit.implants.iter().enumerate() {
                        removable(
                            &simple_row(ui, cx, id, None),
                            Action::RemoveImplant(i),
                            &mut actions,
                        );
                    }
                }
                AdditionsTab::Boosters => {
                    for (i, &id) in fit.boosters.iter().enumerate() {
                        removable(
                            &simple_row(ui, cx, id, None),
                            Action::RemoveBooster(i),
                            &mut actions,
                        );
                    }
                }
                AdditionsTab::Cargo => {
                    for (i, &(id, count)) in fit.cargo.iter().enumerate() {
                        removable(
                            &simple_row(ui, cx, id, Some(count)),
                            Action::RemoveCargo(i),
                            &mut actions,
                        );
                    }
                }
                AdditionsTab::Notes => {}
            });
        for action in actions {
            self.apply(cx.catalog, action);
        }
    }

    fn apply(&mut self, catalog: &Catalog, action: Action) {
        let Some(fit) = self.fit_mut() else { return };
        match action {
            Action::CycleState(slot, i) => {
                let m = &mut fit.rack_mut(slot)[i];
                let t = catalog.get(m.type_id);
                let activatable = t.is_some_and(|t| t.activatable);
                let overloadable = t.is_some_and(|t| t.overloadable);
                m.state = match m.state {
                    ModuleState::Offline => ModuleState::Online,
                    ModuleState::Online if activatable => ModuleState::Active,
                    ModuleState::Active if overloadable => ModuleState::Overload,
                    _ => ModuleState::Offline,
                };
            }
            Action::SetState(slot, i, state) => fit.rack_mut(slot)[i].state = state,
            Action::SetCharge(slot, i, charge) => fit.rack_mut(slot)[i].charge = charge,
            Action::RemoveModule(slot, i) => {
                fit.rack_mut(slot).remove(i);
            }
            Action::RemoveRig(i) => {
                fit.rigs.remove(i);
            }
            Action::RemoveSubsystem(i) => {
                fit.subsystems.remove(i);
            }
            Action::SetDronesActive(i, n) => fit.drones[i].active = n,
            Action::RemoveDrones(i) => {
                fit.drones.remove(i);
            }
            Action::RemoveImplant(i) => {
                fit.implants.remove(i);
            }
            Action::RemoveBooster(i) => {
                fit.boosters.remove(i);
            }
            Action::RemoveCargo(i) => {
                fit.cargo.remove(i);
            }
        }
    }

    // Right: stats

    fn stats_panel(&self, ui: &mut egui::Ui, cx: Cx<'_>) {
        ui.horizontal(|ui| {
            ui.label("Character:");
            egui::ComboBox::from_id_salt("character")
                .selected_text("All 5")
                .show_ui(ui, |ui| {
                    let _ = ui.selectable_label(true, "All 5");
                });
        });
        ui.separator();
        let Some(s) = &self.stats else { return };
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                section(ui, "Resources", |ui| resources(ui, cx, s));
                let ehp: f64 = [s.shield, s.armor, s.hull]
                    .iter()
                    .filter_map(|l| l.ehp)
                    .sum();
                section(
                    ui,
                    &format!("Resistances (Effective HP: {})", short(ehp)),
                    |ui| {
                        resistances(ui, cx, s);
                    },
                );
                section(ui, "Firepower", |ui| firepower(ui, cx, s));
                section(ui, "Capacitor", |ui| capacitor(ui, cx, s));
                section(ui, "Targeting & Misc", |ui| targeting(ui, s));
            });
    }
}

// Fitting view pieces

#[derive(Debug, Clone, Copy)]
enum Row {
    Header {
        label: &'static str,
        icon_id: i32,
        total: u32,
    },
    Module(Slot, usize),
    Empty(Slot),
    Rig(usize),
    Subsystem(usize),
}

/// The fitting view's rows: each rack under its header, padded with empty slots to what the ship
/// has.
fn fitting_rows(fit: &Fit, stats: &Stats) -> Vec<Row> {
    let mut rows = Vec::new();
    let racks = [
        (
            Slot::High,
            "High Slots",
            icon::HIGH,
            stats.high,
            fit.high.len(),
        ),
        (Slot::Mid, "Med Slots", icon::MID, stats.mid, fit.mid.len()),
        (Slot::Low, "Low Slots", icon::LOW, stats.low, fit.low.len()),
        (Slot::Rig, "Rig Slots", icon::RIG, stats.rig, fit.rigs.len()),
        (
            Slot::Subsystem,
            "Subsystem Slots",
            icon::SUBSYSTEM,
            stats.subsystem,
            fit.subsystems.len(),
        ),
    ];
    for (slot, label, icon_id, slots, used) in racks {
        let total = slots.total.unwrap_or(0);
        if total == 0 && used == 0 {
            continue;
        }
        rows.push(Row::Header {
            label,
            icon_id,
            total,
        });
        rows.extend((0..used).map(|i| match slot {
            Slot::Rig => Row::Rig(i),
            Slot::Subsystem => Row::Subsystem(i),
            Slot::High | Slot::Mid | Slot::Low => Row::Module(slot, i),
        }));
        rows.extend((used..total as usize).map(|_| Row::Empty(slot)));
    }
    rows
}

fn blank_cells(row: &mut TableRow<'_, '_>, count: usize) {
    for _ in 0..count {
        row.col(|_| {});
    }
}

/// Published charges that fit a module type, by name.
fn compatible_charges(catalog: &Catalog, module: TypeId) -> Vec<TypeId> {
    let Some(m) = catalog.get(module).filter(|m| !m.charge_groups.is_empty()) else {
        return Vec::new();
    };
    let mut charges: Vec<_> = catalog
        .types
        .values()
        .filter(|c| c.published && c.market_group.is_some() && c.fits_in(m))
        .collect();
    charges.sort_by(|a, b| a.name.cmp(&b.name));
    charges.into_iter().map(|c| c.id).collect()
}

fn module_cells(
    row: &mut TableRow<'_, '_>,
    cx: Cx<'_>,
    (slot, i): (Slot, usize),
    m: &Module,
    info: ModuleInfo,
    actions: &mut Vec<Action>,
) {
    row.col(|ui| {
        if state_button(ui, cx.palette, m.state).clicked() {
            actions.push(Action::CycleState(slot, i));
        }
    });
    row.col(|ui| {
        cx.image(ui, Icon::Type(m.type_id), 22.0);
    });
    row.col(|ui| {
        let text = RichText::new(cx.catalog.name(m.type_id));
        ui.label(if m.state == ModuleState::Offline {
            text.weak()
        } else {
            text
        });
    });
    row.col(|ui| optional_label(ui, info.powergrid.map(short)));
    row.col(|ui| optional_label(ui, info.cpu.map(short)));
    row.col(|ui| optional_label(ui, info.cap_per_sec.map(|v| format!("{:+.1}/s", -v))));
    row.col(|ui| {
        let range = match (info.optimal, info.falloff) {
            (Some(o), Some(f)) => Some(format!("{}+{}", km(o), km(f))),
            (Some(o), None) => Some(km(o)),
            _ => None,
        };
        optional_label(ui, range);
    });
    row.col(|ui| {
        if let Some(charge) = m.charge {
            cx.image(ui, Icon::Type(charge), 20.0);
            ui.label(cx.catalog.name(charge));
        }
    });
}

fn module_menu(
    response: &egui::Response,
    cx: Cx<'_>,
    (slot, i): (Slot, usize),
    m: &Module,
    charges: &[TypeId],
    actions: &mut Vec<Action>,
) {
    response.context_menu(|ui| {
        ui.menu_button("State", |ui| {
            for state in [
                ModuleState::Offline,
                ModuleState::Online,
                ModuleState::Active,
                ModuleState::Overload,
            ] {
                if ui.radio(m.state == state, state.label()).clicked() {
                    actions.push(Action::SetState(slot, i, state));
                    ui.close();
                }
            }
        });
        if !charges.is_empty() {
            ui.menu_button("Charge", |ui| {
                egui::ScrollArea::vertical()
                    .max_height(500.0)
                    .show(ui, |ui| {
                        for &c in charges {
                            let picked = ui
                                .horizontal(|ui| {
                                    cx.image(ui, Icon::Type(c), 18.0);
                                    ui.add(egui::Button::new(cx.catalog.name(c)).frame(false))
                                })
                                .inner
                                .clicked();
                            if picked {
                                actions.push(Action::SetCharge(slot, i, Some(c)));
                                ui.close();
                            }
                        }
                    });
            });
        }
        if m.charge.is_some() && ui.button("Unload charge").clicked() {
            actions.push(Action::SetCharge(slot, i, None));
            ui.close();
        }
        ui.separator();
        if ui.button("Remove").clicked() {
            actions.push(Action::RemoveModule(slot, i));
            ui.close();
        }
    });
}

/// The state column: a mark showing whether the module is off, online, active or overheated.
fn state_button(ui: &mut egui::Ui, palette: Palette, state: ModuleState) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::click());
    let painter = ui.painter();
    let c = rect.center();
    match state {
        ModuleState::Offline => {
            painter.circle_stroke(c, 5.0, egui::Stroke::new(1.5, palette.muted));
        }
        ModuleState::Online => {
            painter.circle_stroke(c, 5.0, egui::Stroke::new(1.5, palette.yellow));
        }
        ModuleState::Active => {
            let stroke = egui::Stroke::new(2.0, palette.green);
            painter.line_segment([c + vec2(-5.0, 0.0), c + vec2(-1.5, 4.0)], stroke);
            painter.line_segment([c + vec2(-1.5, 4.0), c + vec2(5.0, -4.5)], stroke);
        }
        ModuleState::Overload => {
            painter.circle_filled(c, 5.5, palette.orange);
            painter.circle_filled(c, 2.5, palette.yellow);
        }
    }
    response.on_hover_text(format!("{} (click to change)", state.label()))
}

fn optional_label(ui: &mut egui::Ui, text: Option<String>) {
    if let Some(text) = text {
        ui.label(text);
    }
}

fn slot_name(slot: Slot) -> &'static str {
    match slot {
        Slot::High => "High",
        Slot::Mid => "Med",
        Slot::Low => "Low",
        Slot::Rig => "Rig",
        Slot::Subsystem => "Subsystem",
    }
}

/// A market entry. Returns true when double-clicked.
fn item_row(ui: &mut egui::Ui, cx: Cx<'_>, id: TypeId) -> bool {
    let Some(t) = cx.catalog.get(id) else {
        return false;
    };
    let response = ui
        .horizontal(|ui| {
            cx.image(ui, Icon::Type(id), 24.0);
            ui.add(egui::Label::new(&t.name).truncate());
        })
        .response
        .interact(Sense::click());
    if response.hovered() {
        let fill = ui.visuals().widgets.hovered.bg_fill.gamma_multiply(0.3);
        ui.painter().rect_filled(response.rect, 2.0, fill);
    }
    response
        .on_hover_text("Double-click to add")
        .double_clicked()
}

fn market_group(ui: &mut egui::Ui, cx: Cx<'_>, id: i32, add: &mut Option<TypeId>) {
    let Some(group) = cx.catalog.market_groups.get(&id) else {
        return;
    };
    let state_id = ui.make_persistent_id(("market", id));
    egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), state_id, false)
        .show_header(ui, |ui| {
            if let Some(icon_id) = group.icon_id {
                cx.image(ui, Icon::Id(icon_id), 18.0);
            }
            ui.label(&group.name);
        })
        .body(|ui| {
            for &child in &group.children {
                market_group(ui, cx, child, add);
            }
            for &t in &group.types {
                if item_row(ui, cx, t) {
                    *add = Some(t);
                }
            }
        });
}

fn simple_row(ui: &mut egui::Ui, cx: Cx<'_>, id: TypeId, count: Option<u32>) -> egui::Response {
    ui.horizontal(|ui| {
        cx.image(ui, Icon::Type(id), 22.0);
        let text = match count {
            Some(n) => format!("{} x{n}", cx.catalog.name(id)),
            None => cx.catalog.name(id),
        };
        ui.add(egui::Label::new(text).sense(Sense::click()))
    })
    .inner
}

/// Double-click or right-click > Remove on a row queues `action`.
fn removable(response: &egui::Response, action: Action, actions: &mut Vec<Action>) {
    let mut remove = response.double_clicked();
    response.context_menu(|ui| {
        if ui.button("Remove").clicked() {
            remove = true;
            ui.close();
        }
    });
    if remove {
        actions.push(action);
    }
}

// Stats sections

fn section(ui: &mut egui::Ui, title: &str, body: impl FnOnce(&mut egui::Ui)) {
    // Keyed on the first word, so the resistances header can carry its EHP total.
    egui::CollapsingHeader::new(RichText::new(title).strong())
        .id_salt(title.split(' ').next())
        .default_open(true)
        .show(ui, body);
}

fn resources(ui: &mut egui::Ui, cx: Cx<'_>, s: &Stats) {
    ui.horizontal(|ui| {
        for (icon_id, tip, slots) in [
            (icon::TURRET, "Turret hardpoints", s.turret),
            (icon::LAUNCHER, "Launcher hardpoints", s.launcher),
            (icon::DRONE_BAY, "Drones in space", s.drones_launched),
        ] {
            cx.image(ui, Icon::Id(icon_id), 24.0).on_hover_text(tip);
            let total = slots.total.unwrap_or(0);
            warn_label(ui, format!("{}/{total}", slots.used), slots_over(slots));
            ui.add_space(6.0);
        }
        cx.image(ui, Icon::Id(icon::CALIBRATION), 24.0)
            .on_hover_text("Calibration");
        let calibration = s.calibration;
        let text = format!(
            "{:.0}/{:.0}",
            calibration.used,
            calibration.output.unwrap_or(0.0)
        );
        warn_label(ui, text, resource_over(calibration));
    });
    ui.add_space(4.0);
    egui::Grid::new("resource_bars")
        .num_columns(4)
        .spacing([6.0, 4.0])
        .show(ui, |ui| {
            resource_bar(ui, cx, (icon::CPU, "CPU"), s.cpu, " tf");
            resource_bar(ui, cx, (icon::DRONE_BAY, "Drone bay"), s.drone_bay, " m³");
            ui.end_row();
            resource_bar(ui, cx, (icon::POWERGRID, "Powergrid"), s.powergrid, " MW");
            resource_bar(
                ui,
                cx,
                (icon::DRONE_BANDWIDTH, "Drone bandwidth"),
                s.drone_bandwidth,
                " Mbit/s",
            );
            ui.end_row();
        });
}

fn resource_bar(
    ui: &mut egui::Ui,
    cx: Cx<'_>,
    (icon_id, tip): (i32, &str),
    r: Resource,
    unit: &str,
) {
    cx.image(ui, Icon::Id(icon_id), 24.0).on_hover_text(tip);
    let out = r.output.unwrap_or(0.0);
    let fraction = match (out > 0.0, r.used > 0.0) {
        (true, _) => (r.used / out) as f32,
        (false, true) => 1.0,
        (false, false) => 0.0,
    };
    let color = if resource_over(r) {
        cx.palette.red
    } else {
        cx.palette.green
    };
    ui.vertical(|ui| {
        ui.small(format!("{}/{}{unit}", short(r.used), short(out)));
        ui.add(
            egui::ProgressBar::new(fraction.min(1.0))
                .desired_width(100.0)
                .desired_height(14.0)
                .fill(color)
                .text(format!("{:.1}%", fraction * 100.0)),
        );
    });
}

fn resistances(ui: &mut egui::Ui, cx: Cx<'_>, s: &Stats) {
    egui::Grid::new("resists")
        .num_columns(6)
        .spacing([4.0, 4.0])
        .show(ui, |ui| {
            ui.label("");
            for (icon_id, tip) in [
                (icon::EM, "EM"),
                (icon::THERMAL, "Thermal"),
                (icon::KINETIC, "Kinetic"),
                (icon::EXPLOSIVE, "Explosive"),
            ] {
                ui.vertical_centered(|ui| cx.image(ui, Icon::Id(icon_id), 20.0).on_hover_text(tip));
            }
            ui.weak("EHP");
            ui.end_row();
            for (icon_id, name, layer) in [
                (icon::SHIELD, "Shield", s.shield),
                (icon::ARMOR, "Armor", s.armor),
                (icon::HULL, "Hull", s.hull),
            ] {
                resist_row(ui, cx, (icon_id, name), layer);
            }
        });
}

fn resist_row(ui: &mut egui::Ui, cx: Cx<'_>, (icon_id, name): (i32, &str), layer: Layer) {
    cx.image(ui, Icon::Id(icon_id), 20.0)
        .on_hover_text(format!("{name}: {} HP", short(layer.hp)));
    for (resist, color) in layer.resists.iter().zip(DAMAGE_COLORS) {
        ui.add(
            egui::ProgressBar::new(*resist as f32)
                .desired_width(56.0)
                .desired_height(16.0)
                .fill(color)
                .text(format!("{:.1}%", resist * 100.0)),
        );
    }
    ui.label(layer.ehp.map_or_else(|| "–".into(), short));
    ui.end_row();
}

fn firepower(ui: &mut egui::Ui, cx: Cx<'_>, s: &Stats) {
    ui.horizontal(|ui| {
        for (icon_id, label, dps) in [
            (icon::TURRET, "Weapon", s.weapon_dps),
            (icon::DRONE_BANDWIDTH, "Drone", s.drone_dps),
        ] {
            cx.image(ui, Icon::Id(icon_id), 24.0);
            ui.vertical(|ui| {
                ui.small(label);
                ui.label(format!("{dps:.1} DPS"));
            });
            ui.add_space(8.0);
        }
        cx.image(ui, Icon::Id(icon::DAMAGE), 24.0);
        ui.vertical(|ui| {
            ui.small(format!("Volley: {}", short(s.volley.total())));
            ui.strong(format!("DPS: {:.1}", s.dps.total()));
        });
    });
    let parts = [s.dps.em, s.dps.thermal, s.dps.kinetic, s.dps.explosive];
    let total: f64 = parts.iter().sum();
    if total <= 0.0 {
        return;
    }
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 6.0), Sense::hover());
    let mut x = rect.left();
    for (part, color) in parts.iter().zip(DAMAGE_COLORS) {
        let width = rect.width() * (*part / total) as f32;
        let segment =
            egui::Rect::from_min_size(egui::pos2(x, rect.top()), vec2(width, rect.height()));
        ui.painter().rect_filled(segment, 0.0, color);
        x += width;
    }
}

fn capacitor(ui: &mut egui::Ui, cx: Cx<'_>, s: &Stats) {
    ui.horizontal(|ui| {
        cx.image(ui, Icon::Id(icon::CAPACITOR), 24.0);
        ui.vertical(|ui| {
            ui.small(format!(
                "Total: {} GJ",
                s.cap_amount.map_or_else(|| "–".into(), short)
            ));
            ui.label(match s.cap {
                Some(Cap::Stable(level)) => format!("Stable: {:.1}%", level * 100.0),
                Some(Cap::Lasts(secs)) => format!("Lasts {}", duration(secs)),
                None => "–".into(),
            });
        });
        ui.add_space(16.0);
        cx.image(ui, Icon::Id(icon::RECHARGE), 24.0);
        ui.vertical(|ui| {
            ui.small("Balance");
            ui.label(
                s.cap_balance
                    .map_or_else(|| "–".into(), |v| format!("{v:+.1} GJ/s")),
            );
        });
    });
}

fn targeting(ui: &mut egui::Ui, s: &Stats) {
    let show = |v: Option<f64>, fmt: &dyn Fn(f64) -> String| v.map_or_else(|| "–".into(), fmt);
    let left = [
        (
            "Targets",
            show(s.locks.map(f64::from), &|v| format!("{v:.0}")),
        ),
        (
            "Range",
            show(s.lock_range, &|v| format!("{:.1} km", v / 1000.0)),
        ),
        ("Scan res.", show(s.scan_res, &|v| format!("{v:.0} mm"))),
        (
            "Sensor str.",
            show(s.sensor_strength, &|v| format!("{v:.1}")),
        ),
    ];
    let right = [
        ("Speed", show(s.speed, &|v| format!("{v:.1} m/s"))),
        ("Align time", show(s.align_time, &|v| format!("{v:.2} s"))),
        ("Signature", show(s.sig_radius, &|v| format!("{v:.0} m"))),
        (
            "Warp speed",
            show(s.warp_speed, &|v| format!("{v:.2} AU/s")),
        ),
    ];
    egui::Grid::new("targeting")
        .num_columns(4)
        .spacing([10.0, 2.0])
        .show(ui, |ui| {
            for ((left_name, left_value), (right_name, right_value)) in left.into_iter().zip(right)
            {
                ui.weak(left_name);
                ui.label(left_value);
                ui.weak(right_name);
                ui.label(right_value);
                ui.end_row();
            }
        });
}

// Formatting

fn slots_over(s: Slots) -> bool {
    s.used > s.total.unwrap_or(0)
}

fn resource_over(r: Resource) -> bool {
    r.output.is_some_and(|out| r.used > out + 1e-6)
}

fn warn_label(ui: &mut egui::Ui, text: String, warn: bool) {
    let text = RichText::new(text);
    ui.label(if warn {
        text.color(ui.visuals().error_fg_color)
    } else {
        text
    });
}

/// Short numbers the way pyfa writes them: 1.38k, 16.6k, 2.25M.
fn short(v: f64) -> String {
    let magnitude = v.abs();
    let (scaled, suffix) = if magnitude >= 1e9 {
        (v / 1e9, "B")
    } else if magnitude >= 1e6 {
        (v / 1e6, "M")
    } else if magnitude >= 1e4 {
        (v / 1e3, "k")
    } else if magnitude >= 100.0 || v.fract().abs() < 1e-9 {
        return format!("{v:.0}");
    } else {
        return format!("{v:.1}");
    };
    let digits = match scaled.abs() {
        a if a >= 100.0 => 0,
        a if a >= 10.0 => 1,
        _ => 2,
    };
    format!("{scaled:.digits$}{suffix}")
}

fn km(meters: f64) -> String {
    if meters >= 1000.0 {
        format!("{:.1}km", meters / 1000.0)
    } else {
        format!("{meters:.0}m")
    }
}

fn duration(secs: f64) -> String {
    let secs = secs.round() as u64;
    match secs {
        0..60 => format!("{secs}s"),
        60..3600 => format!("{}m {}s", secs / 60, secs % 60),
        _ => format!("{}h {}m", secs / 3600, (secs % 3600) / 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_numbers_match_pyfa() {
        for (value, want) in [
            (0.0, "0"),
            (12.5, "12.5"),
            (630.0, "630"),
            (16_590.0, "16.6k"),
            (2_250_000.0, "2.25M"),
        ] {
            assert_eq!(short(value), want, "value {value}");
        }
    }
}
