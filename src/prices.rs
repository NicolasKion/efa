//! Item prices from ESI's public market prices route, which lists every traded type in one
//! response. The result is kept on disk with its `ETag` and expiry, so a start within the hour
//! makes no request at all and a later one usually gets a cheap 304.
//!
//! The price is CCP's universe-wide average (what the game shows as estimated price), not a
//! Jita sell price. pyfa's default source, Fuzzwork's hub sell prices, can be picked instead in
//! the preferences; it is asked per type, for the types the saved fits use.

use std::{
    collections::{HashMap, HashSet},
    path::Path,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

use crate::data::TypeId;

const URL: &str = "https://esi.evetech.net/markets/prices";
/// ESI behaviour gyfa is written against. Bump on purpose after checking what changed.
const COMPATIBILITY_DATE: &str = "2026-08-18";
const AGENT: &str = "gyfa/0.1 (EVE Online fitting tool)";
const CACHE_FILE: &str = "prices.json";
/// ESI caches this route for an hour; used when the response does not say.
const DEFAULT_MAX_AGE: Duration = Duration::from_secs(3600);
const FUZZWORK_URL: &str = "https://market.fuzzwork.co.uk/aggregates/";
/// Types per Fuzzwork request, so the query string stays a sane length.
const FUZZWORK_CHUNK: usize = 200;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Prices {
    /// Seconds since the epoch after which ESI may have newer data.
    expires: u64,
    etag: Option<String>,
    #[serde(rename = "prices")]
    by_type: HashMap<TypeId, f64>,
    /// Where these prices came from; prices from another source count as stale.
    #[serde(default)]
    origin: Origin,
    /// Types a per-type source was asked about, priced or not, so they are not asked again
    /// before the prices expire.
    #[serde(default)]
    asked: HashSet<TypeId>,
}

/// Where prices come from, pyfa's price source preference.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PriceSource {
    /// CCP's universe-wide average from ESI, every traded type in one request.
    #[default]
    EsiAverage,
    /// Fuzzwork's market aggregates: the 5th percentile sell price at a trade hub, pyfa's
    /// default source.
    Fuzzwork,
}

impl PriceSource {
    pub const ALL: [Self; 2] = [Self::EsiAverage, Self::Fuzzwork];

    pub fn label(self) -> &'static str {
        match self {
            Self::EsiAverage => "ESI average price",
            Self::Fuzzwork => "Fuzzwork market",
        }
    }

    /// Whether the market system matters to this source.
    pub fn uses_system(self) -> bool {
        self == Self::Fuzzwork
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

    /// The hub station Fuzzwork aggregates over, the ones pyfa picks.
    fn station(self) -> Option<u32> {
        match self {
            Self::Jita => Some(60_003_760),
            Self::Amarr => Some(60_008_494),
            Self::Dodixie => Some(60_011_866),
            Self::Rens => Some(60_004_588),
            Self::Hek => Some(60_005_686),
            Self::Universe => None,
        }
    }
}

/// The source and system a set of prices was fetched from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Origin {
    pub source: PriceSource,
    pub system: PriceSystem,
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

    pub fn is_empty(&self) -> bool {
        self.by_type.is_empty()
    }

    /// Whether ESI may have something newer than this.
    pub fn is_stale(&self) -> bool {
        now() >= self.expires
    }

    /// Whether these prices need fetching for `origin`: they expired, came from elsewhere, or a
    /// per-type source was never asked about one of `types`.
    pub fn is_stale_for(&self, origin: Origin, types: &HashSet<TypeId>) -> bool {
        if self.origin != origin || self.is_stale() {
            return true;
        }
        match origin.source {
            PriceSource::EsiAverage => false,
            PriceSource::Fuzzwork => !types.is_subset(&self.asked),
        }
    }

    /// Fetch prices from `origin` when [`Self::is_stale_for`] says so, through `http`, and keep
    /// them on disk. `types` are what a per-type source is asked about. Blocking.
    pub fn refresh_from(
        self,
        cache_dir: &Path,
        origin: Origin,
        types: &HashSet<TypeId>,
        http: &reqwest::blocking::Client,
    ) -> Result<Self, String> {
        // Switching sources starts over rather than mixing two kinds of price.
        let current = if self.origin == origin {
            self
        } else {
            Self {
                origin,
                ..Self::default()
            }
        };
        match origin.source {
            PriceSource::EsiAverage => current.refresh_with(cache_dir, http),
            PriceSource::Fuzzwork => {
                let refreshed = current.refresh_fuzzwork(origin.system, types, http)?;
                refreshed.store(cache_dir);
                Ok(refreshed)
            }
        }
    }

    fn refresh_fuzzwork(
        self,
        system: PriceSystem,
        types: &HashSet<TypeId>,
        http: &reqwest::blocking::Client,
    ) -> Result<Self, String> {
        let expired = self.is_stale();
        let (mut by_type, mut asked) = if expired {
            (HashMap::new(), HashSet::new())
        } else {
            (self.by_type, self.asked)
        };
        let mut wanted: Vec<TypeId> = types.difference(&asked).copied().collect();
        wanted.sort_unstable();
        for chunk in wanted.chunks(FUZZWORK_CHUNK) {
            let ids: Vec<String> = chunk.iter().map(ToString::to_string).collect();
            // IDs and station numbers need no URL escaping.
            let mut url = format!("{FUZZWORK_URL}?types={}", ids.join(","));
            if let Some(station) = system.station() {
                url = format!("{url}&station={station}");
            }
            let body = http
                .get(&url)
                .send()
                .and_then(reqwest::blocking::Response::error_for_status)
                .and_then(reqwest::blocking::Response::bytes)
                .map_err(|e| format!("{FUZZWORK_URL}: {e}"))?;
            by_type.extend(parse_fuzzwork(&body)?);
            asked.extend(chunk);
        }
        Ok(Self {
            expires: if expired {
                now() + DEFAULT_MAX_AGE.as_secs()
            } else {
                self.expires
            },
            etag: None,
            by_type,
            origin: Origin {
                source: PriceSource::Fuzzwork,
                system,
            },
            asked,
        })
    }

    /// Fetch newer prices if these are stale, and keep them on disk. Blocking.
    pub fn refresh(self, cache_dir: &Path) -> Result<Self, String> {
        let http = reqwest::blocking::Client::builder()
            .user_agent(AGENT)
            .build()
            .map_err(|e| e.to_string())?;
        self.refresh_with(cache_dir, &http)
    }

    fn refresh_with(
        self,
        cache_dir: &Path,
        http: &reqwest::blocking::Client,
    ) -> Result<Self, String> {
        if !self.is_stale() {
            return Ok(self);
        }
        let mut request = http
            .get(URL)
            .header("X-Compatibility-Date", COMPATIBILITY_DATE);
        if let Some(etag) = self.etag.as_deref().filter(|_| !self.by_type.is_empty()) {
            request = request.header("If-None-Match", etag);
        }
        let response = request.send().map_err(|e| format!("{URL}: {e}"))?;
        let expires = now() + max_age(response.headers()).as_secs();
        let refreshed = if response.status() == reqwest::StatusCode::NOT_MODIFIED {
            Self { expires, ..self }
        } else {
            let response = response
                .error_for_status()
                .map_err(|e| format!("{URL}: {e}"))?;
            let etag = response
                .headers()
                .get(reqwest::header::ETAG)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let body = response.bytes().map_err(|e| format!("{URL}: {e}"))?;
            Self {
                expires,
                etag,
                by_type: parse(&body)?,
                origin: Origin::default(),
                asked: HashSet::new(),
            }
        };
        refreshed.store(cache_dir);
        Ok(refreshed)
    }

    fn store(&self, cache_dir: &Path) {
        let Ok(json) = serde_json::to_vec(self) else {
            return;
        };
        let _ = std::fs::create_dir_all(cache_dir);
        let _ = std::fs::write(cache_dir.join(CACHE_FILE), json);
    }
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

/// How long the response may be reused, from its Cache-Control max-age.
fn max_age(headers: &reqwest::header::HeaderMap) -> Duration {
    headers
        .get(reqwest::header::CACHE_CONTROL)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| {
            v.split(',')
                .find_map(|part| part.trim().strip_prefix("max-age="))
                .and_then(|secs| secs.parse().ok())
        })
        .map_or(DEFAULT_MAX_AGE, Duration::from_secs)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn another_source_or_new_types_are_stale() {
        let types: HashSet<TypeId> = [34].into();
        let jita = Origin {
            source: PriceSource::Fuzzwork,
            system: PriceSystem::Jita,
        };
        let prices = Prices {
            expires: u64::MAX,
            asked: types.clone(),
            origin: jita,
            ..Prices::default()
        };
        assert!(!prices.is_stale_for(jita, &types));
        let amarr = Origin {
            system: PriceSystem::Amarr,
            ..jita
        };
        assert!(prices.is_stale_for(amarr, &types));
        assert!(prices.is_stale_for(jita, &[34, 35].into()));
    }

    #[test]
    fn reads_max_age() {
        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(
            reqwest::header::CACHE_CONTROL,
            "public, max-age=1234".parse().expect("valid header"),
        );
        assert_eq!(max_age(&headers), Duration::from_secs(1234));
        assert_eq!(max_age(&reqwest::header::HeaderMap::new()), DEFAULT_MAX_AGE);
    }
}
