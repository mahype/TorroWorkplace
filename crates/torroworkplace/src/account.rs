//! The core's Nextcloud accounts: the stored accounts and their app passwords, signing in with
//! Login Flow v2, revoking an app password on the server when its account is removed, and the
//! remote wipe the server may ask for once it refuses the app password ([`remote_wipe`], task S.12).
//!
//! Task K.6 in `docs/tasks/kalender-aufgaben.md`; several accounts since task M1.1: signing in
//! adds an account, removing one leaves the others. Building blocks that exist:
//! `torro_core::account` (accounts.json + keychain), `torro_pim_caldav::nextcloud`
//! (`normalize_server`, `start_login`, `LoginFlow::poll`, `dav_url`), `torro_pim_caldav::
//! http_client`, `torro_pim_caldav::CalDavClient`. `pim.rs` (task K.5) uses this module.

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

/// Every stored Nextcloud account with its secret from the keychain, in the order of
/// `accounts.json` (task M1.1). An account whose secret is gone is left out with a log line: it
/// cannot sign in, and signing in to it again brings it back.
pub fn stored() -> Result<Vec<Connected>, String> {
    let mut connected = Vec::new();
    for account in account::load().map_err(|err| err.to_string())? {
        if account.kind != AccountKind::Nextcloud {
            continue;
        }
        match account.secret().map_err(|err| err.to_string())? {
            Some(app_password) => connected.push(Connected { account, app_password }),
            None => eprintln!("torroworkplace: {} has no secret in the keychain; sign in again", account.id),
        }
    }
    Ok(connected)
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
pub async fn finish(http: &reqwest::Client, flow: &LoginFlow) -> Result<Connected, String> {
    let credentials = poll_until_granted(|| flow.poll(http), POLL_INTERVAL, SIGN_IN_TIMEOUT).await?;
    keychain(move || {
        let account = Account::nextcloud(&credentials.server, &credentials.login_name);
        account.set_secret(&credentials.app_password).map_err(|err| err.to_string())?;
        let accounts = with_account(account::load().map_err(|err| err.to_string())?, account.clone());
        account::save(&accounts).map_err(|err| err.to_string())?;
        Ok(Connected { account, app_password: credentials.app_password })
    })
    .await
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
