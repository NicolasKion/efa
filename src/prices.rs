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
    time::Duration,
};

use serde::{Deserialize, Serialize};

pub use crate::net::{Http, Reply};
use crate::{data::TypeId, net::now};

mod cache;
pub mod optimize;
mod sources;

use sources::fetch;

/// How long prices are kept; ESI says so itself, the others are refreshed this often.
const DEFAULT_MAX_AGE: Duration = Duration::from_secs(3600);

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

impl Prices {
    /// A fixed price list, for tests and offline use; nothing about it is ever stale.
    pub fn fixed(prices: impl IntoIterator<Item = (TypeId, f64)>) -> Self {
        Self {
            by_type: prices.into_iter().collect(),
            ..Self::default()
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
}
