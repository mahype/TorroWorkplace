//! AI accounts (task KI.1a, `docs/ki.md`): servers the suite asks a model – Ollama and LM Studio on
//! this computer, OpenRouter in the cloud, any other OpenAI-compatible server. Connecting one
//! (`accounts.connect_ai`), its models (`ai.list_models`), finding local servers
//! (`ai.find_local_servers`), removing it (`accounts.remove_account`).
//!
//! The wire formats are `torro_core::ai::{openai, responses, anthropic}`; here they are sent, one
//! implementation of [`Provider`] per account family ([`OpenAiCompatible`] and [`Anthropic`]), both over
//! the same [`Wire`], which holds the rules that must not differ between them. Where a model runs
//! (task S.23): Ollama's cloud models
//! run on `ollama.com` through the local Ollama, so its model list says so per model
//! ([`AiModel::remote`]), and the core keeps the last list per account ([`AiAccounts::known_models`])
//! so the data rule need not ask the server at every message. The accounts are kept like the GitHub accounts
//! ([`super::github`]): the list in `accounts.json` (kind `ai`, the server's base address in
//! `server`, the kind of server in `Account::ai`), the API key in the system keychain under the
//! account id, loaded once in the background ([`AiAccounts::start`]).
//!
//! The key leaves the core only towards the account's own server, and only in the key header of
//! its protocol: `Authorization: Bearer` for the OpenAI-compatible one, `x-api-key` for
//! Anthropic's, which takes it nowhere else ([`Auth`]). It travels only over https or to this
//! computer ([`torro_core::ai::key_may_travel`]), the HTTP client follows no redirect (a redirect
//! could carry it elsewhere), the header is marked sensitive so no `Debug` prints it, and every
//! error passes through [`openai::scrub`], so no answer, message or log line repeats it. All four
//! live in [`Wire`], once for both protocols.
//!
//! Which models are on for the suite (task S.24, `docs/settings.md` › KI) is kept here as well, apart
//! from the lists the servers give: `<config>/ai-models.json` (0600, written whole and atomically,
//! [`torro_core::ai::ModelSwitches`]; never written over while it does not read, `Switches::damaged`). Listing a server's models never sets [`AiModel::enabled`];
//! the core marks a list it answers with ([`AiAccounts::mark_enabled`]). Where a model computes,
//! which the rule at the start needs, is the data rule's word ([`crate::assistant::place_of`]).

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::sync::RwLock;
use torro_core::Account;
use torro_core::account::{self, AccountKind, Secret};
use torro_core::ai::{
    self, AiError, AiModel, AiProvider, BoxFuture, ChatReply, ChatRequest, ModelSwitches, Provider, anthropic, openai,
    responses, systemone,
};
use torro_core::i18n::{tr, trf};
use torro_ipc::{AccountInfo, AiServer};
use torro_pim::AccountState;

use super::github::{SystemTokens, TokenStore};
use super::{keychain, with_account};

/// The local servers [`AiAccounts::find_local`] asks, in this order: the providers that run on
/// this computer, at their usual addresses.
pub const LOCAL_SERVERS: [AiProvider; 2] = [AiProvider::Ollama, AiProvider::LmStudio];

/// How long a connection to an AI server may take to open.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// How long the server may stay silent within an answer. Long: a local server loads a large model
/// before the first token, which can take minutes on a small computer. There is no limit for the
/// whole answer; a conversation is cancelled by dropping its future.
const READ_TIMEOUT: Duration = Duration::from_secs(300);

/// How long [`AiAccounts::find_local`] waits for each local server: one that runs answers at once.
const PROBE_TIMEOUT: Duration = Duration::from_secs(2);

/// How long Ollama may take to describe one model (`/api/show`, read from disk) or to list where its
/// models run (`/api/tags`); a model it does not describe in time stays without a word on tools, so
/// one stuck model never holds up the list, and without the tags in time every model counts as
/// elsewhere.
const SHOW_TIMEOUT: Duration = Duration::from_secs(5);

/// The HTTP client for AI servers: no redirects (the key must not follow one to another host), no
/// limit for a whole answer (streams run long), [`READ_TIMEOUT`] between two pieces of it. The
/// shared client of the calendars (`torro_pim_caldav::http_client`) ends every request after 60 s,
/// too short for a model's answer.
pub fn http_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .user_agent("Torro Workplace")
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|err| err.to_string())
}

/// How a protocol carries the API key. Both ways are a header to the account's own server and
/// nothing else; no protocol reads the other's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Auth {
    /// `Authorization: Bearer <key>`, the OpenAI-compatible protocol.
    Bearer,
    /// `x-api-key: <key>`, Anthropic's protocol, which ignores `Authorization` for an API key. Its
    /// requests also carry `anthropic-version`.
    ApiKey,
}

/// What both protocols need of the network, in one place: the address, the key in the header its
/// protocol reads, a request that is sent and read, and the two rules that must never differ
/// between them – every error through [`openai::scrub`], and the key only ever in that one header.
/// `Debug` never shows the key ([`Secret`]).
#[derive(Debug, Clone)]
struct Wire {
    http: reqwest::Client,
    /// The base address of the API, as `torro_core::ai::normalize_base` writes it, without `/` at
    /// the end: `http://localhost:11434/v1`.
    base: String,
    key: Option<Secret>,
    auth: Auth,
}

impl Wire {
    fn new(http: reqwest::Client, base: &str, key: Option<Secret>, auth: Auth) -> Wire {
        Wire { http, base: base.trim_end_matches('/').to_string(), key, auth }
    }

    /// A request to `<base>/<path>` with the key, if there is one.
    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.authorized(self.http.request(method, format!("{}/{path}", self.base)))
    }

    /// The key goes into the one header its protocol reads and nowhere else, marked sensitive so
    /// no `Debug` of the request prints it (`bearer_auth` marks its own). A key that is no legal
    /// header value is left out; the server then answers `auth`, which is what it is.
    fn authorized(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let builder = match self.auth {
            Auth::ApiKey => builder.header("anthropic-version", anthropic::VERSION),
            Auth::Bearer => builder,
        };
        let Some(key) = &self.key else { return builder };
        match self.auth {
            Auth::Bearer => builder.bearer_auth(key.expose()),
            Auth::ApiKey => match reqwest::header::HeaderValue::from_str(key.expose()) {
                Ok(mut value) => {
                    value.set_sensitive(true);
                    builder.header("x-api-key", value)
                }
                Err(_) => builder,
            },
        }
    }

    /// Sends `builder`; an answer other than 2xx is the error its status and body say.
    async fn send(&self, builder: reqwest::RequestBuilder) -> Result<reqwest::Response, AiError> {
        let response = builder.send().await.map_err(|err| self.scrubbed(transport_error(&err)))?;
        if response.status().is_success() {
            return Ok(response);
        }
        let status = response.status().as_u16();
        let body = response.bytes().await.unwrap_or_default();
        Err(self.scrubbed(openai::error_of(status, &body)))
    }

    async fn body(&self, response: reqwest::Response) -> Result<Vec<u8>, AiError> {
        response.bytes().await.map(|bytes| bytes.to_vec()).map_err(|err| self.scrubbed(transport_error(&err)))
    }

    /// Reads the body of `response` as it arrives, handing every piece to `read` – the decoder of
    /// the protocol, which is the only part of streaming that differs.
    async fn chunks(
        &self,
        mut response: reqwest::Response,
        read: &mut (dyn FnMut(&[u8]) -> Result<(), AiError> + Send),
    ) -> Result<(), AiError> {
        loop {
            match response.chunk().await {
                Ok(Some(bytes)) => read(&bytes)?,
                Ok(None) => return Ok(()),
                Err(err) => return Err(self.scrubbed(transport_error(&err))),
            }
        }
    }

    /// `error` without any server text that repeats the key ([`openai::scrub`]).
    fn scrubbed(&self, error: AiError) -> AiError {
        openai::scrub(error, self.key.as_ref().map(Secret::expose))
    }
}

/// Whether `response` is a stream of server-sent events, or a whole answer because the server
/// ignored `stream: true`.
fn streamed(response: &reqwest::Response) -> bool {
    response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("text/event-stream"))
}

/// Some OpenAI models accept Responses tools but no configurable reasoning effort. A 400 before
/// streaming is safe to retry without that optional setting; other errors must remain visible.
fn refuses_reasoning(error: &AiError) -> bool {
    let AiError::Server { status: 400, message } = error else { return false };
    let message = message.to_ascii_lowercase();
    message.contains("reasoning")
        && ["not support", "unsupported", "unknown parameter", "invalid"].iter().any(|word| message.contains(word))
}

/// An OpenAI-compatible account: Chat Completions for most providers, Responses for OpenAI.
/// `Debug` never shows the key ([`Secret`]).
#[derive(Debug, Clone)]
pub struct OpenAiCompatible {
    wire: Wire,
    provider: AiProvider,
}

impl OpenAiCompatible {
    /// The server of `provider` at `base` with `key` (none for most local servers). Sends nothing
    /// yet.
    pub fn new(http: reqwest::Client, provider: AiProvider, base: &str, key: Option<Secret>) -> OpenAiCompatible {
        OpenAiCompatible { wire: Wire::new(http, base, key, Auth::Bearer), provider }
    }

    /// Whether the server takes this account: OpenRouter's key with `GET <base>/key` (its model
    /// list is public, so it would not tell), any other server by listing its models.
    pub async fn check(&self) -> Result<(), AiError> {
        match self.provider {
            AiProvider::OpenRouter => self.wire.send(self.wire.request(reqwest::Method::GET, "key")).await.map(drop),
            _ => self.list().await.map(drop),
        }
    }

    /// `GET <base>/models`, without asking Ollama about tools: what finding a server needs.
    pub async fn list(&self) -> Result<Vec<AiModel>, AiError> {
        let response = self.wire.send(self.wire.request(reqwest::Method::GET, "models")).await?;
        let body = self.wire.body(response).await?;
        openai::parse_models(&body).map_err(|err| self.wire.scrubbed(err))
    }

    /// Ollama's word on whether `model` calls tools, thinks (issue #131) and decides (task
    /// E12.3) – `POST <root>/api/show`; `None` for each when it does not answer within
    /// [`SHOW_TIMEOUT`] or does not say.
    async fn ollama_capabilities(&self, model: &str) -> (Option<bool>, Option<bool>, Option<bool>) {
        let url = format!("{}/api/show", openai::ollama_root(&self.wire.base));
        let builder = self.wire.authorized(self.wire.http.post(url)).json(&openai::ollama_show_body(model));
        let asked = async {
            let response = self.wire.send(builder).await.ok()?;
            response.bytes().await.ok()
        };
        match tokio::time::timeout(SHOW_TIMEOUT, asked).await.ok().flatten() {
            Some(body) => (openai::ollama_tools(&body), openai::ollama_thinking(&body), openai::ollama_decision(&body)),
            None => (None, None, None),
        }
    }

    /// Which of Ollama's models run elsewhere (`GET <root>/api/tags`, task S.23,
    /// [`openai::ollama_remote`]): one request for all models. `None` when it does not answer
    /// within [`SHOW_TIMEOUT`] or not with a model list; then every model counts as elsewhere.
    async fn ollama_tags(&self) -> Option<HashMap<String, bool>> {
        let url = format!("{}/api/tags", openai::ollama_root(&self.wire.base));
        let builder = self.wire.authorized(self.wire.http.get(url));
        let asked = async {
            let response = self.wire.send(builder).await.ok()?;
            response.bytes().await.ok()
        };
        let body = tokio::time::timeout(SHOW_TIMEOUT, asked).await.ok().flatten()?;
        openai::ollama_remote(&body)
    }

    /// The answer of `POST <base>/chat/completions`, read as server-sent events, or whole when the
    /// server ignored `stream: true` and answered with plain JSON.
    async fn stream(
        &self,
        response: reqwest::Response,
        on_text: &mut (dyn FnMut(&str) + Send),
    ) -> Result<ChatReply, AiError> {
        if !streamed(&response) {
            let reply = openai::parse_reply(&self.wire.body(response).await?).map_err(|err| self.wire.scrubbed(err))?;
            if !reply.text.is_empty() {
                on_text(&reply.text);
            }
            return Ok(reply);
        }
        let mut decoder = openai::StreamDecoder::new();
        self.wire
            .chunks(response, &mut |bytes| decoder.push(bytes, on_text).map_err(|err| self.wire.scrubbed(err)))
            .await?;
        decoder.finish(on_text).map_err(|err| self.wire.scrubbed(err))
    }

    /// OpenAI's Responses API uses a different body and stream format from Chat Completions.
    async fn stream_responses(
        &self,
        response: reqwest::Response,
        on_text: &mut (dyn FnMut(&str) + Send),
    ) -> Result<ChatReply, AiError> {
        if !streamed(&response) {
            let reply =
                responses::parse_reply(&self.wire.body(response).await?).map_err(|err| self.wire.scrubbed(err))?;
            if !reply.text.is_empty() {
                on_text(&reply.text);
            }
            return Ok(reply);
        }
        let mut decoder = responses::StreamDecoder::new();
        self.wire
            .chunks(response, &mut |bytes| decoder.push(bytes, on_text).map_err(|err| self.wire.scrubbed(err)))
            .await?;
        decoder.finish(on_text).map_err(|err| self.wire.scrubbed(err))
    }
}

impl Provider for OpenAiCompatible {
    /// `GET <base>/models`; for Ollama each model's capabilities from `/api/show` besides, one after
    /// the other (a local server, a handful of models), and where each runs from `/api/tags`
    /// ([`openai::ollama_runs_elsewhere`]: a cloud model, or every model when the tags do not
    /// answer, is [`AiModel::remote`]).
    fn models(&self) -> BoxFuture<'_, Result<Vec<AiModel>, AiError>> {
        Box::pin(async move {
            let mut models = self.list().await?;
            if self.provider == AiProvider::Ollama {
                let tags = self.ollama_tags().await;
                for model in &mut models {
                    (model.tools, model.reasoning, model.decision) = self.ollama_capabilities(&model.id).await;
                    model.remote = Some(openai::ollama_runs_elsewhere(&model.id, tags.as_ref()));
                }
            }
            Ok(models)
        })
    }

    /// OpenAI uses `POST <base>/responses`; other providers use `POST <base>/chat/completions`.
    /// Both stream when supported and accept a whole JSON answer from a compatible server.
    fn chat<'a>(
        &'a self,
        request: &'a ChatRequest,
        on_text: &'a mut (dyn FnMut(&str) + Send),
    ) -> BoxFuture<'a, Result<ChatReply, AiError>> {
        Box::pin(async move {
            if self.provider == AiProvider::OpenAi {
                let mut body = responses::body(request, true);
                let response =
                    match self.wire.send(self.wire.request(reqwest::Method::POST, "responses").json(&body)).await {
                        Ok(response) => response,
                        Err(error) if body.get("reasoning").is_some() && refuses_reasoning(&error) => {
                            body.as_object_mut().expect("Responses body is an object").remove("reasoning");
                            self.wire.send(self.wire.request(reqwest::Method::POST, "responses").json(&body)).await?
                        }
                        Err(error) => return Err(error),
                    };
                return self.stream_responses(response, on_text).await;
            }
            let body = openai::chat_body_for(self.provider, request, true);
            let builder = self.wire.request(reqwest::Method::POST, "chat/completions").json(&body);
            let response = self.wire.send(builder).await?;
            self.stream(response, on_text).await
        })
    }

    /// `POST <base>/systemone` (task E12.3): the base already ends in `/v1`, so this is Ollama's
    /// `/v1/systemone`. Only Ollama speaks the protocol; every other OpenAI-compatible server
    /// keeps the trait's default and refuses, rather than posting to an address it does not know.
    fn decide<'a>(
        &'a self,
        request: &'a systemone::DecisionRequest,
    ) -> BoxFuture<'a, Result<systemone::DecisionReply, AiError>> {
        Box::pin(async move {
            if self.provider != AiProvider::Ollama {
                return Err(AiError::Invalid(
                    tr("Only Ollama offers decision models at the moment. Pick a model of an Ollama account.").into(),
                ));
            }
            let body = systemone::decide_body_for(request)?;
            let builder = self.wire.request(reqwest::Method::POST, "systemone").json(&body);
            let response = self.wire.send(builder).await?;
            let bytes = self.wire.body(response).await?;
            systemone::read_decision(&bytes)
        })
    }
}

/// Anthropic's Messages API (task KI.1c), the Claude models: the second protocol of the suite. Its
/// wire format is `torro_core::ai::anthropic`; what differs here from [`OpenAiCompatible`] is only
/// the paths (`/messages`, `/models`), the key header ([`Auth::ApiKey`]) and the decoder.
/// `Debug` never shows the key ([`Secret`]).
#[derive(Debug, Clone)]
pub struct Anthropic {
    wire: Wire,
}

impl Anthropic {
    /// The server at `base` with `key`. Sends nothing yet.
    pub fn new(http: reqwest::Client, base: &str, key: Option<Secret>) -> Anthropic {
        Anthropic { wire: Wire::new(http, base, key, Auth::ApiKey) }
    }

    /// Whether the server takes this account: by listing its models, which needs the key, so a
    /// key it refuses is [`AiError::Auth`].
    pub async fn check(&self) -> Result<(), AiError> {
        self.list().await.map(drop)
    }

    /// `GET <base>/models?limit=1000`: one request for the whole catalogue. The server's default
    /// page is 20 models and its limit is 1000, far above every model Anthropic offers, so the
    /// answer is never cut short.
    pub async fn list(&self) -> Result<Vec<AiModel>, AiError> {
        let response = self.wire.send(self.wire.request(reqwest::Method::GET, "models?limit=1000")).await?;
        let body = self.wire.body(response).await?;
        anthropic::parse_models(&body).map_err(|err| self.wire.scrubbed(err))
    }

    /// The answer of `POST <base>/messages`, read as server-sent events, or whole when the server
    /// answered with plain JSON.
    async fn stream(
        &self,
        response: reqwest::Response,
        on_text: &mut (dyn FnMut(&str) + Send),
    ) -> Result<ChatReply, AiError> {
        if !streamed(&response) {
            let reply =
                anthropic::parse_reply(&self.wire.body(response).await?).map_err(|err| self.wire.scrubbed(err))?;
            if !reply.text.is_empty() {
                on_text(&reply.text);
            }
            return Ok(reply);
        }
        let mut decoder = anthropic::StreamDecoder::new();
        self.wire
            .chunks(response, &mut |bytes| decoder.push(bytes, on_text).map_err(|err| self.wire.scrubbed(err)))
            .await?;
        decoder.finish(on_text).map_err(|err| self.wire.scrubbed(err))
    }
}

impl Provider for Anthropic {
    /// `GET <base>/models?limit=1000`. Every model of this provider calls tools and runs in its
    /// cloud, which [`anthropic::parse_models`] says; nothing else is asked.
    fn models(&self) -> BoxFuture<'_, Result<Vec<AiModel>, AiError>> {
        Box::pin(self.list())
    }

    /// `POST <base>/messages` with `stream: true` and the effort as `output_config.effort`
    /// ([`anthropic::chat_body`]); read as server-sent events.
    fn chat<'a>(
        &'a self,
        request: &'a ChatRequest,
        on_text: &'a mut (dyn FnMut(&str) + Send),
    ) -> BoxFuture<'a, Result<ChatReply, AiError>> {
        Box::pin(async move {
            let body = anthropic::chat_body(request, true);
            let builder = self.wire.request(reqwest::Method::POST, "messages").json(&body);
            let response = self.wire.send(builder).await?;
            self.stream(response, on_text).await
        })
    }
}

/// One AI account's server, whichever protocol it speaks: what [`AiAccounts`] builds from an
/// account and hands out as `dyn Provider`. The choice is the account's [`AiProvider`] and is made
/// in one place ([`AiClient::new`]), so no caller has to know the protocols.
#[derive(Debug, Clone)]
enum AiClient {
    OpenAi(OpenAiCompatible),
    Anthropic(Anthropic),
}

impl AiClient {
    /// The server of `provider` at `base` with `key`. Anthropic speaks its own protocol, every
    /// other provider the OpenAI-compatible one.
    fn new(http: reqwest::Client, provider: AiProvider, base: &str, key: Option<Secret>) -> AiClient {
        match provider {
            AiProvider::Anthropic => AiClient::Anthropic(Anthropic::new(http, base, key)),
            _ => AiClient::OpenAi(OpenAiCompatible::new(http, provider, base, key)),
        }
    }

    /// Whether the server takes this account, as its protocol checks it.
    async fn check(&self) -> Result<(), AiError> {
        match self {
            AiClient::OpenAi(client) => client.check().await,
            AiClient::Anthropic(client) => client.check().await,
        }
    }
}

impl Provider for AiClient {
    fn models(&self) -> BoxFuture<'_, Result<Vec<AiModel>, AiError>> {
        match self {
            AiClient::OpenAi(client) => client.models(),
            AiClient::Anthropic(client) => client.models(),
        }
    }

    fn chat<'a>(
        &'a self,
        request: &'a ChatRequest,
        on_text: &'a mut (dyn FnMut(&str) + Send),
    ) -> BoxFuture<'a, Result<ChatReply, AiError>> {
        match self {
            AiClient::OpenAi(client) => client.chat(request, on_text),
            AiClient::Anthropic(client) => client.chat(request, on_text),
        }
    }

    /// Handed on, so a decision reaches the one protocol that speaks it (task E12.3); Anthropic
    /// keeps the trait's default.
    fn decide<'a>(
        &'a self,
        request: &'a systemone::DecisionRequest,
    ) -> BoxFuture<'a, Result<systemone::DecisionReply, AiError>> {
        match self {
            AiClient::OpenAi(client) => client.decide(request),
            AiClient::Anthropic(client) => client.decide(request),
        }
    }
}

/// A request that got no answer: unreachable or too slow is [`AiError::Offline`].
fn transport_error(err: &reqwest::Error) -> AiError {
    if err.is_connect() || err.is_timeout() {
        AiError::Offline
    } else {
        AiError::Format(torro_core::i18n::trf(
            "The AI server could not be asked: {error}",
            &[("error", &err.to_string())],
        ))
    }
}

/// One AI account as the core holds it.
#[derive(Debug)]
struct Slot {
    account: Account,
    /// `None` without a key: a local server that wants none, or a key the keychain did not give
    /// (`problem` then says so).
    key: Option<Secret>,
    /// When the server last answered for this account (connecting, listing models).
    checked: Option<DateTime<Utc>>,
    problem: Option<String>,
    /// All its models as the server last listed them ([`AiAccounts::models`]); `None` until it did.
    /// What the data rule reads where a model runs ([`AiAccounts::known_models`]).
    models: Option<Vec<AiModel>>,
}

impl Slot {
    fn provider(&self) -> AiProvider {
        self.account.ai.unwrap_or(AiProvider::OpenAiCompatible)
    }

    fn info(&self) -> AccountInfo {
        AccountInfo {
            id: self.account.id.clone(),
            kind: AccountKind::Ai.name().into(),
            label: label(&self.account),
            state: AccountState::Connected {
                server: self.account.server.clone(),
                login_name: String::new(),
                last_sync: self.checked,
                problem: self.problem.clone(),
            },
            collections: Vec::new(),
            scopes: None,
            mail: None,
            ntfy: None,
            services: None,
        }
    }
}

/// What to call an AI account: the provider's name at its usual address (`Ollama`, `OpenRouter`),
/// with the host elsewhere (`Ollama (gpu.local:11434)`), the host alone for any other server.
pub fn label(account: &Account) -> String {
    let host = account.id.split_once('@').map_or(account.server.as_str(), |(_, host)| host);
    match account.ai.unwrap_or(AiProvider::OpenAiCompatible) {
        AiProvider::OpenAiCompatible => host.to_string(),
        provider if provider.default_url() == Some(account.server.as_str()) => provider.name().to_string(),
        provider => format!("{} ({host})", provider.name()),
    }
}

/// The core's AI accounts: loaded at start ([`AiAccounts::start`]), connected through
/// `accounts.connect_ai`, asked for models with `ai.list_models`, removed with
/// `accounts.remove_account`.
pub struct AiAccounts {
    /// `accounts.json`; `None` without a home directory, and then nothing can be stored.
    accounts_file: Option<PathBuf>,
    keys: Arc<dyn TokenStore>,
    /// `None` when no HTTP client could be built; every request then fails with a message.
    http: Option<reqwest::Client>,
    /// What [`AiAccounts::find_local`] asks: [`LOCAL_SERVERS`] at their usual addresses; fake
    /// servers in tests.
    local: Vec<(AiProvider, String)>,
    /// In the order of `accounts.json`; a new account comes last.
    slots: RwLock<Vec<Slot>>,
    /// Which models are on (task S.24): `ai-models.json` next to `accounts.json`, read on first use
    /// ([`AiAccounts::switches`]); `None` until then. One lock for reading and writing, so two
    /// switches at once never write over each other.
    switches: tokio::sync::Mutex<Option<Switches>>,
    /// `false` while the core keeps the models in use on at its start (#231,
    /// [`AiAccounts::hold_until_kept`]): until then the readers of the switches wait
    /// ([`AiAccounts::kept`]), so no module counts a model in use as off. `true` otherwise.
    kept: tokio::sync::watch::Sender<bool>,
}

/// How long the readers of the switches wait for the models in use to be kept on at the start
/// ([`AiAccounts::kept`]) before they read the switches as they are: it reads two files and writes
/// one, so this is only a bound for a disk that hangs.
const KEPT_WAIT: Duration = Duration::from_secs(10);

/// What the core holds of `ai-models.json` (task S.24), as the file holds it: the switches, and
/// whether the models in use are still to be kept on.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Switches {
    #[serde(flatten)]
    held: ModelSwitches,
    /// The first start after the update that brought the switches has not passed yet (#231): there
    /// was no file when the core first read it, so the models in use are still to be kept on
    /// ([`AiAccounts::keep_used_models_on`]). A mark of its own, kept in the file, so a switch a
    /// person turns before (which writes the file) does not stop it, and a core that stops in
    /// between keeps them on at its next start. Never set for a file that was there (older ones
    /// read without it), one that does not read, or without a home directory: nothing is turned
    /// on in doubt.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    keep_used: bool,
    /// Accounts a module used with "Automatic" before the switches came (#231), whose model is still
    /// to be kept on: at the first start after the update the core may not reach their servers, and
    /// what "Automatic" took is in the list of the server only. Taken over at the first list that
    /// loads ([`AiAccounts::take_over`]); until then such an account counts as offering a model
    /// ([`AiAccounts::offers_a_model`]). A person who switches at the account drops it.
    #[serde(default, skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    automatic: std::collections::BTreeMap<String, Automatic>,
    /// The file that is there but did not read when the core first looked (PR review #242), so the
    /// switches above are empty and only the rule decides. Never written then
    /// ([`Switches::writable`]): writing would lose every switch the person made. Only in memory.
    #[serde(skip)]
    damaged: Option<PathBuf>,
}

impl Switches {
    /// The switches as they were read from `file`, when it is there but does not read.
    fn damaged(file: &std::path::Path) -> Switches {
        Switches { damaged: Some(file.to_path_buf()), ..Switches::default() }
    }

    /// `Ok` when these switches may be written; the sentence that names the file when it did not
    /// read ([`Switches::damaged`]): the person repairs or removes it, the core never does. By its
    /// name only: clients read the sentence too (`ai.set_model_enabled` is an MCP tool), the whole
    /// path is in the core's log from the first read ([`read_switches`]), as for `approvals.json`.
    fn writable(&self) -> Result<(), String> {
        match &self.damaged {
            None => Ok(()),
            Some(_) => Err(trf(
                "{file} cannot be read, so no model can be turned on or off. Repair or remove it, then restart Torro Workplace.",
                &[("file", &ai::MODELS_FILE)],
            )),
        }
    }
}

/// What a module used "Automatic" at an AI account for before the switches came (#231), so the
/// first start after the update keeps on the model it took ([`AiAccounts::keep_used_models_on`]).
/// Each mark that is set turns on its own pick ([`Automatic::picks`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Automatic {
    /// New chats or a bot: the model "Automatic" took then ([`crate::assistant::ModelAccess::automatic`]:
    /// open weights, on this computer first, with tools).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub chat: bool,
    /// A function that needs a decision model, with mail allowed to the cloud: the first model that
    /// decides, as sorting mail took it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub decides: bool,
    /// The same while mail may not leave this computer: the first model that decides and computes here.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub decides_here: bool,
}

impl Automatic {
    /// What "Automatic" was used for at a function: its decision model for one that needs one
    /// (only one on this computer for a function that reads mail while `mail_to_cloud` is off), else
    /// the model a chat takes.
    pub fn for_function(function: ai::AiFunction, mail_to_cloud: bool) -> Automatic {
        match function.needs_decision() {
            true if function.reads_mail() && !mail_to_cloud => Automatic { decides_here: true, ..Automatic::default() },
            true => Automatic { decides: true, ..Automatic::default() },
            false => Automatic { chat: true, ..Automatic::default() },
        }
    }

    /// Both uses together: every pick of either.
    fn and(self, other: Automatic) -> Automatic {
        Automatic {
            chat: self.chat || other.chat,
            decides: self.decides || other.decides,
            decides_here: self.decides_here || other.decides_here,
        }
    }

    /// The models of `models` (all of the account's, as its server lists them) that "Automatic"
    /// took for the uses marked, `place` saying where each computes; none for a use the account
    /// offers nothing for.
    pub fn picks(self, models: &[AiModel], place: impl Fn(&AiModel) -> crate::assistant::Place) -> Vec<AiModel> {
        let here = |model: &AiModel| place(model) == crate::assistant::Place::Local;
        let decides = |model: &&AiModel| model.decision == Some(true);
        let mut picks = Vec::new();
        if self.chat {
            picks.extend(crate::assistant::ModelAccess::automatic(models.to_vec(), &place).ok());
        }
        if self.decides {
            picks.extend(models.iter().find(decides).cloned());
        }
        if self.decides_here {
            picks.extend(models.iter().filter(decides).find(|model| here(model)).cloned());
        }
        picks
    }
}

/// Whether `model` of the account at `server` (its kind and base address, `None` when it is gone)
/// computes on this computer, as the data rule says ([`crate::assistant::place_of`]).
fn computes_here(server: Option<&(AiProvider, String)>, model: &AiModel) -> bool {
    server.is_some_and(|(provider, base)| {
        crate::assistant::place_of(*provider, base, model) == crate::assistant::Place::Local
    })
}

/// Whether any model of the account at `server` could compute on this computer: one that its server
/// says runs on its own machine would. Never for a provider in the cloud or a server on the network,
/// whose models are on only once someone turns them on.
fn may_compute_here(server: Option<&(AiProvider, String)>) -> bool {
    computes_here(server, &AiModel { remote: Some(false), ..AiModel::named("") })
}

/// The switches in `file`: none, with the models in use still to be kept on, when it is not there;
/// empty without a home directory. Empty and marked damaged ([`Switches::damaged`]) when it is there
/// but does not read, which the core's log then says: only the rule decides then (nothing elsewhere
/// is on), and the file is left as it is.
fn read_switches(file: Option<&std::path::Path>) -> Switches {
    let Some(file) = file else { return Switches::default() };
    match std::fs::read(file) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Switches { keep_used: true, ..Switches::default() },
        Err(err) => {
            eprintln!(
                "torroworkplace: cannot read {}, so only the rule decides which models are on: {err}",
                file.display()
            );
            Switches::damaged(file)
        }
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_else(|err| {
            eprintln!(
                "torroworkplace: {} does not read, so only the rule decides which models are on: {err}",
                file.display()
            );
            Switches::damaged(file)
        }),
    }
}

/// Writes `switches` to `file` (0600, atomically), creating the folder.
fn write_switches(file: &std::path::Path, switches: &Switches) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(switches).map_err(|err| err.to_string())?;
    if let Some(dir) = file.parent() {
        crate::clients::create_private_dir(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    }
    crate::clients::write_private(file, &bytes).map_err(|err| err.to_string())
}

impl AiAccounts {
    /// The service on the user's account list and keychain, as the daemon runs it. Reads and
    /// sends nothing yet; [`AiAccounts::start`] loads the accounts.
    pub fn system() -> AiAccounts {
        let http = match http_client() {
            Ok(http) => Some(http),
            Err(err) => {
                eprintln!("torroworkplace: no HTTP client for AI accounts: {err}");
                None
            }
        };
        let local = LOCAL_SERVERS
            .into_iter()
            .filter_map(|provider| provider.default_url().map(|url| (provider, url.to_string())))
            .collect();
        AiAccounts::new(account::accounts_file().ok(), Arc::new(SystemTokens), http, local)
    }

    /// The service on `accounts_file` and `keys`, asking servers with `http` and looking for local
    /// ones at `local`. Tests pass a temp file, a map and fake servers. Reads and sends nothing yet.
    pub fn new(
        accounts_file: Option<PathBuf>,
        keys: Arc<dyn TokenStore>,
        http: Option<reqwest::Client>,
        local: Vec<(AiProvider, String)>,
    ) -> AiAccounts {
        AiAccounts {
            accounts_file,
            keys,
            http,
            local,
            slots: RwLock::new(Vec::new()),
            switches: tokio::sync::Mutex::new(None),
            kept: tokio::sync::watch::Sender::new(true),
        }
    }

    /// Loads the stored AI accounts with their keys, in the background. Asks no server: a local one
    /// may not run now, and that is no problem until it is used – except the servers of accounts
    /// still to take over from "Automatic" (#231, [`AiAccounts::take_over`]), after the core kept
    /// the models in use on. An account that needs a key and has none is listed with a problem and
    /// can be connected again or removed.
    pub fn start(self: &Arc<Self>) {
        let service = Arc::clone(self);
        tokio::spawn(async move {
            if let Err(err) = service.load().await {
                eprintln!("torroworkplace: cannot read the AI accounts: {err}");
            }
            // What "Automatic" used before the switches (#231): once the core has noted it.
            service.kept().await;
            service.take_over_automatic().await;
        });
    }

    /// Reads the AI accounts of `accounts.json` and their keys.
    pub(crate) async fn load(&self) -> Result<(), String> {
        let Some(path) = self.accounts_file.clone() else { return Ok(()) };
        let keys = Arc::clone(&self.keys);
        let loaded = keychain(move || {
            let accounts = account::load_from(&path).map_err(|err| err.to_string())?;
            Ok(accounts
                .into_iter()
                .filter(|account| account.kind == AccountKind::Ai)
                .map(|account| {
                    let key = keys.get(&account);
                    (account, key)
                })
                .collect::<Vec<_>>())
        })
        .await?;
        let mut held = self.slots.write().await;
        for (account, key) in loaded {
            let needs_key = account.ai.is_some_and(AiProvider::needs_key);
            let (key, problem) = match key {
                Ok(Some(key)) => (Some(key), None),
                Ok(None) if needs_key => (None, Some(tr("Connect again: no API key is stored on this device").into())),
                Ok(None) => (None, None),
                Err(err) => (None, Some(format!("{} ({err})", tr("Connect again: the keychain could not be read")))),
            };
            // An account connected while loading is newer than the stored one.
            if !held.iter().any(|slot| slot.account.id == account.id) {
                held.push(Slot { account, key, checked: None, problem, models: None });
            }
        }
        Ok(())
    }

    /// The accounts as `accounts.list_accounts` lists them, never with their keys.
    pub async fn infos(&self) -> Vec<AccountInfo> {
        self.slots.read().await.iter().map(Slot::info).collect()
    }

    /// `id` is one of these accounts.
    pub async fn knows(&self, id: &str) -> bool {
        self.slots.read().await.iter().any(|slot| slot.account.id == id)
    }

    /// The kind of server and the base address of the account `id`, for the assistant's data rule
    /// (task C.2: where its model runs); `None` for an id that is no AI account.
    pub async fn server(&self, id: &str) -> Option<(AiProvider, String)> {
        let slots = self.slots.read().await;
        slots.iter().find(|slot| slot.account.id == id).map(|slot| (slot.provider(), slot.account.server.clone()))
    }

    /// The model behind the account `id`, for whatever asks it (the assistant, task C.2).
    /// [`AiError::NotFound`] for an id that is no AI account, [`AiError::NoKey`] when the account
    /// needs a key and none is stored on this device.
    pub async fn provider(&self, id: &str) -> Result<Box<dyn Provider>, AiError> {
        Ok(Box::new(self.client(id).await?))
    }

    async fn client(&self, id: &str) -> Result<AiClient, AiError> {
        let slots = self.slots.read().await;
        let slot = slots.iter().find(|slot| slot.account.id == id).ok_or_else(|| no_account(id))?;
        if slot.key.is_none() && slot.provider().needs_key() {
            return Err(AiError::NoKey);
        }
        Ok(AiClient::new(self.http()?.clone(), slot.provider(), &slot.account.server, slot.key.clone()))
    }

    /// One call to the decision model of the account `id` (task E12.6, `torro_core::ai::systemone`).
    /// `not-connected` when the account has no key on this device, `not-found` when it is gone.
    pub async fn decide(
        &self,
        id: &str,
        request: &systemone::DecisionRequest,
    ) -> Result<systemone::DecisionReply, AiError> {
        self.client(id).await?.decide(request).await
    }

    /// Checks the account with its server ([`OpenAiCompatible::check`]) and, when the server takes
    /// it, stores the key in the keychain (or removes an old one when there is none now) and the
    /// account in `accounts.json` (next to the others; the same provider at the same host replaces
    /// it), then answers with the account. Nothing is stored when the check fails.
    ///
    /// `server` is what a person typed ([`ai::normalize_base`]); `None` or blank is the provider's
    /// usual address. A blank `key` is none. Refused before anything is sent:
    /// [`AiProvider::OpenRouter`] without a key, a key to another computer without TLS
    /// ([`ai::key_may_travel`]), an address that is none.
    pub async fn connect(
        &self,
        provider: AiProvider,
        server: Option<&str>,
        key: Secret,
    ) -> Result<AccountInfo, AiError> {
        let base = match server.map(str::trim).filter(|server| !server.is_empty()) {
            Some(typed) => ai::normalize_base(typed)?,
            None => provider
                .default_url()
                .map(str::to_string)
                .ok_or_else(|| AiError::Invalid(tr("Enter the address of the server.").into()))?,
        };
        let key = Some(Secret::new(key.expose().trim())).filter(|key| !key.is_blank());
        if key.is_none() && provider.needs_key() {
            let needs =
                trf("{provider} needs an API key. Create one on its website.", &[("provider", &provider.name())]);
            return Err(AiError::Invalid(needs));
        }
        if key.is_some() && !ai::key_may_travel(&base) {
            return Err(AiError::Invalid(
                tr("An API key is only sent over https or to this computer. Use an https address.").into(),
            ));
        }
        let client = AiClient::new(self.http()?.clone(), provider, &base, key.clone());
        client.check().await?;
        let account = Account::ai(provider, &base);
        self.store(&account, key.as_ref()).await.map_err(AiError::Format)?;
        let slot = Slot { account, key, checked: Some(Utc::now()), problem: None, models: None };
        let info = slot.info();
        let mut slots = self.slots.write().await;
        match slots.iter_mut().find(|held| held.account.id == slot.account.id) {
            Some(held) => *held = slot,
            None => slots.push(slot),
        }
        Ok(info)
    }

    /// The key into the keychain (or an old one out of it), then the account into `accounts.json`:
    /// a stored key nobody lists is better than a listed account without one.
    async fn store(&self, account: &Account, key: Option<&Secret>) -> Result<(), String> {
        let path = self.accounts_file.clone().ok_or("No home directory, so the account cannot be stored.")?;
        let (keys, account, key) = (Arc::clone(&self.keys), account.clone(), key.cloned());
        keychain(move || {
            match &key {
                Some(key) => keys.set(&account, key)?,
                None => keys.delete(&account)?,
            }
            let accounts = account::load_from(&path).map_err(|err| err.to_string())?;
            account::save_to(&path, &with_account(accounts, account)).map_err(|err| err.to_string())
        })
        .await
    }

    /// The models of the account `id`, asked from its server now ([`Provider::models`]); without
    /// `all` only those not known to have closed weights ([`ai::open_weights_only`]). Reading
    /// changes no data, only how the account is shown: an answer renews when it last answered, a
    /// refused key becomes its problem until it answers again. The whole list is kept for
    /// [`AiAccounts::known_models`]. The one exception (#231): the first list after the update keeps
    /// on what "Automatic" used at the account ([`AiAccounts::take_over`]), once.
    pub async fn models(&self, id: &str, all: bool) -> Result<Vec<AiModel>, AiError> {
        let result = self.provider(id).await?.models().await;
        if let Some(slot) = self.slots.write().await.iter_mut().find(|slot| slot.account.id == id) {
            match &result {
                Ok(models) => {
                    slot.checked = Some(Utc::now());
                    slot.problem = None;
                    slot.models = Some(models.clone());
                }
                Err(AiError::Auth) => {
                    slot.problem = Some(tr("Connect again: the server does not accept the API key").into());
                }
                Err(_) => {}
            }
        }
        let models = result?;
        self.take_over(id, &models).await;
        Ok(if all { models } else { ai::open_weights_only(models) })
    }

    /// Keeps on what "Automatic" took at the account `id` before the switches came (#231,
    /// [`Switches::automatic`]), now that its server listed `models` (all of them): once, and only
    /// while it is still to be taken over. A file that cannot be written is only reported; the next
    /// list tries again. The log names the account and how many models, never which.
    async fn take_over(&self, id: &str, models: &[AiModel]) {
        let server = self.server(id).await;
        let mut held = self.switches().await;
        let Some(switches) = held.as_mut() else { return };
        let Some(automatic) = switches.automatic.get(id).copied() else { return };
        let Some((provider, base)) = server else { return };
        let place = |model: &AiModel| crate::assistant::place_of(provider, &base, model);
        let mut changed = switches.clone();
        changed.automatic.remove(id);
        let mut kept = 0;
        for model in automatic.picks(models, place) {
            if !changed.held.is_on(id, &model, place(&model) == crate::assistant::Place::Local) {
                changed.held.turn_on(id, &model.id);
                kept += 1;
            }
        }
        match self.store_switches(&changed).await {
            Ok(()) => {
                *switches = changed;
                eprintln!("torroworkplace: {id}: kept on {kept} model(s) \"Automatic\" used before the switches");
            }
            Err(err) => eprintln!("torroworkplace: cannot keep on what \"Automatic\" used at {id}: {err}"),
        }
    }

    /// Asks the server of every account still to take over from "Automatic" for its models
    /// ([`AiAccounts::take_over`], #231). One that does not answer stays marked for its first list.
    async fn take_over_automatic(&self) {
        let pending: Vec<String> =
            self.switches().await.as_ref().map(|held| held.automatic.keys().cloned().collect()).unwrap_or_default();
        for id in pending {
            if let Err(err) = self.models(&id, true).await {
                eprintln!("torroworkplace: {id}: \"Automatic\" is taken over at its first list ({})", err.code());
            }
        }
    }

    /// All models of the account `id` as its server last listed them ([`AiAccounts::models`]), asked
    /// now when it never did or when the list lacks `model`: what the data rule needs to know where
    /// a model runs ([`AiModel::remote`], task S.23) without asking the server at every message. It
    /// lives as long as the capabilities of the same list: until the account is listed again (the
    /// apps do with `ai.list_models`) or connected anew. Errors as [`AiAccounts::models`].
    pub async fn known_models(&self, id: &str, model: Option<&str>) -> Result<Vec<AiModel>, AiError> {
        let kept =
            self.slots.read().await.iter().find(|slot| slot.account.id == id).and_then(|slot| slot.models.clone());
        match kept {
            Some(models) if model.is_none_or(|model| models.iter().any(|known| known.id == model)) => Ok(models),
            _ => self.models(id, true).await,
        }
    }

    /// The local servers that answer now ([`LOCAL_SERVERS`]), asked side by side, each for at most
    /// [`PROBE_TIMEOUT`]: provider, address, how many models, and the AI account connected there.
    /// Never with a key.
    pub async fn find_local(&self) -> Vec<AiServer> {
        let Ok(http) = self.http() else { return Vec::new() };
        let mut probes = tokio::task::JoinSet::new();
        for (at, (provider, url)) in self.local.iter().cloned().enumerate() {
            let client = OpenAiCompatible::new(http.clone(), provider, &url, None);
            probes.spawn(async move {
                let models = tokio::time::timeout(PROBE_TIMEOUT, client.list()).await.ok()?.ok()?;
                Some((at, provider, url, models.len()))
            });
        }
        let mut found: Vec<(usize, AiProvider, String, usize)> =
            probes.join_all().await.into_iter().flatten().collect();
        found.sort_by_key(|(at, ..)| *at);
        let slots = self.slots.read().await;
        found
            .into_iter()
            .map(|(_, provider, url, models)| {
                let id = Account::ai(provider, &url).id;
                let account = slots.iter().any(|slot| slot.account.id == id).then_some(id);
                AiServer { provider, url, models, account }
            })
            .collect()
    }

    /// Removes the account `id`, its key and its entry in `accounts.json`; the other accounts stay.
    /// The key goes first, as for the other accounts.
    pub async fn remove(&self, id: &str) -> Result<(), String> {
        let account = self.slots.read().await.iter().find(|slot| slot.account.id == id).map(|s| s.account.clone());
        let Some(account) = account else { return Err(format!("no account {id:?}")) };
        let (keys, path) = (Arc::clone(&self.keys), self.accounts_file.clone());
        keychain(move || {
            keys.delete(&account)?;
            let Some(path) = path else { return Ok(()) };
            let mut accounts = account::load_from(&path).map_err(|err| err.to_string())?;
            accounts.retain(|stored| stored.id != account.id);
            account::save_to(&path, &accounts).map_err(|err| err.to_string())
        })
        .await?;
        self.slots.write().await.retain(|slot| slot.account.id != id);
        self.forget_switches(id).await;
        Ok(())
    }

    // Which models are on (task S.24, `docs/settings.md` › KI).

    /// `ai-models.json` next to `accounts.json`; `None` without a home directory.
    fn models_file(&self) -> Option<PathBuf> {
        self.accounts_file.as_ref().map(|file| file.with_file_name(ai::MODELS_FILE))
    }

    /// The switches, read from `ai-models.json` on first use. Holds the lock: whoever changes them
    /// writes the file before letting go.
    async fn switches(&self) -> tokio::sync::MutexGuard<'_, Option<Switches>> {
        let mut held = self.switches.lock().await;
        if held.is_none() {
            let file = self.models_file();
            let read = tokio::task::spawn_blocking(move || read_switches(file.as_deref())).await;
            *held = Some(read.unwrap_or_default());
        }
        held
    }

    /// Makes the readers of the switches ([`AiAccounts::mark_enabled`], [`AiAccounts::offers_a_model`])
    /// wait until [`AiAccounts::keep_used_models_on`] has run (#231), at most [`KEPT_WAIT`]. The core
    /// calls it at its start, before anything can read them, and then keeps the models in use on.
    pub fn hold_until_kept(&self) {
        self.kept.send_replace(false);
    }

    /// Waits until the models in use are kept on ([`AiAccounts::hold_until_kept`]); at once when
    /// nobody holds the readers.
    async fn kept(&self) {
        let mut kept = self.kept.subscribe();
        // Timed out, the switches are read as they are: better a model counted off than a core
        // that hangs. Then for good, so a takeover that never came does not hold every reader again.
        if tokio::time::timeout(KEPT_WAIT, kept.wait_for(|kept| *kept)).await.is_err() {
            self.kept.send_replace(true);
        }
    }

    /// Writes `switches` to `ai-models.json` on a blocking thread; never while the file did not read
    /// ([`Switches::writable`]).
    async fn store_switches(&self, switches: &Switches) -> Result<(), String> {
        switches.writable()?;
        let file = self.models_file().ok_or("No home directory, so the models turned on cannot be stored.")?;
        let switches = switches.clone();
        tokio::task::spawn_blocking(move || write_switches(&file, &switches)).await.map_err(|err| err.to_string())?
    }

    /// The label of the account `id` ([`label`]), as the person reads it: `Ollama`, `OpenRouter`.
    pub async fn label_of(&self, id: &str) -> Option<String> {
        self.slots.read().await.iter().find(|slot| slot.account.id == id).map(|slot| label(&slot.account))
    }

    /// `models` of the account `id` with [`AiModel::enabled`] set from the switches: what
    /// `ai.list_models` answers. A list straight from the server has it `false` everywhere.
    pub async fn mark_enabled(&self, id: &str, models: Vec<AiModel>) -> Vec<AiModel> {
        self.kept().await;
        let server = self.server(id).await;
        let held = self.switches().await;
        let switches = held.as_ref().map(|held| &held.held);
        models
            .into_iter()
            .map(|mut model| {
                let here = computes_here(server.as_ref(), &model);
                model.enabled = switches.is_some_and(|switches| switches.is_on(id, &model, here));
                model
            })
            .collect()
    }

    /// Whether the model `model` of the account `id` is on: judged by its entry in `listed` (its
    /// account's models as the server listed them), else by its id alone ([`AiModel::named`]: a
    /// model of Ollama the list lacks counts as elsewhere, so it is on only when turned on).
    pub async fn is_on(&self, id: &str, model: &str, listed: Option<&[AiModel]>) -> bool {
        let described = listed.and_then(|models| models.iter().find(|known| known.id == model));
        let model = described.cloned().unwrap_or_else(|| AiModel::named(model));
        self.mark_enabled(id, vec![model]).await.first().is_some_and(|model| model.enabled)
    }

    /// Whether the account `id` has at least one model on that can chat (its tools not ruled out: a
    /// model that only decides, like nimble, is on for the mail sorting, never for a chat), what
    /// `assistant.list_engines` lists and
    /// what an answer needs: judged by the account's list as the core last held it, where it holds
    /// one (a model turned on that the server no longer offers does not count). Without one, a model
    /// turned on by id counts, and so does an account still to take over from "Automatic"
    /// ([`Switches::automatic`]); the server is asked only where the rule could turn a model on: at
    /// an account where nothing was turned off yet, whose models could compute on this computer
    /// ([`AiAccounts::known_models`]). A list that fails then counts as "yes": the answer reports the
    /// server's problem, not the switches. `false` for an id that is no AI account.
    pub async fn offers_a_model(&self, id: &str) -> bool {
        self.kept().await;
        let Some(server) = self.server(id).await else { return false };
        let (follows_rule, turned_on, automatic) = {
            let held = self.switches().await;
            let automatic = held.as_ref().is_some_and(|held| held.automatic.contains_key(id));
            let switches = held.as_ref().map(|held| &held.held);
            (switches.is_none_or(|s| s.follows_rule(id)), switches.is_some_and(|s| s.has_turned_on(id)), automatic)
        };
        // Models turned on by id count while the core has no list to check them against; with one,
        // only those the server still offers. So does a model "Automatic" took before the switches
        // came (#231), still to be kept on: nobody loses the assistant while its server is away.
        let listed =
            self.slots.read().await.iter().find(|slot| slot.account.id == id).and_then(|slot| slot.models.clone());
        if (turned_on || automatic) && listed.is_none() {
            return true;
        }
        if let Some(listed) = listed {
            return self.mark_enabled(id, listed).await.iter().any(chats);
        }
        if !follows_rule || !may_compute_here(Some(&server)) {
            return false;
        }
        match self.known_models(id, None).await {
            Ok(models) => self.mark_enabled(id, models).await.iter().any(chats),
            Err(_) => true,
        }
    }

    /// Whether turning on `model` of the account `id` would let something compute elsewhere, what
    /// only a person may do (`docs/api.md`, "Was nie nach außen geht", 6): judged by the account's
    /// list as the core holds it (asked now when it lacks the model), else by its id alone, so in
    /// doubt it counts as elsewhere for Ollama. `Ok(false)` for an id that is no AI account: the
    /// command then answers `not-found`. The list's error when the core holds no list and cannot
    /// load one (#231): then it cannot say, and the caller answers with the server's problem rather
    /// than a refusal – safe, since nothing is turned on without the list.
    pub async fn runs_elsewhere(&self, id: &str, model: &str) -> Result<bool, AiError> {
        let Some(server) = self.server(id).await else { return Ok(false) };
        let listed = self.known_models(id, Some(model)).await?;
        let described = listed.into_iter().find(|known| known.id == model);
        Ok(!computes_here(Some(&server), &described.unwrap_or_else(|| AiModel::named(model))))
    }

    /// The name of the model `model` of the account `id` and whether it computes on this computer,
    /// as the account's list in the core says (asked now when it lacks the model), for an approval's
    /// summary; `None` for a model the account does not list. Never the client's words.
    pub async fn described(&self, id: &str, model: &str) -> Option<(String, bool)> {
        let server = self.server(id).await?;
        let models = self.known_models(id, Some(model)).await.ok()?;
        let described = models.into_iter().find(|known| known.id == model)?;
        let here = computes_here(Some(&server), &described);
        Some((described.name, here))
    }

    /// `ai.set_model_enabled`: turns `model` of the account `id` on or off and stores it. Turning on
    /// asks the server for its models first ([`AiAccounts::models`]) and takes only one of them
    /// ([`AiError::NotFound`] otherwise); turning off takes any id. The first time something is turned
    /// off at an account whose models could compute here, the models the rule has on are kept on
    /// ([`ModelSwitches::turn_off`]), so the list is needed then ([`AiAccounts::known_models`]).
    /// [`AiError::NotFound`] for an id that is no AI account, [`AiError::Format`] when the file
    /// cannot be written (nothing changes then) or is there but did not read (PR review #242: it is
    /// never written over, [`Switches::writable`]). An account still to take over from "Automatic"
    /// ([`Switches::automatic`], #231) is not any more once the switch is stored: the person's
    /// switch wins; a switch that fails is none, and a list it loaded takes over as any first list.
    pub async fn set_enabled(&self, id: &str, model: &str, enabled: bool) -> Result<(), AiError> {
        // Taken out while switching, so the list asked for below takes nothing over; the switch
        // stores the account without it.
        let noted = self.switches().await.as_mut().and_then(|held| held.automatic.remove(id));
        let switched = self.switch(id, model, enabled).await;
        if switched.is_err()
            && let Some(noted) = noted
        {
            if let Some(held) = self.switches().await.as_mut() {
                held.automatic.entry(id.to_string()).or_insert(noted);
            }
            // A list loaded on the way: taken over now, as at any first list.
            let listed =
                self.slots.read().await.iter().find(|slot| slot.account.id == id).and_then(|slot| slot.models.clone());
            if let Some(models) = listed {
                self.take_over(id, &models).await;
            }
        }
        switched
    }

    /// `Ok` while the switches may be written; [`AiError::Format`] with the sentence that names
    /// `ai-models.json` while it is there but did not read (PR review #242): then every switch is
    /// refused. The core asks it before anything else of `ai.set_model_enabled` – before a server
    /// is asked and before an approval –, so a client and the person get the same answer.
    pub async fn switches_writable(&self) -> Result<(), AiError> {
        self.switches().await.as_ref().map_or(Ok(()), Switches::writable).map_err(AiError::Format)
    }

    /// [`AiAccounts::set_enabled`] apart from what "Automatic" used.
    async fn switch(&self, id: &str, model: &str, enabled: bool) -> Result<(), AiError> {
        let server = self.server(id).await.ok_or_else(|| no_account(id))?;
        // Before any server is asked: a file that did not read refuses every switch alike.
        self.switches_writable().await?;
        let on_by_rule: Vec<String> = if enabled {
            let models = self.models(id, true).await?;
            if !models.iter().any(|known| known.id == model) {
                let provider = self.label_of(id).await.unwrap_or_else(|| id.to_string());
                return Err(AiError::NotFound(trf(
                    "{provider} does not offer the model {model}.",
                    &[("provider", &provider), ("model", &model)],
                )));
            }
            Vec::new()
        } else if may_compute_here(Some(&server))
            && self.switches().await.as_ref().is_none_or(|held| held.held.follows_rule(id))
        {
            let models = self.known_models(id, None).await?;
            models
                .iter()
                .filter(|known| ModelSwitches::starts_on(known, computes_here(Some(&server), known)))
                .map(|known| known.id.clone())
                .collect()
        } else {
            Vec::new()
        };
        let mut held = self.switches().await;
        let switches = held.get_or_insert_with(Switches::default);
        let mut changed = switches.clone();
        match enabled {
            true => changed.held.turn_on(id, model),
            false => changed.held.turn_off(id, model, on_by_rule),
        }
        self.store_switches(&changed).await.map_err(AiError::Format)?;
        *switches = changed;
        Ok(())
    }

    /// The first start after the update that brought the switches (task S.24): when there was no
    /// `ai-models.json` at the core's first look ([`Switches::keep_used`]), the models the suite
    /// already uses – `used` as pairs of account id and model id (the assistant's default for new
    /// chats, the models of the bots and of the functions) – are turned on, so nothing that worked
    /// stops, and the file is written with the mark gone. Afterwards it does nothing, and a model a
    /// module chooses later turns nothing on. Ids that are no AI account (`ai:…`) are left out, and
    /// so are accounts a person switched a model at already (#231): their switch wins. Lets the
    /// readers of the switches go ([`AiAccounts::hold_until_kept`]) whatever came of it.
    ///
    /// `automatic` (#231): the accounts used with "Automatic" (no model named) and what for. The
    /// model it took is in the server's list only, so they are noted ([`Switches::automatic`]) and
    /// taken over at the first list that loads ([`AiAccounts::take_over`]); no server is asked here.
    pub async fn keep_used_models_on(
        &self,
        used: Vec<(String, String)>,
        automatic: Vec<(String, Automatic)>,
    ) -> Result<(), String> {
        let kept = self.keep_used(used, automatic).await;
        self.kept.send_replace(true);
        kept
    }

    /// [`AiAccounts::keep_used_models_on`] without letting the readers go.
    async fn keep_used(&self, used: Vec<(String, String)>, automatic: Vec<(String, Automatic)>) -> Result<(), String> {
        let mut held = self.switches().await;
        let switches = held.get_or_insert_with(Switches::default);
        if !switches.keep_used {
            return Ok(());
        }
        // While the mark stands, every entry is a person's: there was no file before.
        let switched = switches.held.accounts.keys().cloned().collect::<std::collections::BTreeSet<_>>();
        let mut seeded = switches.clone();
        for (account, model) in used.iter().filter(|(account, _)| account.starts_with("ai:")) {
            if !switched.contains(account) {
                seeded.held.turn_on(account, model);
            }
        }
        for (account, uses) in automatic.into_iter().filter(|(account, _)| account.starts_with("ai:")) {
            if !switched.contains(&account) {
                let noted = seeded.automatic.entry(account).or_default();
                *noted = noted.and(uses);
            }
        }
        seeded.keep_used = false;
        self.store_switches(&seeded).await?;
        *switches = seeded;
        Ok(())
    }

    /// Forgets the switches of the removed account `id`, so it starts with the rule when connected
    /// again. A file that cannot be written is only reported: the account is gone either way.
    async fn forget_switches(&self, id: &str) {
        let mut held = self.switches().await;
        let switches = held.get_or_insert_with(Switches::default);
        if !switches.held.accounts.contains_key(id) && !switches.automatic.contains_key(id) {
            return;
        }
        let mut changed = switches.clone();
        changed.held.forget(id);
        changed.automatic.remove(id);
        match self.store_switches(&changed).await {
            Ok(()) => *switches = changed,
            Err(err) => eprintln!("torroworkplace: cannot forget the models of a removed AI account: {err}"),
        }
    }

    fn http(&self) -> Result<&reqwest::Client, AiError> {
        self.http.as_ref().ok_or_else(|| AiError::Format(tr("No HTTP client to ask AI servers with.").into()))
    }
}

/// [`AiError::NotFound`] for the id `id` that is no AI account.
pub(crate) fn no_account(id: &str) -> AiError {
    AiError::NotFound(torro_core::i18n::trf("There is no AI account {id}.", &[("id", &id)]))
}

/// Whether `model` is on and may answer a chat: its tools are not ruled out (#231). A model that
/// only decides is on for the mail sorting and counts for nothing else.
fn chats(model: &AiModel) -> bool {
    model.enabled && model.tools != Some(false)
}

#[cfg(test)]
pub(crate) mod tests {
    use httpmock::MockServer;
    use serde_json::json;
    use torro_core::ai::{ChatMessage, Effort, Finish, ToolCall, ToolSpec, Usage};

    use super::*;
    use crate::account::github::tests::{MemoryTokens, accounts_file};

    pub(crate) const KEY: &str = "sk-or-v1-0123456789SECRETabcdef";

    /// Like [`http_client`], without the system's proxy, so a proxy set on the test machine never
    /// sees the fake servers.
    pub(crate) fn http() -> reqwest::Client {
        reqwest::Client::builder().no_proxy().redirect(reqwest::redirect::Policy::none()).build().expect("http client")
    }

    /// The service on `dir/config/accounts.json` and `keys`, looking for local servers at `local`.
    pub(crate) fn service(dir: &std::path::Path, keys: &MemoryTokens, local: Vec<(AiProvider, String)>) -> AiAccounts {
        AiAccounts::new(Some(accounts_file(dir)), Arc::new(keys.clone()), Some(http()), local)
    }

    /// An address where nothing answers.
    pub(crate) const NOBODY: &str = "http://127.0.0.1:9/v1";

    /// An Ollama with two models, one of which calls tools, and a third it cannot describe.
    pub(crate) async fn fake_ollama() -> MockServer {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/v1/models").header_missing("authorization");
                then.status(200).json_body(json!({"object": "list", "data": [
                    {"id": "qwen3:8b", "object": "model", "owned_by": "library"},
                    {"id": "gemma3:4b", "object": "model", "owned_by": "library"},
                    {"id": "broken:1b", "object": "model", "owned_by": "library"},
                ]}));
            })
            .await;
        for (model, capabilities) in
            [("qwen3:8b", json!(["completion", "tools", "thinking"])), ("gemma3:4b", json!(["completion"]))]
        {
            server
                .mock_async(|when, then| {
                    when.method("POST").path("/api/show").json_body_includes(json!({"model": model}).to_string());
                    then.status(200).json_body(json!({"capabilities": capabilities}));
                })
                .await;
        }
        server
            .mock_async(|when, then| {
                when.method("POST").path("/api/show").json_body_includes(r#"{"model":"broken:1b"}"#);
                then.status(500).json_body(json!({"error": "cannot read the model"}));
            })
            .await;
        // Every model runs on this computer (task S.23).
        server
            .mock_async(|when, then| {
                when.method("GET").path("/api/tags");
                then.status(200).json_body(json!({"models": [
                    {"name": "qwen3:8b", "model": "qwen3:8b"},
                    {"name": "gemma3:4b", "model": "gemma3:4b"},
                    {"name": "broken:1b", "model": "broken:1b"},
                ]}));
            })
            .await;
        server
    }

    /// Task E12.3: an Ollama that offers a decision model and answers one call at
    /// `/v1/systemone`.
    pub(crate) async fn fake_deciding_ollama() -> MockServer {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/v1/models");
                then.status(200).json_body(json!({"object": "list", "data": [
                    {"id": "nimble", "object": "model", "owned_by": "library"},
                    {"id": "qwen3:8b", "object": "model", "owned_by": "library"},
                ]}));
            })
            .await;
        for (model, capabilities) in
            [("nimble", json!(["completion", "decision"])), ("qwen3:8b", json!(["completion", "tools"]))]
        {
            server
                .mock_async(|when, then| {
                    when.method("POST").path("/api/show").json_body_includes(json!({"model": model}).to_string());
                    then.status(200).json_body(json!({"capabilities": capabilities}));
                })
                .await;
        }
        server
            .mock_async(|when, then| {
                when.method("GET").path("/api/tags");
                then.status(200).json_body(json!({"models": [{"name": "nimble", "model": "nimble"}]}));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method("POST").path("/v1/systemone").json_body_includes(r#"{"model":"nimble"}"#);
                then.status(200).json_body(json!({
                    "model": "nimble",
                    "answers": {
                        "category": {"type": "choice", "choice": "invoice",
                                     "probabilities": {"invoice": 0.91, "other": 0.09}, "confidence": 0.91},
                        "needs_reply": {"type": "noul", "noul": 0.2},
                        "urgency": {"type": "score", "score": 0.7, "confidence": 0.8}
                    },
                    "usage": {"input_tokens": 120, "output_tokens": 12}
                }));
            })
            .await;
        server
    }

    /// An Ollama with a cloud model first and a local one (task S.23), both calling tools; with
    /// `tags` it says in `/api/tags` which runs elsewhere, without it does not answer there.
    pub(crate) async fn fake_ollama_with_cloud(tags: bool) -> MockServer {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/v1/models");
                then.status(200).json_body(json!({"object": "list", "data": [
                    {"id": "glm-5.3:cloud", "object": "model", "owned_by": "library"},
                    {"id": "qwen3:8b", "object": "model", "owned_by": "library"},
                ]}));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method("POST").path("/api/show");
                then.status(200).json_body(json!({"capabilities": ["completion", "tools"]}));
            })
            .await;
        if tags {
            server
                .mock_async(|when, then| {
                    when.method("GET").path("/api/tags");
                    then.status(200).json_body(json!({"models": [
                        {"name": "glm-5.3:cloud", "model": "glm-5.3:cloud", "remote_model": "glm-5.3",
                         "remote_host": "https://ollama.com:443", "size": 384},
                        {"name": "qwen3:8b", "model": "qwen3:8b", "size": 5_225_388_164_u64},
                    ]}));
                })
                .await;
        }
        server
    }

    /// OpenRouter: `GET /api/v1/key` takes only [`KEY`]; its models, one open, two closed.
    pub(crate) async fn fake_openrouter() -> MockServer {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/api/v1/key").header("authorization", format!("Bearer {KEY}"));
                then.status(200).json_body(json!({"data": {"label": "sk-or-v1-012…def", "limit": null}}));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/api/v1/key");
                // Some servers repeat the key they refuse.
                then.status(401).json_body(json!({"error": {"message": format!("Invalid key {KEY}"), "code": 401}}));
            })
            .await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/api/v1/models");
                then.status(200).json_body(json!({"data": [
                    {"id": "qwen/qwen3-14b", "name": "Qwen: Qwen3 14B", "context_length": 40960,
                     "hugging_face_id": "Qwen/Qwen3-14B", "supported_parameters": ["tools", "temperature"]},
                    {"id": "openai/gpt-4o", "name": "OpenAI: GPT-4o", "hugging_face_id": null,
                     "supported_parameters": ["tools"]},
                    {"id": "google/gemini-2.5-flash", "name": "Gemini 2.5 Flash", "hugging_face_id": "",
                     "supported_parameters": ["tools"]},
                ]}));
            })
            .await;
        server
    }

    fn texts(models: &[AiModel]) -> Vec<(&str, Option<bool>, Option<bool>)> {
        models.iter().map(|m| (m.id.as_str(), m.tools, m.open_weights)).collect()
    }

    /// Task E12.3: Ollama says which models decide, a decision goes to `/v1/systemone`, and no
    /// other provider takes one.
    #[tokio::test]
    async fn ollama_says_which_models_decide_and_answers_a_decision() {
        use std::collections::BTreeMap;
        use torro_core::ai::systemone::{DecisionRequest, Question};

        let server = fake_deciding_ollama().await;
        let dir = tempfile::tempdir().expect("a directory");
        let keys = MemoryTokens::default();
        let base = format!("{}/v1", server.base_url());
        let accounts = service(dir.path(), &keys, Vec::new());
        accounts.connect(AiProvider::Ollama, Some(&base), Secret::new("")).await.expect("connected");
        let id = format!("ai:ollama@{}", server.address());

        let models = accounts.models(&id, true).await.expect("models");
        let decides: Vec<(String, Option<bool>)> =
            models.iter().map(|model| (model.id.clone(), model.decision)).collect();
        assert_eq!(decides, [("nimble".to_string(), Some(true)), ("qwen3:8b".to_string(), Some(false))]);

        let mut questions = BTreeMap::new();
        questions.insert(
            "category".to_string(),
            Question::Choice {
                instructions: "What is this mail?".into(),
                criteria: vec![("invoice".into(), "A bill".into()), ("other".into(), "Anything else".into())],
            },
        );
        questions.insert("needs_reply".to_string(), Question::Noul { instructions: "Is an answer waited for?".into() });
        let asked = DecisionRequest { model: "nimble".into(), state: "Your invoice".into(), questions };
        let client = accounts.client(&id).await.expect("a client");
        let reply = client.decide(&asked).await.expect("a decision");
        let (choice, confidence) = reply.choice("category").expect("a choice");
        assert_eq!(choice, "invoice");
        assert!((confidence - 0.91).abs() < 0.001);
        assert!((reply.noul("needs_reply").expect("a noul") - 0.2).abs() < 0.001);
        assert_eq!(reply.usage.expect("counted").prompt_tokens, 120);
    }

    #[tokio::test]
    async fn ollama_is_connected_without_a_key_and_lists_what_each_model_can_do() {
        let dir = tempfile::tempdir().expect("temp dir");
        let keys = MemoryTokens::default();
        let ollama = fake_ollama().await;
        let ai = service(dir.path(), &keys, Vec::new());

        let info =
            ai.connect(AiProvider::Ollama, Some(&ollama.url("/v1")), Secret::default()).await.expect("connected");
        let host = format!("127.0.0.1:{}", ollama.port());
        assert_eq!((info.id.as_str(), info.kind.as_str()), (format!("ai:ollama@{host}").as_str(), "ai"));
        assert_eq!(info.label, format!("Ollama ({host})"));
        assert!(matches!(&info.state, AccountState::Connected { server, problem: None, last_sync: Some(_), .. }
            if *server == ollama.url("/v1")));
        assert_eq!(keys.get_plain(&info.id), None, "no key, nothing in the keychain");

        let models = ai.models(&info.id, false).await.expect("models");
        assert_eq!(
            texts(&models),
            [("qwen3:8b", Some(true), None), ("gemma3:4b", Some(false), None), ("broken:1b", None, None)],
            "the filter keeps every local model"
        );
        let reasoning: Vec<Option<bool>> = models.iter().map(|model| model.reasoning).collect();
        assert_eq!(reasoning, [Some(true), Some(false), None], "issue #131: from the same answer");
        assert!(models.iter().all(|model| model.remote == Some(false)), "task S.23: all on this computer");
        let listed: Vec<String> = ai.infos().await.into_iter().map(|listed| listed.id).collect();
        assert_eq!(listed, [info.id]);
    }

    /// Task S.23: a cloud model of Ollama runs elsewhere, a local one does not; without `/api/tags`
    /// every model counts as elsewhere. The list is kept, so the data rule does not ask again.
    #[tokio::test]
    async fn ollama_says_which_models_run_elsewhere_and_without_tags_all_do() {
        let dir = tempfile::tempdir().expect("temp dir");
        let keys = MemoryTokens::default();
        let ai = service(dir.path(), &keys, Vec::new());
        let remote = |models: &[AiModel]| -> Vec<(String, Option<bool>)> {
            models.iter().map(|model| (model.id.clone(), model.remote)).collect()
        };

        let ollama = fake_ollama_with_cloud(true).await;
        let info = ai.connect(AiProvider::Ollama, Some(&ollama.url("/v1")), Secret::default()).await.expect("ok");
        let models = ai.models(&info.id, false).await.expect("models");
        assert_eq!(remote(&models), [("glm-5.3:cloud".into(), Some(true)), ("qwen3:8b".into(), Some(false))]);
        assert_eq!(serde_json::to_value(&models[0]).expect("json")["remote"], true, "ai.list_models shows it");

        // Kept: asked again only for a model the list lacks.
        ollama.reset_async().await;
        let known = ai.known_models(&info.id, Some("qwen3:8b")).await.expect("kept");
        assert_eq!(remote(&known), remote(&models));
        assert!(ai.known_models(&info.id, Some("llama3:8b")).await.is_err(), "asked anew, and gone");

        let without = fake_ollama_with_cloud(false).await;
        let info = ai.connect(AiProvider::Ollama, Some(&without.url("/v1")), Secret::default()).await.expect("ok");
        let known = ai.known_models(&info.id, None).await.expect("listed now");
        assert_eq!(remote(&known), [("glm-5.3:cloud".into(), Some(true)), ("qwen3:8b".into(), Some(true))]);
    }

    #[tokio::test]
    async fn openrouter_keeps_its_key_in_the_keychain_and_lists_open_models_unless_all() {
        let dir = tempfile::tempdir().expect("temp dir");
        let keys = MemoryTokens::default();
        let openrouter = fake_openrouter().await;
        let ai = service(dir.path(), &keys, Vec::new());

        let info = ai
            .connect(AiProvider::OpenRouter, Some(&openrouter.url("/api/v1")), Secret::new(format!(" {KEY}\n")))
            .await
            .expect("connected");
        assert_eq!(info.label, format!("OpenRouter (127.0.0.1:{})", openrouter.port()));
        assert_eq!(keys.get_plain(&info.id).as_deref(), Some(KEY), "trimmed, in the keychain");
        let stored = std::fs::read_to_string(accounts_file(dir.path())).expect("stored");
        assert!(stored.contains("\"openrouter\"") && !stored.contains("SECRET"), "{stored}");
        for shown in
            [format!("{info:?}"), serde_json::to_string(&info).expect("json"), format!("{:?}", ai.infos().await)]
        {
            assert!(!shown.contains("SECRET"), "{shown}");
        }

        let open = ai.models(&info.id, false).await.expect("models");
        assert_eq!(texts(&open), [("qwen/qwen3-14b", Some(true), Some(true))]);
        assert_eq!(open[0].context, Some(40960));
        let all = ai.models(&info.id, true).await.expect("models");
        assert_eq!(all.len(), 3, "closed ones too");
    }

    /// Task W.3: a cloud provider of its own is checked by its model list with the key, and named
    /// by the provider.
    #[tokio::test]
    async fn a_cloud_provider_is_checked_by_its_models_with_the_key() {
        let dir = tempfile::tempdir().expect("temp dir");
        let keys = MemoryTokens::default();
        let mistral = MockServer::start_async().await;
        mistral
            .mock_async(|when, then| {
                when.method("GET").path("/v1/models").header("authorization", format!("Bearer {KEY}"));
                then.status(200).json_body(json!({"object": "list", "data": [{"id": "mistral-small-latest"}]}));
            })
            .await;
        mistral
            .mock_async(|when, then| {
                when.method("GET").path("/v1/models");
                then.status(401).json_body(json!({"message": "Unauthorized"}));
            })
            .await;
        let ai = service(dir.path(), &keys, Vec::new());
        let refused = ai.connect(AiProvider::Mistral, Some(&mistral.url("/v1")), Secret::new("wrong")).await;
        assert_eq!(refused, Err(AiError::Auth));
        let info =
            ai.connect(AiProvider::Mistral, Some(&mistral.url("/v1")), Secret::new(KEY)).await.expect("connected");
        assert_eq!(info.label, format!("Mistral (127.0.0.1:{})", mistral.port()));
        let keyless = ai.connect(AiProvider::DeepSeek, None, Secret::default()).await;
        assert!(matches!(keyless, Err(AiError::Invalid(ref why)) if why.contains("DeepSeek")), "{keyless:?}");
    }

    #[tokio::test]
    async fn a_refused_key_is_auth_stores_nothing_and_never_shows_the_key() {
        let dir = tempfile::tempdir().expect("temp dir");
        let keys = MemoryTokens::default();
        let openrouter = fake_openrouter().await;
        let ai = service(dir.path(), &keys, Vec::new());
        let wrong = "sk-or-v1-WRONGWRONGWRONG";

        let refused = ai.connect(AiProvider::OpenRouter, Some(&openrouter.url("/api/v1")), Secret::new(wrong)).await;
        assert_eq!(refused, Err(AiError::Auth));
        assert!(!format!("{refused:?} {}", AiError::Auth).contains("WRONG"));
        assert!(ai.infos().await.is_empty() && !accounts_file(dir.path()).exists());
        assert_eq!(keys.get_plain(&Account::ai(AiProvider::OpenRouter, &openrouter.url("/api/v1")).id), None);
    }

    #[tokio::test]
    async fn keys_never_travel_unencrypted_and_openrouter_needs_one() {
        let dir = tempfile::tempdir().expect("temp dir");
        let keys = MemoryTokens::default();
        let ai = service(dir.path(), &keys, Vec::new());
        // Refused before anything is sent: there is no server at either address.
        let plain =
            ai.connect(AiProvider::OpenAiCompatible, Some("http://gpu.example.org:8000"), Secret::new(KEY)).await;
        assert!(matches!(plain, Err(AiError::Invalid(_))), "{plain:?}");
        let keyless = ai.connect(AiProvider::OpenRouter, None, Secret::new("  ")).await;
        assert!(matches!(keyless, Err(AiError::Invalid(_))), "{keyless:?}");
        let nowhere = ai.connect(AiProvider::OpenAiCompatible, None, Secret::default()).await;
        assert!(matches!(nowhere, Err(AiError::Invalid(_))), "{nowhere:?}");
        let offline = ai.connect(AiProvider::OpenAiCompatible, Some(NOBODY), Secret::default()).await;
        assert_eq!(offline, Err(AiError::Offline));
        assert!(ai.infos().await.is_empty());
    }

    /// A stream as Ollama sends a tool call: the whole call in one chunk, with `index`.
    const OLLAMA_TOOL_CALL: &str = "data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"\",\"tool_calls\":[{\"id\":\"call_x1\",\"index\":0,\"type\":\"function\",\"function\":{\"name\":\"calendar_list_events\",\"arguments\":\"{\\\"start\\\":\\\"2026-10-13\\\"}\"}}]},\"finish_reason\":null}]}\n\n\
        data: {\"id\":\"chatcmpl-1\",\"object\":\"chat.completion.chunk\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"\"},\"finish_reason\":\"tool_calls\"}]}\n\n\
        data: [DONE]\n\n";

    fn ask_calendar() -> ChatRequest {
        ChatRequest {
            model: "qwen3:8b".into(),
            messages: vec![ChatMessage::system("You help with the calendar."), ChatMessage::user("What is on today?")],
            tools: vec![ToolSpec {
                name: "calendar_list_events".into(),
                description: "Lists events between start and end.".into(),
                parameters: json!({"type": "object", "properties": {"start": {"type": "string"}}}),
            }],
            max_tokens: None,
            effort: None,
        }
    }

    /// Issue #131: the effort reaches each server where it reads it.
    #[tokio::test]
    async fn the_effort_is_sent_as_each_provider_reads_it() {
        let server = MockServer::start_async().await;
        let ollama = server
            .mock_async(|when, then| {
                when.method("POST").path("/v1/chat/completions").json_body_includes(r#"{"reasoning_effort":"low"}"#);
                then.status(200).header("content-type", "text/event-stream").body(OLLAMA_TOOL_CALL);
            })
            .await;
        let openrouter = server
            .mock_async(|when, then| {
                when.method("POST")
                    .path("/api/v1/chat/completions")
                    .json_body_includes(r#"{"reasoning":{"effort":"high"}}"#);
                then.status(200).header("content-type", "text/event-stream").body(OLLAMA_TOOL_CALL);
            })
            .await;
        let low = ChatRequest { effort: Some(Effort::Low), ..ask_calendar() };
        let local = OpenAiCompatible::new(http(), AiProvider::Ollama, &server.url("/v1"), None);
        local.chat(&low, &mut |_| {}).await.expect("ollama answers");
        let high = ChatRequest { effort: Some(Effort::High), ..ask_calendar() };
        let remote =
            OpenAiCompatible::new(http(), AiProvider::OpenRouter, &server.url("/api/v1"), Some(Secret::new(KEY)));
        remote.chat(&high, &mut |_| {}).await.expect("openrouter answers");
        assert_eq!((ollama.calls_async().await, openrouter.calls_async().await), (1, 1));
    }

    #[tokio::test]
    async fn openai_uses_responses_for_tool_rounds_with_reasoning() {
        let server = MockServer::start_async().await;
        let first = server
            .mock_async(|when, then| {
                when.method("POST")
                    .path("/v1/responses")
                    .header("authorization", format!("Bearer {KEY}"))
                    .json_body_includes(r#"{"model":"gpt-6.1-sol","reasoning":{"effort":"high"},"store":false}"#)
                    .body_includes("\"tasks_list\"");
                then.status(200).header("content-type", "text/event-stream").body(format!(
                    "data: {}\n\ndata: [DONE]\n\n",
                    json!({
                        "type":"response.completed",
                        "response":{
                            "status":"completed",
                            "output":[
                                {"type":"reasoning","id":"rs_1","summary":[],"encrypted_content":"opaque"},
                                {"type":"function_call","id":"fc_1","call_id":"call_1","name":"tasks_list","arguments":"{}"}
                            ]
                        }
                    })
                ));
            })
            .await;
        let openai = OpenAiCompatible::new(http(), AiProvider::OpenAi, &server.url("/v1"), Some(Secret::new(KEY)));
        let mut request = ChatRequest { model: "gpt-6.1-sol".into(), effort: Some(Effort::High), ..ask_calendar() };
        request.tools[0].name = "tasks_list".into();
        let reply = openai.chat(&request, &mut |_| {}).await.expect("first tool call");
        assert_eq!(first.calls_async().await, 1);
        first.delete_async().await;
        let next = server
            .mock_async(|when, then| {
                when.method("POST")
                    .path("/v1/responses")
                    .body_includes("\"function_call_output\"")
                    .body_includes("\"encrypted_content\"");
                then.status(200).json_body(json!({
                    "status":"completed",
                    "output":[{"type":"message","role":"assistant","content":[
                        {"type":"output_text","text":"Moved 13 tasks."}
                    ]}]
                }));
            })
            .await;
        request
            .messages
            .push(ChatMessage::assistant(reply.text, reply.tool_calls.clone()).with_thinking(reply.thinking));
        request.messages.push(ChatMessage::tool(reply.tool_calls[0].id.clone(), "13 tasks"));
        let mut pieces = Vec::new();
        let reply = openai.chat(&request, &mut |text| pieces.push(text.to_string())).await.expect("final answer");
        assert_eq!((reply.text.as_str(), pieces), ("Moved 13 tasks.", vec!["Moved 13 tasks.".to_string()]));
        assert_eq!(next.calls_async().await, 1);
    }

    #[tokio::test]
    async fn openai_retries_without_effort_when_a_model_refuses_it() {
        let server = MockServer::start_async().await;
        let refused = server
            .mock_async(|when, then| {
                when.method("POST").path("/v1/responses").body_includes("\"reasoning\"");
                then.status(400).json_body(json!({"error":{"message":"This model does not support reasoning.effort"}}));
            })
            .await;
        let accepted = server
            .mock_async(|when, then| {
                when.method("POST").path("/v1/responses").body_excludes("\"reasoning\"").body_includes("\"tools\"");
                then.status(200).json_body(json!({
                    "status":"completed",
                    "output":[{"type":"message","role":"assistant","content":[
                        {"type":"output_text","text":"Done."}
                    ]}]
                }));
            })
            .await;
        let openai = OpenAiCompatible::new(http(), AiProvider::OpenAi, &server.url("/v1"), Some(Secret::new(KEY)));
        let request = ChatRequest { model: "gpt-4o".into(), effort: Some(Effort::High), ..ask_calendar() };
        let reply = openai.chat(&request, &mut |_| {}).await.expect("retry answers");
        assert_eq!(reply.text, "Done.");
        assert_eq!((refused.calls_async().await, accepted.calls_async().await), (1, 1));
    }

    #[tokio::test]
    async fn ollama_streams_a_tool_call() {
        let server = MockServer::start_async().await;
        let chat = server
            .mock_async(|when, then| {
                when.method("POST")
                    .path("/v1/chat/completions")
                    .json_body_includes(r#"{"stream":true,"model":"qwen3:8b"}"#)
                    .body_includes("\"tools\"")
                    .body_includes("calendar_list_events");
                then.status(200).header("content-type", "text/event-stream").body(OLLAMA_TOOL_CALL);
            })
            .await;
        let ollama = OpenAiCompatible::new(http(), AiProvider::Ollama, &server.url("/v1"), None);
        let mut pieces = Vec::new();
        let reply = ollama.chat(&ask_calendar(), &mut |text| pieces.push(text.to_string())).await.expect("reply");
        assert_eq!(chat.calls_async().await, 1);
        assert_eq!(
            reply.tool_calls,
            [ToolCall {
                id: "call_x1".into(),
                name: "calendar_list_events".into(),
                arguments: r#"{"start":"2026-10-13"}"#.into()
            }]
        );
        assert_eq!((reply.finish, reply.text.as_str()), (Finish::ToolCalls, ""));
        assert!(pieces.is_empty(), "no text, no pieces");
    }

    #[tokio::test]
    async fn openrouter_streams_text_in_pieces_with_the_key_and_counts_tokens() {
        let server = MockServer::start_async().await;
        let stream = ": OPENROUTER PROCESSING\n\n\
            data: {\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"Two \"}}]}\n\n\
            data: {\"choices\":[{\"delta\":{\"content\":\"meetings.\"}}]}\n\n\
            data: {\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":41,\"completion_tokens\":3}}\n\n\
            data: [DONE]\n\n";
        server
            .mock_async(|when, then| {
                when.method("POST").path("/api/v1/chat/completions").header("authorization", format!("Bearer {KEY}"));
                then.status(200).header("content-type", "text/event-stream; charset=utf-8").body(stream);
            })
            .await;
        let openrouter =
            OpenAiCompatible::new(http(), AiProvider::OpenRouter, &server.url("/api/v1/"), Some(Secret::new(KEY)));
        assert!(!format!("{openrouter:?}").contains("SECRET"));
        let mut pieces = Vec::new();
        let request = ChatRequest { model: "qwen/qwen3-14b".into(), ..ask_calendar() };
        let reply = openrouter.chat(&request, &mut |text| pieces.push(text.to_string())).await.expect("reply");
        assert_eq!(pieces, ["Two ", "meetings."]);
        assert_eq!((reply.text.as_str(), reply.finish), ("Two meetings.", Finish::Stop));
        assert_eq!(reply.usage, Some(Usage { prompt_tokens: 41, completion_tokens: 3 }));
    }

    #[tokio::test]
    async fn a_server_that_answers_whole_still_hands_out_its_text() {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method("POST").path("/v1/chat/completions");
                then.status(200)
                    .json_body(json!({"choices": [{"message": {"role": "assistant", "content": "Nothing today."},
                    "finish_reason": "stop"}]}));
            })
            .await;
        let lm_studio = OpenAiCompatible::new(http(), AiProvider::LmStudio, &server.url("/v1"), None);
        let mut pieces = Vec::new();
        let reply = lm_studio.chat(&ask_calendar(), &mut |text| pieces.push(text.to_string())).await.expect("reply");
        assert_eq!((pieces, reply.text), (vec!["Nothing today.".to_string()], "Nothing today.".to_string()));
    }

    #[tokio::test]
    async fn chat_errors_have_their_codes_and_never_repeat_the_key() {
        let server = MockServer::start_async().await;
        let answers = [
            ("missing", 404, json!({"error": "model \"missing\" not found, try pulling it first"})),
            ("busy", 429, json!({"error": {"message": "Rate limit exceeded", "code": 429}})),
            ("leaky", 400, json!({"error": {"message": format!("Bad request with key {KEY}")}})),
            ("refused", 401, json!({"error": {"message": format!("Invalid key {KEY}")}})),
        ];
        for (model, status, body) in &answers {
            server
                .mock_async(|when, then| {
                    when.method("POST")
                        .path("/v1/chat/completions")
                        .json_body_includes(json!({"model": model}).to_string());
                    then.status(*status).json_body(body.clone());
                })
                .await;
        }
        let keyed =
            OpenAiCompatible::new(http(), AiProvider::OpenAiCompatible, &server.url("/v1"), Some(Secret::new(KEY)));
        let ask = |model: &str| ChatRequest { model: model.into(), ..ask_calendar() };
        let mut ignore = |_: &str| {};

        let missing = keyed.chat(&ask("missing"), &mut ignore).await;
        assert!(matches!(&missing, Err(AiError::NotFound(message)) if message.contains("missing")), "{missing:?}");
        assert!(matches!(keyed.chat(&ask("busy"), &mut ignore).await, Err(AiError::Busy(_))));
        let leaky = keyed.chat(&ask("leaky"), &mut ignore).await;
        assert_eq!(leaky, Err(AiError::Server { status: 400, message: String::new() }));
        assert_eq!(keyed.chat(&ask("refused"), &mut ignore).await, Err(AiError::Auth));

        let gone = OpenAiCompatible::new(http(), AiProvider::Ollama, NOBODY, None);
        assert_eq!(gone.chat(&ask("qwen3:8b"), &mut ignore).await, Err(AiError::Offline));
        assert_eq!(gone.models().await, Err(AiError::Offline));
        for error in [missing, leaky] {
            let error = error.expect_err("an error");
            assert!(!format!("{error:?} {error}").contains("SECRET"), "{error:?}");
        }
    }

    /// Anthropic's key for the tests; must look like a real one so `scrub` has something to find.
    pub(crate) const CLAUDE_KEY: &str = "sk-ant-api03-0123456789SECRETabcdef";

    /// A fake Anthropic whose `/v1/models` answers with two models for the right key.
    pub(crate) async fn fake_anthropic() -> MockServer {
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/v1/models").header("x-api-key", CLAUDE_KEY);
                then.status(200).json_body(json!({"data": [
                    {"type": "model", "id": "claude-opus-5", "display_name": "Claude Opus 5",
                     "max_input_tokens": 1_000_000,
                     "capabilities": {"effort": {"supported": true},
                      "thinking": {"supported": true, "types": {"adaptive": {"supported": true}}}}},
                    {"type": "model", "id": "claude-haiku-4-5", "display_name": "Claude Haiku 4.5",
                     "max_input_tokens": 200_000,
                     "capabilities": {"effort": {"supported": false},
                      "thinking": {"supported": true, "types": {"adaptive": {"supported": false}}}}},
                ], "has_more": false}));
            })
            .await;
        // Any other key, or none: the server refuses it as its own would.
        server
            .mock_async(|when, then| {
                when.method("GET").path("/v1/models");
                then.status(401).json_body(json!({"type": "error", "error": {"type": "authentication_error",
                    "message": format!("invalid x-api-key: {CLAUDE_KEY}")}}));
            })
            .await;
        server
    }

    #[tokio::test]
    async fn anthropic_lists_its_models_with_the_key_in_its_own_header() {
        let server = fake_anthropic().await;
        let client = Anthropic::new(http(), &server.url("/v1"), Some(Secret::new(CLAUDE_KEY)));
        assert!(!format!("{client:?}").contains("SECRET"), "Debug must not show the key");
        let models = client.models().await.expect("models");
        assert_eq!(texts(&models), [("claude-opus-5", Some(true), None), ("claude-haiku-4-5", Some(true), None)]);
        assert_eq!(models[0].reasoning, Some(true));
        assert_eq!(models[1].reasoning, Some(false));
        assert_eq!(models[0].context, Some(1_000_000));
        for model in &models {
            assert_eq!(model.open_weights, None, "{}: else Open First drops every Claude model", model.id);
        }
        assert_eq!(client.check().await, Ok(()));
    }

    #[tokio::test]
    async fn an_anthropic_account_is_connected_and_lists_its_models_through_the_core() {
        let dir = tempfile::tempdir().expect("temp dir");
        let keys = MemoryTokens::default();
        let server = fake_anthropic().await;
        let ai = service(dir.path(), &keys, Vec::new());
        let base = server.url("/v1");

        let info = ai
            .connect(AiProvider::Anthropic, Some(&base), Secret::new(CLAUDE_KEY))
            .await
            .expect("the server takes the key");
        assert_eq!(info.id, Account::ai(AiProvider::Anthropic, &base).id);
        assert_eq!(keys.get_plain(&info.id).as_deref(), Some(CLAUDE_KEY), "the key went into the keychain");
        assert!(!serde_json::to_string(&info).expect("json").contains("SECRET"), "never in the answer");

        // Open First keeps them: their weights are unknown, not closed.
        let models = ai.models(&info.id, false).await.expect("models");
        assert_eq!(models.len(), 2, "{models:?}");
        assert_eq!(ai.server(&info.id).await, Some((AiProvider::Anthropic, base)));
    }

    #[tokio::test]
    async fn anthropic_streams_text_in_pieces_and_a_tool_call_whole() {
        let server = MockServer::start_async().await;
        let stream = "event: message_start\n\
            data: {\"type\":\"message_start\",\"message\":{\"usage\":{\"input_tokens\":41,\"output_tokens\":1}}}\n\n\
            event: content_block_start\n\
            data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
            data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Two \"}}\n\n\
            data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"meetings.\"}}\n\n\
            data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_5\",\"name\":\"calendar_list_events\"}}\n\n\
            data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"start\\\":\"}}\n\n\
            data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\"today\\\"}\"}}\n\n\
            data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":9}}\n\n\
            data: {\"type\":\"message_stop\"}\n\n";
        server
            .mock_async(|when, then| {
                when.method("POST")
                    .path("/v1/messages")
                    .header("x-api-key", CLAUDE_KEY)
                    .header("anthropic-version", torro_core::ai::anthropic::VERSION)
                    .header_exists("content-type");
                then.status(200).header("content-type", "text/event-stream; charset=utf-8").body(stream);
            })
            .await;
        let client = Anthropic::new(http(), &server.url("/v1"), Some(Secret::new(CLAUDE_KEY)));
        let request = ChatRequest { model: "claude-opus-5".into(), ..ask_calendar() };
        let mut pieces = Vec::new();
        let reply = client.chat(&request, &mut |text| pieces.push(text.to_string())).await.expect("reply");
        assert_eq!(pieces, ["Two ", "meetings."]);
        assert_eq!((reply.text.as_str(), reply.finish), ("Two meetings.", Finish::ToolCalls));
        assert_eq!(reply.usage, Some(Usage { prompt_tokens: 41, completion_tokens: 9 }));
        assert_eq!(reply.tool_calls.len(), 1);
        assert_eq!(reply.tool_calls[0].name, "calendar_list_events");
        assert_eq!(reply.tool_calls[0].arguments, "{\"start\":\"today\"}");
    }

    #[tokio::test]
    async fn anthropic_never_sends_the_key_in_the_authorization_header() {
        let server = MockServer::start_async().await;
        // Matches only a request that carries no `Authorization` at all.
        server
            .mock_async(|when, then| {
                when.method("POST").path("/v1/messages").header_missing("authorization");
                then.status(200).json_body(json!({"type": "message", "content": [{"type": "text", "text": "Fine."}],
                        "stop_reason": "end_turn", "usage": {"input_tokens": 3, "output_tokens": 1}}));
            })
            .await;
        let client = Anthropic::new(http(), &server.url("/v1"), Some(Secret::new(CLAUDE_KEY)));
        let request = ChatRequest { model: "claude-opus-5".into(), ..ask_calendar() };
        let mut pieces = Vec::new();
        let reply = client.chat(&request, &mut |text| pieces.push(text.to_string())).await.expect("reply");
        assert_eq!(reply.text, "Fine.", "a server that answers whole still hands out its text");
        assert_eq!(pieces, ["Fine."]);
    }

    #[tokio::test]
    async fn anthropics_errors_have_their_codes_and_never_repeat_the_key() {
        let dir = tempfile::tempdir().expect("temp dir");
        let keys = MemoryTokens::default();
        let server = fake_anthropic().await;
        let ai = service(dir.path(), &keys, Vec::new());

        // The fake server answers a wrong key with 401 and repeats it, as Anthropic's own does.
        let wrong = "sk-ant-api03-WRONGWRONGWRONG";
        let refused = ai.connect(AiProvider::Anthropic, Some(&server.url("/v1")), Secret::new(wrong)).await;
        assert_eq!(refused, Err(AiError::Auth));
        assert!(!format!("{refused:?} {}", AiError::Auth).contains("WRONG"), "{refused:?}");
        assert!(ai.infos().await.is_empty() && !accounts_file(dir.path()).exists(), "nothing was stored");

        // A server that repeats the key in another error: the text goes, the error stays.
        let loud = MockServer::start_async().await;
        loud.mock_async(|when, then| {
            when.method("GET").path("/v1/models");
            then.status(400).json_body(json!({"type": "error", "error": {"type": "invalid_request_error",
                "message": format!("your key {CLAUDE_KEY} is malformed")}}));
        })
        .await;
        let talkative = Anthropic::new(http(), &loud.url("/v1"), Some(Secret::new(CLAUDE_KEY)));
        let failed = talkative.list().await;
        assert!(matches!(&failed, Err(AiError::Server { status: 400, message }) if message.is_empty()), "{failed:?}");
        assert!(!format!("{failed:?}").contains("SECRET"));

        let busy = MockServer::start_async().await;
        busy.mock_async(|when, then| {
            when.method("GET").path("/v1/models");
            then.status(429).json_body(json!({"type": "error", "error": {"type": "rate_limit_error"}}));
        })
        .await;
        let waiting = Anthropic::new(http(), &busy.url("/v1"), Some(Secret::new(CLAUDE_KEY))).list().await;
        assert!(matches!(&waiting, Err(error) if error.code() == "busy"), "{waiting:?}");

        let gone = Anthropic::new(http(), NOBODY, Some(Secret::new(CLAUDE_KEY)));
        assert_eq!(gone.list().await, Err(AiError::Offline));
        assert_eq!(gone.check().await, Err(AiError::Offline));
    }

    #[tokio::test]
    async fn an_anthropic_account_needs_a_key_and_never_sends_one_unencrypted() {
        let dir = tempfile::tempdir().expect("temp dir");
        let keys = MemoryTokens::default();
        let ai = service(dir.path(), &keys, Vec::new());

        let without = ai.connect(AiProvider::Anthropic, None, Secret::default()).await;
        assert!(matches!(&without, Err(AiError::Invalid(message)) if message.contains("Anthropic")), "{without:?}");

        let plain = ai.connect(AiProvider::Anthropic, Some("http://ai.example.org/v1"), Secret::new(CLAUDE_KEY)).await;
        assert!(matches!(plain, Err(AiError::Invalid(_))), "no key over plain http to another computer");
        assert!(ai.infos().await.is_empty());
    }

    #[tokio::test]
    async fn local_servers_are_found_with_their_accounts() {
        let dir = tempfile::tempdir().expect("temp dir");
        let keys = MemoryTokens::default();
        let ollama = fake_ollama().await;
        let local = vec![(AiProvider::Ollama, ollama.url("/v1")), (AiProvider::LmStudio, NOBODY.to_string())];
        let ai = service(dir.path(), &keys, local);

        let found = ai.find_local().await;
        assert_eq!(
            found,
            [AiServer { provider: AiProvider::Ollama, url: ollama.url("/v1"), models: 3, account: None }]
        );
        let info =
            ai.connect(AiProvider::Ollama, Some(&ollama.url("/v1")), Secret::default()).await.expect("connected");
        assert_eq!(ai.find_local().await[0].account.as_deref(), Some(info.id.as_str()));
        assert!(service(dir.path(), &keys, Vec::new()).find_local().await.is_empty());
    }

    #[tokio::test]
    async fn stored_accounts_are_loaded_a_missing_key_is_a_problem_and_removing_forgets_them() {
        let dir = tempfile::tempdir().expect("temp dir");
        let keys = MemoryTokens::default();
        let ollama = Account::ai(AiProvider::Ollama, "http://localhost:11434/v1");
        let openrouter = Account::ai(AiProvider::OpenRouter, "https://openrouter.ai/api/v1");
        let github = Account::github("anna");
        account::save_to(&accounts_file(dir.path()), &[ollama.clone(), github.clone(), openrouter.clone()])
            .expect("saved");
        let ai = service(dir.path(), &keys, Vec::new());
        ai.load().await.expect("loaded");

        let infos = ai.infos().await;
        let labels: Vec<&str> = infos.iter().map(|info| info.label.as_str()).collect();
        assert_eq!(labels, ["Ollama", "OpenRouter"], "the usual addresses show the name only; GitHub stays out");
        let problem = |at: usize| match &infos[at].state {
            AccountState::Connected { problem, .. } => problem.clone(),
            other => panic!("{other:?}"),
        };
        assert_eq!(problem(0), None, "a local server needs no key");
        assert_eq!(problem(1).as_deref(), Some(tr("Connect again: no API key is stored on this device")));
        assert_eq!(ai.models(&openrouter.id, false).await, Err(AiError::NoKey));
        assert!(matches!(ai.models("ai:nobody@nowhere", false).await, Err(AiError::NotFound(_))));

        keys.put(&openrouter.id, KEY);
        ai.remove(&openrouter.id).await.expect("removed");
        assert_eq!(keys.get_plain(&openrouter.id), None, "its key is gone");
        let left = account::load_from(&accounts_file(dir.path())).expect("read");
        assert_eq!(left, [ollama, github]);
        assert!(!ai.knows(&openrouter.id).await && ai.remove(&openrouter.id).await.is_err());
    }

    /// Which models of `id` are on, by id, as `ai.list_models` would mark them (task S.24).
    async fn on(ai: &AiAccounts, id: &str) -> Vec<String> {
        let models = ai.models(id, true).await.expect("models");
        ai.mark_enabled(id, models).await.into_iter().filter(|model| model.enabled).map(|model| model.id).collect()
    }

    /// Task S.24, the rule at the start: models with tools that compute here are on; Ollama's cloud
    /// model, OpenRouter and Anthropic are off, and an account with nothing on offers no model.
    #[tokio::test]
    async fn at_the_start_only_models_that_compute_here_are_on() {
        let dir = tempfile::tempdir().expect("temp dir");
        let keys = MemoryTokens::default();
        let (ollama, openrouter, anthropic) =
            (fake_ollama_with_cloud(true).await, fake_openrouter().await, fake_anthropic().await);
        let plain = fake_ollama().await;
        let ai = service(dir.path(), &keys, Vec::new());
        let ollama = ai.connect(AiProvider::Ollama, Some(&ollama.url("/v1")), Secret::default()).await.expect("ollama");
        let plain = ai.connect(AiProvider::Ollama, Some(&plain.url("/v1")), Secret::default()).await.expect("ollama");
        let openrouter = ai
            .connect(AiProvider::OpenRouter, Some(&openrouter.url("/api/v1")), Secret::new(KEY))
            .await
            .expect("openrouter");
        let claude = ai
            .connect(AiProvider::Anthropic, Some(&anthropic.url("/v1")), Secret::new(CLAUDE_KEY))
            .await
            .expect("anthropic");

        assert_eq!(on(&ai, &ollama.id).await, ["qwen3:8b"], "the cloud model is off");
        assert_eq!(on(&ai, &plain.id).await, ["qwen3:8b", "broken:1b"], "gemma3 cannot use tools");
        assert!(on(&ai, &openrouter.id).await.is_empty(), "OpenRouter computes elsewhere");
        assert!(on(&ai, &claude.id).await.is_empty(), "Anthropic computes elsewhere");
        assert!(ai.offers_a_model(&ollama.id).await);
        assert!(!ai.offers_a_model(&openrouter.id).await && !ai.offers_a_model(&claude.id).await);
        assert!(!ai.offers_a_model("ai:ollama@nowhere.example").await, "no such account");
        assert_eq!(ai.runs_elsewhere(&ollama.id, "glm-5.3:cloud").await, Ok(true));
        assert_eq!(ai.runs_elsewhere(&ollama.id, "qwen3:8b").await, Ok(false));
        assert_eq!(ai.runs_elsewhere(&openrouter.id, "qwen/qwen3-14b").await, Ok(true), "open, but in the cloud");
        assert_eq!(ai.runs_elsewhere("ai:ollama@nowhere.example", "qwen3:8b").await, Ok(false), "no account");
        assert!(!dir.path().join("config").join(ai::MODELS_FILE).exists(), "nothing stored by reading");
    }

    /// #231: a decision model on this computer starts on, although it has no tools – sorting mail
    /// needs it.
    #[tokio::test]
    async fn a_decision_model_here_starts_on() {
        let dir = tempfile::tempdir().expect("temp dir");
        let keys = MemoryTokens::default();
        let server = fake_deciding_ollama().await;
        let ai = service(dir.path(), &keys, Vec::new());
        let ollama = ai.connect(AiProvider::Ollama, Some(&server.url("/v1")), Secret::default()).await.expect("ollama");
        // The fake's tags list only nimble, so qwen3 counts as elsewhere.
        assert_eq!(on(&ai, &ollama.id).await, ["nimble"]);
        assert!(!ai.offers_a_model(&ollama.id).await, "on for the mail sorting, never a chat engine");
    }

    /// Task S.24: switching stores `ai-models.json` (0600) and survives a new start; turning on takes
    /// only a model the server offers, turning off any; the first model turned off keeps the others
    /// the rule had on, and new ones stay off from then on; a removed account forgets its switches.
    #[tokio::test]
    async fn switches_are_stored_and_read_again_at_the_next_start() {
        let dir = tempfile::tempdir().expect("temp dir");
        let keys = MemoryTokens::default();
        let (ollama, openrouter) = (fake_ollama().await, fake_openrouter().await);
        let ai = service(dir.path(), &keys, Vec::new());
        let local = ai.connect(AiProvider::Ollama, Some(&ollama.url("/v1")), Secret::default()).await.expect("ollama");
        let cloud = ai
            .connect(AiProvider::OpenRouter, Some(&openrouter.url("/api/v1")), Secret::new(KEY))
            .await
            .expect("openrouter");

        assert!(matches!(ai.set_enabled(&cloud.id, "nope/none", true).await, Err(AiError::NotFound(_))));
        assert!(matches!(
            ai.set_enabled("ai:ollama@nowhere.example", "qwen3:8b", false).await,
            Err(AiError::NotFound(_))
        ));
        ai.set_enabled(&cloud.id, "openai/gpt-4o", true).await.expect("on");
        ai.set_enabled(&local.id, "broken:1b", false).await.expect("off");
        ai.set_enabled(&local.id, "llama3:8b", false).await.expect("a model it does not list goes off too");
        assert_eq!(on(&ai, &cloud.id).await, ["openai/gpt-4o"]);
        assert_eq!(on(&ai, &local.id).await, ["qwen3:8b"], "the rule's other model stays on");
        let switches = ai.switches().await.as_ref().map(|held| held.held.clone()).expect("held");
        assert!(!switches.follows_rule(&local.id), "new models stay off from now on");

        let file = dir.path().join("config").join(ai::MODELS_FILE);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(&file).expect("stored").permissions().mode() & 0o777, 0o600);
        }
        let again = service(dir.path(), &keys, Vec::new());
        again.load().await.expect("loaded");
        assert_eq!(on(&again, &cloud.id).await, ["openai/gpt-4o"], "after a new start");
        assert_eq!(on(&again, &local.id).await, ["qwen3:8b"]);
        assert!(again.keep_used_models_on(vec![(local.id.clone(), "gemma3:4b".into())], Vec::new()).await.is_ok());
        assert_eq!(on(&again, &local.id).await, ["qwen3:8b"], "a person switched there: their switch wins");

        again.remove(&cloud.id).await.expect("removed");
        let stored = std::fs::read_to_string(&file).expect("stored");
        assert!(!stored.contains(&cloud.id) && !stored.contains("SECRET"), "{stored}");
    }

    /// Task S.24, the first start after the update: the models in use stay on, also one elsewhere,
    /// once; ids of other engines are left out.
    #[tokio::test]
    async fn the_models_in_use_stay_on_at_the_first_start() {
        let dir = tempfile::tempdir().expect("temp dir");
        let keys = MemoryTokens::default();
        let openrouter = fake_openrouter().await;
        let ai = service(dir.path(), &keys, Vec::new());
        let cloud = ai
            .connect(AiProvider::OpenRouter, Some(&openrouter.url("/api/v1")), Secret::new(KEY))
            .await
            .expect("openrouter");
        let used = vec![(cloud.id.clone(), "qwen/qwen3-14b".to_string()), ("claude-code".into(), "opus".into())];
        ai.keep_used_models_on(used, Vec::new()).await.expect("kept on");
        assert_eq!(on(&ai, &cloud.id).await, ["qwen/qwen3-14b"]);
        assert!(ai.offers_a_model(&cloud.id).await);
        let stored = std::fs::read_to_string(dir.path().join("config").join(ai::MODELS_FILE)).expect("stored");
        assert!(!stored.contains("claude-code"), "{stored}");
        ai.keep_used_models_on(vec![(cloud.id.clone(), "openai/gpt-4o".into())], Vec::new()).await.expect("once only");
        assert_eq!(on(&ai, &cloud.id).await, ["qwen/qwen3-14b"]);
    }

    /// #231: a switch a person turns before the models in use are kept on does not stop it – the
    /// mark is the file's own – but wins at its account; a file from before the mark keeps nothing on.
    #[tokio::test]
    async fn an_early_switch_does_not_stop_keeping_the_models_in_use_on() {
        let dir = tempfile::tempdir().expect("temp dir");
        let keys = MemoryTokens::default();
        let (ollama, openrouter) = (fake_ollama().await, fake_openrouter().await);
        let ai = service(dir.path(), &keys, Vec::new());
        let local = ai.connect(AiProvider::Ollama, Some(&ollama.url("/v1")), Secret::default()).await.expect("ollama");
        let cloud = ai
            .connect(AiProvider::OpenRouter, Some(&openrouter.url("/api/v1")), Secret::new(KEY))
            .await
            .expect("openrouter");
        ai.set_enabled(&local.id, "broken:1b", false).await.expect("a person's switch, written at once");
        let file = dir.path().join("config").join(ai::MODELS_FILE);
        assert!(std::fs::read_to_string(&file).expect("stored").contains("keep_used"), "still to do");

        // The core stopped here; at its next start it keeps them on all the same.
        let again = service(dir.path(), &keys, Vec::new());
        again.load().await.expect("loaded");
        let used = vec![(cloud.id.clone(), "qwen/qwen3-14b".to_string()), (local.id.clone(), "gemma3:4b".into())];
        again.keep_used_models_on(used, Vec::new()).await.expect("kept on");
        assert_eq!(on(&again, &cloud.id).await, ["qwen/qwen3-14b"]);
        assert_eq!(on(&again, &local.id).await, ["qwen3:8b"], "the person's account stays theirs");
        assert!(!std::fs::read_to_string(&file).expect("stored").contains("keep_used"), "done");

        // A file written before the mark existed: the update has passed, nothing is turned on.
        let old = tempfile::tempdir().expect("temp dir");
        let ai = service(old.path(), &keys, Vec::new());
        let cloud = ai
            .connect(AiProvider::OpenRouter, Some(&openrouter.url("/api/v1")), Secret::new(KEY))
            .await
            .expect("openrouter");
        let file = old.path().join("config").join(ai::MODELS_FILE);
        std::fs::write(&file, r#"{"accounts": {}}"#).expect("an old file");
        let ai = service(old.path(), &keys, Vec::new());
        ai.load().await.expect("loaded");
        ai.keep_used_models_on(vec![(cloud.id.clone(), "qwen/qwen3-14b".into())], Vec::new())
            .await
            .expect("nothing to do");
        assert!(on(&ai, &cloud.id).await.is_empty());
    }

    /// PR review #242: an `ai-models.json` that does not read is never written over. Reading falls
    /// back on the rule (no model elsewhere is on), every switch is refused with `error` and a
    /// sentence naming the file, and the file stays as it was, with every switch the person made.
    #[tokio::test]
    async fn a_file_of_switches_that_does_not_read_is_never_written_over() {
        let dir = tempfile::tempdir().expect("temp dir");
        let keys = MemoryTokens::default();
        let (ollama, openrouter) = (fake_ollama().await, fake_openrouter().await);
        let ai = service(dir.path(), &keys, Vec::new());
        let local = ai.connect(AiProvider::Ollama, Some(&ollama.url("/v1")), Secret::default()).await.expect("ollama");
        let cloud = ai
            .connect(AiProvider::OpenRouter, Some(&openrouter.url("/api/v1")), Secret::new(KEY))
            .await
            .expect("openrouter");
        let file = dir.path().join("config").join(ai::MODELS_FILE);
        let damaged = br#"{"accounts": {"#.to_vec();
        std::fs::write(&file, &damaged).expect("a damaged file");

        let ai = service(dir.path(), &keys, Vec::new());
        ai.load().await.expect("loaded");
        assert!(on(&ai, &cloud.id).await.is_empty(), "nothing elsewhere is on");
        assert!(on(&ai, &local.id).await.contains(&"qwen3:8b".to_string()), "the rule decides");
        let refused = trf(
            "{file} cannot be read, so no model can be turned on or off. Repair or remove it, then restart Torro Workplace.",
            &[("file", &ai::MODELS_FILE)],
        );
        // Clients read it too (the MCP tool): the file's name only, the path is the log's.
        assert!(!refused.contains(&dir.path().display().to_string()), "{refused}");
        for (account, model, enabled) in [(&cloud.id, "openai/gpt-4o", true), (&local.id, "qwen3:8b", false)] {
            let err = ai.set_enabled(account, model, enabled).await.expect_err("refused");
            assert_eq!((err.code(), err.to_string()), ("error", refused.clone()));
        }
        ai.keep_used_models_on(vec![(cloud.id.clone(), "openai/gpt-4o".into())], Vec::new()).await.expect("nothing");
        ai.remove(&cloud.id).await.expect("removed");
        assert_eq!(std::fs::read(&file).expect("still there"), damaged, "never written over");
    }

    /// #231: while the core keeps the models in use on, the readers of the switches wait for it, so
    /// none counts a model in use as off.
    #[tokio::test]
    async fn readers_wait_until_the_models_in_use_are_kept_on() {
        let dir = tempfile::tempdir().expect("temp dir");
        let keys = MemoryTokens::default();
        let openrouter = fake_openrouter().await;
        let ai = Arc::new(service(dir.path(), &keys, Vec::new()));
        let cloud = ai
            .connect(AiProvider::OpenRouter, Some(&openrouter.url("/api/v1")), Secret::new(KEY))
            .await
            .expect("openrouter");
        ai.hold_until_kept();
        let reader = tokio::spawn({
            let (ai, id) = (Arc::clone(&ai), cloud.id.clone());
            async move { ai.offers_a_model(&id).await }
        });
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(!reader.is_finished(), "it waits");
        ai.keep_used_models_on(vec![(cloud.id.clone(), "qwen/qwen3-14b".into())], Vec::new()).await.expect("kept on");
        assert!(reader.await.expect("read"), "and reads the model kept on");
    }

    /// #231: what "Automatic" took before the switches – for a chat open weights, here first, with
    /// tools; for sorting mail the first model that decides, here only where mail may not leave.
    #[test]
    fn automatic_picks_what_it_took_before_the_switches() {
        use crate::assistant::Place;
        let model = |id: &str, tools: bool, open: bool, decides: bool, remote: bool| AiModel {
            tools: Some(tools),
            open_weights: Some(open),
            decision: Some(decides),
            remote: Some(remote),
            ..AiModel::named(id)
        };
        let models = [
            model("closed", true, false, false, false),
            model("cloud-decides", false, true, true, true),
            model("cloud-tools", true, true, false, true),
            model("here-decides", false, true, true, false),
            model("here-tools", true, true, false, false),
        ];
        let place = |model: &AiModel| if model.remote == Some(true) { Place::Remote } else { Place::Local };
        let ids = |uses: Automatic| uses.picks(&models, place).into_iter().map(|m| m.id).collect::<Vec<_>>();
        assert_eq!(ids(Automatic { chat: true, ..Automatic::default() }), ["here-tools"]);
        assert_eq!(ids(Automatic { decides: true, ..Automatic::default() }), ["cloud-decides"]);
        assert_eq!(ids(Automatic { decides_here: true, ..Automatic::default() }), ["here-decides"]);
        let both = Automatic { chat: true, ..Automatic::default() }
            .and(Automatic { decides_here: true, ..Automatic::default() });
        assert_eq!(ids(both), ["here-tools", "here-decides"]);
        assert!(Automatic::default().picks(&models, place).is_empty());
        let sorting = ai::AiFunction::InboxCategorize;
        assert_eq!(Automatic::for_function(sorting, false), Automatic { decides_here: true, ..Automatic::default() });
        assert_eq!(Automatic::for_function(sorting, true), Automatic { decides: true, ..Automatic::default() });
    }

    /// #231: an account used with "Automatic" before the switches keeps that model on at its first
    /// list – OpenRouter and Anthropic compute elsewhere, so the rule alone would leave them off –,
    /// counts as offering a model until then, and is taken over once.
    #[tokio::test]
    async fn what_automatic_used_stays_on_at_the_first_list() {
        let dir = tempfile::tempdir().expect("temp dir");
        let keys = MemoryTokens::default();
        let (openrouter, anthropic) = (fake_openrouter().await, fake_anthropic().await);
        let ai = service(dir.path(), &keys, Vec::new());
        let cloud = ai
            .connect(AiProvider::OpenRouter, Some(&openrouter.url("/api/v1")), Secret::new(KEY))
            .await
            .expect("openrouter");
        let claude = ai
            .connect(AiProvider::Anthropic, Some(&anthropic.url("/v1")), Secret::new(CLAUDE_KEY))
            .await
            .expect("anthropic");
        let chat = Automatic { chat: true, ..Automatic::default() };
        let automatic = vec![(cloud.id.clone(), chat), (claude.id.clone(), chat), ("claude-code".into(), chat)];
        ai.keep_used_models_on(Vec::new(), automatic).await.expect("noted");
        let file = dir.path().join("config").join(ai::MODELS_FILE);
        let stored = std::fs::read_to_string(&file).expect("stored");
        assert!(stored.contains("automatic") && !stored.contains("claude-code"), "{stored}");
        assert!(ai.offers_a_model(&cloud.id).await && ai.offers_a_model(&claude.id).await, "before any list");

        assert_eq!(on(&ai, &cloud.id).await, ["qwen/qwen3-14b"], "open weights first");
        assert_eq!(on(&ai, &claude.id).await, ["claude-opus-5"]);
        assert!(!std::fs::read_to_string(&file).expect("stored").contains("automatic"), "taken over");
        ai.set_enabled(&cloud.id, "qwen/qwen3-14b", false).await.expect("off");
        assert!(on(&ai, &cloud.id).await.is_empty(), "once only: a list later turns nothing on again");
    }

    /// #231: a server that does not answer at the first start keeps the account marked, and it
    /// counts as offering a model; its first list takes it over. A person who switches there first
    /// wins: nothing is taken over.
    #[tokio::test]
    async fn automatic_waits_for_a_server_that_is_away_and_gives_way_to_a_person() {
        let dir = tempfile::tempdir().expect("temp dir");
        let keys = MemoryTokens::default();
        let server = MockServer::start_async().await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/api/v1/key");
                then.status(200).json_body(json!({"data": {"label": "sk-or-v1-012…def", "limit": null}}));
            })
            .await;
        let away = server
            .mock_async(|when, then| {
                when.method("GET").path("/api/v1/models");
                then.status(503);
            })
            .await;
        let ai = service(dir.path(), &keys, Vec::new());
        let cloud = ai
            .connect(AiProvider::OpenRouter, Some(&server.url("/api/v1")), Secret::new(KEY))
            .await
            .expect("connected");
        let chat = Automatic { chat: true, ..Automatic::default() };
        ai.keep_used_models_on(Vec::new(), vec![(cloud.id.clone(), chat)]).await.expect("noted");
        ai.take_over_automatic().await;
        assert!(ai.models(&cloud.id, true).await.is_err(), "the server is away");
        assert!(ai.offers_a_model(&cloud.id).await, "nobody loses the assistant meanwhile");

        away.delete_async().await;
        server
            .mock_async(|when, then| {
                when.method("GET").path("/api/v1/models");
                then.status(200).json_body(json!({"data": [
                    {"id": "qwen/qwen3-14b", "name": "Qwen3 14B", "hugging_face_id": "Qwen/Qwen3-14B",
                     "supported_parameters": ["tools"]},
                ]}));
            })
            .await;
        ai.take_over_automatic().await;
        assert_eq!(on(&ai, &cloud.id).await, ["qwen/qwen3-14b"], "taken over at the first list");

        // Another account, where the person switches before the list loads.
        let other = tempfile::tempdir().expect("temp dir");
        let openrouter = fake_openrouter().await;
        let ai = service(other.path(), &keys, Vec::new());
        let cloud = ai
            .connect(AiProvider::OpenRouter, Some(&openrouter.url("/api/v1")), Secret::new(KEY))
            .await
            .expect("openrouter");
        ai.keep_used_models_on(Vec::new(), vec![(cloud.id.clone(), chat)]).await.expect("noted");
        ai.set_enabled(&cloud.id, "openai/gpt-4o", true).await.expect("the person's switch");
        assert_eq!(on(&ai, &cloud.id).await, ["openai/gpt-4o"], "their switch wins");

        // A switch that fails is none: the list it loaded takes over all the same.
        let third = tempfile::tempdir().expect("temp dir");
        let ai = service(third.path(), &keys, Vec::new());
        let cloud = ai
            .connect(AiProvider::OpenRouter, Some(&openrouter.url("/api/v1")), Secret::new(KEY))
            .await
            .expect("openrouter");
        ai.keep_used_models_on(Vec::new(), vec![(cloud.id.clone(), chat)]).await.expect("noted");
        assert!(ai.set_enabled(&cloud.id, "nope/none", true).await.is_err(), "not offered");
        assert_eq!(on(&ai, &cloud.id).await, ["qwen/qwen3-14b"]);
    }

    /// Task S.24: a model turned on that the server no longer offers counts once the core holds the
    /// account's list, so the account then has no model on.
    #[tokio::test]
    async fn a_model_turned_on_that_the_server_no_longer_offers_does_not_count() {
        let dir = tempfile::tempdir().expect("temp dir");
        let keys = MemoryTokens::default();
        let openrouter = fake_openrouter().await;
        let ai = service(dir.path(), &keys, Vec::new());
        let cloud = ai
            .connect(AiProvider::OpenRouter, Some(&openrouter.url("/api/v1")), Secret::new(KEY))
            .await
            .expect("openrouter");
        ai.keep_used_models_on(vec![(cloud.id.clone(), "gone/model".into())], Vec::new()).await.expect("kept on");
        assert!(ai.offers_a_model(&cloud.id).await, "without a list the switch counts");
        assert!(on(&ai, &cloud.id).await.is_empty(), "the server does not list it");
        assert!(!ai.offers_a_model(&cloud.id).await, "with the list it does not count");
    }

    #[test]
    fn labels_name_the_provider_and_the_host_where_it_is_not_the_usual_one() {
        assert_eq!(label(&Account::ai(AiProvider::LmStudio, "http://localhost:1234/v1")), "LM Studio");
        assert_eq!(label(&Account::ai(AiProvider::Ollama, "http://gpu.local:11434/v1")), "Ollama (gpu.local:11434)");
        assert_eq!(label(&Account::ai(AiProvider::OpenAiCompatible, "https://ai.example.org/v1")), "ai.example.org");
    }
}
