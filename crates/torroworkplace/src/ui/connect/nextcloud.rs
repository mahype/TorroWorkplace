//! Signing in to a Nextcloud (task W.5): the address, Login Flow v2 in the browser, and how the
//! sign-in ends. The add-account dialog (`ui/wizard.rs`) and the first-run setup (`ui/setup`) both
//! use it; each keeps its own steps, words and footer around it.
//!
//! The sign-in ends in the browser, so [`NextcloudSignIn::observe`] follows it through every reload
//! of `data` (task M1.4's rule): it has ended once the entry with an empty id has been seen and is
//! gone again. It succeeded when an affected account changed – a new Nextcloud account on that
//! server, or an existing one whose `last_sync` is newer than the start of the sign-in or whose
//! `problem` changed (signing in to an account that exists already); else it failed and the
//! account is as it was.

use chrono::{DateTime, Utc};
use iced::Element;
use iced::widget::{column, row};
use torro_core::i18n::tr;
use torro_gui::icon::Icon;
use torro_gui::theme::{self, space};
use torro_gui::{style, typo, widgets};
use torro_ipc::{AccountInfo, Request, Response};
use torro_pim::AccountState;

use super::super::wizard::field;
use super::super::{Action, Data};

/// A Nextcloud sign-in: the address as typed and, while Login Flow v2 runs, what it compares
/// against to tell how it ended.
#[derive(Debug, Clone, Default)]
pub struct NextcloudSignIn {
    /// The server address as typed.
    pub server: String,
    running: Option<Running>,
}

/// A sign-in that runs.
#[derive(Debug, Clone)]
struct Running {
    started: DateTime<Utc>,
    /// Every account with its problem when the sign-in started.
    before: Vec<(String, Option<String>)>,
    /// The entry with an empty id was in a reload, so its absence means the end.
    seen_pending: bool,
    /// The server the core signs in at, from that entry; only its accounts count.
    server: Option<String>,
}

/// How a sign-in that ran has ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ended {
    /// Signed in: the id of the account that is new or changed.
    SignedIn(String),
    /// The browser sign-in finished without a change: nothing was connected.
    Failed,
}

impl NextcloudSignIn {
    /// Whether a sign-in can start: there is an address (spaces do not count) and none runs.
    pub fn can_start(&self) -> bool {
        !self.server.trim().is_empty() && self.running.is_none()
    }

    /// Whether Login Flow v2 runs.
    pub fn running(&self) -> bool {
        self.running.is_some()
    }

    /// Starts Login Flow v2 at the typed address and notes what the accounts are, so
    /// [`NextcloudSignIn::observe`] can tell the end. The core opens the browser.
    pub fn start(&mut self, data: &Data) -> Action {
        let before = data.accounts.iter().map(|account| (account.id.clone(), problem_of(&account.state))).collect();
        self.running = Some(Running { started: Utc::now(), before, seen_pending: false, server: None });
        Action::Send(Request::ConnectNextcloud { server: self.server.trim().to_string() })
    }

    /// Stops waiting for the browser; the address stays.
    pub fn stop(&mut self) {
        self.running = None;
    }

    /// Follows the sign-in through a reload of `data`; `Some` once it has ended.
    pub fn observe(&mut self, data: &Data) -> Option<Ended> {
        let running = self.running.as_mut()?;
        if let Some(pending) = data.accounts.iter().find(|account| account.id.is_empty()) {
            running.seen_pending = true;
            if let AccountState::SigningIn { server, .. } = &pending.state {
                running.server = Some(server.clone());
            }
            return None;
        }
        let ended = match data.accounts.iter().find(|account| signed_in(running, account)) {
            Some(account) => Ended::SignedIn(account.id.clone()),
            None if running.seen_pending => Ended::Failed,
            None => return None,
        };
        self.running = None;
        Some(ended)
    }

    /// The answer to `ConnectNextcloud` while the sign-in runs: an error means it could not start
    /// (no Nextcloud at that address), and the error is handed back; `LoginStarted` only starts it.
    /// `None` when the answer is not the sign-in's.
    pub fn answered(&mut self, result: &Result<Response, String>) -> Option<Result<(), String>> {
        self.running.as_ref()?;
        match result {
            Err(message) | Ok(Response::Error { message, .. }) => {
                self.running = None;
                Some(Err(message.clone()))
            }
            Ok(_) => Some(Ok(())),
        }
    }
}

/// `account` is a Nextcloud account on the server of `running` that changed since it started:
/// new, synced afterwards, or with another problem than before.
fn signed_in(running: &Running, account: &AccountInfo) -> bool {
    let AccountState::Connected { server, last_sync, problem, .. } = &account.state else { return false };
    let same_server = |a: &str, b: &str| a.trim_end_matches('/') == b.trim_end_matches('/');
    if account.kind != "nextcloud" || !running.server.as_deref().is_none_or(|s| same_server(s, server)) {
        return false;
    }
    match running.before.iter().find(|(id, _)| *id == account.id) {
        None => true,
        Some((_, before)) => last_sync.is_some_and(|at| at > running.started) || problem != before,
    }
}

fn problem_of(state: &AccountState) -> Option<String> {
    match state {
        AccountState::Connected { problem, .. } => problem.clone(),
        AccountState::NotConnected | AccountState::SigningIn { .. } => None,
    }
}

/// The login URL of the account that is signing in, if the core reports one.
pub fn login_url(data: &Data) -> Option<&str> {
    data.accounts.iter().find_map(|account| match &account.state {
        AccountState::SigningIn { login_url, .. } => Some(login_url.as_str()),
        _ => None,
    })
}

/// The address field; Enter sends `on_submit` while a sign-in can start.
pub fn address<'a, M: Clone + 'a>(
    sign_in: &'a NextcloudSignIn,
    on_input: fn(String) -> M,
    on_submit: M,
) -> Element<'a, M> {
    field("cloud.example.org", &sign_in.server)
        .on_input(on_input)
        .on_submit_maybe(sign_in.can_start().then_some(on_submit))
        .into()
}

/// While the browser signs in: the explanation, the waiting line, the login link to copy and a
/// button to open it again (`on_open`; `on_edit` is ignored, the link field only lets it be copied).
pub fn waiting<'a, M: Clone + 'a>(data: &'a Data, on_open: M, on_edit: fn(String) -> M) -> Element<'a, M> {
    let login = login_url(data);
    let mut body = column![
        typo::text(
            typo::body(),
            tr("Nextcloud handles the sign-in. You sign in on its own page; Torro Workplace never sees your password.")
        ),
        typo::text(typo::body(), tr("Waiting for the browser …")).style(theme::text_2),
    ]
    .spacing(space::m());
    if let Some(url) = login {
        // `text_input` without an edit handler cannot take focus, so no copying. Editing is
        // allowed but ignored: the field always shows `url`, so the text stays unchanged.
        body = body
            .push(typo::text(typo::body(), tr("If the browser does not open, copy this link:")).style(theme::text_2))
            .push(field("", url).on_input(on_edit));
    }
    let again = widgets::text_button(
        Some(Icon::ArrowUpRight),
        tr("Open the page again"),
        style::button::secondary,
        login.map(|_| on_open),
    );
    body.push(row![again]).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nextcloud(id: &str, last_sync: Option<DateTime<Utc>>, problem: Option<&str>) -> AccountInfo {
        AccountInfo {
            id: id.into(),
            kind: "nextcloud".into(),
            label: id.into(),
            state: AccountState::Connected {
                server: "https://cloud.example.org".into(),
                login_name: "anna".into(),
                last_sync,
                problem: problem.map(Into::into),
            },
            collections: vec![],
            scopes: None,
            mail: None,
            ntfy: None,
            services: None,
        }
    }

    fn pending() -> AccountInfo {
        AccountInfo {
            id: String::new(),
            kind: "nextcloud".into(),
            label: "cloud.example.org".into(),
            state: AccountState::SigningIn {
                server: "https://cloud.example.org".into(),
                login_url: "https://cloud.example.org/login/v2/flow/x".into(),
            },
            collections: vec![],
            scopes: None,
            mail: None,
            ntfy: None,
            services: None,
        }
    }

    fn data(accounts: Vec<AccountInfo>) -> Data {
        Data { accounts, ..Data::default() }
    }

    #[test]
    fn a_new_account_ends_the_sign_in_with_its_id() {
        let mut sign_in = NextcloudSignIn { server: " cloud.example.org ".into(), ..Default::default() };
        assert!(sign_in.can_start());
        match sign_in.start(&data(vec![])) {
            Action::Send(Request::ConnectNextcloud { server }) => assert_eq!(server, "cloud.example.org"),
            other => panic!("{other:?}"),
        }
        assert!(sign_in.running() && !sign_in.can_start(), "one at a time");
        assert_eq!(sign_in.observe(&data(vec![pending()])), None);
        let anna = nextcloud("nextcloud:anna@cloud.example.org", None, None);
        assert_eq!(
            sign_in.observe(&data(vec![anna])),
            Some(Ended::SignedIn("nextcloud:anna@cloud.example.org".into()))
        );
        assert!(!sign_in.running());
    }

    #[test]
    fn an_unchanged_account_after_the_browser_means_it_failed() {
        let anna = nextcloud("nextcloud:anna@cloud.example.org", None, Some("Sign in again"));
        let mut sign_in = NextcloudSignIn { server: "cloud.example.org".into(), ..Default::default() };
        sign_in.start(&data(vec![anna.clone()]));
        assert_eq!(sign_in.observe(&data(vec![anna.clone()])), None, "no sign-in seen yet");
        assert_eq!(sign_in.observe(&data(vec![anna.clone(), pending()])), None);
        assert_eq!(sign_in.observe(&data(vec![anna])), Some(Ended::Failed));
    }

    #[test]
    fn a_refused_start_hands_back_its_error() {
        let mut sign_in = NextcloudSignIn { server: "nowhere.example".into(), ..Default::default() };
        assert_eq!(sign_in.answered(&Err("x".into())), None, "not running: not its answer");
        sign_in.start(&data(vec![]));
        let started = Ok(Response::LoginStarted { login_url: "https://x/login".into() });
        assert_eq!(sign_in.answered(&started), Some(Ok(())));
        assert!(sign_in.running());
        assert_eq!(sign_in.answered(&Err("no Nextcloud".into())), Some(Err("no Nextcloud".into())));
        assert!(!sign_in.running());
    }

    #[test]
    fn spaces_are_no_address() {
        for typed in ["", "   "] {
            assert!(!NextcloudSignIn { server: typed.into(), ..Default::default() }.can_start(), "{typed:?}");
        }
    }
}
