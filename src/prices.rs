//! Item prices from ESI's public market prices route, which lists every traded type in one
//! response. The result is kept on disk with its `ETag` and expiry, so a start within the hour
//! makes no request at all and a later one usually gets a cheap 304.
//!
//! The price is CCP's universe-wide average (what the game shows as estimated price), not a
//! Jita sell price; pyfa's default source differs from it.

use std::{
    collections::HashMap,
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

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Prices {
    /// Seconds since the epoch after which ESI may have newer data.
    expires: u64,
    etag: Option<String>,
    #[serde(rename = "prices")]
    by_type: HashMap<TypeId, f64>,
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

    /// Fetch newer prices if these are stale, and keep them on disk. Blocking.
    pub fn refresh(self, cache_dir: &Path) -> Result<Self, String> {
        if !self.is_stale() {
            return Ok(self);
        }
        let http = reqwest::blocking::Client::builder()
            .user_agent(AGENT)
            .build()
            .map_err(|e| e.to_string())?;
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
