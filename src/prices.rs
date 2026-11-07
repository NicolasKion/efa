//! Item prices from pyfa's market sources (service/price.py and service/marketSources), plus
//! ESI's public market prices route. What was fetched is kept on disk with its expiry, and for
//! ESI its `ETag`, so a start within the hour makes no request at all.
//!
//! Like pyfa, the chosen source is asked first and whatever it cannot price is asked of the
//! others in turn, and each price remembers where it came from. pyfa's eve-marketdata.com
//! source is gone (pyfa no longer registers it, and the site is down), and its Serenity source
//! has no use without Serenity data, so neither is here.

use std::{
    collections::{HashMap, HashSet},
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::data::TypeId;

pub mod optimize;

const ESI_URL: &str = "https://esi.evetech.net/markets/prices";
/// ESI behaviour gyfa is written against. Bump on purpose after checking what changed.
const COMPATIBILITY_DATE: &str = "2026-08-18";
const AGENT: &str = "gyfa/0.1 (EVE Online fitting tool)";
const CACHE_FILE: &str = "prices.json";
/// How long prices are kept; ESI says so itself, the others are refreshed this often.
const DEFAULT_MAX_AGE: Duration = Duration::from_secs(3600);
const FUZZWORK_URL: &str = "https://market.fuzzwork.co.uk/aggregates/";
const CEVE_URL: &str = "https://www.ceve-market.org/tqapi/marketstat";
const TYCOON_URL: &str = "https://evetycoon.com/api/v1/market/stats";
/// Types per request to the sources that take a list, so the query string stays a sane length.
const CHUNK: usize = 200;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Prices {
    /// Seconds since the epoch after which the prices may be out of date.
    expires: u64,
    /// ESI's `ETag` for its price list, when that list is what `by_type` holds.
    etag: Option<String>,
    #[serde(rename = "prices")]
    by_type: HashMap<TypeId, f64>,
    /// Prices that came from another source than `origin`'s, by the source they came from.
    #[serde(default)]
    sources: HashMap<TypeId, PriceSource>,
    /// The source and hub asked first; prices for another one count as stale.
    #[serde(default)]
    origin: Origin,
    /// Types that were asked about, priced or not, so they are not asked again before the
    /// prices expire.
    #[serde(default)]
    asked: HashSet<TypeId>,
}

/// Where prices come from, pyfa's price source preference.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PriceSource {
    /// Fuzzwork's market aggregates: the 5th percentile sell price at a trade hub, pyfa's
    /// default source.
    #[default]
    Fuzzwork,
    /// ceve-market.org's Tranquility statistics: the sell percentile in a hub's system.
    CeveMarket,
    /// EVE Tycoon's market stats: the average of the cheapest 5% of sell orders at a hub.
    EveTycoon,
    /// CCP's universe-wide average from ESI, every traded type in one request.
    EsiAverage,
}

impl PriceSource {
    /// Every source, in the order pyfa falls back through them, ESI last.
    pub const ALL: [Self; 4] = [
        Self::Fuzzwork,
        Self::CeveMarket,
        Self::EveTycoon,
        Self::EsiAverage,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Fuzzwork => "Fuzzwork market",
            Self::CeveMarket => "ceve-market.org",
            Self::EveTycoon => "EVE Tycoon",
            Self::EsiAverage => "ESI average price",
        }
    }

    /// Whether the market system matters to this source.
    pub fn uses_system(self) -> bool {
        self != Self::EsiAverage
    }

    /// `first`, then the other sources in pyfa's order.
    pub fn fallback_order(first: Self) -> impl Iterator<Item = Self> {
        std::iter::once(first).chain(Self::ALL.into_iter().filter(move |&s| s != first))
    }
}

/// The trade hub prices come from, pyfa's list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PriceSystem {
    #[default]
    Jita,
    Amarr,
    Dodixie,
    Rens,
    Hek,
    Universe,
}

/// Where a hub is: solar system, region and the station whose orders count.
struct Hub {
    system: u32,
    region: u32,
    station: u32,
}

impl PriceSystem {
    pub const ALL: [Self; 6] = [
        Self::Jita,
        Self::Amarr,
        Self::Dodixie,
        Self::Rens,
        Self::Hek,
        Self::Universe,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Jita => "Jita",
            Self::Amarr => "Amarr",
            Self::Dodixie => "Dodixie",
            Self::Rens => "Rens",
            Self::Hek => "Hek",
            Self::Universe => "Universe",
        }
    }

    /// The hub, with the stations pyfa's sources pick.
    fn hub(self) -> Option<Hub> {
        let (system, region, station) = match self {
            Self::Jita => (30_000_142, 10_000_002, 60_003_760),
            Self::Amarr => (30_002_187, 10_000_043, 60_008_494),
            Self::Dodixie => (30_002_659, 10_000_032, 60_011_866),
            Self::Rens => (30_002_510, 10_000_030, 60_004_588),
            Self::Hek => (30_002_053, 10_000_042, 60_005_686),
            Self::Universe => return None,
        };
        Some(Hub {
            system,
            region,
            station,
        })
    }
}

/// The source asked first and the hub prices are fetched for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    pub source: PriceSource,
    pub system: PriceSystem,
}

/// What came back from a GET.
#[derive(Debug, Clone, Default)]
pub struct Reply {
    pub status: u16,
    pub etag: Option<String>,
    /// From Cache-Control.
    pub max_age: Option<Duration>,
    pub body: Vec<u8>,
}

impl Reply {
    fn ok(self, url: &str) -> Result<Self, String> {
        if (200..300).contains(&self.status) {
            Ok(self)
        } else {
            Err(format!("{url}: HTTP {}", self.status))
        }
    }
}

/// Something that can do a GET; a reqwest client, or canned replies in tests.
pub trait Http {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Reply, String>;
}

impl Http for reqwest::blocking::Client {
    fn get(&self, url: &str, headers: &[(&str, &str)]) -> Result<Reply, String> {
        let mut request = reqwest::blocking::Client::get(self, url);
        for &(name, value) in headers {
            request = request.header(name, value);
        }
        let response = request.send().map_err(|e| format!("{url}: {e}"))?;
        let header = |name| {
            response
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        let etag = header(reqwest::header::ETAG);
        let max_age = header(reqwest::header::CACHE_CONTROL).and_then(|v| max_age(&v));
        let status = response.status().as_u16();
        let body = response.bytes().map_err(|e| format!("{url}: {e}"))?;
        Ok(Reply {
            status,
            etag,
            max_age,
            body: body.to_vec(),
        })
    }
}

#[derive(Deserialize)]
struct EsiPrice {
    type_id: TypeId,
    average_price: Option<f64>,
    adjusted_price: Option<f64>,
}

impl Prices {
    /// What was fetched last time, whatever its age; empty when nothing was.
    pub fn cached(cache_dir: &Path) -> Self {
        std::fs::read(cache_dir.join(CACHE_FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default()
    }

    /// Forget every price and the copy on disk, pyfa's "Delete All Prices". The next refresh
    /// fetches them again.
    pub fn delete_cached(cache_dir: &Path) -> Result<(), String> {
        match std::fs::remove_file(cache_dir.join(CACHE_FILE)) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        }
    }

    pub fn get(&self, type_id: TypeId) -> Option<f64> {
        self.by_type.get(&type_id).copied()
    }

    /// The source a type's price came from.
    pub fn source_of(&self, type_id: TypeId) -> Option<PriceSource> {
        self.by_type.contains_key(&type_id).then(|| {
            self.sources
                .get(&type_id)
                .copied()
                .unwrap_or(self.origin.source)
        })
    }

    /// Where a type's price came from, for a tooltip: the source, and the hub where it has one.
    pub fn source_label(&self, type_id: TypeId) -> Option<String> {
        self.source_of(type_id).map(|source| self.describe(source))
    }

    /// A source with the hub it was asked about, "Fuzzwork market, Jita".
    pub fn describe(&self, source: PriceSource) -> String {
        if source.uses_system() {
            format!("{}, {}", source.label(), self.origin.system.label())
        } else {
            source.label().to_owned()
        }
    }

    /// Where the prices of `types` came from, one line per source, the most used first, e.g.
    /// "Fuzzwork market, Jita: 12 items". Empty when none of them has a price.
    pub fn sources_summary(&self, types: impl IntoIterator<Item = TypeId>) -> String {
        let unique: HashSet<TypeId> = types.into_iter().collect();
        let mut counts: Vec<(PriceSource, usize)> = Vec::new();
        for source in unique.into_iter().filter_map(|t| self.source_of(t)) {
            match counts.iter_mut().find(|(s, _)| *s == source) {
                Some((_, n)) => *n += 1,
                None => counts.push((source, 1)),
            }
        }
        counts.sort_by_key(|&(s, n)| (std::cmp::Reverse(n), s.label()));
        counts
            .into_iter()
            .map(|(s, n)| {
                let noun = if n == 1 { "item" } else { "items" };
                format!("{}: {n} {noun}", self.describe(s))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn is_empty(&self) -> bool {
        self.by_type.is_empty()
    }

    /// Whether these prices may be out of date.
    pub fn is_stale(&self) -> bool {
        now() >= self.expires
    }

    /// Whether these prices need fetching for `origin`: they expired, came from elsewhere, or
    /// one of `types` was never asked about.
    pub fn is_stale_for(&self, origin: Origin, types: &HashSet<TypeId>) -> bool {
        self.origin != origin || self.is_stale() || !types.is_subset(&self.asked)
    }

    /// Fetch prices when [`Self::is_stale_for`] says so, through `http`, and keep them on disk.
    /// `origin`'s source is asked first; types it has no price for are asked of the others in
    /// pyfa's order. Fails only when every source that was tried failed. Blocking.
    pub fn refresh_from(
        self,
        cache_dir: &Path,
        origin: Origin,
        types: &HashSet<TypeId>,
        http: &impl Http,
    ) -> Result<Self, String> {
        let refreshed = self.fetched(origin, types, http)?;
        refreshed.store(cache_dir);
        Ok(refreshed)
    }

    /// [`Self::refresh_from`] without the disk.
    fn fetched(
        self,
        origin: Origin,
        types: &HashSet<TypeId>,
        http: &impl Http,
    ) -> Result<Self, String> {
        // Switching sources or hubs starts over rather than mixing two kinds of price.
        let mut current = if self.origin == origin {
            self
        } else {
            Self {
                origin,
                ..Self::default()
            }
        };
        let mut errors: Vec<String> = Vec::new();
        let mut answered = false;
        if current.is_stale() {
            if origin.source == PriceSource::EsiAverage {
                match current.refresh_esi(http) {
                    Ok(()) => answered = true,
                    Err(e) => errors.push(e),
                }
            } else {
                current = Self {
                    origin,
                    expires: now() + DEFAULT_MAX_AGE.as_secs(),
                    ..Self::default()
                };
            }
        }
        let asking: Vec<TypeId> = {
            let mut asking: Vec<TypeId> = types.difference(&current.asked).copied().collect();
            asking.sort_unstable();
            asking
        };
        let mut wanted: Vec<TypeId> = asking
            .iter()
            .copied()
            .filter(|t| !current.by_type.contains_key(t))
            .collect();
        for source in PriceSource::fallback_order(origin.source) {
            if wanted.is_empty() {
                break;
            }
            // ESI's list was fetched above when it is the main source.
            if source == PriceSource::EsiAverage && origin.source == source {
                continue;
            }
            let mut found = HashMap::new();
            let result = fetch(source, origin.system, &wanted, http, &mut found);
            wanted.retain(|t| !found.contains_key(t));
            for (type_id, price) in found {
                current.by_type.insert(type_id, price);
                if source == origin.source {
                    current.sources.remove(&type_id);
                } else {
                    current.sources.insert(type_id, source);
                }
            }
            match result {
                Ok(()) => answered = true,
                Err(e) => errors.push(e),
            }
        }
        if !answered && !errors.is_empty() {
            return Err(errors.join("; "));
        }
        for e in &errors {
            eprintln!("prices: {e}");
        }
        current.asked.extend(asking);
        Ok(current)
    }

    /// ESI's whole price list, or a 304 for the one held. Either way prices from other sources
    /// go, so they are asked again.
    fn refresh_esi(&mut self, http: &impl Http) -> Result<(), String> {
        let etag = self.etag.clone().filter(|_| !self.by_type.is_empty());
        let mut headers = vec![("X-Compatibility-Date", COMPATIBILITY_DATE)];
        if let Some(etag) = etag.as_deref() {
            headers.push(("If-None-Match", etag));
        }
        let reply = http.get(ESI_URL, &headers)?;
        let expires = now() + reply.max_age.unwrap_or(DEFAULT_MAX_AGE).as_secs();
        if reply.status == 304 {
            let others = std::mem::take(&mut self.sources);
            self.by_type.retain(|t, _| !others.contains_key(t));
        } else {
            let reply = reply.ok(ESI_URL)?;
            self.by_type = parse(&reply.body)?;
            self.sources.clear();
            self.etag = reply.etag;
        }
        self.expires = expires;
        self.asked.clear();
        Ok(())
    }

    fn store(&self, cache_dir: &Path) {
        let Ok(json) = serde_json::to_vec(self) else {
            return;
        };
        let _ = std::fs::create_dir_all(cache_dir);
        let _ = std::fs::write(cache_dir.join(CACHE_FILE), json);
    }
}

/// Ask `source` for the prices of `wanted` at `system`, into `found`. What was found before an
/// error stays found. Like pyfa, sources that can be asked about a hub try the whole market for
/// what the hub has no orders for.
fn fetch(
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
            for &type_id in wanted {
                let url = format!(
                    "{TYCOON_URL}/{}/{type_id}?locationId={}",
                    hub.region, hub.station
                );
                let reply = http.get(&url, &[])?;
                if reply.status != 200 {
                    continue;
                }
                if let Some(price) = parse_tycoon(&reply.body)? {
                    found.insert(type_id, price);
                }
            }
        }
        PriceSource::EsiAverage => {
            let reply = http
                .get(ESI_URL, &[("X-Compatibility-Date", COMPATIBILITY_DATE)])?
                .ok(ESI_URL)?;
            let all = parse(&reply.body)?;
            found.extend(wanted.iter().filter_map(|t| Some((*t, *all.get(t)?))));
        }
    }
    Ok(())
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

/// The user agent gyfa sends, for clients built elsewhere.
pub fn user_agent() -> &'static str {
    AGENT
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

/// How long a response may be reused, from its Cache-Control max-age.
fn max_age(cache_control: &str) -> Option<Duration> {
    cache_control
        .split(',')
        .find_map(|part| part.trim().strip_prefix("max-age="))
        .and_then(|secs| secs.parse().ok())
        .map(Duration::from_secs)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    /// Canned replies by URL prefix, first match wins; every request is logged.
    #[derive(Default)]
    struct Canned {
        replies: Vec<(String, Result<Reply, String>)>,
        log: RefCell<Vec<String>>,
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
                .borrow()
                .iter()
                .filter(|u| u.starts_with(prefix))
                .count()
        }
    }

    impl Http for Canned {
        fn get(&self, url: &str, _: &[(&str, &str)]) -> Result<Reply, String> {
            self.log.borrow_mut().push(url.to_owned());
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
        assert_eq!(http.log.borrow().len(), 1);
        assert!(http.log.borrow()[0].ends_with("?types=34,35&station=60003760"));
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

    #[test]
    fn reads_max_age() {
        assert_eq!(
            max_age("public, max-age=1234"),
            Some(Duration::from_secs(1234))
        );
        assert_eq!(max_age("public"), None);
    }
}
