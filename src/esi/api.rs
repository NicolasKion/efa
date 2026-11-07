//! The ESI routes gyfa calls for a logged-in character, with its access token refreshed first
//! when it is about to run out.

use reqwest::{Method, StatusCode, blocking::Client};
use serde::Deserialize;

use super::{
    Account, IMPLANTS_SCOPE, READ_FITTINGS_SCOPE, SKILLS_SCOPE, WRITE_FITTINGS_SCOPE,
    accounts::Access, sso,
};
use crate::{data::TypeId, port::esi::Fitting};

const ESI_URL: &str = "https://esi.evetech.net";
/// ESI behaviour gyfa is written against, the same date the price fetch pins.
const COMPATIBILITY_DATE: &str = "2026-08-18";

/// What "Get Skills" brings in for a character.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sheet {
    /// Trained levels by skill.
    pub skills: Vec<(TypeId, u8)>,
    /// Plugged-in implants, when the character granted access to them.
    pub implants: Option<Vec<TypeId>>,
    pub sec_status: Option<f64>,
}

/// Requests on behalf of one character. Take `account` back afterwards: its tokens may have
/// been refreshed, and EVE SSO hands out a new refresh token each time.
pub struct Session {
    http: Client,
    client_id: String,
    pub account: Account,
}

impl Session {
    pub fn new(http: Client, client_id: String, account: Account) -> Self {
        Self {
            http,
            client_id,
            account,
        }
    }

    /// A current access token, refreshed when needed.
    fn token(&mut self) -> Result<String, String> {
        let now = super::now();
        if self.account.needs_refresh(now) {
            let (metadata, _) = sso::endpoints(&self.http, false)?;
            let response = sso::refresh(
                &self.http,
                &metadata.token_endpoint,
                &self.client_id,
                &self.account.refresh_token,
            )
            .map_err(|e| format!("{}: {e}", self.account.name))?;
            let claims = sso::verify(&self.http, &response.access_token, &self.client_id, now)?;
            if claims.character_id != self.account.character_id {
                return Err("EVE SSO answered for another character".to_owned());
            }
            self.account.refresh_token = response.refresh_token;
            self.account.scopes = claims.scopes;
            self.account.access = Some(Access {
                token: response.access_token,
                expires_at: claims.expires,
            });
        }
        Ok(self
            .account
            .access
            .as_ref()
            .map_or_else(String::new, |a| a.token.clone()))
    }

    fn require(&self, scope: &str) -> Result<(), String> {
        if self.account.has_scope(scope) {
            Ok(())
        } else {
            Err(format!(
                "{} did not grant {scope}; log the character in again",
                self.account.name
            ))
        }
    }

    /// Send a request for the character and hand back the body of a successful answer.
    fn send(
        &mut self,
        method: Method,
        path: &str,
        body: Option<String>,
        auth: bool,
    ) -> Result<Vec<u8>, String> {
        let mut request = self
            .http
            .request(method, format!("{ESI_URL}{path}"))
            .header("X-Compatibility-Date", COMPATIBILITY_DATE)
            .header(reqwest::header::ACCEPT, "application/json");
        if auth {
            request = request.bearer_auth(self.token()?);
        }
        if let Some(body) = body {
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(body);
        }
        let response = request.send().map_err(|e| format!("ESI: {e}"))?;
        let status = response.status();
        let retry = response
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        let bytes = response.bytes().map_err(|e| format!("ESI: {e}"))?;
        if status.is_success() {
            return Ok(bytes.to_vec());
        }
        Err(failure(status, &bytes, retry.as_deref()))
    }

    fn get<T: serde::de::DeserializeOwned>(&mut self, path: &str, auth: bool) -> Result<T, String> {
        let bytes = self.send(Method::GET, path, None, auth)?;
        serde_json::from_slice(&bytes).map_err(|e| format!("ESI sent something unreadable: {e}"))
    }

    pub fn skills(&mut self) -> Result<Vec<(TypeId, u8)>, String> {
        #[derive(Deserialize)]
        struct Skills {
            skills: Vec<Skill>,
        }
        #[derive(Deserialize)]
        struct Skill {
            skill_id: TypeId,
            trained_skill_level: u8,
        }
        self.require(SKILLS_SCOPE)?;
        let path = format!("/characters/{}/skills", self.account.character_id);
        let skills: Skills = self.get(&path, true)?;
        Ok(skills
            .skills
            .into_iter()
            .map(|s| (s.skill_id, s.trained_skill_level))
            .collect())
    }

    pub fn implants(&mut self) -> Result<Vec<TypeId>, String> {
        self.require(IMPLANTS_SCOPE)?;
        let path = format!("/characters/{}/implants", self.account.character_id);
        self.get(&path, true)
    }

    /// Public information, no scope needed.
    pub fn sec_status(&mut self) -> Result<f64, String> {
        #[derive(Deserialize)]
        struct Public {
            #[serde(default)]
            security_status: f64,
        }
        let path = format!("/characters/{}", self.account.character_id);
        let public: Public = self.get(&path, false)?;
        Ok(public.security_status)
    }

    /// Skills, implants if granted, and security status, as pyfa's Get Skills fetches them.
    pub fn sheet(&mut self) -> Result<Sheet, String> {
        let skills = self.skills()?;
        let implants = if self.account.has_scope(IMPLANTS_SCOPE) {
            Some(self.implants()?)
        } else {
            None
        };
        let sec_status = self.sec_status().ok();
        Ok(Sheet {
            skills,
            implants,
            sec_status,
        })
    }

    pub fn fittings(&mut self) -> Result<Vec<Fitting>, String> {
        self.require(READ_FITTINGS_SCOPE)?;
        let path = format!("/characters/{}/fittings", self.account.character_id);
        self.get(&path, true)
    }

    /// Save a fitting in the game from [`crate::port::esi::upload`]'s JSON; returns its ID.
    pub fn add_fitting(&mut self, json: String) -> Result<u64, String> {
        #[derive(Deserialize)]
        struct Created {
            fitting_id: u64,
        }
        self.require(WRITE_FITTINGS_SCOPE)?;
        let path = format!("/characters/{}/fittings", self.account.character_id);
        let bytes = self.send(Method::POST, &path, Some(json), true)?;
        let created: Created = serde_json::from_slice(&bytes)
            .map_err(|e| format!("ESI sent something unreadable: {e}"))?;
        Ok(created.fitting_id)
    }

    pub fn delete_fitting(&mut self, fitting_id: u64) -> Result<(), String> {
        self.require(WRITE_FITTINGS_SCOPE)?;
        let path = format!(
            "/characters/{}/fittings/{fitting_id}",
            self.account.character_id
        );
        self.send(Method::DELETE, &path, None, true).map(drop)
    }
}

/// A failed answer in words, with ESI's own message when it sent one.
fn failure(status: StatusCode, body: &[u8], retry_after: Option<&str>) -> String {
    #[derive(Deserialize)]
    struct Error {
        error: String,
    }
    let message = serde_json::from_slice::<Error>(body).map_or_else(|_| String::new(), |e| e.error);
    let detail = if message.is_empty() {
        String::new()
    } else {
        format!(": {message}")
    };
    match status.as_u16() {
        401 | 403 => format!("ESI refused access ({status}){detail}. Log the character in again."),
        420 | 429 => format!(
            "ESI asks gyfa to slow down; try again in {} seconds",
            retry_after.unwrap_or("60")
        ),
        _ => format!("ESI answered {status}{detail}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failures_read_well() {
        let forbidden = failure(
            StatusCode::FORBIDDEN,
            br#"{"error":"token not valid"}"#,
            None,
        );
        assert!(forbidden.contains("token not valid") && forbidden.contains("again"));
        let slow = failure(StatusCode::TOO_MANY_REQUESTS, b"", Some("12"));
        assert!(slow.contains("12 seconds"));
        assert_eq!(
            failure(StatusCode::BAD_GATEWAY, b"<html>", None),
            "ESI answered 502 Bad Gateway"
        );
    }

    #[test]
    fn missing_scopes_fail_before_any_request() {
        let http = Client::new();
        let account = Account {
            character_id: 1,
            name: "Pilot".to_owned(),
            ..Account::default()
        };
        let mut session = Session::new(http, "client".to_owned(), account);
        let e = session.fittings().expect_err("no scope");
        assert!(e.contains(READ_FITTINGS_SCOPE));
    }
}
