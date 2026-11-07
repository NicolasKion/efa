//! pyfa's "Select a format" copy dialog (gui/copySelectDialog.py) as a window of its own: pick a
//! format and its options, then copy the current fit in it.

use std::rc::Rc;

use gpui_kit::component::{
    ActiveTheme, Disableable, Sizable,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    h_flex,
    radio::Radio,
    v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext, ClipboardItem, Context, Entity, IntoElement, ParentElement,
    Render, SharedString, Styled, WeakEntity, Window, WindowBounds, WindowOptions, div,
    prelude::FluentBuilder, px, size,
};

use super::{Efa, Hub, Loaded, stats::text};
use crate::{
    engine::Stats,
    fit::Fit,
    port::{self, ExportSettings, Format, dna, eft, multibuy, xml},
    prices::optimize,
};

/// One option checkbox: label, tooltip, whether it is on, and how to set it.
type OptionBox = (
    &'static str,
    &'static str,
    bool,
    fn(&mut ExportSettings, bool),
);

/// What the window copies: the fit as it was when the window opened.
pub struct ExportWindow {
    hub: Entity<Hub>,
    efa: WeakEntity<Efa>,
    loaded: Rc<Loaded>,
    fit: Fit,
    stats: Option<Stats>,
    sizes: Option<eft::RackSizes>,
    settings: ExportSettings,
    error: Option<SharedString>,
}

impl Efa {
    /// pyfa's "To Clipboard": open the format picker for the current fit.
    pub(super) fn open_export_window(&mut self, cx: &mut Context<Self>) {
        let (Some(loaded), Some(fit)) = (self.loaded(), self.fit().cloned()) else {
            return;
        };
        // Everything the window needs is taken now: it is built while this view is being
        // updated, so it must not read this view.
        let window = ExportWindow {
            hub: self.hub.clone(),
            efa: cx.entity().downgrade(),
            loaded,
            fit,
            stats: self.stats.clone(),
            sizes: self.rack_sizes(),
            settings: self.saved.export,
            error: None,
        };
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(380.), px(480.)), cx)),
            app_id: Some("efa".into()),
            ..Default::default()
        };
        let opened = gpui_kit::open_window(options, cx, move |window_handle, cx| {
            window_handle.set_window_title("efa: Export");
            cx.new(|_| window)
        });
        if let Err(e) = opened {
            self.status = Some(format!("Could not open a window: {e}").into());
            cx.notify();
        }
    }
}

impl ExportWindow {
    /// The fit in the chosen format, with its options.
    fn render_text(&self, cx: &App) -> Result<String, String> {
        let catalog = &self.loaded.catalog;
        let fit = &self.fit;
        match self.settings.format {
            Format::Eft => Ok(eft::export(fit, catalog, &self.settings.eft, self.sizes)),
            Format::MultiBuy => Ok(multibuy::export(fit, catalog, &self.settings.multibuy)),
            Format::Esi => port::esi::export(fit, catalog),
            Format::Dna => Ok(dna::export(fit, catalog, &self.settings.dna)),
            Format::Xml => Ok(xml::export(&[fit], catalog)),
            Format::Efs => {
                let hub = self.hub.read(cx);
                let characters = &hub.characters;
                let pilot = characters.pilot(fit.character, catalog);
                let id = characters.resolve(fit.character);
                let name = characters.name(id);
                let exporter = format!("efa {}", env!("CARGO_PKG_VERSION"));
                let context = crate::engine::EfsContext {
                    catalog,
                    fits: &hub.saved.fits,
                    pilot: &pilot,
                    character: (id != crate::character::CharacterId::All5).then_some(name.as_str()),
                    exporter: &exporter,
                };
                port::efs::export(&self.loaded.engine, fit, &context)
            }
            Format::Stats => {
                let stats = self.stats.as_ref().ok_or("no stats for this fit yet")?;
                let ship = fit.ship.map_or_default(|s| catalog.name(s));
                Ok(text::fit_stats(fit, stats, &ship))
            }
        }
    }

    fn copy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let settings = self.settings;
        if settings.format == Format::MultiBuy && settings.multibuy.optimize_prices {
            self.copy_cheaper_multibuy(window, cx);
            return;
        }
        match self.render_text(cx) {
            Ok(text) => {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                let settings = self.settings;
                self.hub
                    .update(cx, |hub, cx| hub.update_saved(|s| s.export = settings, cx));
                let what = format!("fit as {}", settings.format.label());
                let _ = self.efa.update(cx, |g, cx| {
                    g.status = Some(format!("Copied {what} to clipboard").into());
                    cx.notify();
                });
                window.remove_window();
            }
            Err(e) => {
                self.error = Some(e.into());
                cx.notify();
            }
        }
    }

    /// pyfa's multibuy with Optimize Prices: the main window prices the items and their
    /// replacements, fetching what it lacks, then copies the list.
    fn copy_cheaper_multibuy(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let settings = self.settings;
        self.hub
            .update(cx, |hub, cx| hub.update_saved(|s| s.export = settings, cx));
        let loaded = Rc::clone(&self.loaded);
        let fit = self.fit.clone();
        let amounts = multibuy::amounts(&fit, &loaded.catalog, &settings.multibuy);
        let types = optimize::types_to_price(&loaded.catalog, amounts.iter().map(|&(t, _)| t));
        let _ = self.efa.update(cx, |g, cx| {
            g.with_prices_for(types, cx, move |g, cx| {
                let prices = Rc::clone(&g.prices);
                let text =
                    multibuy::export_priced(&fit, &loaded.catalog, &settings.multibuy, |t| {
                        prices.get(t)
                    });
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                g.status = Some("Copied fit as MultiBuy to clipboard".into());
                cx.notify();
            });
        });
        window.remove_window();
    }

    /// The option checkboxes under a format, live only while it is picked.
    fn options(&self, format: Format, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let enabled = self.settings.format == format;
        let s = &self.settings;
        let boxes: Vec<OptionBox> = match format {
            Format::Eft => vec![
                (
                    "Loaded Charges",
                    "Export charges loaded into modules",
                    s.eft.loaded_charges,
                    |s, v| {
                        s.eft.loaded_charges = v;
                    },
                ),
                (
                    "Mutated Attributes",
                    "Export mutated modules' stats",
                    s.eft.mutations,
                    |s, v| {
                        s.eft.mutations = v;
                    },
                ),
                ("Implants", "Export implants", s.eft.implants, |s, v| {
                    s.eft.implants = v;
                }),
                ("Boosters", "Export boosters", s.eft.boosters, |s, v| {
                    s.eft.boosters = v;
                }),
                (
                    "Cargo",
                    "Export cargo hold contents",
                    s.eft.cargo,
                    |s, v| {
                        s.eft.cargo = v;
                    },
                ),
            ],
            Format::MultiBuy => vec![
                (
                    "Loaded Charges",
                    "Export charges loaded into modules",
                    s.multibuy.loaded_charges,
                    |s, v| {
                        s.multibuy.loaded_charges = v;
                    },
                ),
                (
                    "Implants",
                    "Export implants",
                    s.multibuy.implants,
                    |s, v| {
                        s.multibuy.implants = v;
                    },
                ),
                (
                    "Boosters",
                    "Export boosters",
                    s.multibuy.boosters,
                    |s, v| {
                        s.multibuy.boosters = v;
                    },
                ),
                (
                    "Cargo",
                    "Export cargo contents",
                    s.multibuy.cargo,
                    |s, v| {
                        s.multibuy.cargo = v;
                    },
                ),
                (
                    "Optimize Prices",
                    "Replace items by cheaper alternatives",
                    s.multibuy.optimize_prices,
                    |s, v| {
                        s.multibuy.optimize_prices = v;
                    },
                ),
            ],
            Format::Dna => vec![(
                "Formatting Tags",
                "Include formatting tags to paste fit directly into corp bulletins, MOTD, etc.",
                s.dna.formatting,
                |s, v| {
                    s.dna.formatting = v;
                },
            )],
            Format::Esi | Format::Efs | Format::Stats | Format::Xml => Vec::new(),
        };
        boxes
            .into_iter()
            .map(|(label, tip, checked, set)| {
                Checkbox::new(SharedString::from(format!("{}-{label}", format.label())))
                    .label(label)
                    .tooltip(tip)
                    .checked(checked)
                    .disabled(!enabled)
                    .on_click(cx.listener(move |this, value: &bool, _, cx| {
                        set(&mut this.settings, *value);
                        cx.notify();
                    }))
                    .into_any_element()
            })
            .collect()
    }
}

impl Render for ExportWindow {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let formats: Vec<AnyElement> = Format::ALL
            .into_iter()
            .map(|format| {
                let options = self.options(format, cx);
                v_flex()
                    .gap_1()
                    .child(
                        Radio::new(SharedString::from(format!("format-{}", format.label())))
                            .label(format.label())
                            .checked(self.settings.format == format)
                            .on_click(cx.listener(move |this, _: &bool, _, cx| {
                                this.settings.format = format;
                                this.error = None;
                                cx.notify();
                            })),
                    )
                    .when(!options.is_empty(), |col| {
                        col.child(v_flex().pl_6().gap_1().children(options))
                    })
                    .into_any_element()
            })
            .collect();
        let theme = cx.theme();
        v_flex()
            .size_full()
            .p_4()
            .gap_3()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_sm()
            .child(
                div()
                    .font_weight(gpui_kit::FontWeight::SEMIBOLD)
                    .child(format!("Copy \"{}\" as", self.fit.name)),
            )
            .child(v_flex().flex_1().gap_2().children(formats))
            .when_some(self.error.clone(), |col, e| {
                col.child(div().text_color(theme.danger).child(e))
            })
            .child(
                h_flex()
                    .justify_end()
                    .gap_2()
                    .child(
                        Button::new("export-cancel")
                            .small()
                            .label("Cancel")
                            .on_click(|_, window: &mut Window, _: &mut App| window.remove_window()),
                    )
                    .child(
                        Button::new("export-copy")
                            .small()
                            .primary()
                            .label("Copy")
                            .on_click(cx.listener(|this, _, window, cx| this.copy(window, cx))),
                    ),
            )
    }
}
