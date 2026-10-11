//! The core's Nextcloud account: the stored account and its app password, signing in with
//! Login Flow v2 and revoking the app password on the server when the account is removed.
//!
//! Task K.6 in `docs/tasks/kalender-aufgaben.md`. Building blocks that exist:
//! `torro_core::account` (accounts.json + keychain), `torro_pim_caldav::nextcloud`
//! (`normalize_server`, `start_login`, `LoginFlow::poll`, `dav_url`), `torro_pim_caldav::
//! http_client`, `torro_pim_caldav::CalDavClient`. `pim.rs` (task K.5) uses this module.

use std::future::Future;
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

/// The stored Nextcloud account with its secret from the keychain; `None` when there is
/// none (or its secret is gone).
pub fn stored() -> Result<Option<Connected>, String> {
    let Some(account) = account::load()
        .map_err(|err| err.to_string())?
        .into_iter()
        .find(|account| account.kind == AccountKind::Nextcloud)
    else {
        return Ok(None);
    };
    let Some(app_password) = account.secret().map_err(|err| err.to_string())? else {
        return Ok(None);
    };
    Ok(Some(Connected { account, app_password }))
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
/// account (replacing any other Nextcloud account) and its app password.
pub async fn finish(http: &reqwest::Client, flow: &LoginFlow) -> Result<Connected, String> {
    let credentials = poll_until_granted(|| flow.poll(http), POLL_INTERVAL, SIGN_IN_TIMEOUT).await?;
    keychain(move || {
        let account = Account::nextcloud(&credentials.server, &credentials.login_name);
        account.set_secret(&credentials.app_password).map_err(|err| err.to_string())?;
        let mut accounts = take_other_accounts(Some(&account.id))?;
        accounts.push(account.clone());
        account::save(&accounts).map_err(|err| err.to_string())?;
        Ok(Connected { account, app_password: credentials.app_password })
    })
    .await
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

/// Removes the stored account and its secret.
pub fn forget() -> Result<(), String> {
    let accounts = take_other_accounts(None)?;
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

/// A CalDAV client for the account (`<server>/remote.php/dav/`).
pub fn caldav(http: &reqwest::Client, connected: &Connected) -> Result<CalDavClient, String> {
    let dav = CalDavAccount {
        url: nextcloud::dav_url(&connected.account.server),
        username: connected.account.login_name.clone(),
    };
    CalDavClient::new(http.clone(), &dav, connected.app_password.clone()).map_err(|err| err.to_string())
}

/// Deletes the secrets of all Nextcloud accounts except the one with id `keep` (its secret
/// is about to be overwritten) and returns the accounts that stay in the list.
fn take_other_accounts(keep: Option<&str>) -> Result<Vec<Account>, String> {
    let mut kept = Vec::new();
    for account in account::load().map_err(|err| err.to_string())? {
        if account.kind != AccountKind::Nextcloud {
            kept.push(account);
        } else if Some(account.id.as_str()) != keep {
            account.delete_secret().map_err(|err| err.to_string())?;
        }
    }
    Ok(kept)
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
