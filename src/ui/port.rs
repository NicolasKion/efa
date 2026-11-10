//! Fits in and out of the main window: clipboard, files, backups and the HTML page, plus undo
//! and redo, as pyfa's main frame handles them (gui/mainFrame.py).

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use gpui_kit::{AppContext, Context, PathPromptOptions, Task, WeakEntity};

use super::{Action, AddKind, AdditionsTab, Efa};
use crate::{
    data::{Catalog, Kind, TypeId},
    fit::{Fit, ImplantSource},
    port::{self, Format, eft, html, xml},
};

/// How many imported fits get a tab; more than this stay in the fittings browser, like pyfa
/// which only opens a handful after a large import.
const OPEN_AFTER_IMPORT: usize = 10;

impl Efa {
    /// What a fit new to efa starts with, from the preferences: pyfa's "use character implants
    /// by default" applies to fits that bring none of their own.
    pub(super) fn new_fit_defaults(&self, fit: &mut Fit) {
        fit.character = self.last_character;
        fit.implant_source = if fit.implants.is_empty()
            && self.saved.settings.preferences.general.character_implants
        {
            ImplantSource::Character
        } else {
            ImplantSource::Fit
        };
    }

    /// Import fits from text in any format, opening them. Returns how many came in; a failure
    /// goes to the status line.
    pub(super) fn import_fits(&mut self, text: &str, file_name: Option<&str>) -> usize {
        let Some(loaded) = self.loaded() else {
            return 0;
        };
        match port::import(&loaded.catalog, text, file_name) {
            Ok(imported) => {
                let count = imported.fits.len();
                self.add_imported(imported);
                count
            }
            Err(e) => {
                self.set_error(format!("Import failed: {e}"));
                0
            }
        }
    }

    pub(super) fn add_imported(&mut self, imported: port::Imported) {
        let count = imported.fits.len();
        for (n, mut fit) in imported.fits.into_iter().enumerate() {
            self.new_fit_defaults(&mut fit);
            fit.modified = crate::fit::now();
            self.saved.push_fit(fit);
            let index = self.saved.fits.len() - 1;
            if n < OPEN_AFTER_IMPORT {
                self.saved.settings.open.push(index);
                self.saved.settings.current = Some(index);
            }
        }
        let mut status = match count {
            1 => "Imported 1 fit".to_owned(),
            n => format!("Imported {n} fits"),
        };
        if !imported.unknown.is_empty() {
            let mut unknown = imported.unknown;
            unknown.dedup();
            status = format!("{status}; skipped unknown items: {}", unknown.join(", "));
        }
        self.set_status(status);
    }

    /// pyfa's "From Clipboard": a fit in any format becomes a new fit; a list of items goes into
    /// the current fit, onto the additions tab they belong to.
    pub(super) fn paste_from_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            self.set_status("Nothing on the clipboard");
            cx.notify();
            return;
        };
        let Some(loaded) = self.loaded() else { return };
        match port::import(&loaded.catalog, &text, None) {
            Ok(imported) => {
                self.add_imported(imported);
                self.clear_selection();
                self.changed(cx);
            }
            Err(e) => {
                // An item link from the game: the mutated item it names, fetched from ESI.
                let link = port::muta::parse_item_link(&text)
                    .filter(|_| self.saved.settings.current.is_some());
                if let Some((type_id, item_id)) = link {
                    self.paste_item_link(type_id, item_id, cx);
                    return;
                }
                // pyfa fits a pasted mutated item, as its muta text, into the current fit.
                let mutant = port::muta::parse_single(&loaded.catalog, &text)
                    .and_then(|p| p.mutation(&loaded.catalog))
                    .filter(|_| self.saved.settings.current.is_some());
                if let Some((type_id, mutation)) = mutant {
                    self.apply(Action::AddMutated(type_id, mutation), cx);
                    return;
                }
                let items = port::parse_items(&loaded.catalog, &text)
                    .filter(|_| self.saved.settings.current.is_some());
                if let Some(items) = items {
                    self.paste_items(&loaded.catalog, &text, items, cx);
                } else {
                    self.set_error(format!("Import failed: {e}"));
                    cx.notify();
                }
            }
        }
    }

    /// A mutated item linked from the game, as pyfa pastes one: ESI says what it was made from
    /// and what it rolled, and it goes into the current fit.
    fn paste_item_link(&mut self, type_id: TypeId, item_id: i64, cx: &mut Context<Self>) {
        let network = self.saved.settings.preferences.network.clone();
        if !network.enabled {
            self.set_error("Pasting an item from the game needs the network, which is off in Preferences > Network");
            cx.notify();
            return;
        }
        self.set_status("Fetching the item from ESI...");
        cx.notify();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let fetched = cx
                .background_spawn(async move {
                    let http = network.proxy.client()?;
                    port::muta::fetch_item(&http, type_id, item_id)
                })
                .await;
            let _ = this.update(cx, |g, cx| {
                let Some(loaded) = g.loaded() else { return };
                let catalog = &loaded.catalog;
                match fetched.map(|parsed| parsed.mutation(catalog)) {
                    Ok(Some((abyssal, mutation))) if g.saved.settings.current.is_some() => {
                        g.apply(Action::AddMutated(abyssal, mutation), cx);
                    }
                    Ok(Some(_)) => g.set_error("No fit open to paste the item into"),
                    Ok(None) => g.set_error(format!(
                        "{} is not a mutated item efa knows",
                        catalog.name(type_id)
                    )),
                    Err(e) => g.set_error(format!("Could not fetch the item: {e}")),
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Items pasted onto the current fit, routed like pyfa's additions import: all drones go to
    /// drones, all implants to implants, all boosters to boosters, a single module is fitted,
    /// all fighters become squadrons, and anything else lands in cargo. `text` is what `items` came
    /// from, which fighters read again so a line without a count stays a full squadron.
    fn paste_items(
        &mut self,
        catalog: &Catalog,
        text: &str,
        items: Vec<(TypeId, u32)>,
        cx: &mut Context<Self>,
    ) {
        let kind_of = |t: TypeId| catalog.get(t).map(|t| t.kind);
        let all = |want: fn(Kind) -> bool| items.iter().all(|&(t, _)| kind_of(t).is_some_and(want));
        if let [(type_id, 1)] = items[..]
            && matches!(kind_of(type_id), Some(Kind::Module(_)))
        {
            self.add_item(type_id, cx);
            return;
        }
        if all(|k| k == Kind::Fighter) {
            self.status = None;
            self.add_fighter_squadrons(super::additions::fighters::parse_lines(catalog, text), cx);
            return;
        }
        let (kind, tab) = if all(|k| k == Kind::Drone) {
            (AddKind::Drones, AdditionsTab::Drones)
        } else if all(|k| k == Kind::Implant) {
            (AddKind::Implants, AdditionsTab::Implants)
        } else if all(|k| k == Kind::Booster) {
            (AddKind::Boosters, AdditionsTab::Boosters)
        } else {
            (AddKind::Cargo, AdditionsTab::Cargo)
        };
        self.show_additions_tab(tab);
        self.status = None;
        self.apply(Action::AddAdditions(kind, items), cx);
    }

    /// pyfa's "Import Fittings": pick files in any format; every fit in them comes in.
    pub(super) fn import_files_dialog(cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Import".into()),
        });
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let _ = this.update(cx, |g, cx| {
                let mut imported = 0;
                let mut failed = Vec::new();
                for path in &paths {
                    let name = path.file_name().and_then(|n| n.to_str());
                    match std::fs::read(path) {
                        Ok(bytes) if bytes.is_empty() => {}
                        Ok(bytes) => {
                            let text = String::from_utf8_lossy(&bytes);
                            let n = g.import_fits(&text, name);
                            if n == 0 {
                                failed.push(path.display().to_string());
                            }
                            imported += n;
                        }
                        Err(e) => failed.push(format!("{}: {e}", path.display())),
                    }
                }
                if !failed.is_empty() {
                    g.set_error(format!(
                        "Imported {imported} fits; failed: {}",
                        failed.join(", ")
                    ));
                }
                g.clear_selection();
                g.changed(cx);
            });
        })
        .detach();
    }

    /// pyfa's "Export Fitting": save the current fit to a file. pyfa only writes EVE XML here;
    /// the file name picks the format, so .txt and .cfg give EFT and .json gives ESI as well.
    pub(super) fn save_fit_dialog(&mut self, cx: &mut Context<Self>) {
        let (Some(loaded), Some(fit)) = (self.loaded(), self.fit().cloned()) else {
            return;
        };
        let ship = fit.ship.map_or_default(|s| loaded.catalog.name(s));
        let suggested = sanitize(&format!("{ship} - {}.xml", fit.name));
        let sizes = self.rack_sizes();
        Self::save_text_dialog(&suggested, cx, move |path| {
            let format = match extension(path).as_deref() {
                Some("txt" | "cfg") => Format::Eft,
                Some("json") => Format::Esi,
                _ => Format::Xml,
            };
            match format {
                Format::Eft => Ok(eft::export(
                    &fit,
                    &loaded.catalog,
                    &eft::Options::default(),
                    sizes,
                )),
                Format::Esi => port::esi::export(&fit, &loaded.catalog),
                _ => Ok(xml::export(&[&fit], &loaded.catalog)),
            }
        });
    }

    /// pyfa's "Backup All Fittings": every fit in one EVE XML file.
    pub(super) fn backup_dialog(&mut self, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded() else { return };
        let fits = self.saved.fits.clone();
        let suggested = format!("efa-fits-{}.xml", timestamp());
        Self::save_text_dialog(&suggested, cx, move |_| {
            let refs: Vec<&Fit> = fits.iter().map(|f| &**f).collect();
            Ok(xml::export(&refs, &loaded.catalog))
        });
    }

    /// Ask where to save, then write what `render` makes for that path.
    fn save_text_dialog(
        suggested: &str,
        cx: &mut Context<Self>,
        render: impl FnOnce(&Path) -> Result<String, String> + 'static,
    ) {
        let dir = crate::paths::home_dir();
        let picked = cx.prompt_for_new_path(&dir, Some(suggested));
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let path = with_default_extension(path, "xml");
            let outcome = match render(&path)
                .and_then(|text| std::fs::write(&path, text).map_err(|e| e.to_string()))
            {
                Ok(()) => Ok(format!("Saved {}", path.display()).into()),
                Err(e) => Err(format!("Saving {} failed: {e}", path.display()).into()),
            };
            let _ = this.update(cx, |g, cx| {
                g.set_outcome(outcome);
                cx.notify();
            });
        })
        .detach();
    }

    /// pyfa's "Export All Fittings to HTML", to the path set in the preferences.
    pub(super) fn export_html(&mut self, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded() else { return };
        let settings = &self.saved.settings.preferences.html;
        let path = html_path(&settings.path);
        let page = html::export(&self.saved.fits, &loaded.catalog, settings.minimal);
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, page));
        let outcome = match written {
            Ok(()) => {
                cx.open_url(&file_url(&path));
                Ok(format!(
                    "Exported {} fits to {}",
                    self.saved.fits.len(),
                    path.display()
                )
                .into())
            }
            Err(e) => Err(format!("HTML export to {} failed: {e}", path.display()).into()),
        };
        self.set_outcome(outcome);
        cx.notify();
    }

    /// Slot counts of the current fit's ship, for EFT's empty slot lines.
    pub(super) fn rack_sizes(&self) -> Option<eft::RackSizes> {
        let stats = self.stats.as_ref()?;
        let size = |total: Option<u32>| total.unwrap_or(0) as usize;
        Some(eft::RackSizes {
            high: size(stats.high.total),
            mid: size(stats.mid.total),
            low: size(stats.low.total),
            rig: size(stats.rig.total),
            service: size(stats.service.total),
        })
    }

    /// Search the market once typing pauses for the delay set in the preferences.
    pub(super) fn search_after_delay(&mut self, text: String, cx: &mut Context<Self>) {
        let delay = self.saved.settings.preferences.market.search_delay_ms;
        if delay == 0 || text.is_empty() {
            self.search_debounce = None;
            self.search_text = text;
            cx.notify();
            return;
        }
        let task: Task<()> = cx.spawn(async move |this: WeakEntity<Self>, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(delay))
                .await;
            let _ = this.update(cx, |g, cx| {
                g.search_text = text;
                g.search_debounce = None;
                cx.notify();
            });
        });
        self.search_debounce = Some(task);
    }

    /// Open a fit from the fittings browser: in a tab of its own, or in place of the current
    /// one, as pyfa's "Open fittings in a new page by default" says.
    pub(super) fn open_from_browser(&mut self, index: usize, cx: &mut Context<Self>) {
        let replace = !self.saved.settings.preferences.general.open_fit_in_new_tab
            && !self.saved.settings.open.contains(&index);
        if replace
            && let Some(current) = self.saved.settings.current
            && let Some(tab) = self.saved.settings.open.iter_mut().find(|t| **t == current)
        {
            *tab = index;
        }
        self.open_fit(index, cx);
    }
}

/// A file's address for the browser: forward slashes, and the characters a URL cannot hold as
/// they are escaped.
fn file_url(path: &std::path::Path) -> String {
    let path = path.to_string_lossy().replace('\\', "/");
    let mut url = String::from(if path.starts_with('/') {
        "file://"
    } else {
        "file:///"
    });
    for c in path.chars() {
        match c {
            ' ' => url.push_str("%20"),
            '#' => url.push_str("%23"),
            '%' => url.push_str("%25"),
            '?' => url.push_str("%3F"),
            c => url.push(c),
        }
    }
    url
}

/// Where the HTML export goes: the preference, or a file in efa's data directory.
pub(super) fn html_path(preference: &str) -> PathBuf {
    if !preference.trim().is_empty() {
        return PathBuf::from(preference.trim());
    }
    crate::paths::data_file("fits.html").unwrap_or_else(|| PathBuf::from("fits.html"))
}

fn extension(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(str::to_lowercase)
}

/// pyfa adds the format's extension when the name has none.
fn with_default_extension(path: PathBuf, default: &str) -> PathBuf {
    if path.extension().is_some() {
        path
    } else {
        path.with_extension(default)
    }
}

/// A fit name made safe as a file name.
fn sanitize(name: &str) -> String {
    name.chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '\0') {
                '_'
            } else {
                c
            }
        })
        .collect()
}

/// UTC time as pyfa stamps its backup names, `YYYYMMDD_HHMMSS`.
fn timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (days, rest) = (secs / 86_400, secs % 86_400);
    let (y, m, d) = civil_from_days(days.cast_signed());
    format!(
        "{y:04}{m:02}{d:02}_{:02}{:02}{:02}",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Days since 1970-01-01 to a calendar date (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_from_days() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(19_723), (2024, 1, 1));
        assert_eq!(civil_from_days(20_513), (2026, 3, 1));
    }

    #[test]
    fn file_names() {
        assert_eq!(sanitize("Vexor - A/B: test.xml"), "Vexor - A_B_ test.xml");
        assert_eq!(
            with_default_extension(PathBuf::from("/tmp/fits"), "xml"),
            PathBuf::from("/tmp/fits.xml")
        );
        assert_eq!(html_path(" /tmp/x.html "), PathBuf::from("/tmp/x.html"));
        assert_eq!(
            file_url(std::path::Path::new("/home/a b/fits#1.html")),
            "file:///home/a%20b/fits%231.html"
        );
        assert_eq!(
            file_url(std::path::Path::new("C:\\Users\\a\\fits.html")),
            "file:///C:/Users/a/fits.html"
        );
    }
}
