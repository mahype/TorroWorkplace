//! AI accounts (task KI.1a, `docs/ki.md`): servers the suite asks a model – Ollama and LM Studio on
//! this computer, OpenRouter in the cloud, any other OpenAI-compatible server. Connecting one
//! (`accounts.connect_ai`), its models (`ai.list_models`), finding local servers
//! (`ai.find_local_servers`), removing it (`accounts.remove_account`).
//!
//! The protocol's wire format is `torro_core::ai::openai`; here it is sent ([`OpenAiCompatible`],
//! the one implementation of [`Provider`]). The accounts are kept like the GitHub accounts
//! ([`super::github`]): the list in `accounts.json` (kind `ai`, the server's base address in
//! `server`, the kind of server in `Account::ai`), the API key in the system keychain under the
//! account id, loaded once in the background ([`AiAccounts::start`]).
//!
//! The key leaves the core only towards the account's own server, in the `Authorization` header.
//! It travels only over https or to this computer ([`torro_core::ai::key_may_travel`]), the HTTP
//! client follows no redirect (a redirect could carry it elsewhere), and every error passes
//! through [`openai::scrub`], so no answer, message or log line repeats it.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use tokio::sync::RwLock;
use torro_core::Account;
use torro_core::account::{self, AccountKind, Secret};
use torro_core::ai::{self, AiError, AiModel, AiProvider, BoxFuture, ChatReply, ChatRequest, Provider, openai};
use torro_core::i18n::tr;
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

/// How long Ollama may take to describe one model (`/api/show`, read from disk); a model it does not
/// describe in time stays without a word on tools, so one stuck model never holds up the list.
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

/// One AI server with the OpenAI-compatible protocol: Ollama, LM Studio, vLLM, llama.cpp,
/// OpenRouter. `Debug` never shows the key ([`Secret`]).
#[derive(Debug, Clone)]
pub struct OpenAiCompatible {
    http: reqwest::Client,
    provider: AiProvider,
    /// The base address of its API, as `torro_core::ai::normalize_base` writes it, without `/` at
    /// the end: `http://localhost:11434/v1`.
    base: String,
    key: Option<Secret>,
}

impl OpenAiCompatible {
    /// The server of `provider` at `base` with `key` (none for most local servers). Sends nothing
    /// yet.
    pub fn new(http: reqwest::Client, provider: AiProvider, base: &str, key: Option<Secret>) -> OpenAiCompatible {
        OpenAiCompatible { http, provider, base: base.trim_end_matches('/').to_string(), key }
    }

    /// Whether the server takes this account: OpenRouter's key with `GET <base>/key` (its model
    /// list is public, so it would not tell), any other server by listing its models.
    pub async fn check(&self) -> Result<(), AiError> {
        match self.provider {
            AiProvider::OpenRouter => self.send(self.request(reqwest::Method::GET, "key")).await.map(drop),
            _ => self.list().await.map(drop),
        }
    }

    /// `GET <base>/models`, without asking Ollama about tools: what finding a server needs.
    pub async fn list(&self) -> Result<Vec<AiModel>, AiError> {
        let response = self.send(self.request(reqwest::Method::GET, "models")).await?;
        let body = self.body(response).await?;
        openai::parse_models(&body).map_err(|err| self.scrubbed(err))
    }

    /// Ollama's word on whether `model` calls tools (`POST <root>/api/show`); `None` when it does
    /// not answer within [`SHOW_TIMEOUT`] or does not say.
    async fn ollama_tools(&self, model: &str) -> Option<bool> {
        let url = format!("{}/api/show", openai::ollama_root(&self.base));
        let builder = self.authorized(self.http.post(url)).json(&openai::ollama_show_body(model));
        let asked = async {
            let response = self.send(builder).await.ok()?;
            openai::ollama_tools(&response.bytes().await.ok()?)
        };
        tokio::time::timeout(SHOW_TIMEOUT, asked).await.ok().flatten()
    }

    /// A request to `<base>/<path>` with the key, if there is one.
    fn request(&self, method: reqwest::Method, path: &str) -> reqwest::RequestBuilder {
        self.authorized(self.http.request(method, format!("{}/{path}", self.base)))
    }

    /// The key goes in the `Authorization` header only, which reqwest marks as sensitive.
    fn authorized(&self, builder: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.key {
            Some(key) => builder.bearer_auth(key.expose()),
            None => builder,
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

    /// `error` without any server text that repeats the key ([`openai::scrub`]).
    fn scrubbed(&self, error: AiError) -> AiError {
        openai::scrub(error, self.key.as_ref().map(Secret::expose))
    }

    async fn stream(
        &self,
        response: reqwest::Response,
        on_text: &mut (dyn FnMut(&str) + Send),
    ) -> Result<ChatReply, AiError> {
        let streamed = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.contains("text/event-stream"));
        let mut response = response;
        if !streamed {
            // A server that ignored `stream: true` answers whole.
            let reply = openai::parse_reply(&self.body(response).await?).map_err(|err| self.scrubbed(err))?;
            if !reply.text.is_empty() {
                on_text(&reply.text);
            }
            return Ok(reply);
        }
        let mut decoder = openai::StreamDecoder::new();
        loop {
            match response.chunk().await {
                Ok(Some(bytes)) => decoder.push(&bytes, on_text).map_err(|err| self.scrubbed(err))?,
                Ok(None) => break,
                Err(err) => return Err(self.scrubbed(transport_error(&err))),
            }
        }
        decoder.finish(on_text).map_err(|err| self.scrubbed(err))
    }
}

impl Provider for OpenAiCompatible {
    /// `GET <base>/models`; for Ollama each model's capabilities from `/api/show` besides, one after
    /// the other (a local server, a handful of models).
    fn models(&self) -> BoxFuture<'_, Result<Vec<AiModel>, AiError>> {
        Box::pin(async move {
            let mut models = self.list().await?;
            if self.provider == AiProvider::Ollama {
                for model in &mut models {
                    model.tools = self.ollama_tools(&model.id).await;
                }
            }
            Ok(models)
        })
    }

    /// `POST <base>/chat/completions` with `stream: true`; read as server-sent events, or whole
    /// when the server answers with plain JSON.
    fn chat<'a>(
        &'a self,
        request: &'a ChatRequest,
        on_text: &'a mut (dyn FnMut(&str) + Send),
    ) -> BoxFuture<'a, Result<ChatReply, AiError>> {
        Box::pin(async move {
            let builder =
                self.request(reqwest::Method::POST, "chat/completions").json(&openai::chat_body(request, true));
            let response = self.send(builder).await?;
            self.stream(response, on_text).await
        })
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
        AiAccounts { accounts_file, keys, http, local, slots: RwLock::new(Vec::new()) }
    }

    /// Loads the stored AI accounts with their keys, in the background. Asks no server: a local one
    /// may not run now, and that is no problem until it is used. An account that needs a key and
    /// has none is listed with a problem and can be connected again or removed.
    pub fn start(self: &Arc<Self>) {
        let service = Arc::clone(self);
        tokio::spawn(async move {
            if let Err(err) = service.load().await {
                eprintln!("torroworkplace: cannot read the AI accounts: {err}");
            }
        });
    }

    /// Reads the AI accounts of `accounts.json` and their keys.
    async fn load(&self) -> Result<(), String> {
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
                held.push(Slot { account, key, checked: None, problem });
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

    async fn client(&self, id: &str) -> Result<OpenAiCompatible, AiError> {
        let slots = self.slots.read().await;
        let slot = slots.iter().find(|slot| slot.account.id == id).ok_or_else(|| no_account(id))?;
        if slot.key.is_none() && slot.provider().needs_key() {
            return Err(AiError::NoKey);
        }
        Ok(OpenAiCompatible::new(self.http()?.clone(), slot.provider(), &slot.account.server, slot.key.clone()))
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
            return Err(AiError::Invalid(tr("OpenRouter needs an API key. Create one on openrouter.ai.").into()));
        }
        if key.is_some() && !ai::key_may_travel(&base) {
            return Err(AiError::Invalid(
                tr("An API key is only sent over https or to this computer. Use an https address.").into(),
            ));
        }
        let client = OpenAiCompatible::new(self.http()?.clone(), provider, &base, key.clone());
        client.check().await?;
        let account = Account::ai(provider, &base);
        self.store(&account, key.as_ref()).await.map_err(AiError::Format)?;
        let slot = Slot { account, key, checked: Some(Utc::now()), problem: None };
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
    /// refused key becomes its problem until it answers again.
    pub async fn models(&self, id: &str, all: bool) -> Result<Vec<AiModel>, AiError> {
        let result = self.provider(id).await?.models().await;
        if let Some(slot) = self.slots.write().await.iter_mut().find(|slot| slot.account.id == id) {
            match &result {
                Ok(_) => {
                    slot.checked = Some(Utc::now());
                    slot.problem = None;
                }
                Err(AiError::Auth) => {
                    slot.problem = Some(tr("Connect again: the server does not accept the API key").into());
                }
                Err(_) => {}
            }
        }
        let models = result?;
        Ok(if all { models } else { ai::open_weights_only(models) })
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
        Ok(())
    }

    fn http(&self) -> Result<&reqwest::Client, AiError> {
        self.http.as_ref().ok_or_else(|| AiError::Format(tr("No HTTP client to ask AI servers with.").into()))
    }
}

fn no_account(id: &str) -> AiError {
    AiError::NotFound(torro_core::i18n::trf("There is no AI account {id}.", &[("id", &id)]))
}

#[cfg(test)]
pub(crate) mod tests {
    use httpmock::MockServer;
    use serde_json::json;
    use torro_core::ai::{ChatMessage, Finish, ToolCall, ToolSpec, Usage};

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
            [("qwen3:8b", json!(["completion", "tools"])), ("gemma3:4b", json!(["completion"]))]
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
        let listed: Vec<String> = ai.infos().await.into_iter().map(|listed| listed.id).collect();
        assert_eq!(listed, [info.id]);
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
        }
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

    #[test]
    fn labels_name_the_provider_and_the_host_where_it_is_not_the_usual_one() {
        assert_eq!(label(&Account::ai(AiProvider::LmStudio, "http://localhost:1234/v1")), "LM Studio");
        assert_eq!(label(&Account::ai(AiProvider::Ollama, "http://gpu.local:11434/v1")), "Ollama (gpu.local:11434)");
        assert_eq!(label(&Account::ai(AiProvider::OpenAiCompatible, "https://ai.example.org/v1")), "ai.example.org");
    }
}
