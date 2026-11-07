//! EVE SSO and the authenticated ESI routes pyfa uses (service/esi.py, service/esiAccess.py):
//! logging characters in with the PKCE flow for native apps, keeping their refresh tokens, and
//! reading skills, implants and fittings or writing fittings.
//!
//! Everything here blocks; the UI runs it on background threads.

pub mod accounts;
pub mod api;
pub mod sso;

pub use accounts::{Account, Accounts};

/// efa's own application at developers.eveonline.com, registered with the callback
/// `http://localhost:6461/callback` and [`SCOPES`]. A client ID is public; logging in uses PKCE,
/// so no client secret is involved anywhere.
pub const CLIENT_ID: &str = "fe05a0cb454843e6b8d4b73613445011";

/// What efa asks a character for. The app registered at developers.eveonline.com needs every
/// one of them, or the login fails.
pub const SCOPES: [&str; 4] = [
    "esi-skills.read_skills.v1",
    "esi-clones.read_implants.v1",
    "esi-fittings.read_fittings.v1",
    "esi-fittings.write_fittings.v1",
];

pub const SKILLS_SCOPE: &str = SCOPES[0];
pub const IMPLANTS_SCOPE: &str = SCOPES[1];
pub const READ_FITTINGS_SCOPE: &str = SCOPES[2];
pub const WRITE_FITTINGS_SCOPE: &str = SCOPES[3];

/// The port the login callback listens on unless the preferences say otherwise.
pub const DEFAULT_PORT: u16 = 6461;

/// Where EVE SSO sends the browser back to; the app has to be registered with exactly this.
pub fn callback_url(port: u16) -> String {
    format!("http://localhost:{port}/callback")
}

/// Seconds since the epoch.
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}
