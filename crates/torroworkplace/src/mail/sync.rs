//! The sync of one mail account (task M2.6): lists the folders and their roles, then brings each
//! folder of the store in line with the server. [`MailService`](super::service::MailService) runs
//! it: [`sync_account`] on the command connection every five minutes and on `accounts.sync`,
//! [`sync_inbox`] on the IDLE connection whenever the server reports a change in the inbox.
//!
//! **One folder** ([`sync_folder`]):
//!
//! 1. `SELECT` it and tell the store its UIDVALIDITY; a new value drops what was cached (the UIDs
//!    mean other messages now) and the folder is synced from scratch.
//! 2. With CONDSTORE, an unchanged HIGHESTMODSEQ and the same number of messages, nothing changed:
//!    done. Otherwise `UID SEARCH ALL` and the flags (only those changed since the last
//!    HIGHESTMODSEQ with CONDSTORE, else all) are compared with the store: messages gone from the
//!    server (or marked `\Deleted`) are dropped, changed flags taken over.
//! 3. Headers of the new messages, **newest first, in batches** of [`HEADER_BATCH`]: a first sync
//!    of a large inbox shows the latest mail at once, and the command connection is handed back
//!    between batches, so commands run in between.
//! 4. Texts of the new messages of the last [`BODY_RETENTION_DAYS`] days, newest first (the core
//!    decides, task M2.5); bigger than [`BODY_PREFETCH_MAX`], or older, they are fetched when
//!    someone opens them (task M3.1). Fetched with `BODY.PEEK[]`: syncing never marks mail read.
//! 5. Only a folder synced to the end remembers its HIGHESTMODSEQ, so an interrupted sync starts
//!    over where it matters next time.
//!
//! Every step that found something sends a [`MailChange`].
//!
//! Everything read from the server is data from outside: it is parsed (`mime::read`), stored and
//! never acted on.

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Duration, Utc};
use sha2::{Digest, Sha256};
use torro_mail::{Flag, Role};
use torro_mail_imap::folders;
use torro_mail_imap::imap::{FetchedHeader, ImapError, Search, Session};
use torro_mail_imap::mime::{self, ReadMessage};

use super::service::{Command, MailChange, MailError, Mailbox, SharedStore};
use super::store::{BODY_RETENTION_DAYS, Body, NewMessage, UidValidity};

/// How many headers one `UID FETCH` asks for.
pub const HEADER_BATCH: usize = 100;

/// Texts of messages bigger than this (5 MiB, attachments included) are not fetched ahead.
pub const BODY_PREFETCH_MAX: u32 = 5 << 20;

/// A folder to sync: its id in the store, its path on the server and its role.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    id: String,
    path: String,
    role: Option<Role>,
}

/// Where a folder sync gets its connection from.
enum Conn<'a> {
    /// The IDLE connection's own session, held for the whole sync.
    Own(&'a mut Session),
    /// The account's command connection, taken for each step and handed back after it.
    Commands(&'a Mailbox),
}

/// A connection for one step of a sync ([`Conn::lease`]).
enum Lease<'a> {
    Own(&'a mut Session),
    // Boxed: a session with its TLS state is large, a reference small.
    Command(Box<Command>),
}

impl Conn<'_> {
    async fn lease(&mut self) -> Result<Lease<'_>, MailError> {
        match self {
            Conn::Own(session) => Ok(Lease::Own(session)),
            Conn::Commands(mailbox) => Ok(Lease::Command(Box::new(mailbox.command().await?))),
        }
    }
}

impl Lease<'_> {
    fn session(&mut self) -> &mut Session {
        match self {
            Lease::Own(session) => session,
            Lease::Command(command) => command.session(),
        }
    }

    /// The step worked: the command connection goes back for the next one. Dropping a lease
    /// instead closes the command connection (see [`Command`]).
    fn done(self) {
        if let Lease::Command(command) = self {
            command.release();
        }
    }
}

/// Syncs the whole account on its command connection: lists the folders (creating a missing
/// drafts or sent folder, task M2.3), keeps the store's folder list in line, then syncs every
/// folder, the inbox first, and drops texts older than [`BODY_RETENTION_DAYS`]. A folder that
/// vanished on the server meanwhile is skipped; any other failure ends the sync with that error.
pub(crate) async fn sync_account(mailbox: &Mailbox) -> Result<(), MailError> {
    let _one = mailbox.one_sync().await;
    let store = mailbox.store()?;
    let mut command = mailbox.command().await?;
    let list = folders::discover(command.session(), mailbox.folder_roles()).await?;
    command.release();
    // Only folders that hold messages; the others only hold folders.
    let listed: Vec<folders::RemoteFolder> = list.folders.into_iter().filter(|folder| folder.selectable).collect();
    let mut targets = store
        .run(move |store| {
            let mut targets = Vec::with_capacity(listed.len());
            for folder in listed {
                let id = store.upsert_folder(&folder.path, &folder.name, folder.role)?;
                targets.push(Target { id, path: folder.path, role: folder.role });
            }
            // An empty list would drop every folder; a server always has an inbox.
            if !targets.is_empty() {
                let paths: Vec<String> = targets.iter().map(|target| target.path.clone()).collect();
                store.retain_folders(&paths)?;
            }
            Ok(targets)
        })
        .await?;
    targets.sort_by_key(|target| target.role != Some(Role::Inbox));
    if let Some(inbox) = targets.iter().find(|target| target.role == Some(Role::Inbox)) {
        mailbox.set_inbox_path(&inbox.path);
    }
    let mut conn = Conn::Commands(mailbox);
    for target in &targets {
        match sync_folder(&mut conn, mailbox, &store, target, Utc::now()).await {
            Ok(()) | Err(MailError::Server(ImapError::NotFound(_))) => {}
            Err(err) => return Err(err),
        }
    }
    store.run(|store| store.prune_bodies(Utc::now()).map(drop)).await?;
    Ok(())
}

/// Syncs the inbox on `session`, the IDLE connection, which keeps the inbox open afterwards. Does
/// nothing before [`sync_account`] listed the folders once.
pub(crate) async fn sync_inbox(session: &mut Session, mailbox: &Mailbox) -> Result<(), MailError> {
    let _one = mailbox.one_sync().await;
    let store = mailbox.store()?;
    let path = mailbox.inbox_path();
    let lookup = path.clone();
    let Some(folder) = store.run(move |store| store.folder_by_path(&lookup)).await? else {
        return Ok(());
    };
    let target = Target { id: folder.id, path, role: Some(Role::Inbox) };
    sync_folder(&mut Conn::Own(session), mailbox, &store, &target, Utc::now()).await
}

/// What the store knew of a folder before this sync.
struct Known {
    validity: UidValidity,
    /// HIGHESTMODSEQ of the last complete sync under the same UIDVALIDITY.
    modseq: Option<u64>,
    /// UIDs and flags.
    messages: BTreeMap<u32, Vec<Flag>>,
    /// Ids dropped because the server renumbered the folder.
    dropped: Vec<String>,
}

/// Brings one folder of the store in line with the server; see the module documentation.
async fn sync_folder(
    conn: &mut Conn<'_>,
    mailbox: &Mailbox,
    store: &SharedStore,
    target: &Target,
    now: DateTime<Utc>,
) -> Result<(), MailError> {
    let mut lease = conn.lease().await?;
    let selected = lease.session().select(&target.path).await?;
    let condstore = lease.session().capabilities().has("CONDSTORE");
    let uidvalidity = selected.uid_validity;
    let known = know(store, &target.id, uidvalidity).await?;
    let unchanged = known.validity == UidValidity::Unchanged
        && condstore
        && selected.highest_modseq.is_some()
        && selected.highest_modseq == known.modseq
        && selected.exists as usize == known.messages.len();
    if unchanged {
        lease.done();
        return Ok(());
    }
    let on_server: BTreeSet<u32> = lease.session().uid_search(&Search::All).await?.into_iter().collect();
    let states = match known.messages.is_empty() {
        true => Vec::new(),
        false => lease.session().fetch_flags(known.modseq.filter(|_| condstore)).await?,
    };
    lease.done();

    // What went, and which flags changed.
    let deleted: BTreeSet<u32> = states.iter().filter(|state| state.deleted).map(|state| state.uid).collect();
    let gone: Vec<u32> =
        known.messages.keys().filter(|uid| !on_server.contains(uid) || deleted.contains(uid)).copied().collect();
    let changed: Vec<(u32, Vec<Flag>)> = states
        .iter()
        .filter(|state| !state.deleted && on_server.contains(&state.uid))
        .filter(|state| known.messages.get(&state.uid).is_some_and(|flags| !same_flags(flags, &state.flags)))
        .map(|state| (state.uid, state.flags.clone()))
        .collect();
    let folder = target.id.clone();
    let (mut removed, flags) = store
        .run(move |store| {
            let mut removed = Vec::new();
            for uid in &gone {
                removed.extend(store.message_id_for_uid(&folder, *uid)?);
            }
            store.delete_uids(&folder, &gone)?;
            let mut flagged = Vec::new();
            for (uid, flags) in &changed {
                if store.set_flags_by_uid(&folder, *uid, flags)? {
                    flagged.extend(store.message_id_for_uid(&folder, *uid)?);
                }
            }
            Ok((removed, flagged))
        })
        .await?;
    removed.splice(0..0, known.dropped);
    let initial = known.validity != UidValidity::Unchanged;
    let change = |new: Vec<String>, flags: Vec<String>, removed: Vec<String>| MailChange {
        account: mailbox.id().to_string(),
        folder: target.id.clone(),
        role: target.role,
        new,
        flags,
        removed,
        initial,
    };
    mailbox.announce(change(Vec::new(), flags, removed));

    // New messages: headers, newest first, batch by batch.
    let new: Vec<u32> = on_server.iter().rev().filter(|uid| !known.messages.contains_key(uid)).copied().collect();
    let recent_since = now - Duration::days(BODY_RETENTION_DAYS);
    let mut ahead: Vec<(String, FetchedHeader)> = Vec::new();
    for batch in new.chunks(HEADER_BATCH) {
        let mut lease = conn.lease().await?;
        if !reselect(lease.session(), &target.path, uidvalidity).await? {
            // Renumbered meanwhile: the next sync starts the folder over.
            lease.done();
            return Ok(());
        }
        let mut headers = lease.session().fetch_headers(batch).await?;
        lease.done();
        headers.retain(|header| !header.deleted);
        headers.sort_by_key(|header| std::cmp::Reverse(header.uid));
        let folder = target.id.clone();
        let stored = store
            .run(move |store| {
                let mut stored = Vec::with_capacity(headers.len());
                for header in headers {
                    let read = mime::read(&header.header).unwrap_or_else(|_| unreadable());
                    let message = new_message(&folder, uidvalidity, &header, &read, now);
                    let id = store.upsert_message(&folder, uidvalidity, &message)?;
                    stored.push((id, message.date, header));
                }
                Ok(stored)
            })
            .await?;
        let ids = stored.iter().map(|(id, _, _)| id.clone()).collect();
        ahead.extend(
            stored
                .into_iter()
                .filter(|(_, date, header)| *date >= recent_since && header.size <= BODY_PREFETCH_MAX)
                .map(|(id, _, header)| (id, header)),
        );
        mailbox.announce(change(ids, Vec::new(), Vec::new()));
    }

    // Texts of the recent ones, newest first.
    for (id, header) in ahead {
        let mut lease = conn.lease().await?;
        if !reselect(lease.session(), &target.path, uidvalidity).await? {
            lease.done();
            return Ok(());
        }
        let raw = lease.session().fetch_message(header.uid).await?;
        lease.done();
        let Some(read) = raw.and_then(|raw| mime::read(&raw).ok()) else { continue };
        let folder = target.id.clone();
        store
            .run(move |store| {
                // The flags as the store has them now: a command may have changed them meanwhile.
                let flags = store.summary(&id)?.map_or_else(|| header.flags.clone(), |summary| summary.flags);
                let mut message = new_message(&folder, uidvalidity, &header, &read, now);
                message.flags = flags;
                store.upsert_message(&folder, uidvalidity, &message)?;
                let body = Body { text: read.text, from_html: read.from_html, attachments: read.attachments };
                store.set_body(&id, &body)?;
                Ok(())
            })
            .await?;
    }

    let (folder, modseq) = (target.id.clone(), selected.highest_modseq);
    store.run(move |store| store.set_highest_modseq(&folder, modseq)).await?;
    Ok(())
}

/// Reads what the store knows of `folder` and tells it the UIDVALIDITY the server reports. After
/// a renumbering the ids of the dropped messages are kept, so the change can name them.
async fn know(store: &SharedStore, folder: &str, uidvalidity: u32) -> Result<Known, MailError> {
    let folder = folder.to_string();
    let known = store
        .run(move |store| {
            let before = store.resolve_folder(&folder)?.ok_or(super::MailStoreError::UnknownFolder)?;
            let renumbered = before.uidvalidity.is_some_and(|seen| seen != uidvalidity);
            let mut dropped = Vec::new();
            if renumbered {
                for (uid, _) in store.folder_uids(&folder)? {
                    dropped.extend(store.message_id_for_uid(&folder, uid)?);
                }
            }
            let validity = store.note_uidvalidity(&folder, uidvalidity)?;
            let modseq = before.highest_modseq.filter(|_| validity == UidValidity::Unchanged);
            let messages = store.folder_uids(&folder)?.into_iter().collect();
            Ok(Known { validity, modseq, messages, dropped })
        })
        .await?;
    Ok(known)
}

/// Opens `path` again when the session has another folder open (a command ran in between).
/// `false` when the folder's UIDVALIDITY is no longer `uidvalidity`.
async fn reselect(session: &mut Session, path: &str, uidvalidity: u32) -> Result<bool, MailError> {
    if let Some(selected) = session.selected()
        && selected.path == path
    {
        return Ok(selected.uid_validity == uidvalidity);
    }
    Ok(session.select(path).await?.uid_validity == uidvalidity)
}

fn same_flags(a: &[Flag], b: &[Flag]) -> bool {
    let set = |flags: &[Flag]| flags.iter().copied().collect::<BTreeSet<Flag>>();
    set(a) == set(b)
}

/// The headers of a message as the store keeps them. `read` may come from the header block only
/// (then the text and its snippet are empty) or from the whole message.
fn new_message(
    folder: &str,
    uidvalidity: u32,
    header: &FetchedHeader,
    read: &ReadMessage,
    now: DateTime<Utc>,
) -> NewMessage {
    NewMessage {
        id: None,
        uid: header.uid,
        rfc_message_id: read.message_id.clone(),
        thread_id: thread_id(read).unwrap_or_else(|| format!("t:{folder}:{uidvalidity}:{}", header.uid)),
        from: read.from.clone(),
        to: read.to.clone(),
        cc: read.cc.clone(),
        reply_to: read.reply_to.clone(),
        subject: read.subject.clone(),
        date: read.date.or(header.internal_date).unwrap_or(now),
        snippet: read.snippet(),
        flags: header.flags.clone(),
        has_attachments: header.has_attachments || read.has_attachments(),
        traits: read.traits.clone(),
        invitation: read.invitation,
    }
}

/// The conversation of a message: the first message it refers to (`References:`, else
/// `In-Reply-To:`), else itself, as a short hash. `None` without any `Message-ID`.
fn thread_id(read: &ReadMessage) -> Option<String> {
    let root = read.references.first().or(read.in_reply_to.as_ref()).or(read.message_id.as_ref())?;
    let hash: String = Sha256::digest(root.as_bytes()).iter().take(12).map(|byte| format!("{byte:02x}")).collect();
    Some(format!("t:{hash}"))
}

/// What is kept of a header block `mime::read` cannot read: nothing but the server's markers.
fn unreadable() -> ReadMessage {
    ReadMessage {
        message_id: None,
        in_reply_to: None,
        references: Vec::new(),
        from: None,
        to: Vec::new(),
        cc: Vec::new(),
        reply_to: Vec::new(),
        subject: String::new(),
        date: None,
        text: String::new(),
        from_html: false,
        attachments: Vec::new(),
        traits: torro_mail::MessageTraits::default(),
        invitation: false,
    }
}

/// A scripted IMAP server for the tests of the mail sync, after `torro-mail-imap`'s
/// `tests/imap_support`, but answering from a small world of folders and messages instead of a
/// fixed script, so a sync can run against it as against a real server: sign-in (`LOGIN`), `LIST`
/// with SPECIAL-USE, `SELECT` with CONDSTORE, `UID SEARCH`, `UID FETCH` (headers, flags with
/// `CHANGEDSINCE`, whole messages), `IDLE` with push, `NOOP`, `LOGOUT`. Tests change the world
/// ([`FakeImap::add`], [`FakeImap::set_flags`], …) and cut connections; idling clients hear of
/// every change.
///
/// TLS with the test certificate of `torro-mail-imap/tests/fixtures/imap-tls/` (for `127.0.0.1`);
/// the client trusts only its CA ([`fake::trust`]). Loopback only, no network.
#[cfg(test)]
pub(crate) mod fake {
    use std::collections::BTreeSet;
    use std::sync::{Arc, Mutex};

    use chrono::{DateTime, Duration, Utc};
    use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
    use tokio::io::{AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;
    use tokio::sync::watch;
    use tokio_rustls::TlsAcceptor;
    use torro_mail::{Endpoint, Security};
    use torro_mail_imap::imap::Trust;

    const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../torro-mail-imap/tests/fixtures/imap-tls/");
    const CAPABILITIES: &str = "IMAP4rev1 IDLE CONDSTORE SPECIAL-USE NAMESPACE";

    /// The client side's trust: only the test CA.
    pub(crate) fn trust() -> Trust {
        Trust::Only(vec![CertificateDer::from(fixture("ca.der"))])
    }

    fn fixture(name: &str) -> Vec<u8> {
        std::fs::read(format!("{FIXTURES}{name}")).expect("test certificate of torro-mail-imap")
    }

    /// A message `days_ago` days old with `subject`, `Message-ID` `<subject>@example.org` (spaces
    /// as dashes) and, if given, `In-Reply-To`.
    pub(crate) fn mail(subject: &str, days_ago: i64, in_reply_to: Option<&str>) -> String {
        let date = (Utc::now() - Duration::days(days_ago)).to_rfc2822();
        let id = subject.replace(' ', "-");
        let reply = in_reply_to.map(|to| format!("In-Reply-To: <{to}>\r\n")).unwrap_or_default();
        format!(
            "From: Bernd <bernd@example.com>\r\nTo: anna@example.org\r\nSubject: {subject}\r\nDate: {date}\r\n\
             Message-ID: <{id}@example.org>\r\n{reply}Content-Type: text/plain; charset=utf-8\r\n\r\nText of {subject}\r\n"
        )
    }

    struct Message {
        uid: u32,
        flags: BTreeSet<String>,
        modseq: u64,
        raw: Vec<u8>,
        received: DateTime<Utc>,
    }

    struct Folder {
        path: String,
        attributes: String,
        uidvalidity: u32,
        next_uid: u32,
        modseq: u64,
        messages: Vec<Message>,
    }

    struct World {
        password: String,
        folders: Vec<Folder>,
        modseq: u64,
        /// Every line the clients sent, the password blanked.
        lines: Vec<String>,
        logins: usize,
    }

    impl World {
        fn folder(&mut self, path: &str) -> &mut Folder {
            let at = self.folders.iter().position(|folder| folder.path == path).expect("a folder of the fake");
            &mut self.folders[at]
        }

        fn bump(&mut self) -> u64 {
            self.modseq += 1;
            self.modseq
        }
    }

    /// A running fake server; clones share it.
    #[derive(Clone)]
    pub(crate) struct FakeImap {
        pub(crate) endpoint: Endpoint,
        world: Arc<Mutex<World>>,
        /// Counts changes of the world; idling clients wait on it.
        changes: watch::Sender<u64>,
        /// Counts cuts; every open connection ends on one.
        cuts: watch::Sender<u64>,
    }

    impl FakeImap {
        /// A server on `127.0.0.1` that takes `password` for any login, with the folders INBOX,
        /// Drafts, Sent and Archive (marked with their SPECIAL-USE attributes), all empty.
        pub(crate) async fn start(password: &str) -> FakeImap {
            let listener = TcpListener::bind("127.0.0.1:0").await.expect("a free port");
            let port = listener.local_addr().expect("address").port();
            let folder = |path: &str, attributes: &str, uidvalidity: u32| Folder {
                path: path.into(),
                attributes: attributes.into(),
                uidvalidity,
                next_uid: 1,
                modseq: 1,
                messages: Vec::new(),
            };
            let world = World {
                password: password.into(),
                folders: vec![
                    folder("INBOX", "\\HasNoChildren", 11),
                    folder("Drafts", "\\HasNoChildren \\Drafts", 12),
                    folder("Sent", "\\HasNoChildren \\Sent", 13),
                    folder("Archive", "\\HasNoChildren \\Archive", 14),
                ],
                modseq: 1,
                lines: Vec::new(),
                logins: 0,
            };
            let server = FakeImap {
                endpoint: Endpoint { host: "127.0.0.1".into(), port, security: Security::Tls },
                world: Arc::new(Mutex::new(world)),
                changes: watch::Sender::new(0),
                cuts: watch::Sender::new(0),
            };
            let accepting = server.clone();
            tokio::spawn(async move {
                let acceptor = acceptor();
                while let Ok((tcp, _)) = listener.accept().await {
                    let (acceptor, server) = (acceptor.clone(), accepting.clone());
                    tokio::spawn(async move {
                        if let Ok(tls) = acceptor.accept(tcp).await {
                            server.serve(tls).await;
                        }
                    });
                }
            });
            server
        }

        fn world(&self) -> std::sync::MutexGuard<'_, World> {
            self.world.lock().expect("the fake's world")
        }

        fn changed(&self) {
            self.changes.send_modify(|count| *count += 1);
        }

        /// Delivers `raw` into `folder` with `flags`; returns its UID.
        pub(crate) fn add(&self, folder: &str, raw: &str, flags: &[&str]) -> u32 {
            let uid = {
                let mut world = self.world();
                let modseq = world.bump();
                let folder = world.folder(folder);
                let uid = folder.next_uid;
                folder.next_uid += 1;
                folder.modseq = modseq;
                folder.messages.push(Message {
                    uid,
                    flags: flags.iter().map(|flag| flag.to_string()).collect(),
                    modseq,
                    raw: raw.as_bytes().to_vec(),
                    received: Utc::now(),
                });
                uid
            };
            self.changed();
            uid
        }

        /// Sets the flags of message `uid` in `folder`, as another mail program would.
        pub(crate) fn set_flags(&self, folder: &str, uid: u32, flags: &[&str]) {
            {
                let mut world = self.world();
                let modseq = world.bump();
                let folder = world.folder(folder);
                folder.modseq = modseq;
                let message = folder.messages.iter_mut().find(|m| m.uid == uid).expect("a message of the fake");
                message.flags = flags.iter().map(|flag| flag.to_string()).collect();
                message.modseq = modseq;
            }
            self.changed();
        }

        /// Removes message `uid` from `folder` for good.
        pub(crate) fn expunge(&self, folder: &str, uid: u32) {
            {
                let mut world = self.world();
                let modseq = world.bump();
                let folder = world.folder(folder);
                folder.modseq = modseq;
                folder.messages.retain(|message| message.uid != uid);
            }
            self.changed();
        }

        /// Gives `folder` a new UIDVALIDITY and its messages new UIDs, as a server that rebuilt it.
        pub(crate) fn renumber(&self, folder: &str) {
            {
                let mut world = self.world();
                let modseq = world.bump();
                let folder = world.folder(folder);
                folder.uidvalidity += 100;
                folder.modseq = modseq;
                for message in &mut folder.messages {
                    message.uid += 1000;
                }
                folder.next_uid += 1000;
            }
            self.changed();
        }

        /// Ends every open connection, as a network that went away.
        pub(crate) fn drop_connections(&self) {
            self.cuts.send_modify(|count| *count += 1);
        }

        /// Every line clients sent so far, the password blanked.
        pub(crate) fn lines(&self) -> Vec<String> {
            self.world().lines.clone()
        }

        /// How many sign-ins were tried.
        pub(crate) fn logins(&self) -> usize {
            self.world().logins
        }

        async fn serve<S>(&self, stream: S)
        where
            S: tokio::io::AsyncRead + AsyncWrite + Unpin,
        {
            let (reader, mut writer) = tokio::io::split(stream);
            let mut reader = BufReader::new(reader);
            let mut changes = self.changes.subscribe();
            let mut cuts = self.cuts.subscribe();
            cuts.borrow_and_update();
            let mut selected: Option<String> = None;
            // The changes this connection has seen with its last SELECT; newer ones are pushed at
            // the start of an IDLE, as a real server reports what happened since.
            let mut seen = 0;
            if write(&mut writer, "* OK [CAPABILITY IMAP4rev1] Fake IMAP ready").await.is_err() {
                return;
            }
            let mut line = String::new();
            loop {
                line.clear();
                let read = tokio::select! {
                    read = reader.read_line(&mut line) => read,
                    _ = cuts.changed() => return,
                };
                if !matches!(read, Ok(n) if n > 0) {
                    return;
                }
                let Some(text) = with_literals(&mut reader, &mut writer, &line).await else { return };
                let (tag, rest) = text.split_once(' ').unwrap_or((text.as_str(), ""));
                let (tag, rest) = (tag.to_string(), rest.to_string());
                let command = rest.split(' ').next().unwrap_or_default().to_ascii_uppercase();
                self.world().lines.push(if command == "LOGIN" { format!("{tag} LOGIN …") } else { text.clone() });
                let answer = match command.as_str() {
                    "LOGIN" => self.login(&tag, &rest),
                    "CAPABILITY" => format!("* CAPABILITY {CAPABILITIES}\r\n{tag} OK done"),
                    "NOOP" => format!("{tag} OK done"),
                    "LOGOUT" => {
                        let _ = write(&mut writer, &format!("* BYE bye\r\n{tag} OK done")).await;
                        return;
                    }
                    "NAMESPACE" => format!("* NAMESPACE ((\"\" \"/\")) NIL NIL\r\n{tag} OK done"),
                    "LIST" => self.list(&tag),
                    "SELECT" => {
                        let path = quoted(&rest["SELECT ".len()..]);
                        seen = *changes.borrow_and_update();
                        let answer = self.select(&tag, &path);
                        selected = Some(path);
                        answer
                    }
                    "UID" => self.uid(&tag, &rest["UID ".len()..], selected.as_deref()),
                    "IDLE" => {
                        if write(&mut writer, "+ idling").await.is_err() {
                            return;
                        }
                        if *changes.borrow_and_update() != seen && self.push(&mut writer, selected.as_deref()).await {
                            return;
                        }
                        loop {
                            line.clear();
                            tokio::select! {
                                read = reader.read_line(&mut line) => {
                                    if !matches!(read, Ok(n) if n > 0) {
                                        return;
                                    }
                                    self.world().lines.push(line.trim_end().to_string());
                                    break;
                                }
                                _ = changes.changed() => {
                                    if self.push(&mut writer, selected.as_deref()).await {
                                        return;
                                    }
                                }
                                _ = cuts.changed() => return,
                            }
                        }
                        format!("{tag} OK Idle completed")
                    }
                    _ => format!("{tag} BAD unknown command"),
                };
                if write(&mut writer, &answer).await.is_err() {
                    return;
                }
            }
        }

        /// Tells an idling client the size of its folder; `true` when the connection broke.
        async fn push<W: AsyncWrite + Unpin>(&self, writer: &mut W, selected: Option<&str>) -> bool {
            let exists = selected.map_or(0, |path| self.world().folder(path).messages.len());
            write(writer, &format!("* {exists} EXISTS")).await.is_err()
        }

        fn login(&self, tag: &str, rest: &str) -> String {
            let mut world = self.world();
            world.logins += 1;
            let arguments = &rest["LOGIN ".len()..];
            let user = quoted(arguments);
            let after = &arguments[user.len() + 2..].trim_start();
            if !user.is_empty() && quoted(after) == world.password {
                format!("{tag} OK [CAPABILITY {CAPABILITIES}] Logged in")
            } else {
                format!("{tag} NO [AUTHENTICATIONFAILED] Authentication failed.")
            }
        }

        fn list(&self, tag: &str) -> String {
            let world = self.world();
            let mut lines: Vec<String> = world
                .folders
                .iter()
                .map(|folder| format!("* LIST ({}) \"/\" \"{}\"", folder.attributes, folder.path))
                .collect();
            lines.push(format!("{tag} OK List completed"));
            lines.join("\r\n")
        }

        fn select(&self, tag: &str, path: &str) -> String {
            let mut world = self.world();
            let folder = world.folder(path);
            format!(
                "* {} EXISTS\r\n* OK [UIDVALIDITY {}] UIDs valid\r\n* OK [UIDNEXT {}] Next\r\n\
                 * OK [HIGHESTMODSEQ {}] Highest\r\n{tag} OK [READ-WRITE] Select completed",
                folder.messages.len(),
                folder.uidvalidity,
                folder.next_uid,
                folder.modseq
            )
        }

        fn uid(&self, tag: &str, rest: &str, selected: Option<&str>) -> String {
            let Some(path) = selected else { return format!("{tag} BAD no folder selected") };
            let mut world = self.world();
            let folder = world.folder(path);
            let mut lines = Vec::new();
            if let Some(criteria) = rest.strip_prefix("SEARCH ") {
                let uids: Vec<String> = folder
                    .messages
                    .iter()
                    .filter(|message| matches_search(criteria, message))
                    .map(|message| message.uid.to_string())
                    .collect();
                lines.push(format!("* SEARCH {}", uids.join(" ")).trim_end().to_string());
            } else if let Some(fetch) = rest.strip_prefix("FETCH ") {
                let (set, items) = fetch.split_once(' ').unwrap_or((fetch, ""));
                let since = items
                    .split_once("CHANGEDSINCE ")
                    .and_then(|(_, after)| after.trim_end_matches(')').parse::<u64>().ok());
                for (at, message) in folder.messages.iter().enumerate() {
                    if !in_set(set, message.uid) || since.is_some_and(|since| message.modseq <= since) {
                        continue;
                    }
                    lines.push(fetched(at + 1, message, items));
                }
            } else {
                return format!("{tag} BAD unknown UID command");
            }
            lines.push(format!("{tag} OK done"));
            lines.join("\r\n")
        }
    }

    /// One `FETCH` response for `message` (sequence number `seq`) with what `items` asks for.
    fn fetched(seq: usize, message: &Message, items: &str) -> String {
        let flags = message.flags.iter().cloned().collect::<Vec<_>>().join(" ");
        let size = message.raw.len();
        if items.contains("BODY.PEEK[HEADER]") {
            let end = message.raw.windows(4).position(|window| window == b"\r\n\r\n").map_or(size, |at| at + 4);
            let header = String::from_utf8_lossy(&message.raw[..end]);
            let date = message.received.format("%d-%b-%Y %H:%M:%S +0000");
            format!(
                "* {seq} FETCH (UID {} FLAGS ({flags}) INTERNALDATE \"{date}\" RFC822.SIZE {size} BODYSTRUCTURE \
                 (\"text\" \"plain\" (\"charset\" \"utf-8\") NIL NIL \"8bit\" {size} 1 NIL NIL NIL) MODSEQ ({}) \
                 BODY[HEADER] {{{}}}\r\n{header})",
                message.uid, message.modseq, end
            )
        } else if items.contains("BODY.PEEK[]") {
            let raw = String::from_utf8_lossy(&message.raw);
            format!("* {seq} FETCH (UID {} BODY[] {{{size}}}\r\n{raw})", message.uid)
        } else {
            format!("* {seq} FETCH (UID {} FLAGS ({flags}) MODSEQ ({}))", message.uid, message.modseq)
        }
    }

    /// `uid` is in the sequence set `set` (`1:3,5,7:*`).
    fn in_set(set: &str, uid: u32) -> bool {
        set.split(',').any(|part| match part.split_once(':') {
            Some((from, "*")) => from.parse::<u32>().is_ok_and(|from| uid >= from),
            Some((from, to)) => {
                matches!((from.parse::<u32>(), to.parse::<u32>()), (Ok(from), Ok(to)) if (from..=to).contains(&uid))
            }
            None => part.parse::<u32>().is_ok_and(|one| one == uid),
        })
    }

    /// The first quoted string of `text`, without quotes and escapes; the text up to the first
    /// space when it is not quoted.
    fn quoted(text: &str) -> String {
        let Some(inner) = text.strip_prefix('"') else {
            return text.split(' ').next().unwrap_or_default().to_string();
        };
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            match c {
                '\\' => out.extend(chars.next()),
                '"' => break,
                c => out.push(c),
            }
        }
        out
    }

    // ----- search criteria and literals (task M3.1) -----

    /// The command line `line` without its line end, with every literal (`{n}` at the end of a
    /// line, as a client sends a word that is not plain ASCII) asked for with `+`, read and put in
    /// as a quoted string. `None` when the connection broke.
    async fn with_literals<R, W>(reader: &mut R, writer: &mut W, line: &str) -> Option<String>
    where
        R: tokio::io::AsyncBufRead + Unpin,
        W: AsyncWrite + Unpin,
    {
        use tokio::io::AsyncReadExt;
        let mut text = line.trim_end_matches(['\r', '\n']).to_string();
        while let Some(open) = text.strip_suffix('}').and_then(|inner| inner.rfind('{')) {
            let size: usize = text[open + 1..text.len() - 1].parse().ok()?;
            text.truncate(open);
            write(writer, "+ go on").await.ok()?;
            let mut literal = vec![0u8; size];
            reader.read_exact(&mut literal).await.ok()?;
            let literal = String::from_utf8_lossy(&literal).replace('\\', "\\\\").replace('"', "\\\"");
            let mut rest = String::new();
            reader.read_line(&mut rest).await.ok()?;
            text = format!("{text}\"{literal}\"{}", rest.trim_end_matches(['\r', '\n']));
        }
        Some(text)
    }

    /// `message` matches the `UID SEARCH` criteria: `ALL`, `UID <set>`, `CHARSET <name>` (taken
    /// as UTF-8), `TEXT <string>` (anywhere in the raw message, any case), `SINCE`/`BEFORE <day>`.
    /// The days compare with the `Date:` header, since the fake receives every message now.
    fn matches_search(criteria: &str, message: &Message) -> bool {
        let raw = String::from_utf8_lossy(&message.raw);
        let date = date_of(&raw).unwrap_or(message.received).date_naive();
        let lower = raw.to_lowercase();
        let day =
            |text: Option<String>| text.and_then(|text| chrono::NaiveDate::parse_from_str(&text, "%d-%b-%Y").ok());
        let mut words = search_words(criteria).into_iter();
        while let Some(word) = words.next() {
            let holds = match word.to_ascii_uppercase().as_str() {
                "ALL" => true,
                "CHARSET" => words.next().is_some(),
                "UID" => words.next().is_some_and(|set| in_set(&set, message.uid)),
                "TEXT" => words.next().is_some_and(|text| lower.contains(&text.to_lowercase())),
                "SINCE" => day(words.next()).is_some_and(|since| date >= since),
                "BEFORE" => day(words.next()).is_some_and(|before| date < before),
                _ => false,
            };
            if !holds {
                return false;
            }
        }
        true
    }

    /// The criteria split into atoms and quoted strings (without quotes and escapes).
    fn search_words(criteria: &str) -> Vec<String> {
        let mut words = Vec::new();
        let mut chars = criteria.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                ' ' => {}
                '"' => {
                    let mut word = String::new();
                    while let Some(c) = chars.next() {
                        match c {
                            '\\' => word.extend(chars.next()),
                            '"' => break,
                            c => word.push(c),
                        }
                    }
                    words.push(word);
                }
                c => {
                    let mut word = c.to_string();
                    while let Some(c) = chars.next_if(|c| *c != ' ') {
                        word.push(c);
                    }
                    words.push(word);
                }
            }
        }
        words
    }

    /// The `Date:` header of a raw message.
    fn date_of(raw: &str) -> Option<DateTime<Utc>> {
        let date = raw.lines().take_while(|line| !line.is_empty()).find_map(|line| line.strip_prefix("Date:"))?;
        DateTime::parse_from_rfc2822(date.trim()).ok().map(|date| date.with_timezone(&Utc))
    }

    async fn write<W: AsyncWrite + Unpin>(writer: &mut W, text: &str) -> std::io::Result<()> {
        writer.write_all(format!("{text}\r\n").as_bytes()).await?;
        writer.flush().await
    }

    fn acceptor() -> TlsAcceptor {
        let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
        let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(fixture("server.key.der")));
        let config = rustls::ServerConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .expect("protocol versions")
            .with_no_client_auth()
            .with_single_cert(vec![CertificateDer::from(fixture("server.der"))], key)
            .expect("server certificate");
        TlsAcceptor::from(Arc::new(config))
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use torro_mail::MessageSummary;

    use super::fake::{self, FakeImap};
    use super::*;
    use crate::mail::service::tests::{PASSWORD, mail_account, mailbox_in};
    use crate::mail::store::SearchQuery;

    async fn mailbox(dir: &Path, server: &FakeImap) -> Mailbox {
        mailbox_in(dir, mail_account("anna@example.org", &server.endpoint), PASSWORD).await
    }

    async fn all(mailbox: &Mailbox) -> Vec<MessageSummary> {
        let store = mailbox.store().expect("store");
        store.run(|store| store.search(&SearchQuery::default(), 1000, 0)).await.expect("searched")
    }

    async fn text_of(mailbox: &Mailbox, id: &str) -> Option<String> {
        let (store, id) = (mailbox.store().expect("store"), id.to_string());
        store.run(move |store| Ok(store.message(&id)?.and_then(|message| message.text))).await.expect("read")
    }

    fn fetches(server: &FakeImap, what: &str) -> Vec<String> {
        server.lines().into_iter().filter(|line| line.contains("UID FETCH") && line.contains(what)).collect()
    }

    #[tokio::test]
    async fn the_first_sync_takes_the_newest_headers_first_in_batches_and_the_recent_texts() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        for n in 1..=130 {
            // UID 1 is the oldest: 129 days, UID 130 today.
            server.add("INBOX", &fake::mail(&format!("Mail {n}"), 130 - n, None), &["\\Seen"]);
        }
        server.add("Archive", &fake::mail("Kept", 3, None), &[]);
        let mailbox = mailbox(dir.path(), &server).await;
        let mut changes = mailbox_events(&mailbox);

        sync_account(&mailbox).await.expect("synced");

        let headers = fetches(&server, "BODY.PEEK[HEADER]");
        assert!(headers[0].contains(" 31:130 "), "the newest hundred first: {headers:?}");
        assert!(headers[1].contains(" 1:30 "), "then the rest: {headers:?}");
        let first = changes.try_recv().expect("the first batch is announced");
        assert!(first.initial && first.new.len() == HEADER_BATCH && first.role == Some(Role::Inbox), "{first:?}");
        let messages = all(&mailbox).await;
        assert_eq!(messages.len(), 131);
        let newest = messages.iter().find(|m| m.subject == "Mail 130").expect("synced");
        assert_eq!((newest.flags.as_slice(), newest.snippet.as_str()), ([Flag::Seen].as_slice(), "Text of Mail 130"));
        assert_eq!(text_of(&mailbox, &newest.id).await.as_deref(), Some("Text of Mail 130\n"));
        let old = messages.iter().find(|m| m.subject == "Mail 1").expect("synced");
        assert_eq!(text_of(&mailbox, &old.id).await, None, "older than 90 days: only the headers");
        let bodies = fetches(&server, "BODY.PEEK[]");
        // Mail 41 to 130 (Mail 40 is a moment more than 90 days old) and the archive's one.
        assert_eq!(bodies.len(), 90 + 1, "texts of the last 90 days only");
        let stores: Vec<String> = server.lines().into_iter().filter(|line| line.contains(" UID STORE ")).collect();
        assert!(stores.is_empty(), "syncing never marks mail read: {stores:?}");
    }

    #[tokio::test]
    async fn a_second_sync_takes_over_removals_and_flags_and_skips_unchanged_folders() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let first = server.add("INBOX", &fake::mail("First", 2, None), &[]);
        let second = server.add("INBOX", &fake::mail("Second", 1, Some("First@example.org")), &[]);
        let mailbox = mailbox(dir.path(), &server).await;
        sync_account(&mailbox).await.expect("synced");
        let before = all(&mailbox).await;
        assert_eq!(before[0].thread_id, before[1].thread_id, "a reply is in the thread of what it answers");
        let lines = server.lines().len();

        sync_account(&mailbox).await.expect("synced again");
        let again: Vec<String> = server.lines()[lines..].to_vec();
        assert!(!again.iter().any(|line| line.contains("UID ")), "nothing changed, nothing fetched: {again:?}");

        let mut changes = mailbox_events(&mailbox);
        server.expunge("INBOX", first);
        server.set_flags("INBOX", second, &["\\Flagged"]);
        sync_account(&mailbox).await.expect("synced after the changes");
        let change = changes.try_recv().expect("announced");
        let first_id = before.iter().find(|m| m.subject == "First").map(|m| m.id.clone());
        let second_id = before.iter().find(|m| m.subject == "Second").map(|m| m.id.clone());
        assert_eq!((change.removed, change.flags), (Vec::from_iter(first_id), Vec::from_iter(second_id)));
        let after = all(&mailbox).await;
        assert_eq!((after.len(), after[0].flags.as_slice()), (1, [Flag::Flagged].as_slice()));
        assert!(
            server.lines().iter().any(|line| line.contains("(CHANGEDSINCE ")),
            "only changed flags are asked for with CONDSTORE"
        );
    }

    #[tokio::test]
    async fn a_renumbered_folder_is_synced_from_scratch() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &fake::mail("Only", 1, None), &[]);
        let mailbox = mailbox(dir.path(), &server).await;
        sync_account(&mailbox).await.expect("synced");
        let before = all(&mailbox).await;

        let mut changes = mailbox_events(&mailbox);
        server.renumber("INBOX");
        sync_account(&mailbox).await.expect("synced after renumbering");
        let removed = changes.try_recv().expect("the old ids are gone");
        assert_eq!((removed.removed, removed.initial), (vec![before[0].id.clone()], true));
        let added = changes.try_recv().expect("and come back under new ones");
        assert!(added.initial && added.new.len() == 1);
        let after = all(&mailbox).await;
        assert_eq!((after.len(), after[0].subject.as_str()), (1, "Only"));
        assert_ne!(after[0].id, before[0].id);
    }

    #[tokio::test]
    async fn the_inbox_sync_of_the_idle_connection_waits_for_the_folder_list() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &fake::mail("Waiting", 0, None), &[]);
        let mailbox = mailbox(dir.path(), &server).await;
        let mut session = mailbox.connect().await.expect("connected");
        sync_inbox(&mut session, &mailbox).await.expect("nothing to do yet");
        assert!(all(&mailbox).await.is_empty());
        sync_account(&mailbox).await.expect("synced");
        server.add("INBOX", &fake::mail("Next", 0, None), &[]);
        sync_inbox(&mut session, &mailbox).await.expect("the inbox synced");
        assert_eq!(all(&mailbox).await.len(), 2);
        assert_eq!(session.selected().map(|selected| selected.path.as_str()), Some("INBOX"), "ready to IDLE");
    }

    #[test]
    fn threads_follow_the_first_reference_and_messages_without_ids_stand_alone() {
        let mut read = unreadable();
        assert_eq!(thread_id(&read), None);
        read.message_id = Some("b@x".into());
        let alone = thread_id(&read);
        read.in_reply_to = Some("a@x".into());
        let reply = thread_id(&read);
        read.references = vec!["root@x".into(), "a@x".into()];
        let deeper = thread_id(&read);
        assert_ne!(alone, reply);
        assert_ne!(reply, deeper);
        let mut root = unreadable();
        root.message_id = Some("root@x".into());
        assert_eq!(thread_id(&root), deeper);
    }

    fn mailbox_events(mailbox: &Mailbox) -> tokio::sync::broadcast::Receiver<MailChange> {
        mailbox.subscribe()
    }
}
