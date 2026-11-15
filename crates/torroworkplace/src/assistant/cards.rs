//! Cards in the chat (issue #225, task WZ.5; "Karten im Chat" and "Datei-Karte" in `docs/assistent.md`): where a
//! card comes from and what it shows.
//!
//! A card points at an item, it never copies it: the conversation keeps only [`EntryContent::Card`] with what it
//! points at ([`CardRef`]). The core adds one right after the tool line of a command that made an item ([`after`];
//! the first: `tools.create_pdf`, whose file the card shows). Engine A reads the core's own answer; engine B gets
//! the answer back from the agent as text, so an id read from it counts only when the register knows it for
//! exactly the client of the answer ([`of_agent`], `Host::knows_file`).
//!
//! What a card shows, `assistant.get_cards` fetches fresh from the core ([`resolve`]): with the rights of the
//! conversation's client (the assistant's, or the bot's), for a client within its own rights as well. A file shows
//! only while the register keeps it as made by that client, the file is intact, and that client may reach the
//! tools' files (`crate::tools::reaches_files`); otherwise [`Card::Unavailable`]. Nothing a model wrote ever
//! reaches a card, and nothing here logs a title, a path or content.
//!
//! A suggestion of a mail (issue #605, [`suggestion_after`]) becomes a card after `mail.extract_action` answered
//! one in a chat about a mail; the loop asks for it only there, in any other chat the tool line is all. It shows
//! while the mail still has that suggestion open, as `mail.list_suggestions` lists it with the rights of the
//! conversation's client ([`Held`], which the core fills, [`suggestion_mails`]); accepted, dismissed or out of
//! reach, it is [`Card::Unavailable`].
//!
//! The content cards (issue #784, `docs/inhaltskarten.md`) – a link, a mail, an event, a task, a draft, a contact
//! and a group of items – are looked up one by one ([`look`]) with what the core lends for it ([`Lookup`]), each
//! kind in its own module, and handed to [`resolve`] as [`Looked`]:
//!
//! - [`link`]: a web address (issue #785, #790), through [`crate::link_cards`],
//! - [`mail`]: a mail (issue #786),
//! - [`pim`]: an event and a task (issue #797),
//! - [`draft`]: a draft of a mail and its open approval (issue #797),
//! - [`contact`]: a contact (issue #789),
//! - [`group`]: several items found together (issue #786), each looked up as it would be alone.
//!
//! Every look answers [`Card::Unavailable`] for an item gone or out of the conversation client's reach, and says
//! nothing about why; none logs a title, an address or content.

use std::collections::HashMap;
use std::sync::Arc;

use torro_ipc::ClientInfo;
use torro_ipc::actions::Suggestion;
use torro_ipc::api::Rights;
use torro_ipc::assistant::{Card, CardEntry, CardRef, Entry, EntryContent, ItemRef};
use torro_ipc::tools::FileInfo;
use torro_ipc::{Request, Response};

use super::model::{Actor, Host};
use crate::account::github::GithubAccounts;
use crate::approvals::Approvals;
use crate::chat::ChatService;
use crate::contacts::ContactsService;
use crate::mail::service::MailService;
use crate::pim::{PimService, Reach};
use crate::tools::{Maker, Register};

pub(crate) mod contact;
pub(crate) mod draft;
pub(crate) mod group;
pub(crate) mod link;
pub(crate) mod mail;
pub(crate) mod pim;

/// The command whose file becomes a card.
const CREATE_PDF: &str = "tools.create_pdf";

/// The commands whose item becomes a card (issue #797): an event saved, a task saved, a draft saved.
const SAVE_EVENT: &str = "calendar.save_event";
const SAVE_TODO: &str = "tasks.save_todo";
const SAVE_DRAFT: &str = "mail.save_draft";

/// The card to add after the command `name` (its catalogue name, `tools.create_pdf`) answered `response`: a file
/// card after a PDF was made; an event card after `calendar.save_event`, a task card after `tasks.save_todo` and a
/// draft card after `mail.save_draft` (issue #797), each pointing at the item as the core answered it; `None` after
/// an error and after every other command.
pub fn after(name: &str, response: &Response) -> Option<EntryContent> {
    let card = match (name, response) {
        (CREATE_PDF, Response::File { file }) => return Some(file_card(file)),
        (SAVE_EVENT, Response::Event { event, .. }) => {
            CardRef::Event { uid: event.uid.clone(), recurrence_id: event.recurrence_id }
        }
        (SAVE_TODO, Response::Todo { todo }) => CardRef::Todo { uid: todo.uid.clone() },
        (SAVE_DRAFT, Response::MailDraft { draft }) => CardRef::Draft { id: draft.id.clone()? },
        _ => return None,
    };
    Some(EntryContent::Card { card })
}

/// What the core lends a look at one content card (issue #784): the conversation's client and its rights, and the
/// services its items live in. The core fills it (`Core::lookup`) for `assistant.get_cards`, `assistant.show_card`
/// and the tool lines of engine B ([`of_agent`], [`Host::looks_up`]); there is none without such a client (none set
/// up, the bot deleted), and then every content card is [`Card::Unavailable`].
pub struct Lookup<'a> {
    /// The conversation's client: the assistant's own, or the bot's. A look finds an item only as that client may
    /// (a draft only when it wrote it, `crate::mail::store::DraftAuthor`).
    pub client: &'a ClientInfo,
    /// That client's rights as they are now, for a client that asks cut to its own ([`Rights::common`]): what
    /// every look checks before it reads ([`Lookup::reach`]). Never wider than the client's own.
    pub rights: &'a Rights,
    /// The conversation's id, for what the core keeps per card (a link shown once, [`link::Shown`]).
    pub conversation: &'a str,
    /// The mail accounts: mails ([`mail`]) and drafts ([`draft`]).
    pub mail: &'a MailService,
    /// The calendars and task lists ([`pim`]).
    pub pim: &'a PimService,
    /// The address books ([`contact`]).
    pub contacts: &'a ContactsService,
    /// The chat accounts: the own Nextcloud server, its references and Settings › Privacy ([`link`]).
    pub chat: &'a Arc<ChatService>,
    /// The GitHub accounts, for an issue or pull request ([`link`], issue #790).
    pub github: &'a GithubAccounts,
    /// The approvals, for the open `mail.send_draft` of a draft ([`draft`]).
    pub approvals: &'a Approvals,
    /// The links shown once in the assistant's conversations ([`link`], `assistant.show_card`).
    pub shown: &'a link::Shown,
}

impl Lookup<'_> {
    /// The reach of the looks: the conversation's client with [`Lookup::rights`].
    pub fn reach(&self) -> Reach<'_> {
        Reach::Client(self.rights)
    }
}

/// What the content cards of a conversation show now, by the number of their entry ([`Entry::seq`]): what
/// [`look`] answered for each, for [`resolve`].
pub type Looked = HashMap<u64, Card>;

/// What the content card `card` of the entry `seq` shows now ([`Lookup`]): each kind from its module, a group with
/// each of its items looked up alone ([`look_item`]). `None` for a file and a suggestion, which [`resolve`] shows from
/// the register and the held suggestions; [`Card::Unavailable`] for a kind this build does not know.
pub async fn look(lookup: &Lookup<'_>, seq: u64, card: &CardRef) -> Option<Card> {
    Some(match card {
        CardRef::File { .. } | CardRef::Suggestion { .. } => return None,
        CardRef::Link { url, from } => link::look(lookup, seq, url, *from).await,
        CardRef::Mail { account, id } => mail::look(lookup, account, id).await,
        CardRef::Event { uid, recurrence_id } => pim::look_event(lookup, uid, recurrence_id.as_ref()).await,
        CardRef::Todo { uid } => pim::look_todo(lookup, uid).await,
        CardRef::Draft { id } => draft::look(lookup, id).await,
        CardRef::Contact { id } => contact::look(lookup, id).await,
        CardRef::Group { items, area, total } => group::look(lookup, items, *area, *total).await,
        CardRef::Unknown => Card::Unavailable,
    })
}

/// What the item `item` of a group shows now: as its card alone would ([`ItemRef::card`]); [`Card::Unavailable`] for
/// an item of another kind, so a group never shows a group.
pub async fn look_item(lookup: &Lookup<'_>, item: &ItemRef) -> Card {
    match item {
        ItemRef::Mail { account, id } => mail::look(lookup, account, id).await,
        ItemRef::Event { uid, recurrence_id } => pim::look_event(lookup, uid, recurrence_id.as_ref()).await,
        ItemRef::Todo { uid } => pim::look_todo(lookup, uid).await,
        ItemRef::Contact { id } => contact::look(lookup, id).await,
        ItemRef::Unknown => Card::Unavailable,
    }
}

/// Whether `card` is one a look found (not [`Card::Unavailable`] or [`Card::Unknown`]): what [`of_agent`] asks
/// through [`Host::looks_up`] before it takes a card from an agent's text.
pub fn found(card: &Card) -> bool {
    !matches!(card, Card::Unavailable | Card::Unknown)
}

/// The card to add after `request` answered `response` (issue #605): a suggestion card after
/// `mail.extract_action` read one out of a mail, pointing at that mail and the kind of action; `None` after an
/// error and after every other command. [`after`] keeps its own commands, this one the suggestions. The caller asks
/// only in a chat about a mail.
pub fn suggestion_after(request: &Request, response: &Response) -> Option<EntryContent> {
    match (request, response) {
        (Request::ExtractAction { account, id, kind, .. }, Response::Suggestion { .. }) => Some(EntryContent::Card {
            card: CardRef::Suggestion { account: account.clone(), id: id.clone(), action: kind.clone() },
        }),
        _ => None,
    }
}

/// The open suggestions of mails (issue #605): by `(account, id)` of the mail, as `mail.list_suggestions` lists
/// them for the conversation's client now. A mail that client may not read is not in it.
pub type Held = HashMap<(String, String), Vec<Suggestion>>;

/// The mails the suggestion cards among `entries` point at, `(account, id)`, each once in their order: what the
/// core looks up for [`Held`].
pub fn suggestion_mails(entries: &[Entry]) -> Vec<(String, String)> {
    let mut mails: Vec<(String, String)> = Vec::new();
    for entry in entries {
        if let EntryContent::Card { card: CardRef::Suggestion { account, id, .. } } = &entry.content {
            let mail = (account.clone(), id.clone());
            if !mails.contains(&mail) {
                mails.push(mail);
            }
        }
    }
    mails
}

/// [`after`] for engine B, whose agent hands over the core's answer as the tool result `text` (the answer's JSON,
/// as the MCP server writes it): a file card only when the register keeps the file as made by `actor`'s client
/// ([`Host::knows_file`]); a content card (an event, a task, a draft, issue #797) only when a look finds it for that
/// client now ([`Host::looks_up`]). An agent could hand over any text; an id the core does not know for this client
/// (made up, another client's, changed meanwhile) becomes no card.
pub(super) async fn of_agent(host: &dyn Host, actor: &Actor, name: &str, text: &str) -> Option<EntryContent> {
    let response = serde_json::from_str::<Response>(text).ok()?;
    let card = after(name, &response)?;
    let known = match &card {
        EntryContent::Card { card: CardRef::File { id } } => host.knows_file(actor, id).await,
        EntryContent::Card { card } => host.looks_up(actor, card).await,
        _ => false,
    };
    known.then_some(card)
}

/// The cards among `entries`, in their order, as they are now: a file the register keeps as made by `client` and
/// that is still intact ([`Register::get`]) shows ([`Card::File`]), a suggestion of a mail (issue #605) while its
/// mail has an open suggestion of its kind in `held` ([`Card::Suggestion`]), a content card (issue #784) as `looked`
/// has it for its entry ([`look`]); every other card is [`Card::Unavailable`]. `client`: the id of the
/// conversation's client when it may reach the tools' files now (the core decides, with its rights cut to the
/// asker's); `None` makes every file card unavailable. `held`: the open suggestions of the mails that client may read
/// now ([`Held`]). Reads the register and every file: call it on a blocking thread.
pub fn resolve(
    entries: &[Entry],
    register: &Register,
    client: Option<&str>,
    held: &Held,
    looked: &Looked,
) -> Vec<CardEntry> {
    entries
        .iter()
        .filter_map(|entry| match &entry.content {
            EntryContent::Card { card } => {
                let card = looked.get(&entry.seq).cloned().unwrap_or_else(|| shown(card, register, client, held));
                Some(CardEntry { seq: entry.seq, card })
            }
            _ => None,
        })
        .collect()
}

/// What the card `card` shows for `client` and the suggestions `held` ([`resolve`]); a content card that was not
/// looked up is [`Card::Unavailable`].
fn shown(card: &CardRef, register: &Register, client: Option<&str>, held: &Held) -> Card {
    match (card, client) {
        (CardRef::File { id }, Some(client)) => {
            register.get(id, Maker::Client(client)).map_or(Card::Unavailable, |entry| Card::File(entry.info()))
        }
        (CardRef::File { .. }, None) => Card::Unavailable,
        (CardRef::Suggestion { account, id, action }, _) => held
            .get(&(account.clone(), id.clone()))
            .and_then(|open| open.iter().find(|suggestion| suggestion.kind == *action))
            .map_or(Card::Unavailable, |suggestion| Card::Suggestion {
                account: account.clone(),
                id: id.clone(),
                suggestion: Box::new(suggestion.clone()),
            }),
        (_, _) => Card::Unavailable,
    }
}

/// The card of `file`.
fn file_card(file: &FileInfo) -> EntryContent {
    EntryContent::Card { card: CardRef::File { id: file.id.clone() } }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Mutex;

    use chrono::Utc;
    use torro_core::ai::{AiError, BoxFuture};
    use torro_ipc::ClientInfo;
    use torro_ipc::actions::SuggestionState;

    use super::*;
    use crate::assistant::ModelAccess;
    use crate::tools::files::tests::registered;

    /// A host whose register knows the file `known` for the client `client` and records what it was asked.
    struct Knowing {
        client: &'static str,
        known: String,
        asked: Mutex<Vec<(Actor, String)>>,
    }

    impl Host for Knowing {
        fn client(&self) -> Option<ClientInfo> {
            None
        }

        fn model<'a>(
            &'a self,
            _engine: &'a str,
            _model: Option<&'a str>,
        ) -> BoxFuture<'a, Result<ModelAccess, AiError>> {
            Box::pin(async { Err(AiError::NotFound("no model here".into())) })
        }

        fn run<'a>(&'a self, _request: Request, _engine: &'a str, _untrusted: bool) -> BoxFuture<'a, Response> {
            Box::pin(async { Response::Ok })
        }

        fn approval<'a>(&'a self, _id: &'a str) -> BoxFuture<'a, Response> {
            Box::pin(async { Response::Ok })
        }

        fn knows_file<'a>(&'a self, actor: &'a Actor, id: &'a str) -> BoxFuture<'a, bool> {
            self.asked.lock().expect("lock").push((actor.clone(), id.to_string()));
            let mine = match actor {
                Actor::Assistant => self.client == "assistant",
                Actor::Bot(client) => client == self.client,
                Actor::Delegated(delegation) => delegation.client == self.client,
            };
            Box::pin(async move { mine && id == self.known })
        }
    }

    fn file(id: &str) -> FileInfo {
        FileInfo {
            id: id.into(),
            name: "Report.pdf".into(),
            path: "/home/anna/Desktop/Report.pdf".into(),
            title: "Report".into(),
            template: torro_core::settings::PdfTemplateId::Report,
            size: 1_234,
            created: Utc::now(),
        }
    }

    fn card(id: &str) -> EntryContent {
        EntryContent::Card { card: CardRef::File { id: id.into() } }
    }

    fn at(seq: u64, content: EntryContent) -> Entry {
        Entry { seq, time: Utc::now(), content }
    }

    #[test]
    fn a_made_pdf_gets_a_card_and_nothing_else_does() {
        let made = Response::File { file: file("1a2b3c4d") };
        assert_eq!(after("tools.create_pdf", &made), Some(card("1a2b3c4d")));
        let failed = Response::Error { code: "forbidden".into(), message: "off".into() };
        assert_eq!(after("tools.create_pdf", &failed), None, "no card after an error");
        assert_eq!(after("tools.open_file", &Response::Ok), None);
        assert_eq!(after("tools.open_file", &made), None, "only the command that made it");
        assert_eq!(after("calendar.list_events", &Response::Ok), None);
    }

    /// Engine B (issue #225): the id in the agent's tool result becomes a card only when the register knows it for
    /// the answer's client.
    #[tokio::test]
    async fn an_agents_tool_line_gets_a_card_only_for_a_file_of_its_client() {
        let host = Knowing { client: "assistant", known: "1a2b3c4d".into(), asked: Mutex::new(Vec::new()) };
        let text = serde_json::to_string(&Response::File { file: file("1a2b3c4d") }).expect("json");
        let assistant = Actor::Assistant;
        assert_eq!(of_agent(&host, &assistant, "tools.create_pdf", &text).await, Some(card("1a2b3c4d")));
        assert_eq!(host.asked.lock().expect("lock").as_slice(), [(Actor::Assistant, "1a2b3c4d".to_string())]);

        let made_up = serde_json::to_string(&Response::File { file: file("ffffffff") }).expect("json");
        assert_eq!(of_agent(&host, &assistant, "tools.create_pdf", &made_up).await, None, "unknown to the register");
        let bot = Actor::Bot("b0b0b0b0".into());
        assert_eq!(of_agent(&host, &bot, "tools.create_pdf", &text).await, None, "another client's file");
        for broken in ["", "Done", "forbidden: switched off", r#"{"result":"ok"}"#] {
            assert_eq!(of_agent(&host, &assistant, "tools.create_pdf", broken).await, None, "{broken:?}");
        }
        assert_eq!(of_agent(&host, &assistant, "tools.open_file", &text).await, None);
        assert_eq!(host.asked.lock().expect("lock").len(), 3, "the register is asked only for a file card");
    }

    fn saved(kind: &str) -> Response {
        let json = match kind {
            "event" => serde_json::json!({"result": "event", "event": {
                "uid": "e1", "collection": "/cal/work/", "summary": "Team sync",
                "start": {"datetime": "2026-11-16T13:00:00Z"}, "recurring": false
            }}),
            "todo" => serde_json::json!({"result": "todo", "todo": {
                "uid": "t1", "collection": "/tasks/work/", "summary": "Send the offer", "completed": false
            }}),
            _ => {
                serde_json::json!({"result": "mail-draft", "draft": {"id": "m:9", "account": "mail:anna@example.org"}})
            }
        };
        serde_json::from_value(json).expect("a saved item")
    }

    /// Issue #797: an event, a task and a draft the assistant saved get a card that points at them.
    #[test]
    fn a_saved_event_task_or_draft_gets_a_card() {
        let pointed = |card: CardRef| Some(EntryContent::Card { card });
        assert_eq!(
            after("calendar.save_event", &saved("event")),
            pointed(CardRef::Event { uid: "e1".into(), recurrence_id: None })
        );
        assert_eq!(after("tasks.save_todo", &saved("todo")), pointed(CardRef::Todo { uid: "t1".into() }));
        assert_eq!(after("mail.save_draft", &saved("draft")), pointed(CardRef::Draft { id: "m:9".into() }));
        assert_eq!(after("calendar.list_events", &saved("event")), None, "only the command that saved it");
    }

    /// A host whose looks find what `found` says, and record what they were asked.
    struct Looking {
        found: bool,
        asked: Mutex<Vec<CardRef>>,
    }

    impl Host for Looking {
        fn client(&self) -> Option<ClientInfo> {
            None
        }

        fn model<'a>(
            &'a self,
            _engine: &'a str,
            _model: Option<&'a str>,
        ) -> BoxFuture<'a, Result<ModelAccess, AiError>> {
            Box::pin(async { Err(AiError::NotFound("no model here".into())) })
        }

        fn run<'a>(&'a self, _request: Request, _engine: &'a str, _untrusted: bool) -> BoxFuture<'a, Response> {
            Box::pin(async { Response::Ok })
        }

        fn approval<'a>(&'a self, _id: &'a str) -> BoxFuture<'a, Response> {
            Box::pin(async { Response::Ok })
        }

        fn looks_up<'a>(&'a self, _actor: &'a Actor, card: &'a CardRef) -> BoxFuture<'a, bool> {
            self.asked.lock().expect("lock").push(card.clone());
            Box::pin(async move { self.found })
        }
    }

    /// Issue #784: engine B's agent hands over an event as text; it becomes a card only when a look finds it for the
    /// answer's client.
    #[tokio::test]
    async fn an_agents_saved_event_gets_a_card_only_when_a_look_finds_it() {
        let text = serde_json::to_string(&saved("event")).expect("json");
        let event = CardRef::Event { uid: "e1".into(), recurrence_id: None };
        let found = Looking { found: true, asked: Mutex::new(Vec::new()) };
        let card = of_agent(&found, &Actor::Assistant, "calendar.save_event", &text).await;
        assert_eq!(card, Some(EntryContent::Card { card: event.clone() }));
        assert_eq!(found.asked.lock().expect("lock").as_slice(), [event]);
        let missing = Looking { found: false, asked: Mutex::new(Vec::new()) };
        assert_eq!(of_agent(&missing, &Actor::Assistant, "calendar.save_event", &text).await, None, "made up");
    }

    /// Issue #784: a content card shows what its look found; one not looked up is unavailable.
    #[test]
    fn content_cards_show_what_was_looked_up() {
        let dir = tempfile::tempdir().expect("temp dir");
        let register = Register::in_dir(Some(dir.path().join("tools")));
        let todo = |uid: &str| EntryContent::Card { card: CardRef::Todo { uid: uid.into() } };
        let entries = vec![at(1, todo("t1")), at(2, todo("t2")), at(3, EntryContent::Card { card: CardRef::Unknown })];
        let mut looked = Looked::new();
        looked.insert(1, Card::Group { items: Vec::new(), area: torro_ipc::api::Area::Tasks, total: Some(12) });
        let cards = resolve(&entries, &register, Some("a55e5517"), &Held::new(), &looked);
        assert_eq!(cards[0].card, looked[&1]);
        assert_eq!((&cards[1].card, &cards[2].card), (&Card::Unavailable, &Card::Unavailable));
        assert!(found(&cards[0].card) && !found(&Card::Unknown) && !found(&Card::Unavailable));
    }

    #[test]
    fn cards_show_a_file_of_the_conversations_client_while_it_is_intact() {
        let dir = tempfile::tempdir().expect("temp dir");
        let register = Register::in_dir(Some(dir.path().join("tools")));
        let mine = registered(&register, dir.path(), "Mine.pdf", b"%PDF mine", Maker::Client("a55e5517"));
        let theirs = registered(&register, dir.path(), "Theirs.pdf", b"%PDF theirs", Maker::Client("b0b0b0b0"));
        let entries = vec![
            at(1, EntryContent::Stopped),
            at(2, card(&mine.id)),
            at(3, EntryContent::Error { code: "error".into(), message: "x".into() }),
            at(4, card(&theirs.id)),
            at(5, card("00000000")),
        ];

        let cards = resolve(&entries, &register, Some("a55e5517"), &Held::new(), &Looked::new());
        let seqs: Vec<u64> = cards.iter().map(|card| card.seq).collect();
        assert_eq!(seqs, [2, 4, 5], "the card entries only, in order");
        assert_eq!(cards[0].card, Card::File(mine.info()));
        assert_eq!((&cards[1].card, &cards[2].card), (&Card::Unavailable, &Card::Unavailable));

        let out_of_reach = resolve(&entries, &register, None, &Held::new(), &Looked::new());
        assert!(out_of_reach.iter().all(|card| card.card == Card::Unavailable), "without the rights");

        fs::remove_file(&mine.path).expect("removed");
        assert_eq!(
            resolve(&entries, &register, Some("a55e5517"), &Held::new(), &Looked::new())[0].card,
            Card::Unavailable,
            "deleted"
        );
    }

    fn suggestion(kind: &str) -> Suggestion {
        Suggestion {
            kind: kind.into(),
            state: SuggestionState::Ready,
            task: None,
            event: None,
            reason: Some("The sender proposes a meeting".into()),
        }
    }

    fn extract(kind: &str) -> Request {
        Request::ExtractAction {
            account: "mail:anna@example.org".into(),
            id: "m:4".into(),
            kind: kind.into(),
            lists: Vec::new(),
            focus: Some("the meeting on Thursday".into()),
        }
    }

    fn pointed(id: &str, action: &str) -> EntryContent {
        EntryContent::Card {
            card: CardRef::Suggestion { account: "mail:anna@example.org".into(), id: id.into(), action: action.into() },
        }
    }

    /// Issue #605: a suggestion read out of a mail gets a card that points at the mail and its kind; nothing else
    /// does, and `after` stays as it was.
    #[test]
    fn a_suggestion_read_out_of_a_mail_gets_a_card() {
        let read = Response::Suggestion { suggestion: suggestion("event") };
        assert_eq!(suggestion_after(&extract("event"), &read), Some(pointed("m:4", "event")));
        assert_eq!(after("mail.extract_action", &read), None, "after keeps its own commands");
        let failed = Response::Error { code: "not-connected".into(), message: "no model".into() };
        assert_eq!(suggestion_after(&extract("event"), &failed), None, "no card after an error");
        let listed = Request::ListSuggestions { account: "mail:anna@example.org".into(), id: "m:4".into() };
        assert_eq!(suggestion_after(&listed, &read), None, "only the command that read it");
    }

    /// Issue #605: a suggestion card shows while its mail has that suggestion open for the conversation's client,
    /// else it is unavailable; the mails to look up are named once each.
    #[test]
    fn a_suggestion_card_shows_while_the_suggestion_is_open() {
        let dir = tempfile::tempdir().expect("temp dir");
        let register = Register::in_dir(Some(dir.path().join("tools")));
        let entries = vec![
            at(1, pointed("m:4", "event")),
            at(2, pointed("m:4", "task")),
            at(3, pointed("m:9", "event")),
            at(4, pointed("m:4", "event")),
        ];
        assert_eq!(
            suggestion_mails(&entries),
            [("mail:anna@example.org".to_string(), "m:4".to_string()), ("mail:anna@example.org".into(), "m:9".into())]
        );
        let mut held = Held::new();
        held.insert(("mail:anna@example.org".into(), "m:4".into()), vec![suggestion("event")]);
        let cards = resolve(&entries, &register, None, &held, &Looked::new());
        assert_eq!(cards.iter().map(|card| card.seq).collect::<Vec<_>>(), [1, 2, 3, 4]);
        let shown = Card::Suggestion {
            account: "mail:anna@example.org".into(),
            id: "m:4".into(),
            suggestion: Box::new(suggestion("event")),
        };
        assert_eq!(cards[0].card, shown);
        assert_eq!(cards[1].card, Card::Unavailable, "no open task suggestion: accepted, dismissed or never");
        assert_eq!(cards[2].card, Card::Unavailable, "a mail out of reach");
        assert_eq!(cards[3].card, shown);
        assert!(
            resolve(&entries, &register, Some("a55e5517"), &Held::new(), &Looked::new())
                .iter()
                .all(|card| card.card == Card::Unavailable)
        );
    }
}
