//! The settings of an app, shown in its own window: what the gear of every TorroWorkplace app
//! opens. Concept: `docs/settings.md` ("Einstellungen in den Apps"), task S.16 in
//! `docs/tasks/steuerzentrale.md`.
//!
//! The rule (`docs/settings.md`, "Die Regel"): what only changes how one app works belongs into
//! that app, exactly once, behind its gear in the same window. The control centre keeps what the
//! whole suite shares and only points there. This library holds everything both sides need:
//!
//! * [`parts`] – the rows a settings page is built from ([`parts::setting_row`],
//!   [`parts::pressable`], [`parts::chevron`], [`parts::dialog`], [`parts::confirm`] …). The
//!   control centre uses **only** these, never the host.
//! * the sections of each app, one module per app (`assistant`, `calendar`, `inbox`, `mail` …,
//!   tasks S.17 to S.20), each written in the shape below.
//! * [`host`] – the [`Host`], which shows an app's sections in its content area: the section list,
//!   the header bar with the path, the page, and the sending and loading around it.
//!
//! # The shape of a section
//!
//! Every section is a module of this library with the same shape as a page of the control centre,
//! so a page can move from there to here unchanged but for its imports:
//!
//! ```text
//! pub struct State            page-local UI state (Default)
//! pub enum Message            what the page's widgets report
//! pub fn update(&mut State, Message, &Data) -> Action<A>
//! pub fn view<'a>(&'a State, &'a Data) -> Element<'a, Message>
//! pub fn header<'a>(&'a State, &'a Data, SizeClass) -> Page<'a, Message>   (optional)
//! pub fn overlay<'a>(&'a State, &'a Data) -> Option<Element<'a, Message>>  (optional)
//! ```
//!
//! A page never talks to the daemon itself: it returns an [`Action`], and the [`Host`] carries it
//! out. What a page shows comes from [`Data`], which the host loads with the requests the app
//! names ([`Host::new`]).
//!
//! `A` is the app's own part of an [`Action`] ([`Action::App`]) – what only the app can do, such
//! as opening the bot editor. A section that needs nothing of the kind stays generic over `A`; a
//! section that does names its own type (`pub enum AppAction { EditBot(String) }`) and the app
//! matches on it. [`Action::map_app`] fits sections with different types into one app.
//!
//! # The way to the daemon
//!
//! The host never calls [`torro_ipc::client`] itself: that reads the app key, so a visual check
//! with `--demo` would change real rights and settings. The app hands it a [`Backend`] instead –
//! its own socket connection, or [`demo::Demo`], which answers sample data from memory and never
//! opens a socket. Nothing in this library knows the socket (a test watches over that).

pub mod assistant;
pub mod calendar;
pub mod collections;
pub mod demo;
pub mod host;
pub mod inbox;
pub mod mail;
pub mod parts;

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use torro_core::ai::AiModel;
use torro_core::settings::Settings;
use torro_inbox::{Rule, SourceInfo};
use torro_ipc::assistant::{Engine, Note, Profile};
use torro_ipc::bots::BotSummary;
use torro_ipc::{AccountInfo, ClientInfo, Request, Response, RuleHits, VoiceStatus};

pub use host::{Form, Frame, Host, Outcome, Page, Section, form};

/// The answer of a [`Backend`], boxed so a `dyn Backend` can be handed around.
pub type Answer = Pin<Box<dyn Future<Output = Result<Response, String>> + Send>>;

/// The way to the daemon, which the app gives the [`Host`] ([`Host::new`]).
///
/// The app implements it over its own socket connection (`torro_ipc::client::call`); with `--demo`
/// it passes [`demo::Demo`], which answers from memory. The host and the sections only ever speak
/// through this trait, so the demo cannot touch anything real.
///
/// An answer the daemon reports as [`Response::Error`] comes back as `Err(message)`, with the
/// message for a person – like the control centre's own `send`. Everything else is `Ok`.
pub trait Backend: Send + Sync + 'static {
    /// Sends `request` and waits for the answer.
    fn call(&self, request: Request) -> Answer;
}

/// Everything the sections of one app show, as far as it was loaded.
///
/// The app names the requests its sections need ([`Host::new`]); [`load`] sends them and
/// [`Data::take`] puts every answer where it belongs. A section that needs something new adds its
/// field here and the one arm that fills it – nothing else changes.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Data {
    /// The suite-wide settings (`settings.settings`).
    pub settings: Settings,
    /// The clients the daemon knows (`clients.list_clients`), never with their keys: the sections
    /// read them to tell whether the assistant is on. No section ever changes one – who may use
    /// what, and every key, stays in the control centre (`docs/api.md`, "Was nie nach außen
    /// geht").
    pub clients: Vec<ClientInfo>,
    /// Who the assistant is (`assistant.get_profile`); `None` until an answer came.
    pub profile: Option<Profile>,
    /// The notes of the assistant's memory, oldest first (`assistant.list_memories`).
    pub memories: Vec<Note>,
    /// The bots (`bots.list_bots`); none from a daemon that has none.
    pub bots: Vec<BotSummary>,
    /// What the assistant can answer with (`assistant.list_engines`): an AI account, a coding
    /// agent with its own models.
    pub engines: Vec<Engine>,
    /// The models of the AI account the assistant's default engine names (`ai.list_models`): the
    /// one request only the loaded data can name ([`Data::follow_up`]). Empty for an agent's
    /// engine, which brings its models with it, and while no default is set.
    pub default_models: Vec<AiModel>,
    /// How talking by voice stands (`settings.voice_status`); `None` until an answer came.
    pub voice: Option<VoiceStatus>,
    /// Every account with its collections, hidden ones included (`accounts.list_accounts`): the
    /// calendar app's "Calendars" and the tasks app's "Lists" group by it (task S.19,
    /// [`collections`]). A sign-in that still runs has an empty id and no collections.
    pub accounts: Vec<AccountInfo>,
    /// Which AI account and model each function uses (`ai.list_assignments`, task E12.10): the
    /// mail window's section "Categorization" reads and changes the one for sorting mail.
    pub assignments: Vec<torro_ipc::AiAssignmentInfo>,
    /// How the sorting of mail by a model stands (`inbox.get_categorization`, task E12.10): the
    /// backlog per account.
    pub categorization: torro_ipc::inbox::Categorization,
    /// The models of every AI account, each with the account it came from (`ai.list_models`,
    /// task E12.10): the mail window's "Categorization" offers the deciding ones of all of them,
    /// so a model can be picked before anything is assigned. Only filled where the load asked
    /// for the assignments ([`Data::follow_up`]).
    pub decider_models: Vec<(String, AiModel)>,
    /// The inbox sources the window may see, muted ones included (`inbox.list_sources`): the
    /// inbox's section "Sources" tells by them whether a source is muted (task S.20,
    /// [`inbox::sources`]).
    pub sources: Vec<SourceInfo>,
    /// The lasting rules of the inbox (`inbox.list_rules`), in the core's order: the inbox's
    /// section "Rules" lists them with "Remove" (task S.20, [`inbox::rules`]).
    pub rules: Vec<Rule>,
    /// What each rule of [`Data::rules`] caught since it was made (`inbox.list_rules`), one per
    /// rule by its id; empty from a core before task E8.8a.
    pub rule_hits: Vec<RuleHits>,
    /// Why the load failed, if it did (daemon not running …); the host shows it over the page.
    pub error: Option<String>,
}

impl Data {
    /// Puts one answer where it belongs. An answer no section asked for is dropped: a section that
    /// wants it adds its field and its arm.
    pub fn take(&mut self, response: Response) {
        match response {
            Response::Settings { settings } => self.settings = settings,
            Response::Clients { clients } => self.clients = clients,
            Response::Profile { profile } => self.profile = Some(profile),
            Response::Memories { notes } => self.memories = notes,
            Response::Bots { bots } => self.bots = bots,
            Response::Engines { engines } => self.engines = engines,
            Response::AiModels { account, models } => {
                // Several accounts answer the same command in one load (task E12.10); each
                // answer says which account it is from, and only the assistant's default engine
                // fills its own list. A core from before that field says nothing: then the one
                // answer is the assistant's, as it used to be.
                let default = assistant::models_request(self);
                let mine = matches!(&default, Some(Request::AiModels { account: asked, .. }) if *asked == account);
                if account.is_empty() || mine {
                    self.default_models = models.clone();
                }
                self.decider_models.retain(|(held, _)| *held != account);
                self.decider_models.extend(models.into_iter().map(|model| (account.clone(), model)));
            }
            Response::AiAssignments { assignments } => self.assignments = assignments,
            Response::Categorization { categorization } => self.categorization = categorization,
            Response::Voice { voice } => self.voice = Some(voice),
            Response::Accounts { accounts } => self.accounts = accounts,
            Response::Sources { sources } => self.sources = sources,
            Response::Rules { rules, hits } => {
                self.rules = rules;
                self.rule_hits = hits;
            }
            _ => {}
        }
    }

    /// What a load asks for in a second round, once the first answers are in: requests that only
    /// the loaded data can name. Both are `ai.list_models` – the AI account the assistant's
    /// default engine names ([`assistant::models_request`]), and, where the load asked for the
    /// assignments, every AI account, so the mail window can offer a model to sort mail with
    /// before anything is assigned (task E12.10).
    ///
    /// Asked once, never again from its own answers: a section that adds a round here keeps it to
    /// what the first round brought.
    pub fn follow_up(&self) -> Vec<Request> {
        let mut wanted: Vec<Request> = assistant::models_request(self).into_iter().collect();
        // Only a load that wants the assignments pays for asking every AI server: the control
        // centre keeps the one request it always had.
        if self.assignments.is_empty() {
            return wanted;
        }
        let asked = |wanted: &[Request], id: &str| {
            wanted.iter().any(|request| matches!(request, Request::AiModels { account, .. } if account == id))
        };
        for account in self.accounts.iter().filter(|account| account.kind == "ai" && !account.id.is_empty()) {
            if !asked(&wanted, &account.id) {
                wanted.push(Request::AiModels { account: account.id.clone(), all: false });
            }
        }
        wanted
    }
}

/// Loads `wanted` through `backend`, in the given order, into one [`Data`].
///
/// Every request is sent, also after one failed: a section whose data is missing shows its empty
/// state, and the first failure stands in [`Data::error`] over the page.
pub async fn load(backend: Arc<dyn Backend>, wanted: Vec<Request>) -> Data {
    let mut data = Data::default();
    round(&backend, wanted, &mut data).await;
    // The second round: what only the first one could name ([`Data::follow_up`]).
    let follow_up = data.follow_up();
    round(&backend, follow_up, &mut data).await;
    data
}

/// Sends `requests` in order and puts every answer into `data`; the first failure stands in
/// [`Data::error`].
async fn round(backend: &Arc<dyn Backend>, requests: Vec<Request>, data: &mut Data) {
    for request in requests {
        match backend.call(request).await {
            Ok(response) => data.take(response),
            Err(problem) => {
                if data.error.is_none() {
                    data.error = Some(problem);
                }
            }
        }
    }
}

/// Sends `requests` one after the other and then loads `wanted` again: what the host does with an
/// [`Action::Send`].
///
/// Stops at the first request that fails and reports it, so a half-done change does not look
/// finished; what the daemon already did stays done, and the next load shows it.
pub async fn send(backend: Arc<dyn Backend>, requests: Vec<Request>, wanted: Vec<Request>) -> Result<Data, String> {
    for request in requests {
        backend.call(request).await?;
    }
    Ok(load(backend, wanted).await)
}

/// Sends one request and hands the answer back as it came: what the host does with an
/// [`Action::Ask`]. Nothing is loaded again – a page that asks wants this one answer.
pub async fn ask(backend: Arc<dyn Backend>, request: Request) -> Result<Response, String> {
    backend.call(request).await
}

/// What a section wants the [`Host`] to do. `A` is the app's own part ([`Action::App`]).
#[derive(Debug, Clone, PartialEq)]
#[expect(
    clippy::large_enum_variant,
    reason = "a request can carry a whole task and is handled once; boxing would only add an allocation"
)]
pub enum Action<A> {
    /// Nothing.
    None,
    /// Send these requests to the daemon, one after the other, and load the sections' data again
    /// afterwards. One request per thing that changed (`settings.set_setting` with its key, not a
    /// whole `settings.update_settings`), so two windows never overwrite each other.
    Send(Vec<Request>),
    /// Send this one request and bring its answer back to the section: for a request whose answer
    /// is the point ("Say something" shows what was understood, `settings.test_voice`), not a
    /// change the next load would show. The host sends it and gives the app
    /// [`Outcome::Answer`], which the app hands to the section's `answered`; the sections' data is
    /// **not** loaded again – that is [`Action::Send`].
    Ask(Request),
    /// Open a web page in the browser (`torro_gui::links`).
    OpenUrl(String),
    /// Show another section of these settings, by its [`Section::id`].
    Open(&'static str),
    /// Put the keyboard focus into the text field with this id: what a row that opens a field
    /// (a note of the assistant's memory) asks for. The host does it itself
    /// (`iced::widget::operation::focus`), so a page never builds a task.
    Focus(&'static str),
    /// Open the control centre at this section – what `torroworkplace ui --section <s>` takes
    /// (`accounts`, `appearance`, `assistant`, `bot:<id>` …), through
    /// `torro_gui::connect::open_section`. For everything that stays in the control centre:
    /// accounts, rights, keys, approvals.
    ControlCentre(String),
    /// Close the settings: the window shows what its content area showed before
    /// ([`Outcome::Close`]). For a section that was asked whether the window may close
    /// ([`assistant::closing`]) and has got its answer – "Discard" in "Discard changes?" – so the
    /// question and the closing stay in one place.
    Close,
    /// What only the app itself can do – open the bot editor, show a chat. The app chooses the
    /// type; a section that needs none stays generic over it.
    App(A),
}

impl<A> Action<A> {
    /// One request to send, then reload: the usual case of [`Action::Send`].
    pub fn send(request: Request) -> Self {
        Action::Send(vec![request])
    }

    /// The same action with the app's part mapped, so sections with app actions of their own fit
    /// into one app (`action.map_app(AppAction::Assistant)`).
    pub fn map_app<B>(self, into: impl FnOnce(A) -> B) -> Action<B> {
        match self {
            Action::None => Action::None,
            Action::Send(requests) => Action::Send(requests),
            Action::Ask(request) => Action::Ask(request),
            Action::OpenUrl(url) => Action::OpenUrl(url),
            Action::Open(section) => Action::Open(section),
            Action::Focus(field) => Action::Focus(field),
            Action::ControlCentre(section) => Action::ControlCentre(section),
            Action::Close => Action::Close,
            Action::App(app) => Action::App(into(app)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    /// Nothing in this library may talk to the daemon itself: the host and the sections only speak
    /// through [`Backend`], so `--demo` cannot reach a socket. A section that needs a request asks
    /// the host for it (task S.16).
    ///
    /// Every way into the socket is watched, not only the plain one: `InboxClient` and `PimClient`
    /// call it inside, and `socket_path` is the path itself (found in the API review of task S.22).
    #[test]
    fn no_source_file_speaks_to_the_socket() {
        // Spelled in pieces: the scan reads this file too, and the words below are what it looks
        // for, not something it should find.
        let forbidden = [
            ["client", "::call"].concat(),
            ["torro_ipc::", "client"].concat(),
            ["Inbox", "Client"].concat(),
            ["Pim", "Client"].concat(),
            ["socket", "_path"].concat(),
        ];
        let mut found = Vec::new();
        visit(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut |file: &Path, text: &str| {
            for line in code_of(text) {
                for word in &forbidden {
                    if line.contains(word.as_str()) {
                        found.push(format!("{}: {word}", file.display()));
                    }
                }
            }
        });
        assert!(found.is_empty(), "the settings library reaches the socket: {found:?}");
    }

    /// The code of a source file: without comments (a doc comment may name what the code must not
    /// do) and without its test module, as `scripts/design-check.sh` reads a file.
    fn code_of(text: &str) -> impl Iterator<Item = &str> {
        text.lines()
            .take_while(|line| line.trim() != "#[cfg(test)]")
            .filter(|line| !line.trim_start().starts_with("//"))
    }

    /// Hands every `.rs` file under `dir` to `each`, with its text.
    fn visit(dir: &Path, each: &mut impl FnMut(&Path, &str)) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                visit(&path, each);
            } else if path.extension().is_some_and(|ext| ext == "rs")
                && let Ok(text) = std::fs::read_to_string(&path)
            {
                each(&path, &text);
            }
        }
    }

    /// Sample data comes back without a daemon: the demo answers from memory (task S.16).
    #[tokio::test]
    async fn the_demo_answers_without_a_daemon() {
        let backend: Arc<dyn Backend> = Arc::new(demo::Demo::new());
        let data = load(Arc::clone(&backend), vec![Request::Settings]).await;
        assert!(data.error.is_none(), "the demo failed: {:?}", data.error);
        assert_eq!(data.settings.appearance, demo::Demo::new().settings().appearance);
    }

    /// A send goes through in order and ends in fresh data.
    #[tokio::test]
    async fn a_send_asks_for_the_data_again() {
        let backend = Arc::new(demo::Demo::new());
        let request = Request::SetSetting { key: torro_ipc::SettingKey::Zoom, value: serde_json::json!(1.1) };
        let data = send(Arc::clone(&backend) as Arc<dyn Backend>, vec![request.clone()], vec![Request::Settings])
            .await
            .expect("the demo answers");
        assert!(data.error.is_none());
        assert_eq!(backend.sent(), vec![request], "the demo remembers what it was asked to change");
    }

    /// The app's part of an action can be wrapped, the rest stays as it is.
    #[test]
    fn map_app_only_touches_the_app_s_part() {
        let open: Action<u8> = Action::Open("reminders");
        assert_eq!(open.map_app(|n| n.to_string()), Action::Open("reminders"));
        let app: Action<u8> = Action::App(7);
        assert_eq!(app.map_app(|n| n.to_string()), Action::App("7".to_string()));
    }
}
