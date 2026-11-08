//! EVE SSO and ESI in the UI: logging characters in, and running ESI requests for them off the
//! UI thread. The logged-in characters live in the [`Hub`], like the efa characters, so every
//! window sees a login or a removal right away.
//!
//! The windows: Manage ESI Characters (pyfa's SSO Character Management), Browse ESI Fittings
//! (Ctrl+B) and Export to EVE (Ctrl+E).

mod characters;
mod fittings;
mod upload;

use std::{
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use gpui_kit::{App, AppContext, Context, Entity, SharedString, Task, WeakEntity};

use super::{Efa, Hub, chrome::open_single};
use crate::{
    esi::{Account, Accounts, api::Session, sso::Login},
    preferences::Proxy,
};

pub use characters::EsiCharactersWindow;
pub(super) use characters::login_status;

/// Login progress and the last word on it, for the windows that offer a login.
#[derive(Default)]
pub struct EsiState {
    pub accounts: Rc<Accounts>,
    /// Set while a login waits for the browser; setting the flag gives up on it.
    pub login: Option<Arc<AtomicBool>>,
    pub status: Option<SharedString>,
}

impl EsiState {
    pub fn new(accounts: Accounts) -> Self {
        Self {
            accounts: Rc::new(accounts),
            ..Self::default()
        }
    }
}

/// What every ESI request needs from the preferences.
struct Setup {
    client_id: String,
    port: u16,
    proxy: Proxy,
}

impl Hub {
    /// The settings an ESI request needs, or what to do first.
    fn esi_setup(&self) -> Result<Setup, String> {
        let prefs = &self.saved.settings.preferences;
        if !prefs.network.enabled {
            return Err("The network is switched off in Preferences > Network.".to_owned());
        }
        // A Client ID in the preferences overrides efa's own application.
        let client_id = match prefs.esi.client_id.trim() {
            "" => crate::esi::CLIENT_ID,
            own => own,
        };
        Ok(Setup {
            client_id: client_id.to_owned(),
            port: prefs.esi.port,
            proxy: prefs.network.proxy.clone(),
        })
    }

    /// Log a character in through the browser. `done` gets its character ID, once this hub is
    /// no longer being updated.
    pub fn esi_login(
        &mut self,
        cx: &mut Context<Self>,
        done: impl FnOnce(Result<u64, String>, &mut App) + 'static,
    ) {
        self.esi_cancel_login();
        let setup = match self.esi_setup() {
            Ok(setup) => setup,
            Err(e) => {
                self.esi.status = Some(e.clone().into());
                cx.notify();
                cx.defer(move |cx| done(Err(e), cx));
                return;
            }
        };
        let cancel = Arc::new(AtomicBool::new(false));
        self.esi.login = Some(Arc::clone(&cancel));
        self.esi.status = Some("Starting the login".into());
        cx.notify();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let Setup {
                client_id,
                port,
                proxy,
            } = setup;
            let started = cx
                .background_spawn(async move {
                    let http = proxy.client()?;
                    let login = Login::start(&http, &client_id, port)?;
                    Ok::<_, String>((http, login))
                })
                .await;
            let result = match started {
                Ok((http, login)) => {
                    let url = login.url.clone();
                    cx.update(|cx| cx.open_url(&url));
                    let _ = this.update(cx, |hub, cx| {
                        hub.esi.status = Some(
                            "Log in with EVE in the browser window that opened, then come back."
                                .into(),
                        );
                        cx.notify();
                    });
                    let cancel = Arc::clone(&cancel);
                    cx.background_spawn(async move { login.finish(&http, &cancel) })
                        .await
                }
                Err(e) => Err(e),
            };
            let result = this
                .update(cx, |hub, cx| {
                    // A newer login took over; this one has nothing more to say.
                    if hub
                        .esi
                        .login
                        .as_ref()
                        .is_some_and(|c| Arc::ptr_eq(c, &cancel))
                    {
                        hub.esi.login = None;
                    }
                    let result = result.map(|account| {
                        let (id, name) = (account.character_id, account.name.clone());
                        let missing = account.missing_scopes();
                        hub.esi_store(account, true, cx);
                        hub.esi.status = Some(if missing.is_empty() {
                            format!("Logged in as {name}.").into()
                        } else {
                            format!(
                                "Logged in as {name}, without {}. Check the scopes of the app.",
                                missing.join(", ")
                            )
                            .into()
                        });
                        id
                    });
                    if let Err(e) = &result {
                        hub.esi.status = Some(format!("Login failed: {e}").into());
                    }
                    cx.notify();
                    result
                })
                .unwrap_or_else(|_| Err("efa is closing".to_owned()));
            cx.update(|cx| done(result, cx));
        })
        .detach();
    }

    pub fn esi_cancel_login(&mut self) {
        if let Some(cancel) = self.esi.login.take() {
            cancel.store(true, Ordering::Relaxed);
            self.esi.status = None;
        }
    }

    /// Keep an account's tokens, writing them out when that changes the file. With `add` false
    /// a character removed in the meantime stays removed.
    fn esi_store(&mut self, account: Account, add: bool, cx: &mut Context<Self>) {
        if !add && self.esi.accounts.get(account.character_id).is_none() {
            return;
        }
        let mut accounts = (*self.esi.accounts).clone();
        if accounts.upsert(account)
            && let Err(e) = self.write_accounts(&accounts)
        {
            eprintln!("saving ESI logins: {e}");
        }
        self.esi.accounts = Rc::new(accounts);
        cx.notify();
    }

    /// Forget a character's login. Its refresh token is gone from disk with the next save.
    pub fn esi_remove(&mut self, character_id: u64, cx: &mut Context<Self>) {
        let mut accounts = (*self.esi.accounts).clone();
        accounts.remove(character_id);
        if let Err(e) = self.write_accounts(&accounts) {
            eprintln!("saving ESI logins: {e}");
        }
        self.esi.accounts = Rc::new(accounts);
        cx.notify();
    }

    /// Fits for the main window to add as new tabs, as if pasted.
    pub fn import_fits(&mut self, imported: crate::port::Imported, cx: &mut Context<Self>) {
        self.inbox.fit_imports.push(imported);
        cx.notify();
    }
}

/// Run `job` for an ESI character on a background thread and hand its result to `done` on the
/// window `W`. Refreshed tokens go back to the hub on the way.
pub fn call<W: 'static, T: Send + 'static>(
    hub: &Entity<Hub>,
    character_id: u64,
    cx: &mut Context<W>,
    job: impl FnOnce(&mut Session) -> Result<T, String> + Send + 'static,
    done: impl FnOnce(&mut W, Result<T, String>, &mut Context<W>) + 'static,
) -> Task<()> {
    let prepared = {
        let hub = hub.read(cx);
        hub.esi_setup().and_then(|setup| {
            hub.esi
                .accounts
                .get(character_id)
                .cloned()
                .map(|account| (setup, account))
                .ok_or_else(|| "That EVE character is not logged in any more.".to_owned())
        })
    };
    let hub = hub.downgrade();
    cx.spawn(async move |this: WeakEntity<W>, cx| {
        let result = match prepared {
            Err(e) => Err(e),
            Ok((setup, account)) => {
                let (account, result) = cx
                    .background_spawn(async move {
                        let http = match setup.proxy.client() {
                            Ok(http) => http,
                            Err(e) => return (None, Err(e)),
                        };
                        let mut session = Session::new(http, setup.client_id, account);
                        let result = job(&mut session);
                        (Some(session.account), result)
                    })
                    .await;
                if let Some(account) = account {
                    let _ = hub.update(cx, |hub, cx| hub.esi_store(account, false, cx));
                }
                result
            }
        };
        let _ = this.update(cx, |w, cx| done(w, result, cx));
    })
}

/// The ESI character to preselect: the one asked for if it is still logged in, else the first.
pub fn pick(accounts: &Accounts, wanted: Option<u64>) -> Option<u64> {
    wanted
        .filter(|&id| accounts.get(id).is_some())
        .or_else(|| accounts.sorted().first().map(|a| a.character_id))
}

/// The Manage ESI Characters window.
pub fn open_characters_window(hub: Entity<Hub>, cx: &mut App) -> Task<Result<(), SharedString>> {
    open_single(
        "ESI Characters",
        (520., 460.),
        cx,
        |_, _, _| {},
        move |window, cx| EsiCharactersWindow::new(hub, window, cx),
    )
}

impl Efa {
    /// pyfa's Character > Manage ESI Characters.
    pub(super) fn open_esi_characters(&mut self, cx: &mut Context<Self>) {
        let opening = open_characters_window(self.hub.clone(), cx);
        Self::report_open(opening, cx);
    }

    /// Log a character in without opening a window, for the Character menu.
    pub(super) fn add_esi_character(&mut self, cx: &mut Context<Self>) {
        let this = cx.weak_entity();
        self.hub.update(cx, |hub, cx| {
            hub.esi_login(cx, move |result, cx| {
                let failure = result.err().map(|e| format!("ESI login failed: {e}"));
                let _ = this.update(cx, |g, cx| {
                    if let Some(failure) = failure {
                        g.set_error(failure);
                    }
                    cx.notify();
                });
            });
        });
        self.set_status("Log in with EVE in your browser");
        cx.notify();
    }

    /// pyfa's Fit > Browse ESI Fittings (Ctrl+B).
    pub(super) fn open_esi_fittings(&mut self, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded() else { return };
        let hub = self.hub.clone();
        let opening = open_single(
            "Browse ESI Fittings",
            (980., 640.),
            cx,
            |_, _, _| {},
            move |window, cx| fittings::EsiFittingsWindow::new(hub, loaded, window, cx),
        );
        Self::report_open(opening, cx);
    }

    /// pyfa's Fit > Export to ESI (Ctrl+E), for the current fit.
    pub(super) fn open_export_to_eve(&mut self, cx: &mut Context<Self>) {
        let Some(loaded) = self.loaded() else { return };
        let Some(fit) = self.fit().cloned() else {
            self.set_status("Open a fit to export it to EVE");
            cx.notify();
            return;
        };
        if fit.ship.is_none() {
            self.set_status("The fit has no ship to export");
            cx.notify();
            return;
        }
        let hub = self.hub.clone();
        let preferred = self.characters.esi_character(fit.character);
        // Open already, it takes this fit instead.
        let again = (hub.clone(), Rc::clone(&loaded), fit.clone());
        let opening = open_single(
            "Export to EVE",
            (460., 340.),
            cx,
            move |w, _, cx| {
                let (hub, loaded, fit) = again;
                *w = upload::ExportToEveWindow::new(hub, loaded, fit, preferred, cx);
                cx.notify();
            },
            move |_, cx| upload::ExportToEveWindow::new(hub, loaded, fit, preferred, cx),
        );
        Self::report_open(opening, cx);
    }
}
