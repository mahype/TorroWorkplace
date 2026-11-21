//! Signing in to an AI account with a browser (issue #324, `docs/ki.md`, "Anmelden mit ChatGPT"):
//! `accounts.start_ai_sign_in`, `accounts.check_ai_sign_in`, `accounts.cancel_ai_sign_in`.
//!
//! The protocol is `torro_core::ai::oauth`; here it runs. Starting a sign-in binds a listener on `127.0.0.1` (the
//! provider's port, else any free one), builds the authorization address with PKCE and hands it to the control
//! centre, which opens it in the browser. The listener waits for the provider's callback, checks it, exchanges the
//! code for tokens and connects the account ([`AiAccounts::connect_signed_in`]): the tokens go into the keychain and
//! never leave the core. The control centre only asks how the sign-in stands.
//!
//! What a provider needs to remember between sign-ins and is no secret – this computer's id towards it
//! (`ext_agent_host_id`) and the client id it issued at the first sign-in – is kept in `<config>/ai-sign-in.json`
//! (0600), so it outlives signing out, as the provider asks.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use torro_core::Account;
use torro_core::account::{self, Secret};
use torro_core::ai::oauth::{self, Attempt, CallbackError, SignIn};
use torro_core::ai::{AiError, AiProvider};
use torro_core::i18n::{tr, trf};
use torro_ipc::AccountInfo;

use super::ai::{AiAccounts, unix_now};

/// The file of what sign-ins remember that is no secret, next to `accounts.json`.
pub const SIGN_IN_FILE: &str = "ai-sign-in.json";

/// How long a sign-in waits for the person in the browser.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(10 * 60);

/// How long one request to the callback address may take to arrive; and the most of it that is read.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_MAX: usize = 16 * 1024;

/// What a provider's sign-ins remember on this computer ([`SIGN_IN_FILE`]).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Remembered {
    /// This computer's id towards the provider (`ext_agent_host_id`): `urn:uuid:…`, made once.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    host_id: String,
    /// The client id the provider issued at the first sign-in (`oaiapp_…`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    client_id: Option<String>,
}

/// The result of a sign-in, once it ended.
type Outcome = Arc<Mutex<Option<Result<AccountInfo, AiError>>>>;

struct Flow {
    provider: AiProvider,
    outcome: Outcome,
    task: tokio::task::AbortHandle,
}

/// The sign-ins under way. One per provider: starting another ends the one before.
pub struct SignIns {
    /// [`SIGN_IN_FILE`]; `None` without a home directory, and then nothing is remembered.
    file: Option<PathBuf>,
    /// The API's address per provider as tests point it at fake servers; else the provider's usual one.
    bases: Vec<(AiProvider, String)>,
    flows: Mutex<HashMap<String, Flow>>,
}

impl SignIns {
    /// The sign-ins of the user's account list, as the daemon runs them.
    pub fn system() -> SignIns {
        SignIns::new(account::accounts_file().ok().map(|file| file.with_file_name(SIGN_IN_FILE)))
    }

    /// Sign-ins that remember in `file`. Tests pass a temp file.
    pub fn new(file: Option<PathBuf>) -> SignIns {
        SignIns { file, bases: Vec::new(), flows: Mutex::new(HashMap::new()) }
    }

    /// The API of `provider` at `base` instead of its usual address: fake servers in tests.
    #[cfg(test)]
    pub fn with_base(mut self, provider: AiProvider, base: &str) -> SignIns {
        self.bases.push((provider, base.to_string()));
        self
    }

    fn base_of(&self, provider: AiProvider) -> Option<String> {
        self.bases
            .iter()
            .find(|(held, _)| *held == provider)
            .map(|(_, base)| base.clone())
            .or_else(|| provider.default_url().map(str::to_string))
    }

    fn flows(&self) -> std::sync::MutexGuard<'_, HashMap<String, Flow>> {
        self.flows.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Starts signing in to `provider` for `ai` (`accounts.start_ai_sign_in`): the flow's id and the address to open.
    /// `None` for a provider without a sign-in.
    pub async fn start(&self, ai: &Arc<AiAccounts>, provider: AiProvider) -> Option<Result<(String, String), AiError>> {
        let sign_in = ai.sign_in_of(provider)?;
        // Decision on #324: without a client id of the suite's own (ChatGPT) nothing starts and nothing is written.
        if !sign_in.offered() {
            return Some(Err(oauth::not_configured()));
        }
        Some(self.begin(ai, provider, sign_in).await)
    }

    async fn begin(
        &self,
        ai: &Arc<AiAccounts>,
        provider: AiProvider,
        sign_in: SignIn,
    ) -> Result<(String, String), AiError> {
        let base =
            self.base_of(provider).ok_or_else(|| AiError::Invalid(tr("Enter the address of the server.").into()))?;
        let remembered = self.remembered(provider).await?;
        let client = sign_in.client(remembered.client_id.as_deref())?;
        // The provider's port first, any free one when it is taken: only the port may differ between sign-ins.
        let listener = match TcpListener::bind(("127.0.0.1", sign_in.port)).await {
            Ok(listener) => listener,
            Err(_) => TcpListener::bind(("127.0.0.1", 0)).await.map_err(|_| {
                AiError::Invalid(tr("No port is free on this computer for the sign-in. Try again.").into())
            })?,
        };
        let port = listener.local_addr().map_err(|err| AiError::Format(err.to_string()))?.port();
        let attempt = Attempt {
            state: random()?,
            nonce: random()?,
            verifier: Secret::new(random()?),
            redirect_uri: sign_in.redirect_uri(port),
            client,
            host_id: remembered.host_id,
        };
        let url = oauth::authorize_url(&sign_in, &attempt)?;
        let flow = random()?;
        let outcome: Outcome = Arc::new(Mutex::new(None));
        let run = Run { ai: Arc::clone(ai), provider, sign_in, base, attempt, file: self.file.clone() };
        let written = Arc::clone(&outcome);
        let task = tokio::spawn(async move {
            let result = match tokio::time::timeout(SIGN_IN_TIMEOUT, run.listen(listener)).await {
                Ok(result) => result,
                Err(_) => Err(AiError::Invalid(tr("The sign-in took too long. Try again.").into())),
            };
            *written.lock().unwrap_or_else(PoisonError::into_inner) = Some(result);
        });
        let mut flows = self.flows();
        // One sign-in per provider: the person clicked again, the older one is over.
        flows.retain(|_, held| {
            let other = held.provider != provider;
            if !other {
                held.task.abort();
            }
            other
        });
        flows.insert(flow.clone(), Flow { provider, outcome, task: task.abort_handle() });
        Ok((flow, url))
    }

    /// How the sign-in `flow` stands (`accounts.check_ai_sign_in`): `None` for a flow the core does not know,
    /// `Some(None)` while the person is in the browser, else its end, which the flow is forgotten with.
    pub fn check(&self, flow: &str) -> Option<Option<Result<AccountInfo, AiError>>> {
        let mut flows = self.flows();
        let held = flows.get(flow)?;
        let ended = held.outcome.lock().unwrap_or_else(PoisonError::into_inner).take();
        if ended.is_some() {
            flows.remove(flow);
        }
        Some(ended)
    }

    /// Ends the sign-in `flow` (`accounts.cancel_ai_sign_in`): stops listening, stores nothing. Fine for a flow
    /// the core does not know.
    pub fn cancel(&self, flow: &str) {
        if let Some(held) = self.flows().remove(flow) {
            held.task.abort();
        }
    }

    /// What sign-ins to `provider` remember, with this computer's id made and kept the first time.
    async fn remembered(&self, provider: AiProvider) -> Result<Remembered, AiError> {
        let mut all = read_file(self.file.as_ref()).await;
        let mut held = all.get(provider.id()).cloned().unwrap_or_default();
        if held.host_id.is_empty() {
            held.host_id = host_id()?;
            all.insert(provider.id().to_string(), held.clone());
            if let Some(file) = &self.file {
                write_file(file, &all).await.map_err(AiError::Format)?;
            }
        }
        Ok(held)
    }
}

/// One sign-in, waiting for its callback.
struct Run {
    ai: Arc<AiAccounts>,
    provider: AiProvider,
    sign_in: SignIn,
    base: String,
    attempt: Attempt,
    file: Option<PathBuf>,
}

impl Run {
    /// Answers requests to the callback address until one ends the sign-in. Requests that are no answer to this
    /// sign-in (another path, another `state`) get "not found" and change nothing.
    async fn listen(self, listener: TcpListener) -> Result<AccountInfo, AiError> {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else { continue };
            let Some(target) = read_target(&mut stream).await else {
                respond(&mut stream, "400 Bad Request", None).await;
                continue;
            };
            match oauth::read_callback(&self.sign_in, &self.attempt, &target) {
                Err(CallbackError::Unrelated | CallbackError::Foreign) => {
                    respond(&mut stream, "404 Not Found", None).await;
                }
                Err(CallbackError::Failed(error)) => {
                    respond(&mut stream, "200 OK", Some(Err(&error))).await;
                    return Err(error);
                }
                Ok(callback) => {
                    let result = self.finish(callback).await;
                    respond(&mut stream, "200 OK", Some(result.as_ref().map(drop))).await;
                    return result;
                }
            }
        }
    }

    /// Exchanges the code for tokens, remembers the client id and connects the account.
    async fn finish(&self, callback: oauth::Callback) -> Result<AccountInfo, AiError> {
        let http = self.ai.http()?.clone();
        let response = http
            .post(self.sign_in.token_url)
            .form(&oauth::code_form(&self.sign_in, &self.attempt, &callback))
            .send()
            .await
            .map_err(|err| {
                if err.is_connect() || err.is_timeout() {
                    AiError::Offline
                } else {
                    AiError::Format(tr("The provider could not be asked.").into())
                }
            })?;
        let status = response.status().as_u16();
        let body = response.bytes().await.map_err(|_| AiError::Offline)?;
        if !(200..300).contains(&status) {
            return Err(oauth::token_error(status, &body));
        }
        let id = Account::ai(self.provider, &self.base).id;
        let previous = self.ai.tokens_of(&id).await;
        let mut tokens = oauth::read_tokens(
            &self.sign_in,
            &body,
            unix_now(),
            &callback.client_id,
            previous.as_ref(),
            Some(&self.attempt.nonce),
        )?;
        tokens.host_id = self.attempt.host_id.clone();
        if let Some(file) = &self.file {
            let mut all = read_file(Some(file)).await;
            let held = all.entry(self.provider.id().to_string()).or_default();
            held.host_id = self.attempt.host_id.clone();
            held.client_id = Some(callback.client_id.clone());
            if let Err(err) = write_file(file, &all).await {
                eprintln!("torroworkplace: cannot remember the client id of the sign-in: {err}");
            }
        }
        self.ai.connect_signed_in(self.provider, &self.base, tokens).await
    }
}

/// The target of the request on `stream` (`/auth/callback?…`), read up to [`REQUEST_MAX`] within
/// [`REQUEST_TIMEOUT`]; `None` for anything that is no `GET`.
async fn read_target(stream: &mut TcpStream) -> Option<String> {
    let mut head = Vec::new();
    let mut buffer = [0u8; 2048];
    let read = async {
        while !head.windows(4).any(|end| end == b"\r\n\r\n") && head.len() < REQUEST_MAX {
            let n = stream.read(&mut buffer).await.ok()?;
            if n == 0 {
                break;
            }
            head.extend_from_slice(&buffer[..n]);
        }
        Some(())
    };
    tokio::time::timeout(REQUEST_TIMEOUT, read).await.ok()??;
    let text = String::from_utf8_lossy(&head);
    let mut words = text.lines().next()?.split_whitespace();
    (words.next()? == "GET").then_some(())?;
    words.next().filter(|target| target.starts_with('/')).map(str::to_string)
}

/// Answers the browser: a short page that says how the sign-in went, or nothing for a request that was no answer.
async fn respond(stream: &mut TcpStream, status: &str, ended: Option<Result<(), &AiError>>) {
    let body = match ended {
        None => String::new(),
        Some(Ok(())) => page(tr("You are signed in"), tr("You can close this window and go back to Torro Workplace.")),
        Some(Err(error)) => page(
            tr("The sign-in did not work"),
            &trf("{error} You can close this window.", &[("error", &error.to_string())]),
        ),
    };
    let reply = format!(
        "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(reply.as_bytes()).await;
    let _ = stream.shutdown().await;
}

fn page(title: &str, text: &str) -> String {
    let escape = |text: &str| text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{title}</title></head>\
         <body style=\"font-family:system-ui,sans-serif;max-width:32rem;margin:4rem auto;padding:0 1rem\">\
         <h1 style=\"font-size:1.4rem\">{title}</h1><p>{text}</p></body></html>",
        title = escape(title),
        text = escape(text)
    )
}

/// 32 random bytes as base64url: `state`, `nonce`, PKCE's verifier, a flow's id.
fn random() -> Result<String, AiError> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|err| AiError::Format(format!("no random numbers for the sign-in: {err}")))?;
    Ok(oauth::url_safe(&bytes))
}

/// A new id of this computer towards a provider: `urn:uuid:` with a random (version 4) UUID, nothing about the person.
fn host_id() -> Result<String, AiError> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|err| AiError::Format(format!("no random numbers for the sign-in: {err}")))?;
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    Ok(format!("urn:uuid:{}-{}-{}-{}-{}", &hex[0..8], &hex[8..12], &hex[12..16], &hex[16..20], &hex[20..32]))
}

async fn read_file(file: Option<&PathBuf>) -> HashMap<String, Remembered> {
    let Some(file) = file else { return HashMap::new() };
    tokio::fs::read(file).await.ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

async fn write_file(file: &std::path::Path, all: &HashMap<String, Remembered>) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(all).map_err(|err| err.to_string())?;
    if let Some(dir) = file.parent() {
        crate::clients::create_private_dir(dir).map_err(|err| format!("{}: {err}", dir.display()))?;
    }
    crate::clients::write_private(file, &bytes).map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests {
    use httpmock::prelude::*;
    use serde_json::json;
    use torro_core::ai::oauth::Registration;

    use super::*;
    use crate::account::ai::tests::service;
    use crate::account::github::tests::MemoryTokens;

    fn leak(text: String) -> &'static str {
        Box::leak(text.into_boxed_str())
    }

    /// ChatGPT's sign-in at the fake `server`, on any free port, with the configured client id `oaiapp_1`.
    fn fake_sign_in(server: &MockServer) -> SignIn {
        SignIn {
            issuer: leak(server.url("")),
            authorize_url: leak(server.url("/authorize")),
            token_url: leak(server.url("/token")),
            revoke_url: Some(leak(server.url("/revoke"))),
            port: 0,
            client_env: "TORRO_TEST_NO_SUCH_VARIABLE",
            built_client_id: Some("oaiapp_1"),
            ..oauth::CHATGPT
        }
    }

    /// A provider that lets the app register itself (the scaffold's other way, not ChatGPT's since #324).
    fn registering(server: &MockServer) -> SignIn {
        SignIn {
            registration: Registration::Dynamic { entry: "dynamic_agent_client", name_hint: "Torro Workplace" },
            built_client_id: None,
            ..fake_sign_in(server)
        }
    }

    fn jwt(claims: serde_json::Value) -> String {
        format!("e30.{}.c2ln", oauth::url_safe(claims.to_string().as_bytes()))
    }

    /// Asks the callback address of `url` as the browser would after the person allowed it.
    async fn callback(url: &str, extra: &str) -> String {
        let parsed = url::Url::parse(url).expect("url");
        let pairs: HashMap<String, String> = parsed.query_pairs().into_owned().collect();
        let redirect = url::Url::parse(&pairs["redirect_uri"]).expect("redirect");
        let target = format!("{}?code=code-1&state={}{extra}", redirect.path(), pairs["state"]);
        let mut stream = TcpStream::connect(("127.0.0.1", redirect.port().expect("port"))).await.expect("listening");
        stream.write_all(format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").as_bytes()).await.expect("sent");
        let mut answer = String::new();
        stream.read_to_string(&mut answer).await.expect("answer");
        answer
    }

    async fn ended(sign_ins: &SignIns, flow: &str) -> Result<AccountInfo, AiError> {
        for _ in 0..200 {
            match sign_ins.check(flow) {
                Some(Some(result)) => return result,
                Some(None) => tokio::time::sleep(Duration::from_millis(10)).await,
                None => panic!("flow forgotten"),
            }
        }
        panic!("the sign-in did not end");
    }

    #[tokio::test]
    async fn a_first_sign_in_registers_exchanges_the_code_and_keeps_the_tokens_in_the_keychain_only() {
        let dir = tempfile::tempdir().expect("dir");
        let server = MockServer::start_async().await;
        let sign_in = registering(&server);
        // The nonce is random: the fake token endpoint answers with the one the test reads from the address.
        let keys = MemoryTokens::default();
        let ai = Arc::new(service(dir.path(), &keys, Vec::new()).with_sign_in(AiProvider::ChatGpt, sign_in));
        let sign_ins =
            SignIns::new(Some(dir.path().join(SIGN_IN_FILE))).with_base(AiProvider::ChatGpt, &server.url("/v1"));
        let (flow, url) = sign_ins.start(&ai, AiProvider::ChatGpt).await.expect("signs in").expect("started");
        let pairs: HashMap<String, String> = url::Url::parse(&url).expect("url").query_pairs().into_owned().collect();
        assert_eq!(pairs["client_id"], "dynamic_agent_client");
        assert!(pairs["ext_agent_host_id"].starts_with("urn:uuid:"));
        let id_token_with_nonce = jwt(json!({
            "iss": server.url(""), "aud": "oaiapp_1", "sub": "user-1", "email": "anna@example.org",
            "exp": 4_000_000_000u64, "nonce": pairs["nonce"],
        }));
        let token = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/token")
                    .form_urlencoded_tuple("grant_type", "authorization_code")
                    .form_urlencoded_tuple("client_id", "oaiapp_1")
                    .form_urlencoded_tuple("code", "code-1")
                    .form_urlencoded_tuple_exists("code_verifier")
                    .form_urlencoded_tuple("resource", "https://api.openai.com/v1");
                then.status(200).json_body(json!({
                    "access_token": "access-1", "refresh_token": "refresh-1", "token_type": "Bearer",
                    "expires_in": 3600, "scope": oauth::CHATGPT.scopes.join(" "), "id_token": id_token_with_nonce,
                }));
            })
            .await;
        let models = server
            .mock_async(|when, then| {
                when.method(GET).path("/v1/models").header("authorization", "Bearer access-1");
                then.status(200).json_body(
                    json!({"models": [{"slug": "gpt-6.1-sol", "display_name": "GPT-6.1 Sol", "visibility": "list"}]}),
                );
            })
            .await;
        assert_eq!(sign_ins.check(&flow), Some(None), "pending while the person is in the browser");
        // Something else asking the address ends nothing.
        let unrelated = callback(&url.replace("auth%2Fcallback", "favicon.ico"), "").await;
        assert!(unrelated.starts_with("HTTP/1.1 404"), "{unrelated}");
        let page = callback(&url, "&client_id=oaiapp_1").await;
        assert!(page.contains(tr("You are signed in")), "{page}");
        let account = ended(&sign_ins, &flow).await.expect("connected");
        token.assert_async().await;
        models.assert_async().await;
        assert_eq!(account.id, Account::ai(AiProvider::ChatGpt, &server.url("/v1")).id);
        assert_eq!(sign_ins.check(&flow), None, "asked once, then forgotten");
        let stored = keys.get_plain(&account.id).expect("in the keychain");
        let tokens = oauth::TokenSet::from_secret(&stored).expect("a token set");
        assert_eq!((tokens.client_id.as_str(), tokens.email.as_str()), ("oaiapp_1", "anna@example.org"));
        let remembered = std::fs::read_to_string(dir.path().join(SIGN_IN_FILE)).expect("remembered");
        assert!(
            remembered.contains("oaiapp_1") && !remembered.contains("access-1") && !remembered.contains("refresh-1")
        );
        let listed = std::fs::read_to_string(dir.path().join("config").join("accounts.json")).expect("accounts");
        assert!(!listed.contains("access-1") && listed.contains("anna@example.org"), "{listed}");
        // The next sign-in sends the issued client id.
        let (again, url) = sign_ins.start(&ai, AiProvider::ChatGpt).await.expect("signs in").expect("started");
        assert!(url.contains("client_id=oaiapp_1") && !url.contains("agent_name_hint"), "{url}");
        sign_ins.cancel(&again);
        assert_eq!(sign_ins.check(&again), None);
    }

    #[tokio::test]
    async fn a_declined_sign_in_stores_nothing_and_says_so_in_the_browser() {
        let dir = tempfile::tempdir().expect("dir");
        let server = MockServer::start_async().await;
        let keys = MemoryTokens::default();
        let ai =
            Arc::new(service(dir.path(), &keys, Vec::new()).with_sign_in(AiProvider::ChatGpt, fake_sign_in(&server)));
        let sign_ins =
            SignIns::new(Some(dir.path().join(SIGN_IN_FILE))).with_base(AiProvider::ChatGpt, &server.url("/v1"));
        let (flow, url) = sign_ins.start(&ai, AiProvider::ChatGpt).await.expect("signs in").expect("started");
        let pairs: HashMap<String, String> = url::Url::parse(&url).expect("url").query_pairs().into_owned().collect();
        let redirect = url::Url::parse(&pairs["redirect_uri"]).expect("redirect");
        let mut stream = TcpStream::connect(("127.0.0.1", redirect.port().expect("port"))).await.expect("listening");
        let target = format!("/auth/callback?error=access_denied&state={}", pairs["state"]);
        stream.write_all(format!("GET {target} HTTP/1.1\r\n\r\n").as_bytes()).await.expect("sent");
        let mut page = String::new();
        stream.read_to_string(&mut page).await.expect("page");
        assert!(page.contains(tr("The sign-in did not work")), "{page}");
        assert!(matches!(ended(&sign_ins, &flow).await, Err(AiError::Invalid(_))));
        assert!(!ai.knows(&Account::ai(AiProvider::ChatGpt, &server.url("/v1")).id).await);
    }

    #[tokio::test]
    async fn chatgpt_signs_in_with_its_configured_client_id_and_this_computers_id() {
        let dir = tempfile::tempdir().expect("dir");
        let server = MockServer::start_async().await;
        let keys = MemoryTokens::default();
        let ai =
            Arc::new(service(dir.path(), &keys, Vec::new()).with_sign_in(AiProvider::ChatGpt, fake_sign_in(&server)));
        let sign_ins =
            SignIns::new(Some(dir.path().join(SIGN_IN_FILE))).with_base(AiProvider::ChatGpt, &server.url("/v1"));
        let (flow, url) = sign_ins.start(&ai, AiProvider::ChatGpt).await.expect("signs in").expect("started");
        let pairs: HashMap<String, String> = url::Url::parse(&url).expect("url").query_pairs().into_owned().collect();
        assert_eq!(pairs["client_id"], "oaiapp_1");
        assert!(!pairs.contains_key("agent_name_hint"), "no registration");
        assert!(pairs["ext_agent_host_id"].starts_with("urn:uuid:"));
        sign_ins.cancel(&flow);
    }

    /// Decision on #324: without a configured client id ChatGPT is refused, also when an id was issued before.
    #[tokio::test]
    async fn without_a_configured_client_id_chatgpt_is_refused_with_a_sentence() {
        let dir = tempfile::tempdir().expect("dir");
        let server = MockServer::start_async().await;
        let unconfigured = SignIn { built_client_id: None, ..fake_sign_in(&server) };
        assert!(!unconfigured.offered());
        let file = dir.path().join(SIGN_IN_FILE);
        std::fs::write(&file, r#"{"chatgpt":{"host_id":"urn:uuid:x","client_id":"oaiapp_old"}}"#).expect("file");
        let keys = MemoryTokens::default();
        let ai = Arc::new(service(dir.path(), &keys, Vec::new()).with_sign_in(AiProvider::ChatGpt, unconfigured));
        let sign_ins = SignIns::new(Some(file));
        let refused = sign_ins.start(&ai, AiProvider::ChatGpt).await.expect("a sign-in provider");
        assert_eq!(refused, Err(oauth::not_configured()));
        assert!(sign_ins.start(&ai, AiProvider::OpenRouter).await.is_none(), "a key provider has no sign-in");
    }

    /// Tokens of a ChatGPT account that end in `left` seconds.
    fn tokens(left: u64) -> oauth::TokenSet {
        oauth::TokenSet {
            client_id: "oaiapp_1".into(),
            host_id: "urn:uuid:x".into(),
            access_token: Secret::new("access-1"),
            refresh_token: Secret::new("refresh-1"),
            id_token: Secret::default(),
            expires_at: unix_now() + left,
            scopes: oauth::CHATGPT.scopes.iter().map(|scope| scope.to_string()).collect(),
            subject: "user-1".into(),
            email: "anna@example.org".into(),
        }
    }

    async fn plan_models<'a>(server: &'a MockServer, access: &str) -> httpmock::Mock<'a> {
        let access = format!("Bearer {access}");
        server
            .mock_async(move |when, then| {
                when.method(GET).path("/v1/models").header("authorization", access);
                then.status(200).json_body(
                    json!({"models": [{"slug": "gpt-6.1-sol", "display_name": "GPT-6.1 Sol", "visibility": "list"}]}),
                );
            })
            .await
    }

    #[tokio::test]
    async fn tokens_are_renewed_once_before_they_end_and_the_rotated_refresh_token_is_kept() {
        let dir = tempfile::tempdir().expect("dir");
        let server = MockServer::start_async().await;
        let keys = MemoryTokens::default();
        let ai =
            Arc::new(service(dir.path(), &keys, Vec::new()).with_sign_in(AiProvider::ChatGpt, fake_sign_in(&server)));
        let first = plan_models(&server, "access-1").await;
        let base = server.url("/v1");
        let account = ai.connect_signed_in(AiProvider::ChatGpt, &base, tokens(30)).await.expect("connected");
        first.assert_async().await;
        let refresh = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/token")
                    .form_urlencoded_tuple("grant_type", "refresh_token")
                    .form_urlencoded_tuple("refresh_token", "refresh-1")
                    .form_urlencoded_tuple("client_id", "oaiapp_1");
                then.status(200)
                    .json_body(json!({"access_token": "access-2", "refresh_token": "refresh-2", "expires_in": 3600}));
            })
            .await;
        let renewed = plan_models(&server, "access-2").await;
        // Two requests at once: a refresh token works once, so only one of them renews.
        let (one, two) = tokio::join!(ai.models(&account.id, true), ai.models(&account.id, true));
        assert_eq!(one.expect("models")[0].id, "gpt-6.1-sol");
        assert!(two.is_ok());
        refresh.assert_calls_async(1).await;
        renewed.assert_calls_async(2).await;
        let stored = oauth::TokenSet::from_secret(&keys.get_plain(&account.id).expect("kept")).expect("tokens");
        assert_eq!((stored.access_token.expose(), stored.refresh_token.expose()), ("access-2", "refresh-2"));
    }

    #[tokio::test]
    async fn a_refresh_token_the_provider_no_longer_takes_ends_the_sign_in_on_this_device() {
        let dir = tempfile::tempdir().expect("dir");
        let server = MockServer::start_async().await;
        let keys = MemoryTokens::default();
        let ai =
            Arc::new(service(dir.path(), &keys, Vec::new()).with_sign_in(AiProvider::ChatGpt, fake_sign_in(&server)));
        plan_models(&server, "access-1").await;
        let account =
            ai.connect_signed_in(AiProvider::ChatGpt, &server.url("/v1"), tokens(30)).await.expect("connected");
        server
            .mock_async(|when, then| {
                when.method(POST).path("/token");
                then.status(400).json_body(json!({"error": "refresh_token_reused", "error_description": "refresh-1"}));
            })
            .await;
        assert_eq!(ai.models(&account.id, true).await, Err(AiError::Auth));
        assert_eq!(keys.get_plain(&account.id), None, "unusable tokens are dropped");
        let listed = ai.infos().await;
        let torro_pim::AccountState::Connected { problem, .. } = &listed[0].state else { panic!("connected") };
        assert!(problem.as_deref().is_some_and(|problem| problem.contains("ChatGPT")), "{problem:?}");
        assert_eq!(ai.models(&account.id, true).await, Err(AiError::NoKey));
    }

    #[tokio::test]
    async fn removing_a_signed_in_account_revokes_its_refresh_token() {
        let dir = tempfile::tempdir().expect("dir");
        let server = MockServer::start_async().await;
        let keys = MemoryTokens::default();
        let ai =
            Arc::new(service(dir.path(), &keys, Vec::new()).with_sign_in(AiProvider::ChatGpt, fake_sign_in(&server)));
        plan_models(&server, "access-1").await;
        let account =
            ai.connect_signed_in(AiProvider::ChatGpt, &server.url("/v1"), tokens(3600)).await.expect("connected");
        let revoke = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/revoke")
                    .form_urlencoded_tuple("token", "refresh-1")
                    .form_urlencoded_tuple("token_type_hint", "refresh_token")
                    .form_urlencoded_tuple("client_id", "oaiapp_1");
                then.status(200);
            })
            .await;
        ai.remove(&account.id).await.expect("removed");
        revoke.assert_async().await;
        assert_eq!(keys.get_plain(&account.id), None);
        assert!(!ai.knows(&account.id).await);
    }

    #[tokio::test]
    async fn the_plans_chat_sends_instructions_streams_and_reads_the_plans_errors() {
        let dir = tempfile::tempdir().expect("dir");
        let server = MockServer::start_async().await;
        let keys = MemoryTokens::default();
        let ai =
            Arc::new(service(dir.path(), &keys, Vec::new()).with_sign_in(AiProvider::ChatGpt, fake_sign_in(&server)));
        plan_models(&server, "access-1").await;
        let account =
            ai.connect_signed_in(AiProvider::ChatGpt, &server.url("/v1"), tokens(3600)).await.expect("connected");
        let limit = server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/v1/responses")
                    .header("authorization", "Bearer access-1")
                    .json_body_includes(r#"{"store": false, "stream": true, "instructions": "Help."}"#);
                then.status(429).json_body(json!({"error": {"code": "subscription_sharing_usage_limit_exceeded"}}));
            })
            .await;
        let request = torro_core::ai::ChatRequest {
            model: "gpt-6.1-sol".into(),
            messages: vec![torro_core::ai::ChatMessage::system("Help."), torro_core::ai::ChatMessage::user("Hi")],
            tools: Vec::new(),
            max_tokens: Some(100),
            effort: None,
        };
        let provider = ai.provider(&account.id).await.expect("provider");
        let refused = provider.chat(&request, &mut |_| {}).await;
        limit.assert_async().await;
        assert!(matches!(refused, Err(AiError::Busy(message)) if message.contains("ChatGPT")));
    }
}
