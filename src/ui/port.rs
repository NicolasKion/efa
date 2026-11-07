//! Fits in and out of the main window: clipboard, files, backups and the HTML page, plus undo
//! and redo, as pyfa's main frame handles them (gui/mainFrame.py).

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use gpui_kit::{Context, PathPromptOptions, SharedString, Task, WeakEntity};

use super::{Action, AddKind, AdditionsTab, Gyfa};
use crate::{
    data::{Catalog, Kind, TypeId},
    fit::{Fit, ImplantSource},
    port::{self, Format, eft, html, xml},
};

/// How many imported fits get a tab; more than this stay in the fittings browser, like pyfa
/// which only opens a handful after a large import.
const OPEN_AFTER_IMPORT: usize = 10;

impl Gyfa {
    /// What a fit new to gyfa starts with, from the preferences: pyfa's "use character implants
    /// by default" applies to fits that bring none of their own.
    pub(super) fn new_fit_defaults(&self, fit: &mut Fit) {
        fit.character = self.last_character;
        fit.implant_source =
            if fit.implants.is_empty() && self.saved.preferences.general.character_implants {
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
                self.status = Some(format!("Import failed: {e}").into());
                0
            }
        }
    }

    fn add_imported(&mut self, imported: port::Imported) {
        let count = imported.fits.len();
        for (n, mut fit) in imported.fits.into_iter().enumerate() {
            self.new_fit_defaults(&mut fit);
            fit.modified = crate::fit::now();
            self.saved.fits.push(fit);
            let index = self.saved.fits.len() - 1;
            if n < OPEN_AFTER_IMPORT {
                self.saved.open.push(index);
                self.saved.current = Some(index);
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
        self.status = Some(status.into());
    }

    /// pyfa's "From Clipboard": a fit in any format becomes a new fit; a list of items goes into
    /// the current fit, onto the additions tab they belong to.
    pub(super) fn paste_from_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            self.status = Some("Nothing on the clipboard".into());
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
                let items = port::parse_items(&loaded.catalog, &text)
                    .filter(|_| self.saved.current.is_some());
                if let Some(items) = items {
                    self.paste_items(&loaded.catalog, items, cx);
                } else {
                    self.status = Some(format!("Import failed: {e}").into());
                    cx.notify();
                }
            }
        }
    }

    /// Items pasted onto the current fit, routed like pyfa's additions import: all drones go to
    /// drones, all implants to implants, all boosters to boosters, a single module is fitted, and
    /// anything else lands in cargo.
    fn paste_items(
        &mut self,
        catalog: &Catalog,
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
        let (kind, tab) = if all(|k| k == Kind::Drone) {
            (AddKind::Drones, AdditionsTab::Drones)
        } else if all(|k| k == Kind::Implant) {
            (AddKind::Implants, AdditionsTab::Implants)
        } else if all(|k| k == Kind::Booster) {
            (AddKind::Boosters, AdditionsTab::Boosters)
        } else {
            (AddKind::Cargo, AdditionsTab::Cargo)
        };
        self.additions_tab = tab;
        self.additions_selection.clear();
        self.additions_anchor = None;
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
                    g.status = Some(
                        format!("Imported {imported} fits; failed: {}", failed.join(", ")).into(),
                    );
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
        let suggested = format!("gyfa-fits-{}.xml", timestamp());
        Self::save_text_dialog(&suggested, cx, move |_| {
            let refs: Vec<&Fit> = fits.iter().collect();
            Ok(xml::export(&refs, &loaded.catalog))
        });
    }

    /// Ask where to save, then write what `render` makes for that path.
    fn save_text_dialog(
        suggested: &str,
        cx: &mut Context<Self>,
        render: impl FnOnce(&Path) -> Result<String, String> + 'static,
    ) {
        let dir = home_dir();
        let picked = cx.prompt_for_new_path(&dir, Some(suggested));
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let Ok(Ok(Some(path))) = picked.await else {
                return;
            };
            let path = with_default_extension(path, "xml");
            let status: SharedString = match render(&path)
                .and_then(|text| std::fs::write(&path, text).map_err(|e| e.to_string()))
            {
                Ok(()) => format!("Saved {}", path.display()).into(),
                Err(e) => format!("Saving {} failed: {e}", path.display()).into(),
            };
            let _ = this.update(cx, |g, cx| {
                g.status = Some(status);
                cx.notify();
            });
        })
        .detach();
    }

    /// pyfa's "Export All Fittings to HTML", to the path set in the preferences.
    pub(super) fn export_html(&mut self, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded() else { return };
        let settings = &self.saved.preferences.html;
        let path = html_path(&settings.path);
        let page = html::export(&self.saved.fits, &loaded.catalog, settings.minimal);
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&path, page));
        self.status = Some(match written {
            Ok(()) => format!(
                "Exported {} fits to {}",
                self.saved.fits.len(),
                path.display()
            )
            .into(),
            Err(e) => format!("HTML export to {} failed: {e}", path.display()).into(),
        });
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
        })
    }

    pub(super) fn can_undo(&self) -> bool {
        self.saved.current.is_some_and(|i| self.history.can_undo(i))
    }

    pub(super) fn can_redo(&self) -> bool {
        self.saved.current.is_some_and(|i| self.history.can_redo(i))
    }

    pub(super) fn undo(&mut self, cx: &mut Context<Self>) {
        let Some(i) = self.saved.current else { return };
        let current = self.saved.fits[i].clone();
        if let Some(previous) = self.history.undo(i, current) {
            self.restore(i, previous, cx);
        } else {
            self.status = Some("Nothing to undo".into());
            cx.notify();
        }
    }

    pub(super) fn redo(&mut self, cx: &mut Context<Self>) {
        let Some(i) = self.saved.current else { return };
        let current = self.saved.fits[i].clone();
        if let Some(next) = self.history.redo(i, current) {
            self.restore(i, next, cx);
        } else {
            self.status = Some("Nothing to redo".into());
            cx.notify();
        }
    }

    fn restore(&mut self, i: usize, fit: Fit, cx: &mut Context<Self>) {
        self.saved.fits[i] = fit;
        self.clear_selection();
        self.inputs_show = None;
        self.status = None;
        self.changed(cx);
    }

    /// Search the market once typing pauses for the delay set in the preferences.
    pub(super) fn search_after_delay(&mut self, text: String, cx: &mut Context<Self>) {
        let delay = self.saved.preferences.market.search_delay_ms;
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
        let replace = !self.saved.preferences.general.open_fit_in_new_tab
            && !self.saved.open.contains(&index);
        if replace
            && let Some(current) = self.saved.current
            && let Some(tab) = self.saved.open.iter_mut().find(|t| **t == current)
        {
            *tab = index;
        }
        self.open_fit(index, cx);
    }
}

/// Where the HTML export goes: the preference, or a file in gyfa's data directory.
pub(super) fn html_path(preference: &str) -> PathBuf {
    if !preference.trim().is_empty() {
        return PathBuf::from(preference.trim());
    }
    let data = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
        .unwrap_or_default();
    data.join("gyfa/fits.html")
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from)
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
    }
}
