//! Characters logged in with EVE SSO, pyfa's SSO characters. Their refresh tokens are kept in
//! `esi.json` in the data directory, readable by the user only; access tokens stay in memory and
//! are refreshed shortly before they run out.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Refresh an access token this many seconds before it expires, so a request does not race it.
pub const REFRESH_MARGIN: u64 = 60;
const FILE: &str = "esi.json";

/// A character gyfa may act for.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Account {
    pub character_id: u64,
    pub name: String,
    pub refresh_token: String,
    /// The scopes the character granted.
    pub scopes: Vec<String>,
    /// The current access token. Never written out.
    #[serde(skip)]
    pub access: Option<Access>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Access {
    pub token: String,
    /// Seconds since the epoch.
    pub expires_at: u64,
}

impl Account {
    /// Whether the access token has to be refreshed before a request made at `now`.
    pub fn needs_refresh(&self, now: u64) -> bool {
        self.access
            .as_ref()
            .is_none_or(|a| a.expires_at <= now + REFRESH_MARGIN)
    }

    pub fn has_scope(&self, scope: &str) -> bool {
        self.scopes.iter().any(|s| s == scope)
    }

    /// The scopes gyfa asks for that this character did not grant.
    pub fn missing_scopes(&self) -> Vec<&'static str> {
        super::SCOPES
            .into_iter()
            .filter(|s| !self.has_scope(s))
            .collect()
    }

    /// The parts that are written out, to tell whether a change needs saving.
    fn saved_part(&self) -> (u64, &str, &str, &[String]) {
        (
            self.character_id,
            &self.name,
            &self.refresh_token,
            &self.scopes,
        )
    }
}

/// Every logged-in character.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Accounts {
    pub accounts: Vec<Account>,
}

impl Accounts {
    pub fn load() -> Self {
        path().map_or_else(Self::default, |p| Self::load_from(&p))
    }

    pub fn load_from(path: &Path) -> Self {
        crate::persist::load(path)
    }

    pub fn save(&self) -> Result<(), String> {
        self.save_to(&path().ok_or("no home directory")?)
    }

    pub fn save_to(&self, path: &Path) -> Result<(), String> {
        crate::persist::save_private(path, self)
    }

    pub fn get(&self, character_id: u64) -> Option<&Account> {
        self.accounts
            .iter()
            .find(|a| a.character_id == character_id)
    }

    pub fn name(&self, character_id: u64) -> Option<&str> {
        self.get(character_id).map(|a| a.name.as_str())
    }

    /// Add a character or replace its earlier login, in place. Returns whether anything that
    /// is written out changed.
    pub fn upsert(&mut self, account: Account) -> bool {
        if let Some(old) = self
            .accounts
            .iter_mut()
            .find(|a| a.character_id == account.character_id)
        {
            let changed = old.saved_part() != account.saved_part();
            *old = account;
            changed
        } else {
            self.accounts.push(account);
            true
        }
    }

    pub fn remove(&mut self, character_id: u64) {
        self.accounts.retain(|a| a.character_id != character_id);
    }

    /// By name, as the lists show them.
    pub fn sorted(&self) -> Vec<&Account> {
        let mut accounts: Vec<&Account> = self.accounts.iter().collect();
        accounts.sort_by(|a, b| a.name.cmp(&b.name));
        accounts
    }
}

fn path() -> Option<PathBuf> {
    crate::persist::data_dir().map(|d| d.join(FILE))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn account(id: u64, token: &str) -> Account {
        Account {
            character_id: id,
            name: format!("Pilot {id}"),
            refresh_token: token.to_owned(),
            scopes: vec![super::super::SKILLS_SCOPE.to_owned()],
            access: None,
        }
    }

    #[test]
    fn refreshes_shortly_before_expiry() {
        let mut a = account(1, "r");
        assert!(a.needs_refresh(1_000), "no access token yet");
        a.access = Some(Access {
            token: "t".to_owned(),
            expires_at: 2_000,
        });
        assert!(!a.needs_refresh(1_000));
        assert!(!a.needs_refresh(2_000 - REFRESH_MARGIN - 1));
        assert!(a.needs_refresh(2_000 - REFRESH_MARGIN));
        assert!(a.needs_refresh(5_000));
    }

    #[test]
    fn missing_scopes_are_listed() {
        let a = account(1, "r");
        let missing = a.missing_scopes();
        assert_eq!(missing.len(), super::super::SCOPES.len() - 1);
        assert!(!missing.contains(&super::super::SKILLS_SCOPE));
    }

    #[test]
    fn upsert_reports_what_needs_saving() {
        let mut accounts = Accounts::default();
        assert!(accounts.upsert(account(1, "r1")));
        let mut fresh = account(1, "r1");
        fresh.access = Some(Access {
            token: "t".to_owned(),
            expires_at: 1,
        });
        assert!(
            !accounts.upsert(fresh),
            "a new access token is not written out"
        );
        assert!(
            accounts.upsert(account(1, "r2")),
            "a rotated refresh token is"
        );
        assert_eq!(accounts.accounts.len(), 1);
        accounts.remove(1);
        assert_eq!(accounts, Accounts::default());
    }

    #[test]
    fn saved_privately_without_access_tokens() {
        let dir = std::env::temp_dir().join(format!("gyfa-esi-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join(FILE);
        let mut a = account(7, "secret");
        a.access = Some(Access {
            token: "access".to_owned(),
            expires_at: 9,
        });
        let accounts = Accounts { accounts: vec![a] };
        accounts.save_to(&path).expect("save");
        let text = std::fs::read_to_string(&path).expect("read");
        assert!(text.contains("secret") && !text.contains("access\""));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = std::fs::metadata(&path).expect("meta").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let loaded = Accounts::load_from(&path);
        assert_eq!(loaded.accounts[0].refresh_token, "secret");
        assert!(loaded.accounts[0].access.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
