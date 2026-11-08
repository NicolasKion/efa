//! "Export All Fittings to HTML" (gui/utils/exportHtml.py): every saved fit as a link that
//! opens it in a browser fitting tool, by ship. pyfa links to Osmium, which is gone; these go to
//! EVEShip.fit. The full page adds the EFT text under each fit;
//! pyfa's full page pulls in jQuery Mobile for that, this one uses plain details elements.

use std::{borrow::Borrow, fmt::Write};

use super::{dna, eft, escape_markup};
use crate::{data::Catalog, fit::Fit};

/// Where the links send a fit's DNA.
const DNA_URL: &str = "https://eveship.fit/?fit=dna:";

pub fn export(fits: &[impl Borrow<Fit>], catalog: &Catalog, minimal: bool) -> String {
    // pyfa groups by ship group and ship, both by name.
    let mut rows: Vec<(String, String, &Fit)> = fits
        .iter()
        .map(Borrow::borrow)
        .filter_map(|fit| {
            let ship = catalog.get(fit.ship?)?;
            let group = catalog
                .group_names
                .get(&ship.group_id)
                .cloned()
                .unwrap_or_default();
            Some((group, ship.name.clone(), fit))
        })
        .collect();
    rows.sort_by(|a, b| (&a.0, &a.1, &a.2.name).cmp(&(&b.0, &b.1, &b.2.name)));
    let link = |fit: &Fit| {
        let dna = dna::export(fit, catalog, &dna::Options { formatting: false });
        format!("{DNA_URL}{dna}")
    };

    if minimal {
        let mut out = String::new();
        for (_, ship, fit) in &rows {
            let _ = writeln!(
                out,
                "<a class=\"outOfGameBrowserLink\" target=\"_blank\" href=\"{}\">{}: {}</a><br>",
                escape_markup(&link(fit)),
                escape_markup(ship),
                escape_markup(&fit.name)
            );
        }
        return out;
    }

    let mut out = String::from(
        "<!DOCTYPE html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n<title>efa fittings</title>\n\
         <style>body{font-family:sans-serif;max-width:60em;margin:1em auto;padding:0 1em}\
         h2{margin-top:1.5em}details{margin:.3em 0}pre{background:#f4f4f4;padding:.5em}</style>\n\
         </head>\n<body>\n<h1>Fittings</h1>\n",
    );
    let _ = writeln!(out, "<p>{} fits</p>", rows.len());
    let mut last_group = None;
    for (group, ship, fit) in &rows {
        if last_group != Some(group) {
            let _ = writeln!(out, "<h2>{}</h2>", escape_markup(group));
            last_group = Some(group);
        }
        let text = eft::export(fit, catalog, &eft::Options::default(), None);
        let _ = writeln!(
            out,
            "<details><summary><a target=\"_blank\" href=\"{}\">{}: {}</a></summary><pre>{}</pre></details>",
            escape_markup(&link(fit)),
            escape_markup(ship),
            escape_markup(&fit.name),
            escape_markup(&text)
        );
    }
    out.push_str("</body>\n</html>\n");
    out
}
