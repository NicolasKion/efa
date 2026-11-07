//! The fitting wizard: one dialog over the fit with every choice on it, defaults picked for the
//! hull, and a live preview of what goes where and what it costs. The picking is in
//! [`crate::wizard`].

use std::{collections::BTreeMap, rc::Rc, time::Duration};

use gpui_kit::assets::IconName as AssetIcon;
use gpui_kit::component::{
    ActiveTheme, Disableable, Icon as UiIcon, IconName, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    h_flex, v_flex,
};
use gpui_kit::{
    AnyElement, App, AppContext, Context, Div, FocusHandle, InteractiveElement, IntoElement,
    KeyDownEvent, ParentElement, Render, SharedString, StatefulInteractiveElement, Styled,
    WeakEntity, Window, div, prelude::FluentBuilder, px,
};

use super::{Action, Gyfa, Loaded, item_stats, widgets};
use crate::{
    data::{Slot, TypeId},
    engine::{Pilot, Resource, Stats},
    fit::Fit,
    icons::Icon,
    item_data::ItemData,
    theme::Palette,
    wizard::{
        self, Ammo, Choices, DamageMods, Facts, Family, Pick, Prop, Rigs, Setup, Tank, Tier,
        Utility,
    },
};

const WIDTH: f32 = 900.;
const LABEL_W: f32 = 84.;
const ICON: f32 = 16.;

/// The rows of choices, top to bottom, for the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Row {
    Weapon,
    Tier,
    Ammo,
    Prop,
    Utility,
    Tank,
    Damage,
    Rigs,
    Options,
}

impl Row {
    const ALL: [Self; 9] = [
        Self::Weapon,
        Self::Tier,
        Self::Ammo,
        Self::Prop,
        Self::Utility,
        Self::Tank,
        Self::Damage,
        Self::Rigs,
        Self::Options,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::Weapon => "Weapons",
            Self::Tier => "Tier",
            Self::Ammo => "Ammo",
            Self::Prop => "Propulsion",
            Self::Utility => "Utility",
            Self::Tank => "Tank",
            Self::Damage => "Damage",
            Self::Rigs => "Rigs",
            Self::Options => "Racks",
        }
    }
}

/// One clickable option of a row.
struct Chip {
    label: SharedString,
    icon: Option<TypeId>,
    on: bool,
    /// Shown greyed out: nothing to put on, or no slot for it.
    disabled: bool,
}

pub(super) struct WizardPanel {
    gyfa: WeakEntity<Gyfa>,
    loaded: Rc<Loaded>,
    data: Rc<ItemData>,
    pilot: Rc<Pilot>,
    /// The fit as it was when the wizard opened.
    fit: Fit,
    facts: Facts,
    families: Vec<Family>,
    choices: Choices,
    picks: Vec<Pick>,
    stats: Stats,
    /// "Make it fit" changed the picks; a new choice starts over from the choices.
    made_to_fit: bool,
    row: usize,
    /// Which option of each row the keyboard is on.
    cursor: [usize; Row::ALL.len()],
    focus: FocusHandle,
    focused: bool,
    palette: Palette,
}

impl Gyfa {
    /// Open the wizard over the current fit. It needs a ship, and the item data for the hull's
    /// bonuses and the modules' fitting costs, which may still be loading the first time.
    pub(super) fn open_wizard(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded() else { return };
        if self.fit().and_then(|f| f.ship).is_none() {
            self.status = Some("The fitting wizard needs a ship first".into());
            cx.notify();
            return;
        }
        let Some(data) = item_stats::hover_data(&loaded, cx) else {
            self.status = Some("Loading item data for the fitting wizard…".into());
            cx.notify();
            let this = cx.entity().downgrade();
            let window_handle = window.window_handle();
            cx.spawn(async move |_, cx| {
                for _ in 0..600 {
                    cx.background_executor()
                        .timer(Duration::from_millis(100))
                        .await;
                    let ready = cx.update(|cx| item_stats::hover_data(&loaded, cx).is_some());
                    if ready {
                        let _ = window_handle.update(cx, |_, window, cx| {
                            Gyfa::app_update(&this, cx, |g, cx| {
                                g.status = None;
                                g.open_wizard(window, cx);
                            });
                        });
                        return;
                    }
                }
            })
            .detach();
            return;
        };
        let Some(fit) = self.fit().cloned() else {
            return;
        };
        let pilot = self.pilot(&loaded);
        let Some(facts) = Facts::gather(&loaded.engine, &data, &loaded.catalog, &fit, &pilot)
        else {
            return;
        };
        let families = wizard::families(&loaded.catalog, &data, &facts);
        let choices = Setup::new(&loaded.catalog, &data, &facts, &families).defaults();
        let gyfa = cx.entity().downgrade();
        let palette = self.palette;
        let panel = cx.new(|cx| {
            let mut panel = WizardPanel {
                gyfa,
                loaded,
                data,
                pilot,
                fit,
                facts,
                families,
                choices,
                picks: Vec::new(),
                stats: Stats::default(),
                made_to_fit: false,
                row: 0,
                cursor: [0; Row::ALL.len()],
                focus: cx.focus_handle(),
                focused: false,
                palette,
            };
            panel.replan();
            panel.sync_cursors();
            panel
        });
        let title = format!(
            "Fitting Wizard: {}",
            self.fit()
                .and_then(|f| f.ship)
                .and_then(|s| self.loaded().map(|l| l.catalog.name(s)))
                .unwrap_or_default()
        );
        window.open_dialog(cx, move |dialog, _, _| {
            dialog
                .title(title.clone())
                .w(px(WIDTH))
                .child(panel.clone())
        });
    }
}

impl WizardPanel {
    fn setup(&self) -> Setup<'_> {
        Setup::new(
            &self.loaded.catalog,
            &self.data,
            &self.facts,
            &self.families,
        )
    }

    /// Plan again from the choices and preview the result.
    fn replan(&mut self) {
        let picks = self.setup().plan(&self.fit, &self.choices);
        self.set_picks(picks);
        self.made_to_fit = false;
    }

    fn set_picks(&mut self, picks: Vec<Pick>) {
        let placements = self
            .setup()
            .placements(&self.fit, &picks, self.choices.clear);
        self.stats = wizard::preview(
            &self.loaded.engine,
            &self.pilot,
            &self.fit,
            &placements,
            self.choices.clear,
        );
        self.picks = picks;
    }

    fn make_it_fit(&mut self, cx: &mut Context<Self>) {
        let picks = self.setup().make_it_fit(
            &self.loaded.engine,
            &self.pilot,
            &self.fit,
            &self.picks,
            self.choices.clear,
        );
        self.set_picks(picks);
        self.made_to_fit = true;
        cx.notify();
    }

    fn apply(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let clear = self.choices.clear;
        let placements = self.setup().placements(&self.fit, &self.picks, clear);
        window.close_dialog(cx);
        Gyfa::app_update(&self.gyfa, cx, |g, cx| {
            g.apply(Action::ApplyWizard { placements, clear }, cx);
        });
    }

    fn change(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut Choices)) {
        let before = self.choices.clone();
        f(&mut self.choices);
        // A new weapon kind brings its own damage modules.
        if self.choices.family != before.family && self.choices.damage == DamageMods::None {
            self.choices.damage = before.damage;
        }
        if self.choices != before || self.made_to_fit {
            self.replan();
        }
        self.sync_cursors();
        cx.notify();
    }

    /// Put each single-choice row's keyboard cursor on its current choice.
    fn sync_cursors(&mut self) {
        for (i, row) in Row::ALL.into_iter().enumerate() {
            if let Some(on) = self.chips(row).iter().position(|c| c.on)
                && row != Row::Utility
            {
                self.cursor[i] = on;
            }
        }
    }

    fn kind(&self) -> Option<wizard::WeaponKind> {
        self.setup().family(&self.choices).map(|f| f.kind)
    }

    fn base(&self, name: &str) -> Option<TypeId> {
        self.loaded.catalog.find(name).map(|t| t.id)
    }

    /// A name's T2 (or nearest) variation, for labels and icons.
    fn shown(&self, name: &str) -> Option<TypeId> {
        self.base(name).map(|b| self.setup().resolve(b, Tier::T2))
    }

    fn chips(&self, row: Row) -> Vec<Chip> {
        let catalog = &self.loaded.catalog;
        let setup = self.setup();
        let c = &self.choices;
        let named = |id: Option<TypeId>, fallback: &str, on: bool| Chip {
            label: id
                .map_or_else(|| fallback.to_owned(), |id| catalog.name(id))
                .into(),
            icon: id,
            on,
            disabled: id.is_none(),
        };
        match row {
            Row::Weapon => self
                .families
                .iter()
                .enumerate()
                .map(|(i, f)| Chip {
                    label: format!("{}x {}", f.count, f.name).into(),
                    icon: Some(f.base),
                    on: c.family == Some(i),
                    disabled: false,
                })
                .chain(std::iter::once(Chip {
                    label: "None".into(),
                    icon: None,
                    on: c.family.is_none(),
                    disabled: false,
                }))
                .collect(),
            Row::Tier => {
                let family = setup.family(c);
                Tier::ALL
                    .into_iter()
                    .map(|tier| {
                        let id = family.map(|f| setup.resolve(f.base, tier));
                        let missing = family.is_some_and(|f| {
                            tier != Tier::T1 && id == Some(f.base)
                                || tier == Tier::Faction
                                    && id.and_then(|i| catalog.get(i)).and_then(|t| t.meta_group)
                                        != Some(4)
                        });
                        Chip {
                            label: tier.label().into(),
                            icon: None,
                            on: c.tier == tier,
                            disabled: family.is_none() || missing,
                        }
                    })
                    .collect()
            }
            Row::Ammo => {
                let weapon = setup.family(c).map(|f| setup.resolve(f.base, c.tier));
                Ammo::ALL
                    .into_iter()
                    .map(|ammo| {
                        let id = weapon.and_then(|w| setup.ammo(w, ammo));
                        match ammo {
                            Ammo::None => Chip {
                                label: "None".into(),
                                icon: None,
                                on: c.ammo == ammo,
                                disabled: false,
                            },
                            Ammo::Faction => named(id, "Faction", c.ammo == ammo),
                            Ammo::Standard => named(id, "T1", c.ammo == ammo),
                        }
                    })
                    .collect()
            }
            Row::Prop => Prop::ALL
                .into_iter()
                .map(|prop| match setup.prop_name(prop) {
                    Some(name) => named(self.shown(&name), &name, c.prop == prop),
                    None => Chip {
                        label: "None".into(),
                        icon: None,
                        on: c.prop == prop,
                        disabled: false,
                    },
                })
                .collect(),
            Row::Utility => {
                let kind = self.kind();
                Utility::ALL
                    .into_iter()
                    .map(|u| {
                        let id = self.shown(&setup.utility_name(u, kind));
                        Chip {
                            label: u.label().into(),
                            icon: id,
                            on: c.has(u),
                            disabled: id.is_none() || self.facts.total(u.slot()) == 0,
                        }
                    })
                    .collect()
            }
            Row::Tank => Tank::ALL
                .into_iter()
                .map(|tank| Chip {
                    label: tank.label().into(),
                    icon: None,
                    on: c.tank == tank,
                    disabled: false,
                })
                .collect(),
            Row::Damage => {
                let kind = self.kind();
                DamageMods::ALL
                    .into_iter()
                    .map(|d| match setup.damage_mod_name(d, kind) {
                        Some(name) => named(self.shown(name), name, c.damage == d),
                        None => Chip {
                            label: "None".into(),
                            icon: None,
                            on: c.damage == d,
                            disabled: d != DamageMods::None,
                        },
                    })
                    .collect()
            }
            Row::Rigs => Rigs::ALL
                .into_iter()
                .map(|rigs| Chip {
                    label: rigs.label().into(),
                    icon: None,
                    on: c.rigs == rigs,
                    disabled: self.facts.total(Slot::Rig) == 0 && rigs != Rigs::None,
                })
                .collect(),
            Row::Options => [false, true]
                .into_iter()
                .map(|clear| Chip {
                    label: if clear {
                        "Clear racks first"
                    } else {
                        "Fill free slots"
                    }
                    .into(),
                    icon: None,
                    on: c.clear == clear,
                    disabled: false,
                })
                .collect(),
        }
    }

    /// Pick option `i` of a row: single-choice rows switch to it, the utility row toggles it.
    fn choose(&mut self, row: Row, i: usize, cx: &mut Context<Self>) {
        let chips = self.chips(row);
        if chips.get(i).is_none_or(|c| c.disabled) {
            return;
        }
        let at = Row::ALL.iter().position(|&r| r == row).unwrap_or(0);
        self.row = at;
        self.cursor[at] = i;
        let families = self.families.len();
        self.change(cx, |c| match row {
            Row::Weapon => c.family = (i < families).then_some(i),
            Row::Tier => c.tier = Tier::ALL[i],
            Row::Ammo => c.ammo = Ammo::ALL[i],
            Row::Prop => c.prop = Prop::ALL[i],
            Row::Utility => c.toggle(Utility::ALL[i]),
            Row::Tank => c.tank = Tank::ALL[i],
            Row::Damage => c.damage = DamageMods::ALL[i],
            Row::Rigs => c.rigs = Rigs::ALL[i],
            Row::Options => c.clear = i == 1,
        });
    }

    fn key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        if keystroke.modifiers.control || keystroke.modifiers.alt {
            return;
        }
        let row = Row::ALL[self.row];
        let len = self.chips(row).len();
        // One option back or forward, around the ends, past what cannot be picked.
        let step = |back: bool| {
            let mut at = self.cursor[self.row];
            let chips = self.chips(row);
            let len = len.max(1);
            for _ in 0..len {
                at = if back {
                    (at + len - 1) % len
                } else {
                    (at + 1) % len
                };
                if chips.get(at).is_some_and(|c| !c.disabled) {
                    break;
                }
            }
            at
        };
        match keystroke.key.as_str() {
            "enter" => self.apply(window, cx),
            "up" => {
                self.row = self.row.checked_sub(1).unwrap_or(Row::ALL.len() - 1);
                cx.notify();
            }
            "down" | "tab" => {
                self.row = (self.row + 1) % Row::ALL.len();
                cx.notify();
            }
            "left" | "right" => {
                let next = step(keystroke.key == "left");
                if row == Row::Utility {
                    self.cursor[self.row] = next;
                    cx.notify();
                } else {
                    self.choose(row, next, cx);
                }
            }
            "space" => {
                let i = self.cursor[self.row];
                self.choose(row, i, cx);
            }
            "m" if !wizard::fits(&self.stats) => self.make_it_fit(cx),
            key => {
                if let Ok(n) = key.parse::<usize>()
                    && n >= 1
                {
                    self.choose(row, n - 1, cx);
                } else {
                    return;
                }
            }
        }
        cx.stop_propagation();
    }

    fn chip(&self, row: Row, i: usize, chip: Chip, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let at = Row::ALL.iter().position(|&r| r == row).unwrap_or(0);
        let cursor = at == self.row && self.cursor[at] == i;
        let (bg, fg, border) = if chip.on {
            (theme.primary, theme.primary_foreground, theme.primary)
        } else {
            (theme.secondary, theme.foreground, theme.border)
        };
        h_flex()
            .id(SharedString::from(format!("wizard-{}-{i}", row.label())))
            .gap_1()
            .h(px(22.))
            .px_1p5()
            .rounded(px(4.))
            .border_1()
            .border_color(if cursor { theme.ring } else { border })
            .bg(bg)
            .text_color(fg)
            .text_xs()
            .when(chip.disabled, |c| c.opacity(0.4))
            .when(!chip.disabled, |c| {
                c.cursor_pointer()
                    .hover(|s| s.opacity(0.85))
                    .on_click(cx.listener(move |this, _, _, cx| this.choose(row, i, cx)))
            })
            .when(row == Row::Utility, |c| {
                c.child(
                    UiIcon::new(if chip.on {
                        AssetIcon::SquareCheck
                    } else {
                        AssetIcon::Square
                    })
                    .xsmall(),
                )
            })
            .when_some(chip.icon, |c, id| {
                c.child(widgets::icon_box(
                    &self.loaded.catalog,
                    &self.loaded.icons,
                    Icon::Type(id),
                    ICON,
                ))
            })
            .child(chip.label)
            .into_any_element()
    }

    fn row(&self, row: Row, cx: &mut Context<Self>) -> Div {
        let at = Row::ALL.iter().position(|&r| r == row).unwrap_or(0);
        let active = at == self.row;
        let chips: Vec<AnyElement> = self
            .chips(row)
            .into_iter()
            .enumerate()
            .map(|(i, chip)| self.chip(row, i, chip, cx))
            .collect();
        h_flex()
            .items_start()
            .gap_2()
            .child(
                div()
                    .w(px(LABEL_W))
                    .flex_none()
                    .pt(px(3.))
                    .text_xs()
                    .text_color(if active {
                        cx.theme().foreground
                    } else {
                        cx.theme().muted_foreground
                    })
                    .when(active, |d| d.font_weight(gpui_kit::FontWeight::SEMIBOLD))
                    .child(row.label()),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_w_0()
                    .flex_wrap()
                    .gap_1()
                    .children(chips),
            )
    }

    fn preview(&self, cx: &mut Context<Self>) -> Div {
        let catalog = &self.loaded.catalog;
        let theme = cx.theme();
        let mut racks = v_flex().gap_1();
        for slot in [Slot::High, Slot::Mid, Slot::Low, Slot::Rig] {
            let total = self.facts.total(slot);
            if total == 0 {
                continue;
            }
            let picks: Vec<&Pick> = self.picks.iter().filter(|p| p.slot == slot).collect();
            let kept = if self.choices.clear {
                0
            } else if slot == Slot::Rig {
                self.fit.rigs.iter().flatten().count()
            } else {
                self.fit.rack(slot).iter().flatten().count()
            };
            // Same module and charge on one line, in the order they go on.
            let mut lines: Vec<((TypeId, Option<TypeId>), usize)> = Vec::new();
            for p in &picks {
                match lines.iter_mut().find(|(k, _)| *k == (p.type_id, p.charge)) {
                    Some((_, n)) => *n += 1,
                    None => lines.push(((p.type_id, p.charge), 1)),
                }
            }
            let name = match slot {
                Slot::High => "High",
                Slot::Mid => "Mid",
                Slot::Low => "Low",
                _ => "Rigs",
            };
            let used = (kept + picks.len()).min(total);
            racks = racks.child(
                h_flex()
                    .justify_between()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(name)
                    .child(if kept > 0 {
                        format!("{used}/{total}, {kept} kept")
                    } else {
                        format!("{used}/{total}")
                    }),
            );
            for ((type_id, charge), n) in lines {
                racks = racks.child(
                    h_flex()
                        .gap_1()
                        .text_xs()
                        .child(widgets::icon_box(
                            catalog,
                            &self.loaded.icons,
                            Icon::Type(type_id),
                            ICON,
                        ))
                        .child(format!("{n}x {}", catalog.name(type_id)))
                        .when_some(charge, |row, c| {
                            row.child(
                                div()
                                    .text_color(theme.muted_foreground)
                                    .truncate()
                                    .child(catalog.name(c)),
                            )
                        }),
                );
            }
        }
        let gauges = v_flex()
            .gap_1()
            .child(self.resource("CPU", &self.stats.cpu, " tf", cx))
            .child(self.resource("Powergrid", &self.stats.powergrid, " MW", cx))
            .child(self.resource("Calibration", &self.stats.calibration, "", cx));
        v_flex().gap_2().child(racks).child(gauges)
    }

    fn resource(&self, label: &str, r: &Resource, unit: &str, cx: &App) -> Div {
        let out = r.output.unwrap_or(0.0);
        let fraction = if out > 0.0 { r.used / out } else { 0.0 };
        let over = r.used > out + 1e-6;
        let fill = if over {
            self.palette.red
        } else {
            self.palette.green
        };
        v_flex()
            .child(
                h_flex()
                    .justify_between()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(label.to_owned())
                    .child(format!("{:.1}/{out:.1}{unit}", r.used)),
            )
            .child(
                widgets::gauge(fraction, fill, format!("{:.1}%", fraction * 100.0), 0., cx)
                    .w_full(),
            )
    }
}

impl Render for WizardPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.focused {
            self.focused = true;
            let focus = self.focus.clone();
            window.defer(cx, move |window, cx| window.focus(&focus, cx));
        }
        let fits = wizard::fits(&self.stats);
        let rows: Vec<Div> = Row::ALL.into_iter().map(|r| self.row(r, cx)).collect();
        let by_slot: BTreeMap<Slot, usize> = self.picks.iter().fold(BTreeMap::new(), |mut m, p| {
            *m.entry(p.slot).or_default() += 1;
            m
        });
        let count: usize = by_slot.values().sum();
        let theme = cx.theme();
        let hint = div()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child("Up/Down: section, Left/Right or 1-9: choose, Space: toggle, Enter: apply");
        let footer = h_flex().justify_between().gap_2().child(hint).child(
            h_flex()
                .gap_2()
                .when(!fits, |f| {
                    f.child(
                        Button::new("wizard-make-fit")
                            .small()
                            .icon(AssetIcon::Minimize2)
                            .label("Make it fit (M)")
                            .on_click(cx.listener(|this, _, _, cx| this.make_it_fit(cx))),
                    )
                })
                .child(
                    Button::new("wizard-apply")
                        .small()
                        .primary()
                        .icon(IconName::Check)
                        .label(format!("Apply {count} modules"))
                        .disabled(count == 0)
                        .on_click(cx.listener(|this, _, window, cx| this.apply(window, cx))),
                ),
        );
        v_flex()
            .track_focus(&self.focus)
            .key_context("FittingWizard")
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                this.key(event, window, cx);
            }))
            .gap_3()
            .child(
                h_flex()
                    .items_start()
                    .gap_4()
                    .child(v_flex().flex_1().min_w_0().gap_1p5().children(rows))
                    .child(
                        v_flex()
                            .w(px(290.))
                            .flex_none()
                            .p_2()
                            .rounded(px(4.))
                            .border_1()
                            .border_color(theme.border)
                            .child(self.preview(cx)),
                    ),
            )
            .child(footer)
    }
}
