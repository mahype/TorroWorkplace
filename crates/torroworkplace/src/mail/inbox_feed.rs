//! Mail as a source of the inbox (task E4.1): new mail in the inbox folder of each mail account
//! becomes an inbox entry, and what happens to the mail elsewhere comes back to the entry.
//!
//! - **New mail** ([`MailChange::new`] of an inbox folder): every unread message is filed
//!   ([`Inbox::file_mail`]), one entry per conversation (a second mail counts on the same entry),
//!   in the lane the rules give: mail sent by machines or to lists ([`bulk_reason`]) to "Info", other
//!   mail "for me", a lane the person kept for the sender before either; the entry keeps why
//!   (`Item::reason`, issue #128). The account's setting
//!   `notify` decides what is filed at all (all, only "for me", nothing). A folder synced for the
//!   first time, or anew after the server renumbered it, brings only the mail of the last
//!   [`FIRST_SYNC_DAYS`] days, so connecting an account floods nothing.
//! - **Read or moved elsewhere** ([`MailChange::flags`], [`MailChange::removed`]): an open entry of
//!   which no message is still unread in the inbox folder is done by itself.
//! - **Done in the inbox** ([`mark_read`]): after `inbox.set_item_done` or
//!   `inbox.create_task_from_item` marked a mail entry done, the core hands it here and its mail is
//!   marked read on the server (`\Seen`), never moved or archived (decision 6 in
//!   `docs/eingang.md`). The same for the GitHub mail filed with a GitHub entry (task E5.6). The
//!   inbox checked before that the caller may write mail (and those accounts).
//! - **GitHub mail** (task E5.6, [`github_thread`]): a mail is one about a GitHub issue or pull
//!   request only when `From` is `notifications@github.com` with `X-GitHub-Reason`, `List-Id` names
//!   the repository (`<repo.owner.github.com>`) and the `Message-ID` the same repository and the
//!   number (`<owner/repo/pull/<n>…@github.com>`, `/issues/<n>`). `In-Reply-To` is not looked at:
//!   the store does not keep it, and every GitHub mail names its item in its own `Message-ID`.
//!   Anything that does not agree is an ordinary mail. The inbox then files it with the item's
//!   GitHub entry (`Inbox::file_mail`).
//! - **Missed changes** (the feed fell behind the service, `RecvError::Lagged`): every account is
//!   looked at anew ([`Feed::catch_up`]): unread mail of the last [`FIRST_SYNC_DAYS`] days is filed
//!   (a message filed before is never filed twice) and every open entry is checked.
//! - **Accounts gone** (removed, or mail switched off): every [`RETIRE_EVERY`] the open entries of
//!   accounts the service no longer syncs are done ([`Feed::retire`]); nothing could mark their
//!   mail read any more. Kept here rather than where accounts change, so the core's account
//!   commands need not know the inbox.
//! - **Tasks made of mail** (task E8.7a): a task keeps its mail as `mail:<address>:<Message-ID>`;
//!   [`mail_for_task`], [`linked_mail`], [`linked_details`], [`mails_of`] and [`conversation_of`]
//!   find the mail again by its Message-ID in whatever folder it is, and [`from_mail_line`] names it
//!   in the task's description. They read the store of the account named, whoever asks: the core
//!   checks the caller's rights before.
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
use torro_core::i18n::{tr, trf};
use torro_inbox::{Item, LaneReason, Sender, SourceKind, State};
use torro_mail::{Flag, MailAddress, MessageTraits, Notify, Role};
use torro_pim_github::IssueRef;

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

/// Whether the mail was sent by a machine or to a list (`docs/eingang.md`, "Sortieren"), and which
/// header says so, in this order: `List-Id`
/// ([`LaneReason::List`]), `Precedence` ([`LaneReason::Bulk`]), `Auto-Submitted`
/// ([`LaneReason::Automated`], other than `no`); `None` for mail a person wrote. Bulk mail goes to
/// "Info" with this reason, unless the person keeps the sender elsewhere. `List-Unsubscribe` alone
/// counts for nothing (issue #128): services put it on mail meant for one person too – a creator's
/// post to a supporter, a receipt –, and such mail belongs "for me".
pub fn bulk_reason(traits: &MessageTraits) -> Option<LaneReason> {
    let precedence = traits.precedence.as_deref().is_some_and(|value| matches!(value, "bulk" | "list" | "junk"));
    let automatic = traits.auto_submitted.as_deref().is_some_and(|value| value != "no");
    if traits.list_id.is_some() {
        Some(LaneReason::List)
    } else if precedence {
        Some(LaneReason::Bulk)
    } else if automatic {
        Some(LaneReason::Automated)
    } else {
        None
    }
}

/// The sender of every mail GitHub sends about notifications.
const GITHUB_SENDER: &str = "notifications@github.com";

/// The GitHub issue or pull request a mail is about (task E5.6), from `from` (`From:`), its
/// `traits` and `message_id` (`Message-ID:`, with or without angle brackets): only when all three
/// agree, as the module docs say, else `None` – an ordinary mail. The repository is written as the
/// `Message-ID` has it. Pure: nothing is looked up, nothing logged. Headers come from outside, so
/// this decides nothing but which GitHub entry the mail may join (`Inbox::file_mail`).
pub fn github_thread(from: Option<&MailAddress>, traits: &MessageTraits, message_id: Option<&str>) -> Option<IssueRef> {
    if !from?.address.trim().eq_ignore_ascii_case(GITHUB_SENDER) || traits.github.is_none() {
        return None;
    }
    // `List-Id: owner/repo <repo.owner.github.com>`; an owner holds no dot, a repository may.
    let list = traits.list_id.as_deref()?;
    let list = list.rsplit_once('<').and_then(|(_, rest)| rest.trim().strip_suffix('>'))?;
    let host = list.trim().to_ascii_lowercase();
    let (name, owner) = host.strip_suffix(".github.com")?.rsplit_once('.')?;
    // `Message-ID: <owner/repo/pull/42/…@github.com>`.
    let id = message_id?.trim();
    let id = id.strip_prefix('<').unwrap_or(id);
    let id = id.strip_suffix('>').unwrap_or(id);
    let (path, domain) = id.rsplit_once('@')?;
    if !domain.eq_ignore_ascii_case("github.com") {
        return None;
    }
    let mut parts = path.split('/');
    let (id_owner, id_name, page, number) = (parts.next()?, parts.next()?, parts.next()?, parts.next()?);
    let agrees = id_owner.eq_ignore_ascii_case(owner) && id_name.eq_ignore_ascii_case(name);
    let digits = !number.is_empty() && number.len() <= 12 && number.bytes().all(|byte| byte.is_ascii_digit());
    if !(agrees && name_part(id_owner) && name_part(id_name) && matches!(page, "pull" | "issues") && digits) {
        return None;
    }
    let number: u64 = number.parse().ok().filter(|number| *number > 0)?;
    Some(IssueRef { repo: format!("{id_owner}/{id_name}"), number })
}

/// An owner or repository name as GitHub allows it: ASCII letters, digits, `-`, `_` and `.`.
fn name_part(part: &str) -> bool {
    !part.is_empty()
        && part.len() <= 100
        && part.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
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
/// the inbox just marked done (task E4.1), or a done GitHub entry with GitHub mail filed with it
/// (task E5.6, in each of its accounts); any other entry is left alone. The core calls it with the
/// entry `inbox.set_item_done` or `inbox.create_task_from_item` answered with, after the inbox
/// checked the caller's write access to mail (and to those accounts).
///
/// The entry's messages are read at once, so mail that arrives in the conversation meanwhile
/// stays unread. Messages read already or gone are left as they are; failures go to the log (the
/// entry stays done, the mail unread), with the account and a code only.
pub fn mark_read(inbox: &Inbox, mail: &Arc<MailService>, item: &Item) {
    let holds_mail = matches!(item.source.kind, SourceKind::Mail | SourceKind::Github);
    if !holds_mail || !matches!(item.state, State::Done { .. }) {
        return;
    }
    let by_account = match inbox.mail_by_account(&item.id) {
        Ok(by_account) if !by_account.is_empty() => by_account,
        Ok(_) => return,
        Err(err) => {
            let source = item.source.key();
            eprintln!("torroworkplace: inbox: cannot read the mail of a done entry of {source}: {err}");
            return;
        }
    };
    let Ok(runtime) = tokio::runtime::Handle::try_current() else { return };
    for (account, messages) in by_account {
        let mail = Arc::clone(mail);
        runtime.spawn(async move {
            if let Err(err) = set_seen(&mail, &account, messages).await {
                eprintln!(
                    "torroworkplace: inbox: cannot mark the mail of a done entry of {account} read: {}",
                    err.code()
                );
            }
        });
    }
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

// ----- tasks made of mail (task E8.7a) -----
//
// A task keeps its mail as `mail:<address>:<Message-ID>` (`torro_pim::model::Link::mail`). These
// look the mail up again in the account's store by its Message-ID, so a mail moved to another
// folder is still found. Rights are the caller's to check: they read with the store of `account`
// alone, whoever asks.

/// A mail as a task is made of it ([`mail_for_task`]): its summary, the start of its text and its
/// `Message-ID` header, `None` when it has none (it cannot be linked then).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MailForTask {
    pub summary: torro_mail::MessageSummary,
    pub rfc_message_id: Option<String>,
}

/// The mail `id` (`m:…`) of `account` as a task is made of it. [`MailError::NotFound`] for an
/// account without mail on, or a mail its store does not hold.
pub async fn mail_for_task(mail: &MailService, account: &str, id: &str) -> Result<MailForTask, MailError> {
    let store = mail.store(account).await?;
    let id = id.to_string();
    let found = store
        .run(move |store| {
            let Some(summary) = store.summary(&id)? else { return Ok(None) };
            let rfc_message_id = store.resolve_message(&id)?.and_then(|found| found.rfc_message_id);
            Ok(Some(MailForTask { summary, rfc_message_id }))
        })
        .await
        .map_err(MailError::Store)?;
    found.ok_or(MailError::NotFound)
}

/// The mail message `id` (`m:…`) as an entry made of it would be (issue #123,
/// [`torro_inbox::rules::message_stand_in`]): its account, conversation, sender and the traits the
/// feed would give it ([`bulk_reason`], `List-Id`), never stored. For muting a folder's message
/// that has no entry. Looked up only in the stores `reach` sees ([`MailService::stores`]); `None`
/// when none of them holds it, or a store fails (logged with the account, never the id).
pub async fn message_stand_in(mail: &MailService, reach: Reach<'_>, id: &str) -> Option<Item> {
    for (account, store) in mail.stores(reach).await {
        let lookup = id.to_string();
        match store.run(move |store| store.message(&lookup)).await {
            Ok(Some(message)) => {
                let (list, bulk) = (message.traits.list_id.as_deref(), bulk_reason(&message.traits).is_some());
                return torro_inbox::rules::message_stand_in(&message.summary, list, bulk);
            }
            Ok(None) => {}
            Err(err) => eprintln!("torroworkplace: inbox: cannot look up a mail to mute in {account}: {err}"),
        }
    }
    None
}

/// The newest mail of `account` with the `Message-ID` `message_id` (any folder), `None` when its
/// store holds none, the account has no mail on or the store fails (that goes to the log, without
/// the id).
pub async fn linked_mail(mail: &MailService, account: &str, message_id: &str) -> Option<torro_mail::MessageSummary> {
    let store = mail.store(account).await.ok()?;
    let rfc = torro_pim::model::bare_message_id(message_id).to_string();
    match store.run(move |store| store.messages_by_rfc_id(&rfc)).await {
        Ok(found) => found.into_iter().next(),
        Err(err) => {
            eprintln!("torroworkplace: inbox: cannot look up the mail of a task in {account}: {err}");
            None
        }
    }
}

/// What a task's link shows of its mail ([`torro_pim::model::MailDetails`]): subject, sender and
/// date of the newest copy with `message_id`, and the replies after it that are not the
/// account's own. `None` as for [`linked_mail`].
pub async fn linked_details(
    mail: &MailService,
    account: &str,
    message_id: &str,
) -> Option<torro_pim::model::MailDetails> {
    let found = linked_mail(mail, account, message_id).await?;
    let store = mail.store(account).await.ok()?;
    let own = account.strip_prefix(torro_mail::ACCOUNT_PREFIX).unwrap_or(account).to_string();
    let (thread, date) = (found.thread_id.clone(), found.date);
    let new_replies = store.run(move |store| store.replies_after(&thread, date, &own)).await.unwrap_or_default();
    let sender = found.from.as_ref().map(|from| sender(from).name);
    Some(torro_pim::model::MailDetails {
        message: found.id,
        subject: found.subject,
        sender_address: found.from.map(|from| from.address),
        sender,
        date: found.date,
        new_replies,
        untrusted: true,
    })
}

/// What a mail entry's mails (`messages`, `m:…` of `account`) are, to find the tasks made of them:
/// their `Message-ID`s and their conversations (`thread_id`), as the store has them. A task linked
/// to a mail of the same conversation belongs to the entry too, also after that mail was moved or
/// deleted and the entry holds only a reply. Mails no longer held are left out, and so is
/// everything when the store fails.
pub async fn mails_of(mail: &MailService, account: &str, messages: Vec<String>) -> (Vec<String>, Vec<String>) {
    let Ok(store) = mail.store(account).await else { return (Vec::new(), Vec::new()) };
    let found = store
        .run(move |store| {
            let (mut ids, mut threads) = (Vec::new(), Vec::new());
            for message in &messages {
                ids.extend(store.resolve_message(message)?.and_then(|found| found.rfc_message_id));
                threads.extend(store.summary(message)?.map(|summary| summary.thread_id));
            }
            Ok((ids, threads))
        })
        .await;
    found.unwrap_or_default()
}

/// The ids (`m:…`) of every mail of the conversation of the mail with `message_id` in `account`,
/// its copies included: what an entry holding that conversation holds. Empty when the store holds
/// no such mail. `Err` only for an account without mail on ([`MailError::NotFound`]) or a store
/// that failed, so a caller can tell "gone" from "unknown".
pub async fn conversation_of(mail: &MailService, account: &str, message_id: &str) -> Result<Vec<String>, MailError> {
    let store = mail.store(account).await?;
    let rfc = torro_pim::model::bare_message_id(message_id).to_string();
    let found = store
        .run(move |store| {
            let copies = store.messages_by_rfc_id(&rfc)?;
            let mut ids: Vec<String> = copies.iter().map(|summary| summary.id.clone()).collect();
            if let Some(newest) = copies.first() {
                ids.extend(store.thread_newest(&newest.thread_id, THREAD_LOOKUP_MAX)?.into_iter().map(|m| m.id));
            }
            Ok(ids)
        })
        .await
        .map_err(MailError::Store)?;
    Ok(found)
}

/// At most this many mails of a conversation are looked at ([`conversation_of`]), the newest: a
/// conversation a sender made up to be huge stays bounded.
const THREAD_LOOKUP_MAX: u32 = 100;

/// The readable line a task made of a mail ends its description with (task E8.7a): "From mail:
/// Anna Keller, “Offer”, 13 Oct", so the mail is named also where `X-TORRO-LINK` means nothing
/// (Nextcloud Tasks, other programs). Sender and subject are the mail's; the day in local time.
pub fn from_mail_line(summary: &torro_mail::MessageSummary) -> String {
    let who = summary.from.as_ref().map_or_else(|| tr("unknown sender").to_string(), |from| sender(from).name);
    let subject = match summary.subject.trim() {
        "" => tr("(no subject)").to_string(),
        subject => subject.to_string(),
    };
    let day = torro_gui::datetime::format_day_month(summary.date.with_timezone(&chrono::Local).date_naive());
    trf("From mail: {sender}, “{subject}”, {day}", &[("sender", &who), ("subject", &subject), ("day", &day)])
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
        // Only a mail with `X-GitHub-Reason` can be a GitHub mail: the others need no lookup.
        let github = match &message.traits.github {
            Some(_) => {
                let rfc = store.resolve_message(id)?.and_then(|found| found.rfc_message_id);
                github_thread(summary.from.as_ref(), &message.traits, rfc.as_deref())
            }
            None => None,
        };
        mails.push(NewMail {
            account: account.to_string(),
            message: summary.id,
            thread: summary.thread_id,
            subject: summary.subject,
            snippet: summary.snippet,
            from: summary.from.as_ref().map(sender),
            bulk: bulk_reason(&message.traits),
            list: message.traits.list_id.clone(),
            date: summary.date,
            github,
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
pub(crate) mod tests {
    use std::path::Path;

    use torro_inbox::item::FacetKey;
    use torro_inbox::{Kind, Lane, Link};
    use torro_ipc::{Request, Response};
    use torro_mail::MailSettings;
    use torro_pim::AccountState;

    use torro_inbox::SourceRef;

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
    pub(crate) fn anna(dir: &Path, server: &FakeImap, notify: Notify) -> MemoryTokens {
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
        let all = Request::ListItems {
            lane: None,
            source: None,
            state: None,
            since: None,
            limit: None,
            order: None,
            dismissed: None,
        };
        match inbox.handle(all, None, Utc::now()) {
            Response::Items { items } => items,
            other => panic!("expected items, got {other:?}"),
        }
    }

    fn entry<'a>(items: &'a [Item], title: &str) -> &'a Item {
        items.iter().find(|item| item.title == title).unwrap_or_else(|| panic!("no entry {title:?} in {items:?}"))
    }

    pub(crate) async fn synced(mail: &MailService) -> bool {
        let infos = mail.infos(Reach::Suite).await;
        !infos.is_empty()
            && infos.iter().all(|info| matches!(&info.state, AccountState::Connected { last_sync: Some(_), .. }))
    }

    pub(crate) fn idling(server: &FakeImap) -> bool {
        server.lines().iter().any(|line| line.ends_with(" IDLE"))
    }

    /// A mail as GitHub sends it about `torro/workplace#42`, with `Message-ID` `message_id`,
    /// `List-Id` `list` and the sender `from`, sent `minutes_ago` minutes ago.
    pub(crate) fn github_letter(message_id: &str, list: &str, from: &str, minutes_ago: i64) -> String {
        let date = (Utc::now() - TimeDelta::minutes(minutes_ago)).to_rfc2822();
        format!(
            "From: Bernd Beispiel <{from}>\r\nTo: torro/workplace <workplace@noreply.github.com>\r\n\
             Subject: [torro/workplace] Add the inbox (PR #42)\r\nDate: {date}\r\nMessage-ID: <{message_id}>\r\n\
             X-GitHub-Reason: review_requested\r\nList-ID: torro/workplace <{list}>\r\nPrecedence: list\r\n\
             Content-Type: text/plain; charset=utf-8\r\n\r\n@bernd requested your review.\r\n"
        )
    }

    /// The `List-Id` of `torro/workplace` and the `Message-ID` of a review request on its #42.
    pub(crate) const GITHUB_LIST: &str = "workplace.torro.github.com";
    pub(crate) const REVIEW_ID: &str = "torro/workplace/pull/42/issue_event/1234567@github.com";

    #[test]
    fn a_github_mail_is_one_only_when_sender_list_and_message_id_agree() {
        let from = MailAddress { name: Some("Bernd".into()), address: "Notifications@GitHub.com".into() };
        let traits = MessageTraits {
            list_id: Some("torro/workplace <workplace.torro.github.com>".into()),
            github: Some("review_requested".into()),
            ..MessageTraits::default()
        };
        let thread = |from: &MailAddress, traits: &MessageTraits, id: &str| github_thread(Some(from), traits, Some(id));
        let pr = IssueRef { repo: "torro/workplace".into(), number: 42 };
        assert_eq!(thread(&from, &traits, REVIEW_ID), Some(pr.clone()));
        assert_eq!(thread(&from, &traits, "<torro/workplace/pull/42@github.com>"), Some(pr), "in angle brackets");
        let issue = thread(&from, &traits, "Torro/Workplace/issues/7/987654@github.com").expect("an issue");
        assert_eq!((issue.repo.as_str(), issue.number), ("Torro/Workplace", 7), "as the Message-ID writes it");
        let dotted = MessageTraits { list_id: Some("<torro.github.io.torro.github.com>".into()), ..traits.clone() };
        assert_eq!(
            thread(&from, &dotted, "torro/torro.github.io/pull/3@github.com"),
            Some(IssueRef { repo: "torro/torro.github.io".into(), number: 3 }),
            "a repository with dots"
        );

        let someone = MailAddress { name: None, address: "notifications@github.com.evil.example".into() };
        let no_reason = MessageTraits { github: None, ..traits.clone() };
        let other_list = MessageTraits { list_id: Some("torro/site <site.torro.github.com>".into()), ..traits.clone() };
        let no_list = MessageTraits { list_id: None, ..traits.clone() };
        assert_eq!(thread(&someone, &traits, REVIEW_ID), None, "another sender");
        assert_eq!(thread(&from, &no_reason, REVIEW_ID), None, "no X-GitHub-Reason");
        assert_eq!(thread(&from, &other_list, REVIEW_ID), None, "the list names another repository");
        assert_eq!(thread(&from, &no_list, REVIEW_ID), None);
        for id in [
            "torro/workplace/pull/42@github.com.evil.example",
            "torro/site/pull/42@github.com",
            "torro/workplace/commit/0123abc@github.com",
            "torro/workplace/pull/0@github.com",
            "torro/workplace/pull/4x2@github.com",
            "torro/workplace/pull@github.com",
            "torro/work place/pull/42@github.com",
            "42@example.org",
        ] {
            assert_eq!(thread(&from, &traits, id), None, "{id}");
        }
        assert_eq!(github_thread(None, &traits, Some(REVIEW_ID)), None);
        assert_eq!(github_thread(Some(&from), &traits, None), None);
    }

    #[tokio::test]
    async fn the_feed_files_a_github_mail_with_its_github_entry_and_a_contradicting_one_as_mail() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &github_letter(REVIEW_ID, GITHUB_LIST, GITHUB_SENDER, 2), &[]);
        let forged = github_letter("torro/workplace/pull/42/c1@github.com", "site.torro.github.com", GITHUB_SENDER, 1);
        server.add("INBOX", &forged, &[]);
        let secrets = anna(dir.path(), &server, Notify::All);
        let inbox = Arc::new(inbox_in(dir.path()));
        let review = crate::inbox::tests::remote(SourceRef::github("anna-berg"), "42", &["101"], Utc::now());
        let github_entry = crate::inbox::tests::filed_remote(&inbox, &review, Utc::now());
        let mail = Arc::new(service_in(dir.path(), &secrets));
        start(Arc::clone(&inbox), Arc::clone(&mail), Some(accounts_file(dir.path())));
        mail.reload().await;

        eventually("filed", || async { entries(&inbox).len() == 2 }).await;
        eventually("synced", || synced(&mail)).await;
        let items = entries(&inbox);
        let mail_entry = items.iter().find(|item| item.id != github_entry).expect("the contradicting mail");
        assert_eq!(mail_entry.source.key(), ANNA, "an ordinary mail entry");
        let filed = inbox.mail_by_account(&github_entry).expect("read");
        assert_eq!(filed.get(ANNA).map(Vec::len), Some(1), "the review request went with the GitHub entry");
    }

    #[test]
    fn list_and_machine_mail_is_bulk_and_says_why() {
        assert_eq!(bulk_reason(&MessageTraits::default()), None);
        let a_person = MessageTraits { auto_submitted: Some("no".into()), ..MessageTraits::default() };
        assert_eq!(bulk_reason(&a_person), None);
        let unsubscribe = MessageTraits { list_unsubscribe: true, ..MessageTraits::default() };
        assert_eq!(bulk_reason(&unsubscribe), None, "List-Unsubscribe alone is no sign (issue #128)");
        for (traits, reason) in [
            (MessageTraits { list_id: Some("<news.shop.example>".into()), ..unsubscribe.clone() }, LaneReason::List),
            (MessageTraits { precedence: Some("bulk".into()), ..MessageTraits::default() }, LaneReason::Bulk),
            (MessageTraits { precedence: Some("list".into()), ..unsubscribe.clone() }, LaneReason::Bulk),
            (MessageTraits { auto_submitted: Some("auto-generated".into()), ..unsubscribe }, LaneReason::Automated),
        ] {
            assert_eq!(bulk_reason(&traits), Some(reason), "{traits:?}");
        }
    }

    /// The headers of the mail of issue #128 (`tests/fixtures/mail/creator-post.eml`): a creator's
    /// post as a membership platform sends it to a supporter, sent now, with `List-Unsubscribe` but
    /// neither `List-Id`, `Precedence` nor `Auto-Submitted`; CRLF line ends, as servers hand out mail.
    fn creator_post() -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/mail/creator-post.eml");
        let text = std::fs::read_to_string(path).expect("the fixture");
        text.replace("{DATE}", &Utc::now().to_rfc2822()).replace("\r\n", "\n").replace('\n', "\r\n")
    }

    /// Issue #128: the mail was filed in "Info" only because of `List-Unsubscribe`, so it never
    /// stood under "For me".
    #[test]
    fn a_post_with_only_list_unsubscribe_is_for_me() {
        let read = torro_mail_imap::mime::read(creator_post().as_bytes()).expect("readable");
        assert!(read.traits.list_unsubscribe, "the fixture has the header: {:?}", read.traits);
        assert_eq!(bulk_reason(&read.traits), None);
        assert_eq!(torro_inbox::rules::mail_lane(bulk_reason(&read.traits).is_some(), None), Lane::ForMe);
    }

    #[tokio::test]
    async fn the_feed_files_the_post_of_issue_128_for_me_and_a_newsletter_in_info_with_its_reason() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &creator_post(), &[]);
        let news = letter("Weekly news", "Shop <news@shop.example>", 0, "List-Id: <news.shop.example>\r\n", None);
        server.add("INBOX", &news, &[]);
        let secrets = anna(dir.path(), &server, Notify::All);
        let (inbox, mail) = fed(dir.path(), &secrets).await;

        eventually("filed", || async { entries(&inbox).len() == 2 }).await;
        eventually("synced", || synced(&mail)).await;
        let items = entries(&inbox);
        let post = items.iter().find(|item| item.title != "Weekly news").expect("the post");
        assert_eq!((post.lane, post.reason), (Lane::ForMe, None), "{post:?}");
        let news = entry(&items, "Weekly news");
        assert_eq!((news.lane, news.reason), (Lane::Info, Some(LaneReason::List)));
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

    /// Issue #123: a folder's message that never became an entry stands in for one to mute by, with
    /// the traits the feed would give it – only for whom reaches its account.
    #[tokio::test]
    async fn a_message_of_a_folder_stands_in_for_an_entry_to_mute_by() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let news = letter("Weekly news", "Shop <news@shop.example>", 0, "List-Id: <news.shop.example>\r\n", None);
        server.add("Archive", &news, &[]);
        let secrets = anna(dir.path(), &server, Notify::All);
        let (inbox, mail) = fed(dir.path(), &secrets).await;
        eventually("synced", || synced(&mail)).await;
        let search = Request::SearchMail {
            account: None,
            folder: None,
            query: None,
            unread: None,
            flagged: None,
            since: None,
            before: None,
            limit: 10,
            cursor: None,
            order: None,
            sort: None,
        };
        let Response::MailMessages { messages, .. } = crate::mail::read::answer(&mail, Reach::Suite, search).await
        else {
            panic!("a page of messages")
        };
        let id = messages.into_iter().find(|message| message.subject == "Weekly news").expect("synced").id;

        let stand_in = message_stand_in(&mail, Reach::Suite, &id).await.expect("the message");
        assert_eq!((stand_in.id.as_str(), stand_in.source.key()), (id.as_str(), ANNA.to_string()));
        let keys: Vec<FacetKey> = stand_in.facets.iter().map(|facet| facet.key).collect();
        assert_eq!(keys, [FacetKey::Account, FacetKey::Sender, FacetKey::Domain, FacetKey::List, FacetKey::Bulk]);
        assert_eq!(inbox.entry_of_message(ANNA, &id), None, "an archived mail is never filed");
        let mut rights = torro_ipc::api::RightsTemplate::FullAccess.rights();
        rights.mail_accounts = torro_ipc::api::Selection::Only(vec!["mail:bernd@example.com".into()]);
        assert_eq!(message_stand_in(&mail, Reach::Client(&rights), &id).await, None, "another account only");
        assert_eq!(message_stand_in(&mail, Reach::Suite, "m:nope").await, None);
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
            let all = Request::ListItems {
                lane: None,
                source: None,
                state: None,
                since: None,
                limit: None,
                order: None,
                dismissed: None,
            };
            match core.handle(all).await {
                Response::Items { items } => items,
                other => panic!("expected items, got {other:?}"),
            }
        };

        let uid = server.add("INBOX", &bernd("Offer", 0), &[]);
        let other = server.add("INBOX", &bernd("Another", 0), &[]);
        eventually("filed", || async { listed().await.len() == 2 }).await;
        let id = entry(&listed().await, "Offer").id.clone();
        let done = core.handle(Request::SetItemDone { id, done: true, dismissed: false }).await;
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
        inbox.handle(Request::SetItemDone { id, done: false, dismissed: false }, None, Utc::now());
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
