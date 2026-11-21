//! Signing in to an AI account with a browser instead of an API key (issue #324, `docs/ki.md`, "Anmelden mit
//! ChatGPT"): OAuth 2.0 Authorization Code with PKCE (S256) and a callback on this computer's loopback address.
//!
//! The protocol of a sign-in, pure and without a network, for every provider that offers one. A provider names its
//! way in with a [`SignIn`] ([`crate::ai::AiProvider::sign_in`]); the core (`torroworkplace::account::sign_in`)
//! listens for the callback, sends the forms built here and keeps the [`TokenSet`] as the account's one secret in
//! the system keychain. A further provider (xAI, …) is one more [`SignIn`], never a change of the types here.
//!
//! The first provider is ChatGPT ([`CHATGPT`], "Sign in with ChatGPT", checked against
//! <https://developers.openai.com/siwc/token-sharing-open-source> on 2026-11-21). OpenAI would let an open-source
//! app register itself at its first sign-in ([`Registration::Dynamic`]); the suite does not (decision on #324): it
//! offers ChatGPT only with a client id of its own from OpenAI, given at build or run time
//! ([`SignIn::client_env`], [`Registration::Configured`]). Without one the sign-in is not offered
//! ([`SignIn::offered`]) and the core refuses it. No other app's client id is ever used.
//!
//! Tokens are secrets: [`TokenSet`] never shows one in `Debug`, and no error here repeats one.

use std::fmt;

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{AiError, AiModel, openai};
use crate::account::Secret;
use crate::i18n::{tr, trf};

/// How a provider hands out the client id a sign-in needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Registration {
    /// The app registers itself at the first sign-in of an account: it sends `entry` as the client id with
    /// `agent_name_hint` (the app's name) and `ext_agent_host_id` (this computer), and the callback brings the
    /// issued client id, which the core keeps and sends from then on.
    Dynamic {
        /// The client id of the first, registering sign-in: `dynamic_agent_client`.
        entry: &'static str,
        /// The app's name the person sees when they allow it: always the same.
        name_hint: &'static str,
    },
    /// Only a client id given at build or run time ([`SignIn::client_env`]); without one, no sign-in.
    Configured,
}

/// One provider's way in with a browser: where to send the person, where tokens come from, what to ask for.
/// A const per provider; adding one is adding a const and its arm in [`crate::ai::AiProvider::sign_in`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SignIn {
    /// The issuer, as the ID token names it.
    pub issuer: &'static str,
    /// The authorization endpoint the browser opens.
    pub authorize_url: &'static str,
    /// The token endpoint: code and refresh token are exchanged there.
    pub token_url: &'static str,
    /// The revocation endpoint (RFC 7009), if the provider has one.
    pub revoke_url: Option<&'static str>,
    /// The scopes asked for, in this order.
    pub scopes: &'static [&'static str],
    /// The scope the account cannot be used without; a grant without it is refused.
    pub required_scope: Option<&'static str>,
    /// `resource` (RFC 8707) of every request, if the provider wants one.
    pub resource: Option<&'static str>,
    /// The path of the callback on `http://127.0.0.1:<port>`.
    pub callback_path: &'static str,
    /// The port tried first for the callback; any free port when it is taken.
    pub port: u16,
    /// How the client id comes about.
    pub registration: Registration,
    /// The environment variable that gives a client id at run time (and, read by `option_env!`, at build time:
    /// [`SignIn::built_client_id`]).
    pub client_env: &'static str,
    /// The client id given when the suite was built, if any.
    pub built_client_id: Option<&'static str>,
    /// Where the person sets how much of their plan the suite may use, for "Manage usage".
    pub usage_url: Option<&'static str>,
    /// The provider wants this computer's id (`ext_agent_host_id`) with every authorization request.
    pub sends_host_id: bool,
}

/// "Sign in with ChatGPT" (issue #324): endpoints from `https://auth.openai.com/.well-known/openid-configuration`,
/// scopes and `resource` from the guide for open-source apps (both checked on 2026-11-21). Tokens are used at
/// `https://api.openai.com/v1` ([`crate::ai::AiProvider::ChatGpt`]). Only with a client id of the suite's own from
/// OpenAI (`TORRO_CHATGPT_CLIENT_ID`, decision on #324); without one ChatGPT is not offered.
pub const CHATGPT: SignIn = SignIn {
    issuer: "https://auth.openai.com",
    authorize_url: "https://auth.openai.com/api/accounts/authorize",
    token_url: "https://auth.openai.com/api/accounts/oauth/token",
    revoke_url: Some("https://auth.openai.com/api/accounts/oauth/revoke"),
    scopes: &["openid", "profile", "email", "offline_access", "resource.invoke", "chatgpt.tokens.use.direct"],
    required_scope: Some("chatgpt.tokens.use.direct"),
    resource: Some("https://api.openai.com/v1"),
    callback_path: "/auth/callback",
    port: 1455,
    registration: Registration::Configured,
    client_env: "TORRO_CHATGPT_CLIENT_ID",
    built_client_id: option_env!("TORRO_CHATGPT_CLIENT_ID"),
    usage_url: Some("https://chatgpt.com/settings/usage"),
    sends_host_id: true,
};

/// Which client id a sign-in sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Client {
    /// A known client id: given at build or run time, or issued at an earlier sign-in.
    Known(String),
    /// The first sign-in registers the app ([`Registration::Dynamic`]); the callback brings the client id.
    Register,
}

impl SignIn {
    /// The client id given at run time (`client_env`), else at build time; blank counts as none.
    pub fn configured_client_id(&self) -> Option<String> {
        std::env::var(self.client_env)
            .ok()
            .or_else(|| self.built_client_id.map(str::to_string))
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty())
    }

    /// The sign-in can be offered: a client id is configured, or the provider lets the app register itself. The
    /// control centre lists only these; the core refuses the others ([`SignIn::client`]).
    pub fn offered(&self) -> bool {
        self.configured_client_id().is_some() || matches!(self.registration, Registration::Dynamic { .. })
    }

    /// The client id of the next sign-in: the configured one first. Where the provider lets the app register
    /// itself, else `issued` (kept from an earlier sign-in), else a registration. [`AiError::Invalid`] with a
    /// sentence for people when a provider that needs a configured client id has none, even with an id issued
    /// before: only a configured one counts there.
    pub fn client(&self, issued: Option<&str>) -> Result<Client, AiError> {
        if let Some(id) = self.configured_client_id() {
            return Ok(Client::Known(id));
        }
        match self.registration {
            Registration::Configured => Err(not_configured()),
            Registration::Dynamic { .. } => Ok(match issued.map(str::trim).filter(|id| !id.is_empty()) {
                Some(id) => Client::Known(id.to_string()),
                None => Client::Register,
            }),
        }
    }

    /// The callback address on this computer at `port`: `http://127.0.0.1:1455/auth/callback`. Always the
    /// address, never `localhost`: the provider compares it as written.
    pub fn redirect_uri(&self, port: u16) -> String {
        format!("http://127.0.0.1:{port}{}", self.callback_path)
    }
}

/// The sentence when a provider needs a client id the suite was not built with.
pub fn not_configured() -> AiError {
    AiError::Invalid(
        tr("Signing in is not available in this version of Torro Workplace yet: it has no client ID from the provider.")
            .into(),
    )
}

/// One sign-in on its way: what the callback and the code exchange are checked against. Lives in the core only
/// and never travels; `Debug` hides the verifier.
#[derive(Clone, PartialEq, Eq)]
pub struct Attempt {
    /// Random, sent with the request and expected back unchanged in the callback.
    pub state: String,
    /// Random, expected back in the ID token.
    pub nonce: String,
    /// PKCE's secret half; only its [`challenge`] travels with the authorization request.
    pub verifier: Secret,
    /// The callback address, sent with the authorization request and again with the code.
    pub redirect_uri: String,
    /// Which client id this attempt sends.
    pub client: Client,
    /// This computer's id towards the provider (`ext_agent_host_id`), stable across sign-ins.
    pub host_id: String,
}

impl fmt::Debug for Attempt {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Attempt")
            .field("redirect_uri", &self.redirect_uri)
            .field("client", &self.client)
            .finish_non_exhaustive()
    }
}

/// `bytes` as base64url without padding: how `state`, `nonce` and the verifier are written from random bytes.
pub fn url_safe(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// PKCE's S256 challenge of `verifier`: base64url of its SHA-256, without padding (RFC 7636).
pub fn challenge(verifier: &str) -> String {
    url_safe(&Sha256::digest(verifier.as_bytes()))
}

/// The address the browser opens for `attempt`. A registering attempt adds the app's name; every attempt names
/// this computer (`ext_agent_host_id`), where the provider asks for it.
pub fn authorize_url(sign_in: &SignIn, attempt: &Attempt) -> Result<String, AiError> {
    let mut url = url::Url::parse(sign_in.authorize_url)
        .map_err(|_| AiError::Format(tr("The sign-in address is not valid.").into()))?;
    {
        let mut query = url.query_pairs_mut();
        match (&attempt.client, sign_in.registration) {
            (Client::Register, Registration::Dynamic { entry, name_hint }) => {
                query.append_pair("client_id", entry).append_pair("agent_name_hint", name_hint);
            }
            (Client::Known(id), _) => {
                query.append_pair("client_id", id);
            }
            (Client::Register, Registration::Configured) => return Err(not_configured()),
        }
        if sign_in.sends_host_id || matches!(sign_in.registration, Registration::Dynamic { .. }) {
            query.append_pair("ext_agent_host_id", &attempt.host_id);
        }
        query
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", &attempt.redirect_uri)
            .append_pair("scope", &sign_in.scopes.join(" "))
            .append_pair("state", &attempt.state)
            .append_pair("nonce", &attempt.nonce)
            .append_pair("code_challenge", &challenge(attempt.verifier.expose()))
            .append_pair("code_challenge_method", "S256");
        if let Some(resource) = sign_in.resource {
            query.append_pair("resource", resource);
        }
    }
    Ok(url.into())
}

/// What a callback brought: the code and the client id it is exchanged with.
#[derive(Clone, PartialEq, Eq)]
pub struct Callback {
    pub code: Secret,
    pub client_id: String,
}

impl fmt::Debug for Callback {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Callback").field("client_id", &self.client_id).finish_non_exhaustive()
    }
}

/// Why a request to the callback address is no answer to the attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallbackError {
    /// Another path or no query: something else on this computer asked (a favicon); keep listening.
    Unrelated,
    /// The answer belongs to another attempt (`state` differs): ignored, keep listening.
    Foreign,
    /// The attempt is over with this error for people.
    Failed(AiError),
}

/// Reads the request target `target` (`/auth/callback?code=…&state=…`) of a request to the callback address.
/// Checks the path and `state` first, so a page that guesses the address ends nothing; then the provider's error
/// (`access_denied`: the person declined), the code and the client id: a registration must bring one, any other
/// attempt must bring none or its own.
pub fn read_callback(sign_in: &SignIn, attempt: &Attempt, target: &str) -> Result<Callback, CallbackError> {
    let url = url::Url::parse(&format!("http://127.0.0.1{target}")).map_err(|_| CallbackError::Unrelated)?;
    if url.path() != sign_in.callback_path {
        return Err(CallbackError::Unrelated);
    }
    let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
    let value = |name: &str| pairs.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str());
    if value("state") != Some(attempt.state.as_str()) {
        return Err(if pairs.is_empty() { CallbackError::Unrelated } else { CallbackError::Foreign });
    }
    if let Some(error) = value("error") {
        return Err(CallbackError::Failed(match error {
            "access_denied" => AiError::Invalid(tr("The sign-in was declined in the browser.").into()),
            _ => AiError::Invalid(tr("The provider ended the sign-in. Try again.").into()),
        }));
    }
    let Some(code) = value("code").filter(|code| !code.is_empty()) else {
        return Err(CallbackError::Failed(AiError::Format(tr("The sign-in came back without a code.").into())));
    };
    let returned = value("client_id").filter(|id| !id.is_empty());
    let client_id = match (&attempt.client, returned) {
        (Client::Register, Some(id)) => id.to_string(),
        (Client::Register, None) => {
            return Err(CallbackError::Failed(AiError::Format(
                tr("The provider did not finish registering Torro Workplace. Try again.").into(),
            )));
        }
        (Client::Known(known), Some(id)) if id != known => {
            return Err(CallbackError::Failed(AiError::Invalid(
                tr("The sign-in came back for another registration. Try again.").into(),
            )));
        }
        (Client::Known(known), _) => known.clone(),
    };
    Ok(Callback { code: Secret::new(code), client_id })
}

/// The form that exchanges `callback`'s code for tokens (`grant_type=authorization_code`). No client secret: the
/// app is a public client, PKCE's verifier proves it started the attempt.
pub fn code_form(sign_in: &SignIn, attempt: &Attempt, callback: &Callback) -> Vec<(&'static str, String)> {
    let mut form = vec![
        ("grant_type", "authorization_code".to_string()),
        ("client_id", callback.client_id.clone()),
        ("code", callback.code.expose().to_string()),
        ("code_verifier", attempt.verifier.expose().to_string()),
        ("redirect_uri", attempt.redirect_uri.clone()),
    ];
    if let Some(resource) = sign_in.resource {
        form.push(("resource", resource.to_string()));
    }
    form
}

/// The form that renews `tokens` (`grant_type=refresh_token`), without `scope`, so the grant stays as it is.
pub fn refresh_form(sign_in: &SignIn, tokens: &TokenSet) -> Vec<(&'static str, String)> {
    let mut form = vec![
        ("grant_type", "refresh_token".to_string()),
        ("client_id", tokens.client_id.clone()),
        ("refresh_token", tokens.refresh_token.expose().to_string()),
    ];
    if let Some(resource) = sign_in.resource {
        form.push(("resource", resource.to_string()));
    }
    form
}

/// The form that revokes `tokens`' refresh token (RFC 7009) when the account is removed.
pub fn revoke_form(tokens: &TokenSet) -> Vec<(&'static str, String)> {
    vec![
        ("token", tokens.refresh_token.expose().to_string()),
        ("token_type_hint", "refresh_token".to_string()),
        ("client_id", tokens.client_id.clone()),
    ]
}

/// The tokens of a signed-in account: the account's one secret in the keychain, written as JSON
/// ([`TokenSet::to_secret`]). The client id and this computer's id go with them, so a refresh and the next
/// sign-in send the same. `Debug` shows no token.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenSet {
    /// The client id the tokens were issued to (`oaiapp_…` for ChatGPT).
    pub client_id: String,
    /// This computer's id towards the provider (`ext_agent_host_id`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub host_id: String,
    pub access_token: Secret,
    pub refresh_token: Secret,
    /// Kept to send as `id_token_hint` at the next sign-in of the same account.
    #[serde(default, skip_serializing_if = "Secret::is_blank")]
    pub id_token: Secret,
    /// When the access token ends, in seconds since 1970 (UTC).
    pub expires_at: u64,
    /// The scopes granted, as the token answer lists them.
    #[serde(default)]
    pub scopes: Vec<String>,
    /// Who signed in (`sub` of the ID token): a new sign-in must be the same person to replace the tokens.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub subject: String,
    /// The address the person signed in with, if the ID token names it: shown as the account's login.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub email: String,
}

impl fmt::Debug for TokenSet {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TokenSet")
            .field("client_id", &self.client_id)
            .field("expires_at", &self.expires_at)
            .field("scopes", &self.scopes)
            .finish_non_exhaustive()
    }
}

/// How long before its end an access token is renewed: a request started just before must not run out.
pub const REFRESH_MARGIN_SECS: u64 = 120;

impl TokenSet {
    /// The set as the keychain keeps it: one JSON text.
    pub fn to_secret(&self) -> Result<Secret, AiError> {
        serde_json::to_string(self)
            .map(Secret::new)
            .map_err(|_| AiError::Format(tr("The sign-in could not be stored.").into()))
    }

    /// The set from the keychain; `None` when the text is no token set (an API key, a damaged entry).
    pub fn from_secret(secret: &str) -> Option<TokenSet> {
        serde_json::from_str(secret).ok()
    }

    /// The access token ends within [`REFRESH_MARGIN_SECS`] of `now` (seconds since 1970).
    pub fn needs_refresh(&self, now: u64) -> bool {
        self.expires_at <= now.saturating_add(REFRESH_MARGIN_SECS)
    }
}

/// Reads a token answer of `sign_in` for `client_id`, received at `now` (seconds since 1970). `previous` is the set
/// it renews or replaces: a refresh without a new refresh token keeps the old one, a new sign-in must be the same
/// person (`sub`). `nonce` is the attempt's, checked in the ID token of a sign-in (a refresh has none).
///
/// The ID token came straight from the token endpoint over TLS, so issuer, audience, nonce and expiry are checked,
/// not its signature (OpenID Connect Core 3.1.3.7, point 6). A grant without [`SignIn::required_scope`] is refused:
/// the person did not allow the plan to be used.
pub fn read_tokens(
    sign_in: &SignIn,
    bytes: &[u8],
    now: u64,
    client_id: &str,
    previous: Option<&TokenSet>,
    nonce: Option<&str>,
) -> Result<TokenSet, AiError> {
    let unreadable = || AiError::Format(tr("The provider's sign-in answer could not be read.").into());
    let answer: Value = serde_json::from_slice(bytes).map_err(|_| unreadable())?;
    let text = |name: &str| answer.get(name).and_then(Value::as_str).map(str::trim).filter(|text| !text.is_empty());
    let access = text("access_token").ok_or_else(unreadable)?;
    if let Some(kind) = text("token_type")
        && !kind.eq_ignore_ascii_case("bearer")
    {
        return Err(unreadable());
    }
    let refresh = text("refresh_token")
        .map(Secret::new)
        .or_else(|| previous.map(|previous| previous.refresh_token.clone()))
        .ok_or_else(|| AiError::Format(tr("The provider gave no lasting sign-in. Try again.").into()))?;
    let expires_in = answer.get("expires_in").and_then(Value::as_u64).unwrap_or(3600);
    let scopes: Vec<String> = match text("scope") {
        Some(scope) => scope.split_whitespace().map(str::to_string).collect(),
        None => previous.map(|previous| previous.scopes.clone()).unwrap_or_default(),
    };
    if let Some(required) = sign_in.required_scope
        && !scopes.iter().any(|scope| scope == required)
    {
        return Err(AiError::Invalid(
            tr("The sign-in did not allow Torro Workplace to use your plan. Sign in again and allow it.").into(),
        ));
    }
    let id_token = text("id_token").map(Secret::new);
    let mut tokens = TokenSet {
        client_id: client_id.to_string(),
        host_id: previous.map(|previous| previous.host_id.clone()).unwrap_or_default(),
        access_token: Secret::new(access),
        refresh_token: refresh,
        id_token: previous.map(|previous| previous.id_token.clone()).unwrap_or_default(),
        expires_at: now.saturating_add(expires_in),
        scopes,
        subject: previous.map(|previous| previous.subject.clone()).unwrap_or_default(),
        email: previous.map(|previous| previous.email.clone()).unwrap_or_default(),
    };
    if let Some(id_token) = id_token {
        let claims = claims_of(id_token.expose()).ok_or_else(unreadable)?;
        check_claims(sign_in, &claims, client_id, nonce, now)?;
        let subject = claims.get("sub").and_then(Value::as_str).unwrap_or_default().to_string();
        if let Some(previous) = previous
            && !previous.subject.is_empty()
            && previous.subject != subject
        {
            return Err(AiError::Invalid(
                tr("You signed in as someone else. Remove the account first to change who is signed in.").into(),
            ));
        }
        tokens.subject = subject;
        if let Some(email) = claims.get("email").and_then(Value::as_str) {
            tokens.email = email.to_string();
        }
        tokens.id_token = id_token;
    } else if nonce.is_some() {
        // A sign-in must say who it is; only a refresh may leave the ID token out.
        return Err(unreadable());
    }
    Ok(tokens)
}

/// The claims of a JWT (its middle part), unverified.
fn claims_of(jwt: &str) -> Option<serde_json::Map<String, Value>> {
    let payload = jwt.split('.').nth(1)?;
    let bytes = URL_SAFE_NO_PAD.decode(payload.trim_end_matches('=')).ok()?;
    match serde_json::from_slice(&bytes).ok()? {
        Value::Object(claims) => Some(claims),
        _ => None,
    }
}

fn check_claims(
    sign_in: &SignIn,
    claims: &serde_json::Map<String, Value>,
    client_id: &str,
    nonce: Option<&str>,
    now: u64,
) -> Result<(), AiError> {
    let refused = || AiError::Invalid(tr("The sign-in answer does not belong to this sign-in. Try again.").into());
    if claims.get("iss").and_then(Value::as_str).map(|iss| iss.trim_end_matches('/'))
        != Some(sign_in.issuer.trim_end_matches('/'))
    {
        return Err(refused());
    }
    let audience = match claims.get("aud") {
        Some(Value::String(aud)) => aud == client_id,
        Some(Value::Array(auds)) => auds.iter().any(|aud| aud.as_str() == Some(client_id)),
        _ => false,
    };
    if !audience {
        return Err(refused());
    }
    if let Some(nonce) = nonce
        && claims.get("nonce").and_then(Value::as_str) != Some(nonce)
    {
        return Err(refused());
    }
    // A minute of leeway for a clock that is a little off.
    if claims.get("exp").and_then(Value::as_u64).is_some_and(|exp| exp.saturating_add(60) < now) {
        return Err(refused());
    }
    if claims.get("sub").and_then(Value::as_str).is_none_or(str::is_empty) {
        return Err(refused());
    }
    Ok(())
}

/// What a failed answer of the token endpoint means. A refresh token that no longer works (expired, revoked,
/// reused) is [`AiError::Auth`]: the person signs in again. A client id the provider does not know is a sentence of
/// its own. The provider's words never appear: they could repeat a token.
pub fn token_error(status: u16, body: &[u8]) -> AiError {
    let code = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|json| {
            json.get("error").and_then(|error| error.as_str().map(str::to_string).or_else(|| text_at(error, "code")))
        })
        .unwrap_or_default();
    match code.as_str() {
        "invalid_grant"
        | "invalid_refresh_token"
        | "token_expired"
        | "refresh_token_expired"
        | "refresh_token_invalidated"
        | "refresh_token_reused" => AiError::Auth,
        "invalid_client" | "unauthorized_client" => AiError::Invalid(
            tr("The provider does not accept the client ID of Torro Workplace. Remove the account and sign in again.")
                .into(),
        ),
        _ => match status {
            400 | 401 | 403 => AiError::Auth,
            429 => AiError::Busy(tr("The provider limits sign-ins right now. Try again later.").into()),
            _ => AiError::Server { status, message: String::new() },
        },
    }
}

fn text_at(value: &Value, name: &str) -> Option<String> {
    value.get(name).and_then(Value::as_str).map(str::to_string)
}

/// What a failed answer of the API means for an account signed in with ChatGPT (issue #324): the codes of plan use
/// (`subscription_sharing_…`) get sentences of their own, everything else is read as every OpenAI answer is
/// ([`openai::error_of`]).
pub fn plan_error_of(status: u16, body: &[u8]) -> AiError {
    let code = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|json| json.get("error").and_then(|error| text_at(error, "code")))
        .unwrap_or_default();
    match code.as_str() {
        "subscription_sharing_usage_limit_exceeded" => AiError::Busy(
            tr("The usage limit for Torro Workplace in your ChatGPT plan is reached. Raise it in ChatGPT or wait.")
                .into(),
        ),
        "subscription_sharing_usage_unavailable" | "subscription_sharing_user_unavailable" => {
            AiError::Busy(tr("ChatGPT cannot be used through your plan right now. Try again later.").into())
        }
        "subscription_sharing_user_not_eligible" => {
            AiError::Invalid(tr("Your ChatGPT plan cannot be used in other apps.").into())
        }
        "subscription_sharing_unsupported_capability" => AiError::Invalid(trf(
            "ChatGPT does not offer this through your plan yet: {what}",
            &[(
                "what",
                &serde_json::from_slice::<Value>(body)
                    .ok()
                    .and_then(|json| json.get("error").and_then(|error| text_at(error, "param")))
                    .unwrap_or_default(),
            )],
        )),
        "subscription_sharing_route_not_supported"
        | "chatpass_v2_scope_not_authorized"
        | "chatpass_v2_invalid_authorization_context"
        | "subscription_sharing_invalid_user" => AiError::Auth,
        _ => openai::error_of(status, body),
    }
}

/// The models of an account signed in with ChatGPT (`GET /v1/models`): `{"models": [{"slug", "display_name",
/// "visibility"}]}`, in the server's order, only those meant to be listed (`visibility: "list"`). They compute in
/// OpenAI's cloud and have closed weights; whether one calls tools the list does not say.
pub fn plan_models(bytes: &[u8]) -> Result<Vec<AiModel>, AiError> {
    let answer: Value = serde_json::from_slice(bytes)
        .map_err(|_| AiError::Format(tr("The AI server sent an answer that is not JSON.").into()))?;
    let models = answer
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| AiError::Format(tr("The AI server sent no model list.").into()))?;
    Ok(models
        .iter()
        .filter(|model| model.get("visibility").and_then(Value::as_str).is_none_or(|seen| seen == "list"))
        .filter_map(|model| {
            let slug = model.get("slug").and_then(Value::as_str).filter(|slug| !slug.is_empty())?;
            let mut found = AiModel::named(slug);
            if let Some(name) = model.get("display_name").and_then(Value::as_str).filter(|name| !name.is_empty()) {
                found.name = name.to_string();
            }
            found.open_weights = Some(false);
            found.remote = Some(true);
            Some(found)
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attempt(client: Client) -> Attempt {
        Attempt {
            state: "state-1".into(),
            nonce: "nonce-1".into(),
            verifier: Secret::new("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            redirect_uri: CHATGPT.redirect_uri(1455),
            client,
            host_id: "urn:uuid:0f8fad5b-d9cb-469f-a165-70867728950e".into(),
        }
    }

    fn jwt(claims: Value) -> String {
        format!("eyJhbGciOiJSUzI1NiJ9.{}.c2ln", url_safe(claims.to_string().as_bytes()))
    }

    fn id_token(client: &str, nonce: &str, sub: &str) -> String {
        jwt(serde_json::json!({
            "iss": "https://auth.openai.com", "aud": client, "nonce": nonce, "sub": sub,
            "email": "anna@example.org", "exp": 2_000_000_000u64,
        }))
    }

    const SCOPE: &str = "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";

    #[test]
    fn the_challenge_is_rfc_7636s_s256() {
        // The example of RFC 7636, appendix B.
        assert_eq!(
            challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    /// The way a provider that lets apps register themselves signs in (the scaffold keeps it for later providers).
    const REGISTERING: SignIn = SignIn {
        registration: Registration::Dynamic { entry: "dynamic_agent_client", name_hint: "Torro Workplace" },
        client_env: "TORRO_TEST_NO_SUCH_VARIABLE",
        built_client_id: None,
        ..CHATGPT
    };

    #[test]
    fn a_first_sign_in_registers_with_the_apps_name_and_this_computer() {
        let url = authorize_url(&REGISTERING, &attempt(Client::Register)).expect("url");
        let parsed = url::Url::parse(&url).expect("parses");
        let pairs: std::collections::HashMap<_, _> = parsed.query_pairs().into_owned().collect();
        assert!(url.starts_with("https://auth.openai.com/api/accounts/authorize?"));
        assert_eq!(pairs["client_id"], "dynamic_agent_client");
        assert_eq!(pairs["agent_name_hint"], "Torro Workplace");
        assert_eq!(pairs["ext_agent_host_id"], "urn:uuid:0f8fad5b-d9cb-469f-a165-70867728950e");
        assert_eq!(pairs["redirect_uri"], "http://127.0.0.1:1455/auth/callback");
        assert_eq!(pairs["scope"], SCOPE);
        assert_eq!(pairs["resource"], "https://api.openai.com/v1");
        assert_eq!(pairs["code_challenge_method"], "S256");
        assert_eq!(pairs["code_challenge"], "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        assert!(!url.contains("dBjftJeZ"), "the verifier never travels");
        let known = authorize_url(&CHATGPT, &attempt(Client::Known("oaiapp_1".into()))).expect("url");
        assert!(known.contains("client_id=oaiapp_1") && !known.contains("agent_name_hint"));
        assert!(known.contains("ext_agent_host_id=urn"), "ChatGPT wants this computer's id with its own client id");
        assert_eq!(authorize_url(&CHATGPT, &attempt(Client::Register)), Err(not_configured()));
    }

    #[test]
    fn chatgpt_is_offered_only_with_a_configured_client_id() {
        let configured =
            SignIn { client_env: "TORRO_TEST_NO_SUCH_VARIABLE", built_client_id: Some(" own "), ..CHATGPT };
        assert!(configured.offered());
        assert_eq!(configured.client(Some("oaiapp_1")), Ok(Client::Known("own".into())));
        let plain = SignIn { client_env: "TORRO_TEST_NO_SUCH_VARIABLE", built_client_id: None, ..CHATGPT };
        assert!(!plain.offered(), "no client id, no ChatGPT");
        assert_eq!(plain.client(None), Err(not_configured()));
        assert_eq!(plain.client(Some("oaiapp_1")), Err(not_configured()), "an id issued before does not count");
        // A provider that lets apps register themselves: the issued id, else a registration.
        assert!(REGISTERING.offered());
        assert_eq!(REGISTERING.client(Some("oaiapp_1")), Ok(Client::Known("oaiapp_1".into())));
        assert_eq!(REGISTERING.client(None), Ok(Client::Register));
    }

    #[test]
    fn the_callback_is_checked_by_path_and_state_before_anything_else() {
        let register = attempt(Client::Register);
        let read = |target: &str| read_callback(&REGISTERING, &register, target);
        assert_eq!(read("/favicon.ico"), Err(CallbackError::Unrelated));
        assert_eq!(read("/auth/callback"), Err(CallbackError::Unrelated));
        assert_eq!(read("/auth/callback?code=x&state=other&client_id=oaiapp_1"), Err(CallbackError::Foreign));
        assert!(matches!(read("/auth/callback?error=access_denied&state=state-1"), Err(CallbackError::Failed(_))));
        assert!(matches!(read("/auth/callback?code=x&state=state-1"), Err(CallbackError::Failed(_))), "no client id");
        let ok = read("/auth/callback?code=c%2B1&scope=openid&state=state-1&client_id=oaiapp_1").expect("ok");
        assert_eq!((ok.code.expose(), ok.client_id.as_str()), ("c+1", "oaiapp_1"));
        assert!(!format!("{ok:?}").contains("c+1"));
        let known = attempt(Client::Known("oaiapp_1".into()));
        let back = read_callback(&CHATGPT, &known, "/auth/callback?code=x&state=state-1").expect("own id");
        assert_eq!(back.client_id, "oaiapp_1");
        let other = read_callback(&CHATGPT, &known, "/auth/callback?code=x&state=state-1&client_id=oaiapp_2");
        assert!(matches!(other, Err(CallbackError::Failed(_))), "another registration is refused");
    }

    #[test]
    fn forms_carry_the_verifier_the_resource_and_never_a_scope_on_refresh() {
        let register = attempt(Client::Register);
        let callback = Callback { code: Secret::new("code-1"), client_id: "oaiapp_1".into() };
        let form = code_form(&CHATGPT, &register, &callback);
        let get =
            |form: &[(&str, String)], name: &str| form.iter().find(|(key, _)| *key == name).map(|(_, v)| v.clone());
        assert_eq!(get(&form, "client_id").as_deref(), Some("oaiapp_1"));
        assert_eq!(get(&form, "code_verifier").as_deref(), Some(register.verifier.expose()));
        assert_eq!(get(&form, "resource").as_deref(), Some("https://api.openai.com/v1"));
        let tokens = signed_in();
        let refresh = refresh_form(&CHATGPT, &tokens);
        assert_eq!(get(&refresh, "refresh_token").as_deref(), Some("refresh-1"));
        assert!(get(&refresh, "scope").is_none());
        assert_eq!(get(&revoke_form(&tokens), "token_type_hint").as_deref(), Some("refresh_token"));
    }

    fn signed_in() -> TokenSet {
        let answer = serde_json::json!({
            "access_token": "access-1", "refresh_token": "refresh-1", "token_type": "Bearer", "expires_in": 3600,
            "scope": SCOPE, "id_token": id_token("oaiapp_1", "nonce-1", "user-1"),
        });
        read_tokens(&CHATGPT, answer.to_string().as_bytes(), 1_000, "oaiapp_1", None, Some("nonce-1")).expect("tokens")
    }

    #[test]
    fn a_token_answer_becomes_a_set_that_hides_its_tokens() {
        let tokens = signed_in();
        assert_eq!(
            (tokens.expires_at, tokens.subject.as_str(), tokens.email.as_str()),
            (4_600, "user-1", "anna@example.org")
        );
        let shown = format!("{tokens:?}");
        assert!(!shown.contains("access-1") && !shown.contains("refresh-1"), "{shown}");
        let stored = tokens.to_secret().expect("json");
        assert_eq!(TokenSet::from_secret(stored.expose()), Some(tokens.clone()));
        assert_eq!(TokenSet::from_secret("sk-an-api-key"), None);
        assert!(!tokens.needs_refresh(1_000) && tokens.needs_refresh(4_500) && tokens.needs_refresh(9_000));
    }

    #[test]
    fn a_refresh_keeps_who_signed_in_and_rotates_the_refresh_token() {
        let before = signed_in();
        let rotated = serde_json::json!({"access_token": "access-2", "refresh_token": "refresh-2", "expires_in": 3600});
        let after = read_tokens(&CHATGPT, rotated.to_string().as_bytes(), 5_000, "oaiapp_1", Some(&before), None)
            .expect("refreshed");
        assert_eq!((after.access_token.expose(), after.refresh_token.expose()), ("access-2", "refresh-2"));
        assert_eq!((after.subject.as_str(), after.scopes.len()), ("user-1", 6));
        let kept = serde_json::json!({"access_token": "access-3"});
        let same =
            read_tokens(&CHATGPT, kept.to_string().as_bytes(), 5_000, "oaiapp_1", Some(&after), None).expect("ok");
        assert_eq!(same.refresh_token.expose(), "refresh-2");
    }

    #[test]
    fn the_id_token_must_belong_to_this_attempt_and_the_plan_must_be_allowed() {
        let answer = |token: String, scope: &str| {
            serde_json::json!({"access_token": "a", "refresh_token": "r", "scope": scope, "id_token": token})
                .to_string()
        };
        let read = |body: String, previous: Option<&TokenSet>| {
            read_tokens(&CHATGPT, body.as_bytes(), 1_000, "oaiapp_1", previous, Some("nonce-1"))
        };
        assert!(read(answer(id_token("oaiapp_2", "nonce-1", "u"), SCOPE), None).is_err(), "audience");
        assert!(read(answer(id_token("oaiapp_1", "nonce-2", "u"), SCOPE), None).is_err(), "nonce");
        assert!(read(answer(id_token("oaiapp_1", "nonce-1", "u"), "openid email"), None).is_err(), "no plan use");
        assert!(read(answer("not-a-jwt".into(), SCOPE), None).is_err());
        let before = signed_in();
        assert!(read(answer(id_token("oaiapp_1", "nonce-1", "user-2"), SCOPE), Some(&before)).is_err(), "someone else");
        assert!(read(answer(id_token("oaiapp_1", "nonce-1", "user-1"), SCOPE), Some(&before)).is_ok());
        let without = serde_json::json!({"access_token": "a", "refresh_token": "r", "scope": SCOPE}).to_string();
        assert!(read(without, None).is_err(), "a sign-in says who it is");
    }

    #[test]
    fn token_errors_ask_to_sign_in_again_and_never_repeat_the_provider() {
        for code in ["invalid_grant", "refresh_token_reused", "refresh_token_expired"] {
            let body = serde_json::json!({"error": code, "error_description": "refresh-1 is gone"}).to_string();
            assert_eq!(token_error(400, body.as_bytes()), AiError::Auth, "{code}");
        }
        let client = token_error(401, br#"{"error":"invalid_client"}"#);
        assert!(matches!(client, AiError::Invalid(_)));
        assert!(!token_error(500, b"refresh-1").to_string().contains("refresh-1"));
    }

    #[test]
    fn plan_errors_say_where_the_limit_is_set() {
        let limit = br#"{"error":{"code":"subscription_sharing_usage_limit_exceeded","message":"x"}}"#;
        assert!(matches!(plan_error_of(429, limit), AiError::Busy(_)));
        let tools = br#"{"error":{"code":"subscription_sharing_unsupported_capability","param":"tools"}}"#;
        assert!(matches!(plan_error_of(400, tools), AiError::Invalid(message) if message.contains("tools")));
        assert_eq!(plan_error_of(401, br#"{"detail":"no"}"#), AiError::Auth);
    }

    #[test]
    fn the_plans_models_are_the_listed_ones_in_the_servers_order() {
        let body = br#"{"models":[
            {"slug":"gpt-6.2-sol","display_name":"GPT-6.2 Sol","visibility":"list"},
            {"slug":"hidden","display_name":"Hidden","visibility":"hide"},
            {"slug":"gpt-6.1-sol","display_name":"GPT-6.1 Sol","visibility":"list"}]}"#;
        let models = plan_models(body).expect("models");
        let ids: Vec<_> = models.iter().map(|m| (m.id.as_str(), m.name.as_str())).collect();
        assert_eq!(ids, [("gpt-6.2-sol", "GPT-6.2 Sol"), ("gpt-6.1-sol", "GPT-6.1 Sol")]);
        assert!(models.iter().all(|m| m.remote == Some(true) && m.open_weights == Some(false) && !m.enabled));
        assert!(plan_models(br#"{"data":[]}"#).is_err());
    }
}
