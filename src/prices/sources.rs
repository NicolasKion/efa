//! Asking each price source: ESI's price list, Fuzzwork, ceve-market and EVE Tycoon, and reading
//! their replies.

use std::collections::HashMap;

use serde::Deserialize;

use super::{DEFAULT_MAX_AGE, Http, PriceSource, PriceSystem, Prices};
use crate::{
    data::TypeId,
    net::{COMPATIBILITY_DATE, ESI_BASE, now},
};

/// ESI's price list, under [`ESI_BASE`].
const ESI_PRICES: &str = "/markets/prices";

const FUZZWORK_URL: &str = "https://market.fuzzwork.co.uk/aggregates/";
const CEVE_URL: &str = "https://www.ceve-market.org/tqapi/marketstat";
const TYCOON_URL: &str = "https://evetycoon.com/api/v1/market/stats";
/// Types per request to the sources that take a list, so the query string stays a sane length.
const CHUNK: usize = 200;
/// Requests EVE Tycoon gets at once; it is asked one type at a time.
const TYCOON_THREADS: usize = 4;

#[derive(Deserialize)]
struct EsiPrice {
    type_id: TypeId,
    average_price: Option<f64>,
    adjusted_price: Option<f64>,
}

impl Prices {
    /// ESI's whole price list, or a 304 for the one held. Either way prices from other sources
    /// go, so they are asked again.
    pub(super) fn refresh_esi(&mut self, http: &impl Http) -> Result<(), String> {
        let etag = self.etag.clone().filter(|_| !self.by_type.is_empty());
        let mut headers = vec![("X-Compatibility-Date", COMPATIBILITY_DATE)];
        if let Some(etag) = etag.as_deref() {
            headers.push(("If-None-Match", etag));
        }
        let url = esi_url();
        let reply = http.get(&url, &headers)?;
        let expires = now() + reply.max_age.unwrap_or(DEFAULT_MAX_AGE).as_secs();
        if reply.status == 304 {
            let others = std::mem::take(&mut self.sources);
            self.by_type.retain(|t, _| !others.contains_key(t));
        } else {
            let reply = reply.ok(&url)?;
            self.by_type = parse(&reply.body)?;
            self.sources.clear();
            self.etag = reply.etag;
        }
        self.expires = expires;
        self.asked.clear();
        Ok(())
    }
}

/// Ask `source` for the prices of `wanted` at `system`, into `found`. What was found before an
/// error stays found. Like pyfa, sources that can be asked about a hub try the whole market for
/// what the hub has no orders for.
pub(super) fn fetch(
    source: PriceSource,
    system: PriceSystem,
    wanted: &[TypeId],
    http: &impl Http,
    found: &mut HashMap<TypeId, f64>,
) -> Result<(), String> {
    let hub = system.hub();
    let missing = |found: &HashMap<TypeId, f64>| -> Vec<TypeId> {
        wanted
            .iter()
            .copied()
            .filter(|t| !found.contains_key(t))
            .collect()
    };
    match source {
        PriceSource::Fuzzwork => {
            let at_hub = hub.as_ref().map(|h| format!("&station={}", h.station));
            fetch_lists(wanted, found, |ids| {
                let ids: Vec<String> = ids.iter().map(ToString::to_string).collect();
                let url = format!("{FUZZWORK_URL}?types={}", ids.join(","));
                http_body(http, &format!("{url}{}", at_hub.as_deref().unwrap_or("")))
                    .and_then(|body| parse_fuzzwork(&body))
            })?;
            if hub.is_some() {
                fetch_lists(&missing(found), found, |ids| {
                    let ids: Vec<String> = ids.iter().map(ToString::to_string).collect();
                    http_body(http, &format!("{FUZZWORK_URL}?types={}", ids.join(",")))
                        .and_then(|body| parse_fuzzwork(&body))
                })?;
            }
        }
        PriceSource::CeveMarket => {
            let url = |ids: &[TypeId], system: Option<u32>| {
                let ids: Vec<String> = ids.iter().map(|t| format!("typeid={t}")).collect();
                let mut url = format!("{CEVE_URL}?{}", ids.join("&"));
                if let Some(system) = system {
                    url = format!("{url}&usesystem={system}");
                }
                url
            };
            let system = hub.as_ref().map(|h| h.system);
            fetch_lists(wanted, found, |ids| {
                http_body(http, &url(ids, system)).and_then(|body| parse_ceve(&body))
            })?;
            if system.is_some() {
                fetch_lists(&missing(found), found, |ids| {
                    http_body(http, &url(ids, None)).and_then(|body| parse_ceve(&body))
                })?;
            }
        }
        PriceSource::EveTycoon => {
            // EVE Tycoon is asked per type, and about Jita when the hub is the whole universe,
            // as pyfa does.
            let hub = hub
                .or_else(|| PriceSystem::Jita.hub())
                .expect("Jita is a hub");
            let url = |type_id: TypeId| {
                format!(
                    "{TYCOON_URL}/{}/{type_id}?locationId={}",
                    hub.region, hub.station
                )
            };
            fetch_each(wanted, found, |type_id| {
                let reply = http.get(&url(type_id), &[])?;
                if reply.status != 200 {
                    return Ok(None);
                }
                parse_tycoon(&reply.body)
            })?;
        }
        PriceSource::EsiAverage => {
            let url = esi_url();
            let reply = http
                .get(&url, &[("X-Compatibility-Date", COMPATIBILITY_DATE)])?
                .ok(&url)?;
            let all = parse(&reply.body)?;
            found.extend(wanted.iter().filter_map(|t| Some((*t, *all.get(t)?))));
        }
    }
    Ok(())
}

/// Ask a per-type source about each of `wanted`, a few at once, into `found`. After an error
/// no new requests start, and what was found until then stays found.
fn fetch_each(
    wanted: &[TypeId],
    found: &mut HashMap<TypeId, f64>,
    ask: impl Fn(TypeId) -> Result<Option<f64>, String> + Sync,
) -> Result<(), String> {
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    let next = AtomicUsize::new(0);
    let shared = Mutex::new((std::mem::take(found), None::<String>));
    std::thread::scope(|scope| {
        for _ in 0..TYCOON_THREADS.min(wanted.len()) {
            scope.spawn(|| {
                while let Some(&type_id) = wanted.get(next.fetch_add(1, Ordering::Relaxed)) {
                    let answer = ask(type_id);
                    let mut shared = shared.lock().expect("price lock poisoned");
                    match answer {
                        Ok(Some(price)) => {
                            shared.0.insert(type_id, price);
                        }
                        Ok(None) => {}
                        Err(e) => {
                            next.store(wanted.len(), Ordering::Relaxed);
                            shared.1.get_or_insert(e);
                        }
                    }
                }
            });
        }
    });
    let (prices, error) = shared.into_inner().expect("price lock poisoned");
    *found = prices;
    error.map_or(Ok(()), Err)
}

/// Ask a list source about `wanted` a chunk at a time, keeping only types that were asked for.
fn fetch_lists(
    wanted: &[TypeId],
    found: &mut HashMap<TypeId, f64>,
    mut ask: impl FnMut(&[TypeId]) -> Result<HashMap<TypeId, f64>, String>,
) -> Result<(), String> {
    for chunk in wanted.chunks(CHUNK) {
        let prices = ask(chunk)?;
        found.extend(prices.into_iter().filter(|(t, _)| chunk.contains(t)));
    }
    Ok(())
}

fn http_body(http: &impl Http, url: &str) -> Result<Vec<u8>, String> {
    Ok(http.get(url, &[])?.ok(url)?.body)
}

/// Fuzzwork's aggregates by type ID; the 5th percentile sell price, as pyfa takes it. Zero means
/// no orders.
fn parse_fuzzwork(body: &[u8]) -> Result<HashMap<TypeId, f64>, String> {
    #[derive(Deserialize)]
    struct Side {
        percentile: serde_json::Value,
    }
    #[derive(Deserialize)]
    struct Aggregate {
        sell: Side,
    }
    let rows: HashMap<String, Aggregate> =
        serde_json::from_slice(body).map_err(|e| format!("fuzzwork prices: {e}"))?;
    Ok(rows
        .into_iter()
        .filter_map(|(id, row)| {
            // Fuzzwork sends its numbers as strings.
            let price: f64 = match row.sell.percentile {
                serde_json::Value::String(s) => s.parse().ok()?,
                serde_json::Value::Number(n) => n.as_f64()?,
                _ => return None,
            };
            (price > 0.0).then_some((id.parse().ok()?, price))
        })
        .collect())
}

/// ceve-market.org's EVE-Central style XML; the sell percentile, as pyfa takes it. Zero means no
/// orders.
fn parse_ceve(body: &[u8]) -> Result<HashMap<TypeId, f64>, String> {
    fn child<'a, 'i>(node: roxmltree::Node<'a, 'i>, name: &str) -> Option<roxmltree::Node<'a, 'i>> {
        node.children().find(|c| c.has_tag_name(name))
    }
    let text = std::str::from_utf8(body).map_err(|e| format!("ceve-market prices: {e}"))?;
    let doc = roxmltree::Document::parse(text).map_err(|e| format!("ceve-market prices: {e}"))?;
    Ok(doc
        .descendants()
        .filter(|n| n.has_tag_name("type"))
        .filter_map(|n| {
            let id: TypeId = n.attribute("id")?.parse().ok()?;
            let price: f64 = child(child(n, "sell")?, "percentile")?
                .text()?
                .trim()
                .parse()
                .ok()?;
            (price > 0.0).then_some((id, price))
        })
        .collect())
}

/// EVE Tycoon's stats for one type: the average of the cheapest 5% of sell orders. Zero means no
/// orders.
fn parse_tycoon(body: &[u8]) -> Result<Option<f64>, String> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Stats {
        sell_avg_five_percent: Option<f64>,
    }
    let stats: Stats =
        serde_json::from_slice(body).map_err(|e| format!("EVE Tycoon prices: {e}"))?;
    Ok(stats.sell_avg_five_percent.filter(|&p| p > 0.0))
}

fn esi_url() -> String {
    format!("{ESI_BASE}{ESI_PRICES}")
}

/// The average price where ESI has one, the adjusted price otherwise.
fn parse(body: &[u8]) -> Result<HashMap<TypeId, f64>, String> {
    let rows: Vec<EsiPrice> =
        serde_json::from_slice(body).map_err(|e| format!("market prices: {e}"))?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            let price = r.average_price.or(r.adjusted_price)?;
            (price > 0.0).then_some((r.type_id, price))
        })
        .collect())
}
#[cfg(test)]
mod tests {
    use std::{collections::HashSet, sync::Mutex};

    use super::*;
    use crate::{net::Reply, prices::Origin};

    const ESI_URL: &str = "https://esi.evetech.net/markets/prices";

    /// Canned replies by URL prefix, first match wins; every request is logged.
    #[derive(Default)]
    struct Canned {
        replies: Vec<(String, Result<Reply, String>)>,
        log: Mutex<Vec<String>>,
    }

    impl Canned {
        fn reply(mut self, prefix: impl Into<String>, status: u16, body: &str) -> Self {
            let reply = Reply {
                status,
                body: body.as_bytes().to_vec(),
                ..Reply::default()
            };
            self.replies.push((prefix.into(), Ok(reply)));
            self
        }

        fn down(mut self, prefix: &str) -> Self {
            self.replies
                .push((prefix.to_owned(), Err(format!("{prefix}: down"))));
            self
        }

        fn asked(&self, prefix: &str) -> usize {
            self.log
                .lock()
                .expect("log")
                .iter()
                .filter(|u| u.starts_with(prefix))
                .count()
        }
    }

    impl Http for Canned {
        fn get(&self, url: &str, _: &[(&str, &str)]) -> Result<Reply, String> {
            self.log.lock().expect("log").push(url.to_owned());
            self.replies
                .iter()
                .find(|(prefix, _)| url.starts_with(prefix.as_str()))
                .map_or_else(|| Err(format!("{url}: no reply")), |(_, r)| r.clone())
        }
    }

    const JITA: Origin = Origin {
        source: PriceSource::Fuzzwork,
        system: PriceSystem::Jita,
    };

    #[test]
    fn falls_back_through_the_sources_in_order() {
        let http = Canned::default()
            .down(FUZZWORK_URL)
            .reply(
                CEVE_URL,
                200,
                r#"<evec_api><marketstat><type id="34"><sell><percentile>4.11</percentile></sell></type></marketstat></evec_api>"#,
            )
            .reply(
                format!("{TYCOON_URL}/10000002/35?"),
                200,
                r#"{"sellAvgFivePercent": 9.5}"#,
            )
            .reply(TYCOON_URL, 404, "")
            .reply(
                ESI_URL,
                200,
                r#"[{"type_id": 36, "average_price": 100.0}, {"type_id": 99, "average_price": 1.0}]"#,
            );
        let types: HashSet<TypeId> = [34, 35, 36, 37].into();
        let prices = Prices::default()
            .fetched(JITA, &types, &http)
            .expect("some sources answered");
        assert_eq!(prices.get(34), Some(4.11));
        assert_eq!(prices.get(35), Some(9.5));
        assert_eq!(prices.get(36), Some(100.0));
        assert_eq!(prices.get(37), None);
        assert_eq!(prices.get(99), None, "only what was asked for is kept");
        assert_eq!(prices.source_of(34), Some(PriceSource::CeveMarket));
        assert_eq!(prices.source_of(35), Some(PriceSource::EveTycoon));
        assert_eq!(prices.source_of(36), Some(PriceSource::EsiAverage));
        assert_eq!(prices.source_label(35).as_deref(), Some("EVE Tycoon, Jita"));
        assert_eq!(
            prices.source_label(36).as_deref(),
            Some("ESI average price")
        );
        // Fuzzwork failed outright, so its universe-wide retry never happened.
        assert_eq!(http.asked(FUZZWORK_URL), 1);
        // ceve-market is asked at Jita, then everywhere for what Jita lacks.
        assert_eq!(http.asked(CEVE_URL), 2);
        // EVE Tycoon is asked per type, only about what ceve-market lacked.
        assert_eq!(http.asked(TYCOON_URL), 3);
        assert!(!prices.is_stale_for(JITA, &types), "every type was asked");
    }

    #[test]
    fn the_chosen_source_answers_first() {
        let http = Canned::default().reply(
            FUZZWORK_URL,
            200,
            r#"{"34": {"sell": {"percentile": "4.5"}}, "35": {"sell": {"percentile": "7"}}}"#,
        );
        let types: HashSet<TypeId> = [34, 35].into();
        let prices = Prices::default()
            .fetched(JITA, &types, &http)
            .expect("fuzzwork answered");
        assert_eq!(prices.get(35), Some(7.0));
        assert_eq!(prices.source_of(34), Some(PriceSource::Fuzzwork));
        let log = http.log.lock().expect("log");
        assert_eq!(log.len(), 1);
        assert!(log[0].ends_with("?types=34,35&station=60003760"));
    }

    #[test]
    fn fails_when_every_source_fails() {
        let http = Canned::default();
        let types: HashSet<TypeId> = [34].into();
        assert!(Prices::default().fetched(JITA, &types, &http).is_err());
    }

    #[test]
    fn esi_first_then_hubs_for_what_it_lacks() {
        let http = Canned::default()
            .reply(ESI_URL, 200, r#"[{"type_id": 34, "average_price": 5.0}]"#)
            .reply(
                FUZZWORK_URL,
                200,
                r#"{"35": {"sell": {"percentile": "2"}}}"#,
            );
        let origin = Origin {
            source: PriceSource::EsiAverage,
            system: PriceSystem::Jita,
        };
        let prices = Prices::default()
            .fetched(origin, &[34, 35].into(), &http)
            .expect("both answered");
        assert_eq!(prices.source_of(34), Some(PriceSource::EsiAverage));
        assert_eq!(prices.source_of(35), Some(PriceSource::Fuzzwork));
        assert_eq!(
            prices.source_label(35).as_deref(),
            Some("Fuzzwork market, Jita")
        );
        assert_eq!(http.asked(ESI_URL), 1);
    }

    #[test]
    fn prefers_average_and_skips_unpriced() {
        let body = br#"[
            {"type_id": 34, "average_price": 4.2, "adjusted_price": 3.9},
            {"type_id": 35, "adjusted_price": 10.0},
            {"type_id": 36},
            {"type_id": 37, "average_price": 0.0}
        ]"#;
        let prices = parse(body).expect("the sample parses");
        assert_eq!(prices.get(&34), Some(&4.2));
        assert_eq!(prices.get(&35), Some(&10.0));
        assert!(!prices.contains_key(&36));
        assert!(!prices.contains_key(&37));
    }

    #[test]
    fn reads_fuzzwork_aggregates() {
        let body = br#"{
            "34": {"buy": {"percentile": "4.0"}, "sell": {"percentile": "4.51"}},
            "35": {"buy": {"percentile": "0"}, "sell": {"percentile": "0"}}
        }"#;
        let prices = parse_fuzzwork(body).expect("the sample parses");
        assert_eq!(prices.get(&34), Some(&4.51));
        assert!(!prices.contains_key(&35));
    }

    #[test]
    fn reads_ceve_market_stats() {
        let body = br#"<?xml version="1.0" encoding="utf-8"?><evec_api method="marketstat_xml" version="2.0"><marketstat><type id="34"><buy><percentile>3.83</percentile></buy><sell><volume>5</volume><percentile>4.11</percentile></sell></type><type id="35"><sell><percentile>0.00</percentile></sell></type></marketstat></evec_api>"#;
        let prices = parse_ceve(body).expect("the sample parses");
        assert_eq!(prices.get(&34), Some(&4.11));
        assert!(!prices.contains_key(&35));
    }

    #[test]
    fn reads_eve_tycoon_stats() {
        let body = br#"{"buyVolume":8794931176,"sellAvgFivePercent":4.12,"minSell":4.12}"#;
        assert_eq!(parse_tycoon(body), Ok(Some(4.12)));
        assert_eq!(parse_tycoon(br#"{"sellAvgFivePercent":0}"#), Ok(None));
    }

    #[test]
    fn another_source_or_new_types_are_stale() {
        let types: HashSet<TypeId> = [34].into();
        let prices = Prices {
            expires: u64::MAX,
            asked: types.clone(),
            origin: JITA,
            ..Prices::default()
        };
        assert!(!prices.is_stale_for(JITA, &types));
        let amarr = Origin {
            system: PriceSystem::Amarr,
            ..JITA
        };
        assert!(prices.is_stale_for(amarr, &types));
        assert!(prices.is_stale_for(JITA, &[34, 35].into()));
    }
}
