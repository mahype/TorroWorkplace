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
//! - **The own server and GitHub** (issue #790): a file, a quoted chat message and a GitHub item get their cards
//!   from [`crate::link_cards::nextcloud`], [`crate::link_cards::chat`] and [`crate::link_cards::github`], with the
//!   rights of the conversation's client, without Settings › Privacy. A GitHub item no account shows is the page it
//!   is.
//! - **Show once** ([`show_once`], `assistant.show_card`, a person's click): the card is fetched once and held in
//!   memory ([`Shown`]); Settings › Privacy stays as it is.
//!
//! Nothing here logs an address.
//!
//! [`MAX_LINK_CARDS`]: torro_ipc::assistant::MAX_LINK_CARDS

use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use torro_chat::ChatError;
use torro_core::i18n::tr;
use torro_core::privacy::{Decision, Privacy};
use torro_ipc::Response;
use torro_ipc::api::Rights;
use torro_ipc::assistant::{Card, CardRef, EntryContent, LinkCard, LinkOrigin};
use torro_ipc::chat::PreviewSize;

use super::Lookup;
use crate::account::github::GithubAccounts;
use crate::chat::ChatService;
use crate::link_cards::nextcloud::{self, OwnServer};
use crate::link_cards::web::{self, Resolved};
use crate::link_cards::{self, LinkKind};
use crate::pim::Reach;

/// The most link cards [`Shown`] holds; the oldest go first.
pub const SHOWN_KEPT: usize = 2000;

/// How long a server that could not be reached is not asked again ([`link_cards::RETRY_AFTER`]).
const RETRY_AFTER: Duration = link_cards::RETRY_AFTER;

/// What [`Shown`] holds for one link card.
#[derive(Debug, Clone)]
enum Held {
    /// Resolved by the server of `account` (its card is `None` when there is none, so it is not asked again). A card
    /// of the own server or GitHub keeps the `rights` it was resolved with and shows only while they are the same;
    /// a page's (`None`) needs none.
    Resolved { account: String, resolved: Box<Resolved>, rights: Option<Box<Rights>> },
    /// The server could not be reached then.
    Failed(Instant),
    /// A GitHub link without an item for `rights` (issue #784: no account, no right, none shows it), so GitHub is not
    /// asked again while the rights stay the same and the link is the page it is; `failed` when GitHub could not be
    /// asked then, asked again after [`RETRY_AFTER`].
    NoItem { rights: Box<Rights>, failed: Option<Instant> },
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
            Held::Failed(_) | Held::NoItem { .. } => None,
        }
    }

    /// Keeps `resolved`, the page's card of the entry `seq` of `conversation` as the server of `account` resolved
    /// it (`assistant.show_card`, or loaded by itself); the oldest go beyond [`SHOWN_KEPT`].
    pub fn keep(&self, conversation: &str, seq: u64, account: &str, resolved: Resolved) {
        self.put(
            conversation,
            seq,
            Held::Resolved { account: account.to_owned(), resolved: Box::new(resolved), rights: None },
        );
    }

    /// The account and the picture of the card held for the entry `seq` of `conversation`, when it has one
    /// (`assistant.get_card_picture`).
    pub fn picture(&self, conversation: &str, seq: u64) -> Option<(String, web::CardPicture)> {
        match self.held(conversation, seq)? {
            Held::Resolved { account, resolved, .. } => Some((account, resolved.picture?)),
            Held::Failed(_) | Held::NoItem { .. } => None,
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

/// Whether what the command `name` (its catalogue name, as a tool line keeps it) answered counts as read for
/// [`after_answer`] (decision 1 of issue #784): only a command that reads ([`torro_ipc::api::Access::Read`] in the
/// catalogue). What a command that writes answers echoes what the model sent it (`mail.save_draft` answers the
/// draft the model wrote), so an address in it is the model's; so is one from a server's tool, which the catalogue
/// does not know.
pub fn reads(name: &str) -> bool {
    torro_ipc::api::CATALOGUE
        .iter()
        .find(|command| command.name == name)
        .is_some_and(|command| command.access == torro_ipc::api::Access::Read)
}

/// The link cards to add after the answer `answer` (issue #785): one [`CardRef::Link`] for each of its first
/// [`MAX_LINK_CARDS`] `https` addresses ([`link_cards::links`]), in their order, with where the address came from:
/// [`LinkOrigin::Person`] when it stands whole in one of `person` (the person's own messages of the conversation,
/// never a client's), else [`LinkOrigin::Read`] when it stands whole in one of `read` (what commands of the
/// conversation that read answered, as the model read it: the caller passes only answers of commands that
/// [`reads`]), else [`LinkOrigin::Model`] ([`link_cards::stands_in`]: a part or a
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
/// `lookup` with the rights of its client ([`look_in`]); a page is fetched only when the person asks
/// ([`Lookup::asker`] the app key).
pub async fn look(lookup: &Lookup<'_>, seq: u64, url: &str, from: LinkOrigin) -> Card {
    let person = matches!(lookup.asker, Reach::Suite);
    let at = Place { chat: lookup.chat, github: lookup.github, rights: lookup.rights, shown: lookup.shown, person };
    look_in(&at, lookup.conversation, seq, url, from).await
}

/// What a look at a link card works with: the chat accounts (the own Nextcloud), the GitHub accounts, the rights of
/// the conversation's client, the cards held, and whether the person asks.
pub struct Place<'a> {
    pub chat: &'a Arc<ChatService>,
    pub github: &'a GithubAccounts,
    pub rights: &'a Rights,
    pub shown: &'a Shown,
    /// The asker is the person (the app key, their window): only then is a page fetched or its provider asked about
    /// (issue #784). A client's read gets a card held from before, else the plain link.
    pub person: bool,
}

/// What the link card of the entry `seq` of `conversation` for `url` shows now, by its kind
/// ([`link_cards::classify`] on the own server the link lies on, [`nextcloud::own_server`]):
///
/// - a file of the own server: its card when `rights` reach it ([`nextcloud::resolve`]), else a link – never a page
///   card, which would name the file all the same;
/// - a quoted chat message: its card when `rights` reach the conversation ([`link_cards::chat::resolve`]), else
///   [`Card::Unavailable`] without any text;
/// - a GitHub item: its card when an account shows it within `rights` ([`link_cards::github::resolve`]), else the
///   page it is;
/// - a page ([`step`] decides): a provider Settings › Privacy blocks, or any page at "Never show", a link (also for a
///   card held from before); a card held (shown once, or loaded before) as it was fetched; a page of another server
///   from the person or read ([`LinkOrigin::Person`], [`LinkOrigin::Read`]) whose provider Settings › Privacy lets
///   load, fetched now through the first Nextcloud account and held; one whose provider is not asked about yet, and
///   every address [`LinkOrigin::Model`] wrote ("Always show" too), `ask` with the provider; any other page of the
///   own server a link. Fetching and asking only for the person ([`Place::person`]): a client gets the card held,
///   else the plain link.
///
/// A card of the own server or GitHub is held with the rights it was resolved with and resolved again once they
/// differ; so is a GitHub link no account shows for those rights ([`Held::NoItem`]), which is then the page it is.
/// When a server cannot be reached the card says so (`offline`, S3 of issue #784) and the server is asked again
/// after [`RETRY_AFTER`]. Without a Nextcloud account a link stays a link. Never fetches the page itself, logs no
/// address.
pub async fn look_in(at: &Place<'_>, conversation: &str, seq: u64, url: &str, from: LinkOrigin) -> Card {
    let offline = || offline_link(url);
    let shown = |card: Option<LinkCard>| Card::Link {
        url: url.to_owned(),
        card: card.map(Box::new),
        ask: None,
        offline: false,
    };
    let own = nextcloud::own_server(at.chat, url).await;
    let kind = link_cards::classify(url, own.as_ref().map(|own| own.server.as_str()));
    let held = at.shown.held(conversation, seq);
    let now = Instant::now();
    let reach = Reach::Client(at.rights);
    // A card of the own server or GitHub resolved with these rights.
    let mine = match &held {
        Some(Held::Resolved { resolved, rights: Some(rights), .. }) if **rights == *at.rights => {
            Some(resolved.card.clone())
        }
        _ => None,
    };
    let failed = matches!(held, Some(Held::Failed(at)) if now.duration_since(at) < RETRY_AFTER);
    match (&kind, own) {
        (LinkKind::NextcloudFile(file), Some(own)) => {
            let found = match mine {
                Some(card) => Ok(card),
                None if failed => return offline(),
                None => nextcloud::resolve(at.chat, reach, &own.account, url, file).await,
            };
            match at.hold(conversation, seq, &own, &kind, found) {
                Some(card) => shown(card),
                None => offline(),
            }
        }
        (LinkKind::ChatMessage { token, message }, Some(own)) => {
            let found = match mine {
                Some(card) => Ok(card),
                None if failed => return offline(),
                None => link_cards::chat::resolve(at.chat, reach, &own.account, url, token, *message).await,
            };
            match at.hold(conversation, seq, &own, &kind, found) {
                Some(card) => link_cards::chat::assistant_card(url, card),
                None => offline(),
            }
        }
        (LinkKind::Github { repo, number, .. }, _) => {
            // What was held of GitHub with these rights: nothing to show (`None`), or a failure then (`Some`).
            let no_item = match &held {
                Some(Held::NoItem { rights, failed }) if **rights == *at.rights => Some(*failed),
                _ => None,
            };
            let found = match (mine, no_item) {
                (Some(card), _) => card,
                (None, Some(Some(then))) if now.duration_since(then) < RETRY_AFTER => return offline(),
                (None, Some(None)) => None,
                (None, _) if failed => return offline(),
                (None, _) if matches!(held, Some(Held::Resolved { rights: None, .. })) => None,
                (None, _) => {
                    let asked = link_cards::github::resolve(at.github, reach, url, repo, *number).await;
                    let rights = Box::new(at.rights.clone());
                    match asked {
                        Ok(Some(card)) => Some(card),
                        Ok(None) => {
                            at.shown.put(conversation, seq, Held::NoItem { rights, failed: None });
                            None
                        }
                        Err(_) => {
                            at.shown.put(conversation, seq, Held::NoItem { rights, failed: Some(now) });
                            return offline();
                        }
                    }
                }
            };
            match found {
                Some(card) => {
                    let resolved = Resolved::found(Some(card.clone()), &kind);
                    let rights = Some(Box::new(at.rights.clone()));
                    at.shown.put(
                        conversation,
                        seq,
                        Held::Resolved { account: String::new(), resolved: Box::new(resolved), rights },
                    );
                    shown(Some(card))
                }
                None => at.page(conversation, seq, url, from, false).await,
            }
        }
        (_, own) => at.page(conversation, seq, url, from, own.is_some()).await,
    }
}

impl Place<'_> {
    /// Holds what a card of the own server `own` resolved as (`kind`), with the rights it was resolved with, and
    /// answers it: `Some(card)` (`None` inside when there is none), or `None` when the server could not be asked.
    fn hold(
        &self,
        conversation: &str,
        seq: u64,
        own: &OwnServer,
        kind: &LinkKind,
        found: Result<Option<LinkCard>, ChatError>,
    ) -> Option<Option<LinkCard>> {
        match found {
            Ok(card) => {
                let resolved = Resolved::found(card.clone(), kind);
                let rights = Some(Box::new(self.rights.clone()));
                self.shown.put(
                    conversation,
                    seq,
                    Held::Resolved { account: own.account.clone(), resolved: Box::new(resolved), rights },
                );
                Some(card)
            }
            Err(_) => {
                self.shown.put(conversation, seq, Held::Failed(Instant::now()));
                None
            }
        }
    }

    /// The card of `url` as the page it is ([`step`]), `own` when it lies on the own server. Only the person's look
    /// fetches it or asks about its provider ([`Place::person`]); a client's gets the card held, else the link.
    async fn page(&self, conversation: &str, seq: u64, url: &str, from: LinkOrigin, own: bool) -> Card {
        let link = || Card::Link { url: url.to_owned(), card: None, ask: None, offline: false };
        let Some(account) = self.chat.accounts().await.into_iter().next() else { return link() };
        let Ok(remote) = self.chat.remote(&account).await else { return link() };
        let held = self.shown.held(conversation, seq);
        match step(&self.chat.privacy(), url, own, from, held.as_ref(), Instant::now()) {
            Step::Link => link(),
            Step::Held => Card::Link {
                url: url.to_owned(),
                card: self.shown.get(conversation, seq).map(Box::new),
                ask: None,
                offline: false,
            },
            Step::Offline | Step::Ask(_) | Step::Load if !self.person => link(),
            Step::Offline => offline_link(url),
            Step::Ask(provider) => Card::Link { url: url.to_owned(), card: None, ask: Some(provider), offline: false },
            Step::Load => match web::resolve(&remote, url).await {
                Ok(resolved) => {
                    let card = resolved.card.clone();
                    self.shown.keep(conversation, seq, &account, resolved);
                    Card::Link { url: url.to_owned(), card: card.map(Box::new), ask: None, offline: false }
                }
                Err(_) => {
                    self.shown.put(conversation, seq, Held::Failed(Instant::now()));
                    offline_link(url)
                }
            },
        }
    }
}

/// `assistant.show_card` for the link card of the entry `seq` of `conversation` with the address `url` (a person's
/// click, "Show once"): a page fetched once through the first Nextcloud account of `chat`, whatever server it is on
/// and wherever the address came from, and held in `shown`; Settings › Privacy stays as it is. A link to the own
/// server is not fetched as a page: its card comes with the rights of the conversation's client ([`look_in`]).
/// `Err` with the answer: `not-connected` without a Nextcloud account, `unsupported` for a provider Settings ›
/// Privacy blocks and while it says "Never show", the server's error when it cannot resolve it now.
pub async fn show_once(
    chat: &Arc<ChatService>,
    shown: &Shown,
    conversation: &str,
    seq: u64,
    url: &str,
) -> Result<(), Response> {
    let (account, remote) = nextcloud(chat).await?;
    if nextcloud::own_server(chat, url).await.is_some() {
        return Ok(());
    }
    if web::turned_off(&chat.privacy(), url, &remote.server()) {
        return Err(refused("unsupported", tr("Content from this provider is turned off in Settings › Privacy.")));
    }
    let resolved = web::resolve(&remote, url).await.map_err(|err| crate::chat::media::file_refusal(&err))?;
    shown.keep(conversation, seq, &account, resolved);
    Ok(())
}

/// `assistant.get_card_picture` for the entry `seq` of `conversation`, once the caller checked that its card shows
/// now with a picture ([`look`]): the picture of the card held in `shown`, from the server that resolved it, as
/// `chat.get_preview` with `card` fetches it ([`ChatService::card_picture_of`]: never an SVG; a file's preview at
/// `size`). `not-found` without one, `not-connected` when that account is gone.
pub async fn picture(
    chat: &Arc<ChatService>,
    shown: &Shown,
    conversation: &str,
    seq: u64,
    size: PreviewSize,
) -> Response {
    let Some((account, picture)) = shown.picture(conversation, seq) else {
        return refused("not-found", tr("This card has no picture."));
    };
    match chat.remote(&account).await {
        Ok(remote) => chat.card_picture_of(&account, &remote, &picture, size).await,
        Err(_) => not_connected(),
    }
}

/// The link card of `url` while the server that resolves it cannot be reached (S3 of issue #784).
fn offline_link(url: &str) -> Card {
    Card::Link { url: url.to_owned(), card: None, ask: None, offline: true }
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

/// What a page's link card does now ([`step`]).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Step {
    /// Stays a link: no card, no question.
    Link,
    /// Shows the card held in [`Shown`] (none when the server knows none: a link).
    Held,
    /// The server could not be reached a moment ago: the card says so, asked again after [`RETRY_AFTER`].
    Offline,
    /// Asks whether the provider's content may load.
    Ask(torro_chat::ForeignProvider),
    /// Is fetched now through the own Nextcloud.
    Load,
}

/// What the page's link card for `url` does with Settings › Privacy `privacy`, `own` when it lies on the own server,
/// the origin `from` and what `held` keeps of it at `now` ([`look_in`] tells it in words). The rule of decision 1:
/// only an address of the person or one read loads by itself; one the model wrote asks, at "Always show" too.
fn step(privacy: &Privacy, url: &str, own: bool, from: LinkOrigin, held: Option<&Held>, now: Instant) -> Step {
    let provider = web::provider(url);
    if !own && privacy.decide(&provider) == Decision::Skip {
        return Step::Link;
    }
    match held {
        Some(Held::Resolved { .. }) => return Step::Held,
        Some(Held::Failed(at)) if now.duration_since(*at) < RETRY_AFTER => return Step::Offline,
        _ => {}
    }
    // Any other page of the own server than a file or a message stays a link: its references could tell what the
    // conversation's client may not read (a card of a board), and no right of the client covers them.
    if own {
        return Step::Link;
    }
    let ask = |provider: torro_core::privacy::Provider| {
        Step::Ask(torro_chat::ForeignProvider { key: provider.key, name: provider.name })
    };
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
        assert_eq!(asks(&step(&privacy, PAGE, false, LinkOrigin::Model, None, now)), Some("example.net"));
        assert_eq!(step(&privacy, PAGE, false, LinkOrigin::Person, None, now), Step::Load);
        assert_eq!(step(&privacy, PAGE, false, LinkOrigin::Read, None, now), Step::Load);
        let mut allowed = Privacy::default();
        allowed.providers.insert("example.net".into(), ProviderChoice::Allowed);
        assert_eq!(asks(&step(&allowed, PAGE, false, LinkOrigin::Model, None, now)), Some("example.net"));
        assert_eq!(step(&allowed, PAGE, false, LinkOrigin::Person, None, now), Step::Load);
        let asked = Privacy::default();
        assert_eq!(asks(&step(&asked, PAGE, false, LinkOrigin::Person, None, now)), Some("example.net"));
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
        let resolved = Resolved { card: Some(card), picture: None };
        let held = Held::Resolved { account: "nc".into(), resolved: Box::new(resolved), rights: None };
        assert_eq!(step(&blocked, PAGE, false, LinkOrigin::Person, Some(&held), now), Step::Link);
        let never = Privacy { content: ForeignContent::Never, ..Privacy::default() };
        assert_eq!(step(&never, PAGE, false, LinkOrigin::Model, None, now), Step::Link);
        assert_eq!(step(&Privacy::default(), PAGE, false, LinkOrigin::Model, Some(&held), now), Step::Held);
        let failed = Held::Failed(now);
        assert_eq!(step(&always(), PAGE, false, LinkOrigin::Person, Some(&failed), now), Step::Offline);
        let later = now + RETRY_AFTER;
        assert_eq!(step(&always(), PAGE, false, LinkOrigin::Person, Some(&failed), later), Step::Load);
        let own = "https://cloud.example.org/apps/deck/card/4";
        assert_eq!(step(&always(), own, true, LinkOrigin::Person, None, now), Step::Link);
    }

    /// Decision 1 of issue #784: only what a command that reads answered counts as read; an echo of a command that
    /// writes is the model's.
    #[test]
    fn only_commands_that_read_count_as_read() {
        assert!(reads("mail.search_messages") && reads("calendar.list_events") && reads("chat.get_messages"));
        assert!(!reads("mail.save_draft") && !reads("calendar.save_event") && !reads("tasks.save_todo"));
        assert!(!reads("ext__someserver__fetch"), "a server's tool");
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

    fn github() -> GithubAccounts {
        let tokens = Arc::new(crate::account::github::tests::MemoryTokens::default());
        GithubAccounts::new(None, tokens, None, String::new(), Vec::new())
    }

    fn rights() -> Rights {
        torro_ipc::api::RightsTemplate::FullAccess.rights()
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
        let (github, rights) = (github(), rights());
        let at = Place { chat: &chat, github: &github, rights: &rights, shown: &shown, person: true };
        let title = |card: &Card| match card {
            Card::Link { card: Some(card), .. } => Some(card.title.clone()),
            _ => None,
        };
        let by_person = look_in(&at, "c1", 2, PAGE, LinkOrigin::Person).await;
        assert_eq!(title(&by_person).as_deref(), Some("A post"));
        look_in(&at, "c1", 2, PAGE, LinkOrigin::Person).await;
        assert_eq!(server.count("resolve"), 1, "held");

        let by_model = look_in(&at, "c1", 3, PAGE, LinkOrigin::Model).await;
        assert!(
            matches!(&by_model, Card::Link { card: None, ask: Some(provider), .. } if provider.key == "example.net")
        );
        assert!(
            matches!(picture(&chat, &shown, "c1", 3, PreviewSize::Small).await, Response::Error { ref code, .. } if code == "not-found")
        );
        show_once(&chat, &shown, "c1", 3, PAGE).await.expect("shown once");
        let after = look_in(&at, "c1", 3, PAGE, LinkOrigin::Model).await;
        assert_eq!(title(&after).as_deref(), Some("A post"));
        assert!(matches!(picture(&chat, &shown, "c1", 3, PreviewSize::Small).await, Response::ChatPicture { .. }));
        assert_eq!(chat.privacy(), always(), "Settings › Privacy stays");
    }

    /// Without a Nextcloud account a link stays a link and "Show once" says so; a blocked provider is refused.
    #[tokio::test]
    async fn without_nextcloud_a_link_stays_a_link() {
        let none = Arc::new(ChatService::fixed(Vec::new()));
        let shown = Shown::default();
        let (github, rights) = (github(), rights());
        let at = Place { chat: &none, github: &github, rights: &rights, shown: &shown, person: true };
        let card = look_in(&at, "c1", 1, PAGE, LinkOrigin::Person).await;
        assert_eq!(card, Card::Link { url: PAGE.into(), card: None, ask: None, offline: false });
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

    /// Issue #784: only the person's look fetches a page; a client's gets the card held, else the plain link, and is
    /// never asked about a provider.
    #[tokio::test]
    async fn a_clients_look_fetches_no_page() {
        let (chat, server) = chat(always());
        let reference = torro_nextcloud::Reference { title: "A post".into(), description: None, image: None };
        server.state().references.insert(PAGE.into(), reference);
        let shown = Shown::default();
        let (github, rights) = (github(), rights());
        let client = Place { chat: &chat, github: &github, rights: &rights, shown: &shown, person: false };
        assert_eq!(look_in(&client, "c1", 2, PAGE, LinkOrigin::Person).await, page_link());
        assert_eq!(look_in(&client, "c1", 3, PAGE, LinkOrigin::Model).await, page_link(), "no question for a client");
        assert_eq!(server.count("resolve"), 0, "nothing fetched for a client");
        let person = Place { chat: &chat, github: &github, rights: &rights, shown: &shown, person: true };
        look_in(&person, "c1", 2, PAGE, LinkOrigin::Person).await;
        let held = look_in(&client, "c1", 2, PAGE, LinkOrigin::Person).await;
        assert!(matches!(&held, Card::Link { card: Some(card), .. } if card.title == "A post"), "{held:?}");
        assert_eq!(server.count("resolve"), 1);
    }

    /// S3 of issue #784: while the own server cannot be reached a card says so, and it is not asked again at once.
    #[tokio::test]
    async fn an_unreachable_server_gives_an_offline_card() {
        let (chat, server) = chat(always());
        let file = "https://cloud.example.org/f/12";
        let shown = Shown::default();
        let (github, mut rights) = (github(), rights());
        rights.chat_accounts = torro_ipc::api::Selection::All;
        let at = Place { chat: &chat, github: &github, rights: &rights, shown: &shown, person: true };
        let offline = Card::Link { url: file.into(), card: None, ask: None, offline: true };
        server.state().offline = true;
        assert_eq!(look_in(&at, "c1", 1, file, LinkOrigin::Person).await, offline);
        let asked = server.count("resolve");
        server.state().offline = false;
        assert_eq!(look_in(&at, "c1", 1, file, LinkOrigin::Person).await, offline, "within a minute");
        assert_eq!(server.count("resolve"), asked, "not asked again at once");
    }

    fn page_link() -> Card {
        Card::Link { url: PAGE.into(), card: None, ask: None, offline: false }
    }

    /// GitHub through a fake whose item answer is `item` (status and body), with one connected account.
    async fn github_answering(
        dir: &std::path::Path,
        item: (u16, &'static str),
    ) -> (GithubAccounts, Arc<std::sync::Mutex<Vec<crate::account::github::tests::Seen>>>) {
        use crate::account::github::tests::{MemoryTokens, NO_CLI, fake_github, service, viewer};
        let (url, record) = fake_github(move |seen| match seen.body.contains("viewer") {
            true => (200, Some("repo"), viewer("anna-berg")),
            false => (item.0, None, item.1.to_string()),
        })
        .await;
        let accounts = service(dir, &MemoryTokens::default(), &url, NO_CLI);
        accounts.connect_token(torro_core::account::Secret::new("ghp_testTOKEN")).await.expect("connected");
        record.lock().expect("record").clear();
        (accounts, record)
    }

    /// Issue #784: a GitHub link no account shows is held as such, so GitHub is not asked with every look; one GitHub
    /// could not be asked for says so (S3), and is asked again only after a minute.
    #[tokio::test]
    async fn a_github_link_without_an_item_is_held() {
        const ISSUE: &str = "https://github.com/torro/workplace/issues/9";
        let dir = tempfile::tempdir().expect("temp dir");
        let (chat, _server) = chat(Privacy::default());
        let shown = Shown::default();
        let rights = rights();
        let nothing = r#"{"data":{"i0":{"issueOrPullRequest":null}}}"#;
        let (none, record) = github_answering(dir.path(), (200, nothing)).await;
        let at = Place { chat: &chat, github: &none, rights: &rights, shown: &shown, person: true };
        let first = look_in(&at, "c1", 1, ISSUE, LinkOrigin::Person).await;
        assert!(matches!(&first, Card::Link { card: None, ask: Some(_), offline: false, .. }), "the page: {first:?}");
        look_in(&at, "c1", 1, ISSUE, LinkOrigin::Person).await;
        assert_eq!(record.lock().expect("record").len(), 1, "GitHub asked once");

        let failing_dir = tempfile::tempdir().expect("temp dir");
        let (failing, record) = github_answering(failing_dir.path(), (502, "{}")).await;
        let at = Place { chat: &chat, github: &failing, rights: &rights, shown: &shown, person: true };
        let offline = Card::Link { url: ISSUE.into(), card: None, ask: None, offline: true };
        assert_eq!(look_in(&at, "c1", 2, ISSUE, LinkOrigin::Person).await, offline);
        let asked = record.lock().expect("record").len();
        assert!(asked >= 1, "asked at first");
        assert_eq!(look_in(&at, "c1", 2, ISSUE, LinkOrigin::Person).await, offline);
        assert_eq!(record.lock().expect("record").len(), asked, "not asked again within a minute");
    }

    /// Issue #790: a file of the own server reaches a client only with the chat right of its account
    /// (`nextcloud::reads_files`, issue #784), else it stays a link, never a page card; a quote of a conversation the
    /// client may not read is unavailable, without any text.
    #[tokio::test]
    async fn the_own_server_goes_by_the_rights_of_the_conversations_client() {
        let (chat, server) = chat(always());
        let file = "https://cloud.example.org/f/12";
        let reference = torro_nextcloud::Reference { title: "Plan.pdf".into(), description: None, image: None };
        server.state().references.insert(file.into(), reference);
        let shown = Shown::default();
        let github = github();
        let mut rights = rights();
        rights.chat_accounts = torro_ipc::api::Selection::Only(Vec::new());
        let at = Place { chat: &chat, github: &github, rights: &rights, shown: &shown, person: true };
        let card = look_in(&at, "c1", 1, file, LinkOrigin::Person).await;
        assert_eq!(card, Card::Link { url: file.into(), card: None, ask: None, offline: false });
        assert_eq!(server.count("resolve"), 0, "nothing asked, no page card");
        let at = Place { chat: &chat, github: &github, rights: &rights, shown: &shown, person: true };
        let quote = "https://cloud.example.org/call/ben4chat#message_5";
        assert_eq!(look_in(&at, "c1", 2, quote, LinkOrigin::Model).await, Card::Unavailable);
    }
}
