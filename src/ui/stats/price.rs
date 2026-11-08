//! pyfa's price view (builtinStatsViews/priceViewFull.py) and its menu (priceOptions.py).

use std::rc::Rc;

use gpui_kit::component::{h_flex, menu::PopupMenuItem, v_flex};
use gpui_kit::{App, Div, IntoElement, ParentElement, Styled, div, prelude::FluentBuilder};

use super::{Controls, MenuFn, Pane, Section, format, ids, muted, tip, title};
use crate::{
    data::TypeId,
    fit::Fit,
    icons::Icon,
    store::PriceTotals,
    ui::{
        Efa,
        widgets::{icon, icon_box},
    },
};

/// A fit's value split the way pyfa's price view shows it, in ISK. Items without a price count
/// as free, as in pyfa.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct FitPrice {
    pub ship: f64,
    /// Modules, rigs and subsystems, without their charges.
    pub fittings: f64,
    /// Implants and boosters.
    pub character: f64,
    pub drones: f64,
    pub cargo: f64,
}

impl FitPrice {
    /// The price with `implants` as the character's, for fits that use them.
    pub fn with_character_implants(
        fit: &Fit,
        implants: &[TypeId],
        price: impl Fn(TypeId) -> Option<f64>,
    ) -> Self {
        let each = |id: TypeId| price(id).unwrap_or(0.0);
        let stacks = |items: &mut dyn Iterator<Item = (TypeId, u32)>| -> f64 {
            items.map(|(id, n)| each(id) * f64::from(n)).sum()
        };
        let fittings = fit
            .modules()
            .map(|m| m.type_id)
            .chain(fit.rigs.iter().flatten().copied())
            .chain(fit.subsystems.iter().copied())
            .map(each)
            .sum();
        let character = fit
            .applied_implants(implants)
            .map(|(id, _)| id)
            .chain(fit.boosters.iter().map(|b| b.type_id))
            .map(each)
            .sum();
        Self {
            ship: fit.ship.map_or(0.0, each),
            fittings,
            character,
            drones: stacks(&mut fit.drones.iter().map(|d| (d.type_id, d.count))),
            cargo: stacks(&mut fit.cargo.iter().copied()),
        }
    }

    /// Ship and fittings, plus the groups `totals` switches on.
    pub fn total(&self, totals: PriceTotals) -> f64 {
        let optional = |on: bool, v: f64| if on { v } else { 0.0 };
        self.ship
            + self.fittings
            + optional(totals.drones, self.drones)
            + optional(totals.cargo, self.cargo)
            + optional(totals.character, self.character)
    }
}

/// ISK the way pyfa writes it: three digits, B for billions.
fn isk(v: f64) -> String {
    format!("{} ISK", format::currency(v))
}

pub(super) fn section(p: &Pane<'_>, controls: Option<&Controls<'_>>, cx: &App) -> Section {
    let totals = p.price_totals;
    let price =
        FitPrice::with_character_implants(p.fit, p.character_implants, |id| p.prices.get(id));
    // Prices can come from several sources when the chosen one lacks some items.
    let sources = p.prices.sources_summary(crate::port::fit_types(p.fit));
    let cell = |key: &str, icon_el: Div, label: &str, value: f64| {
        let mut text = format!("{} ISK", format::grouped(value, 2));
        if !sources.is_empty() {
            text = format!("{text}\n\nPrices from\n{sources}");
        }
        h_flex().flex_1().gap_1().child(icon_el).child(
            v_flex().child(div().child(label.to_owned())).child(tip(
                format!("price-{key}"),
                isk(value),
                text,
            )),
        )
    };
    let ship_icon = match p.fit.ship {
        Some(ship) => icon_box(&p.loaded.catalog, &p.loaded.icons, Icon::Type(ship), 24.),
        None => div().size_6(),
    };
    let body = v_flex()
        .gap_1()
        .child(
            h_flex()
                .w_full()
                .child(cell("ship", ship_icon, "Ship", price.ship))
                .child(cell(
                    "fittings",
                    p.icon(ids::FITTINGS, 24.),
                    "Fittings",
                    price.fittings,
                ))
                .child(cell(
                    "character",
                    p.icon(ids::CHARACTER, 24.),
                    "Character",
                    price.character,
                )),
        )
        .child(
            h_flex()
                .w_full()
                .child(cell(
                    "drones",
                    p.icon(icon::DRONE_BAY, 24.),
                    "Drones",
                    price.drones,
                ))
                .child(cell(
                    "cargo",
                    p.icon(ids::CARGO, 24.),
                    "Cargo bay",
                    price.cargo,
                ))
                .child(cell(
                    "total",
                    p.icon(ids::ISK, 24.),
                    "Total",
                    price.total(totals),
                )),
        )
        .into_any_element();

    let header = h_flex()
        .gap_2()
        .child(title("Price"))
        // pyfa's header note while a price fetch is out.
        .when(p.prices_updating, |h| {
            h.child(muted("Updating prices...", cx))
        })
        .into_any_element();

    let menu = controls.map(|controls| -> MenuFn {
        let this = controls.efa.clone();
        Rc::new(move |menu, _, _| {
            let toggle = |label: &'static str, on: bool, flip: fn(&mut PriceTotals)| {
                let this = this.clone();
                PopupMenuItem::new(label)
                    .checked(on)
                    .on_click(move |_, _, cx| {
                        Efa::app_update(&this, cx, |g, cx| {
                            flip(&mut g.saved.price_totals);
                            g.save();
                            cx.notify();
                        });
                    })
            };
            menu.item(toggle("Drones", totals.drones, |t| t.drones = !t.drones))
                .item(toggle("Cargo", totals.cargo, |t| t.cargo = !t.cargo))
                .item(toggle("Implants && Boosters", totals.character, |t| {
                    t.character = !t.character;
                }))
        })
    });
    Section {
        action: None,
        key: "Price",
        title: header,
        body,
        menu,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fit::{Booster, DroneStack, Module, ModuleState};

    #[test]
    fn splits_like_pyfa() {
        let module = |type_id| {
            Some(Module {
                type_id,
                state: ModuleState::Online,
                charge: Some(9),
                spool: None,
                mutation: None,
                rah: None,
            })
        };
        let fit = Fit {
            ship: Some(1),
            high: vec![module(2), None],
            low: vec![module(2)],
            rigs: vec![Some(3), None],
            drones: vec![DroneStack::new(4, 3, 1)],
            implants: vec![5],
            boosters: vec![Booster::new(6)],
            cargo: vec![(7, 10)],
            ..Fit::default()
        };
        let prices = |id: TypeId| (id != 9).then_some(f64::from(id) * 100.0);
        let p = FitPrice::with_character_implants(&fit, &[], prices);
        assert_eq!(
            p,
            FitPrice {
                ship: 100.0,
                fittings: 700.0,
                character: 1100.0,
                drones: 1200.0,
                cargo: 7000.0,
            },
            "charges are left out, stacks count every item"
        );
        assert_eq!(
            p.total(PriceTotals::default()),
            100.0 + 700.0 + 1200.0 + 7000.0
        );
        let all = PriceTotals {
            character: true,
            ..PriceTotals::default()
        };
        assert_eq!(p.total(all), 10_100.0);
    }

    #[test]
    fn isk_uses_b_for_billions() {
        assert_eq!(isk(1_250_000_000.0), "1.25B ISK");
        assert_eq!(isk(14_500_000.0), "14.5M ISK");
        assert_eq!(isk(0.0), "0 ISK");
    }
}
