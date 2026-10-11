//! The core's Nextcloud accounts: the stored accounts and their app passwords, signing in with
//! Login Flow v2, revoking an app password on the server when its account is removed, and the
//! remote wipe the server may ask for once it refuses the app password ([`remote_wipe`], task S.12).
//!
//! Task K.6 in `docs/tasks/kalender-aufgaben.md`; several accounts since task M1.1: signing in
//! adds an account, removing one leaves the others. Building blocks that exist:
//! `torro_core::account` (accounts.json + keychain), `torro_pim_caldav::nextcloud`
//! (`normalize_server`, `start_login`, `LoginFlow::poll`, `dav_url`), `torro_pim_caldav::
//! http_client`, `torro_pim_caldav::CalDavClient`. `pim.rs` (task K.5) uses this module.
//!
//! Task M1.4: a sign-in names the account after the server it was started at, and only when the
//! server's answer names the same origin ([`signed_in_account`]), so no server can pose as an
//! account the user has elsewhere and take over its app password. An account whose secret is
//! missing is still loaded ([`Stored::Unusable`]), so it shows up and can be removed.
//!
//! Task G.1: GitHub accounts live in [`github`], apart from the Nextcloud ones: they have no
//! calendars or task lists, so the calendar service never loads them ([`stored`] keeps only
//! Nextcloud accounts) and its sync is not touched.

use std::future::Future;
use std::path::Path;
use std::time::Duration;

use tokio::time::Instant;
use torro_core::Account;
use torro_core::account::{self, AccountKind};
use torro_pim::ProviderError;
use torro_pim_caldav::nextcloud::{self, Credentials, LoginFlow};
use torro_pim_caldav::{CalDavAccount, CalDavClient};

/// How often the sign-in is polled while the user is in the browser.
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// How long the sign-in may take before we give up; Nextcloud drops the poll token after
/// the same time.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(20 * 60);

/// A signed-in account with its secret.
#[derive(Debug, Clone)]
pub struct Connected {
    pub account: Account,
    pub app_password: String,
}

/// One stored Nextcloud account as the core loads it ([`stored`]).
#[derive(Debug, Clone)]
pub enum Stored {
    /// With its app password from the keychain: ready to sync.
    Ready(Connected),
    /// Its app password is missing, or the keychain failed to read it. The core lists it with
    /// `problem` (it starts with "Sign in again"), sends nothing to its server and lets the user
    /// sign in to it again or remove it.
    Unusable { account: Account, problem: String },
}

impl Stored {
    pub fn account(&self) -> &Account {
        match self {
            Stored::Ready(connected) => &connected.account,
            Stored::Unusable { account, .. } => account,
        }
    }
}

/// Every stored Nextcloud account, in the order of `accounts.json` (task M1.1), each with its
/// secret from the keychain. A secret that is missing or cannot be read costs only that account
/// ([`Stored::Unusable`]), never the others (task M1.4). An unreadable `accounts.json` is an error.
pub fn stored() -> Result<Vec<Stored>, String> {
    let accounts = account::load().map_err(|err| err.to_string())?;
    Ok(with_secrets(accounts, |account| account.secret().map_err(|err| err.to_string())))
}

/// The Nextcloud accounts of `accounts` with what `secret` finds for each; the keychain is a
/// parameter so the sorting can be tested without one.
fn with_secrets(accounts: Vec<Account>, secret: impl Fn(&Account) -> Result<Option<String>, String>) -> Vec<Stored> {
    accounts
        .into_iter()
        .filter(|account| account.kind == AccountKind::Nextcloud)
        .map(|account| match secret(&account) {
            Ok(Some(app_password)) => Stored::Ready(Connected { account, app_password }),
            Ok(None) => {
                eprintln!("torroworkplace: {} has no secret in the keychain; sign in again", account.id);
                Stored::Unusable { account, problem: "Sign in again: no app password is stored on this device".into() }
            }
            Err(err) => {
                eprintln!("torroworkplace: cannot read the secret of {}: {err}", account.id);
                Stored::Unusable { account, problem: format!("Sign in again: the keychain could not be read ({err})") }
            }
        })
        .collect()
}

/// The account that owned every collection before the core held several (task M1.1): the
/// first Nextcloud account in the account list at `path`, the one the core used to pick. Ids
/// stored back then (bare URLs) belong to it. `None` without one, or when the list cannot be
/// read; then nothing is rewritten.
pub fn legacy_owner(path: &Path) -> Option<String> {
    match account::load_from(path) {
        Ok(accounts) => accounts.into_iter().find(|account| account.kind == AccountKind::Nextcloud).map(|a| a.id),
        Err(err) => {
            eprintln!("torroworkplace: cannot read {} to update old collection ids: {err}", path.display());
            None
        }
    }
}

/// Starts Login Flow v2 for what the user typed and opens the login URL in the browser
/// (`webbrowser`); a browser that cannot be opened is not an error (the apps show the URL).
pub async fn start(http: &reqwest::Client, server_input: &str) -> Result<LoginFlow, String> {
    let server = nextcloud::normalize_server(server_input).map_err(|err| err.to_string())?;
    let flow = nextcloud::start_login(http, &server).await.map_err(|err| err.to_string())?;
    if let Err(err) = webbrowser::open(&flow.login_url) {
        eprintln!("torroworkplace: cannot open a browser ({err}); open {} by hand", flow.login_url);
    }
    Ok(flow)
}

/// Polls every 2 s until the user granted access (at most 20 minutes), then stores the
/// account next to the others ([`with_account`]) and its app password.
///
/// The account is the one [`signed_in_account`] accepts; a server that names another origin is
/// refused and nothing is stored. Signing in to an account that exists replaces its app
/// password; the old one is then revoked on the server in the background (best effort), so it
/// does not stay valid under "Devices & sessions".
pub async fn finish(http: &reqwest::Client, flow: &LoginFlow) -> Result<Connected, String> {
    let credentials = poll_until_granted(|| flow.poll(http), POLL_INTERVAL, SIGN_IN_TIMEOUT).await?;
    let account = signed_in_account(&flow.server, &credentials)?;
    let connected = Connected { account, app_password: credentials.app_password };
    let storing = connected.clone();
    let replaced = keychain(move || {
        let account = &storing.account;
        // Only to revoke it afterwards: a secret that cannot be read stays valid on the server.
        let replaced = account.secret().ok().flatten();
        account.set_secret(&storing.app_password).map_err(|err| err.to_string())?;
        let accounts = with_account(account::load().map_err(|err| err.to_string())?, account.clone());
        account::save(&accounts).map_err(|err| err.to_string())?;
        Ok(replaced)
    })
    .await?;
    if let Some(old) = replaced.filter(|old| *old != connected.app_password) {
        let (http, old) = (http.clone(), Connected { account: connected.account.clone(), app_password: old });
        tokio::spawn(async move {
            if let Err(err) = revoke(&http, &old).await {
                eprintln!("torroworkplace: the replaced app password was not revoked on the server: {err}");
            }
        });
    }
    Ok(connected)
}

/// The account a finished sign-in adds: named after `server`, where the sign-in started
/// ([`LoginFlow::server`], normalized), with the login name the server reports.
///
/// Refused when the server names another origin in its answer (`credentials.server`), and when
/// the login name is empty or holds a `/`: account ids are the front part of collection ids, and
/// a `/` in one could make it look like the start of another account's.
fn signed_in_account(server: &str, credentials: &Credentials) -> Result<Account, String> {
    if !nextcloud::same_origin(server, &credentials.server) {
        return Err(format!("sign-in refused: started at {server}, but the server named {}", credentials.server));
    }
    let login_name = credentials.login_name.as_str();
    if login_name.is_empty() || login_name.contains('/') {
        return Err(format!("sign-in refused: the server sent an unusable login name {login_name:?}"));
    }
    Ok(Account::nextcloud(server, login_name))
}

/// `accounts` with `account` added at the end, or in place of the one with the same id (signing
/// in to the same account again replaces it and keeps its place). The others stay.
fn with_account(mut accounts: Vec<Account>, account: Account) -> Vec<Account> {
    match accounts.iter_mut().find(|stored| stored.id == account.id) {
        Some(stored) => *stored = account,
        None => accounts.push(account),
    }
    accounts
}

/// Runs keychain work on a plain thread of its own. The Secret Service client starts its own
/// async runtime and panics when called from inside the daemon's tokio runtime.
pub async fn keychain<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    rx.await.unwrap_or_else(|_| Err("keychain access failed".into()))
}

/// Removes the stored account `id` and its secret; the other accounts stay. The secret goes
/// first: a list that still names the account is better than a secret nobody can find.
pub fn forget(id: &str) -> Result<(), String> {
    let mut accounts = account::load().map_err(|err| err.to_string())?;
    if let Some(account) = accounts.iter().find(|account| account.id == id) {
        account.delete_secret().map_err(|err| err.to_string())?;
    }
    accounts.retain(|account| account.id != id);
    account::save(&accounts).map_err(|err| err.to_string())
}

/// Revokes the account's app password on the server, so it disappears from "Devices &
/// sessions" instead of staying valid after the user removed the account here.
///
/// Nextcloud OCS `DELETE <server>/ocs/v2.php/core/apppassword`, authenticated with the very
/// app password it revokes (`core/Controller/AppPasswordController.php::deleteAppPassword`
/// in nextcloud/server): 200 = revoked; 403 = the request was not made with an app password
/// or the token is unknown. Any other status (401 = already revoked or account disabled,
/// 503 = maintenance) is reported as an error too; callers treat revoking as best effort.
pub async fn revoke(http: &reqwest::Client, connected: &Connected) -> Result<(), String> {
    let request = revoke_request(http, connected)?;
    let response = http.execute(request).await.map_err(|err| format!("cannot revoke the app password: {err}"))?;
    revoke_outcome(response.status())
}

/// The OCS request that revokes the app password. Kept apart from [`revoke`] so it can be
/// checked without a network. The secret only travels in the `Authorization` header.
fn revoke_request(http: &reqwest::Client, connected: &Connected) -> Result<reqwest::Request, String> {
    let url = format!("{}/ocs/v2.php/core/apppassword", connected.account.server.trim_end_matches('/'));
    http.delete(url)
        .basic_auth(&connected.account.login_name, Some(&connected.app_password))
        // Without this header OCS refuses the call as a possible CSRF attempt.
        .header("OCS-APIRequest", "true")
        .header(reqwest::header::ACCEPT, "application/json")
        .build()
        .map_err(|err| format!("cannot build the revoke request: {err}"))
}

/// Maps the server's answer to the revoke request.
fn revoke_outcome(status: reqwest::StatusCode) -> Result<(), String> {
    match status.as_u16() {
        200..=299 => Ok(()),
        401 => Err("the server no longer accepts the app password (already revoked?)".into()),
        403 => Err("the server refused to revoke the app password".into()),
        503 => Err("the server is in maintenance mode".into()),
        _ => Err(format!("revoking the app password failed: HTTP {status}")),
    }
}

/// The remote wipe after the server refused the app password (401/403), in the order Nextcloud
/// expects: ask the server (`check`, see `torro_pim_caldav::nextcloud::check_wipe`); only on a
/// clear "wipe" remove everything local (`wipe`), and only after that confirm to the server
/// (`report`, `nextcloud::confirm_wipe`). Confirming first would let the server delete the token
/// while our data is still here.
///
/// `Ok(true)` = wiped; `Ok(false)` = nothing to wipe, the account stays (the user signs in again).
/// A failed check or a failed local wipe is an error and nothing is confirmed. A failed
/// confirmation is only logged: the local data is gone either way, and the token cannot be
/// retried later because it no longer exists here.
///
/// The steps are closures so the order can be tested without a network or keychain (like
/// [`poll_until_granted`]).
pub async fn remote_wipe<Check, CheckFut, Wipe, WipeFut, Report, ReportFut>(
    check: Check,
    wipe: Wipe,
    report: Report,
) -> Result<bool, String>
where
    Check: FnOnce() -> CheckFut,
    CheckFut: Future<Output = Result<bool, ProviderError>>,
    Wipe: FnOnce() -> WipeFut,
    WipeFut: Future<Output = Result<(), String>>,
    Report: FnOnce() -> ReportFut,
    ReportFut: Future<Output = Result<(), ProviderError>>,
{
    if !check().await.map_err(|err| format!("cannot check for a remote wipe: {err}"))? {
        return Ok(false);
    }
    wipe().await.map_err(|err| format!("remote wipe requested, but the local data could not be removed: {err}"))?;
    if let Err(err) = report().await {
        eprintln!("torroworkplace: remote wipe done, but not confirmed to the server: {err}");
    }
    Ok(true)
}

/// A CalDAV client for the account (`<server>/remote.php/dav/`).
pub fn caldav(http: &reqwest::Client, connected: &Connected) -> Result<CalDavClient, String> {
    let dav = CalDavAccount {
        url: nextcloud::dav_url(&connected.account.server),
        username: connected.account.login_name.clone(),
    };
    CalDavClient::new(http.clone(), &dav, connected.app_password.clone()).map_err(|err| err.to_string())
}

/// Calls `poll` every `interval` until it yields credentials. Errors are retried (the
/// network is often gone while the laptop sleeps), but they count toward `timeout`.
async fn poll_until_granted<Poll, Fut>(
    mut poll: Poll,
    interval: Duration,
    timeout: Duration,
) -> Result<Credentials, String>
where
    Poll: FnMut() -> Fut,
    Fut: Future<Output = Result<Option<Credentials>, ProviderError>>,
{
    let deadline = Instant::now() + timeout;
    let mut last_error = None;
    loop {
        match poll().await {
            Ok(Some(credentials)) => return Ok(credentials),
            Ok(None) => {}
            Err(err) => last_error = Some(err.to_string()),
        }
        if Instant::now() >= deadline {
            return Err(match last_error {
                Some(err) => format!("sign-in timed out (last error: {err})"),
                None => "sign-in timed out".to_string(),
            });
        }
        tokio::time::sleep(interval).await;
    }
}

/// GitHub accounts (task G.1): taking over the GitHub CLI's sign-in or a pasted token, checking
/// it with GitHub, and keeping the accounts with their state for `accounts.list_accounts`.
///
/// The token lives in the system keychain under the account id (`github:<login>`) and in memory
/// as a [`Secret`]. It leaves the core only towards GitHub, in the `Authorization` header of
/// [`check`]; no answer, error message or log line carries it.
pub mod github {
    use std::fmt;
    use std::io;
    use std::path::PathBuf;
    use std::process::{Output, Stdio};
    use std::sync::Arc;
    use std::time::Duration;

    use chrono::{DateTime, Utc};
    use tokio::sync::RwLock;
    use torro_core::Account;
    use torro_core::account::{self, AccountKind, Secret};
    use torro_ipc::AccountInfo;
    use torro_pim::AccountState;

    use super::{keychain, with_account};

    /// GitHub's GraphQL endpoint; every check of a token asks it.
    pub const GRAPHQL_URL: &str = "https://api.github.com/graphql";

    /// The command that prints the GitHub CLI's token.
    const CLI: [&str; 3] = ["gh", "auth", "token"];

    /// How long `gh auth token` may take; it only reads its own config or the keychain.
    const CLI_TIMEOUT: Duration = Duration::from_secs(10);

    /// The one question a check asks GitHub: whose token is this?
    const VIEWER_QUERY: &str = r#"{"query":"query { viewer { login } }"}"#;

    /// Fine-grained personal access tokens start with this; GitHub reports no scopes for them.
    const FINE_GRAINED_PREFIX: &str = "github_pat_";

    const NOT_INSTALLED: &str =
        "The GitHub CLI (gh) is not installed. Install it and run `gh auth login`, or paste a token instead.";
    const NOT_SIGNED_IN: &str =
        "The GitHub CLI is not signed in. Run `gh auth login` in a terminal, or paste a token instead.";

    /// Whom a token belongs to and what it may do.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Checked {
        /// The GitHub login, e.g. `mahype`; the account is `github:<login>`.
        pub login: String,
        /// The token's OAuth scopes; `None` when GitHub does not report them (see
        /// `torro_ipc::AccountInfo::scopes`).
        pub scopes: Option<Vec<String>>,
    }

    /// Why connecting or checking a GitHub account failed. No message ever holds the token.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum GithubError {
        /// The GitHub CLI is missing or not signed in; the text says what to do.
        NoCli(&'static str),
        /// GitHub refused the token (HTTP 401).
        Refused,
        /// GitHub could not be reached.
        Offline,
        /// Anything else, as a sentence for people.
        Other(String),
    }

    impl GithubError {
        /// The error code of the answer (`torro_ipc::Response::Error`).
        pub fn code(&self) -> &'static str {
            match self {
                GithubError::NoCli(_) => "not-connected",
                GithubError::Refused => "auth",
                GithubError::Offline => "offline",
                GithubError::Other(_) => "error",
            }
        }
    }

    impl fmt::Display for GithubError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                GithubError::NoCli(hint) => f.write_str(hint),
                GithubError::Refused => {
                    f.write_str("GitHub did not accept the token. Check that it is complete and has not expired.")
                }
                GithubError::Offline => {
                    f.write_str("GitHub could not be reached. Check the internet connection and try again.")
                }
                GithubError::Other(message) => f.write_str(message),
            }
        }
    }

    /// Asks GitHub whose `token` this is: GraphQL `viewer { login }` at `url` ([`GRAPHQL_URL`]),
    /// the scopes from the `X-OAuth-Scopes` header ([`scopes_of`]).
    pub async fn check(http: &reqwest::Client, url: &str, token: &Secret) -> Result<Checked, GithubError> {
        let request = check_request(http, url, token)?;
        let response = http.execute(request).await.map_err(unreachable)?;
        let status = response.status().as_u16();
        let scopes = response.headers().get("x-oauth-scopes").and_then(|v| v.to_str().ok()).map(str::to_owned);
        let body = response.bytes().await.map_err(unreachable)?;
        read_answer(status, scopes.as_deref(), &body, token)
    }

    /// The GraphQL request of [`check`]; the token only travels in the `Authorization` header,
    /// which reqwest marks as sensitive.
    fn check_request(http: &reqwest::Client, url: &str, token: &Secret) -> Result<reqwest::Request, GithubError> {
        http.post(url)
            .bearer_auth(token.expose())
            .header(reqwest::header::ACCEPT, "application/json")
            .header(reqwest::header::CONTENT_TYPE, "application/json")
            .body(VIEWER_QUERY)
            .build()
            .map_err(|err| GithubError::Other(format!("Cannot build the request to GitHub: {}", err.without_url())))
    }

    fn unreachable(err: reqwest::Error) -> GithubError {
        if err.is_connect() || err.is_timeout() {
            GithubError::Offline
        } else {
            GithubError::Other(format!("GitHub could not be asked: {}", err.without_url()))
        }
    }

    /// GitHub's answer to [`check_request`]: HTTP status, `X-OAuth-Scopes` and body.
    fn read_answer(status: u16, scopes: Option<&str>, body: &[u8], token: &Secret) -> Result<Checked, GithubError> {
        match status {
            200..=299 => {}
            401 => return Err(GithubError::Refused),
            403 => {
                return Err(GithubError::Other(
                    "GitHub refused the request (HTTP 403): the token may need single sign-on approval, or the \
                     rate limit is reached."
                        .into(),
                ));
            }
            other => return Err(GithubError::Other(format!("GitHub answered with HTTP {other}."))),
        }
        let answer: serde_json::Value = serde_json::from_slice(body)
            .map_err(|_| GithubError::Other("GitHub sent an answer that is not JSON.".into()))?;
        match answer.pointer("/data/viewer/login").and_then(serde_json::Value::as_str) {
            Some(login) if usable_login(login) => {
                Ok(Checked { login: login.to_string(), scopes: scopes_of(token, scopes) })
            }
            Some(_) => Err(GithubError::Other("GitHub sent a login Torro Workplace cannot use.".into())),
            None => {
                Err(GithubError::Other(match answer.pointer("/errors/0/message").and_then(serde_json::Value::as_str) {
                    Some(message) => format!("GitHub answered with an error: {message}"),
                    None => "GitHub did not say whose token this is.".into(),
                }))
            }
        }
    }

    /// A login fit for an account id: GitHub's own characters (letters, digits, `-`, and
    /// `[bot]` for app accounts), so never a `/`, which would make the id look like the start of
    /// a collection id.
    fn usable_login(login: &str) -> bool {
        !login.is_empty()
            && login.len() <= 100
            && login.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '[' | ']'))
    }

    /// The scopes in `X-OAuth-Scopes` (`"repo, read:org"`), as GitHub sends it for classic tokens
    /// and the GitHub CLI's OAuth token; a token without scopes gets an empty header, so an empty
    /// list. `None` without the header and always for fine-grained tokens (`github_pat_…`): their
    /// permissions are set per repository and GitHub does not report them.
    fn scopes_of(token: &Secret, header: Option<&str>) -> Option<Vec<String>> {
        if token.expose().starts_with(FINE_GRAINED_PREFIX) {
            return None;
        }
        header.map(|header| header.split(',').map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned).collect())
    }

    /// The GitHub CLI's token: runs `command` (program and arguments, `gh auth token`) and reads
    /// what it prints. [`GithubError::NoCli`] when the program is missing or prints no token.
    pub async fn cli_token(command: &[String]) -> Result<Secret, GithubError> {
        let Some((program, args)) = command.split_first() else {
            return Err(GithubError::NoCli(NOT_INSTALLED));
        };
        let run = tokio::process::Command::new(program)
            .args(args)
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .output();
        match tokio::time::timeout(CLI_TIMEOUT, run).await {
            Ok(result) => cli_outcome(result),
            Err(_) => Err(GithubError::Other("The GitHub CLI did not answer in time.".into())),
        }
    }

    /// What a run of `gh auth token` means. Its output is the token, so it never goes into a
    /// message.
    fn cli_outcome(result: io::Result<Output>) -> Result<Secret, GithubError> {
        let output = match result {
            Ok(output) => output,
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Err(GithubError::NoCli(NOT_INSTALLED)),
            Err(err) => return Err(GithubError::Other(format!("The GitHub CLI could not be started: {err}"))),
        };
        let token = String::from_utf8(output.stdout)
            .ok()
            .map(|token| token.trim().to_owned())
            .filter(|token| !token.is_empty() && !token.contains(char::is_whitespace));
        match token {
            Some(token) if output.status.success() => Ok(Secret::new(token)),
            _ => Err(GithubError::NoCli(NOT_SIGNED_IN)),
        }
    }

    /// Where the tokens are kept: the system keychain in the core ([`SystemTokens`]), a map in
    /// tests, so tests never touch the user's keychain. Errors never hold a token.
    pub trait TokenStore: Send + Sync {
        /// The token of `account`; `None` when none is stored.
        fn get(&self, account: &Account) -> Result<Option<Secret>, String>;
        /// Stores `token` for `account`, replacing what was there.
        fn set(&self, account: &Account, token: &Secret) -> Result<(), String>;
        /// Removes the token of `account`; a missing entry is fine.
        fn delete(&self, account: &Account) -> Result<(), String>;
    }

    /// The system keychain, under the account id like every account secret.
    pub struct SystemTokens;

    impl TokenStore for SystemTokens {
        fn get(&self, account: &Account) -> Result<Option<Secret>, String> {
            account.secret().map(|secret| secret.map(Secret::new)).map_err(|err| err.to_string())
        }

        fn set(&self, account: &Account, token: &Secret) -> Result<(), String> {
            account.set_secret(token.expose()).map_err(|err| err.to_string())
        }

        fn delete(&self, account: &Account) -> Result<(), String> {
            account.delete_secret().map_err(|err| err.to_string())
        }
    }

    /// One GitHub account as the core holds it.
    #[derive(Debug)]
    struct Slot {
        account: Account,
        /// `None` when the keychain had none or could not be read; `problem` then says so.
        token: Option<Secret>,
        /// When GitHub last accepted the token.
        checked: Option<DateTime<Utc>>,
        problem: Option<String>,
        scopes: Option<Vec<String>>,
    }

    impl Slot {
        fn info(&self) -> AccountInfo {
            AccountInfo {
                id: self.account.id.clone(),
                kind: "github".into(),
                label: self.account.login_name.clone(),
                state: AccountState::Connected {
                    server: self.account.server.clone(),
                    login_name: self.account.login_name.clone(),
                    last_sync: self.checked,
                    problem: self.problem.clone(),
                },
                collections: Vec::new(),
                scopes: self.scopes.clone(),
            }
        }

        /// Notes what a new check of the token found at `now`. A token GitHub no longer takes,
        /// or one that now belongs to someone else, needs a new sign-in; being offline keeps the
        /// last known scopes.
        fn apply(&mut self, result: Result<Checked, GithubError>, now: DateTime<Utc>) {
            match result {
                Ok(checked) if checked.login != self.account.login_name => {
                    self.problem = Some("Sign in again: the token now belongs to another GitHub account".into());
                }
                Ok(checked) => {
                    self.checked = Some(now);
                    self.problem = None;
                    self.scopes = checked.scopes;
                }
                Err(GithubError::Refused) => {
                    self.problem = Some("Sign in again: GitHub no longer accepts the token".into());
                }
                Err(err) => self.problem = Some(err.to_string()),
            }
        }
    }

    /// The core's GitHub accounts: loaded at start ([`GithubAccounts::start`]), connected through
    /// `accounts.connect_github` and `accounts.set_github_token`, checked again on
    /// `accounts.sync`, removed with `accounts.remove_account`.
    pub struct GithubAccounts {
        /// `accounts.json`; `None` without a home directory, and then nothing can be stored.
        accounts_file: Option<PathBuf>,
        tokens: Arc<dyn TokenStore>,
        /// `None` when no HTTP client could be built; every check then fails with a message.
        http: Option<reqwest::Client>,
        /// GitHub's GraphQL endpoint ([`GRAPHQL_URL`]); a fake server in tests.
        graphql: String,
        /// The command that prints the GitHub CLI's token ([`cli_token`]).
        cli: Vec<String>,
        /// In the order of `accounts.json`; a new account comes last.
        slots: RwLock<Vec<Slot>>,
    }

    impl GithubAccounts {
        /// The service on the user's account list and keychain, as the daemon runs it. Reads and
        /// sends nothing yet; [`GithubAccounts::start`] loads the accounts.
        pub fn system() -> GithubAccounts {
            let http = match torro_pim_caldav::http_client() {
                Ok(http) => Some(http),
                Err(err) => {
                    eprintln!("torroworkplace: no HTTP client for GitHub: {err}");
                    None
                }
            };
            let cli = CLI.iter().map(|word| word.to_string()).collect();
            GithubAccounts::new(account::accounts_file().ok(), Arc::new(SystemTokens), http, GRAPHQL_URL.into(), cli)
        }

        /// The service on `accounts_file` and `tokens`, asking GitHub at `graphql` with `http` and
        /// the GitHub CLI with `cli` (program and arguments). Tests pass a temp file, a map, a
        /// fake server and a script. Reads and sends nothing yet.
        pub fn new(
            accounts_file: Option<PathBuf>,
            tokens: Arc<dyn TokenStore>,
            http: Option<reqwest::Client>,
            graphql: String,
            cli: Vec<String>,
        ) -> GithubAccounts {
            GithubAccounts { accounts_file, tokens, http, graphql, cli, slots: RwLock::new(Vec::new()) }
        }

        /// Loads the stored GitHub accounts with their tokens and checks each with GitHub, in the
        /// background. A missing token or a refused one costs only that account: it is listed
        /// with a problem ("Sign in again …") and can be removed.
        pub fn start(self: &Arc<Self>) {
            let service = Arc::clone(self);
            tokio::spawn(async move {
                if let Err(err) = service.load().await {
                    eprintln!("torroworkplace: cannot read the GitHub accounts: {err}");
                }
                if let Err(err) = service.check_all().await {
                    eprintln!("torroworkplace: a GitHub account could not be checked: {err}");
                }
            });
        }

        /// Reads the GitHub accounts of `accounts.json` and their tokens.
        async fn load(&self) -> Result<(), String> {
            let Some(path) = self.accounts_file.clone() else { return Ok(()) };
            let tokens = Arc::clone(&self.tokens);
            let loaded = keychain(move || {
                let accounts = account::load_from(&path).map_err(|err| err.to_string())?;
                Ok(accounts
                    .into_iter()
                    .filter(|account| account.kind == AccountKind::GitHub)
                    .map(|account| {
                        let token = tokens.get(&account);
                        (account, token)
                    })
                    .collect::<Vec<_>>())
            })
            .await?;
            let slots = loaded.into_iter().map(|(account, token)| {
                let (token, problem) = match token {
                    Ok(Some(token)) => (Some(token), None),
                    Ok(None) => (None, Some("Sign in again: no token is stored on this device".to_string())),
                    Err(err) => (None, Some(format!("Sign in again: the keychain could not be read ({err})"))),
                };
                Slot { account, token, checked: None, problem, scopes: None }
            });
            let mut held = self.slots.write().await;
            for slot in slots {
                // An account connected while loading is newer than the stored one.
                if !held.iter().any(|held| held.account.id == slot.account.id) {
                    held.push(slot);
                }
            }
            Ok(())
        }

        /// The accounts as `accounts.list_accounts` lists them, never with their tokens.
        pub async fn infos(&self) -> Vec<AccountInfo> {
            self.slots.read().await.iter().map(Slot::info).collect()
        }

        /// `id` is one of these accounts.
        pub async fn knows(&self, id: &str) -> bool {
            self.slots.read().await.iter().any(|slot| slot.account.id == id)
        }

        /// Takes over the GitHub CLI's sign-in ([`cli_token`]) and connects its account
        /// ([`GithubAccounts::connect_token`]).
        pub async fn connect_cli(&self) -> Result<AccountInfo, GithubError> {
            let token = cli_token(&self.cli).await?;
            self.connect(token).await
        }

        /// Checks `token` with GitHub and, when GitHub takes it, stores it in the keychain and the
        /// account in `accounts.json` (next to the others; the same login replaces its token),
        /// then answers with the account. Nothing is stored when the check fails.
        pub async fn connect_token(&self, token: Secret) -> Result<AccountInfo, GithubError> {
            if token.is_blank() {
                return Err(GithubError::Other("The token is empty. Paste the whole token.".into()));
            }
            self.connect(Secret::new(token.expose().trim())).await
        }

        async fn connect(&self, token: Secret) -> Result<AccountInfo, GithubError> {
            let checked = check(self.http()?, &self.graphql, &token).await?;
            let account = Account::github(&checked.login);
            self.store(&account, &token).await.map_err(GithubError::Other)?;
            let slot =
                Slot { account, token: Some(token), checked: Some(Utc::now()), problem: None, scopes: checked.scopes };
            let info = slot.info();
            let mut slots = self.slots.write().await;
            match slots.iter_mut().find(|held| held.account.id == slot.account.id) {
                Some(held) => *held = slot,
                None => slots.push(slot),
            }
            Ok(info)
        }

        /// The token into the keychain, then the account into `accounts.json`: a stored token
        /// nobody lists is better than a listed account without one.
        async fn store(&self, account: &Account, token: &Secret) -> Result<(), String> {
            let path = self.accounts_file.clone().ok_or("No home directory, so the account cannot be stored.")?;
            let (tokens, account, token) = (Arc::clone(&self.tokens), account.clone(), token.clone());
            keychain(move || {
                tokens.set(&account, &token)?;
                let accounts = account::load_from(&path).map_err(|err| err.to_string())?;
                account::save_to(&path, &with_account(accounts, account)).map_err(|err| err.to_string())
            })
            .await
        }

        /// Checks every account's token with GitHub again (`accounts.sync`): scopes and the time
        /// of the check are renewed, a refused token becomes the account's problem. Answers with
        /// the first failure; the other accounts are checked anyway.
        pub async fn check_all(&self) -> Result<(), GithubError> {
            let due: Vec<(String, Secret)> = self
                .slots
                .read()
                .await
                .iter()
                .filter_map(|slot| slot.token.clone().map(|token| (slot.account.id.clone(), token)))
                .collect();
            let mut first = None;
            for (id, token) in due {
                let result = match self.http() {
                    Ok(http) => check(http, &self.graphql, &token).await,
                    Err(err) => Err(err),
                };
                if let Err(err) = &result {
                    first.get_or_insert_with(|| err.clone());
                }
                if let Some(slot) = self.slots.write().await.iter_mut().find(|slot| slot.account.id == id) {
                    slot.apply(result, Utc::now());
                }
            }
            first.map_or(Ok(()), Err)
        }

        /// Removes the account `id`, its token and its entry in `accounts.json`; the other
        /// accounts stay. The token goes first, as for Nextcloud accounts.
        pub async fn remove(&self, id: &str) -> Result<(), String> {
            let account = self.slots.read().await.iter().find(|slot| slot.account.id == id).map(|s| s.account.clone());
            let Some(account) = account else { return Err(format!("no account {id:?}")) };
            let (tokens, path) = (Arc::clone(&self.tokens), self.accounts_file.clone());
            keychain(move || {
                tokens.delete(&account)?;
                let Some(path) = path else { return Ok(()) };
                let mut accounts = account::load_from(&path).map_err(|err| err.to_string())?;
                accounts.retain(|stored| stored.id != account.id);
                account::save_to(&path, &accounts).map_err(|err| err.to_string())
            })
            .await?;
            self.slots.write().await.retain(|slot| slot.account.id != id);
            Ok(())
        }

        fn http(&self) -> Result<&reqwest::Client, GithubError> {
            self.http.as_ref().ok_or_else(|| GithubError::Other("No HTTP client to ask GitHub with.".into()))
        }
    }

    #[cfg(test)]
    pub(crate) mod tests {
        use std::collections::HashMap;
        use std::sync::Mutex;

        use super::*;

        /// A keychain in memory, shared with the test so it can look inside.
        #[derive(Clone, Default)]
        pub(crate) struct MemoryTokens(Arc<Mutex<HashMap<String, String>>>);

        impl MemoryTokens {
            pub(crate) fn get_plain(&self, id: &str) -> Option<String> {
                self.0.lock().expect("lock").get(id).cloned()
            }

            pub(crate) fn put(&self, id: &str, token: &str) {
                self.0.lock().expect("lock").insert(id.into(), token.into());
            }
        }

        impl TokenStore for MemoryTokens {
            fn get(&self, account: &Account) -> Result<Option<Secret>, String> {
                Ok(self.get_plain(&account.id).map(Secret::new))
            }

            fn set(&self, account: &Account, token: &Secret) -> Result<(), String> {
                self.put(&account.id, token.expose());
                Ok(())
            }

            fn delete(&self, account: &Account) -> Result<(), String> {
                self.0.lock().expect("lock").remove(&account.id);
                Ok(())
            }
        }

        /// What the fake GitHub saw of one request.
        #[derive(Debug, Clone)]
        pub(crate) struct Seen {
            pub(crate) path: String,
            pub(crate) authorization: Option<String>,
            pub(crate) body: String,
        }

        /// Status, `X-OAuth-Scopes` and body of an answer of the fake GitHub.
        pub(crate) type Reply = (u16, Option<&'static str>, String);

        /// A GitHub GraphQL endpoint on `127.0.0.1` that answers with `reply` and records every
        /// request, one request per connection. Returns its URL and the record.
        pub(crate) async fn fake_github(
            reply: impl Fn(&Seen) -> Reply + Send + Sync + 'static,
        ) -> (String, Arc<Mutex<Vec<Seen>>>) {
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind loopback");
            let url = format!("http://{}/graphql", listener.local_addr().expect("address"));
            let record = Arc::new(Mutex::new(Vec::new()));
            let (log, reply) = (Arc::clone(&record), Arc::new(reply));
            tokio::spawn(async move {
                while let Ok((mut stream, _)) = listener.accept().await {
                    let (log, reply) = (Arc::clone(&log), Arc::clone(&reply));
                    tokio::spawn(async move {
                        let mut buf = Vec::new();
                        let mut chunk = [0u8; 4096];
                        let head_end = loop {
                            match stream.read(&mut chunk).await {
                                Ok(0) | Err(_) => return,
                                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                            }
                            if let Some(at) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                                break at + 4;
                            }
                        };
                        let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
                        let header = |name: &str| {
                            head.lines()
                                .filter_map(|line| line.split_once(':'))
                                .find(|(key, _)| key.trim().eq_ignore_ascii_case(name))
                                .map(|(_, value)| value.trim().to_string())
                        };
                        let length: usize = header("content-length").and_then(|v| v.parse().ok()).unwrap_or(0);
                        while buf.len() < head_end + length {
                            match stream.read(&mut chunk).await {
                                Ok(0) | Err(_) => break,
                                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                            }
                        }
                        let path = head.lines().next().unwrap_or_default().split(' ').nth(1).unwrap_or_default();
                        let seen = Seen {
                            path: path.to_string(),
                            authorization: header("authorization"),
                            body: String::from_utf8_lossy(&buf[head_end..]).to_string(),
                        };
                        let (status, scopes, body) = reply(&seen);
                        log.lock().expect("record").push(seen);
                        let scopes = scopes.map(|s| format!("X-OAuth-Scopes: {s}\r\n")).unwrap_or_default();
                        let answer = format!(
                            "HTTP/1.1 {status} Fake\r\n{scopes}Content-Type: application/json\r\n\
                             Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = stream.write_all(answer.as_bytes()).await;
                        let _ = stream.shutdown().await;
                    });
                }
            });
            (url, record)
        }

        /// GitHub's answer to the viewer query for `login`.
        pub(crate) fn viewer(login: &str) -> String {
            format!(r#"{{"data":{{"viewer":{{"login":"{login}"}}}}}}"#)
        }

        pub(crate) fn http() -> reqwest::Client {
            reqwest::Client::builder().no_proxy().build().expect("http client")
        }

        /// The service on `dir/config/accounts.json`, `tokens`, the fake GitHub at `graphql` and
        /// the CLI command `cli`.
        pub(crate) fn service(
            dir: &std::path::Path,
            tokens: &MemoryTokens,
            graphql: &str,
            cli: &[&str],
        ) -> GithubAccounts {
            GithubAccounts::new(
                Some(accounts_file(dir)),
                Arc::new(tokens.clone()),
                Some(http()),
                graphql.into(),
                cli.iter().map(|word| word.to_string()).collect(),
            )
        }

        pub(crate) fn accounts_file(dir: &std::path::Path) -> PathBuf {
            dir.join("config").join(account::ACCOUNTS_FILE)
        }

        /// A command that is never installed: the GitHub CLI is missing.
        pub(crate) const NO_CLI: &[&str] = &["torro-test-no-such-gh-command", "auth", "token"];

        const CLASSIC: &str = "ghp_classicSECRET";

        #[tokio::test]
        async fn a_check_asks_for_the_viewer_with_the_token_in_the_header_only() {
            let (url, record) = fake_github(|_| (200, Some("repo, read:org"), viewer("mahype"))).await;
            let checked = check(&http(), &url, &Secret::new(CLASSIC)).await;
            assert_eq!(
                checked,
                Ok(Checked { login: "mahype".into(), scopes: Some(vec!["repo".into(), "read:org".into()]) })
            );
            let seen = record.lock().expect("record")[0].clone();
            assert_eq!(seen.path, "/graphql");
            assert_eq!(seen.authorization.as_deref(), Some("Bearer ghp_classicSECRET"));
            assert_eq!(seen.body, VIEWER_QUERY);
            let request = check_request(&http(), &url, &Secret::new(CLASSIC)).expect("request");
            assert!(!request.url().as_str().contains("SECRET"), "never in the URL");
        }

        #[test]
        fn scopes_come_from_the_header_except_for_fine_grained_tokens() {
            let classic = Secret::new(CLASSIC);
            assert_eq!(
                scopes_of(&classic, Some("repo, read:project")),
                Some(vec!["repo".into(), "read:project".into()])
            );
            assert_eq!(scopes_of(&classic, Some("")), Some(vec![]), "a token without scopes");
            assert_eq!(scopes_of(&classic, None), None, "GitHub did not tell");
            assert_eq!(scopes_of(&Secret::new("github_pat_11AB"), Some("repo")), None);
        }

        #[test]
        fn answers_other_than_a_login_are_errors_without_the_token() {
            let token = Secret::new(CLASSIC);
            let cases = [
                (401, viewer("x"), "auth"),
                (403, String::new(), "error"),
                (502, String::new(), "error"),
                (200, "not json".to_string(), "error"),
                (200, r#"{"errors":[{"message":"Something went wrong"}]}"#.to_string(), "error"),
                (200, viewer("anna/https:"), "error"),
                (200, viewer(""), "error"),
            ];
            for (status, body, code) in cases {
                let err = read_answer(status, None, body.as_bytes(), &token).expect_err("not a login");
                assert_eq!(err.code(), code, "{status} {body}");
                assert!(!err.to_string().contains("SECRET") && !format!("{err:?}").contains("SECRET"));
            }
            assert_eq!(
                read_answer(200, None, viewer("dependabot[bot]").as_bytes(), &token).map(|c| c.login),
                Ok("dependabot[bot]".into())
            );
        }

        #[tokio::test]
        async fn an_unreachable_github_is_offline() {
            // A port nothing listens on: bound, then let go.
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
            let url = format!("http://{}/graphql", listener.local_addr().expect("address"));
            drop(listener);
            assert_eq!(check(&http(), &url, &Secret::new(CLASSIC)).await, Err(GithubError::Offline));
        }

        #[tokio::test]
        async fn a_missing_cli_says_how_to_go_on() {
            let command: Vec<String> = NO_CLI.iter().map(|w| w.to_string()).collect();
            let err = cli_token(&command).await.expect_err("not installed");
            assert_eq!((err.code(), err.clone()), ("not-connected", GithubError::NoCli(NOT_INSTALLED)));
            assert!(cli_token(&[]).await.is_err());
        }

        #[cfg(unix)]
        #[tokio::test]
        async fn the_cli_token_is_what_gh_prints() {
            let run = |script: &str| vec!["sh".to_string(), "-c".to_string(), script.to_string()];
            assert_eq!(cli_token(&run("printf 'gho_token\\n'")).await, Ok(Secret::new("gho_token")));
            // `gh auth token` without a sign-in fails; printing nothing is no token either.
            for script in ["echo 'no oauth token' >&2; exit 1", "true", "printf 'two words'"] {
                assert_eq!(cli_token(&run(script)).await, Err(GithubError::NoCli(NOT_SIGNED_IN)), "{script}");
            }
        }

        #[tokio::test]
        async fn connecting_stores_the_token_in_the_keychain_and_the_account_without_it() {
            let dir = tempfile::tempdir().expect("temp dir");
            let tokens = MemoryTokens::default();
            let nextcloud = Account::nextcloud("https://cloud.example.org", "anna");
            account::save_to(&accounts_file(dir.path()), std::slice::from_ref(&nextcloud)).expect("saved");
            let (url, _) = fake_github(|_| (200, Some("repo"), viewer("mahype"))).await;
            let service = service(dir.path(), &tokens, &url, NO_CLI);

            let info = service.connect_token(Secret::new(format!("  {CLASSIC}\n"))).await.expect("connected");
            assert_eq!(
                (info.id.as_str(), info.kind.as_str(), info.label.as_str()),
                ("github:mahype", "github", "mahype")
            );
            assert_eq!(info.scopes, Some(vec!["repo".to_string()]));
            assert!(matches!(info.state, AccountState::Connected { last_sync: Some(_), problem: None, .. }));
            assert_eq!(tokens.get_plain("github:mahype").as_deref(), Some(CLASSIC), "trimmed, in the keychain");
            let stored = account::load_from(&accounts_file(dir.path())).expect("read");
            assert_eq!(stored, [nextcloud, Account::github("mahype")], "added next to the others");
            let file = std::fs::read_to_string(accounts_file(dir.path())).expect("file");
            assert!(!file.contains("SECRET"), "{file}");
            assert_eq!(service.infos().await, std::slice::from_ref(&info));
            assert!(!format!("{:?}", service.slots.read().await).contains("SECRET"));
            assert!(!serde_json::to_string(&info).expect("json").contains("SECRET"));

            // The same login again replaces the token and keeps one account.
            service.connect_token(Secret::new("ghp_otherSECRET")).await.expect("again");
            assert_eq!(service.infos().await.len(), 1);
            assert_eq!(tokens.get_plain("github:mahype").as_deref(), Some("ghp_otherSECRET"));
            assert_eq!(account::load_from(&accounts_file(dir.path())).expect("read").len(), 2);
        }

        #[tokio::test]
        async fn a_refused_token_stores_nothing() {
            let dir = tempfile::tempdir().expect("temp dir");
            let tokens = MemoryTokens::default();
            let (url, _) = fake_github(|_| (401, None, r#"{"message":"Bad credentials"}"#.into())).await;
            let service = service(dir.path(), &tokens, &url, NO_CLI);
            assert_eq!(service.connect_token(Secret::new(CLASSIC)).await, Err(GithubError::Refused));
            assert_eq!(service.connect_token(Secret::new("  ")).await.map_err(|e| e.code()), Err("error"));
            assert!(service.infos().await.is_empty());
            assert!(!accounts_file(dir.path()).exists());
            assert_eq!(tokens.get_plain("github:mahype"), None);
            assert_eq!(service.connect_cli().await, Err(GithubError::NoCli(NOT_INSTALLED)));
        }

        #[tokio::test]
        async fn stored_accounts_load_with_their_tokens_and_a_missing_one_asks_to_sign_in_again() {
            let dir = tempfile::tempdir().expect("temp dir");
            let tokens = MemoryTokens::default();
            tokens.put("github:anna", CLASSIC);
            let accounts = [
                Account::nextcloud("https://cloud.example.org", "anna"),
                Account::github("anna"),
                Account::github("bob"),
            ];
            account::save_to(&accounts_file(dir.path()), &accounts).expect("saved");
            let (url, record) = fake_github(|seen| match seen.authorization.as_deref() {
                Some("Bearer ghp_classicSECRET") => (200, Some("repo, read:project"), viewer("anna")),
                _ => (401, None, String::new()),
            })
            .await;
            let service = service(dir.path(), &tokens, &url, NO_CLI);
            service.load().await.expect("loaded");
            let ids: Vec<String> = service.infos().await.into_iter().map(|info| info.id).collect();
            assert_eq!(ids, ["github:anna", "github:bob"], "only the GitHub accounts, in their order");

            assert!(service.check_all().await.is_ok(), "bob has no token to check");
            assert_eq!(record.lock().expect("record").len(), 1, "only anna's token went to GitHub");
            let infos = service.infos().await;
            assert!(matches!(&infos[0].state, AccountState::Connected { last_sync: Some(_), problem: None, .. }));
            assert_eq!(infos[0].scopes, Some(vec!["repo".to_string(), "read:project".to_string()]));
            assert!(
                matches!(&infos[1].state, AccountState::Connected { problem: Some(p), .. } if p.starts_with("Sign in again")),
                "{:?}",
                infos[1].state
            );

            // The token was revoked on GitHub meanwhile.
            tokens.put("github:anna", "ghp_revoked");
            service.slots.write().await[0].token = Some(Secret::new("ghp_revoked"));
            assert_eq!(service.check_all().await, Err(GithubError::Refused));
            let state = service.infos().await[0].state.clone();
            assert!(
                matches!(state, AccountState::Connected { problem: Some(p), .. } if p.starts_with("Sign in again"))
            );

            service.remove("github:bob").await.expect("removed");
            let left: Vec<String> =
                account::load_from(&accounts_file(dir.path())).expect("read").into_iter().map(|a| a.id).collect();
            assert_eq!(left, ["nextcloud:anna@cloud.example.org", "github:anna"]);
            assert!(!service.knows("github:bob").await && service.knows("github:anna").await);
            service.remove("github:anna").await.expect("removed");
            assert_eq!(tokens.get_plain("github:anna"), None, "its token is gone too");
            assert!(service.remove("github:anna").await.is_err(), "unknown now");
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::VecDeque;

    use super::*;

    fn granted() -> Credentials {
        Credentials {
            server: "https://cloud.example.org".into(),
            login_name: "anna".into(),
            app_password: "app-password".into(),
        }
    }

    fn connected() -> Connected {
        Connected { account: Account::nextcloud("https://cloud.example.org/", "anna"), app_password: "secret".into() }
    }

    #[test]
    fn signing_in_adds_an_account_and_replaces_only_the_same_one() {
        let anna = Account::nextcloud("https://cloud.example.org", "anna");
        let bob = Account::nextcloud("https://other.example.org", "bob");
        let accounts = with_account(vec![anna.clone()], bob.clone());
        assert_eq!(accounts, [anna.clone(), bob.clone()]);
        let moved = Account { server: "https://cloud.example.org/".into(), ..anna.clone() };
        assert_eq!(with_account(accounts, moved.clone()), [moved, bob], "same id: replaced in place");
    }

    #[test]
    fn a_sign_in_is_named_after_the_server_it_started_at() {
        let account = signed_in_account("https://cloud.example.org", &granted()).expect("same server");
        assert_eq!(account.id, "nextcloud:anna@cloud.example.org");
        assert_eq!(account.server, "https://cloud.example.org");
        // Nextcloud may spell its address differently; the id follows what the user signed in at.
        let reported = Credentials { server: "https://cloud.example.org:443/".into(), ..granted() };
        let account = signed_in_account("https://cloud.example.org", &reported).expect("same origin");
        assert_eq!(account.id, "nextcloud:anna@cloud.example.org");
        let sub = Credentials { server: "https://example.org/nc".into(), ..granted() };
        assert_eq!(
            signed_in_account("https://example.org/nc", &sub).map(|a| a.id),
            Ok("nextcloud:anna@example.org/nc".into())
        );
    }

    #[test]
    fn a_sign_in_that_names_another_server_or_a_strange_login_is_refused() {
        // A server that poses as the user's account elsewhere, or names a path that would make
        // its id look like the start of another one.
        for server in [
            "https://other.example.org",
            "http://cloud.example.org",
            "https://cloud.example.org:8443",
            "https://cloud.example.org.evil.example/https:",
            "",
        ] {
            let credentials = Credentials { server: server.into(), ..granted() };
            assert!(signed_in_account("https://cloud.example.org", &credentials).is_err(), "{server}");
        }
        let posing = Credentials { server: "https://cloud.example.org".into(), ..granted() };
        assert!(signed_in_account("https://evil.example.org", &posing).is_err(), "named after where it started");
        for login_name in ["", "anna@cloud.example.org/https:", "a/b"] {
            let credentials = Credentials { login_name: login_name.into(), ..granted() };
            assert!(signed_in_account("https://cloud.example.org", &credentials).is_err(), "{login_name:?}");
        }
    }

    #[test]
    fn an_account_without_secret_is_loaded_with_a_problem_and_does_not_stop_the_others() {
        let accounts = vec![
            Account::nextcloud("https://cloud.example.org", "anna"),
            Account::nextcloud("https://other.example.org", "bob"),
            Account::nextcloud("https://third.example.org", "carl"),
        ];
        let loaded = with_secrets(accounts, |account| match account.login_name.as_str() {
            "anna" => Ok(Some("secret".into())),
            "bob" => Ok(None),
            _ => Err("locked".into()),
        });
        let ids: Vec<&str> = loaded.iter().map(|stored| stored.account().id.as_str()).collect();
        assert_eq!(
            ids,
            ["nextcloud:anna@cloud.example.org", "nextcloud:bob@other.example.org", "nextcloud:carl@third.example.org"],
            "all of them, in their order"
        );
        assert!(matches!(&loaded[0], Stored::Ready(connected) if connected.app_password == "secret"));
        for stored in &loaded[1..] {
            assert!(
                matches!(stored, Stored::Unusable { problem, .. } if problem.starts_with("Sign in again")),
                "{stored:?}"
            );
        }
    }

    #[test]
    fn the_legacy_owner_is_the_first_nextcloud_account_of_the_given_list() {
        let dir = tempfile::tempdir().expect("temp dir");
        let path = dir.path().join(account::ACCOUNTS_FILE);
        assert_eq!(legacy_owner(&path), None, "no list, no owner");
        let accounts = [
            Account::nextcloud("https://cloud.example.org", "anna"),
            Account::nextcloud("https://other.example.org", "bob"),
        ];
        std::fs::write(&path, serde_json::to_vec(&accounts).expect("json")).expect("written");
        assert_eq!(legacy_owner(&path).as_deref(), Some("nextcloud:anna@cloud.example.org"));
        std::fs::write(&path, "{ damaged").expect("damaged");
        assert_eq!(legacy_owner(&path), None, "a damaged list rewrites nothing");
    }

    #[test]
    fn revoke_request_is_an_authenticated_ocs_delete() {
        let http = reqwest::Client::new();
        let request = revoke_request(&http, &connected()).expect("request");
        assert_eq!(request.method(), reqwest::Method::DELETE);
        assert_eq!(request.url().as_str(), "https://cloud.example.org/ocs/v2.php/core/apppassword");
        assert!(!request.url().as_str().contains("secret"), "the secret never goes into the URL");
        let headers = request.headers();
        assert_eq!(headers.get("OCS-APIRequest").and_then(|v| v.to_str().ok()), Some("true"));
        // "anna:secret" in base64.
        assert_eq!(
            headers.get(reqwest::header::AUTHORIZATION).and_then(|v| v.to_str().ok()),
            Some("Basic YW5uYTpzZWNyZXQ=")
        );
        assert!(request.body().is_none());
    }

    #[test]
    fn revoke_outcome_accepts_only_success() {
        assert_eq!(revoke_outcome(reqwest::StatusCode::OK), Ok(()));
        for status in [401, 403, 404, 500, 503] {
            let status = reqwest::StatusCode::from_u16(status).expect("status");
            assert!(revoke_outcome(status).is_err(), "{status}");
        }
    }

    /// Runs [`remote_wipe`] with scripted answers and returns its result plus the steps taken.
    async fn wipe_script(
        check: Result<bool, ProviderError>,
        wipe: Result<(), String>,
    ) -> (Result<bool, String>, Vec<&'static str>) {
        let steps = RefCell::new(Vec::new());
        let result = remote_wipe(
            || {
                steps.borrow_mut().push("check");
                async { check }
            },
            || {
                steps.borrow_mut().push("wipe");
                async { wipe }
            },
            || {
                steps.borrow_mut().push("report");
                async { Err::<(), _>(ProviderError::Offline) }
            },
        )
        .await;
        (result, steps.into_inner())
    }

    #[tokio::test]
    async fn remote_wipe_wipes_then_confirms() {
        let (result, steps) = wipe_script(Ok(true), Ok(())).await;
        // A failed confirmation does not undo the wipe.
        assert_eq!(result, Ok(true));
        assert_eq!(steps, ["check", "wipe", "report"]);
    }

    #[tokio::test]
    async fn no_remote_wipe_keeps_everything() {
        let (result, steps) = wipe_script(Ok(false), Ok(())).await;
        assert_eq!(result, Ok(false));
        assert_eq!(steps, ["check"]);
    }

    #[tokio::test]
    async fn a_failed_wipe_check_keeps_everything() {
        let (result, steps) = wipe_script(Err(ProviderError::Offline), Ok(())).await;
        assert!(result.is_err());
        assert_eq!(steps, ["check"]);
    }

    #[tokio::test]
    async fn a_failed_local_wipe_is_not_confirmed() {
        let (result, steps) = wipe_script(Ok(true), Err("keychain locked".into())).await;
        assert!(result.is_err());
        assert_eq!(steps, ["check", "wipe"]);
    }

    #[tokio::test]
    async fn succeeds_after_two_pending_polls() {
        let script = RefCell::new(VecDeque::from([Ok(None), Ok(None), Ok(Some(granted()))]));
        let poll = || {
            let next = script.borrow_mut().pop_front().unwrap_or(Ok(None));
            async move { next }
        };
        let credentials = poll_until_granted(poll, Duration::from_millis(1), Duration::from_secs(10)).await;
        assert_eq!(credentials, Ok(granted()));
        assert!(script.borrow().is_empty(), "exactly three polls expected");
    }

    #[tokio::test]
    async fn retries_errors() {
        let script = RefCell::new(VecDeque::from([
            Err(ProviderError::Offline),
            Err(ProviderError::Other("reset".into())),
            Ok(Some(granted())),
        ]));
        let poll = || {
            let next = script.borrow_mut().pop_front().unwrap_or(Ok(None));
            async move { next }
        };
        let credentials = poll_until_granted(poll, Duration::from_millis(1), Duration::from_secs(10)).await;
        assert_eq!(credentials, Ok(granted()));
        assert!(script.borrow().is_empty(), "exactly three polls expected");
    }

    #[tokio::test]
    async fn times_out_when_never_granted() {
        let poll = || async { Ok::<_, ProviderError>(None) };
        let result = poll_until_granted(poll, Duration::from_millis(1), Duration::from_millis(20)).await;
        assert_eq!(result, Err("sign-in timed out".to_string()));
    }

    #[tokio::test]
    async fn errors_count_toward_the_timeout() {
        let poll = || async { Err::<Option<Credentials>, _>(ProviderError::Offline) };
        let result = poll_until_granted(poll, Duration::from_millis(1), Duration::from_millis(20)).await;
        assert_eq!(result, Err("sign-in timed out (last error: server not reachable)".to_string()));
    }
}

#[cfg(test)]
mod keychain_tests {
    use super::*;

    /// Writes, reads and deletes a test secret in the real system keychain from inside a
    /// multi-threaded tokio runtime, like the daemon does. Run by hand:
    /// `cargo test -p torroworkplace keychain -- --ignored`.
    #[test]
    #[ignore = "uses the real system keychain"]
    fn keychain_works_inside_the_daemon_runtime() {
        let runtime = tokio::runtime::Builder::new_multi_thread().enable_all().build().expect("runtime");
        let read = runtime.block_on(async {
            tokio::spawn(async {
                keychain(|| {
                    let account = Account::nextcloud("https://example.invalid", "torro-keychain-test");
                    account.set_secret("secret").map_err(|err| err.to_string())?;
                    let read = account.secret().map_err(|err| err.to_string())?;
                    account.delete_secret().map_err(|err| err.to_string())?;
                    Ok(read)
                })
                .await
            })
            .await
            .expect("task")
        });
        assert_eq!(read, Ok(Some("secret".to_string())));
    }
}
