//! Messages of the own server's chat (issue #790, `docs/inhaltskarten.md` "Zitat einer Chat-Nachricht"): their links
//! ([`parse`]) and their cards ([`resolve`]: author, conversation, time, the start of the message), only when the
//! reader may read that conversation; otherwise no card and no text ([`assistant_card`]).

use std::sync::Arc;

use torro_chat::{ChatError, LinkCard, LinkCardKind, LinkQuote, MessageKind, conversation_id, message_id};
use torro_ipc::Request;
use torro_ipc::assistant::Card;
use torro_nextcloud::RoomToken;
use url::Url;

use super::LinkKind;
use crate::chat::ChatService;
use crate::pim::Reach;

/// The most characters of a quoted message's start ([`LinkQuote::text`]); more ends in "…".
pub const QUOTE_CHARS: usize = 200;

/// What `url` on the own server `own` names as a chat message: `/call/<token>#message_<id>` (also below
/// `/index.php/`); `None` for any other address, a conversation without a message too. `url` is on `own` already
/// ([`super::classify`] checked).
pub fn parse(url: &Url, own: &str) -> Option<LinkKind> {
    let path = super::nextcloud::path_below(url, own)?;
    let token = path.strip_prefix("/call/")?.trim_end_matches('/');
    RoomToken::parse(token)?;
    let number = url.fragment()?.strip_prefix("message_")?;
    let digits = !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit());
    let message = digits.then(|| number.parse::<u64>().ok()).flatten().filter(|number| *number > 0)?;
    Some(LinkKind::ChatMessage { token: token.to_owned(), message })
}

/// Whether `reach` may read the conversation `token` of the chat account `account`: the app key (the person)
/// always; a client when its rights let it call `chat.get_messages` on that conversation (read access to chat, the
/// command not switched off) and reach the account ([`torro_ipc::api::Rights::reaches_chat_account`]).
pub fn reads_conversation(reach: Reach<'_>, account: &str, token: &str) -> bool {
    let Reach::Client(rights) = reach else { return true };
    let read = Request::ChatGetMessages {
        conversation: conversation_id(account, token),
        before: None,
        after: None,
        around: None,
        limit: Some(1),
    };
    rights.allows_request(&read) && rights.reaches_chat_account(account)
}

/// The quote of the message `message` of the conversation `token`, linked as `link` on the server of the chat
/// account `account`, for `reach`.
///
/// In this order, each before anything further is asked:
/// 1. the right of `reach` to the quoted conversation (`reads_conversation`: for a client read access to chat, the
///    account in its rights) – without it nothing is asked of the server;
/// 2. the membership: the person's conversations name `token` ([`ChatService::conversation_name`]);
/// 3. the message itself, a message a person wrote (no system message, none deleted).
///
/// - `Ok(None)` when any of them fails, also for a link on another server than `account`'s: **no card and no
///   text**. The assistant shows [`Card::Unavailable`] ([`assistant_card`]); the chat with people shows no card.
/// - `Err`: the server cannot be asked now (offline, busy, sign-in refused); asked again with the next read.
///
/// A card held from an earlier read may only go to a reader that passes step 1 again (the chat with people hands
/// held cards to clients too, `crate::chat::attachments`). Reading changes nothing on the server. Logs no address.
pub async fn resolve(
    chat: &Arc<ChatService>,
    reach: Reach<'_>,
    account: &str,
    link: &str,
    token: &str,
    message: u64,
) -> Result<Option<LinkCard>, ChatError> {
    if !reads_conversation(reach, account, token) {
        return Ok(None);
    }
    let Some(token) = RoomToken::parse(token) else { return Ok(None) };
    let (remote, caps, _) = match chat.known(account).await {
        Ok(known) => known,
        Err(ChatError::NotFound) => return Ok(None),
        Err(err) => return Err(err),
    };
    if !caps.chat || !torro_nextcloud::same_origin(link, &remote.server()) {
        return Ok(None);
    }
    let name = match chat.conversation_name(account, &token).await {
        Ok(name) => name,
        Err(ChatError::NotFound | ChatError::Forbidden(_) | ChatError::NotConnected) => return Ok(None),
        Err(err) => return Err(err),
    };
    // The message and those before it, one of them: as `ChatService::message` reads one by its number.
    let fetched = remote.messages(token.as_str(), Some(message.saturating_add(1)), 1, &caps).await;
    let found = match chat.asked(account, fetched).await {
        Ok(messages) => messages.into_iter().find(|found| found.number == message && !found.hidden),
        Err(ChatError::NotFound | ChatError::Forbidden(_)) => None,
        Err(err) => return Err(err),
    };
    let Some(found) = found.filter(|found| matches!(found.kind, MessageKind::Message | MessageKind::Voice)) else {
        return Ok(None);
    };
    let conversation = conversation_id(account, token.as_str());
    let domain = Url::parse(link).ok().and_then(|url| url.host_str().map(str::to_owned)).unwrap_or_default();
    let quote = LinkQuote {
        message: message_id(&conversation, found.number),
        author: found.author_name,
        conversation: name.clone(),
        at: found.at,
        text: start_of(&found.text),
    };
    Ok(Some(LinkCard {
        url: link.to_owned(),
        title: name,
        description: None,
        domain,
        kind: LinkCardKind::ChatMessage,
        quote: Some(Box::new(quote)),
        ..LinkCard::default()
    }))
}

/// The assistant's card of the quote link `url` from what [`resolve`] found: the card, or [`Card::Unavailable`] –
/// which carries no text at all – when there is none. `Err` of [`resolve`] is for the caller to decide (the link
/// stays a link until the next look).
pub fn assistant_card(url: &str, found: Option<LinkCard>) -> Card {
    match found {
        Some(card) => Card::Link { url: url.to_owned(), card: Some(Box::new(card)), ask: None, offline: false },
        None => Card::Unavailable,
    }
}

/// The start of `text` on one line: white space made single spaces, at most [`QUOTE_CHARS`] characters.
fn start_of(text: &str) -> String {
    let line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match line.char_indices().nth(QUOTE_CHARS) {
        Some((end, _)) => format!("{}…", line[..end].trim_end()),
        None => line,
    }
}

#[cfg(test)]
mod tests {
    use torro_ipc::api::{Rights, RightsTemplate, Selection};

    use super::super::classify;
    use super::*;
    use crate::chat::fake::{SERVER, Server};

    const ACCOUNT: &str = "nextcloud:anna@cloud.example.org";

    fn kind(link: &str, own: &str) -> Option<LinkKind> {
        parse(&Url::parse(link).expect("an address"), own)
    }

    /// A message link on the own server is a quote, on a foreign server a page; a conversation alone is none.
    #[test]
    fn a_message_link_of_the_own_server_is_a_quote_and_on_a_foreign_server_a_page() {
        let own = "https://cloud.example.org";
        let quote = Some(LinkKind::ChatMessage { token: "ben4chat".into(), message: 3 });
        assert_eq!(kind("https://cloud.example.org/call/ben4chat#message_3", own), quote);
        assert_eq!(kind("https://cloud.example.org/index.php/call/ben4chat#message_3", own), quote);
        assert_eq!(kind("https://example.org/nc/call/ben4chat#message_3", "https://example.org/nc"), quote);
        for none in [
            "https://cloud.example.org/call/ben4chat",
            "https://cloud.example.org/call/ben4chat#message_",
            "https://cloud.example.org/call/ben4chat#message_x",
            "https://cloud.example.org/call/#message_3",
            "https://cloud.example.org/apps/spreed#message_3",
        ] {
            assert_eq!(kind(none, own), None, "{none}");
        }
        let link = "https://cloud.example.org/call/ben4chat#message_3";
        assert_eq!(classify(link, Some(own)), LinkKind::ChatMessage { token: "ben4chat".into(), message: 3 });
        assert_eq!(classify(link, Some("https://cloud.other.org")), LinkKind::Web, "a foreign Nextcloud");
    }

    #[test]
    fn the_start_of_a_message_is_one_short_line() {
        assert_eq!(start_of("Hello\n  team,\tsee  this"), "Hello team, see this");
        let long = "word ".repeat(100);
        let start = start_of(&long);
        assert!(start.ends_with('…') && start.chars().count() <= QUOTE_CHARS + 1, "{start}");
    }

    fn service() -> (Arc<ChatService>, Arc<Server>) {
        let server = Arc::new(Server::default());
        (Arc::new(ChatService::fixed(vec![(ACCOUNT.into(), Arc::clone(&server))])), server)
    }

    fn link(token: &str, number: u64) -> String {
        format!("{SERVER}/call/{token}#message_{number}")
    }

    #[tokio::test]
    async fn the_person_gets_the_quote_of_a_message_of_their_conversation() {
        let (chat, _server) = service();
        let card = resolve(&chat, Reach::Suite, ACCOUNT, &link("ben4chat", 3), "ben4chat", 3).await.expect("asked");
        let card = card.expect("a card");
        assert_eq!(card.kind, LinkCardKind::ChatMessage);
        let quote = card.quote.expect("the quote");
        assert_eq!(quote.message, message_id(&conversation_id(ACCOUNT, "ben4chat"), 3));
        assert_eq!((quote.conversation.as_str(), quote.text.as_str()), ("Ben Koch", "three"));
        let gone = resolve(&chat, Reach::Suite, ACCOUNT, &link("ben4chat", 99), "ben4chat", 99).await;
        assert_eq!(gone, Ok(None), "a message that is not there");
    }

    /// Not a member: the person's conversations do not name it, and its messages are never asked for.
    #[tokio::test]
    async fn a_conversation_the_person_is_not_in_gives_nothing() {
        let (chat, server) = service();
        server.state().messages.insert("strangers".into(), vec![crate::chat::remote::tests::message(1, "secret")]);
        let card = resolve(&chat, Reach::Suite, ACCOUNT, &link("strangers", 1), "strangers", 1).await;
        assert_eq!(card, Ok(None));
        assert_eq!(server.count("messages strangers"), 0, "no message asked for without the membership");
    }

    /// A client without the right to the quoted conversation (here: its account) gets the card "No longer
    /// available" without any text, and the server is not asked at all; with the right it gets the quote.
    #[tokio::test]
    async fn a_quote_without_the_right_is_unavailable_without_text() {
        let (chat, server) = service();
        let url = link("ben4chat", 3);
        let mut rights = RightsTemplate::ReadOnly.rights();
        rights.chat_accounts = Selection::Only(vec!["nextcloud:anna@other.example.org".into()]);
        let found = resolve(&chat, Reach::Client(&rights), ACCOUNT, &url, "ben4chat", 3).await.expect("asked");
        let card = assistant_card(&url, found);
        assert_eq!(card, Card::Unavailable);
        let shown = serde_json::to_string(&card).expect("json");
        assert!(!shown.contains("three") && !shown.contains("Ben"), "no text: {shown}");
        assert!(server.state().calls.is_empty(), "nothing asked before the right: {:?}", server.state().calls);

        rights.chat_accounts = Selection::Only(vec![ACCOUNT.into()]);
        rights.areas.remove(&torro_ipc::api::Area::Chat);
        let found = resolve(&chat, Reach::Client(&rights), ACCOUNT, &url, "ben4chat", 3).await;
        assert_eq!(found, Ok(None), "the account alone is no read access to chat");
        assert!(server.state().calls.is_empty());

        let rights =
            Rights { chat_accounts: Selection::Only(vec![ACCOUNT.into()]), ..RightsTemplate::ReadOnly.rights() };
        let found = resolve(&chat, Reach::Client(&rights), ACCOUNT, &url, "ben4chat", 3).await.expect("asked");
        assert!(matches!(assistant_card(&url, found), Card::Link { card: Some(_), .. }), "with the right");
    }
}
