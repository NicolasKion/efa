//! The update check (shortly after start, then hourly), the notice the main window shows for a
//! new release, and installing it with progress and a restart. The hub owns the one `Updates`
//! entity; the main window and the preferences window watch it.

use std::sync::Arc;
use std::time::Duration;

use gpui_kit::component::{
    ActiveTheme, Sizable,
    button::{Button, ButtonVariants},
    h_flex,
};
use gpui_kit::{
    App, AppContext, Context, Entity, FontWeight, IntoElement, ParentElement, Render, SharedString,
    Styled, Subscription, Task, WeakEntity, Window, div, prelude::FluentBuilder,
};

use crate::update::{self, Available, Progress};

const FIRST_CHECK: Duration = Duration::from_secs(10);
const CHECK_INTERVAL: Duration = Duration::from_hours(1);
const PROGRESS_TICK: Duration = Duration::from_millis(200);

pub enum State {
    Idle,
    Available(Arc<Available>),
    Downloading(Arc<Available>, Arc<Progress>),
    /// Downloaded and verified. Linux and macOS have it installed already and only need a
    /// restart; Windows keeps the installer to run on restart.
    Ready(Arc<Available>, Option<Vec<u8>>),
    Failed(Arc<Available>, SharedString),
}

pub struct Updates {
    pub state: State,
    /// A check is under way.
    pub checking: bool,
    /// The outcome of the last check, for the preferences window.
    pub last_result: Option<SharedString>,
    /// The version "Later" put away; the hourly check does not bring it back.
    dismissed: Option<String>,
    _checker: Task<()>,
    check_task: Option<Task<()>>,
    install_task: Option<Task<()>>,
}

impl Updates {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let checker = cx.spawn(async move |this: WeakEntity<Self>, cx| {
            cx.background_executor().timer(FIRST_CHECK).await;
            loop {
                if this.update(cx, Updates::auto_check).is_err() {
                    break;
                }
                cx.background_executor().timer(CHECK_INTERVAL).await;
            }
        });
        Self {
            state: State::Idle,
            checking: false,
            last_result: None,
            dismissed: None,
            _checker: checker,
            check_task: None,
            install_task: None,
        }
    }

    /// The scheduled check: release builds only, and only with the network and the preference
    /// on.
    fn auto_check(&mut self, cx: &mut Context<Self>) {
        let network = super::hub::preferences(cx).network;
        if cfg!(debug_assertions) || !network.enabled || !network.check_updates {
            return;
        }
        self.check(false, cx);
    }

    /// Ask for the latest release. A manual check shows a release even after "Later".
    pub fn check(&mut self, manual: bool, cx: &mut Context<Self>) {
        let busy = matches!(self.state, State::Downloading(..) | State::Ready(..));
        if self.checking || busy {
            return;
        }
        let network = super::hub::preferences(cx).network;
        if !network.enabled {
            self.last_result = Some("The network is off in Preferences > Network.".into());
            cx.notify();
            return;
        }
        self.checking = true;
        cx.notify();
        let proxy = network.proxy;
        let task = cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let result = cx
                .background_spawn(async move { update::check(&proxy) })
                .await;
            let _ = this.update(cx, |u, cx| {
                u.checking = false;
                match result {
                    Ok(Some(available)) => {
                        u.last_result = Some(format!("efa {} is out.", available.version).into());
                        if manual || u.dismissed.as_deref() != Some(available.version.as_str()) {
                            u.state = State::Available(Arc::new(available));
                        }
                    }
                    Ok(None) => {
                        u.last_result =
                            Some(format!("efa {} is the latest version.", update::VERSION).into());
                    }
                    Err(e) => {
                        eprintln!("update check: {e}");
                        u.last_result = Some(format!("Could not check: {e}").into());
                    }
                }
                cx.notify();
            });
        });
        self.check_task = Some(task);
    }

    /// Download the release and, where the app can, install it right away.
    pub fn install(&mut self, cx: &mut Context<Self>) {
        let available = match &self.state {
            State::Available(a) | State::Failed(a, _) if a.installable() => Arc::clone(a),
            _ => return,
        };
        let progress = Arc::new(Progress::default());
        self.state = State::Downloading(Arc::clone(&available), Arc::clone(&progress));
        cx.notify();
        let task = cx.spawn(async move |this: WeakEntity<Self>, cx| {
            let job = {
                let (available, progress) = (Arc::clone(&available), Arc::clone(&progress));
                cx.background_spawn(async move {
                    let bundle = update::download(&available, &progress)?;
                    if update::INSTALL_RESTARTS {
                        return Ok(Some(bundle));
                    }
                    update::install(&available, bundle).map(|()| None)
                })
            };
            // Redraw for the progress until the download is done.
            let ticker = cx.spawn({
                let this = this.clone();
                async move |cx| {
                    loop {
                        cx.background_executor().timer(PROGRESS_TICK).await;
                        if this.update(cx, |_, cx| cx.notify()).is_err() {
                            break;
                        }
                    }
                }
            });
            let result: Result<Option<Vec<u8>>, String> = job.await;
            drop(ticker);
            let _ = this.update(cx, |u, cx| {
                u.state = match result {
                    Ok(installer) => State::Ready(available, installer),
                    Err(e) => State::Failed(available, e.into()),
                };
                cx.notify();
            });
        });
        self.install_task = Some(task);
    }

    /// Start the new version and quit this one.
    pub fn restart(&mut self, cx: &mut Context<Self>) {
        let State::Ready(available, installer) = &mut self.state else {
            return;
        };
        let available = Arc::clone(available);
        let result = match installer.take() {
            // Runs the installer, which exits efa and starts it again when done.
            Some(bundle) => update::install(&available, bundle),
            None => update::restart(&available),
        };
        match result {
            Ok(()) => cx.quit(),
            Err(e) => {
                self.state = State::Failed(available, e.into());
                cx.notify();
            }
        }
    }

    /// Put the notice away until the next release. An update installed already takes effect on
    /// the next start.
    pub fn later(&mut self, cx: &mut Context<Self>) {
        match &self.state {
            State::Available(a) | State::Ready(a, _) | State::Failed(a, _) => {
                self.dismissed = Some(a.version.clone());
                self.state = State::Idle;
            }
            State::Idle | State::Downloading(..) => {}
        }
        cx.notify();
    }
}

/// The strip under the main window's menu bar, a view of its own so a download's progress
/// redraws only the strip and not the whole window.
pub struct UpdateNotice {
    updates: Entity<Updates>,
    _watch: Subscription,
}

impl UpdateNotice {
    pub fn new(updates: Entity<Updates>, cx: &mut Context<Self>) -> Self {
        let watch = cx.observe(&updates, |_, _, cx| cx.notify());
        Self {
            updates,
            _watch: watch,
        }
    }
}

impl Render for UpdateNotice {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match notice(&self.updates, cx) {
            Some(notice) => notice.into_any_element(),
            None => div().into_any_element(),
        }
    }
}

/// The strip under the main window's menu bar while a release is waiting. None when there is
/// nothing to say.
fn notice(updates: &Entity<Updates>, cx: &App) -> Option<impl IntoElement> {
    let state = &updates.read(cx).state;
    let (available, text, error): (&Arc<Available>, String, bool) = match state {
        State::Idle => return None,
        State::Available(a) if a.installable() => (a, "Install it now?".into(), false),
        State::Available(a) => (a, "Get it from the release page.".into(), false),
        State::Downloading(a, progress) => (
            a,
            progress.percent().map_or_else(
                || "Downloading...".to_owned(),
                |p| format!("Downloading... {p}%"),
            ),
            false,
        ),
        State::Ready(a, _) => (
            a,
            if update::INSTALL_RESTARTS {
                "Downloaded. Restart to install it."
            } else {
                "Installed. Restart to use it."
            }
            .into(),
            false,
        ),
        State::Failed(a, e) => (a, e.to_string(), true),
    };
    let theme = cx.theme();
    let version = available.version.clone();
    let action =
        |id: &'static str, label: &'static str, f: fn(&mut Updates, &mut Context<Updates>)| {
            let updates = updates.clone();
            Button::new(id)
                .label(label)
                .xsmall()
                .on_click(move |_, _, cx| updates.update(cx, f))
        };
    let notes = {
        let url = available.release_url();
        Button::new("update-notes")
            .label("Release notes")
            .xsmall()
            .ghost()
            .on_click(move |_, _, cx| cx.open_url(&url))
    };
    let buttons = match state {
        State::Available(a) | State::Failed(a, _) if a.installable() => vec![
            action("update-install", "Update", Updates::install).primary(),
            action("update-later", "Later", Updates::later).ghost(),
            notes,
        ],
        State::Ready(..) => vec![
            action("update-restart", "Restart now", Updates::restart).primary(),
            action("update-later", "Later", Updates::later).ghost(),
        ],
        State::Downloading(..) => vec![],
        _ => vec![
            notes.primary(),
            action("update-later", "Later", Updates::later).ghost(),
        ],
    };
    Some(
        h_flex()
            .flex_none()
            .gap_3()
            .px_3()
            .py_1()
            .border_b_1()
            .border_color(theme.border)
            .bg(theme.primary.opacity(0.12))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(format!("efa {version} is out")),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .text_xs()
                    .text_color(if error {
                        theme.danger
                    } else {
                        theme.muted_foreground
                    })
                    .child(text),
            )
            .when(!buttons.is_empty(), |d| {
                d.child(h_flex().flex_none().gap_1().children(buttons))
            }),
    )
}

impl super::Efa {
    /// Help > Check for Updates: a check like the one in the preferences, its answer in the
    /// status bar. A release that is out also shows the update notice.
    pub(super) fn check_updates_now(&mut self, cx: &mut Context<Self>) {
        let updates = self.hub.read(cx).updates.clone();
        updates.update(cx, |u, cx| u.check(true, cx));
        self.set_status("Checking for updates...");
        cx.notify();
        cx.spawn(async move |this: WeakEntity<Self>, cx| {
            loop {
                let done =
                    updates.read_with(cx, |u, _| (!u.checking).then(|| u.last_result.clone()));
                match done {
                    Some(result) => {
                        let _ = this.update(cx, |g, cx| {
                            let text = result.unwrap_or_else(|| "Could not check.".into());
                            if text.starts_with("Could not") || text.starts_with("The network") {
                                g.set_error(text);
                            } else {
                                g.set_status(text);
                            }
                            cx.notify();
                        });
                        break;
                    }
                    None => cx.background_executor().timer(PROGRESS_TICK).await,
                }
            }
        })
        .detach();
    }
}
