//! Tool-free AI workflow proposals. Mail URLs stay ephemeral; the model can only select a locally
//! validated package, not supply steps, descriptions, targets, secrets, grants or automatic rules.

use std::cell::RefCell;
use std::collections::BTreeSet;

use html5ever::tendril::StrTendril;
use html5ever::tokenizer::{BufferQueue, TagKind, Token, TokenSink, TokenSinkResult, Tokenizer, TokenizerOpts};
use torro_core::ai::{AiError, Effort};
use torro_core::i18n::tr;
use torro_core::settings::AssistantDefault;
use torro_ipc::api::{Access, Area, Rights};
use torro_ipc::workflows::{DraftSpec, Package};
use torro_ipc::{Request, Response};

use super::{Error as WorkflowError, Scope, Service, download, security, templates};
use crate::assistant::{Host, ModelAccess, Place};
use crate::clients::Clients;
use crate::mail::service::MailService;

/// Exactly the existing `Request::ProposeWorkflow` inputs. No account, path or URL supplied by a
/// caller is treated as evidence: the service resolves the mail and existing target itself.
#[derive(Clone, Copy)]
pub(crate) struct Input<'a> {
    pub mail: &'a str,
    pub save_target: &'a str,
    pub instruction: &'a str,
}

pub(crate) enum Error {
    NoModel,
    DataRule,
    Workflow(WorkflowError),
    Ai(AiError),
}

impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Provider errors can carry a server's arbitrary words. Never include them in diagnostics.
        std::fmt::Display::fmt(self, f)
    }
}

impl Error {
    pub(crate) fn response(&self) -> Response {
        let (code, message) = match self {
            Self::NoModel => {
                ("not-connected", tr("Set up an AI first: connect an AI account under Settings › AI.").to_string())
            }
            Self::DataRule => (
                "forbidden",
                tr("The mail data rule does not allow the configured chat model to read this mail.").to_string(),
            ),
            Self::Workflow(error) => return error.response(),
            Self::Ai(error) => (
                error.code(),
                // Neither a provider error nor a model response may repeat a private link in an
                // error response or audit surface. Keep the useful code, not arbitrary text.
                tr("The AI could not propose a workflow. Review the chat model settings and try again.").to_string(),
            ),
        };
        Response::Error { code: code.into(), message }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("workflow proposal failed")
    }
}

impl std::error::Error for Error {}

impl From<WorkflowError> for Error {
    fn from(error: WorkflowError) -> Self {
        Self::Workflow(error)
    }
}

/// Produces only a draft specification. The caller stores it through its normal draft path under
/// the *same* scope (including origin/ceiling); it must not substitute suite/app authority.
///
/// Pass `settings.current().ai_slots.chat.as_ref()` for `chat`. A missing/removed slot or model
/// returns `not-connected`; no other account, local model, browser or web service is tried.
/// Scope/target checks precede mail reads and the model call, and are repeated after the await.
/// No conversation/history/memory is read or written, and no test, grant or rule is created.
pub(crate) async fn propose(
    host: &dyn Host,
    chat: Option<&AssistantDefault>,
    service: &Service,
    scope: &Scope,
    clients: &Clients,
    mail: &MailService,
    input: Input<'_>,
) -> Result<DraftSpec, Error> {
    validate_input(input)?;
    let request = Request::ProposeWorkflow {
        mail: input.mail.into(),
        save_target: input.save_target.into(),
        instruction: input.instruction.into(),
    };
    let initial = guard(scope, clients, &request)?;
    // Freeze the initial effective ceiling as well as respecting the durable caller's ceiling.
    // Rights granted while the model is running must not widen this proposal's reach.
    let mut pinned = scope.clone();
    pinned.ceiling = initial;
    let (access, effort) = configured_model(host, chat).await?;
    let rights = guard(&pinned, clients, &request)?;
    let (account, html, fingerprint) = service.context(mail, rights.as_ref(), input.mail, input.save_target).await?;
    let choices = candidates(&html)?;
    let descriptions: Vec<String> = choices.iter().map(Candidate::for_model).collect();
    let rights = guard(&pinned, clients, &request)?;
    if !security::reads(rights.as_ref(), &account) || !security::target(rights.as_ref(), input.mail, input.save_target)
    {
        return Err(WorkflowError::Forbidden.into());
    }
    check_data_rule(host, access.place)?;
    let selected = crate::assistant::workflow_choice(&access, effort, &descriptions, input.instruction)
        .await
        .map_err(Error::Ai)?;
    // Re-resolve with current narrowed rights, not the assistant client's broader rights and not
    // the app key. Pending approvals must enter here with their preserved original Scope too.
    let rights = guard(&pinned, clients, &request)?;
    let (current_account, current_html, current_fingerprint) =
        service.context(mail, rights.as_ref(), input.mail, input.save_target).await?;
    if current_account != account || current_fingerprint != fingerprint || current_html != html {
        return Err(WorkflowError::Conflict.into());
    }
    guard(&pinned, clients, &request)?;
    check_data_rule(host, access.place)?;
    let choice = choices.get(selected).ok_or(WorkflowError::Invalid)?;
    let spec = DraftSpec {
        name: tr(&choice.name).into(),
        description: tr(&choice.description).into(),
        mail: input.mail.into(),
        save_target: input.save_target.into(),
        package: choice.package.clone(),
    };
    super::check_spec(&spec)?;
    Ok(spec)
}

fn validate_input(input: Input<'_>) -> Result<(), WorkflowError> {
    if [input.mail, input.save_target]
        .iter()
        .any(|value| value.is_empty() || !torro_ipc::workflows::safe_text(value, 200))
        || input.instruction.len() > 4096
        || input.instruction.chars().any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    {
        return Err(WorkflowError::Invalid);
    }
    Ok(())
}

fn guard(scope: &Scope, clients: &Clients, request: &Request) -> Result<Option<Rights>, WorkflowError> {
    if scope.origin.without.iter().any(|area| matches!(area, Area::Mail | Area::Workflows))
        || scope.origin.mode.refuses(request.operation())
        || (scope.carrier.is_none() && (scope.origin.behalf.is_some() || scope.ceiling.is_some()))
    {
        return Err(WorkflowError::Forbidden);
    }
    let rights = scope.current(clients)?;
    if rights
        .as_ref()
        .is_some_and(|rights| !rights.allows_request(request) || !rights.area(Area::Mail).allows(Access::Read))
    {
        return Err(WorkflowError::Forbidden);
    }
    Ok(rights)
}

fn check_data_rule(host: &dyn Host, place: Place) -> Result<(), Error> {
    if place != Place::Local && !host.data_rule().mail_to_cloud {
        return Err(Error::DataRule);
    }
    Ok(())
}

async fn configured_model(
    host: &dyn Host,
    chat: Option<&AssistantDefault>,
) -> Result<(ModelAccess, Option<Effort>), Error> {
    let chat = chat.filter(|chat| !chat.engine.is_empty()).ok_or(Error::NoModel)?;
    let access = host.model(&chat.engine, chat.model.as_deref()).await.map_err(|error| match error {
        AiError::NotFound(_) | AiError::NoKey => Error::NoModel,
        other => Error::Ai(other),
    })?;
    if access.model.is_empty()
        || chat.model.as_deref().is_some_and(|selected| !selected.is_empty() && selected != access.model)
    {
        return Err(Error::NoModel);
    }
    check_data_rule(host, access.place)?;
    Ok((access, chat.effort))
}

/// This is deliberately not the model's free-form text. All persistent text is service-owned;
/// even a label such as a private URL, encoded token or account number in the mail is never copied.
struct Candidate {
    name: String,
    description: String,
    package: Package,
}

impl Candidate {
    fn for_model(&self) -> String {
        serde_json::json!({"name": self.name, "description": self.description, "hosts": self.package.hosts}).to_string()
    }
}

/// An intentionally closed vocabulary of supported invoice paths. It does not guess a second
/// page's link/label: an HTML path not covered by a reviewed adapter needs manual configuration.
fn candidates(html: &str) -> Result<Vec<Candidate>, WorkflowError> {
    let mut choices = Vec::new();
    for host in mail_hosts(html)? {
        if matches!(host.as_str(), "pay.stripe.com" | "account.mapbox.com") {
            continue;
        }
        // Reuse the executor's exact HTTPS/port/path/ambiguity checks. This only parses local
        // hrefs; it performs no DNS or network I/O. In particular, two different links on the
        // same host are not silently reduced to one by throwing their tokens away.
        if download::select(html, &host, None, false).is_err() {
            continue;
        }
        let template = match host.as_str() {
            "invoice.stripe.com" => templates::get("stripe_invoice")?,
            "my.paddle.com" => templates::get("paddle_setapp_invoice_copy")?,
            // A generic proposal must have an actual direct PDF link in this mail. Landing
            // pages, receipts and guessed .pdf suffixes are not evidence of an invoice download.
            _ if download::select(html, &host, None, true).is_ok() => {
                choices.push(Candidate {
                    name: tr("Invoice PDF").into(),
                    description: tr("Downloads the selected mail's unambiguous PDF link, verifies it and saves it to the selected target. No login or resource loading.").into(),
                    package: Package::direct_pdf(&host),
                });
                continue;
            }
            _ => continue,
        };
        choices.push(Candidate { name: template.name, description: template.description, package: template.package });
    }
    if choices.is_empty() {
        return Err(WorkflowError::Unsupported);
    }
    for choice in &choices {
        choice.package.validate().map_err(|_| WorkflowError::Unsupported)?;
    }
    Ok(choices)
}

#[derive(Default)]
struct Hosts {
    hosts: BTreeSet<String>,
    tokens: usize,
    failed: bool,
}

struct HostSink(RefCell<Hosts>);

impl TokenSink for HostSink {
    type Handle = ();

    fn process_token(&self, token: Token, _: u64) -> TokenSinkResult<()> {
        let mut state = self.0.borrow_mut();
        state.tokens += 1;
        if state.tokens > 50_000 || state.failed {
            state.failed = true;
            return TokenSinkResult::Continue;
        }
        if let Token::TagToken(tag) = token {
            if tag.attrs.len() > 64 {
                state.failed = true;
            }
            if tag.kind == TagKind::StartTag && matches!(tag.name.as_ref(), "script" | "style") {
                return TokenSinkResult::RawData(if tag.name.as_ref() == "script" {
                    html5ever::tokenizer::states::RawKind::ScriptData
                } else {
                    html5ever::tokenizer::states::RawKind::Rawtext
                });
            }
            if tag.kind == TagKind::StartTag && tag.name.as_ref() == "a" {
                for attr in tag.attrs.iter().filter(|attr| attr.name.local.as_ref() == "href") {
                    let Ok(url) = reqwest::Url::parse(attr.value.as_ref()) else { continue };
                    if let Some(host) = url.domain().filter(|host| torro_ipc::workflows::valid_host(host)) {
                        if state.hosts.len() == 32 && !state.hosts.contains(host) {
                            state.failed = true;
                        } else {
                            state.hosts.insert(host.into());
                        }
                    }
                }
            }
        }
        TokenSinkResult::Continue
    }
}

fn mail_hosts(html: &str) -> Result<BTreeSet<String>, WorkflowError> {
    if html.len() > 2 * 1024 * 1024 {
        return Err(WorkflowError::Unsupported);
    }
    let tokenizer = Tokenizer::new(HostSink(RefCell::new(Hosts::default())), TokenizerOpts::default());
    let queue = BufferQueue::default();
    queue.push_back(StrTendril::from_slice(html));
    let _ = tokenizer.feed(&queue);
    tokenizer.end();
    let state = tokenizer.sink.0.into_inner();
    if state.failed {
        return Err(WorkflowError::Unsupported);
    }
    Ok(state.hosts)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use torro_core::ai::{AiModel, BoxFuture, ChatReply, ChatRequest, Provider};
    use torro_ipc::api::{AreaRights, Level, Selection};

    use super::*;

    #[derive(Default)]
    struct Record {
        models: Mutex<Vec<(String, Option<String>)>>,
        requests: Mutex<Vec<ChatRequest>>,
    }

    struct FakeHost {
        record: Arc<Record>,
        place: Place,
        rule: Mutex<crate::assistant::DataRule>,
        missing: bool,
    }

    impl FakeHost {
        fn new(place: Place, cloud: bool) -> Self {
            Self {
                record: Arc::default(),
                place,
                rule: Mutex::new(crate::assistant::DataRule { mail_to_cloud: cloud }),
                missing: false,
            }
        }
    }

    struct FakeProvider(Arc<Record>);

    impl Provider for FakeProvider {
        fn models(&self) -> BoxFuture<'_, Result<Vec<AiModel>, AiError>> {
            panic!("proposal never lists a provider's models or chooses a fallback")
        }

        fn chat<'a>(
            &'a self,
            request: &'a ChatRequest,
            _: &'a mut (dyn FnMut(&str) + Send),
        ) -> BoxFuture<'a, Result<ChatReply, AiError>> {
            Box::pin(async move {
                self.0.requests.lock().expect("requests").push(request.clone());
                Ok(ChatReply { text: r#"{"candidate":0}"#.into(), ..ChatReply::default() })
            })
        }
    }

    impl Host for FakeHost {
        fn client(&self) -> Option<torro_ipc::ClientInfo> {
            None
        }

        fn model<'a>(&'a self, engine: &'a str, model: Option<&'a str>) -> BoxFuture<'a, Result<ModelAccess, AiError>> {
            Box::pin(async move {
                self.record.models.lock().expect("models").push((engine.into(), model.map(String::from)));
                if self.missing {
                    return Err(AiError::NotFound("removed".into()));
                }
                Ok(ModelAccess {
                    model: model.unwrap_or("configured-automatic").into(),
                    provider: Box::new(FakeProvider(Arc::clone(&self.record))),
                    place: self.place,
                })
            })
        }

        fn data_rule(&self) -> crate::assistant::DataRule {
            *self.rule.lock().expect("rule")
        }

        fn run<'a>(&'a self, _: Request, _: &'a str, _: bool) -> BoxFuture<'a, Response> {
            panic!("proposal must never execute a tool, grant, rule or web fallback")
        }

        fn approval<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Response> {
            panic!("proposal must never ask for execution authority")
        }
    }

    #[tokio::test]
    async fn only_the_configured_host_model_is_used_and_cloud_is_opt_in() {
        let slot = AssistantDefault {
            engine: "ai:synthetic".into(),
            model: Some("chosen".into()),
            effort: Some(Effort::High),
        };
        let mut host = FakeHost::new(Place::Remote, false);
        assert!(matches!(configured_model(&host, None).await, Err(Error::NoModel)));
        assert!(host.record.models.lock().expect("models").is_empty());
        assert!(matches!(configured_model(&host, Some(&slot)).await, Err(Error::DataRule)));
        assert!(host.record.requests.lock().expect("requests").is_empty());
        host.missing = true;
        let error = configured_model(&host, Some(&slot)).await.err().expect("missing slot model");
        assert!(matches!(error.response(), Response::Error { code, .. } if code == "not-connected"));
        host.missing = false;
        *host.rule.lock().expect("rule") = crate::assistant::DataRule { mail_to_cloud: true };
        let (access, effort) = configured_model(&host, Some(&slot)).await.expect("explicit cloud opt-in");
        assert_eq!(access.model, "chosen");
        assert_eq!(effort, Some(Effort::High));
        assert_eq!(crate::assistant::workflow_choice(&access, effort, &["Invoice PDF".into()], "").await, Ok(0));
        *host.rule.lock().expect("rule") = crate::assistant::DataRule::default();
        assert!(check_data_rule(&host, access.place).is_err(), "revoked cloud consent fails closed");
        assert!(
            host.record
                .models
                .lock()
                .expect("models")
                .iter()
                .all(|(engine, model)| engine == "ai:synthetic" && model.as_deref() == Some("chosen"))
        );
        assert_eq!(host.record.requests.lock().expect("requests").len(), 1);
    }

    #[tokio::test]
    async fn proposal_checks_origin_intersection_without_mail_and_revoked_clients() {
        let dir = tempfile::tempdir().expect("temporary clients");
        let clients = crate::clients::tests::open_in(dir.path());
        let mut rights = Rights::none();
        rights.areas.insert(Area::Workflows, AreaRights { level: Level::Full, admin: false });
        rights.areas.insert(Area::Mail, AreaRights { level: Level::Read, admin: false });
        rights.mail_accounts = Selection::Only(vec!["mail:allowed@example.org".into()]);
        let (client, _) = clients.add("Proposal", "agent", rights.clone()).await.expect("client");
        let mut scope =
            Scope { carrier: Some(client.id.clone()), origin: Default::default(), ceiling: Some(rights.clone()) };
        let request = Request::ProposeWorkflow {
            mail: "m:synthetic".into(),
            save_target: "target1".into(),
            instruction: String::new(),
        };
        let effective = guard(&scope, &clients, &request).expect("reachable").expect("client rights");
        assert!(security::reads(Some(&effective), "mail:allowed@example.org"));
        assert!(!security::reads(Some(&effective), "mail:foreign@example.org"));
        scope.origin.without.push(Area::Mail);
        assert!(guard(&scope, &clients, &request).is_err());
        scope.origin.without.clear();
        let (other, _) = clients.add("Narrower", "agent", Rights::none()).await.expect("delegator");
        scope.origin.behalf = Some(other.id.clone());
        assert!(guard(&scope, &clients, &request).is_err());
        clients.remove(&other.id).await.expect("remove delegator");
        assert!(guard(&scope, &clients, &request).is_err());
        scope.origin.behalf = None;
        clients.update(&client.id, Rights::none()).await.expect("revoke rights");
        assert!(guard(&scope, &clients, &request).is_err());
        clients.remove(&client.id).await.expect("remove carrier");
        assert!(guard(&scope, &clients, &request).is_err());
        let mut suite = Scope::suite();
        suite.origin.without = vec![Area::Mail];
        assert!(guard(&suite, &clients, &request).is_err(), "no app escalation around without Mail");
    }

    #[tokio::test]
    async fn proposal_prerequisites_fail_without_model_call_fetch_persistence_or_authority() {
        let dir = tempfile::tempdir().expect("temporary clients");
        let clients = crate::clients::tests::open_in(dir.path());
        let service = Service::memory();
        let mail = MailService::without_home();
        let host = FakeHost::new(Place::Local, false);
        let input = Input { mail: "m:synthetic", save_target: "target1", instruction: "Invoice" };
        let mut scope = Scope::suite();
        scope.origin.without = vec![Area::Mail];
        assert!(matches!(
            propose(&host, None, &service, &scope, &clients, &mail, input).await,
            Err(Error::Workflow(WorkflowError::Forbidden))
        ));
        assert!(host.record.models.lock().expect("models").is_empty());
        scope.origin.without.clear();
        assert!(matches!(propose(&host, None, &service, &scope, &clients, &mail, input).await, Err(Error::NoModel)));
        let slot = AssistantDefault { engine: "ai:synthetic".into(), model: None, effort: None };
        assert!(matches!(
            propose(&host, Some(&slot), &service, &scope, &clients, &mail, input).await,
            Err(Error::Workflow(WorkflowError::NotFound))
        ));
        assert!(host.record.requests.lock().expect("requests").is_empty());
        assert!(
            matches!(service.answer(Request::ListWorkflows, scope.clone(), &clients, &mail).await, Response::Workflows { workflows } if workflows.is_empty())
        );
        assert!(
            matches!(service.answer(Request::ListWorkflowRules, scope.clone(), &clients, &mail).await, Response::WorkflowRules { rules } if rules.is_empty())
        );
        assert!(
            matches!(service.answer(Request::ListWorkflowRuns { workflow: None, mail: None }, scope, &clients, &mail).await, Response::WorkflowRuns { runs } if runs.is_empty())
        );
    }

    #[tokio::test]
    async fn complete_proposal_reads_only_local_mail_and_returns_an_unsaved_secret_free_spec() {
        use crate::mail::service::tests::{mail_account, service_in, store_accounts};
        use crate::mail::store::{Body, NewMessage};

        let dir = tempfile::tempdir().expect("temporary home");
        let secrets = crate::account::github::tests::MemoryTokens::default();
        let endpoint =
            torro_mail::Endpoint { host: "imap.example.org".into(), port: 993, security: torro_mail::Security::Tls };
        let account = mail_account("synthetic@example.org", &endpoint);
        store_accounts(dir.path(), &secrets, &[(&account, "synthetic-nonsecret")]);
        let mail = service_in(dir.path(), &secrets);
        // No background IMAP, SMTP, DNS or logo requests; the fixture exists only in the local store.
        mail.pause_from_start(true);
        mail.reload().await;
        let store = mail.store(&account.id).await.expect("local store");
        let message = store.run(|store| {
            let folder = store.upsert_folder("INBOX", "Inbox", Some(torro_mail::Role::Inbox))?;
            store.note_uidvalidity(&folder, 1)?;
            let id = store.upsert_message(&folder, 1, &NewMessage {
                id: None, uid: 1, rfc_message_id: Some("<proposal@example.org>".into()), thread_id: "proposal".into(),
                from: None, to: Vec::new(), cc: Vec::new(), reply_to: Vec::new(), subject: "Synthetic invoice".into(),
                date: chrono::Utc::now(), snippet: String::new(), flags: Vec::new(), has_attachments: false,
                traits: Default::default(), invitation: false,
            })?;
            store.set_body(&id, &Body {
                text: "Synthetic invoice".into(), from_html: true,
                html: Some("<a href='https://documents.example.org/SAMPLE-TOKEN.pdf?token=SAMPLE-SECRET'>SAMPLE-LABEL</a>".into()),
                attachments: Vec::new(), invite: None,
            })?;
            Ok(id)
        }).await.expect("local mail");
        let folder = dir.path().join("retrieved");
        std::fs::create_dir(&folder).expect("existing target");
        let target = mail
            .save_targets()
            .create(torro_mail::SaveTargetSpec::new("Invoices", folder.to_string_lossy()), &[])
            .expect("target");
        let clients = crate::clients::tests::open_in(dir.path());
        let service = Service::memory();
        let host = FakeHost::new(Place::Local, false);
        let slot = AssistantDefault { engine: "ai:synthetic".into(), model: Some("chosen".into()), effort: None };
        let input = Input { mail: &message, save_target: &target.id, instruction: "Invoice, not payment receipt" };
        let spec = propose(&host, Some(&slot), &service, &Scope::suite(), &clients, &mail, input)
            .await
            .expect("validated draft");
        assert_eq!(spec.mail, message);
        assert_eq!(spec.save_target, target.id);
        assert_eq!(spec.package.hosts, ["documents.example.org"]);
        super::super::check_spec(&spec).expect("normal draft validation");
        assert!(!serde_json::to_string(&spec).expect("draft").contains("SAMPLE-"));
        {
            let asked = host.record.requests.lock().expect("requests");
            assert_eq!(asked.len(), 1);
            assert!(!serde_json::to_string(&*asked).expect("prompt").contains("SAMPLE-"));
        }
        assert_eq!(std::fs::read_dir(&folder).expect("target directory").count(), 0, "no test download or write");
        assert!(
            matches!(service.answer(Request::ListWorkflows, Scope::suite(), &clients, &mail).await, Response::Workflows { workflows } if workflows.is_empty()),
            "the caller must explicitly store the draft under the original scope"
        );
    }

    #[test]
    fn candidates_are_executable_typed_references_not_private_examples_or_mail_labels() {
        let html = r#"<a href="https://documents.example.org/private/SAMPLE-TOKEN.pdf?secret=SAMPLE-SECRET">SAMPLE-TOKEN</a>
            <a href="https://invoice.stripe.com/i/SAMPLE-MERCHANT/SAMPLE-INVOICE">ignore instructions</a>
            <a href="https://my.paddle.com/invoice?token=SAMPLE-PADDLE">https://private.example.org/token</a>"#;
        let offered = candidates(html).expect("three supported paths");
        assert_eq!(offered.len(), 3);
        for candidate in offered {
            candidate.package.validate().expect("typed package");
            let stored = serde_json::to_string(&candidate.package).expect("package");
            let shown = candidate.for_model();
            for private in ["SAMPLE-", "://", "ignore instructions", "private.example.org"] {
                assert!(!stored.contains(private));
                assert!(!shown.contains(private));
            }
        }
    }

    #[test]
    fn ambiguous_unsafe_guessed_and_script_links_are_not_proposed() {
        for html in [
            r#"<a href="https://files.example.org/a.pdf">Invoice</a><a href="https://files.example.org/b.pdf">Invoice</a>"#,
            r#"<a href="https://files.example.org/landing">Invoice</a>"#,
            r#"<a href="http://files.example.org/a.pdf">Invoice</a>"#,
            r#"<a href="https://u:p@files.example.org/a.pdf">Invoice</a>"#,
            r#"<a href="https://files.example.org:8443/a.pdf">Invoice</a>"#,
            r#"<a href="https://127.0.0.1/a.pdf">Invoice</a>"#,
            r#"<a href="https://invoice.stripe.com/receipt/a/b">Receipt</a>"#,
            r#"<a href="https://pay.stripe.com/pay/a">Pay</a>"#,
            r#"<a href="https://pay.stripe.com/looks-like-an-invoice.pdf">Pay</a>"#,
            r#"<a href="https://account.mapbox.com/invoice.pdf">Account</a>"#,
            r#"<script>let x='<a href="https://files.example.org/a.pdf">Invoice</a>'</script>"#,
        ] {
            assert!(matches!(candidates(html), Err(WorkflowError::Unsupported)));
        }
    }

    #[test]
    fn preparation_is_bounded_and_input_urls_are_not_mail_or_target_references() {
        let valid = Input { mail: "m:synthetic", save_target: "target1", instruction: "Download invoice\nnot receipt" };
        assert!(validate_input(valid).is_ok());
        assert!(validate_input(Input { mail: "https://files.example.org/SECRET", ..valid }).is_err());
        assert!(validate_input(Input { instruction: &"x".repeat(4097), ..valid }).is_err());
        assert!(mail_hosts(&"x".repeat(2 * 1024 * 1024 + 1)).is_err());
        let many = (0..33).map(|n| format!("<a href='https://h{n}.example.org/i.pdf'>Invoice</a>")).collect::<String>();
        assert!(mail_hosts(&many).is_err());
    }
}
