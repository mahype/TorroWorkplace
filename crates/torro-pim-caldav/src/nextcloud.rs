//! Signing in to Nextcloud with Login Flow v2, and the remote wipe check that belongs to it.
//!
//! The way Nextcloud's own desktop and mobile clients sign in, and the closest thing to
//! OAuth that works on every Nextcloud without an admin registering a client first:
//!
//! 1. `POST /index.php/login/v2` returns a login URL and a poll token.
//! 2. The user opens the login URL in the browser, signs in there (with 2FA, SSO, …)
//!    and grants access. We never see the password.
//! 3. We poll until Nextcloud hands out an app password for this device. The user sees
//!    it under Settings → Security → Devices & sessions and can revoke it there.
//!
//! https://docs.nextcloud.com/server/latest/developer_manual/client_apis/LoginFlow/index.html
//!
//! Remote wipe ([`check_wipe`], [`confirm_wipe`]): the user or an admin can mark this device for
//! wiping under "Devices & sessions". The server then refuses the app password, and a client that
//! got 401/403 is expected to ask whether it should wipe itself and to confirm when it did.
//!
//! https://docs.nextcloud.com/server/latest/developer_manual/client_apis/RemoteWipe/index.html

use reqwest::StatusCode;
use serde::Deserialize;
use torro_pim::ProviderError;

use crate::http_error;

/// A sign-in in progress.
#[derive(Debug, Clone)]
pub struct LoginFlow {
    /// The server the sign-in was started at, as [`normalize_server`] made it. The account is
    /// named after this address, not after what the server reports when it is done (task M1.4).
    pub server: String,
    /// Open this in the browser.
    pub login_url: String,
    poll_endpoint: String,
    poll_token: String,
}

/// What Nextcloud hands out once the user granted access.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Credentials {
    /// Server base URL as Nextcloud reports it.
    pub server: String,
    pub login_name: String,
    pub app_password: String,
}

#[derive(Deserialize)]
struct StartResponse {
    poll: PollInfo,
    login: String,
}

#[derive(Deserialize)]
struct PollInfo {
    token: String,
    endpoint: String,
}

/// The answer to the wipe check: `{"wipe": true}`.
#[derive(Deserialize)]
struct WipeCheckResponse {
    #[serde(default)]
    wipe: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PollResponse {
    server: String,
    login_name: String,
    app_password: String,
}

/// Turns what a user types (`cloud.example.org`, `https://cloud.example.org/apps/files/`)
/// into a base URL.
pub fn normalize_server(input: &str) -> Result<String, ProviderError> {
    let input = input.trim();
    if input.is_empty() {
        return Err(ProviderError::Other("enter the address of your Nextcloud".into()));
    }
    let with_scheme = if input.contains("://") { input.to_string() } else { format!("https://{input}") };
    let mut url = reqwest::Url::parse(&with_scheme)
        .map_err(|_| ProviderError::Other(format!("{input:?} is not a web address")))?;
    // Nextcloud may live in a subdirectory; cut off everything from the first known app path.
    let path = url.path().to_string();
    let base = ["/index.php", "/apps/", "/remote.php", "/login"]
        .iter()
        .filter_map(|marker| path.find(marker))
        .min()
        .map_or(path.as_str(), |cut| &path[..cut]);
    url.set_path(base.trim_end_matches('/'));
    url.set_query(None);
    url.set_fragment(None);
    Ok(url.as_str().trim_end_matches('/').to_string())
}

/// Step 1: asks the server for a login URL.
pub async fn start_login(http: &reqwest::Client, server: &str) -> Result<LoginFlow, ProviderError> {
    let response = http.post(format!("{server}/index.php/login/v2")).send().await.map_err(http_error)?;
    if !response.status().is_success() {
        return Err(ProviderError::Other(format!(
            "{server} does not look like a Nextcloud (HTTP {})",
            response.status().as_u16()
        )));
    }
    let start: StartResponse =
        response.json().await.map_err(|_| ProviderError::Other(format!("{server} does not look like a Nextcloud")))?;
    Ok(LoginFlow {
        server: server.to_string(),
        login_url: start.login,
        poll_endpoint: start.poll.endpoint,
        poll_token: start.poll.token,
    })
}

impl LoginFlow {
    /// Step 3: `Ok(None)` while the user has not granted access yet. The token is valid
    /// for 20 minutes.
    pub async fn poll(&self, http: &reqwest::Client) -> Result<Option<Credentials>, ProviderError> {
        let response = http
            .post(&self.poll_endpoint)
            .form(&[("token", self.poll_token.as_str())])
            .send()
            .await
            .map_err(http_error)?;
        match response.status() {
            StatusCode::NOT_FOUND => Ok(None),
            status if status.is_success() => {
                let done: PollResponse = response.json().await.map_err(|err| ProviderError::Other(err.to_string()))?;
                Ok(Some(Credentials {
                    server: done.server.trim_end_matches('/').to_string(),
                    login_name: done.login_name,
                    app_password: done.app_password,
                }))
            }
            status => Err(ProviderError::Other(format!("sign-in failed (HTTP {})", status.as_u16()))),
        }
    }
}

/// Asks the server whether this device was marked for wiping. `Ok(true)` means: remove the
/// account, its app password and all cached data, then call [`confirm_wipe`].
///
/// `POST <server>/index.php/core/wipe/check` with the form field `token=<app password>`
/// (`core/Controller/WipeController.php::checkWipe` in nextcloud/server): 200 `{"wipe": true}` =
/// wipe; 404 = nothing to wipe or the token is unknown (revoked, not a Login Flow token). The route
/// is public and rate limited (10 calls in 5 minutes per client), so callers ask once per refusal,
/// never in a loop. A 429, 503 (maintenance) or an unreadable answer is an error: then nothing is
/// known and nothing must be deleted.
pub async fn check_wipe(http: &reqwest::Client, server: &str, app_password: &str) -> Result<bool, ProviderError> {
    let request = wipe_request(http, server, WIPE_CHECK_PATH, app_password)?;
    let response = http.execute(request).await.map_err(http_error)?;
    let status = response.status();
    let answer =
        if status.is_success() { response.json::<WipeCheckResponse>().await.ok().map(|a| a.wipe) } else { None };
    check_wipe_outcome(status, answer)
}

/// Tells the server that this device wiped itself; the server then deletes the app password and
/// tells the user. Same token and route family as [`check_wipe`]
/// (`WipeController::wipeDone`): 200 = done, 404 = no wipe pending for this token.
pub async fn confirm_wipe(http: &reqwest::Client, server: &str, app_password: &str) -> Result<(), ProviderError> {
    let request = wipe_request(http, server, WIPE_SUCCESS_PATH, app_password)?;
    let response = http.execute(request).await.map_err(http_error)?;
    confirm_wipe_outcome(response.status())
}

const WIPE_CHECK_PATH: &str = "/index.php/core/wipe/check";
const WIPE_SUCCESS_PATH: &str = "/index.php/core/wipe/success";

/// The form POST both wipe routes expect. Kept apart so it can be checked without a network: the
/// app password travels in the form body only, never in the URL, and no `Authorization` header is
/// sent (the server already refuses the password as a login).
fn wipe_request(
    http: &reqwest::Client,
    server: &str,
    path: &str,
    app_password: &str,
) -> Result<reqwest::Request, ProviderError> {
    http.post(format!("{}{path}", server.trim_end_matches('/')))
        .form(&[("token", app_password)])
        .build()
        .map_err(|err| ProviderError::Other(format!("cannot build the wipe request: {err}")))
}

/// Maps the wipe check's status and decoded body (`None` when unreadable or not a success).
fn check_wipe_outcome(status: StatusCode, wipe: Option<bool>) -> Result<bool, ProviderError> {
    match (status, wipe) {
        (StatusCode::NOT_FOUND, _) => Ok(false),
        (status, Some(wipe)) if status.is_success() => Ok(wipe),
        (status, None) if status.is_success() => {
            Err(ProviderError::Other("the server's answer to the wipe check is unreadable".into()))
        }
        (status, _) => Err(wipe_status_error("wipe check", status)),
    }
}

fn confirm_wipe_outcome(status: StatusCode) -> Result<(), ProviderError> {
    match status {
        status if status.is_success() => Ok(()),
        StatusCode::NOT_FOUND => Err(ProviderError::Other("the server has no wipe pending for this device".into())),
        status => Err(wipe_status_error("wipe confirmation", status)),
    }
}

fn wipe_status_error(what: &str, status: StatusCode) -> ProviderError {
    ProviderError::Other(match status {
        StatusCode::TOO_MANY_REQUESTS => format!("{what}: the server asks to slow down (HTTP 429)"),
        StatusCode::SERVICE_UNAVAILABLE => format!("{what}: the server is in maintenance mode"),
        status => format!("{what} failed (HTTP {})", status.as_u16()),
    })
}

/// Whether `a` and `b` are URLs with the same origin (scheme, host, port). A finished sign-in
/// must name the server it was started at: a server that reports another one could otherwise
/// pose as an account the user has elsewhere (task M1.4). Unparsable addresses never match.
pub fn same_origin(a: &str, b: &str) -> bool {
    match (reqwest::Url::parse(a), reqwest::Url::parse(b)) {
        (Ok(a), Ok(b)) => a.origin().is_tuple() && a.origin() == b.origin(),
        _ => false,
    }
}

/// CalDAV root of a Nextcloud.
pub fn dav_url(server: &str) -> String {
    format!("{}/remote.php/dav/", server.trim_end_matches('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_what_users_type() {
        let ok = |s: &str| normalize_server(s).ok();
        assert_eq!(ok("cloud.example.org"), Some("https://cloud.example.org".into()));
        assert_eq!(ok(" https://cloud.example.org/ "), Some("https://cloud.example.org".into()));
        assert_eq!(ok("https://example.org/nc/apps/files/?dir=/"), Some("https://example.org/nc".into()));
        assert_eq!(ok("http://localhost:8080/index.php/login"), Some("http://localhost:8080".into()));
        assert!(normalize_server("").is_err());
    }

    #[test]
    fn only_the_same_scheme_host_and_port_is_the_same_server() {
        assert!(same_origin("https://cloud.example.org", "https://cloud.example.org/"));
        assert!(same_origin("https://example.org/nc", "https://example.org:443/nc"));
        assert!(same_origin("https://cloud.example.org", "https://cloud.example.org/https:"), "path is not origin");
        for other in [
            "https://other.example.org",
            "http://cloud.example.org",
            "https://cloud.example.org:8443",
            "https://cloud.example.org.evil.example",
            "cloud.example.org",
            "",
        ] {
            assert!(!same_origin("https://cloud.example.org", other), "{other}");
        }
    }

    #[test]
    fn wipe_requests_post_the_token_as_a_form() {
        let http = reqwest::Client::new();
        for (path, url) in [
            (WIPE_CHECK_PATH, "https://cloud.example.org/nc/index.php/core/wipe/check"),
            (WIPE_SUCCESS_PATH, "https://cloud.example.org/nc/index.php/core/wipe/success"),
        ] {
            let request = wipe_request(&http, "https://cloud.example.org/nc/", path, "app-secret").expect("request");
            assert_eq!(request.method(), reqwest::Method::POST);
            assert_eq!(request.url().as_str(), url);
            assert!(!request.url().as_str().contains("app-secret"), "the secret never goes into the URL");
            assert!(request.headers().get(reqwest::header::AUTHORIZATION).is_none());
            assert_eq!(
                request.headers().get(reqwest::header::CONTENT_TYPE).and_then(|v| v.to_str().ok()),
                Some("application/x-www-form-urlencoded")
            );
            assert_eq!(request.body().and_then(|b| b.as_bytes()), Some(&b"token=app-secret"[..]));
        }
    }

    #[test]
    fn wipe_check_wipes_only_when_the_server_says_so() {
        assert_eq!(check_wipe_outcome(StatusCode::OK, Some(true)), Ok(true));
        assert_eq!(check_wipe_outcome(StatusCode::OK, Some(false)), Ok(false));
        assert_eq!(check_wipe_outcome(StatusCode::NOT_FOUND, None), Ok(false));
        assert!(check_wipe_outcome(StatusCode::OK, None).is_err(), "unreadable answer");
        for status in [401, 403, 429, 500, 503] {
            let status = StatusCode::from_u16(status).expect("status");
            assert!(check_wipe_outcome(status, None).is_err(), "{status}");
            assert!(check_wipe_outcome(status, Some(true)).is_err(), "{status}");
        }
    }

    #[test]
    fn wipe_confirmation_accepts_only_success() {
        assert_eq!(confirm_wipe_outcome(StatusCode::OK), Ok(()));
        for status in [404, 429, 500, 503] {
            let status = StatusCode::from_u16(status).expect("status");
            assert!(confirm_wipe_outcome(status).is_err(), "{status}");
        }
    }
}
