//! What every request efa makes shares: who is asking, ESI's address and the behaviour date it
//! is pinned to, the proxy-aware client, and a small GET abstraction that tests can answer.

use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::preferences::{Proxy, ProxyMode};

/// Sent with every request, so CCP and the price sites can tell efa apart and reach its authors.
pub const USER_AGENT: &str = concat!(
    "efa/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/NicolasKion/efa)"
);

pub const ESI_BASE: &str = "https://esi.evetech.net";

/// ESI behaviour efa is written against. Bump on purpose after checking what changed.
pub const COMPATIBILITY_DATE: &str = "2026-08-18";

/// How long to wait for a server to accept the connection before giving up on it.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Seconds since the epoch.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl Proxy {
    /// A blocking client that goes through this proxy, with efa's user agent.
    pub fn client(&self) -> Result<reqwest::blocking::Client, String> {
        let mut builder = reqwest::blocking::Client::builder()
            .user_agent(USER_AGENT)
            .connect_timeout(CONNECT_TIMEOUT);
        match self.mode {
            ProxyMode::System => {}
            ProxyMode::None => builder = builder.no_proxy(),
            ProxyMode::Manual => {
                let address = self.address.trim();
                let url = if address.contains("://") {
                    address.to_owned()
                } else {
                    format!("http://{address}")
                };
                let mut proxy = reqwest::Proxy::all(&url).map_err(|e| format!("proxy: {e}"))?;
                if !self.username.is_empty() {
                    proxy = proxy.basic_auth(&self.username, &self.password);
                }
                builder = builder.proxy(proxy);
            }
        }
        builder.build().map_err(|e| e.to_string())
    }
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
    /// The reply when its status is a success, an error naming `url` otherwise.
    pub fn ok(self, url: &str) -> Result<Self, String> {
        if (200..300).contains(&self.status) {
            Ok(self)
        } else {
            Err(format!("{url}: HTTP {}", self.status))
        }
    }
}

/// Something that can do a GET; a reqwest client, or canned replies in tests. Shared between
/// threads, since some sources are asked several types at once.
pub trait Http: Sync {
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

/// How long a response may be reused, from its Cache-Control max-age.
pub fn max_age(cache_control: &str) -> Option<Duration> {
    cache_control
        .split(',')
        .find_map(|part| part.trim().strip_prefix("max-age="))
        .and_then(|secs| secs.parse().ok())
        .map(Duration::from_secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_max_age() {
        assert_eq!(
            max_age("public, max-age=1234"),
            Some(Duration::from_secs(1234))
        );
        assert_eq!(max_age("public"), None);
    }

    #[test]
    fn the_user_agent_names_the_version() {
        assert!(USER_AGENT.starts_with(&format!("efa/{} ", env!("CARGO_PKG_VERSION"))));
    }
}
