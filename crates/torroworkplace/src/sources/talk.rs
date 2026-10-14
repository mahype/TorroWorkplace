//! Nextcloud Talk as a source of the inbox (task E5.9): one entry per unread conversation of each
//! Nextcloud account with Talk, `talk:<login>@<host>`, a source of its own so it can be granted,
//! muted and shown apart from the account's notifications.
//!
//! - **No loop of its own:** Talk runs in the loop of its Nextcloud account
//!   (`super::nextcloud`), which shares the sign-in, the capabilities (`spreed`) and the refusals
//!   with it; each round of that loop calls [`Talk::round`] unless this source is muted. A server
//!   without Talk has no Talk source, and no health is shown for one.
//! - **Entry** ([`report`]), where `rules::talk_place` says so ([`place`]): kind `message`, matter
//!   the conversation's token, title its name, text "Anna: …" from the last message, from its
//!   author, link `<server>/call/<token>` (on the account's own origin, rule 15 of E5), traits
//!   `person` and `kind` (`direct`, `mention`, `group`). Changelog and notes to oneself never.
//! - **Counter = unread messages:** the room list names only the last message, so every message
//!   new since the last round is a report with an id of its own ([`ids`]): `<token>/<message id>`
//!   for the last, `<token>/<message id>~<n>` for the `n`-th before it (a federated message has no
//!   id: `@<last activity>` stands in). How many are new comes from the cursor `room:<token>`
//!   ([`Mark`]): the read marker and the unread messages of the last round. At most
//!   [`BATCH_MAX`] reports per round and conversation.
//! - **Resolved by itself:** a conversation without unread messages (read elsewhere), one
//!   `talk_place` no longer wants or one gone from the list takes all ids off its entry, which is
//!   then done. A conversation read up to a point elsewhere keeps its entry and counter.
//! - **Reaching back** ([`settle_done`]): marks the conversation read up to the newest message
//!   among the entry's ids and deletes Talk's chat notifications of it up to the entry's last
//!   report; newer messages stay unread and open the entry again. Done without a read marker on
//!   the server (or when it failed) is not opened again by the same unread messages, as the
//!   cursor already counted them.
//! - **Muted:** asks nothing; the first round heard again only takes the cursors, so what came
//!   meanwhile stays out (in memory only: after a restart it is filed).
//! - **No replies** in E5 (question 8): an entry opens the conversation in Talk.
//!
//! Logs name the account's id and a code, never a title, a text, a person or a token.

use std::collections::BTreeSet;

use chrono::{DateTime, Utc};
use torro_core::i18n::trf;
use torro_inbox::item::{Facet, FacetKey};
use torro_inbox::rules::{self, TalkLevel, TalkRoom};
use torro_inbox::{Item, Kind, Lane, Sender, SourceRef};
use torro_nextcloud::{Listing, Nextcloud, NotificationLevel, OcsError, Room, RoomKind, RoomToken};

use super::Sources;
use super::nextcloud::{code, is_signed_out, keep_cursor, read_cursor};
use crate::inbox::{Remote, RemoteEntry};

/// The most reports filed for one conversation in one round: a conversation with hundreds of
/// unread messages costs no more, and its counter stops there.
const BATCH_MAX: u64 = 99;

/// Talk has no loop of its own: it runs in the loop of its Nextcloud account
/// (`super::nextcloud::start`), so nothing starts here.
pub(super) fn start(_sources: Sources) {}

/// The Talk source of the Nextcloud account whose source is `nextcloud`: `talk:<login>@<host>`.
pub(super) fn source_of(nextcloud: &SourceRef) -> SourceRef {
    SourceRef::talk(nextcloud.name.as_str())
}

/// What Talk keeps between the rounds of its account's loop, in memory.
#[derive(Debug, Default)]
pub(super) struct Talk {
    /// The source was muted at the last round: the next one heard again files nothing.
    muted: bool,
}

/// What the cursor `room:<token>` keeps of a conversation that has an entry: the read marker
/// (`lastReadMessage`, 0 without one) and the unread messages the inbox counted with it. Stored as
/// `<read>:<unread>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Mark {
    read: u64,
    unread: u64,
}

impl Mark {
    fn of(room: &Room) -> Mark {
        Mark { read: room.last_read.unwrap_or(0), unread: room.unread_messages }
    }

    fn parse(text: &str) -> Option<Mark> {
        let (read, unread) = text.split_once(':')?;
        Some(Mark { read: read.parse().ok()?, unread: unread.parse().ok()? })
    }

    fn text(self) -> String {
        format!("{}:{}", self.read, self.unread)
    }

    /// How many of the unread messages of `now` are new since `self` was kept: those beyond the
    /// counted ones while the read marker stayed, all of them once it moved (read elsewhere up to
    /// a point, or done here and newer ones came).
    fn new_in(self, now: Mark) -> u64 {
        match self.read == now.read {
            true => now.unread.saturating_sub(self.unread),
            false => now.unread,
        }
    }
}

impl Talk {
    /// Notes that the source is muted this round (nothing is asked then).
    pub(super) fn mute(&mut self) {
        self.muted = true;
    }

    /// One round for the Talk source `source` with `client` at `now`: lists the conversations,
    /// files what is new where [`place`] wants an entry and resolves the entries of the others
    /// (see the module docs). The first round after a muted one only keeps the cursors. Errors as
    /// [`Nextcloud::rooms`] (`MissingApp` when Talk is not there); nothing is filed for an account
    /// removed while asking.
    pub(super) async fn round(
        &mut self,
        sources: &Sources,
        source: &SourceRef,
        client: &Nextcloud,
        now: DateTime<Utc>,
    ) -> Result<(), OcsError> {
        let rooms = client.rooms().await?;
        // Removed while asking: its entries were retired, none is filed again.
        let account = SourceRef::nextcloud(source.name.as_str()).key();
        if !sources.pim.nextcloud_accounts().await.contains(&account) {
            return Ok(());
        }
        let quiet_round = std::mem::take(&mut self.muted);
        let mut unread = BTreeSet::new();
        for room in &rooms {
            let cursor = format!("room:{}", room.token);
            let Some(lane) = place(room).filter(|_| room.unread_messages > 0) else {
                keep_cursor(sources, source, &cursor, None);
                continue;
            };
            unread.insert(room.token.as_str());
            let mark = Mark::of(room);
            let new = read_cursor(sources, source, &cursor)
                .and_then(|kept| Mark::parse(&kept))
                .map_or(mark.unread, |kept| kept.new_in(mark));
            let filed = quiet_round || file(sources, source, client.server(), room, lane, new, now);
            if filed {
                keep_cursor(sources, source, &cursor, Some(&mark.text()));
            }
        }
        resolve(sources, source, &unread, now);
        Ok(())
    }
}

/// The lane of the entry for `room`, as far as Talk's notification level for it asks for one
/// (`rules::talk_place`; a mention also by `@all`). `None`: no entry, also for Talk's changelog,
/// notes to oneself and kinds this code does not know.
fn place(room: &Room) -> Option<Lane> {
    let kind = talk_room(room.kind)?;
    rules::talk_place(kind, mentioned(room), level(room.notification_level))
}

fn talk_room(kind: RoomKind) -> Option<TalkRoom> {
    match kind {
        RoomKind::OneToOne => Some(TalkRoom::Direct),
        RoomKind::Group | RoomKind::Public | RoomKind::FormerOneToOne => Some(TalkRoom::Group),
        RoomKind::Changelog | RoomKind::NoteToSelf | RoomKind::Other => None,
    }
}

fn level(level: NotificationLevel) -> TalkLevel {
    match level {
        NotificationLevel::Always => TalkLevel::Always,
        NotificationLevel::Mention => TalkLevel::Mentions,
        NotificationLevel::Never => TalkLevel::Never,
        NotificationLevel::Default | NotificationLevel::Other => TalkLevel::Default,
    }
}

fn mentioned(room: &Room) -> bool {
    room.unread_mention || room.unread_mention_direct
}

/// Files `new` reports of `room` (at most [`BATCH_MAX`]), the oldest first, each with an id of
/// its own ([`ids`]). Whether every one was filed.
fn file(
    sources: &Sources,
    source: &SourceRef,
    server: &str,
    room: &Room,
    lane: Lane,
    new: u64,
    now: DateTime<Utc>,
) -> bool {
    let mut filed = true;
    for id in ids(room, new).into_iter().rev() {
        if let Err(err) = sources.inbox.file_remote(&report(source, server, room, lane, id), now) {
            eprintln!("torroworkplace: inbox: cannot file a conversation of {source}: {err}");
            filed = false;
        }
    }
    filed
}

/// The ids of the `new` last messages of `room`, the last one first (see the module docs).
fn ids(room: &Room, new: u64) -> Vec<String> {
    let last = match room.last_message.as_ref().and_then(|message| message.id) {
        Some(id) => id.to_string(),
        None => format!("@{}", room.last_activity.timestamp()),
    };
    (0..new.min(BATCH_MAX))
        .map(|before| match before {
            0 => format!("{}/{last}", room.token),
            before => format!("{}/{last}~{before}", room.token),
        })
        .collect()
}

/// The message id an id of [`ids`] names: the number after the token, `None` for a stand-in.
fn message_id(id: &str) -> Option<u64> {
    let (_, message) = id.split_once('/')?;
    message.split('~').next()?.parse().ok()
}

/// The report of `room` of `source` on `server` in `lane` under the server's id `id`.
fn report(source: &SourceRef, server: &str, room: &Room, lane: Lane, id: String) -> Remote {
    let last = room.last_message.as_ref();
    let author = last.map(|message| message.author_name.clone()).filter(|name| !name.is_empty());
    let body = last.filter(|message| !message.text.trim().is_empty()).map(|message| match (&author, message.system) {
        (Some(name), false) => trf("{name}: {text}", &[("name", name), ("text", &message.text)]),
        _ => message.text.clone(),
    });
    let mention = mentioned(room);
    let kind = talk_room(room.kind).unwrap_or(TalkRoom::Group).facet(mention);
    let facets = [
        Some(Facet { key: FacetKey::Kind, value: kind.into() }),
        author.clone().map(|name| Facet { key: FacetKey::Person, value: name }),
    ];
    Remote {
        source: source.clone(),
        matter: room.token.as_str().to_string(),
        ids: vec![id],
        kind: Kind::Message,
        lane,
        title: room.name.clone(),
        body,
        from: author.map(|name| Sender { name, address: None }),
        url: link(server, &room.token),
        facets: facets.into_iter().flatten().collect(),
        date: room.last_activity,
    }
}

/// `<server>/call/<token>`, only when it lies on the server's origin (it always does: the token
/// has only `a`–`z` and `0`–`9`; checked again, as the rule is the core's).
fn link(server: &str, token: &RoomToken) -> Option<String> {
    let link = format!("{server}/call/{token}");
    torro_nextcloud::same_origin(&link, server).then_some(link)
}

/// Takes all ids off the open entries of `source` whose conversation is not among `unread` (read
/// elsewhere, no longer wanted, or gone from the list): each is then done.
fn resolve(sources: &Sources, source: &SourceRef, unread: &BTreeSet<&str>, now: DateTime<Utc>) {
    let open = match sources.inbox.open_remote(source) {
        Ok(open) => open,
        Err(err) => {
            eprintln!("torroworkplace: inbox: cannot read the open entries of {source}: {err}");
            return;
        }
    };
    let gone = gone(&open, unread);
    if !gone.is_empty()
        && let Err(err) = sources.inbox.resolve_remote(source, &gone, now)
    {
        eprintln!("torroworkplace: inbox: cannot settle the entries of {source}: {err}");
    }
}

/// The ids of the `open` entries whose conversation (the token before `/`) is not in `unread`.
fn gone(open: &[RemoteEntry], unread: &BTreeSet<&str>) -> Vec<String> {
    open.iter()
        .flat_map(|entry| &entry.ids)
        .filter(|id| id.split_once('/').is_none_or(|(token, _)| !unread.contains(token)))
        .cloned()
        .collect()
}

/// Marks the conversation of the done entry `item` read up to the newest message among `ids` and
/// deletes the matching chat notifications, in the background: spawn the calls on the current
/// runtime and return at once. Failures only to the log, with the account and a code.
pub(super) fn settle_done(sources: &Sources, item: &Item, ids: Vec<String>) {
    let Ok(runtime) = tokio::runtime::Handle::try_current() else { return };
    let (sources, item) = (sources.clone(), item.clone());
    runtime.spawn(async move {
        settle(&sources, &item, &ids).await;
    });
}

/// What [`settle`] reached on the server.
#[derive(Debug, Default, PartialEq, Eq)]
struct Settled {
    /// The message the conversation was marked read up to.
    read: Option<u64>,
    /// How many chat notifications were deleted.
    deleted: usize,
}

/// Marks the conversation of `item` (its matter is the token) read up to the newest message among
/// `ids`, then deletes Talk's chat notifications of that conversation (`spreed`, object `chat`)
/// not newer than the entry's last report. Sends nothing while the account's loop found the
/// sign-in refused, and stops at a refusal: asking again with a refused app password would only
/// feed Nextcloud's brute-force protection.
async fn settle(sources: &Sources, item: &Item, ids: &[String]) -> Settled {
    let source = &item.source;
    let mut settled = Settled::default();
    if is_signed_out(sources, source, Utc::now()) {
        eprintln!("{}", log_line(source, "auth"));
        return settled;
    }
    let Some(token) = RoomToken::parse(&item.thread_key) else { return settled };
    let account = SourceRef::nextcloud(source.name.as_str()).key();
    let Some(access) = sources.pim.nextcloud_access(&account).await else {
        eprintln!("{}", log_line(source, "not-connected"));
        return settled;
    };
    if let Some(last) = ids.iter().filter_map(|id| message_id(id)).max() {
        match access.client.mark_read(&token, last).await {
            Ok(()) => settled.read = Some(last),
            Err(error) => {
                eprintln!("{}", log_line(source, code(&error)));
                if refused(&error) {
                    return settled;
                }
            }
        }
    }
    let notifications = match access.client.notifications(None).await {
        Ok(Listing::List { notifications, .. }) => notifications,
        Ok(Listing::NotModified) => Vec::new(),
        Err(error) => {
            eprintln!("{}", log_line(source, code(&error)));
            return settled;
        }
    };
    let chat = notifications.iter().filter(|n| {
        let of_room = n.object_id == token.as_str() || n.object_id.starts_with(&format!("{token}/"));
        n.app == "spreed" && n.object_type == "chat" && of_room && n.time <= item.updated
    });
    for notification in chat {
        match access.client.delete_notification(notification.id).await {
            Ok(()) => settled.deleted += 1,
            Err(error) => {
                eprintln!("{}", log_line(source, code(&error)));
                if refused(&error) {
                    break;
                }
            }
        }
    }
    settled
}

fn refused(error: &OcsError) -> bool {
    matches!(error, OcsError::Refused | OcsError::Forbidden)
}

/// The log line for the code `code` of `source`: the account's id and a fixed word only.
fn log_line(source: &SourceRef, code: &str) -> String {
    format!("torroworkplace: inbox: Talk of {source}: {code}")
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use httpmock::Method::{DELETE, GET, POST};
    use httpmock::Mock;
    use serde_json::{Value, json};
    use torro_inbox::item::SourceHealth;
    use torro_inbox::{Link, State};
    use torro_ipc::api::{RightsTemplate, Selection};
    use torro_ipc::{ClientInfo, Request, Response};

    use super::super::nextcloud::Poller;
    use super::super::nextcloud::tests::{CAPABILITIES_PATH, Fixture, LIST_PATH, envelope, now};
    use super::*;

    const ROOMS_PATH: &str = "/ocs/v2.php/apps/spreed/api/v4/room";

    /// A conversation as Talk lists it: `token`, Talk's `type`, `level`, unread messages, mention,
    /// read marker and the last message `id` by Bert at `activity` (seconds).
    #[allow(clippy::too_many_arguments, reason = "a test row, every column named at the call")]
    fn room(
        token: &str,
        kind: i64,
        level: i64,
        unread: u64,
        mention: bool,
        read: u64,
        id: u64,
        activity: i64,
    ) -> Value {
        json!({
            "token": token,
            "type": kind,
            "name": "bert",
            "displayName": if kind == 1 { "Bert Example" } else { "Team Alpha" },
            "unreadMessages": unread,
            "unreadMention": mention,
            "unreadMentionDirect": mention,
            "notificationLevel": level,
            "lastActivity": activity,
            "lastReadMessage": read,
            "lastMessage": {
                "id": id,
                "actorType": "users",
                "actorId": "bert",
                "actorDisplayName": "Bert Example",
                "message": "Could you look at {file}?",
                "messageParameters": { "file": { "type": "file", "id": "1", "name": "Plan.pdf" } },
                "messageType": "comment"
            }
        })
    }

    /// One unread message from Bert to Anna, the last one `id`.
    fn direct(unread: u64, read: u64, id: u64) -> Value {
        room("a1b2c3d4", 1, 0, unread, false, read, id, 1_791_880_500 + id as i64)
    }

    impl Fixture {
        fn talk(&self) -> SourceRef {
            source_of(&self.source)
        }

        async fn with_talk(&self) -> Mock<'_> {
            let capabilities = json!({
                "notifications": { "ocs-endpoints": ["list", "get", "delete"] },
                "spreed": { "features": ["chat-v2", "chat-read-marker"] },
            });
            let body = envelope(json!({ "version": { "major": 31 }, "capabilities": capabilities }));
            self.server
                .mock_async(|when, then| {
                    when.method(GET).path(CAPABILITIES_PATH);
                    then.status(200).body(body.to_string());
                })
                .await
        }

        async fn rooms(&self, rooms: &[Value]) -> Mock<'_> {
            let body = envelope(Value::Array(rooms.to_vec())).to_string();
            self.server
                .mock_async(|when, then| {
                    when.method(GET).path(ROOMS_PATH).query_param("noStatusUpdate", "1");
                    then.status(200).body(body);
                })
                .await
        }

        async fn no_notifications(&self) -> Mock<'_> {
            self.listing(&[], "\"n\"").await
        }

        /// The open Talk entries.
        fn talk_open(&self) -> Vec<Item> {
            let open = self.sources.inbox.open_remote(&self.talk()).expect("open entries");
            open.iter().map(|entry| self.item(&entry.id)).collect()
        }

        fn talk_health(&self) -> Option<SourceHealth> {
            match self.sources.inbox.handle(Request::ListSources, None, now()) {
                Response::Sources { sources } => {
                    sources.into_iter().find(|info| info.source == self.talk()).and_then(|info| info.health)
                }
                other => panic!("expected sources, got {other:?}"),
            }
        }
    }

    #[test]
    fn every_notification_level_for_every_kind_of_conversation() {
        use NotificationLevel::{Always, Default as Standard, Mention, Never};
        use RoomKind::{Changelog, FormerOneToOne, Group, NoteToSelf, OneToOne, Public};
        let at = |kind, level, mention| Room {
            token: RoomToken::parse("abcd").expect("a token"),
            kind,
            name: "Team Alpha".into(),
            unread_messages: 1,
            unread_mention: mention,
            unread_mention_direct: false,
            notification_level: level,
            last_message: None,
            last_activity: now(),
            last_read: None,
        };
        let (me, info) = (Some(Lane::ForMe), Some(Lane::Info));
        for (kind, level, mention, lane) in [
            (OneToOne, Standard, false, me),
            (OneToOne, Always, false, me),
            (OneToOne, Mention, false, None),
            (OneToOne, Mention, true, me),
            (OneToOne, Never, true, None),
            (Group, Standard, false, None),
            (Group, Standard, true, me),
            (Group, Always, false, info),
            (Group, Always, true, me),
            (Group, Mention, false, None),
            (Group, Mention, true, me),
            (Group, Never, true, None),
            (Public, Standard, true, me),
            (Public, Always, false, info),
            (FormerOneToOne, Standard, false, None),
            (Changelog, Always, true, None),
            (NoteToSelf, Always, true, None),
        ] {
            assert_eq!(place(&at(kind, level, mention)), lane, "{kind:?} {level:?} mention {mention}");
        }
    }

    #[tokio::test]
    async fn an_unread_conversation_is_a_message_counted_by_its_unread_messages() {
        let fixture = Fixture::new().await;
        fixture.with_talk().await;
        fixture.no_notifications().await;
        fixture.rooms(&[direct(2, 5120, 5122)]).await;
        fixture.tick(&mut Poller::default(), now()).await;
        let open = fixture.talk_open();
        assert_eq!(open.len(), 1);
        let entry = &open[0];
        assert_eq!((entry.kind, entry.lane, entry.count), (Kind::Message, Lane::ForMe, 2));
        assert_eq!(entry.thread_key, "a1b2c3d4");
        assert_eq!(entry.title, "Bert Example");
        let text = trf("{name}: {text}", &[("name", &"Bert Example"), ("text", &"Could you look at Plan.pdf?")]);
        assert_eq!(entry.body.as_deref(), Some(text.as_str()));
        assert_eq!(entry.from.as_ref().map(|from| from.name.as_str()), Some("Bert Example"));
        let link = format!("{}/call/a1b2c3d4", fixture.server.base_url());
        assert_eq!(entry.link, Some(Link::Url { url: link }));
        assert!(entry.untrusted);
        let facets = vec![
            Facet { key: FacetKey::Kind, value: "direct".into() },
            Facet { key: FacetKey::Person, value: "Bert Example".into() },
        ];
        assert_eq!(entry.facets, facets);
        let ids = fixture.sources.inbox.remote_ids(&entry.id).expect("ids");
        assert_eq!(ids, ["a1b2c3d4/5122~1", "a1b2c3d4/5122"]);
        assert_eq!(fixture.talk_health(), None);
    }

    #[tokio::test]
    async fn a_new_message_counts_one_more_and_the_same_list_nothing() {
        let fixture = Fixture::new().await;
        fixture.with_talk().await;
        fixture.no_notifications().await;
        let first = fixture.rooms(&[direct(2, 5120, 5122)]).await;
        let mut poller = Poller::default();
        fixture.tick(&mut poller, now()).await;
        fixture.tick(&mut poller, now()).await;
        assert_eq!(fixture.talk_open()[0].count, 2, "listed twice");
        first.delete_async().await;
        fixture.rooms(&[direct(3, 5120, 5125)]).await;
        fixture.tick(&mut poller, now()).await;
        let entry = &fixture.talk_open()[0];
        assert_eq!(entry.count, 3);
        assert!(fixture.sources.inbox.remote_ids(&entry.id).expect("ids").contains(&"a1b2c3d4/5125".to_string()));
    }

    #[tokio::test]
    async fn read_elsewhere_the_entry_is_done() {
        let fixture = Fixture::new().await;
        fixture.with_talk().await;
        fixture.no_notifications().await;
        let unread = fixture.rooms(&[direct(1, 5120, 5122)]).await;
        let mut poller = Poller::default();
        fixture.tick(&mut poller, now()).await;
        let id = fixture.talk_open()[0].id.clone();
        unread.delete_async().await;
        let read = fixture.rooms(&[direct(0, 5122, 5122)]).await;
        fixture.tick(&mut poller, now()).await;
        assert!(fixture.talk_open().is_empty());
        assert!(matches!(fixture.item(&id).state, State::Done { .. }));
        read.delete_async().await;
        fixture.rooms(&[]).await;
        fixture.tick(&mut poller, now()).await;
        assert!(fixture.talk_open().is_empty(), "a conversation gone from the list has no entry");
    }

    #[tokio::test]
    async fn groups_follow_their_level_and_changelog_and_notes_never_come() {
        let fixture = Fixture::new().await;
        fixture.with_talk().await;
        fixture.no_notifications().await;
        fixture
            .rooms(&[
                room("grp0quiet", 2, 0, 4, false, 10, 20, 1_791_880_000),
                room("grp1ment", 2, 0, 4, true, 10, 30, 1_791_880_000),
                room("grp2alw", 2, 1, 2, false, 10, 40, 1_791_880_000),
                room("pub3never", 3, 3, 9, true, 10, 50, 1_791_880_000),
                room("chg4log", 4, 1, 1, false, 10, 60, 1_791_880_000),
                room("note5me", 6, 1, 1, false, 10, 70, 1_791_880_000),
            ])
            .await;
        fixture.tick(&mut Poller::default(), now()).await;
        let mut entries: Vec<(String, Lane, String)> = fixture
            .talk_open()
            .into_iter()
            .map(|item| {
                let kind = item.facets.iter().find(|f| f.key == FacetKey::Kind).map(|f| f.value.clone());
                (item.thread_key, item.lane, kind.unwrap_or_default())
            })
            .collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            entries,
            [("grp1ment".into(), Lane::ForMe, "mention".into()), ("grp2alw".into(), Lane::Info, "group".into())]
        );
    }

    #[tokio::test]
    async fn done_marks_read_up_to_the_last_known_message_and_deletes_its_chat_notifications() {
        let fixture = Fixture::new().await;
        fixture.with_talk().await;
        let base = 1_791_880_500 + 5122;
        let chat = |id: u64, object_id: &str, object_type: &str, at: i64| {
            json!({
                "notification_id": id, "app": "spreed", "user": "anna",
                "datetime": DateTime::from_timestamp(at, 0).expect("a time").to_rfc3339(),
                "object_type": object_type, "object_id": object_id,
                "subject": "Bert Example sent you a message", "message": "", "link": "", "actions": []
            })
        };
        let notifications = [
            chat(3, "a1b2c3d4", "chat", base),
            chat(4, "a1b2c3d4", "chat", base + 60),
            chat(5, "x7k2m9qa", "chat", base),
            chat(6, "a1b2c3d4", "call", base),
        ];
        fixture.listing(&notifications, "\"t\"").await;
        fixture.rooms(&[direct(2, 5120, 5122)]).await;
        fixture.tick(&mut Poller::default(), now()).await;
        assert!(fixture.open().iter().any(|item| item.thread_key == "spreed:call:a1b2c3d4"), "calls stay");
        assert!(!fixture.open().iter().any(|item| item.thread_key.starts_with("spreed:chat:")), "no doubles");
        let read = fixture
            .server
            .mock_async(|when, then| {
                when.method(POST)
                    .path("/ocs/v2.php/apps/spreed/api/v1/chat/a1b2c3d4/read")
                    .json_body(json!({ "lastReadMessage": 5122 }));
                then.status(200).body(envelope(json!({})).to_string());
            })
            .await;
        let deleted = fixture
            .server
            .mock_async(|when, then| {
                when.method(DELETE).path(format!("{LIST_PATH}/3"));
                then.status(200).body(envelope(json!([])).to_string());
            })
            .await;
        let entry = fixture.talk_open()[0].clone();
        let done = Request::SetItemDone { id: entry.id.clone(), done: true, dismissed: false };
        let Response::Item { item } = fixture.sources.inbox.handle(done, None, now()) else { panic!("done") };
        let ids = fixture.sources.inbox.remote_ids(&item.id).expect("ids");
        let settled = settle(&fixture.sources, &item, &ids).await;
        assert_eq!(settled, Settled { read: Some(5122), deleted: 1 });
        assert_eq!((read.calls_async().await, deleted.calls_async().await), (1, 1));
    }

    #[tokio::test]
    async fn settle_done_reaches_talk_in_the_background_and_a_newer_message_opens_the_entry_again() {
        let fixture = Fixture::new().await;
        fixture.with_talk().await;
        fixture.no_notifications().await;
        let unread = fixture.rooms(&[direct(1, 5120, 5122)]).await;
        let mut poller = Poller::default();
        fixture.tick(&mut poller, now()).await;
        let read = fixture
            .server
            .mock_async(|when, then| {
                when.method(POST).path("/ocs/v2.php/apps/spreed/api/v1/chat/a1b2c3d4/read");
                then.status(200).body(envelope(json!({})).to_string());
            })
            .await;
        let id = fixture.talk_open()[0].id.clone();
        let done = Request::SetItemDone { id: id.clone(), done: true, dismissed: false };
        let Response::Item { item } = fixture.sources.inbox.handle(done, None, now()) else { panic!("done") };
        super::super::settle_done(&fixture.sources, &item);
        for _ in 0..100 {
            if read.calls_async().await == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert_eq!(read.calls_async().await, 1);
        unread.delete_async().await;
        let quiet = fixture.rooms(&[direct(0, 5122, 5122)]).await;
        fixture.tick(&mut poller, now()).await;
        assert!(matches!(fixture.item(&id).state, State::Done { .. }));
        quiet.delete_async().await;
        fixture.rooms(&[direct(1, 5122, 5130)]).await;
        fixture.tick(&mut poller, now()).await;
        let again = fixture.item(&id);
        assert_eq!((again.state, again.count), (State::New, 2), "the newer message, counted once");
    }

    #[tokio::test]
    async fn done_without_reaching_the_server_is_not_opened_by_the_same_messages() {
        let fixture = Fixture::new().await;
        fixture.with_talk().await;
        fixture.no_notifications().await;
        fixture.rooms(&[direct(2, 5120, 5122)]).await;
        let mut poller = Poller::default();
        fixture.tick(&mut poller, now()).await;
        let id = fixture.talk_open()[0].id.clone();
        let done = Request::SetItemDone { id: id.clone(), done: true, dismissed: false };
        fixture.sources.inbox.handle(done, None, now());
        fixture.tick(&mut poller, now()).await;
        assert!(matches!(fixture.item(&id).state, State::Done { .. }));
    }

    #[tokio::test]
    async fn without_talk_the_chat_notifications_stay_and_no_rooms_are_asked() {
        let fixture = Fixture::new().await;
        fixture.capabilities(true).await;
        let chat = json!({
            "notification_id": 3, "app": "spreed", "user": "anna", "datetime": "2026-10-14T09:00:00Z",
            "object_type": "chat", "object_id": "a1b2c3d4", "subject": "Bert Example sent you a message",
            "message": "", "link": "", "actions": []
        });
        fixture.listing(&[chat], "\"u\"").await;
        let rooms = fixture.rooms(&[direct(1, 5120, 5122)]).await;
        fixture.tick(&mut Poller::default(), now()).await;
        assert_eq!(rooms.calls_async().await, 0);
        assert_eq!(fixture.open()[0].thread_key, "spreed:chat:a1b2c3d4");
        assert_eq!(fixture.talk_health(), None, "no Talk source without Talk");
    }

    #[tokio::test]
    async fn a_404_of_the_rooms_is_a_missing_app_of_talk_only() {
        let fixture = Fixture::new().await;
        fixture.with_talk().await;
        fixture.no_notifications().await;
        fixture
            .server
            .mock_async(|when, then| {
                when.method(GET).path(ROOMS_PATH);
                then.status(404).body(envelope(json!([])).to_string());
            })
            .await;
        fixture.tick(&mut Poller::default(), now()).await;
        assert_eq!(fixture.talk_health(), Some(SourceHealth::MissingApp));
        assert_eq!(fixture.health(), None, "the notifications work");
    }

    #[tokio::test]
    async fn a_muted_talk_asks_nothing_and_files_nothing_from_meanwhile() {
        let fixture = Fixture::new().await;
        fixture.with_talk().await;
        fixture.no_notifications().await;
        let rooms = fixture.rooms(&[direct(2, 5120, 5122)]).await;
        let mute = Request::MuteSource { source: fixture.talk(), muted: true };
        fixture.sources.inbox.handle(mute, None, now());
        let mut poller = Poller::default();
        fixture.tick(&mut poller, now()).await;
        assert_eq!(rooms.calls_async().await, 0);
        fixture.sources.inbox.handle(Request::MuteSource { source: fixture.talk(), muted: false }, None, now());
        fixture.tick(&mut poller, now()).await;
        assert!(fixture.talk_open().is_empty(), "came while muted");
        rooms.delete_async().await;
        fixture.rooms(&[direct(3, 5120, 5125)]).await;
        fixture.tick(&mut poller, now()).await;
        assert_eq!(fixture.talk_open()[0].count, 1, "only what came since");
    }

    #[tokio::test]
    async fn a_client_with_the_nextcloud_account_does_not_see_its_talk() {
        let fixture = Fixture::new().await;
        fixture.with_talk().await;
        fixture.no_notifications().await;
        fixture.rooms(&[direct(1, 5120, 5122)]).await;
        fixture.tick(&mut Poller::default(), now()).await;
        let id = fixture.talk_open()[0].id.clone();
        let with = |source: &SourceRef| {
            let mut rights = RightsTemplate::FullAccess.rights();
            rights.inbox_sources = Selection::Only(vec![source.key()]);
            ClientInfo {
                id: "dd04".into(),
                name: "codex dd04".into(),
                kind: "codex".into(),
                created: now(),
                last_used: None,
                rights,
                agent: None,
            }
        };
        let get = |client: &ClientInfo| {
            fixture.sources.inbox.handle(Request::GetItem { id: id.clone() }, Some(client), now())
        };
        assert!(matches!(get(&with(&fixture.source)), Response::Error { code, .. } if code == "not-found"));
        assert!(matches!(get(&with(&fixture.talk())), Response::Item { .. }));
    }

    #[test]
    fn ids_name_the_conversation_and_the_message_and_stop_at_the_batch() {
        let room = Room {
            token: RoomToken::parse("abcd").expect("a token"),
            kind: RoomKind::OneToOne,
            name: String::new(),
            unread_messages: 500,
            unread_mention: false,
            unread_mention_direct: false,
            notification_level: NotificationLevel::Default,
            last_message: None,
            last_activity: now(),
            last_read: None,
        };
        let stand_in = ids(&room, 2);
        assert_eq!(stand_in, [format!("abcd/@{}", now().timestamp()), format!("abcd/@{}~1", now().timestamp())]);
        assert_eq!(stand_in.iter().filter_map(|id| message_id(id)).count(), 0, "a stand-in names no message");
        assert_eq!(ids(&room, 500).len(), BATCH_MAX as usize);
        assert_eq!((message_id("abcd/5122"), message_id("abcd/5122~3")), (Some(5122), Some(5122)));
        let open = vec![
            RemoteEntry { id: "a".into(), ids: vec!["abcd/1".into(), "abcd/1~1".into()] },
            RemoteEntry { id: "b".into(), ids: vec!["efgh/2".into()] },
        ];
        assert_eq!(gone(&open, &BTreeSet::from(["efgh"])), ["abcd/1", "abcd/1~1"]);
    }

    #[test]
    fn marks_count_what_is_new() {
        let kept = Mark { read: 10, unread: 2 };
        assert_eq!(kept.new_in(Mark { read: 10, unread: 5 }), 3);
        assert_eq!(kept.new_in(Mark { read: 10, unread: 1 }), 0);
        assert_eq!(kept.new_in(Mark { read: 12, unread: 1 }), 1, "the read marker moved");
        assert_eq!(Mark::parse(&kept.text()), Some(kept));
        assert_eq!(Mark::parse("x"), None);
    }

    #[test]
    fn the_log_names_the_account_and_a_code_only() {
        let source = SourceRef::talk("anna@cloud.example.org");
        assert_eq!(log_line(&source, "auth"), "torroworkplace: inbox: Talk of talk:anna@cloud.example.org: auth");
    }

    #[tokio::test]
    async fn done_sends_nothing_to_talk_while_the_sign_in_is_refused() {
        let fixture = Fixture::new().await;
        fixture.with_talk().await;
        fixture.no_notifications().await;
        let unread = fixture.rooms(&[direct(1, 5120, 5122)]).await;
        let mut poller = Poller::default();
        fixture.tick(&mut poller, now()).await;
        let item = fixture.talk_open()[0].clone();
        unread.delete_async().await;
        fixture
            .server
            .mock_async(|when, then| {
                when.method(GET).path(ROOMS_PATH);
                then.status(401);
            })
            .await;
        let read = fixture
            .server
            .mock_async(|when, then| {
                when.method(POST).path("/ocs/v2.php/apps/spreed/api/v1/chat/a1b2c3d4/read");
                then.status(200).body(envelope(json!({})).to_string());
            })
            .await;
        fixture.tick(&mut poller, now()).await;
        let ids = fixture.sources.inbox.remote_ids(&item.id).expect("ids");
        assert_eq!(settle(&fixture.sources, &item, &ids).await, Settled::default());
        assert_eq!(read.calls_async().await, 0);
    }

    #[tokio::test]
    async fn a_refused_sign_in_marks_both_sources_signed_out() {
        let fixture = Fixture::new().await;
        fixture.with_talk().await;
        fixture.no_notifications().await;
        fixture
            .server
            .mock_async(|when, then| {
                when.method(GET).path(ROOMS_PATH);
                then.status(401);
            })
            .await;
        let mut poller = Poller::default();
        fixture.tick(&mut poller, now()).await;
        assert_eq!(
            (fixture.health(), fixture.talk_health()),
            (Some(SourceHealth::SignedOut), Some(SourceHealth::SignedOut))
        );
    }
}
