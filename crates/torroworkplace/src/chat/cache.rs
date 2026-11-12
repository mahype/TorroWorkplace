//! What the core holds of the chat, in memory only (decision 7 in `docs/chat.md`): the
//! conversations of each account as the server last listed them, and the newest messages of the
//! conversations someone read lately.
//!
//! - At most [`MESSAGES_KEPT`] messages per conversation, of at most [`THREADS_KEPT`]
//!   conversations, each kept for [`KEPT_FOR`] after it was last read. Nothing is written to disk:
//!   the history lives on the server, and a second archive of private chats on this computer would
//!   be a risk without a use.
//! - A system message that only changes another one (an edit, a deletion, a reaction) is not kept:
//!   it changes the message it names ([`Patch`]).
//! - Time comes in from the caller, so the rules are tested without waiting.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::time::{Duration, Instant};

use torro_chat::MessageKind;

use super::remote::{Caps, RemoteMessage, RemoteRing, RemoteRoom};

/// The most messages kept per conversation.
pub const MESSAGES_KEPT: usize = 200;

/// The most conversations whose messages are kept.
pub const THREADS_KEPT: usize = 20;

/// How long the messages of a conversation stay after they were last read.
pub const KEPT_FOR: Duration = Duration::from_secs(30 * 60);

/// How old the list of conversations or the newest messages may be before they are asked for again.
pub const FRESH_FOR: Duration = Duration::from_secs(10);

/// How long a conversation stays noted for live updates after an app last asked for what is new.
pub const WATCHED_FOR: Duration = Duration::from_secs(90);

/// How long an account's capabilities and the person's user id are trusted before they are asked
/// for again.
pub const KNOWN_FOR: Duration = Duration::from_secs(60 * 60);

/// What is known of one account.
#[derive(Debug, Default)]
pub struct Account {
    /// The person's user id on the server (not the login name), once asked.
    pub user: Option<String>,
    /// The person's name on the server, once asked.
    pub name: Option<String>,
    pub caps: Option<Caps>,
    /// When capabilities and user id were last asked.
    pub known_at: Option<Instant>,
    pub rooms: Vec<RemoteRoom>,
    /// When the list of conversations was last fetched.
    pub rooms_at: Option<Instant>,
    /// The calls that ring for the person, as the last look at the notifications found them (stage 6).
    pub rings: Vec<RemoteRing>,
    /// The notifications of calls the ringing window was opened for already, so each opens it once.
    pub announced: BTreeSet<u64>,
}

impl Account {
    pub fn room(&self, token: &str) -> Option<&RemoteRoom> {
        self.rooms.iter().find(|room| room.token == token)
    }

    pub fn room_mut(&mut self, token: &str) -> Option<&mut RemoteRoom> {
        self.rooms.iter_mut().find(|room| room.token == token)
    }

    /// Whether capabilities and user id were asked less than [`KNOWN_FOR`] ago.
    pub fn known_fresh(&self) -> bool {
        self.known_at.is_some_and(|at| at.elapsed() < KNOWN_FOR)
    }

    /// Whether the list of conversations is younger than [`FRESH_FOR`].
    pub fn rooms_fresh(&self, now: Instant) -> bool {
        self.rooms_at.is_some_and(|at| now.duration_since(at) < FRESH_FOR)
    }
}

/// The messages kept of one conversation, by number, each once.
#[derive(Debug)]
pub struct Thread {
    messages: VecDeque<RemoteMessage>,
    /// When someone last read it.
    used: Instant,
    /// When its newest messages were last confirmed with the server.
    pub fetched: Option<Instant>,
    /// Older messages than the first one kept exist on the server (or may).
    pub more: bool,
    /// An app asked for what is new until then: live updates run for it.
    pub watched_until: Option<Instant>,
    /// A live request for it is open.
    pub polling: bool,
    /// The number of the newest message the server sent, kept or not: hidden ones (reactions,
    /// edits) are not kept, and a wait after the newest kept one would get them again and again.
    seen: Option<u64>,
}

impl Thread {
    fn new(now: Instant) -> Thread {
        Thread {
            messages: VecDeque::new(),
            used: now,
            fetched: None,
            more: true,
            watched_until: None,
            polling: false,
            seen: None,
        }
    }

    /// Whether the newest messages were confirmed less than [`FRESH_FOR`] ago, or live updates
    /// keep them current.
    pub fn fresh(&self, now: Instant) -> bool {
        self.polling || self.fetched.is_some_and(|at| now.duration_since(at) < FRESH_FOR)
    }

    pub fn watched(&self, now: Instant) -> bool {
        self.watched_until.is_some_and(|until| now < until)
    }

    /// The number of the newest message kept.
    pub fn newest(&self) -> Option<u64> {
        self.messages.back().map(|message| message.number)
    }

    /// Where a wait for new messages starts: after the newest message the server sent, also a
    /// hidden one ([`Thread::merge`] keeps none of those).
    pub fn wait_after(&self) -> Option<u64> {
        self.newest().max(self.seen)
    }

    pub fn is_empty(&self) -> bool {
        self.messages.is_empty()
    }

    pub fn get(&self, number: u64) -> Option<&RemoteMessage> {
        self.messages.iter().find(|message| message.number == number)
    }

    /// Takes `messages` in: a message that only changes another one changes it and is dropped,
    /// every other one is kept once, in order. Beyond [`MESSAGES_KEPT`] the oldest go.
    pub fn merge(&mut self, messages: Vec<RemoteMessage>) {
        for message in messages {
            self.seen = self.seen.max(Some(message.number));
            if let Some(patch) = &message.patch
                && let Some(target) = self.messages.iter_mut().find(|held| held.number == patch.number)
            {
                if patch.deleted {
                    target.kind = MessageKind::Deleted;
                    target.text.clear();
                    target.files.clear();
                    target.mentions.clear();
                } else if let Some(text) = &patch.text {
                    target.text.clone_from(text);
                    target.edited = true;
                }
                if let Some((reactions, own)) = &patch.reactions {
                    target.reactions.clone_from(reactions);
                    target.own_reactions.clone_from(own);
                }
            }
            if message.hidden {
                continue;
            }
            match self.messages.binary_search_by_key(&message.number, |held| held.number) {
                Ok(at) => self.messages[at] = message,
                Err(at) => self.messages.insert(at, message),
            }
        }
        while self.messages.len() > MESSAGES_KEPT {
            self.messages.pop_front();
            self.more = true;
        }
    }

    /// The newest `limit` messages, the oldest first, and whether older ones exist.
    pub fn last(&self, limit: usize) -> (Vec<RemoteMessage>, bool) {
        let from = self.messages.len().saturating_sub(limit);
        (self.messages.iter().skip(from).cloned().collect(), from > 0 || self.more)
    }

    /// Up to `limit` messages newer than `number`, the oldest first.
    pub fn after(&self, number: u64, limit: usize) -> Vec<RemoteMessage> {
        self.messages.iter().filter(|message| message.number > number).take(limit).cloned().collect()
    }

    /// Up to `limit` messages older than `number`, the oldest first, and whether older ones exist.
    pub fn before(&self, number: u64, limit: usize) -> (Vec<RemoteMessage>, bool) {
        let older: Vec<&RemoteMessage> = self.messages.iter().filter(|message| message.number < number).collect();
        let from = older.len().saturating_sub(limit);
        (older[from..].iter().map(|message| (*message).clone()).collect(), from > 0 || self.more)
    }
}

/// See the module docs.
#[derive(Debug, Default)]
pub struct Cache {
    pub accounts: BTreeMap<String, Account>,
    threads: BTreeMap<String, Thread>,
}

impl Cache {
    pub fn account(&mut self, id: &str) -> &mut Account {
        self.accounts.entry(id.to_string()).or_default()
    }

    /// The messages kept of `conversation`, if any, without counting as a read.
    pub fn peek(&self, conversation: &str) -> Option<&Thread> {
        self.threads.get(conversation)
    }

    pub fn peek_mut(&mut self, conversation: &str) -> Option<&mut Thread> {
        self.threads.get_mut(conversation)
    }

    /// The messages of `conversation` for a read at `now`: it counts as used, and what was not
    /// read for [`KEPT_FOR`] or is beyond [`THREADS_KEPT`] (the longest unused first) goes. A
    /// conversation with a live request open is never dropped under it.
    pub fn thread(&mut self, conversation: &str, now: Instant) -> &mut Thread {
        self.threads
            .retain(|id, thread| id == conversation || thread.polling || now.duration_since(thread.used) < KEPT_FOR);
        while self.threads.len() >= THREADS_KEPT && !self.threads.contains_key(conversation) {
            let oldest = self
                .threads
                .iter()
                .filter(|(_, thread)| !thread.polling)
                .min_by_key(|(_, thread)| thread.used)
                .map(|(id, _)| id.clone());
            match oldest {
                Some(id) => self.threads.remove(&id),
                None => break,
            };
        }
        let thread = self.threads.entry(conversation.to_string()).or_insert_with(|| Thread::new(now));
        thread.used = now;
        thread
    }

    /// The conversations noted for live updates at `now` that have no live request open.
    pub fn waiting(&self, now: Instant) -> Vec<String> {
        self.threads
            .iter()
            .filter(|(_, thread)| thread.watched(now) && !thread.polling)
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// How many live requests are open.
    pub fn polling(&self) -> usize {
        self.threads.values().filter(|thread| thread.polling).count()
    }

    /// Drops everything of `account`: its conversations and their messages (account removed or
    /// wiped).
    pub fn forget_account(&mut self, account: &str) {
        self.accounts.remove(account);
        let prefix = format!("{account}/");
        self.threads.retain(|id, _| !id.starts_with(&prefix));
    }

    /// Drops everything of accounts not among `known`.
    pub fn keep_accounts(&mut self, known: &[String]) {
        let gone: Vec<String> = self.accounts.keys().filter(|id| !known.contains(id)).cloned().collect();
        for account in gone {
            self.forget_account(&account);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::remote::Patch;
    use super::super::remote::tests::message;
    use super::*;

    #[test]
    fn messages_are_kept_once_in_order_and_only_the_newest() {
        let now = Instant::now();
        let mut cache = Cache::default();
        let thread = cache.thread("a/tok1", now);
        thread.more = false;
        thread.merge(vec![message(3, "c"), message(1, "a")]);
        thread.merge(vec![message(2, "b"), message(3, "c again")]);
        assert_eq!(
            thread.last(10).0.iter().map(|m| (m.number, m.text.as_str())).collect::<Vec<_>>(),
            [(1, "a"), (2, "b"), (3, "c again")]
        );
        assert_eq!(thread.after(1, 10).len(), 2);
        assert_eq!(thread.before(3, 1).0.first().map(|m| m.number), Some(2));
        assert!(thread.before(3, 1).1, "one older message is left");
        assert!(!thread.last(10).1);
        thread.merge((10..10 + MESSAGES_KEPT as u64).map(|n| message(n, "x")).collect());
        assert_eq!(thread.last(usize::MAX).0.len(), MESSAGES_KEPT);
        assert_eq!(thread.newest(), Some(9 + MESSAGES_KEPT as u64));
        assert!(thread.more, "what fell out is on the server");
    }

    /// Review of PR #644: someone else's reaction comes as a hidden message whose parent carries the
    /// reactions as they are now; the message reacted to shows them, also while a window watches.
    #[test]
    fn a_reaction_of_someone_else_changes_the_message_it_names() {
        let mut cache = Cache::default();
        let thread = cache.thread("a/tok1", Instant::now());
        thread.merge(vec![message(3, "c")]);
        let reactions = BTreeMap::from([("👍".to_string(), 2)]);
        let reaction = RemoteMessage {
            hidden: true,
            patch: Some(Patch {
                number: 3,
                text: None,
                deleted: false,
                reactions: Some((reactions.clone(), vec!["👍".to_string()])),
            }),
            ..message(4, "")
        };
        thread.merge(vec![reaction]);
        let (kept, _) = thread.last(10);
        assert_eq!((kept[0].reactions.clone(), kept[0].own_reactions.clone()), (reactions, vec!["👍".to_string()]));
        assert_eq!(kept.len(), 1, "the reaction itself is not shown");
    }

    /// Review of #643: a hidden message (a reaction) is not kept, but a wait starts after it, else
    /// the server would answer the same reaction at once again and again.
    #[test]
    fn a_wait_starts_after_a_hidden_message_too() {
        let mut cache = Cache::default();
        let thread = cache.thread("a/tok1", Instant::now());
        thread.merge(vec![message(3, "c")]);
        let reaction = RemoteMessage { hidden: true, ..message(4, "") };
        thread.merge(vec![reaction]);
        assert_eq!((thread.newest(), thread.wait_after()), (Some(3), Some(4)));
    }

    #[test]
    fn edits_and_deletions_change_the_message_they_name_and_are_not_shown() {
        let mut cache = Cache::default();
        let thread = cache.thread("a/tok1", Instant::now());
        thread.merge(vec![message(1, "first"), message(2, "second")]);
        let mut edit = message(3, "You edited a message");
        edit.hidden = true;
        edit.patch = Some(Patch { number: 1, text: Some("first, corrected".into()), deleted: false, reactions: None });
        let mut deletion = message(4, "You deleted a message");
        deletion.hidden = true;
        deletion.patch = Some(Patch { number: 2, text: None, deleted: true, reactions: None });
        thread.merge(vec![edit, deletion]);
        let (kept, _) = thread.last(10);
        assert_eq!(kept.len(), 2);
        assert_eq!((kept[0].text.as_str(), kept[0].edited), ("first, corrected", true));
        assert_eq!((kept[1].kind, kept[1].text.as_str()), (MessageKind::Deleted, ""));
    }

    #[test]
    fn only_some_conversations_are_kept_and_only_for_a_while() {
        let start = Instant::now();
        let mut cache = Cache::default();
        for n in 0..THREADS_KEPT {
            cache.thread(&format!("a/tok{n}"), start + Duration::from_secs(n as u64)).merge(vec![message(1, "x")]);
        }
        // One more pushes out the one unused the longest, unless a live request is open for it.
        cache.peek_mut("a/tok0").expect("kept").polling = true;
        cache.thread("a/new1", start + Duration::from_secs(100));
        assert!(cache.peek("a/tok0").is_some() && cache.peek("a/tok1").is_none() && cache.peek("a/tok2").is_some());
        // After half an hour without a read the rest goes too.
        cache.peek_mut("a/tok0").expect("kept").polling = false;
        cache.thread("a/new2", start + KEPT_FOR + Duration::from_secs(200));
        assert!(cache.peek("a/tok0").is_none() && cache.peek("a/tok2").is_none() && cache.peek("a/new2").is_some());
    }

    #[test]
    fn an_account_takes_its_conversations_with_it() {
        let now = Instant::now();
        let mut cache = Cache::default();
        cache.account("nextcloud:a@x").rooms_at = Some(now);
        cache.account("nextcloud:b@x");
        cache.thread("nextcloud:a@x/tok1", now);
        cache.thread("nextcloud:b@x/tok1", now);
        cache.forget_account("nextcloud:a@x");
        assert!(cache.peek("nextcloud:a@x/tok1").is_none() && cache.peek("nextcloud:b@x/tok1").is_some());
        cache.keep_accounts(&[]);
        assert!(cache.accounts.is_empty() && cache.peek("nextcloud:b@x/tok1").is_none());
    }

    #[test]
    fn watching_ends_by_itself() {
        let now = Instant::now();
        let mut cache = Cache::default();
        cache.thread("a/tok1", now).watched_until = Some(now + WATCHED_FOR);
        assert_eq!(cache.waiting(now), ["a/tok1"]);
        assert!(cache.waiting(now + WATCHED_FOR).is_empty());
        cache.peek_mut("a/tok1").expect("kept").polling = true;
        assert!(cache.waiting(now).is_empty(), "it has its live request");
        assert_eq!(cache.polling(), 1);
    }
}
