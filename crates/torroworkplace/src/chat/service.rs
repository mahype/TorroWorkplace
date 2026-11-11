//! The commands of the area `chat` (`docs/chat.md`, "Befehle"): read from the server through the
//! cache, written to the server at once.
//!
//! - **Reach:** a client sees only the accounts of its rights ([`Rights::reaches_chat_account`]).
//!   The account is read from the id itself, so a conversation, message or person of another
//!   account is `not-found` before anything is asked ([`ChatService::out_of_reach`], checked before
//!   an approval is asked for too).
//! - **Reading changes nothing** on the server: no read marker, no notification, no status
//!   (decision 9, "Lesen ändert nichts"). Only `chat.mark_read` reads.
//! - **Nothing is sent later** (decision 8): without a connection sending answers `offline`.
//! - **Messages the server refuses to name** never reach an answer: every message of the server is
//!   read by the provider, and errors carry this file's own English words only.
//! - **Facts for approvals** ([`ChatService::facts`]) come from the cache or the server by id,
//!   never from the client's request.

use std::sync::Arc;
use std::time::Instant;

use torro_chat::{
    Author, ChatError, Conversation, ConversationKind, Message, MessagePage, Quote, SearchHit, conversation_id,
    message_id, person_id, split_conversation, split_message, split_person,
};
use torro_core::i18n::{tr, trf};
use torro_ipc::chat::{CONVERSATIONS, MAX_CONVERSATIONS, MAX_HITS, MAX_MESSAGES, MAX_PEOPLE, MESSAGES, MIN_QUERY};
use torro_ipc::{Request, Response};
use torro_nextcloud::RoomToken;

use super::ChatService;
use super::cache::WATCHED_FOR;
use super::remote::{Caps, Remote, RemoteMessage, RemoteRoom, Sending};
use crate::pim::Reach;

/// The answer for `err`, with the fixed codes of the area (`docs/chat.md`, "Fehler").
pub fn error_response(err: &ChatError) -> Response {
    let (code, message) = match err {
        ChatError::Auth => ("auth", tr("The server refused the sign-in.").to_string()),
        ChatError::Offline => ("offline", tr("The server cannot be reached.").to_string()),
        ChatError::NotConnected => ("not-connected", tr("This account has no chat on its server.").to_string()),
        ChatError::NotFound => ("not-found", tr("The conversation, message or person does not exist.").to_string()),
        ChatError::Unsupported(message) => ("unsupported", message.clone()),
        ChatError::Forbidden(message) => ("forbidden", message.clone()),
        ChatError::Busy => ("busy", tr("Too many calls. Try again later.").to_string()),
        ChatError::Pending => ("error", tr("The command waits for approval.").to_string()),
        ChatError::Other(message) => ("error", message.clone()),
    };
    Response::Error { code: code.into(), message }
}

/// What an approval shows of a chat command, looked up by the core (`approvals::Facts`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChatFacts {
    /// The conversation's name; `None` when the core does not know it (any more).
    pub conversation: Option<String>,
    pub kind: Option<ConversationKind>,
    /// People of other servers take part.
    pub federated: bool,
    /// The author of the message a reply quotes.
    pub quoted: Option<String>,
    /// The text of the message an edit, a deletion or a reaction is about, as the core holds it.
    pub old_text: Option<String>,
}

/// The account a conversation id names and its checked token; `not-found` for anything that is
/// no conversation id.
fn conversation_of(id: &str) -> Result<(&str, RoomToken), ChatError> {
    let (account, token) = split_conversation(id).ok_or(ChatError::NotFound)?;
    Ok((account, RoomToken::parse(token).ok_or(ChatError::NotFound)?))
}

/// Account, checked token and number of a message id.
fn message_of(id: &str) -> Result<(&str, RoomToken, u64), ChatError> {
    let (conversation, number) = split_message(id).ok_or(ChatError::NotFound)?;
    let (account, token) = conversation_of(conversation)?;
    Ok((account, token, number))
}

/// The number of the message `id` when it is one of `conversation`; an `error` otherwise.
fn number_in(conversation: &str, id: &str) -> Result<u64, ChatError> {
    match split_message(id) {
        Some((of, number)) if of == conversation => Ok(number),
        _ => Err(ChatError::Other(tr("This is no message of the conversation.").to_string())),
    }
}

/// The account a request of the area names, by its ids alone; `Err` for an id that is none.
/// `Ok(None)` for a request over every account and for requests of other areas.
fn account_of(request: &Request) -> Result<Option<&str>, ()> {
    fn conversation(id: &str) -> Result<&str, ()> {
        conversation_of(id).map(|(account, _)| account).map_err(|_| ())
    }
    fn message(id: &str) -> Result<&str, ()> {
        message_of(id).map(|(account, ..)| account).map_err(|_| ())
    }
    match request {
        Request::ChatListConversations { account, .. } | Request::ChatSearchMessages { account, .. } => {
            Ok(account.as_deref())
        }
        Request::ChatFindPeople { account, .. } => Ok(Some(account)),
        Request::ChatCreateConversation { account, with } => match split_person(with) {
            Some((of, _)) if of == account => Ok(Some(account)),
            _ => Err(()),
        },
        Request::ChatGetMessages { conversation: id, .. }
        | Request::ChatSendMessage { conversation: id, .. }
        | Request::ChatMarkRead { conversation: id, .. }
        | Request::ChatMarkUnread { conversation: id }
        | Request::ChatSuggestMentions { conversation: id, .. }
        | Request::ChatSetFavorite { conversation: id, .. }
        | Request::ChatSetNotificationLevel { conversation: id, .. } => conversation(id).map(Some),
        Request::ChatEditMessage { id, .. } | Request::ChatDeleteMessage { id } | Request::ChatReact { id, .. } => {
            message(id).map(Some)
        }
        _ => Ok(None),
    }
}

/// Whether `reach` sees the chats of `account`.
fn reaches(reach: Reach<'_>, account: &str) -> bool {
    match reach {
        Reach::Suite => true,
        Reach::Client(rights) => rights.reaches_chat_account(account),
    }
}

/// `room` of `account` as the API shows it; `me` is the person's user id on the server.
fn conversation(account: &str, room: &RemoteRoom) -> Conversation {
    let id = conversation_id(account, &room.token);
    Conversation {
        last_read: room.last_read.map(|number| message_id(&id, number)),
        id,
        account: account.to_string(),
        name: room.name.clone(),
        kind: room.kind,
        unread: room.unread,
        mentioned: room.mentioned,
        mentioned_directly: room.mentioned_directly,
        last_message: room.last_message.clone(),
        last_activity: room.last_activity,
        person: room.other_user.as_deref().map(|user| person_id(account, user)),
        has_call: room.has_call,
        ringing: false,
        favorite: room.favorite,
        read_only: room.read_only,
        federated: room.federated,
        notify: room.notify,
        untrusted: true,
    }
}

/// `message` of `conversation` as the API shows it.
fn shown(account: &str, conversation: &str, me: &str, message: RemoteMessage) -> Message {
    Message {
        id: message_id(conversation, message.number),
        author: Author {
            own: message.author.as_deref() == Some(me),
            id: message.author.as_deref().map(|user| person_id(account, user)),
            name: message.author_name,
        },
        at: message.at,
        text: message.text,
        kind: message.kind,
        mentions: message.mentions,
        files: message.files,
        reply_to: message.reply_to.map(|quote| Quote {
            id: message_id(conversation, quote.number),
            author: quote.author,
            text: quote.text,
        }),
        reactions: message.reactions,
        own_reactions: message.own_reactions,
        edited: message.edited,
        untrusted: true,
    }
}

/// Who called and how, for the rules that differ between the app key and a client.
#[derive(Debug, Clone, Copy)]
pub struct Caller<'a> {
    pub reach: Reach<'a>,
    /// The client's id; `None` for the app key.
    pub client: Option<&'a str>,
}

impl Caller<'_> {
    fn is_app(&self) -> bool {
        matches!(self.reach, Reach::Suite)
    }
}

impl ChatService {
    /// For a client: the id in `request` it may not reach (an account outside its rights or an id
    /// that names none), answered `not-found` before anything else; `None` when everything is in
    /// reach and for requests of other areas.
    pub fn out_of_reach(&self, reach: Reach<'_>, request: &Request) -> Option<String> {
        let Reach::Client(rights) = reach else { return None };
        if request.operation().area() != torro_ipc::api::Area::Chat {
            return None;
        }
        match account_of(request) {
            Ok(Some(account)) => (!rights.reaches_chat_account(account)).then(|| account.to_string()),
            Ok(None) => None,
            Err(()) => Some(String::new()),
        }
    }

    /// The accounts `reach` sees among those there are, or `account` alone when given.
    async fn accounts_within(&self, reach: Reach<'_>, account: Option<&str>) -> Vec<String> {
        match account {
            Some(account) => vec![account.to_string()],
            None => self.accounts().await.into_iter().filter(|id| reaches(reach, id)).collect(),
        }
    }

    /// The server of `account`, what it can do and the person's user id there; asked once an hour,
    /// else from memory. Also for a server without a chat (presence works without Talk).
    pub(crate) async fn known(&self, account: &str) -> Result<(Remote, Caps, String), ChatError> {
        let remote = self.remote(account).await?;
        let held = {
            let mut cache = self.cache();
            let entry = cache.account(account);
            match (&entry.caps, &entry.user) {
                (Some(caps), Some(user)) if entry.known_fresh() => Some((caps.clone(), user.clone())),
                _ => None,
            }
        };
        let (caps, user) = match held {
            Some(held) => held,
            None => {
                let caps = self.asked(account, remote.caps().await).await?;
                let user = self.asked(account, remote.user().await).await?;
                let mut cache = self.cache();
                let entry = cache.account(account);
                entry.caps = Some(caps.clone());
                entry.user = Some(user.id.clone());
                entry.name = Some(user.name);
                entry.known_at = Some(Instant::now());
                (caps, user.id)
            }
        };
        Ok((remote, caps, user))
    }

    /// The person's name on the server of `account`, once [`ChatService::known`] asked for it.
    pub(crate) fn own_name(&self, account: &str) -> Option<String> {
        self.cache().account(account).name.clone()
    }

    /// What is needed before anything of the chat is asked of `account` ([`ChatService::known`]);
    /// `not-connected` when the server has no chat.
    pub(super) async fn ready(&self, account: &str) -> Result<(Remote, Caps, String), ChatError> {
        let (remote, caps, user) = self.known(account).await?;
        if !caps.chat {
            return Err(ChatError::NotConnected);
        }
        Ok((remote, caps, user))
    }

    /// The other people of the direct chats of `account`, with the names the conversations show;
    /// empty for an account without a chat or one that cannot be reached.
    pub(crate) async fn direct_people(self: &Arc<Self>, account: &str) -> Vec<super::remote::RemotePerson> {
        let rooms = match self.ready(account).await {
            Ok(_) => self.rooms_of(account).await.unwrap_or_default(),
            Err(_) => Vec::new(),
        };
        rooms
            .into_iter()
            .filter(|room| room.kind == ConversationKind::Direct)
            .filter_map(|room| Some(super::remote::RemotePerson { id: room.other_user?, name: room.name }))
            .collect()
    }

    /// `result` of a call to `account`'s server; a refused sign-in also starts the remote wipe
    /// check of the account.
    pub(crate) async fn asked<T>(&self, account: &str, result: Result<T, ChatError>) -> Result<T, ChatError> {
        if matches!(result, Err(ChatError::Auth)) {
            self.refused(account).await;
        }
        result
    }

    /// Fetches the conversations of `account` and keeps them.
    pub async fn refresh_rooms(&self, account: &str) -> Result<(), ChatError> {
        let (remote, ..) = self.ready(account).await?;
        let rooms = self.asked(account, remote.rooms().await).await?;
        self.take_rooms(account, rooms);
        Ok(())
    }

    /// Keeps `rooms` as the conversations of `account`, as fetched now. The loop of the Nextcloud
    /// account hands its list here, so one request serves the inbox and the chat (decision 9).
    pub fn take_rooms(&self, account: &str, rooms: Vec<RemoteRoom>) {
        let mut cache = self.cache();
        let entry = cache.account(account);
        entry.rooms = rooms;
        entry.rooms_at = Some(Instant::now());
    }

    /// The conversations of `account` from memory, fetched first when there are none yet; a list
    /// older than ten seconds is fetched again in the background.
    async fn rooms_of(self: &Arc<Self>, account: &str) -> Result<Vec<RemoteRoom>, ChatError> {
        let (known, fresh) = {
            let mut cache = self.cache();
            let entry = cache.account(account);
            (entry.rooms_at.is_some(), entry.rooms_fresh(Instant::now()))
        };
        if !known {
            self.refresh_rooms(account).await?;
        } else if !fresh {
            let (service, account) = (Arc::clone(self), account.to_string());
            tokio::spawn(async move {
                // Failing is fine: the list held stays, and the next call tries again.
                let _ = service.refresh_rooms(&account).await;
            });
        }
        Ok(self.cache().account(account).rooms.clone())
    }

    /// The conversation `token` of `account` as the list holds it, fetching the list once when it
    /// does not name it (a conversation made a moment ago). `not-found` when the server lists none.
    async fn room(self: &Arc<Self>, account: &str, token: &RoomToken) -> Result<RemoteRoom, ChatError> {
        let find = |rooms: &[RemoteRoom]| rooms.iter().find(|room| room.token == token.as_str()).cloned();
        if let Some(room) = find(&self.rooms_of(account).await?) {
            return Ok(room);
        }
        let fetched_now = self.cache().account(account).rooms_fresh(Instant::now());
        if !fetched_now {
            self.refresh_rooms(account).await?;
        }
        find(&self.cache().account(account).rooms).ok_or(ChatError::NotFound)
    }

    /// Answers a `chat.*` request for `caller` (its rights and reach checked before).
    pub async fn handle(self: &Arc<Self>, request: Request, caller: Caller<'_>) -> Response {
        // What other people see is throttled per client, also when it may send freely.
        if request.operation().command.is_outbound()
            && let Some(client) = caller.client
            && !self.throttle.admit(client, Instant::now())
        {
            return error_response(&ChatError::Busy);
        }
        let answer = match request {
            Request::ChatListConversations { account, unread_only, query, limit } => {
                self.list(caller.reach, account.as_deref(), unread_only, query.as_deref(), limit).await
            }
            Request::ChatGetMessages { conversation, before, after, limit } => self
                .messages(&conversation, before.as_deref(), after.as_deref(), limit, caller)
                .await
                .map(Response::ChatMessages),
            Request::ChatSendMessage { conversation, text, reply_to, silent } => {
                self.send(&conversation, &text, reply_to.as_deref(), silent).await
            }
            Request::ChatMarkRead { conversation, up_to } => self.mark_read(&conversation, up_to.as_deref()).await,
            Request::ChatMarkUnread { conversation } => self.mark_unread(&conversation).await,
            Request::ChatSuggestMentions { conversation, query, limit } => {
                self.mentions(&conversation, &query, limit).await
            }
            Request::ChatFindPeople { account, query, limit } => self.find_people(&account, &query, limit).await,
            Request::ChatCreateConversation { account, with } => self.create(&account, &with).await,
            Request::ChatEditMessage { id, text } => self.edit(&id, &text).await,
            Request::ChatDeleteMessage { id } => self.delete(&id).await,
            Request::ChatReact { id, emoji, remove } => self.react(&id, &emoji, remove).await,
            Request::ChatSetFavorite { conversation, favorite } => self.set_favorite(&conversation, favorite).await,
            Request::ChatSetNotificationLevel { conversation, level } => self.set_notify(&conversation, level).await,
            Request::ChatSearchMessages { query, account, limit } => {
                self.search(caller.reach, &query, account.as_deref(), limit).await
            }
            other => Err(ChatError::Other(format!("{} is no chat command", other.operation().name()))),
        };
        answer.unwrap_or_else(|err| error_response(&err))
    }

    async fn list(
        self: &Arc<Self>,
        reach: Reach<'_>,
        account: Option<&str>,
        unread_only: bool,
        query: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Response, ChatError> {
        let accounts = self.accounts_within(reach, account).await;
        let query = query.map(str::to_lowercase).filter(|query| !query.trim().is_empty());
        let mut conversations = Vec::new();
        let mut failed = None;
        for id in &accounts {
            match self.rooms_of(id).await {
                Ok(rooms) => conversations.extend(
                    rooms
                        .iter()
                        // The provider's own news and kinds this code does not know are no conversations.
                        .filter(|room| room.kind != ConversationKind::Other)
                        .filter(|room| !unread_only || room.unread > 0)
                        .filter(|room| query.as_ref().is_none_or(|query| room.name.to_lowercase().contains(query)))
                        .map(|room| conversation(id, room)),
                ),
                // An account without a chat is no failure of a list over all accounts.
                Err(ChatError::NotConnected) if account.is_none() => {}
                Err(err) => failed = Some(err),
            }
        }
        // One account that fails does not hide the others; alone, or named, it is the answer.
        if let Some(err) = failed
            && conversations.is_empty()
        {
            return Err(err);
        }
        conversations.sort_by_key(|conversation| std::cmp::Reverse(conversation.last_activity));
        conversations.truncate(limit.unwrap_or(CONVERSATIONS).clamp(1, MAX_CONVERSATIONS) as usize);
        Ok(Response::ChatConversations { conversations })
    }

    /// Fetches the newest messages of a conversation into the cache.
    pub(super) async fn fetch_newest(
        &self,
        account: &str,
        token: &RoomToken,
        remote: &Remote,
        caps: &Caps,
    ) -> Result<(), ChatError> {
        let fetched = self.asked(account, remote.messages(token.as_str(), None, MAX_MESSAGES, caps).await).await?;
        let whole = (fetched.len() as u32) < MAX_MESSAGES;
        let now = Instant::now();
        let mut cache = self.cache();
        let thread = cache.thread(&conversation_id(account, token.as_str()), now);
        // Fewer than asked for: that is the whole conversation.
        if thread.is_empty() {
            thread.more = !whole;
        }
        thread.merge(fetched);
        thread.fetched = Some(now);
        Ok(())
    }

    async fn messages(
        self: &Arc<Self>,
        id: &str,
        before: Option<&str>,
        after: Option<&str>,
        limit: Option<u32>,
        caller: Caller<'_>,
    ) -> Result<MessagePage, ChatError> {
        let (account, token) = conversation_of(id)?;
        if before.is_some() && after.is_some() {
            return Err(ChatError::Other(tr("Give either `before` or `after`, not both.").to_string()));
        }
        let before = before.map(|message| number_in(id, message)).transpose()?;
        let after = after.map(|message| number_in(id, message)).transpose()?;
        let limit = limit.unwrap_or(MESSAGES).clamp(1, MAX_MESSAGES) as usize;
        let room = self.room(account, &token).await;
        // The list could not be fetched: what is held still shows, marked offline.
        let room = match room {
            Ok(room) => Some(room),
            Err(ChatError::Offline) => self.cache().account(account).room(token.as_str()).cloned(),
            Err(err) => return Err(err),
        };
        let Some(room) = room else { return Err(ChatError::Offline) };
        let ready = self.ready(account).await;
        let now = Instant::now();
        // Only an app notes a conversation for live updates (decision 9): a client reads the cache.
        if after.is_some() && caller.is_app() {
            self.cache().thread(id, now).watched_until = Some(now + WATCHED_FOR);
            super::live::start(self);
        }
        let mut offline = false;
        match ready {
            Ok((remote, caps, _)) => {
                let (fresh, oldest_needed) = {
                    let mut cache = self.cache();
                    let thread = cache.thread(id, now);
                    (thread.fresh(now) && !thread.is_empty(), before.map(|before| thread.before(before, limit)))
                };
                if !fresh {
                    // One at a time for clients, so an agent cannot keep the server busy.
                    let _one = if caller.is_app() { None } else { Some(self.client_fetch.lock().await) };
                    match self.fetch_newest(account, &token, &remote, &caps).await {
                        Ok(()) => {}
                        Err(ChatError::Offline) => offline = true,
                        Err(err) => return Err(err),
                    }
                }
                // Older than what is held: asked of the server, and not kept (the cache holds the
                // newest messages only).
                if let (Some(before), Some((held, more))) = (before, oldest_needed)
                    && held.len() < limit
                    && more
                    && !offline
                {
                    let user = self.cache().account(account).user.clone().unwrap_or_default();
                    let fetched =
                        self.asked(account, remote.messages(token.as_str(), Some(before), limit as u32, &caps).await);
                    match fetched.await {
                        Ok(fetched) => {
                            let more = fetched.len() >= limit;
                            let messages = fetched
                                .into_iter()
                                .filter(|message| !message.hidden)
                                .map(|message| shown(account, id, &user, message))
                                .collect();
                            return Ok(MessagePage { messages, last_read: last_read(id, &room), more, offline });
                        }
                        Err(ChatError::Offline) => offline = true,
                        Err(err) => return Err(err),
                    }
                }
            }
            Err(ChatError::Offline) => offline = true,
            Err(err) => return Err(err),
        }
        let mut cache = self.cache();
        let user = cache.account(account).user.clone().unwrap_or_default();
        let thread = cache.thread(id, now);
        let (messages, more) = match (before, after) {
            (Some(before), _) => thread.before(before, limit),
            (None, Some(after)) => (thread.after(after, limit), false),
            (None, None) => thread.last(limit),
        };
        let messages = messages.into_iter().map(|message| shown(account, id, &user, message)).collect();
        Ok(MessagePage { messages, last_read: last_read(id, &room), more, offline })
    }

    async fn send(
        self: &Arc<Self>,
        id: &str,
        text: &str,
        reply_to: Option<&str>,
        silent: bool,
    ) -> Result<Response, ChatError> {
        let (account, token) = conversation_of(id)?;
        let reply_to = reply_to.map(|message| number_in(id, message)).transpose()?;
        if text.trim().is_empty() {
            return Err(ChatError::Other(tr("A message needs a text.").to_string()));
        }
        let (remote, caps, _) = self.ready(account).await?;
        let length = text.chars().count();
        if length > caps.max_length {
            return Err(ChatError::Other(trf(
                "The message is too long: {length} of at most {max} characters.",
                &[("length", &length), ("max", &caps.max_length)],
            )));
        }
        if silent && !caps.silent {
            return Err(unsupported("This server cannot send without notifying."));
        }
        let room = self.room(account, &token).await?;
        if room.read_only {
            return Err(unsupported("This conversation is read-only."));
        }
        let sending = Sending { text: text.to_string(), reply_to, reference: reference(), silent };
        let sent = self.asked(account, remote.send(token.as_str(), &sending).await).await?;
        let number = sent.number;
        let now = Instant::now();
        let mut cache = self.cache();
        if let Some(room) = cache.account(account).room_mut(token.as_str()) {
            // Sending reads the conversation on the server, so the list held says so at once.
            room.unread = 0;
            room.mentioned = false;
            room.mentioned_directly = false;
            room.last_read = Some(number);
            room.last_number = Some(number);
            room.last_activity = sent.at;
            room.last_message =
                Some(torro_chat::Preview { author: sent.author_name.clone(), text: sent.text.clone(), system: false });
        }
        cache.thread(id, now).merge(vec![sent]);
        Ok(Response::ChatSent { id: message_id(id, number) })
    }

    async fn mark_read(self: &Arc<Self>, id: &str, up_to: Option<&str>) -> Result<Response, ChatError> {
        let (account, token) = conversation_of(id)?;
        let up_to = up_to.map(|message| number_in(id, message)).transpose()?;
        let (remote, caps, _) = self.ready(account).await?;
        if !caps.read_marker {
            return Err(unsupported("This server cannot mark conversations read."));
        }
        let room = self.room(account, &token).await?;
        let newest = || room.last_number.or_else(|| self.cache().peek(id).and_then(|thread| thread.newest()));
        let Some(number) = up_to.or_else(newest) else {
            // Nothing in it: nothing to read.
            return Ok(Response::Ok);
        };
        self.asked(account, remote.mark_read(token.as_str(), number).await).await?;
        if let Some(room) = self.cache().account(account).room_mut(token.as_str()) {
            room.last_read = Some(number);
            if up_to.is_none() || Some(number) >= room.last_number {
                room.unread = 0;
                room.mentioned = false;
                room.mentioned_directly = false;
            }
        }
        Ok(Response::Ok)
    }

    async fn mark_unread(self: &Arc<Self>, id: &str) -> Result<Response, ChatError> {
        let (account, token) = conversation_of(id)?;
        let (remote, caps, _) = self.ready(account).await?;
        if !caps.unread {
            return Err(unsupported("This server cannot mark conversations unread."));
        }
        self.room(account, &token).await?;
        self.asked(account, remote.mark_unread(token.as_str()).await).await?;
        if let Some(room) = self.cache().account(account).room_mut(token.as_str()) {
            room.unread = room.unread.max(1);
        }
        Ok(Response::Ok)
    }

    async fn mentions(self: &Arc<Self>, id: &str, query: &str, limit: Option<u32>) -> Result<Response, ChatError> {
        let (account, token) = conversation_of(id)?;
        let (remote, ..) = self.ready(account).await?;
        self.room(account, &token).await?;
        let limit = limit.unwrap_or(20).clamp(1, 50);
        let mentions = self.asked(account, remote.mentions(token.as_str(), query, limit).await).await?;
        Ok(Response::ChatMentions { mentions })
    }

    async fn find_people(&self, account: &str, query: &str, limit: Option<u32>) -> Result<Response, ChatError> {
        let query = query.trim();
        if query.chars().count() < MIN_QUERY {
            return Err(ChatError::Other(tr("A search needs at least two characters.").to_string()));
        }
        let (remote, ..) = self.ready(account).await?;
        let limit = limit.unwrap_or(MAX_PEOPLE).clamp(1, MAX_PEOPLE);
        let found = self.asked(account, remote.find_people(query, limit).await).await?;
        let people = found.into_iter().map(|person| super::person(account, person)).collect();
        Ok(Response::ChatPeople { people })
    }

    async fn create(self: &Arc<Self>, account: &str, with: &str) -> Result<Response, ChatError> {
        let user = match split_person(with) {
            Some((of, user)) if of == account => user,
            _ => return Err(ChatError::NotFound),
        };
        let (remote, ..) = self.ready(account).await?;
        let room = self.asked(account, remote.create_direct(user).await).await?;
        RoomToken::parse(&room.token).ok_or(ChatError::NotFound)?;
        let created = conversation(account, &room);
        let mut cache = self.cache();
        let entry = cache.account(account);
        match entry.room_mut(&room.token) {
            Some(held) => *held = room,
            None => entry.rooms.push(room),
        }
        Ok(Response::ChatConversation { conversation: Box::new(created) })
    }

    /// The message `number` of a conversation as the cache holds it, else asked of the server.
    async fn message(
        &self,
        account: &str,
        token: &RoomToken,
        number: u64,
        remote: &Remote,
        caps: &Caps,
    ) -> Result<RemoteMessage, ChatError> {
        let id = conversation_id(account, token.as_str());
        if let Some(held) = self.cache().peek(&id).and_then(|thread| thread.get(number)).cloned() {
            return Ok(held);
        }
        let fetched = remote.messages(token.as_str(), Some(number.saturating_add(1)), 1, caps).await;
        let fetched = self.asked(account, fetched).await?;
        fetched.into_iter().find(|message| message.number == number && !message.hidden).ok_or(ChatError::NotFound)
    }

    /// The own message `id` for an edit or a deletion; `unsupported` for one of someone else.
    async fn own_message(&self, id: &str, wanted: fn(&Caps) -> bool) -> Result<(Remote, RoomToken, u64), ChatError> {
        let (account, token, number) = message_of(id)?;
        let (remote, caps, user) = self.ready(account).await?;
        if !wanted(&caps) {
            return Err(unsupported("This server cannot change messages."));
        }
        let message = self.message(account, &token, number, &remote, &caps).await?;
        if message.author.as_deref() != Some(user.as_str()) {
            return Err(unsupported("Only your own messages can be changed."));
        }
        Ok((remote, token, number))
    }

    async fn edit(&self, id: &str, text: &str) -> Result<Response, ChatError> {
        if text.trim().is_empty() {
            return Err(ChatError::Other(tr("A message needs a text.").to_string()));
        }
        let (account, ..) = message_of(id)?;
        let (remote, token, number) = self.own_message(id, |caps| caps.edit).await?;
        self.asked(account, remote.edit(token.as_str(), number, text).await).await?;
        let patch = super::remote::Patch { number, text: Some(text.to_string()), deleted: false };
        self.patch(&conversation_id(account, token.as_str()), patch);
        Ok(Response::Ok)
    }

    async fn delete(&self, id: &str) -> Result<Response, ChatError> {
        let (account, ..) = message_of(id)?;
        let (remote, token, number) = self.own_message(id, |caps| caps.delete).await?;
        self.asked(account, remote.delete(token.as_str(), number).await).await?;
        let patch = super::remote::Patch { number, text: None, deleted: true };
        self.patch(&conversation_id(account, token.as_str()), patch);
        Ok(Response::Ok)
    }

    /// Applies `patch` to the message held in the cache, if it is held.
    fn patch(&self, conversation: &str, patch: super::remote::Patch) {
        if let Some(thread) = self.cache().peek_mut(conversation)
            && let Some(mut held) = thread.get(patch.number).cloned()
        {
            // A message that names itself changes itself: the cache's one rule for changes.
            held.patch = Some(patch);
            held.hidden = true;
            thread.merge(vec![held]);
        }
    }

    async fn react(&self, id: &str, emoji: &str, remove: bool) -> Result<Response, ChatError> {
        let (account, token, number) = message_of(id)?;
        let emoji = emoji.trim();
        // One emoji, possibly of several code points; never a text.
        if emoji.is_empty()
            || emoji.chars().count() > 16
            || emoji.chars().any(|c| c.is_alphanumeric() || c.is_control())
        {
            return Err(ChatError::Other(tr("A reaction is one emoji.").to_string()));
        }
        let (remote, caps, _) = self.ready(account).await?;
        if !caps.reactions {
            return Err(unsupported("This server has no reactions."));
        }
        self.asked(account, remote.react(token.as_str(), number, emoji, remove).await).await?;
        // The counts come with the next fetch: drop what is held as fresh.
        if let Some(thread) = self.cache().peek_mut(&conversation_id(account, token.as_str())) {
            thread.fetched = None;
        }
        Ok(Response::Ok)
    }

    async fn set_favorite(self: &Arc<Self>, id: &str, favorite: bool) -> Result<Response, ChatError> {
        let (account, token) = conversation_of(id)?;
        let (remote, ..) = self.ready(account).await?;
        self.room(account, &token).await?;
        self.asked(account, remote.set_favorite(token.as_str(), favorite).await).await?;
        if let Some(room) = self.cache().account(account).room_mut(token.as_str()) {
            room.favorite = favorite;
        }
        Ok(Response::Ok)
    }

    async fn set_notify(self: &Arc<Self>, id: &str, level: torro_chat::NotifyLevel) -> Result<Response, ChatError> {
        let (account, token) = conversation_of(id)?;
        let (remote, ..) = self.ready(account).await?;
        self.room(account, &token).await?;
        self.asked(account, remote.set_notify(token.as_str(), level).await).await?;
        if let Some(room) = self.cache().account(account).room_mut(token.as_str()) {
            room.notify = level;
        }
        Ok(Response::Ok)
    }

    async fn search(
        &self,
        reach: Reach<'_>,
        query: &str,
        account: Option<&str>,
        limit: Option<u32>,
    ) -> Result<Response, ChatError> {
        let query = query.trim();
        if query.chars().count() < MIN_QUERY {
            return Err(ChatError::Other(tr("A search needs at least two characters.").to_string()));
        }
        let limit = limit.unwrap_or(20).clamp(1, MAX_HITS);
        let mut hits = Vec::new();
        let mut failed = None;
        for id in self.accounts_within(reach, account).await {
            let found = match self.ready(&id).await {
                Ok((_, caps, _)) if !caps.search => Err(unsupported("This server cannot search messages.")),
                Ok((remote, ..)) => self.asked(&id, remote.search(query, limit).await).await,
                Err(err) => Err(err),
            };
            match found {
                Ok(found) => {
                    hits.extend(found.into_iter().filter(|hit| RoomToken::parse(&hit.token).is_some()).map(|hit| {
                        SearchHit {
                            id: message_id(&conversation_id(&id, &hit.token), hit.number),
                            title: hit.title,
                            text: hit.text,
                            untrusted: true,
                        }
                    }))
                }
                Err(ChatError::NotConnected) if account.is_none() => {}
                Err(err) => failed = Some(err),
            }
        }
        if let Some(err) = failed
            && hits.is_empty()
        {
            return Err(err);
        }
        hits.truncate(limit as usize);
        Ok(Response::ChatFound { hits })
    }

    /// What an approval of `request` shows, looked up by its ids: the conversation's name and
    /// kind, the author a reply quotes, the text an edit, a deletion or a reaction is about.
    /// `None` for requests of other areas. Never asks the server for more than one message.
    pub async fn facts(&self, request: &Request) -> Option<ChatFacts> {
        if request.operation().area() != torro_ipc::api::Area::Chat {
            return None;
        }
        let (account, token, number, quoted) = match request {
            Request::ChatSendMessage { conversation, reply_to, .. } => {
                let (account, token) = conversation_of(conversation).ok()?;
                let quoted = reply_to.as_deref().and_then(|id| number_in(conversation, id).ok());
                (account, token, None, quoted)
            }
            Request::ChatEditMessage { id, .. } | Request::ChatDeleteMessage { id } | Request::ChatReact { id, .. } => {
                let (account, token, number) = message_of(id).ok()?;
                (account, token, Some(number), None)
            }
            Request::ChatMarkRead { conversation, .. }
            | Request::ChatMarkUnread { conversation }
            | Request::ChatSetFavorite { conversation, .. }
            | Request::ChatSetNotificationLevel { conversation, .. } => {
                let (account, token) = conversation_of(conversation).ok()?;
                (account, token, None, None)
            }
            _ => return Some(ChatFacts::default()),
        };
        let room = self.cache().account(account).room(token.as_str()).cloned();
        let mut facts = ChatFacts {
            conversation: room.as_ref().map(|room| room.name.clone()),
            kind: room.as_ref().map(|room| room.kind),
            federated: room.as_ref().is_some_and(|room| room.federated),
            quoted: None,
            old_text: None,
        };
        if let Some(wanted) = number.or(quoted)
            && let Ok((remote, caps, _)) = self.ready(account).await
            && let Ok(message) = self.message(account, &token, wanted, &remote, &caps).await
        {
            match number {
                Some(_) => facts.old_text = Some(message.text),
                None => facts.quoted = Some(message.author_name),
            }
        }
        Some(facts)
    }
}

/// The id of the message the person read last in `room`.
fn last_read(conversation: &str, room: &RemoteRoom) -> Option<String> {
    room.last_read.map(|number| message_id(conversation, number))
}

fn unsupported(text: &'static str) -> ChatError {
    ChatError::Unsupported(tr(text).to_string())
}

/// A random id for one send (`referenceId`): 32 hex digits. With it the server drops a repeat of
/// the same send.
fn reference() -> String {
    let mut bytes = [0u8; 16];
    // Without random bytes the time stands in: the id only has to differ between sends.
    if getrandom::fill(&mut bytes).is_err() {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        bytes.copy_from_slice(&nanos.to_le_bytes());
    }
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use torro_chat::MessageKind;
    use torro_ipc::api::{Rights, RightsTemplate, Selection};

    use super::super::fake::Server;
    use super::super::remote::tests::message;
    use super::*;

    const ACCOUNT: &str = "nextcloud:anna@cloud.example.org";
    const OTHER: &str = "nextcloud:anna@other.example.org";

    fn ben() -> String {
        conversation_id(ACCOUNT, "ben4chat")
    }

    /// The service over one fake server for [`ACCOUNT`] and another for [`OTHER`].
    fn service() -> (Arc<ChatService>, Arc<Server>) {
        let server = Arc::new(Server::default());
        let other = Arc::new(Server::default());
        (Arc::new(ChatService::fixed(vec![(ACCOUNT.into(), Arc::clone(&server)), (OTHER.into(), other)])), server)
    }

    const APP: Caller<'static> = Caller { reach: Reach::Suite, client: None };

    fn code(response: &Response) -> &str {
        match response {
            Response::Error { code, .. } => code,
            _ => "",
        }
    }

    fn get(conversation: &str, before: Option<u64>, after: Option<u64>, limit: Option<u32>) -> Request {
        Request::ChatGetMessages {
            conversation: conversation.into(),
            before: before.map(|n| message_id(conversation, n)),
            after: after.map(|n| message_id(conversation, n)),
            limit,
        }
    }

    fn page(response: Response) -> MessagePage {
        match response {
            Response::ChatMessages(page) => page,
            other => panic!("no messages: {other:?}"),
        }
    }

    fn send(conversation: &str, text: &str) -> Request {
        Request::ChatSendMessage { conversation: conversation.into(), text: text.into(), reply_to: None, silent: false }
    }

    #[tokio::test]
    async fn conversations_are_listed_newest_first_without_the_providers_news() {
        let (chat, server) = service();
        server.state().rooms[1].last_activity += chrono::Duration::hours(1);
        let list = |unread_only, query: Option<&str>| Request::ChatListConversations {
            account: Some(ACCOUNT.into()),
            unread_only,
            query: query.map(str::to_string),
            limit: None,
        };
        let Response::ChatConversations { conversations } = chat.handle(list(false, None), APP).await else {
            panic!("a list")
        };
        let names: Vec<&str> = conversations.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["Team Alpha", "Ben Koch", "Note to self"], "newest first, no \"Talk updates\"");
        let ben = &conversations[1];
        assert_eq!(ben.id, self::ben());
        assert_eq!((ben.unread, ben.person.as_deref()), (2, Some("nextcloud:anna@cloud.example.org/ben")));
        assert_eq!(ben.last_read, Some(message_id(&self::ben(), 3)));
        assert!(ben.untrusted);
        let Response::ChatConversations { conversations } = chat.handle(list(true, None), APP).await else {
            panic!("a list")
        };
        assert_eq!(conversations.len(), 1, "only Ben has unread messages");
        let Response::ChatConversations { conversations } = chat.handle(list(false, Some("team")), APP).await else {
            panic!("a list")
        };
        assert_eq!(conversations.len(), 1);
        assert_eq!(server.count("rooms"), 1, "the list is asked once and then read from memory");
    }

    #[tokio::test]
    async fn reading_messages_changes_nothing_and_is_answered_from_memory() {
        let (chat, server) = service();
        let first = page(chat.handle(get(&ben(), None, None, Some(2)), APP).await);
        assert_eq!(first.messages.iter().map(Message::number).collect::<Vec<_>>(), [4, 5]);
        assert!(first.more && !first.offline);
        assert_eq!(first.last_read, Some(message_id(&ben(), 3)), "the divider goes after the last read one");
        assert!(first.messages.iter().all(|message| message.untrusted));
        let older = page(chat.handle(get(&ben(), Some(4), None, None), APP).await);
        assert_eq!(older.messages.iter().map(Message::number).collect::<Vec<_>>(), [1, 2, 3]);
        assert!(older.messages[1].author.own && !older.messages[0].author.own, "the person's own message");
        assert_eq!(older.messages[0].author.id.as_deref(), Some("nextcloud:anna@cloud.example.org/ben"));
        assert_eq!(server.count("messages"), 1, "one fetch serves every page that is held");
        assert_eq!(server.count("mark_read"), 0, "reading sets no read marker");
        let both = Request::ChatGetMessages {
            conversation: ben(),
            before: Some(message_id(&ben(), 4)),
            after: Some(message_id(&ben(), 1)),
            limit: None,
        };
        assert_eq!(code(&chat.handle(both, APP).await), "error");
        let foreign = Request::ChatGetMessages {
            conversation: ben(),
            before: Some(message_id(&conversation_id(ACCOUNT, "team4alpha"), 1)),
            after: None,
            limit: None,
        };
        assert_eq!(code(&chat.handle(foreign, APP).await), "error", "a message of another conversation");
    }

    #[tokio::test]
    async fn without_a_connection_the_memory_shows_and_nothing_is_sent_later() {
        let (chat, server) = service();
        page(chat.handle(get(&ben(), None, None, None), APP).await);
        server.state().offline = true;
        // The memory is asked again once it is no longer fresh.
        chat.cache().peek_mut(&ben()).expect("held").fetched = None;
        let held = page(chat.handle(get(&ben(), None, None, None), APP).await);
        assert!(held.offline, "marked as what the core still held");
        assert_eq!(held.messages.len(), 5);
        assert_eq!(code(&chat.handle(send(&ben(), "later?"), APP).await), "offline");
        assert!(server.state().sent.is_empty());
        // A conversation the list names but whose messages were never read shows empty, and says why.
        let unread = page(chat.handle(get(&conversation_id(ACCOUNT, "team4alpha"), None, None, None), APP).await);
        assert!(unread.offline && unread.messages.is_empty());
    }

    #[tokio::test]
    async fn a_message_is_sent_once_with_its_reference_and_shows_at_once() {
        let (chat, server) = service();
        let Response::ChatSent { id } = chat.handle(send(&ben(), "On it."), APP).await else { panic!("sent") };
        assert_eq!(id, message_id(&ben(), 6));
        let (token, sending) = server.state().sent[0].clone();
        assert_eq!((token.as_str(), sending.text.as_str(), sending.reference.len()), ("ben4chat", "On it.", 32));
        let after = page(chat.handle(get(&ben(), None, None, Some(1)), APP).await);
        assert_eq!(after.messages[0].text, "On it.");
        assert!(after.messages[0].author.own);
        let Response::ChatConversations { conversations } = chat
            .handle(Request::ChatListConversations { account: None, unread_only: false, query: None, limit: None }, APP)
            .await
        else {
            panic!("a list")
        };
        let held = conversations.iter().find(|c| c.id == ben()).expect("Ben");
        assert_eq!((held.unread, held.last_message.as_ref().map(|m| m.text.as_str())), (0, Some("On it.")));

        assert_eq!(code(&chat.handle(send(&ben(), "  "), APP).await), "error", "an empty text");
        assert_eq!(code(&chat.handle(send(&ben(), &"x".repeat(1001)), APP).await), "error", "too long");
        let reply = Request::ChatSendMessage {
            conversation: ben(),
            text: "re".into(),
            reply_to: Some(message_id(&conversation_id(ACCOUNT, "team4alpha"), 1)),
            silent: false,
        };
        assert_eq!(code(&chat.handle(reply, APP).await), "error", "a reply quotes its own conversation");
        server.state().rooms[0].read_only = true;
        chat.refresh_rooms(ACCOUNT).await.expect("refreshed");
        assert_eq!(code(&chat.handle(send(&ben(), "hello"), APP).await), "unsupported");
        assert_eq!(server.state().sent.len(), 1, "nothing but the first went out");
    }

    #[tokio::test]
    async fn a_client_reaches_only_the_accounts_of_its_rights() {
        let (chat, _) = service();
        let mut rights = RightsTemplate::FullAccess.rights();
        assert!(chat.out_of_reach(Reach::Client(&rights), &get(&ben(), None, None, None)).is_some(), "no account");
        rights.chat_accounts = Selection::Only(vec![ACCOUNT.into()]);
        let reach = Reach::Client(&rights);
        assert_eq!(chat.out_of_reach(reach, &get(&ben(), None, None, None)), None);
        let elsewhere = conversation_id(OTHER, "ben4chat");
        for outside in [
            get(&elsewhere, None, None, None),
            send(&elsewhere, "hi"),
            Request::ChatEditMessage { id: message_id(&elsewhere, 2), text: "x".into() },
            Request::ChatFindPeople { account: OTHER.into(), query: "ben".into(), limit: None },
            Request::ChatSearchMessages { query: "one".into(), account: Some(OTHER.into()), limit: None },
            // An id that names nothing, and a person of another account than the one asked.
            get("nonsense", None, None, None),
            get(&conversation_id(ACCOUNT, "UPPER-case!"), None, None, None),
            Request::ChatCreateConversation { account: ACCOUNT.into(), with: person_id(OTHER, "ben") },
        ] {
            assert!(chat.out_of_reach(reach, &outside).is_some(), "{outside:?}");
        }
        assert_eq!(chat.out_of_reach(Reach::Suite, &get(&elsewhere, None, None, None)), None, "the app key");
        // Lists and searches over every account take the accounts in reach only.
        let caller = Caller { reach, client: Some("c1") };
        let all = Request::ChatListConversations { account: None, unread_only: false, query: None, limit: None };
        let Response::ChatConversations { conversations } = chat.handle(all, caller).await else { panic!("a list") };
        assert!(!conversations.is_empty() && conversations.iter().all(|c| c.account == ACCOUNT));
        let search = Request::ChatSearchMessages { query: "one".into(), account: None, limit: None };
        let Response::ChatFound { hits } = chat.handle(search, caller).await else { panic!("hits") };
        assert_eq!(hits.len(), 1);
        assert!(hits[0].id.starts_with(ACCOUNT) && hits[0].untrusted);
    }

    #[tokio::test]
    async fn a_client_is_throttled_and_the_app_is_not() {
        let (chat, server) = service();
        let rights = Rights::none();
        let agent = Caller { reach: Reach::Client(&rights), client: Some("agent") };
        for n in 0..crate::chat::throttle::PER_MINUTE {
            assert!(matches!(chat.handle(send(&ben(), "hi"), agent).await, Response::ChatSent { .. }), "{n}");
        }
        assert_eq!(code(&chat.handle(send(&ben(), "hi"), agent).await), "busy");
        assert!(matches!(chat.handle(send(&ben(), "hi"), APP).await, Response::ChatSent { .. }), "the app key");
        // Reading and the read marker are no sending and are never counted.
        assert!(matches!(chat.handle(get(&ben(), None, None, None), agent).await, Response::ChatMessages(_)));
        let read = Request::ChatMarkRead { conversation: ben(), up_to: None };
        assert_eq!(chat.handle(read, agent).await, Response::Ok);
        assert_eq!(server.state().sent.len(), crate::chat::throttle::PER_MINUTE + 1);
    }

    #[tokio::test]
    async fn the_read_marker_moves_only_when_asked() {
        let (chat, server) = service();
        let read = |up_to: Option<u64>| Request::ChatMarkRead {
            conversation: ben(),
            up_to: up_to.map(|n| message_id(&ben(), n)),
        };
        assert_eq!(chat.handle(read(Some(4)), APP).await, Response::Ok);
        assert_eq!(server.state().rooms[0].last_read, Some(4));
        assert_eq!(chat.handle(read(None), APP).await, Response::Ok);
        assert_eq!(server.state().rooms[0].last_read, Some(5), "up to the newest the list names");
        assert_eq!(chat.handle(Request::ChatMarkUnread { conversation: ben() }, APP).await, Response::Ok);
        assert_eq!(server.count("mark_unread"), 1);
        server.state().caps.read_marker = false;
        chat.cache().account(ACCOUNT).known_at = None;
        assert_eq!(code(&chat.handle(read(None), APP).await), "unsupported", "a server without the read marker");
    }

    #[tokio::test]
    async fn only_own_messages_are_edited_and_deleted() {
        let (chat, server) = service();
        let edit = |n: u64, text: &str| Request::ChatEditMessage { id: message_id(&ben(), n), text: text.into() };
        assert_eq!(code(&chat.handle(edit(1, "mine now"), APP).await), "unsupported", "Ben's message");
        assert_eq!(code(&chat.handle(edit(99, "x"), APP).await), "not-found");
        assert_eq!(chat.handle(edit(2, "two, corrected"), APP).await, Response::Ok);
        assert_eq!(server.count("edit ben4chat 2"), 1);
        page(chat.handle(get(&ben(), None, None, None), APP).await);
        assert_eq!(chat.handle(Request::ChatDeleteMessage { id: message_id(&ben(), 2) }, APP).await, Response::Ok);
        let held = page(chat.handle(get(&ben(), None, None, None), APP).await);
        let deleted = held.messages.iter().find(|message| message.number() == 2).expect("kept as deleted");
        assert_eq!((deleted.kind, deleted.text.as_str()), (MessageKind::Deleted, ""));
        let react = |emoji: &str| Request::ChatReact { id: message_id(&ben(), 1), emoji: emoji.into(), remove: false };
        assert_eq!(chat.handle(react("👍"), APP).await, Response::Ok);
        assert_eq!(code(&chat.handle(react("ok"), APP).await), "error", "a word is no reaction");
    }

    #[tokio::test]
    async fn an_app_watching_a_conversation_gets_what_comes_and_a_client_does_not() {
        let (chat, server) = service();
        page(chat.handle(get(&ben(), None, None, None), APP).await);
        // A client asking for what is new notes nothing: no live request starts.
        let rights = Rights::none();
        let agent = Caller { reach: Reach::Client(&rights), client: Some("agent") };
        page(chat.handle(get(&ben(), None, Some(5), None), agent).await);
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
        assert_eq!(server.count("wait"), 0);
        // The app does: what the server gets next is in the memory without another fetch.
        assert!(page(chat.handle(get(&ben(), None, Some(5), None), APP).await).messages.is_empty());
        server.state().messages.get_mut("ben4chat").expect("Ben").push(message(6, "six"));
        let mut new = Vec::new();
        for _ in 0..100 {
            new = page(chat.handle(get(&ben(), None, Some(5), None), APP).await).messages;
            if !new.is_empty() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        assert_eq!(new.iter().map(|message| message.text.as_str()).collect::<Vec<_>>(), ["six"]);
        assert!(server.count("wait") > 0);
        assert_eq!(server.count("messages"), 1, "only the first look fetched");
    }

    #[tokio::test]
    async fn approvals_get_their_facts_from_the_service() {
        let (chat, _) = service();
        chat.refresh_rooms(ACCOUNT).await.expect("rooms");
        let reply = Request::ChatSendMessage {
            conversation: ben(),
            text: "the client's words".into(),
            reply_to: Some(message_id(&ben(), 1)),
            silent: false,
        };
        let facts = chat.facts(&reply).await.expect("facts");
        assert_eq!(facts.conversation.as_deref(), Some("Ben Koch"));
        assert_eq!((facts.kind, facts.quoted.as_deref()), (Some(ConversationKind::Direct), Some("Ben Koch")));
        let delete = Request::ChatDeleteMessage { id: message_id(&ben(), 2) };
        assert_eq!(chat.facts(&delete).await.expect("facts").old_text.as_deref(), Some("two"));
        let unknown = send(&conversation_id(ACCOUNT, "gone4ever"), "hi");
        assert_eq!(chat.facts(&unknown).await.expect("facts").conversation, None, "an unknown conversation");
        assert_eq!(chat.facts(&Request::Ping).await, None);
    }

    #[tokio::test]
    async fn a_forgotten_account_takes_everything_with_it() {
        let (chat, server) = service();
        page(chat.handle(get(&ben(), None, None, None), APP).await);
        chat.forget_account(ACCOUNT);
        assert!(chat.cache().peek(&ben()).is_none());
        assert!(!chat.cache().accounts.contains_key(ACCOUNT));
        page(chat.handle(get(&ben(), None, None, None), APP).await);
        assert_eq!(server.count("messages"), 2, "nothing was kept: it is fetched again");
    }
}
