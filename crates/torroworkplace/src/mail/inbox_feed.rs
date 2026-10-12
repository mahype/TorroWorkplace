//! Mail as a source of the inbox (task E4.1): new mail in the inbox folder of each mail account
//! becomes an inbox entry, and what happens to the mail elsewhere comes back to the entry.
//!
//! - **New mail** ([`MailChange::new`] of an inbox folder): every unread message is filed
//!   ([`Inbox::file_mail`]), one entry per conversation (a second mail counts on the same entry),
//!   in the lane the rules give: mail sent by machines or to lists ([`is_bulk`]) to "Info", other
//!   mail "for me", a lane the person kept for the sender before either. The account's setting
//!   `notify` decides what is filed at all (all, only "for me", nothing). A folder synced for the
//!   first time, or anew after the server renumbered it, brings only the mail of the last
//!   [`FIRST_SYNC_DAYS`] days, so connecting an account floods nothing.
//! - **Read or moved elsewhere** ([`MailChange::flags`], [`MailChange::removed`]): an open entry of
//!   which no message is still unread in the inbox folder is done by itself.
//! - **Done in the inbox** ([`mark_read`]): after `inbox.set_item_done` or
//!   `inbox.create_task_from_item` marked a mail entry done, the core hands it here and its mail is
//!   marked read on the server (`\Seen`), never moved or archived (decision 6 in
//!   `docs/eingang.md`). The inbox checked before that the caller may write mail.
//! - **Missed changes** (the feed fell behind the service, `RecvError::Lagged`): every account is
//!   looked at anew ([`Feed::catch_up`]): unread mail of the last [`FIRST_SYNC_DAYS`] days is filed
//!   (a message filed before is never filed twice) and every open entry is checked.
//! - **Accounts gone** (removed, or mail switched off): every [`RETIRE_EVERY`] the open entries of
//!   accounts the service no longer syncs are done ([`Feed::retire`]); nothing could mark their
//!   mail read any more. Kept here rather than where accounts change, so the core's account
//!   commands need not know the inbox.
//!
//! Rights are not checked here: the feed files for the person, and the inbox shows a client a
//! mail entry only as its rights allow (`crate::inbox`). Mail is data from outside: entries are
//! `untrusted`, and the log names the account and an error code, never a subject, sender or text.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, TimeDelta, Utc};
use tokio::sync::broadcast::{Receiver, error::RecvError};
use torro_inbox::{Item, Sender, SourceKind, State};
use torro_mail::{Flag, MailAddress, MessageTraits, Notify, Role};

use super::MailStoreError;
use super::service::{MailChange, MailError, MailService, SharedStore};
use super::store::{MailStore, SearchQuery};
use crate::inbox::{Inbox, NewMail};
use crate::pim::Reach;

/// The first sync of a folder brings only mail of this many days into the inbox.
pub const FIRST_SYNC_DAYS: i64 = 3;

/// The most unread messages per account a catch-up looks at, newest first.
const CATCH_UP_MAX: u32 = 500;

/// How often the entries of mail accounts that were removed or switched off are done.
pub const RETIRE_EVERY: Duration = crate::inbox::SUITE_EVERY;

/// Mail sent by a machine or to a list (`docs/eingang.md`, "Sortieren"): it has `List-Id` or
/// `List-Unsubscribe`, `Precedence` `bulk`, `list` or `junk`, or `Auto-Submitted` other than `no`.
/// Its entry goes to "Info", unless the person keeps the sender elsewhere.
pub fn is_bulk(traits: &MessageTraits) -> bool {
    let precedence = traits.precedence.as_deref().is_some_and(|value| matches!(value, "bulk" | "list" | "junk"));
    let automatic = traits.auto_submitted.as_deref().is_some_and(|value| value != "no");
    traits.list_id.is_some() || traits.list_unsubscribe || precedence || automatic
}

/// Starts the feed of `mail` into `inbox`: listens to the mail service from now on, so call it
/// before the service syncs ([`MailService::start`]), and works through its changes in a task of
/// its own. `accounts_file` is `accounts.json`, where each account's `notify` is read when its mail
/// comes; `None` (no home directory) files nothing. The core calls it once, on its runtime.
pub fn start(inbox: Arc<Inbox>, mail: Arc<MailService>, accounts_file: Option<PathBuf>) {
    let changes = mail.subscribe();
    tokio::spawn(Feed { inbox, mail, accounts_file }.run(changes));
}

/// Marks the mail of `item` read on its server, in the background, when `item` is a mail entry
/// the inbox just marked done (task E4.1); any other entry is left alone. The core calls it with
/// the entry `inbox.set_item_done` or `inbox.create_task_from_item` answered with, after the inbox
/// checked the caller's write access to mail.
///
/// The entry's messages are read at once, so mail that arrives in the conversation meanwhile
/// stays unread. Messages read already or gone are left as they are; failures go to the log (the
/// entry stays done, the mail unread).
pub fn mark_read(inbox: &Inbox, mail: &Arc<MailService>, item: &Item) {
    if item.source.kind != SourceKind::Mail || !matches!(item.state, State::Done { .. }) {
        return;
    }
    let account = item.source.key();
    let messages = match inbox.mail_messages(&item.id) {
        Ok(messages) if !messages.is_empty() => messages,
        Ok(_) => return,
        Err(err) => {
            eprintln!("torroworkplace: inbox: cannot read the mail of a done entry of {account}: {err}");
            return;
        }
    };
    let Ok(runtime) = tokio::runtime::Handle::try_current() else { return };
    let mail = Arc::clone(mail);
    runtime.spawn(async move {
        if let Err(err) = set_seen(&mail, &account, messages).await {
            eprintln!("torroworkplace: inbox: cannot mark the mail of a done entry of {account} read: {}", err.code());
        }
    });
}

/// Sets `\Seen` on the server for those of `messages` (`m:…` of `account`) that are unread, and
/// then in the store, so the next sync finds nothing changed. Returns how many were marked. A
/// folder the server renumbered since is skipped: its UIDs name other mail now.
pub(crate) async fn set_seen(mail: &MailService, account: &str, messages: Vec<String>) -> Result<usize, MailError> {
    let store = mail.store(account).await?;
    let targets = store
        .run(move |store| {
            let mut targets = Vec::new();
            for id in &messages {
                let unread = store.summary(id)?.is_some_and(|summary| !summary.flags.contains(&Flag::Seen));
                if unread && let Some(found) = store.resolve_message(id)? {
                    targets.push(found);
                }
            }
            Ok(targets)
        })
        .await?;
    let mut folders: BTreeMap<(String, u32), Vec<(String, u32)>> = BTreeMap::new();
    for target in targets {
        folders.entry((target.folder_path, target.uidvalidity)).or_default().push((target.id, target.uid));
    }
    if folders.is_empty() {
        return Ok(0);
    }
    let mut marked: Vec<String> = Vec::new();
    let mut command = mail.command(account).await?;
    for ((path, uidvalidity), found) in folders {
        if command.session().select(&path).await?.uid_validity != uidvalidity {
            continue;
        }
        let uids: Vec<u32> = found.iter().map(|(_, uid)| *uid).collect();
        command.session().store_flags(&uids, &[Flag::Seen], &[]).await?;
        marked.extend(found.into_iter().map(|(id, _)| id));
    }
    // Every call worked: the connection may serve the next command.
    command.release();
    let count = marked.len();
    store
        .run(move |store| {
            for id in &marked {
                if let Some(summary) = store.summary(id)?
                    && !summary.flags.contains(&Flag::Seen)
                {
                    let mut flags = summary.flags;
                    flags.push(Flag::Seen);
                    store.set_flags(id, &flags)?;
                }
            }
            Ok(())
        })
        .await?;
    Ok(count)
}

/// The feed: the inbox it files into, the mail service it listens to, and where the accounts'
/// settings are.
struct Feed {
    inbox: Arc<Inbox>,
    mail: Arc<MailService>,
    accounts_file: Option<PathBuf>,
}

impl Feed {
    /// Works through `changes` until the service goes away, and retires the entries of accounts
    /// gone every [`RETIRE_EVERY`].
    async fn run(self, mut changes: Receiver<MailChange>) {
        let mut retire = tokio::time::interval(RETIRE_EVERY);
        retire.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                received = changes.recv() => match received {
                    Ok(change) => self.take(change, Utc::now()).await,
                    Err(RecvError::Lagged(_)) => self.catch_up(Utc::now()).await,
                    Err(RecvError::Closed) => return,
                },
                _ = retire.tick() => self.retire(Utc::now()).await,
            }
        }
    }

    /// Marks done at `now` the open entries of mail accounts the service no longer syncs: removed
    /// from `accounts.json`, or with mail switched off ([`Inbox::retire_mail`]). Nothing before the
    /// service read its accounts, when every account would look gone; an account whose store failed
    /// keeps its entries, its mail is still there.
    async fn retire(&self, now: DateTime<Utc>) {
        if self.mail.account_checks(now).await.is_none() {
            return;
        }
        let mut active = Vec::new();
        for account in self.mail.infos(Reach::Suite).await.into_iter().map(|info| info.id) {
            if !matches!(self.mail.store(&account).await, Err(MailError::NotFound)) {
                active.push(account);
            }
        }
        if let Err(err) = self.inbox.retire_mail(&active, now) {
            eprintln!("torroworkplace: inbox: cannot retire the entries of mail accounts gone: {err}");
        }
    }

    /// One change of one folder at `now`: new mail of an inbox folder is filed, messages whose
    /// flags changed or that went away may settle their entries.
    async fn take(&self, change: MailChange, now: DateTime<Utc>) {
        let MailChange { account, folder, role, new, flags, removed, initial } = change;
        let store = match self.mail.store(&account).await {
            Ok(store) => store,
            Err(err) => {
                eprintln!("torroworkplace: inbox: cannot read the mail of {account}: {}", err.code());
                return;
            }
        };
        if role == Some(Role::Inbox) && !new.is_empty() {
            let since = initial.then(|| now - TimeDelta::days(FIRST_SYNC_DAYS));
            self.file(&store, &account, folder, new, since, now).await;
        }
        let touched: Vec<String> = flags.into_iter().chain(removed).collect();
        if !touched.is_empty() {
            self.settle(&store, &account, Some(&touched), now).await;
        }
    }

    /// Looks at every mail account anew at `now`, after changes were missed: files the unread mail
    /// of its inbox folder of the last [`FIRST_SYNC_DAYS`] days (none twice) and settles every open
    /// entry.
    async fn catch_up(&self, now: DateTime<Utc>) {
        for account in self.mail.infos(Reach::Suite).await.into_iter().map(|info| info.id) {
            // Mail switched off: nothing is synced, nothing to file.
            let Ok(store) = self.mail.store(&account).await else { continue };
            let since = now - TimeDelta::days(FIRST_SYNC_DAYS);
            let unread = store
                .run(move |store| {
                    let Some(inbox) = store.folder_by_role(Role::Inbox)? else { return Ok(None) };
                    let query = SearchQuery {
                        folder: Some(inbox.id.clone()),
                        unread: Some(true),
                        since: Some(since),
                        ..SearchQuery::default()
                    };
                    let ids = store.search(&query, CATCH_UP_MAX, 0)?.into_iter().map(|summary| summary.id).collect();
                    Ok(Some((inbox.id, ids)))
                })
                .await;
            match unread {
                Ok(Some((folder, ids))) => self.file(&store, &account, folder, ids, Some(since), now).await,
                Ok(None) => {}
                Err(err) => eprintln!("torroworkplace: inbox: cannot read the mail of {account}: {err}"),
            }
            self.settle(&store, &account, None, now).await;
        }
    }

    /// Files those of `ids` (messages of `folder`, the inbox folder of `account`) that are still
    /// unread there and, with `since`, not older, as the account's `notify` allows.
    async fn file(
        &self,
        store: &SharedStore,
        account: &str,
        folder: String,
        ids: Vec<String>,
        since: Option<DateTime<Utc>>,
        now: DateTime<Utc>,
    ) {
        let notify = self.notify_of(account).await;
        if notify == Notify::Off {
            return;
        }
        let owner = account.to_string();
        let mails = match store.run(move |store| unread_mail(store, &owner, &folder, &ids, since)).await {
            Ok(mails) => mails,
            Err(err) => {
                eprintln!("torroworkplace: inbox: cannot read the new mail of {account}: {err}");
                return;
            }
        };
        for mail in &mails {
            if let Err(err) = self.inbox.file_mail(mail, notify, now) {
                eprintln!("torroworkplace: inbox: cannot file the new mail of {account}: {err}");
                return;
            }
        }
    }

    /// Marks done the open entries of `account` (with `touching`, only those holding one of these
    /// messages) none of whose messages is still unread in the account's inbox folder: the mail was
    /// read, moved or deleted elsewhere.
    async fn settle(&self, store: &SharedStore, account: &str, touching: Option<&[String]>, now: DateTime<Utc>) {
        let entries = match self.inbox.open_mail_entries(account, touching) {
            Ok(entries) if !entries.is_empty() => entries,
            Ok(_) => return,
            Err(err) => {
                eprintln!("torroworkplace: inbox: cannot read the mail entries of {account}: {err}");
                return;
            }
        };
        let settled = store
            .run(move |store| {
                // Without an inbox folder nothing can be told; the entries stay.
                let Some(inbox) = store.folder_by_role(Role::Inbox)? else { return Ok(Vec::new()) };
                let mut settled = Vec::new();
                for entry in entries {
                    let mut waiting = false;
                    for message in &entry.messages {
                        waiting |= store
                            .summary(message)?
                            .is_some_and(|summary| summary.folder == inbox.id && !summary.flags.contains(&Flag::Seen));
                    }
                    if !waiting {
                        settled.push(entry.id);
                    }
                }
                Ok(settled)
            })
            .await;
        let result = match settled {
            Ok(settled) if settled.is_empty() => Ok(()),
            Ok(settled) => self.inbox.resolve_mail(account, &settled, now),
            Err(err) => Err(err.to_string()),
        };
        if let Err(err) = result {
            eprintln!("torroworkplace: inbox: cannot settle the mail entries of {account}: {err}");
        }
    }

    /// What `account` wants filed (`MailSettings::notify`), read from `accounts.json` now, so a
    /// change in the control centre holds from the next mail on. An account that is gone, or a
    /// file that cannot be read, files nothing.
    async fn notify_of(&self, account: &str) -> Notify {
        let Some(file) = self.accounts_file.clone() else { return Notify::Off };
        let read = tokio::task::spawn_blocking(move || torro_core::account::load_from(&file)).await;
        match read {
            Ok(Ok(accounts)) => accounts
                .into_iter()
                .find(|stored| stored.id == account)
                .map_or(Notify::Off, |stored| stored.mail_settings.map(|settings| settings.notify).unwrap_or_default()),
            Ok(Err(err)) => {
                eprintln!("torroworkplace: inbox: cannot read the mail accounts' settings: {err}");
                Notify::Off
            }
            Err(err) => {
                eprintln!("torroworkplace: inbox: cannot read the mail accounts' settings: {err}");
                Notify::Off
            }
        }
    }
}

/// Those of `ids` that are unread messages of `folder` (and not older than `since`), as the inbox
/// files them, oldest first: the newest mail of a conversation comes last and its entry shows it.
fn unread_mail(
    store: &MailStore,
    account: &str,
    folder: &str,
    ids: &[String],
    since: Option<DateTime<Utc>>,
) -> Result<Vec<NewMail>, MailStoreError> {
    let mut mails = Vec::new();
    for id in ids {
        let Some(message) = store.message(id)? else { continue };
        let summary = message.summary;
        let old = since.is_some_and(|since| summary.date < since);
        if summary.folder != folder || summary.flags.contains(&Flag::Seen) || old {
            continue;
        }
        mails.push(NewMail {
            account: account.to_string(),
            message: summary.id,
            thread: summary.thread_id,
            subject: summary.subject,
            snippet: summary.snippet,
            from: summary.from.as_ref().map(sender),
            bulk: is_bulk(&message.traits),
            date: summary.date,
        });
    }
    mails.sort_by_key(|mail| mail.date);
    Ok(mails)
}

/// The sender of a mail as an inbox entry names it: the display name, else the address.
fn sender(from: &MailAddress) -> Sender {
    let name = from.name.as_deref().map(str::trim).filter(|name| !name.is_empty()).unwrap_or(&from.address);
    Sender { name: name.to_string(), address: Some(from.address.clone()) }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use torro_inbox::{Kind, Lane, Link};
    use torro_ipc::{Request, Response};
    use torro_mail::MailSettings;
    use torro_pim::AccountState;

    use super::*;
    use crate::account::github::tests::{MemoryTokens, accounts_file};
    use crate::inbox::tests::inbox_in;
    use crate::mail::service::tests::{PASSWORD, eventually, mail_account, service_in, store_accounts};
    use crate::mail::sync::fake::FakeImap;

    const ANNA: &str = "mail:anna@example.org";

    /// A mail `days_ago` days old from `from` (`Name <address>`) to Anna, with `Message-ID`
    /// `<subject>@example.org` (spaces as dashes), `headers` (each ending in CRLF) and, if given,
    /// `In-Reply-To`.
    fn letter(subject: &str, from: &str, days_ago: i64, headers: &str, in_reply_to: Option<&str>) -> String {
        let date = (Utc::now() - TimeDelta::days(days_ago)).to_rfc2822();
        let id = subject.replace(' ', "-");
        let reply = in_reply_to.map(|to| format!("In-Reply-To: <{to}>\r\n")).unwrap_or_default();
        format!(
            "From: {from}\r\nTo: anna@example.org\r\nSubject: {subject}\r\nDate: {date}\r\n\
             Message-ID: <{id}@example.org>\r\n{reply}{headers}Content-Type: text/plain; charset=utf-8\r\n\r\n\
             Text of {subject}\r\n"
        )
    }

    fn bernd(subject: &str, days_ago: i64) -> String {
        letter(subject, "Bernd <bernd@example.com>", days_ago, "", None)
    }

    /// Anna's account on `server`, stored in `dir` with its password and `notify`.
    fn anna(dir: &Path, server: &FakeImap, notify: Notify) -> MemoryTokens {
        let secrets = MemoryTokens::default();
        let mut account = mail_account("anna@example.org", &server.endpoint);
        account.mail_settings = Some(MailSettings { notify, ..MailSettings::default() });
        store_accounts(dir, &secrets, &[(&account, PASSWORD)]);
        secrets
    }

    /// The inbox and the mail service in `dir`, with the feed between them listening before the
    /// first sync starts.
    async fn fed(dir: &Path, secrets: &MemoryTokens) -> (Arc<Inbox>, Arc<MailService>) {
        let inbox = Arc::new(inbox_in(dir));
        let mail = Arc::new(service_in(dir, secrets));
        start(Arc::clone(&inbox), Arc::clone(&mail), Some(accounts_file(dir)));
        mail.reload().await;
        (inbox, mail)
    }

    /// Every entry, as the app key lists them.
    fn entries(inbox: &Inbox) -> Vec<Item> {
        let all = Request::ListItems { lane: None, source: None, state: None, since: None, limit: None };
        match inbox.handle(all, None, Utc::now()) {
            Response::Items { items } => items,
            other => panic!("expected items, got {other:?}"),
        }
    }

    fn entry<'a>(items: &'a [Item], title: &str) -> &'a Item {
        items.iter().find(|item| item.title == title).unwrap_or_else(|| panic!("no entry {title:?} in {items:?}"))
    }

    async fn synced(mail: &MailService) -> bool {
        let infos = mail.infos(Reach::Suite).await;
        !infos.is_empty()
            && infos.iter().all(|info| matches!(&info.state, AccountState::Connected { last_sync: Some(_), .. }))
    }

    fn idling(server: &FakeImap) -> bool {
        server.lines().iter().any(|line| line.ends_with(" IDLE"))
    }

    #[test]
    fn list_and_machine_mail_is_bulk() {
        assert!(!is_bulk(&MessageTraits::default()));
        let a_person = MessageTraits { auto_submitted: Some("no".into()), ..MessageTraits::default() };
        assert!(!is_bulk(&a_person));
        for bulk in [
            MessageTraits { list_id: Some("<news.shop.example>".into()), ..MessageTraits::default() },
            MessageTraits { list_unsubscribe: true, ..MessageTraits::default() },
            MessageTraits { precedence: Some("bulk".into()), ..MessageTraits::default() },
            MessageTraits { precedence: Some("list".into()), ..MessageTraits::default() },
            MessageTraits { auto_submitted: Some("auto-generated".into()), ..MessageTraits::default() },
        ] {
            assert!(is_bulk(&bulk), "{bulk:?}");
        }
    }

    #[tokio::test]
    async fn new_mail_becomes_entries_in_their_lanes_and_a_reply_counts() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &bernd("Offer", 1), &[]);
        let news = letter("Weekly news", "Shop <news@shop.example>", 1, "List-Id: <news.shop.example>\r\n", None);
        server.add("INBOX", &news, &[]);
        server.add("INBOX", &bernd("Old offer", 10), &[]);
        server.add("INBOX", &bernd("Read already", 0), &["\\Seen"]);
        server.add("Archive", &bernd("Archived", 0), &[]);
        let secrets = anna(dir.path(), &server, Notify::All);
        let (inbox, mail) = fed(dir.path(), &secrets).await;

        eventually("the first sync filed", || async { entries(&inbox).len() == 2 }).await;
        eventually("synced", || synced(&mail)).await;
        let items = entries(&inbox);
        assert_eq!(items.len(), 2, "not the old offer (three days), not what was read, not the archive");
        let offer = entry(&items, "Offer");
        assert_eq!((offer.kind, offer.lane, offer.urgent, offer.untrusted), (Kind::Message, Lane::ForMe, false, true));
        assert_eq!(offer.source.key(), ANNA);
        assert_eq!(offer.from.as_ref().map(|from| from.name.as_str()), Some("Bernd"));
        assert!(matches!(&offer.link, Some(Link::Mail { message }) if message.starts_with("m:")), "{offer:?}");
        assert_eq!(entry(&items, "Weekly news").lane, Lane::Info, "list mail is for reading later");

        eventually("the inbox watched", || async { idling(&server) }).await;
        let reply = letter("Re Offer", "Bernd <bernd@example.com>", 0, "", Some("Offer@example.org"));
        server.add("INBOX", &reply, &[]);
        eventually("the reply counted", || async {
            entries(&inbox).iter().any(|item| item.title == "Re Offer" && item.count == 2)
        })
        .await;
        assert_eq!(entries(&inbox).len(), 2, "one entry per conversation");
    }

    #[tokio::test]
    async fn only_mail_for_me_when_the_account_says_so() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let news = letter("Weekly news", "Shop <news@shop.example>", 0, "Precedence: bulk\r\n", None);
        server.add("INBOX", &news, &[]);
        server.add("INBOX", &bernd("Offer", 0), &[]);
        let secrets = anna(dir.path(), &server, Notify::ForMeOnly);
        let (inbox, mail) = fed(dir.path(), &secrets).await;

        eventually("the offer filed", || async { entries(&inbox).len() == 1 }).await;
        eventually("synced", || synced(&mail)).await;
        assert_eq!(entries(&inbox).iter().map(|item| item.title.as_str()).collect::<Vec<_>>(), ["Offer"]);
    }

    #[tokio::test]
    async fn mail_read_or_moved_elsewhere_settles_its_entry() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let read = server.add("INBOX", &bernd("Read elsewhere", 0), &[]);
        let moved = server.add("INBOX", &bernd("Moved elsewhere", 0), &[]);
        server.add("INBOX", &bernd("Still waiting", 0), &[]);
        let secrets = anna(dir.path(), &server, Notify::All);
        let (inbox, _mail) = fed(dir.path(), &secrets).await;
        eventually("filed", || async { entries(&inbox).len() == 3 }).await;
        eventually("the inbox watched", || async { idling(&server) }).await;

        server.set_flags("INBOX", read, &["\\Seen"]);
        eventually("read elsewhere: done", || async {
            matches!(entry(&entries(&inbox), "Read elsewhere").state, State::Done { .. })
        })
        .await;
        server.expunge("INBOX", moved);
        eventually("moved elsewhere: done", || async {
            matches!(entry(&entries(&inbox), "Moved elsewhere").state, State::Done { .. })
        })
        .await;
        assert_eq!(entry(&entries(&inbox), "Still waiting").state, State::New);
    }

    #[tokio::test]
    async fn done_in_the_inbox_marks_the_mail_read_on_the_server_and_stays_done() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let secrets = anna(dir.path(), &server, Notify::All);
        let service = service_in(dir.path(), &secrets);
        service.reload().await;
        eventually("synced", || synced(&service)).await;
        eventually("the inbox watched", || async { idling(&server) }).await;
        let settings = crate::settings_service::SettingsStore::at(dir.path().join("config").join("settings.json"));
        let core = crate::core::Core::new(
            torro_wm::Backend::Unsupported { name: "test".into() },
            torro_setups::storage::Store::at(dir.path().to_path_buf()),
            Arc::new(crate::pim::PimService::with_settings(settings)),
            crate::clients::tests::open_in(dir.path()),
        )
        .with_approvals(crate::approvals::tests::quiet_in(dir.path()))
        .with_inbox(inbox_in(dir.path()))
        .with_mail(service);
        core.start_mail_feed(Some(accounts_file(dir.path())));
        let listed = || async {
            let all = Request::ListItems { lane: None, source: None, state: None, since: None, limit: None };
            match core.handle(all).await {
                Response::Items { items } => items,
                other => panic!("expected items, got {other:?}"),
            }
        };

        let uid = server.add("INBOX", &bernd("Offer", 0), &[]);
        let other = server.add("INBOX", &bernd("Another", 0), &[]);
        eventually("filed", || async { listed().await.len() == 2 }).await;
        let id = entry(&listed().await, "Offer").id.clone();
        let done = core.handle(Request::SetItemDone { id, done: true }).await;
        assert!(matches!(&done, Response::Item { item } if matches!(item.state, State::Done { .. })), "{done:?}");

        eventually("read on the server", || async { server.flags("INBOX", uid).contains(&"\\Seen".to_string()) }).await;
        assert!(server.flags("INBOX", other).is_empty(), "only the mail of the entry");
        let stores: Vec<String> = server.lines().into_iter().filter(|line| line.contains("UID STORE")).collect();
        assert_eq!(stores.len(), 1, "marked read once, nothing moved: {stores:?}");
        assert!(stores[0].ends_with("+FLAGS.SILENT (\\Seen)"), "{stores:?}");
        // The server's change comes back through the sync and leaves the entries as they are.
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let items = listed().await;
        assert!(matches!(entry(&items, "Offer").state, State::Done { .. }));
        assert_eq!(entry(&items, "Another").state, State::New);
    }

    #[tokio::test]
    async fn entries_of_an_account_switched_off_or_removed_are_done() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &bernd("Offer", 0), &[]);
        let secrets = anna(dir.path(), &server, Notify::All);
        let (inbox, mail) = fed(dir.path(), &secrets).await;
        eventually("filed", || async { entries(&inbox).len() == 1 }).await;
        let file = Some(accounts_file(dir.path()));
        let feed = Feed { inbox: Arc::clone(&inbox), mail: Arc::clone(&mail), accounts_file: file };
        feed.retire(Utc::now()).await;
        assert_eq!(entries(&inbox)[0].state, State::New, "the account syncs: its entry stays");

        let mut account = mail_account("anna@example.org", &server.endpoint);
        account.mail_settings = Some(MailSettings {
            services: torro_mail::Services { mail: false, calendars: true, tasks: false },
            ..MailSettings::default()
        });
        store_accounts(dir.path(), &secrets, &[(&account, PASSWORD)]);
        mail.reload().await;
        feed.retire(Utc::now()).await;
        assert!(matches!(entries(&inbox)[0].state, State::Done { .. }), "mail switched off");

        // Removed altogether: an entry opened again by hand is done at the next look.
        let id = entries(&inbox)[0].id.clone();
        inbox.handle(Request::SetItemDone { id, done: false }, None, Utc::now());
        store_accounts(dir.path(), &secrets, &[]);
        mail.reload().await;
        feed.retire(Utc::now()).await;
        assert!(matches!(entries(&inbox)[0].state, State::Done { .. }), "account removed");
    }

    #[tokio::test]
    async fn a_feed_that_fell_behind_catches_up_from_the_store() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let uid = server.add("INBOX", &bernd("Offer", 1), &[]);
        server.add("INBOX", &bernd("Old offer", 10), &[]);
        let secrets = anna(dir.path(), &server, Notify::All);
        let inbox = Arc::new(inbox_in(dir.path()));
        let mail = Arc::new(service_in(dir.path(), &secrets));
        // Synced while nobody listened: the feed missed it all.
        mail.reload().await;
        eventually("synced", || synced(&mail)).await;
        let file = Some(accounts_file(dir.path()));
        let feed = Feed { inbox: Arc::clone(&inbox), mail: Arc::clone(&mail), accounts_file: file };

        feed.catch_up(Utc::now()).await;
        let titles: Vec<String> = entries(&inbox).into_iter().map(|item| item.title).collect();
        assert_eq!(titles, ["Offer"], "the last three days only");
        feed.catch_up(Utc::now()).await;
        assert_eq!(entries(&inbox)[0].count, 1, "nothing filed twice");

        server.set_flags("INBOX", uid, &["\\Seen"]);
        mail.sync_all(Reach::Suite).await.expect("synced");
        feed.catch_up(Utc::now()).await;
        assert!(matches!(entries(&inbox)[0].state, State::Done { .. }), "read meanwhile");
    }
}
