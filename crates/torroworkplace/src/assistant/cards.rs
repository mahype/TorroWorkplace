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

use torro_ipc::Response;
use torro_ipc::assistant::{Card, CardEntry, CardRef, Entry, EntryContent};
use torro_ipc::tools::FileInfo;

use super::model::{Actor, Host};
use crate::tools::{Maker, Register};

/// The command whose file becomes a card.
const CREATE_PDF: &str = "tools.create_pdf";

/// The card to add after the command `name` (its catalogue name, `tools.create_pdf`) answered `response`: a file
/// card after a PDF was made; `None` after an error and after every other command.
pub fn after(name: &str, response: &Response) -> Option<EntryContent> {
    match (name, response) {
        (CREATE_PDF, Response::File { file }) => Some(file_card(file)),
        _ => None,
    }
}

/// [`after`] for engine B, whose agent hands over the core's answer as the tool result `text` (the answer's JSON,
/// as the MCP server writes it): the card only when the register keeps the file as made by `actor`'s client
/// ([`Host::knows_file`]). An agent could hand over any text; an id the register does not know for this client
/// (made up, another client's, changed meanwhile) becomes no card.
pub(super) async fn of_agent(host: &dyn Host, actor: &Actor, name: &str, text: &str) -> Option<EntryContent> {
    let response = serde_json::from_str::<Response>(text).ok()?;
    let card = after(name, &response)?;
    let known = match &card {
        EntryContent::Card { card: CardRef::File { id } } => host.knows_file(actor, id).await,
        _ => false,
    };
    known.then_some(card)
}

/// The cards among `entries`, in their order, as they are now: a file the register keeps as made by `client` and
/// that is still intact ([`Register::get`]) shows ([`Card::File`]), every other card is [`Card::Unavailable`].
/// `client`: the id of the conversation's client when it may reach the tools' files now (the core decides, with its
/// rights cut to the asker's); `None` makes every card unavailable. Reads the register and every file: call it on
/// a blocking thread.
pub fn resolve(entries: &[Entry], register: &Register, client: Option<&str>) -> Vec<CardEntry> {
    entries
        .iter()
        .filter_map(|entry| match &entry.content {
            EntryContent::Card { card } => Some(CardEntry { seq: entry.seq, card: shown(card, register, client) }),
            _ => None,
        })
        .collect()
}

/// What the card `card` shows for `client` ([`resolve`]).
fn shown(card: &CardRef, register: &Register, client: Option<&str>) -> Card {
    match (card, client) {
        (CardRef::File { id }, Some(client)) => {
            register.get(id, Maker::Client(client)).map_or(Card::Unavailable, |entry| Card::File(entry.info()))
        }
        (CardRef::File { .. }, None) => Card::Unavailable,
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
    use torro_ipc::{ClientInfo, Request};

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

        let cards = resolve(&entries, &register, Some("a55e5517"));
        let seqs: Vec<u64> = cards.iter().map(|card| card.seq).collect();
        assert_eq!(seqs, [2, 4, 5], "the card entries only, in order");
        assert_eq!(cards[0].card, Card::File(mine.info()));
        assert_eq!((&cards[1].card, &cards[2].card), (&Card::Unavailable, &Card::Unavailable));

        let out_of_reach = resolve(&entries, &register, None);
        assert!(out_of_reach.iter().all(|card| card.card == Card::Unavailable), "without the rights");

        fs::remove_file(&mine.path).expect("removed");
        assert_eq!(resolve(&entries, &register, Some("a55e5517"))[0].card, Card::Unavailable, "deleted");
    }
}
