//! Signing in to Nextcloud with Login Flow v2.
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

use reqwest::StatusCode;
use serde::Deserialize;
use torro_pim::ProviderError;

use crate::http_error;

/// A sign-in in progress.
#[derive(Debug, Clone)]
pub struct LoginFlow {
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
    Ok(LoginFlow { login_url: start.login, poll_endpoint: start.poll.endpoint, poll_token: start.poll.token })
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
}
