//! The link card of the assistant (issue #785, #790, `docs/inhaltskarten.md` "Webseite", "Datenschutz"): a web
//! address in an answer, as the own Nextcloud resolves it, or the question whether its provider's content may load.
//!
//! - **Added** after an answer ([`after_answer`]): its first [`MAX_LINK_CARDS`] addresses, each with where it came
//!   from ([`LinkOrigin`]), as the core saw it.
//! - **Shown** ([`look`]) through [`crate::link_cards`], like a link in the chat with people: the own Nextcloud
//!   resolves the page (decision 1 of issue #784: one way, so a page looks the same in every window; the core never
//!   fetches a page itself). Without a Nextcloud account a link stays a link.
//! - **Settings › Privacy** decides for an address the person wrote or a command read ([`LinkOrigin::Person`],
//!   [`LinkOrigin::Read`]); an address the model wrote always asks first, also at "Always show", since an injected
//!   instruction could make the model put what it read into an address.
//! - **Show once** ([`show_once`], `assistant.show_card`, a person's click): the card is fetched once and held in
//!   memory ([`Shown`]); Settings › Privacy stays as it is.
//!
//! The kinds of strand 2c (files of the own server, quoted chat messages, GitHub) stay links here until
//! `crate::link_cards::{nextcloud,chat,github}` can resolve them (marked "Must (strand 2c)"). Nothing here logs an
//! address.

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use torro_core::i18n::tr;
use torro_core::privacy::{Decision, Privacy};
use torro_ipc::Response;
use torro_ipc::assistant::{Card, CardRef, EntryContent, LinkCard, LinkOrigin};

use super::Lookup;
use crate::chat::ChatService;
use crate::link_cards::web::{self, Resolved};
use crate::link_cards::{self, LinkKind};

/// The most link cards [`Shown`] holds; the oldest go first.
pub const SHOWN_KEPT: usize = 2000;

/// How long a page whose server could not be reached is not asked again, so a server that is offline does not slow
/// every `assistant.get_cards` down.
const RETRY_AFTER: Duration = Duration::from_secs(60);

/// What [`Shown`] holds for one link card.
#[derive(Debug, Clone)]
enum Held {
    /// Resolved by the server of `account` (its card is `None` when the server knows none, so it is not asked again).
    Resolved { account: String, resolved: Resolved },
    /// The server could not be reached then.
    Failed(Instant),
}

/// The link cards of the assistant's conversations the core holds, by conversation and entry number: those a person
/// had shown once (`assistant.show_card`, "Show once") and those that loaded by themselves. In memory only, so after
/// a restart a page loads again, and the question for one shown once comes again. The core holds one
/// ([`Lookup::shown`]); Settings › Privacy is never changed by it.
#[derive(Debug, Default)]
pub struct Shown {
    cards: Mutex<Kept>,
}

#[derive(Debug, Default)]
struct Kept {
    by: HashMap<(String, u64), Held>,
    /// The keys of `by`, the oldest first.
    order: VecDeque<(String, u64)>,
}

impl Shown {
    fn lock(&self) -> std::sync::MutexGuard<'_, Kept> {
        self.cards.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The card held for the entry `seq` of `conversation`, if any: shown once or loaded by itself.
    pub fn get(&self, conversation: &str, seq: u64) -> Option<LinkCard> {
        match self.held(conversation, seq)? {
            Held::Resolved { resolved, .. } => resolved.card,
            Held::Failed(_) => None,
        }
    }

    /// Keeps `resolved`, the card of the entry `seq` of `conversation` as the server of `account` resolved it
    /// (`assistant.show_card`, or loaded by itself); the oldest go beyond [`SHOWN_KEPT`].
    pub fn keep(&self, conversation: &str, seq: u64, account: &str, resolved: Resolved) {
        self.put(conversation, seq, Held::Resolved { account: account.to_owned(), resolved });
    }

    /// The account and the picture's id of the card held for the entry `seq` of `conversation`, when it has a
    /// picture (`assistant.get_card_picture`).
    pub fn picture(&self, conversation: &str, seq: u64) -> Option<(String, String)> {
        match self.held(conversation, seq)? {
            Held::Resolved { account, resolved } => Some((account, resolved.image?)),
            Held::Failed(_) => None,
        }
    }

    fn held(&self, conversation: &str, seq: u64) -> Option<Held> {
        self.lock().by.get(&(conversation.to_owned(), seq)).cloned()
    }

    fn put(&self, conversation: &str, seq: u64, held: Held) {
        let mut kept = self.lock();
        let key = (conversation.to_owned(), seq);
        if kept.by.insert(key.clone(), held).is_none() {
            kept.order.push_back(key);
        }
        while kept.order.len() > SHOWN_KEPT {
            if let Some(oldest) = kept.order.pop_front() {
                kept.by.remove(&oldest);
            }
        }
    }
}

/// The link cards to add after the answer `answer` (issue #785): one [`CardRef::Link`] for each of its first
/// [`MAX_LINK_CARDS`] `https` addresses ([`link_cards::links`]), in their order, with where the address came from:
/// [`LinkOrigin::Person`] when it stands whole in one of `person` (the person's own messages of the conversation,
/// never a client's), else [`LinkOrigin::Read`] when it stands whole in one of `read` (what commands of the
/// conversation answered, as the model read it), else [`LinkOrigin::Model`] ([`link_cards::stands_in`]: a part or a
/// longer form of such an address is the model's). Empty for an answer without addresses.
///
/// [`MAX_LINK_CARDS`]: torro_ipc::assistant::MAX_LINK_CARDS
pub fn after_answer(answer: &str, person: &[&str], read: &[&str]) -> Vec<EntryContent> {
    let stands = |texts: &[&str], url: &str| texts.iter().any(|text| link_cards::stands_in(url, text));
    link_cards::links(answer)
        .into_iter()
        .map(|url| {
            let from = if stands(person, &url) {
                LinkOrigin::Person
            } else if stands(read, &url) {
                LinkOrigin::Read
            } else {
                LinkOrigin::Model
            };
            EntryContent::Card { card: CardRef::Link { url, from } }
        })
        .collect()
}

/// What the link card of the entry `seq` for the address `url` shows now ([`Card::Link`]), for the conversation of
/// `lookup` ([`look_in`] with its chat accounts and [`Shown`]).
pub async fn look(lookup: &Lookup<'_>, seq: u64, url: &str, from: LinkOrigin) -> Card {
    // Must (strand 2c, issue #790): GitHub items resolve through `lookup.github`, the kinds of the own server and
    // GitHub within the conversation client's rights (`lookup.reach()`), at the marked place of [`step`].
    let _ = (lookup.github, lookup.reach());
    look_in(lookup.chat, lookup.shown, lookup.conversation, seq, url, from).await
}

/// What the link card of the entry `seq` of `conversation` for `url` shows now, through the first Nextcloud account
/// of `chat` ([`step`] decides):
///
/// - a provider Settings › Privacy blocks, or any page at "Never show": a link, also for a card held from before,
/// - a card held in `shown` (shown once, or loaded before): as it was fetched,
/// - a page of another server from the person or read ([`LinkOrigin::Person`], [`LinkOrigin::Read`]) whose provider
///   Settings › Privacy lets load: fetched now through the own Nextcloud and held; when the server cannot be reached,
///   a link, asked again after [`RETRY_AFTER`],
/// - one whose provider is not asked about yet, and every address [`LinkOrigin::Model`] wrote ("Always show" too):
///   `ask` with the provider,
/// - a link to the own server, and the kinds of strand 2c: a link for now.
///
/// Without a Nextcloud account a link stays a link. Never fetches the page itself, logs no address.
pub async fn look_in(
    chat: &Arc<ChatService>,
    shown: &Shown,
    conversation: &str,
    seq: u64,
    url: &str,
    from: LinkOrigin,
) -> Card {
    let link = || Card::Link { url: url.to_owned(), card: None, ask: None };
    let Some(account) = chat.accounts().await.into_iter().next() else { return link() };
    let Ok(remote) = chat.remote(&account).await else { return link() };
    let held = shown.held(conversation, seq);
    match step(&chat.privacy(), url, &remote.server(), from, held.as_ref(), Instant::now()) {
        Step::Link => link(),
        Step::Held => Card::Link { url: url.to_owned(), card: shown.get(conversation, seq).map(Box::new), ask: None },
        Step::Ask(provider) => Card::Link { url: url.to_owned(), card: None, ask: Some(provider) },
        Step::Load => match web::resolve(&remote, url).await {
            Ok(resolved) => {
                let card = resolved.card.clone();
                shown.keep(conversation, seq, &account, resolved);
                Card::Link { url: url.to_owned(), card: card.map(Box::new), ask: None }
            }
            Err(_) => {
                shown.put(conversation, seq, Held::Failed(Instant::now()));
                link()
            }
        },
    }
}

/// `assistant.show_card` for the link card of the entry `seq` of `conversation` with the address `url` (a person's
/// click, "Show once"): fetched once through the first Nextcloud account of `chat`, whatever server it is on and
/// wherever the address came from, and held in `shown`; Settings › Privacy stays as it is. `Err` with the answer:
/// `not-connected` without a Nextcloud account, `unsupported` for a provider Settings › Privacy blocks and while it
/// says "Never show", the server's error when it cannot resolve it now.
pub async fn show_once(
    chat: &Arc<ChatService>,
    shown: &Shown,
    conversation: &str,
    seq: u64,
    url: &str,
) -> Result<(), Response> {
    let (account, remote) = nextcloud(chat).await?;
    if web::turned_off(&chat.privacy(), url, &remote.server()) {
        return Err(refused("unsupported", tr("Content from this provider is turned off in Settings › Privacy.")));
    }
    let resolved = web::resolve(&remote, url).await.map_err(|err| crate::chat::media::file_refusal(&err))?;
    shown.keep(conversation, seq, &account, resolved);
    Ok(())
}

/// `assistant.get_card_picture` for the entry `seq` of `conversation`, once the caller checked that its card shows
/// now with a picture ([`look`]): the picture of the card held in `shown`, from the server that resolved it, as
/// `chat.get_preview` with `card` fetches it ([`ChatService::card_picture_of`]: never an SVG). `not-found` without
/// one, `not-connected` when that account is gone.
pub async fn picture(chat: &Arc<ChatService>, shown: &Shown, conversation: &str, seq: u64) -> Response {
    let Some((account, image)) = shown.picture(conversation, seq) else {
        return refused("not-found", tr("This card has no picture."));
    };
    match chat.remote(&account).await {
        Ok(remote) => chat.card_picture_of(&account, &remote, &image).await,
        Err(_) => not_connected(),
    }
}

/// The first Nextcloud account of `chat` and its server; `not-connected` without one.
async fn nextcloud(chat: &Arc<ChatService>) -> Result<(String, crate::chat::remote::Remote), Response> {
    let account = chat.accounts().await.into_iter().next().ok_or_else(not_connected)?;
    let remote = chat.remote(&account).await.map_err(|_| not_connected())?;
    Ok((account, remote))
}

fn not_connected() -> Response {
    refused("not-connected", tr("Connect a Nextcloud account to show cards of links."))
}

fn refused(code: &str, message: &str) -> Response {
    Response::Error { code: code.into(), message: message.into() }
}

/// What a link card does now ([`step`]).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    /// Stays a link: no card, no question.
    Link,
    /// Shows the card held in [`Shown`] (none when the server knows none: a link).
    Held,
    /// Asks whether the provider's content may load.
    Ask(torro_chat::ForeignProvider),
    /// Is fetched now through the own Nextcloud.
    Load,
}

/// What the link card for `url` does with Settings › Privacy `privacy`, the own Nextcloud's address `server`, the
/// origin `from` and what `held` keeps of it at `now` ([`look_in`] tells it in words). The rule of decision 1: only
/// an address of the person or one read loads by itself; one the model wrote asks, at "Always show" too.
fn step(privacy: &Privacy, url: &str, server: &str, from: LinkOrigin, held: Option<&Held>, now: Instant) -> Step {
    let own = torro_nextcloud::same_origin(url, server);
    if !own && web::turned_off(privacy, url, server) {
        return Step::Link;
    }
    match held {
        Some(Held::Resolved { .. }) => return Step::Held,
        Some(Held::Failed(at)) if now.duration_since(*at) < RETRY_AFTER => return Step::Link,
        _ => {}
    }
    match link_cards::classify(url, Some(server)) {
        LinkKind::Web => {}
        // Must (strand 2c, issue #790): a file of the own server, a quoted chat message and a GitHub item get their
        // cards from `link_cards::{nextcloud,chat,github}` within the conversation client's rights
        // (`Lookup::rights`), which have no function to resolve one yet: until then they stay links, and a GitHub
        // item without a connected account goes on as the page it is.
        LinkKind::NextcloudFile(_) | LinkKind::ChatMessage { .. } | LinkKind::Github { .. } => return Step::Link,
    }
    // Any other page of the own server stays a link: its references could tell what the conversation's client may
    // not read (a file, a card of a board), and no right of the client covers them.
    if own {
        return Step::Link;
    }
    let ask = |provider: torro_core::privacy::Provider| {
        Step::Ask(torro_chat::ForeignProvider { key: provider.key, name: provider.name })
    };
    let provider = web::provider(url);
    match (privacy.decide(&provider), from) {
        (Decision::Skip, _) => Step::Link,
        (Decision::Ask(provider), _) => ask(provider),
        (Decision::Load, LinkOrigin::Person | LinkOrigin::Read) => Step::Load,
        (Decision::Load, LinkOrigin::Model) => torro_core::privacy::provider_of(&provider).map_or(Step::Link, ask),
    }
}

#[cfg(test)]
mod tests {
    use torro_core::privacy::{ForeignContent, ProviderChoice};

    use super::*;
    use crate::chat::fake::Server;

    const SERVER: &str = "https://cloud.example.org";
    const PAGE: &str = "https://blog.example.net/post";

    fn always() -> Privacy {
        Privacy { content: ForeignContent::Always, ..Privacy::default() }
    }

    fn asks(step: &Step) -> Option<&str> {
        match step {
            Step::Ask(provider) => Some(provider.key.as_str()),
            _ => None,
        }
    }

    /// Decision 1 of issue #784: an address the model wrote asks at "Always show" too; one the person wrote or a
    /// command read loads.
    #[test]
    fn an_address_of_the_model_asks_at_always() {
        let now = Instant::now();
        let privacy = always();
        assert_eq!(asks(&step(&privacy, PAGE, SERVER, LinkOrigin::Model, None, now)), Some("example.net"));
        assert_eq!(step(&privacy, PAGE, SERVER, LinkOrigin::Person, None, now), Step::Load);
        assert_eq!(step(&privacy, PAGE, SERVER, LinkOrigin::Read, None, now), Step::Load);
        let mut allowed = Privacy::default();
        allowed.providers.insert("example.net".into(), ProviderChoice::Allowed);
        assert_eq!(asks(&step(&allowed, PAGE, SERVER, LinkOrigin::Model, None, now)), Some("example.net"));
        assert_eq!(step(&allowed, PAGE, SERVER, LinkOrigin::Person, None, now), Step::Load);
        let asked = Privacy::default();
        assert_eq!(asks(&step(&asked, PAGE, SERVER, LinkOrigin::Person, None, now)), Some("example.net"));
    }

    /// Issue #672 as in the chat with people: blocked and "Never show" give neither card nor question, a card held
    /// from before too; a held card shows; the own server stays a link here.
    #[test]
    fn blocked_never_held_and_own() {
        let now = Instant::now();
        let mut blocked = always();
        blocked.providers.insert("example.net".into(), ProviderChoice::Blocked);
        blocked.content = ForeignContent::Ask;
        let card = LinkCard { url: PAGE.into(), title: "A post".into(), ..LinkCard::default() };
        let held = Held::Resolved { account: "nc".into(), resolved: Resolved { card: Some(card), image: None } };
        assert_eq!(step(&blocked, PAGE, SERVER, LinkOrigin::Person, Some(&held), now), Step::Link);
        let never = Privacy { content: ForeignContent::Never, ..Privacy::default() };
        assert_eq!(step(&never, PAGE, SERVER, LinkOrigin::Model, None, now), Step::Link);
        assert_eq!(step(&Privacy::default(), PAGE, SERVER, LinkOrigin::Model, Some(&held), now), Step::Held);
        let failed = Held::Failed(now);
        assert_eq!(step(&always(), PAGE, SERVER, LinkOrigin::Person, Some(&failed), now), Step::Link);
        let later = now + RETRY_AFTER;
        assert_eq!(step(&always(), PAGE, SERVER, LinkOrigin::Person, Some(&failed), later), Step::Load);
        let own = "https://cloud.example.org/apps/deck/card/4";
        assert_eq!(step(&always(), own, SERVER, LinkOrigin::Person, None, now), Step::Link);
    }

    /// Issue #785: three addresses give three cards, the fourth none; each says where it came from.
    #[test]
    fn three_links_give_three_cards_and_the_fourth_none() {
        let answer = "See https://a.example.org/x, https://b.example.org/y, https://c.example.org/z and \
                      https://d.example.org/w.";
        let person = ["Look at https://a.example.org/x please"];
        let read = [r#"{"body":"Offer: https://b.example.org/y\nBye"}"#];
        let cards = after_answer(answer, &person, &read);
        let refs: Vec<(String, LinkOrigin)> = cards
            .into_iter()
            .map(|card| match card {
                EntryContent::Card { card: CardRef::Link { url, from } } => (url, from),
                other => panic!("a link card: {other:?}"),
            })
            .collect();
        assert_eq!(
            refs,
            [
                ("https://a.example.org/x".to_string(), LinkOrigin::Person),
                ("https://b.example.org/y".to_string(), LinkOrigin::Read),
                ("https://c.example.org/z".to_string(), LinkOrigin::Model),
            ]
        );
        assert!(after_answer("No address here", &person, &read).is_empty());
    }

    fn chat(privacy: Privacy) -> (Arc<ChatService>, Arc<Server>) {
        let server = Arc::new(Server::default());
        let chat = ChatService::fixed(vec![("nextcloud:anna@cloud.example.org".into(), Arc::clone(&server))]);
        chat.set_privacy(Arc::new(move || privacy.clone()));
        (Arc::new(chat), server)
    }

    fn png() -> Vec<u8> {
        let mut bytes = Vec::new();
        image::DynamicImage::new_rgb8(4, 3)
            .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
            .expect("a PNG");
        bytes
    }

    /// Issue #785: a page the person wrote loads once and is held; one the model wrote asks, and after "Show once"
    /// shows with its picture; Settings › Privacy is not changed by it.
    #[tokio::test]
    async fn a_page_loads_once_and_show_once_fetches_one_that_asks() {
        let (chat, server) = chat(always());
        let reference =
            torro_nextcloud::Reference { title: "A post".into(), description: None, image: Some("p".into()) };
        server.state().references.insert(PAGE.into(), reference);
        let drawn = crate::chat::remote::Picture { bytes: png(), content_type: "image/png".into() };
        server.state().card_pictures.insert("p".into(), drawn);
        let shown = Shown::default();
        let title = |card: &Card| match card {
            Card::Link { card: Some(card), .. } => Some(card.title.clone()),
            _ => None,
        };
        let by_person = look_in(&chat, &shown, "c1", 2, PAGE, LinkOrigin::Person).await;
        assert_eq!(title(&by_person).as_deref(), Some("A post"));
        look_in(&chat, &shown, "c1", 2, PAGE, LinkOrigin::Person).await;
        assert_eq!(server.count("resolve"), 1, "held");

        let by_model = look_in(&chat, &shown, "c1", 3, PAGE, LinkOrigin::Model).await;
        assert!(
            matches!(&by_model, Card::Link { card: None, ask: Some(provider), .. } if provider.key == "example.net")
        );
        assert!(
            matches!(picture(&chat, &shown, "c1", 3).await, Response::Error { ref code, .. } if code == "not-found")
        );
        show_once(&chat, &shown, "c1", 3, PAGE).await.expect("shown once");
        let after = look_in(&chat, &shown, "c1", 3, PAGE, LinkOrigin::Model).await;
        assert_eq!(title(&after).as_deref(), Some("A post"));
        assert!(matches!(picture(&chat, &shown, "c1", 3).await, Response::ChatPicture { .. }));
        assert_eq!(chat.privacy(), always(), "Settings › Privacy stays");
    }

    /// Without a Nextcloud account a link stays a link and "Show once" says so; a blocked provider is refused.
    #[tokio::test]
    async fn without_nextcloud_a_link_stays_a_link() {
        let none = Arc::new(ChatService::fixed(Vec::new()));
        let shown = Shown::default();
        let card = look_in(&none, &shown, "c1", 1, PAGE, LinkOrigin::Person).await;
        assert_eq!(card, Card::Link { url: PAGE.into(), card: None, ask: None });
        let code = |result: Result<(), Response>| match result {
            Err(Response::Error { code, .. }) => code,
            other => format!("{other:?}"),
        };
        assert_eq!(code(show_once(&none, &shown, "c1", 1, PAGE).await), "not-connected");
        let mut blocked = Privacy::default();
        blocked.providers.insert("example.net".into(), ProviderChoice::Blocked);
        let (chat, server) = chat(blocked);
        assert_eq!(code(show_once(&chat, &shown, "c1", 1, PAGE).await), "unsupported");
        assert_eq!(server.count("resolve"), 0);
    }
}
