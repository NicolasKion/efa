//! EVE SSO's authorization code flow with PKCE, the one for apps that cannot keep a secret:
//! the browser logs in, comes back to a listener on localhost with a code, and the code buys an
//! access token (a JWT, checked here against the SSO's keys) and a refresh token.

use std::{
    io::{Read as _, Write as _},
    net::{Ipv4Addr, Ipv6Addr, TcpListener, TcpStream},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use aws_lc_rs::{
    digest,
    rand::{SecureRandom as _, SystemRandom},
    signature,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use reqwest::{StatusCode, Url, blocking::Client};
use serde::Deserialize;

use super::{Account, accounts::Access};

const METADATA_URL: &str = "https://login.eveonline.com/.well-known/oauth-authorization-server";
/// The SSO's metadata and keys change rarely; this long they are used without asking again.
const METADATA_MAX_AGE: Duration = Duration::from_secs(3600);
/// The issuers EVE SSO signs as, with and without the scheme.
const ISSUERS: [&str; 3] = [
    "login.eveonline.com",
    "https://login.eveonline.com",
    "https://login.eveonline.com/",
];
/// Every EVE token names this audience next to the app's client ID.
const AUDIENCE: &str = "EVE Online";
/// Clock difference forgiven when checking a token's expiry.
const LEEWAY: u64 = 60;
/// How long the browser has to come back.
pub const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);
const POLL: Duration = Duration::from_millis(100);

/// Random bytes as base64url, for the PKCE verifier and the state.
pub fn random_token() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| "no random numbers available".to_owned())?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// The PKCE pair: the verifier stays here until the code is exchanged, the challenge goes to
/// the SSO with the login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pkce {
    pub verifier: String,
    pub challenge: String,
}

impl Pkce {
    pub fn new() -> Result<Self, String> {
        Ok(Self::from_verifier(random_token()?))
    }

    /// The S256 challenge: the verifier's SHA-256, base64url without padding.
    pub fn from_verifier(verifier: String) -> Self {
        let hash = digest::digest(&digest::SHA256, verifier.as_bytes());
        let challenge = URL_SAFE_NO_PAD.encode(hash.as_ref());
        Self {
            verifier,
            challenge,
        }
    }
}

/// The SSO's endpoints, from its metadata document.
#[derive(Debug, Clone, Deserialize)]
pub struct Metadata {
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub jwks_uri: String,
}

/// The keys the SSO signs tokens with.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Jwks {
    pub keys: Vec<Jwk>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct Jwk {
    pub kid: String,
    pub kty: String,
    pub alg: String,
    /// RSA modulus and exponent, base64url.
    pub n: String,
    pub e: String,
}

static ENDPOINTS: Mutex<Option<(Instant, Metadata, Jwks)>> = Mutex::new(None);

/// The SSO's endpoints and signing keys, fetched at most once an hour unless `fresh`.
pub fn endpoints(http: &Client, fresh: bool) -> Result<(Metadata, Jwks), String> {
    if !fresh
        && let Ok(cache) = ENDPOINTS.lock()
        && let Some((at, metadata, keys)) = cache.as_ref()
        && at.elapsed() < METADATA_MAX_AGE
    {
        return Ok((metadata.clone(), keys.clone()));
    }
    let metadata: Metadata = get_json(http, METADATA_URL)?;
    let keys: Jwks = get_json(http, &metadata.jwks_uri)?;
    if let Ok(mut cache) = ENDPOINTS.lock() {
        *cache = Some((Instant::now(), metadata.clone(), keys.clone()));
    }
    Ok((metadata, keys))
}

fn get_json<T: serde::de::DeserializeOwned>(http: &Client, url: &str) -> Result<T, String> {
    let response = http.get(url).send().map_err(|e| format!("EVE SSO: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("EVE SSO answered {}", response.status()));
    }
    let bytes = response.bytes().map_err(|e| format!("EVE SSO: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("EVE SSO sent something unreadable: {e}"))
}

/// The page the browser is sent to.
pub fn authorize_url(
    endpoint: &str,
    client_id: &str,
    redirect: &str,
    scopes: &[&str],
    state: &str,
    challenge: &str,
) -> Result<String, String> {
    let scope = scopes.join(" ");
    Url::parse_with_params(
        endpoint,
        [
            ("response_type", "code"),
            ("redirect_uri", redirect),
            ("client_id", client_id),
            ("scope", scope.as_str()),
            ("state", state),
            ("code_challenge", challenge),
            ("code_challenge_method", "S256"),
        ],
    )
    .map(String::from)
    .map_err(|e| format!("EVE SSO authorize endpoint: {e}"))
}

/// Pairs as an `application/x-www-form-urlencoded` body.
fn form(pairs: &[(&str, &str)]) -> String {
    let mut url = Url::parse("http://localhost/").expect("a constant URL parses");
    url.query_pairs_mut().extend_pairs(pairs);
    url.query().unwrap_or_default().to_owned()
}

#[derive(Debug, Clone, Deserialize)]
pub struct TokenResponse {
    pub access_token: String,
    pub refresh_token: String,
}

/// Why the token endpoint said no.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenError {
    /// The refresh token is no good any more: revoked, expired, or for another app.
    Revoked,
    Other(String),
}

impl std::fmt::Display for TokenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Revoked => f.write_str("the login expired or was revoked, log in again"),
            Self::Other(e) => f.write_str(e),
        }
    }
}

/// What the token endpoint says when it refuses.
#[derive(Deserialize)]
struct Refusal {
    error: String,
    #[serde(default)]
    error_description: String,
}

fn token_request(http: &Client, endpoint: &str, body: String) -> Result<TokenResponse, TokenError> {
    let response = http
        .post(endpoint)
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(body)
        .send()
        .map_err(|e| TokenError::Other(format!("EVE SSO: {e}")))?;
    let status = response.status();
    let bytes = response
        .bytes()
        .map_err(|e| TokenError::Other(format!("EVE SSO: {e}")))?;
    if status.is_success() {
        return serde_json::from_slice(&bytes)
            .map_err(|e| TokenError::Other(format!("EVE SSO sent an unreadable token: {e}")));
    }
    match serde_json::from_slice::<Refusal>(&bytes) {
        Ok(r) if r.error == "invalid_grant" && status == StatusCode::BAD_REQUEST => {
            Err(TokenError::Revoked)
        }
        Ok(r) if r.error_description.is_empty() => {
            Err(TokenError::Other(format!("EVE SSO: {}", r.error)))
        }
        Ok(r) => Err(TokenError::Other(format!(
            "EVE SSO: {} ({})",
            r.error_description, r.error
        ))),
        Err(_) => Err(TokenError::Other(format!("EVE SSO answered {status}"))),
    }
}

/// Trade the code from the callback for tokens.
pub fn exchange_code(
    http: &Client,
    endpoint: &str,
    client_id: &str,
    code: &str,
    verifier: &str,
) -> Result<TokenResponse, TokenError> {
    let body = form(&[
        ("grant_type", "authorization_code"),
        ("code", code),
        ("client_id", client_id),
        ("code_verifier", verifier),
    ]);
    token_request(http, endpoint, body)
}

/// A new access token for a refresh token. The answer carries a refresh token to keep instead.
pub fn refresh(
    http: &Client,
    endpoint: &str,
    client_id: &str,
    refresh_token: &str,
) -> Result<TokenResponse, TokenError> {
    let body = form(&[
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
        ("client_id", client_id),
    ]);
    token_request(http, endpoint, body)
}

/// What a checked access token says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claims {
    pub character_id: u64,
    pub name: String,
    pub scopes: Vec<String>,
    /// Seconds since the epoch when the token stops working.
    pub expires: u64,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum OneOrMany {
    One(String),
    Many(Vec<String>),
}

#[derive(Debug, Deserialize)]
struct RawClaims {
    iss: String,
    sub: String,
    #[serde(default)]
    name: String,
    exp: u64,
    aud: OneOrMany,
    scp: Option<OneOrMany>,
}

#[derive(Debug, Deserialize)]
struct Header {
    alg: String,
    #[serde(default)]
    kid: String,
}

fn decode_part<T: serde::de::DeserializeOwned>(part: &str, what: &str) -> Result<T, String> {
    let bytes = URL_SAFE_NO_PAD
        .decode(part)
        .map_err(|e| format!("token {what}: {e}"))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("token {what}: {e}"))
}

/// The key ID a token says it was signed with.
fn key_id(token: &str) -> Option<String> {
    let header: Header = decode_part(token.split('.').next()?, "header").ok()?;
    Some(header.kid)
}

/// Check a token's RS256 signature against `keys`, then its claims.
pub fn validate(token: &str, keys: &Jwks, client_id: &str, now: u64) -> Result<Claims, String> {
    let mut parts = token.split('.');
    let (Some(header), Some(payload), Some(sig), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err("the access token is not a JWT".to_owned());
    };
    let head: Header = decode_part(header, "header")?;
    if head.alg != "RS256" {
        return Err(format!("unexpected token algorithm {}", head.alg));
    }
    let key = keys
        .keys
        .iter()
        .find(|k| k.kid == head.kid && k.kty == "RSA" && (k.alg.is_empty() || k.alg == "RS256"))
        .ok_or_else(|| format!("no EVE SSO key {}", head.kid))?;
    let decode = |s: &str| {
        URL_SAFE_NO_PAD
            .decode(s)
            .map_err(|e| format!("EVE SSO key: {e}"))
    };
    let (n, e, sig) = (decode(&key.n)?, decode(&key.e)?, decode(sig)?);
    let signed = &token[..header.len() + 1 + payload.len()];
    signature::RsaPublicKeyComponents { n: &n, e: &e }
        .verify(
            &signature::RSA_PKCS1_2048_8192_SHA256,
            signed.as_bytes(),
            &sig,
        )
        .map_err(|_| "the access token's signature does not check out".to_owned())?;
    check_claims(&decode_part(payload, "claims")?, client_id, now)
}

/// Issuer, audience and expiry, then the character from the subject.
fn check_claims(raw: &RawClaims, client_id: &str, now: u64) -> Result<Claims, String> {
    if !ISSUERS.contains(&raw.iss.as_str()) {
        return Err(format!("token from an unknown issuer {}", raw.iss));
    }
    let audience = match &raw.aud {
        OneOrMany::One(a) => vec![a.as_str()],
        OneOrMany::Many(v) => v.iter().map(String::as_str).collect(),
    };
    if !audience.contains(&client_id) || !audience.contains(&AUDIENCE) {
        return Err("the token is for another application".to_owned());
    }
    if raw.exp + LEEWAY <= now {
        return Err("the token has expired".to_owned());
    }
    let character_id = raw
        .sub
        .strip_prefix("CHARACTER:EVE:")
        .and_then(|id| id.parse().ok())
        .ok_or_else(|| format!("the token is not for a character: {}", raw.sub))?;
    let scopes = match &raw.scp {
        Some(OneOrMany::One(s)) => vec![s.clone()],
        Some(OneOrMany::Many(v)) => v.clone(),
        None => Vec::new(),
    };
    Ok(Claims {
        character_id,
        name: raw.name.clone(),
        scopes,
        expires: raw.exp,
    })
}

/// [`validate`] with the SSO's current keys, fetching them again once if the token names a
/// key efa has not seen.
pub fn verify(http: &Client, token: &str, client_id: &str, now: u64) -> Result<Claims, String> {
    let (_, keys) = endpoints(http, false)?;
    let known = key_id(token).is_some_and(|kid| keys.keys.iter().any(|k| k.kid == kid));
    let keys = if known {
        keys
    } else {
        endpoints(http, true)?.1
    };
    validate(token, &keys, client_id, now)
}

/// An account from a token answer whose access token checked out.
pub fn account(response: TokenResponse, claims: Claims) -> Account {
    Account {
        character_id: claims.character_id,
        name: claims.name,
        refresh_token: response.refresh_token,
        scopes: claims.scopes,
        access: Some(Access {
            token: response.access_token,
            expires_at: claims.expires,
        }),
    }
}

/// What the browser brought back to the callback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Callback {
    /// Not the callback path, such as the browser asking for a favicon.
    Other,
    /// A callback for another login, such as an old tab reloaded; this login waits on.
    Stray,
    Code(String),
    Refused(String),
}

/// Read the request line of a call to the listener.
pub fn parse_callback(request_line: &str, expected_state: &str) -> Callback {
    let mut words = request_line.split_whitespace();
    let (Some("GET"), Some(target)) = (words.next(), words.next()) else {
        return Callback::Other;
    };
    let Ok(url) = Url::parse(&format!("http://localhost{target}")) else {
        return Callback::Other;
    };
    if url.path().trim_end_matches('/') != "/callback" {
        return Callback::Other;
    }
    let param = |name: &str| {
        url.query_pairs()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.into_owned())
    };
    // Only an answer carrying this login's state ends it; anything else is left alone, so a
    // stray request cannot cut the login short.
    if param("state").as_deref() != Some(expected_state) {
        return Callback::Stray;
    }
    if let Some(error) = param("error") {
        let description = param("error_description").unwrap_or_default();
        return Callback::Refused(if description.is_empty() {
            format!("EVE SSO refused the login: {error}")
        } else {
            format!("EVE SSO refused the login: {description}")
        });
    }
    match param("code") {
        Some(code) if !code.is_empty() => Callback::Code(code),
        _ => Callback::Refused("EVE SSO sent no code".to_owned()),
    }
}

/// The local listener the browser comes back to, on IPv4 and IPv6 localhost both, since
/// "localhost" can be either.
pub struct Listener {
    sockets: Vec<TcpListener>,
}

impl Listener {
    /// Listen on `port`, waiting a moment for a login that was just cancelled to let go of it.
    pub fn bind(port: u16) -> Result<Self, String> {
        let mut tries = 0;
        loop {
            match Self::bind_once(port) {
                Err(_) if tries < 10 => {
                    tries += 1;
                    std::thread::sleep(POLL * 2);
                }
                result => return result,
            }
        }
    }

    fn bind_once(port: u16) -> Result<Self, String> {
        let attempts = [
            TcpListener::bind((Ipv4Addr::LOCALHOST, port)),
            TcpListener::bind((Ipv6Addr::LOCALHOST, port)),
        ];
        let mut sockets = Vec::new();
        let mut error = None;
        for attempt in attempts {
            match attempt {
                Ok(socket) => sockets.push(socket),
                Err(e) => error = Some(e),
            }
        }
        if sockets.is_empty() {
            let e = error.map_or_else(String::new, |e| e.to_string());
            return Err(format!("cannot listen on port {port} for the login: {e}"));
        }
        for socket in &sockets {
            socket
                .set_nonblocking(true)
                .map_err(|e| format!("login listener: {e}"))?;
        }
        Ok(Self { sockets })
    }

    /// Wait for the browser to come back with a code, until `cancel` is set or the time runs
    /// out.
    pub fn wait(&self, state: &str, cancel: &AtomicBool) -> Result<String, String> {
        let deadline = Instant::now() + LOGIN_TIMEOUT;
        while Instant::now() < deadline {
            if cancel.load(Ordering::Relaxed) {
                return Err("login cancelled".to_owned());
            }
            for socket in &self.sockets {
                match socket.accept() {
                    Ok((stream, _)) => match answer(stream, state) {
                        Callback::Other | Callback::Stray => {}
                        Callback::Code(code) => return Ok(code),
                        Callback::Refused(e) => return Err(e),
                    },
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                    Err(e) => return Err(format!("login listener: {e}")),
                }
            }
            std::thread::sleep(POLL);
        }
        Err("the login timed out".to_owned())
    }
}

/// Read one request, answer it with a short page, and say what it was.
fn answer(mut stream: TcpStream, state: &str) -> Callback {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut request = Vec::new();
    let mut buf = [0u8; 2048];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") && request.len() < 16 * 1024 {
        match stream.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => request.extend_from_slice(&buf[..n]),
        }
    }
    let text = String::from_utf8_lossy(&request);
    let callback = parse_callback(text.lines().next().unwrap_or_default(), state);
    let (status, message) = match &callback {
        Callback::Other => ("404 Not Found", "Nothing here.".to_owned()),
        Callback::Stray => (
            "400 Bad Request",
            "This answer belongs to another login. Finish the login in the tab efa opened last."
                .to_owned(),
        ),
        Callback::Code(_) => (
            "200 OK",
            "Logged in. You can close this tab and go back to efa.".to_owned(),
        ),
        Callback::Refused(e) => ("400 Bad Request", format!("The login failed: {e}")),
    };
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>efa</title></head>\
         <body style=\"font-family:sans-serif;margin:3em\"><p>{}</p></body></html>",
        crate::port::escape_markup(&message)
    );
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.flush();
    callback
}

/// A login on its way: the browser is to be sent to `url`, then [`Login::finish`] waits for it.
pub struct Login {
    pub url: String,
    state: String,
    pkce: Pkce,
    client_id: String,
    token_endpoint: String,
    listener: Listener,
}

impl Login {
    /// Listen for the callback and work out where to send the browser.
    pub fn start(http: &Client, client_id: &str, port: u16) -> Result<Self, String> {
        let listener = Listener::bind(port)?;
        let (metadata, _) = endpoints(http, false)?;
        let pkce = Pkce::new()?;
        let state = random_token()?;
        let url = authorize_url(
            &metadata.authorization_endpoint,
            client_id,
            &super::callback_url(port),
            &super::SCOPES,
            &state,
            &pkce.challenge,
        )?;
        Ok(Self {
            url,
            state,
            pkce,
            client_id: client_id.to_owned(),
            token_endpoint: metadata.token_endpoint,
            listener,
        })
    }

    /// Wait for the browser, trade the code for tokens and check them.
    pub fn finish(self, http: &Client, cancel: &AtomicBool) -> Result<Account, String> {
        let code = self.listener.wait(&self.state, cancel)?;
        drop(self.listener);
        let response = exchange_code(
            http,
            &self.token_endpoint,
            &self.client_id,
            &code,
            &self.pkce.verifier,
        )
        .map_err(|e| e.to_string())?;
        let claims = verify(
            http,
            &response.access_token,
            &self.client_id,
            crate::net::now(),
        )?;
        Ok(account(response, claims))
    }
}

#[cfg(test)]
mod tests {
    use aws_lc_rs::{rsa::KeySize, signature::KeyPair as _};

    use super::*;

    #[test]
    fn pkce_challenge_is_the_unpadded_sha256() {
        // Worked out with Python's hashlib and base64.urlsafe_b64encode, padding stripped.
        let pkce = Pkce::from_verifier("dBjftJeZ4CVP-mB92K27uhbUJU4y_ZOuTzQHmGzPU5c".to_owned());
        assert_eq!(
            pkce.challenge,
            "WaFxkRd68rAvd5wWoW86fUhC6ai4uunW92uhjSG1mOk"
        );
    }

    #[test]
    fn random_tokens_are_long_and_differ() {
        let (a, b) = (
            random_token().expect("random"),
            random_token().expect("random"),
        );
        assert_eq!(a.len(), 43, "32 bytes as unpadded base64url");
        assert_ne!(a, b);
        assert!(
            a.chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        );
    }

    #[test]
    fn authorize_url_carries_everything() {
        let url = authorize_url(
            "https://login.eveonline.com/v2/oauth/authorize",
            "abc",
            "http://localhost:6461/callback",
            &["a.v1", "b.v1"],
            "st",
            "ch",
        )
        .expect("url");
        let parsed = Url::parse(&url).expect("parses");
        let get = |k: &str| {
            parsed
                .query_pairs()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.into_owned())
        };
        assert_eq!(
            get("redirect_uri").as_deref(),
            Some("http://localhost:6461/callback")
        );
        assert_eq!(get("scope").as_deref(), Some("a.v1 b.v1"));
        assert_eq!(get("code_challenge_method").as_deref(), Some("S256"));
        assert_eq!(get("client_id").as_deref(), Some("abc"));
        assert_eq!(get("state").as_deref(), Some("st"));
    }

    #[test]
    fn reads_the_callback() {
        assert_eq!(
            parse_callback("GET /callback?code=xyz&state=s1 HTTP/1.1", "s1"),
            Callback::Code("xyz".to_owned())
        );
        assert_eq!(
            parse_callback("GET /favicon.ico HTTP/1.1", "s1"),
            Callback::Other
        );
        assert_eq!(
            parse_callback("GET /callback?code=xyz&state=other HTTP/1.1", "s1"),
            Callback::Stray
        );
        assert_eq!(
            parse_callback("GET /callback?error=access_denied&state=old HTTP/1.1", "s1"),
            Callback::Stray,
            "a refusal for another login does not end this one"
        );
        assert_eq!(
            parse_callback("GET /callback?code=xyz HTTP/1.1", "s1"),
            Callback::Stray
        );
        assert!(matches!(
            parse_callback("GET /callback?error=access_denied&state=s1 HTTP/1.1", "s1"),
            Callback::Refused(e) if e.contains("access_denied")
        ));
    }

    /// A token signed with a key made for the test, and that key as the SSO would publish it.
    fn signed(claims: &serde_json::Value) -> (String, Jwks) {
        let pair = signature::RsaKeyPair::generate(KeySize::Rsa2048).expect("key");
        let public = pair.public_key();
        let jwk = Jwk {
            kid: "JWT-Signature-Key".to_owned(),
            kty: "RSA".to_owned(),
            alg: "RS256".to_owned(),
            n: URL_SAFE_NO_PAD.encode(public.modulus().big_endian_without_leading_zero()),
            e: URL_SAFE_NO_PAD.encode(public.exponent().big_endian_without_leading_zero()),
        };
        let header =
            URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","kid":"JWT-Signature-Key","typ":"JWT"}"#);
        let payload = URL_SAFE_NO_PAD.encode(claims.to_string());
        let input = format!("{header}.{payload}");
        let mut sig = vec![0; pair.public_modulus_len()];
        pair.sign(
            &signature::RSA_PKCS1_SHA256,
            &SystemRandom::new(),
            input.as_bytes(),
            &mut sig,
        )
        .expect("sign");
        (
            format!("{input}.{}", URL_SAFE_NO_PAD.encode(sig)),
            Jwks { keys: vec![jwk] },
        )
    }

    fn claims(exp: u64) -> serde_json::Value {
        serde_json::json!({
            "iss": "https://login.eveonline.com",
            "sub": "CHARACTER:EVE:2112625428",
            "name": "Some Pilot",
            "exp": exp,
            "aud": ["my-client", "EVE Online"],
            "scp": ["esi-skills.read_skills.v1", "esi-fittings.read_fittings.v1"],
        })
    }

    #[test]
    fn validates_a_signed_token() {
        let (token, keys) = signed(&claims(2_000));
        let c = validate(&token, &keys, "my-client", 1_000).expect("valid");
        assert_eq!(c.character_id, 2_112_625_428);
        assert_eq!(c.name, "Some Pilot");
        assert_eq!(c.scopes.len(), 2);
        assert_eq!(c.expires, 2_000);
    }

    #[test]
    fn rejects_wrong_audience_expiry_and_tampering() {
        let (token, keys) = signed(&claims(2_000));
        assert!(validate(&token, &keys, "another-client", 1_000).is_err());
        assert!(validate(&token, &keys, "my-client", 3_000).is_err());
        // Swap in other claims under the same signature.
        let mut parts: Vec<&str> = token.split('.').collect();
        let forged = URL_SAFE_NO_PAD.encode(claims(9_000).to_string());
        parts[1] = &forged;
        assert!(validate(&parts.join("."), &keys, "my-client", 1_000).is_err());
        // Another key with the same ID.
        let (_, other_keys) = signed(&claims(2_000));
        assert!(validate(&token, &other_keys, "my-client", 1_000).is_err());
    }

    #[test]
    fn checks_issuer_and_subject() {
        let raw = |iss: &str, sub: &str| RawClaims {
            iss: iss.to_owned(),
            sub: sub.to_owned(),
            name: String::new(),
            exp: 100,
            aud: OneOrMany::Many(vec!["c".to_owned(), AUDIENCE.to_owned()]),
            scp: Some(OneOrMany::One("s".to_owned())),
        };
        assert!(check_claims(&raw("login.eveonline.com", "CHARACTER:EVE:1"), "c", 0).is_ok());
        assert!(check_claims(&raw("evil.example", "CHARACTER:EVE:1"), "c", 0).is_err());
        assert!(check_claims(&raw("login.eveonline.com", "CORPORATION:EVE:1"), "c", 0).is_err());
        let single_scope = check_claims(&raw("login.eveonline.com", "CHARACTER:EVE:1"), "c", 0);
        assert_eq!(single_scope.expect("valid").scopes, vec!["s".to_owned()]);
    }

    #[test]
    fn form_bodies_are_encoded() {
        assert_eq!(form(&[("a", "b c"), ("d", "e&f")]), "a=b+c&d=e%26f");
    }
}
