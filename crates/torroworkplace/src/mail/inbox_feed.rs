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
//! - **Mail filters** (issue #339, `Feed::filter`): before new mail of an inbox folder is filed,
//!   and whatever `notify` says, the enabled filters of the account run on it
//!   ([`super::filters::on_arrival`]): read, category, move. Mail they read or move gets no entry;
//!   a category they set goes on the entry before the sorting rules ([`NewMail::category`]). Not
//!   on a folder's first sync and not on a catch-up: that mail was there before.
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
//! - **Replies to a follow-up** (task E19.1, `Feed::note_replies`): new mail of **any** folder, read
//!   or not, a first sync too, that lies in the conversation of a follow-up "only if no reply" and
//!   is not from the account itself calls that follow-up off ([`Inbox::reply_came`]), before the
//!   inbox folder's mail is filed. A catch-up asks the store for replies the feed missed
//!   ([`MailStore::replies_after`]). A due follow-up "only if no reply" wakes only after the same
//!   look ([`wake_follow_ups`], every [`RETIRE_EVERY`]): a reply dated before its time may reach
//!   the store after it.
//! - **Answered from "Reply later"** (task E19.2, decision 5, `Feed::note_replies` as well): new mail
//!   of any folder sent from the account itself takes the mail of its conversation off "Reply
//!   later" that was put there before it ([`Inbox::answered`]), whatever program sent it; "Set
//!   aside" stays. A catch-up asks the store for mail of one's own the feed missed
//!   ([`MailStore::sent_after`]).
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
use torro_mail::{Flag, MailAddress, MailCategory, MessageTraits, Notify, Role};
use torro_pim_github::IssueRef;

use super::MailStoreError;
use super::service::{MailChange, MailError, MailService, SharedStore};
use super::store::{MailStore, SearchQuery};
use crate::inbox::{DueFollowUp, Inbox, NewMail};
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
/// its own. `accounts_file` is `accounts.json`, where each account's `notify` is read when its
/// mail comes; `None` (no home directory) files nothing. The core calls it once, on its runtime.
///
/// `decide` asks a decision model what new mail is that the fixed rules could not sort (task
/// E12.6); `None` leaves such mail without a category – exactly what happens when nobody set a
/// model up.
pub fn start_with(inbox: Arc<Inbox>, mail: Arc<MailService>, accounts_file: Option<PathBuf>, decide: Option<Decider>) {
    let changes = mail.subscribe();
    tokio::spawn(Feed { inbox, mail, accounts_file, decide }.run(changes));
}

/// How long one mail may keep the decision model busy, and how long the run waits before the
/// next one (task E12.6): a model that hangs must not keep a round of new mail waiting for ever,
/// and the machine stays usable while it works.
const DECIDE_TIMEOUT: Duration = Duration::from_secs(60);
const DECIDE_PAUSE: Duration = Duration::from_millis(250);

/// Asks the model about `messages` of `account`, one at a time, and puts what it says on their
/// entries too (task E12.6). Runs beside the filing, never in it: the entries are there already,
/// and the category reaches them as soon as the model answered.
async fn decide_later(decide: Decider, inbox: Arc<Inbox>, account: String, messages: Vec<String>) {
    for message in messages {
        let asked = decide(account.clone(), message.clone());
        let category = match tokio::time::timeout(DECIDE_TIMEOUT, asked).await {
            Ok(category) => category,
            Err(_) => {
                eprintln!("torroworkplace: inbox: the decision model took too long for a mail of {account}");
                return;
            }
        };
        // The entry of that mail reads alike; the decider already kept the category with the mail.
        if let Some(category) = category
            && let Some(entry) = inbox.entry_of_message(&account, &message)
            && let Err(err) = inbox.set_category(&entry, Some(category))
        {
            eprintln!("torroworkplace: inbox: cannot put a category on an entry of {account}: {err}");
        }
        tokio::time::sleep(DECIDE_PAUSE).await;
    }
}

/// Asks a decision model what the mail `message` of `account` is and keeps the answer with it
/// (task E12.6); `Ok(None)` when nothing could be said. The core wires `Core::decide_new_mail`
/// in here, so the feed need not know about AI accounts, assignments or the data rule.
pub type Decider =
    Arc<dyn Fn(String, String) -> std::pin::Pin<Box<dyn Future<Output = Option<MailCategory>> + Send>> + Send + Sync>;

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
/// then in the store, so the next sync finds nothing changed; a sync meanwhile leaves their flags
/// alone ([`MailService::hold_flags`]). Returns how many were marked. A folder the server
/// renumbered since is skipped: its UIDs name other mail now.
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
    let mut by_folder: BTreeMap<String, Vec<u32>> = BTreeMap::new();
    for target in targets {
        by_folder.entry(target.folder.clone()).or_default().push(target.uid);
        folders.entry((target.folder_path, target.uidvalidity)).or_default().push((target.id, target.uid));
    }
    if folders.is_empty() {
        return Ok(0);
    }
    // Until the store has `\Seen` too, the account's sync keeps the server's flags of these
    // messages out of the store: it may have read them before the `STORE` (issue #318).
    let mut held = Vec::new();
    for (folder, uids) in &by_folder {
        held.push(mail.hold_flags(account, folder, uids).await?);
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

/// The mail message `id` (`m:…`) as the feed would file it ([`Inbox::file_mail`]), for "Later" on a
/// mail without an entry (task E9.1): `inbox.snooze_item` files it first, then puts the entry off.
/// Any folder, read or not. Looked up only in the stores `reach` sees, as [`message_stand_in`];
/// `None` when none of them holds it, or a store fails (logged with the account, never the id).
pub async fn mail_to_snooze(mail: &MailService, reach: Reach<'_>, id: &str) -> Option<NewMail> {
    for (account, store) in mail.stores(reach).await {
        let (lookup, owner) = (id.to_string(), account.clone());
        match store.run(move |store| mail_to_file(store, &owner, &lookup)).await {
            Ok(Some(found)) => return Some(found),
            Ok(None) => {}
            Err(err) => eprintln!("torroworkplace: inbox: cannot look up a mail to snooze in {account}: {err}"),
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
    let own = own_address(account);
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
    /// Asks a decision model about mail the fixed rules could not sort (task E12.6); `None`
    /// leaves such mail without a category.
    decide: Option<Decider>,
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
                _ = retire.tick() => {
                    self.retire(Utc::now()).await;
                    wake_follow_ups(&self.inbox, &self.mail, Utc::now()).await;
                }
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

    /// One change of one folder at `now`: new mail of any folder may be a reply a follow-up waits
    /// for ([`Feed::note_replies`]), new mail of an inbox folder is filed, messages whose flags
    /// changed or that went away may settle their entries.
    async fn take(&self, change: MailChange, now: DateTime<Utc>) {
        let MailChange { account, folder, role, new, flags, removed, initial } = change;
        let store = match self.mail.store(&account).await {
            Ok(store) => store,
            Err(err) => {
                eprintln!("torroworkplace: inbox: cannot read the mail of {account}: {}", err.code());
                return;
            }
        };
        // Before filing: a reply in the inbox folder then opens a conversation the follow-up let go,
        // rather than bringing it back from "Later".
        if !new.is_empty() {
            self.note_replies(&store, &account, &new, now).await;
        }
        if role == Some(Role::Inbox) && !new.is_empty() {
            // The filters first (issue #339), whatever `notify` says: mail they read or move gets no
            // entry. A folder's first sync brings mail that was there before, which they leave.
            let by_filter = match initial {
                true => Vec::new(),
                false => self.filter(&store, &account, &new).await,
            };
            let since = initial.then(|| now - TimeDelta::days(FIRST_SYNC_DAYS));
            self.file(&store, &account, folder, new, since, &by_filter, now).await;
        }
        if !removed.is_empty() {
            self.forget_gone(&store, &account, Some(&removed)).await;
        }
        let touched: Vec<String> = flags.into_iter().chain(removed).collect();
        if !touched.is_empty() {
            self.settle(&store, &account, Some(&touched), now).await;
        }
    }

    /// The mails `new` of `account` (any folder, read or not, a first sync too) at `now`:
    ///
    /// - one that answers a follow-up "only if no reply" (task E19.1, decision 6) – mail of its
    ///   conversation not sent from the account itself – calls the follow-up off
    ///   ([`Inbox::reply_came`]);
    /// - one sent from the account itself, by any program, takes the mail of its conversation off
    ///   "Reply later" that was put there before it was dated (task E19.2, decision 5,
    ///   [`Inbox::answered`]); "Set aside" stays.
    ///
    /// Nothing is read from the store while no follow-up waits for a reply and nothing lies on
    /// "Reply later".
    async fn note_replies(&self, store: &SharedStore, account: &str, new: &[String], now: DateTime<Utc>) {
        let waiting = self.inbox.awaiting_reply(account).unwrap_or_else(|err| {
            eprintln!("torroworkplace: inbox: cannot read the follow-ups of {account}: {err}");
            Vec::new()
        });
        let piled = self.inbox.reply_later_of(account).unwrap_or_else(|err| {
            eprintln!("torroworkplace: inbox: cannot read the piles of {account}: {err}");
            Vec::new()
        });
        if waiting.is_empty() && piled.is_empty() {
            return;
        }
        let threads: Vec<String> = waiting.into_iter().map(|(thread, _)| thread).collect();
        let own = own_address(account);
        let ids = new.to_vec();
        let found = store
            .run(move |store| {
                // A draft is neither sent nor a reply (review of #315): saved, or left by a sending
                // that failed, it must not take mail off "Reply later" or end a follow-up.
                let drafts = store.folder_by_role(Role::Drafts)?.map(|folder| folder.id);
                let (mut replies, mut sent) = (Vec::new(), Vec::new());
                // One query for the whole change, however many mails it brings (review of #315).
                for summary in summaries(store, &ids)? {
                    if drafts.as_ref() == Some(&summary.folder) {
                        continue;
                    }
                    let mine = summary.from.as_ref().is_some_and(|from| from.address.eq_ignore_ascii_case(&own));
                    match mine {
                        true => sent.push((summary.thread_id, summary.date)),
                        false if threads.contains(&summary.thread_id) => {
                            replies.push((summary.thread_id, summary.date))
                        }
                        false => {}
                    }
                }
                // The mail on "Reply later" in the conversation of a mail of one's own, with its date.
                let mut answered = Vec::new();
                if !sent.is_empty() {
                    let messages: Vec<String> = piled.iter().map(|(message, _)| message.clone()).collect();
                    for summary in summaries(store, &messages)? {
                        let message = summary.id.clone();
                        let dates = sent.iter().filter(|(thread, _)| *thread == summary.thread_id);
                        answered.extend(dates.map(|(_, date)| (message.clone(), *date)));
                    }
                }
                Ok((replies, answered))
            })
            .await;
        let (replies, answered) = match found {
            Ok(found) => found,
            Err(err) => {
                eprintln!("torroworkplace: inbox: cannot read the new mail of {account}: {err}");
                return;
            }
        };
        for (thread, date) in replies {
            if let Err(err) = self.inbox.reply_came(account, &thread, date, now) {
                eprintln!("torroworkplace: inbox: cannot note a reply in {account}: {err}");
            }
        }
        for (message, sent) in answered {
            if let Err(err) = self.inbox.answered(account, &[message], sent) {
                eprintln!("torroworkplace: inbox: cannot take mail off its pile in {account}: {err}");
            }
        }
    }

    /// The mail of `account` on "Reply later" checked anew at `now` against everything its store
    /// holds (task E19.2), after changes were missed: a conversation with a mail of one's own dated
    /// after the mail was put there ([`MailStore::sent_after`]) takes it off ([`Inbox::answered`],
    /// with `now` for the date: it came after, which is all that is asked).
    async fn catch_up_answered(&self, store: &SharedStore, account: &str, now: DateTime<Utc>) {
        let piled = match self.inbox.reply_later_of(account) {
            Ok(piled) if piled.is_empty() => return,
            Ok(piled) => piled,
            Err(err) => {
                eprintln!("torroworkplace: inbox: cannot read the piles of {account}: {err}");
                return;
            }
        };
        let own = own_address(account);
        let answered = store
            .run(move |store| {
                let mut answered = Vec::new();
                for (message, at) in piled {
                    let Some(summary) = store.summary(&message)? else { continue };
                    if store.sent_after(&summary.thread_id, at, &own)? > 0 {
                        answered.push(message);
                    }
                }
                Ok(answered)
            })
            .await;
        match answered {
            Ok(answered) if answered.is_empty() => {}
            Ok(answered) => {
                if let Err(err) = self.inbox.answered(account, &answered, now) {
                    eprintln!("torroworkplace: inbox: cannot take mail off its pile in {account}: {err}");
                }
            }
            Err(err) => eprintln!("torroworkplace: inbox: cannot read the mail of {account}: {err}"),
        }
    }

    /// Takes mail of `account` off its pile that the store no longer holds (review of #315): with
    /// `removed`, only those of these messages (a folder's change said they went), else every mail on
    /// a pile (a catch-up). Deleted mail is gone for good; mail another program moved comes back under
    /// a new id, which the pile cannot know, so its old id goes too. The core's own moves keep the id
    /// and the store holds it still: such mail stays on its pile.
    async fn forget_gone(&self, store: &SharedStore, account: &str, removed: Option<&[String]>) {
        let piled = match self.inbox.mail_piles(&[account.to_string()]) {
            Ok(piles) => piles.all_of(account),
            Err(err) => {
                eprintln!("torroworkplace: inbox: cannot read the piles of {account}: {err}");
                return;
            }
        };
        let asked: Vec<String> = match removed {
            Some(removed) => piled.into_iter().filter(|message| removed.contains(message)).collect(),
            None => piled,
        };
        if asked.is_empty() {
            return;
        }
        let ids = asked.clone();
        let held = match store.run(move |store| summaries(store, &ids)).await {
            Ok(held) => held,
            Err(err) => {
                eprintln!("torroworkplace: inbox: cannot read the mail of {account}: {err}");
                return;
            }
        };
        let gone: Vec<String> =
            asked.into_iter().filter(|message| !held.iter().any(|summary| &summary.id == message)).collect();
        if !gone.is_empty()
            && let Err(err) = self.inbox.forget_piled(account, &gone)
        {
            eprintln!("torroworkplace: inbox: cannot take gone mail off its pile in {account}: {err}");
        }
    }

    /// The follow-ups "only if no reply" of `account` checked anew at `now` against everything its
    /// store holds (task E19.1), after changes were missed: a conversation with a reply since the
    /// follow-up was set ([`MailStore::replies_after`]) calls it off. The reply's own date is not
    /// needed: it came after `since`, which is all [`Inbox::reply_came`] asks, so `now` stands for it.
    async fn catch_up_replies(&self, store: &SharedStore, account: &str, now: DateTime<Utc>) {
        let waiting = match self.inbox.awaiting_reply(account) {
            Ok(waiting) if waiting.is_empty() => return,
            Ok(waiting) => waiting,
            Err(err) => {
                eprintln!("torroworkplace: inbox: cannot read the follow-ups of {account}: {err}");
                return;
            }
        };
        let own = own_address(account);
        let answered = store
            .run(move |store| {
                let mut answered = Vec::new();
                for (thread, since) in waiting {
                    if store.replies_after(&thread, since, &own)? > 0 {
                        answered.push(thread);
                    }
                }
                Ok(answered)
            })
            .await;
        match answered {
            Ok(answered) => {
                for thread in answered {
                    if let Err(err) = self.inbox.reply_came(account, &thread, now, now) {
                        eprintln!("torroworkplace: inbox: cannot note a reply in {account}: {err}");
                    }
                }
            }
            Err(err) => eprintln!("torroworkplace: inbox: cannot read the replies of {account}: {err}"),
        }
    }

    /// Looks at every mail account anew at `now`, after changes were missed: calls off follow-ups
    /// that got a reply meanwhile ([`Feed::catch_up_replies`]), takes mail answered meanwhile off
    /// "Reply later" ([`Feed::catch_up_answered`]), files the unread mail of its inbox
    /// folder of the last [`FIRST_SYNC_DAYS`] days (none twice) and settles every open entry.
    async fn catch_up(&self, now: DateTime<Utc>) {
        wake_follow_ups(&self.inbox, &self.mail, now).await;
        for account in self.mail.infos(Reach::Suite).await.into_iter().map(|info| info.id) {
            // Mail switched off: nothing is synced, nothing to file.
            let Ok(store) = self.mail.store(&account).await else { continue };
            self.forget_gone(&store, &account, None).await;
            self.catch_up_replies(&store, &account, now).await;
            self.catch_up_answered(&store, &account, now).await;
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
                // No filters here (issue #339): what a catch-up finds may have been there for days.
                Ok(Some((folder, ids))) => self.file(&store, &account, folder, ids, Some(since), &[], now).await,
                Ok(None) => {}
                Err(err) => eprintln!("torroworkplace: inbox: cannot read the mail of {account}: {err}"),
            }
            self.settle(&store, &account, None, now).await;
        }
    }

    /// Runs the mail filters of `account` on its new mail `new` of the inbox folder
    /// ([`super::filters::on_arrival`], issue #339) and gives back the mails that got a filter's
    /// category, with it. Without the catalogue of categories no category is set; reading and
    /// moving still happen.
    async fn filter(&self, store: &SharedStore, account: &str, new: &[String]) -> Vec<(String, MailCategory)> {
        let categories: Vec<torro_mail::Category> = match self.inbox.catalogue() {
            Ok(catalogue) => catalogue.into_iter().map(|def| def.category).collect(),
            Err(err) => {
                eprintln!("torroworkplace: inbox: cannot read the categories to filter mail of {account}: {err}");
                Vec::new()
            }
        };
        super::filters::on_arrival(&self.mail, store, account, new, &categories).await
    }

    /// Files those of `ids` (messages of `folder`, the inbox folder of `account`) that are still
    /// unread there and, with `since`, not older, as the account's `notify` allows. `by_filter`
    /// names the mails a filter gave a category ([`Feed::filter`]): their entries take it before
    /// the sorting rules (issue #339, decision 4).
    #[expect(clippy::too_many_arguments, reason = "the feed's state and one round of mail")]
    async fn file(
        &self,
        store: &SharedStore,
        account: &str,
        folder: String,
        ids: Vec<String>,
        since: Option<DateTime<Utc>>,
        by_filter: &[(String, MailCategory)],
        now: DateTime<Utc>,
    ) {
        let notify = self.notify_of(account).await;
        if notify == Notify::Off {
            return;
        }
        let owner = account.to_string();
        let mut mails = match store.run(move |store| unread_mail(store, &owner, &folder, &ids, since)).await {
            Ok(mails) => mails,
            Err(err) => {
                eprintln!("torroworkplace: inbox: cannot read the new mail of {account}: {err}");
                return;
            }
        };
        for mail in &mut mails {
            mail.category =
                by_filter.iter().find(|(message, _)| *message == mail.message).map(|(_, category)| *category);
        }
        // Mail that already has a category is left as it is (task E12.6): this path runs again
        // after a gap in the change stream and on the way back from "Later", and sorting it
        // anew would write a rule's answer over what the person filed by hand. The backlog run
        // leaves it alone for the same reason (`uncategorised`).
        let ids: Vec<String> = mails.iter().map(|mail| mail.message.clone()).collect();
        let held: Vec<String> = match store.run(move |store| store.categorised(&ids)).await {
            Ok(held) => held,
            Err(err) => {
                eprintln!("torroworkplace: inbox: cannot read the categories of {account}: {err}");
                Vec::new()
            }
        };
        // What the fixed rules make of each mail (task E12.2): kept with the mail, so the list,
        // the search and the sidebar show it, and put on the entry by `file_mail` as well. Mail
        // nothing can be said about yet stays without one until a model is asked (task E12.6).
        let decided: Vec<(String, MailCategory)> = mails
            .iter()
            .filter(|mail| !held.contains(&mail.message))
            .filter_map(|mail| match self.inbox.mail_category_of(mail) {
                Ok(Some(category)) => Some((mail.message.clone(), category)),
                Ok(None) => None,
                Err(err) => {
                    eprintln!("torroworkplace: inbox: cannot sort the new mail of {account}: {err}");
                    None
                }
            })
            .collect();
        let sorted: Vec<String> = decided.iter().map(|(message, _)| message.clone()).collect();
        if !decided.is_empty()
            && let Err(err) = store
                .run(move |store| {
                    for (message, category) in decided {
                        store.set_category(&message, Some(category))?;
                    }
                    Ok(())
                })
                .await
        {
            eprintln!("torroworkplace: inbox: cannot keep the category of the new mail of {account}: {err}");
        }
        for mail in &mails {
            if let Err(err) = self.inbox.file_mail(mail, notify, now) {
                eprintln!("torroworkplace: inbox: cannot file the new mail of {account}: {err}");
                return;
            }
        }
        // What no rule could tell goes to the decision model – **after** the mail is filed, in a
        // task of its own (task E12.6). New mail must stand in the list at once; a local model
        // takes seconds per mail, and waiting here would hold up every entry of this round.
        if let Some(decide) = &self.decide {
            let undecided: Vec<String> = mails
                .iter()
                .filter(|mail| !sorted.contains(&mail.message) && !held.contains(&mail.message))
                .map(|mail| mail.message.clone())
                .collect();
            if !undecided.is_empty() {
                let (decide, inbox, owner) = (Arc::clone(decide), Arc::clone(&self.inbox), account.to_string());
                tokio::spawn(decide_later(decide, inbox, owner, undecided));
            }
        }
    }

    /// Marks done the open entries of `account` (with `touching`, only those holding one of these
    /// messages) none of whose messages is still unread in the account's inbox folder: the mail was
    /// read, moved or deleted elsewhere. A snoozed entry only once none of its messages is in the
    /// inbox folder at all (task E9.1): reading mail the person put off does not take it out of
    /// "Later".
    ///
    /// An entry with a follow-up (task E19.1), snoozed or open, waits while one of its messages lies
    /// in any folder but the archive, the trash or spam (a folder without a role and the sent folder
    /// count): "Follow up" goes on any mail, and the main case of "only if no reply" is a sent mail,
    /// which is never in the inbox folder – judged by it, the follow-up would end unseen at the next
    /// catch-up. Open, it is one without the condition a reply opened (review of #315, decisions 6
    /// and 7): reading the reply must not end it, it still comes back at its date. Archived, thrown
    /// away or marked as spam, the person dealt with it, and it ends as decision 6 says.
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
                // Where a follow-up ends (task E19.1): mail there was dealt with.
                let dealt_with: Vec<String> = store
                    .folders()?
                    .into_iter()
                    .filter(|folder| matches!(folder.role, Some(Role::Archive | Role::Trash | Role::Junk)))
                    .map(|folder| folder.id)
                    .collect();
                let mut settled = Vec::new();
                for entry in entries {
                    let mut waiting = false;
                    for message in &entry.messages {
                        // With a follow-up it waits anywhere its mail was not dealt with; put off by
                        // the person (task E9.1) as long as its mail is in the inbox folder, read or
                        // not; else while it is unread there.
                        waiting |= store.summary(message)?.is_some_and(|summary| match entry.snoozed {
                            _ if entry.follow_up => !dealt_with.contains(&summary.folder),
                            true => summary.folder == inbox.id,
                            false => summary.folder == inbox.id && !summary.flags.contains(&Flag::Seen),
                        });
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
        let summary = &message.summary;
        let old = since.is_some_and(|since| summary.date < since);
        if summary.folder != folder || summary.flags.contains(&Flag::Seen) || old {
            continue;
        }
        mails.push(as_new_mail(store, account, message)?);
    }
    mails.sort_by_key(|mail| mail.date);
    Ok(mails)
}

/// The message `id` of `account` as [`Inbox::file_mail`] files it, wherever it is and read or
/// not; `None` when the store does not hold it.
fn mail_to_file(store: &MailStore, account: &str, id: &str) -> Result<Option<NewMail>, MailStoreError> {
    match store.message(id)? {
        Some(message) => Ok(Some(as_new_mail(store, account, message)?)),
        None => Ok(None),
    }
}

/// `message` of `account`, read from `store`, as [`Inbox::file_mail`] files it.
fn as_new_mail(store: &MailStore, account: &str, message: torro_mail::Message) -> Result<NewMail, MailStoreError> {
    let summary = message.summary;
    // Where it lies decides one category: mail the server put in its junk folder is spam (E12.2).
    let junk = store.folder_by_role(Role::Junk)?.is_some_and(|folder| folder.id == summary.folder);
    // Only a mail with `X-GitHub-Reason` can be a GitHub mail: the others need no lookup.
    let github = match &message.traits.github {
        Some(_) => {
            let rfc = store.resolve_message(&summary.id)?.and_then(|found| found.rfc_message_id);
            github_thread(summary.from.as_ref(), &message.traits, rfc.as_deref())
        }
        None => None,
    };
    Ok(NewMail {
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
        invite: summary.invite.is_some(),
        auto_submitted: message.traits.auto_submitted.clone(),
        junk,
        // A filter's category is put on by the feed, which ran the filters (issue #339).
        category: None,
    })
}

/// Wakes the follow-ups "only if no reply" due at `now` (review of #315), after a look in each
/// account's store for a reply since the follow-up was set (not one's own, not a draft,
/// [`MailStore::replies_after`]): with one the follow-up is called off ([`Inbox::reply_came`]) – the
/// reply may be dated before the time and have reached the store only after it, or the feed missed
/// its change –, without one the entry wakes ([`Inbox::wake_follow_up`]). An account whose store
/// cannot be read wakes its entries all the same: a follow-up must not wait for ever. The feed calls
/// this every [`RETIRE_EVERY`] and on a catch-up, so a due one comes back within a minute.
pub(crate) async fn wake_follow_ups(inbox: &Inbox, mail: &MailService, now: DateTime<Utc>) {
    let due = match inbox.due_follow_ups(now) {
        Ok(due) => due,
        Err(err) => {
            eprintln!("torroworkplace: inbox: cannot read the follow-ups that are due: {err}");
            return;
        }
    };
    for follow_up in due {
        let DueFollowUp { id, account, thread, since } = follow_up;
        let replied = match mail.store(&account).await {
            Ok(store) => {
                let (lookup, own) = (thread.clone(), own_address(&account));
                match store.run(move |store| store.replies_after(&lookup, since, &own)).await {
                    Ok(count) => count > 0,
                    Err(err) => {
                        eprintln!("torroworkplace: inbox: cannot look for replies in {account}: {err}");
                        false
                    }
                }
            }
            Err(_) => false,
        };
        let done = match replied {
            true => inbox.reply_came(&account, &thread, now, now).map(drop),
            false => inbox.wake_follow_up(&id, now).map(drop),
        };
        if let Err(err) = done {
            eprintln!("torroworkplace: inbox: cannot wake a follow-up of {account}: {err}");
        }
    }
}

/// The messages of `ids` the store holds, in one query (review of #315: a change may bring many),
/// in no particular order; ids it does not hold are left out.
fn summaries(store: &MailStore, ids: &[String]) -> Result<Vec<torro_mail::MessageSummary>, MailStoreError> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let query = SearchQuery { only: Some(ids.to_vec()), ..SearchQuery::default() };
    store.search(&query, u32::try_from(ids.len()).unwrap_or(u32::MAX), 0)
}

/// The address of the mail account `account` (`mail:<address>`): its own mail is no reply.
pub(crate) fn own_address(account: &str) -> String {
    account.strip_prefix(torro_mail::ACCOUNT_PREFIX).unwrap_or(account).to_string()
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
        start_with(Arc::clone(&inbox), Arc::clone(&mail), Some(accounts_file(dir)), None);
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
            categories: Vec::new(),
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
        start_with(Arc::clone(&inbox), Arc::clone(&mail), Some(accounts_file(dir.path())), None);
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
            snoozed: None,
            categories: Vec::new(),
            pile: None,
        };
        let Response::MailMessages { messages, .. } =
            crate::mail::read::answer(&mail, &inbox, Reach::Suite, search).await
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

    /// Task E19.1, decision 6: a reply calls off a follow-up "only if no reply" in whatever folder
    /// it lands, read or not – the entry is open, not back, and does not come back at the date –;
    /// mail of one's own does not, and a catch-up finds what the feed missed.
    #[tokio::test]
    async fn a_reply_anywhere_calls_off_a_follow_up_unless_reply() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        for subject in ["Offer", "Plan", "Trip"] {
            server.add("INBOX", &bernd(subject, 5), &["\\Seen"]);
        }
        let reply = |subject: &str, from: &str, to: &str| letter(subject, from, 1, "", Some(to));
        server.add("Sent", &reply("Re Offer mine", "Anna <anna@example.org>", "Offer@example.org"), &["\\Seen"]);
        server.add("Archive", &reply("Re Offer", "Bernd <bernd@example.com>", "Offer@example.org"), &[]);
        server.add("INBOX", &reply("Re Plan", "Bernd <bernd@example.com>", "Plan@example.org"), &["\\Seen"]);
        server.add("Archive", &reply("Re Trip", "Bernd <bernd@example.com>", "Trip@example.org"), &[]);
        let secrets = anna(dir.path(), &server, Notify::All);
        let inbox = Arc::new(inbox_in(dir.path()));
        let mail = Arc::new(service_in(dir.path(), &secrets));
        mail.reload().await;
        mail.sync_all(Reach::Suite).await.expect("synced");
        let store = mail.store(ANNA).await.expect("the store");
        let all = store.run(|store| store.search(&SearchQuery::default(), 50, 0)).await.expect("read");
        let of = |subject: &str| all.iter().find(|m| m.subject == subject).cloned().expect("synced");
        assert_eq!(of("Re Offer").thread_id, of("Offer").thread_id, "one conversation");

        // Followed up four days ago, until in an hour.
        let (since, until) = (Utc::now() - TimeDelta::days(4), Utc::now() + TimeDelta::hours(1));
        let mut followed = BTreeMap::new();
        for subject in ["Offer", "Plan", "Trip"] {
            let found = mail_to_snooze(&mail, Reach::Suite, &of(subject).id).await.expect("the mail");
            let entry = inbox.file_mail(&found, Notify::All, since).expect("filed").expect("an entry");
            let request =
                Request::SnoozeItem { id: entry.clone(), until: Some(until), unless_reply: true, keep_open: false };
            assert!(matches!(inbox.handle(request, None, since), Response::Item { .. }));
            followed.insert(subject, entry);
        }
        let state = |subject: &str, now| {
            let request = Request::GetItem { id: followed[subject].clone() };
            match inbox.handle(request, None, now) {
                Response::Item { item } => (item.state, item.follow_up.is_some()),
                other => panic!("the entry, got {other:?}"),
            }
        };
        let change = |message: &torro_mail::MessageSummary, role| MailChange {
            account: ANNA.into(),
            folder: message.folder.clone(),
            role,
            new: vec![message.id.clone()],
            flags: Vec::new(),
            removed: Vec::new(),
            initial: false,
        };
        let feed = Feed { inbox: Arc::clone(&inbox), mail: Arc::clone(&mail), accounts_file: None, decide: None };

        feed.take(change(&of("Re Offer mine"), Some(Role::Sent)), Utc::now()).await;
        assert_eq!(state("Offer", Utc::now()), (State::Snoozed { until }, true), "one's own mail is no reply");
        feed.take(change(&of("Re Offer"), None), Utc::now()).await;
        assert_eq!(state("Offer", Utc::now()), (State::Seen, false), "a reply in the archive");
        feed.take(change(&of("Re Plan"), Some(Role::Inbox)), Utc::now()).await;
        assert_eq!(state("Plan", Utc::now()), (State::Seen, false), "a reply read already");
        let later = inbox.mail_later(&[ANNA.to_string()], Utc::now()).expect("read");
        assert!(later.back_of(ANNA).is_empty(), "nothing is back");
        assert_eq!(state("Trip", Utc::now()), (State::Snoozed { until }, true), "its reply was missed");

        feed.catch_up(Utc::now()).await;
        assert!(inbox.awaiting_reply(ANNA).expect("read").is_empty(), "the catch-up found the reply");
        let at_the_date = until + TimeDelta::minutes(1);
        for subject in ["Offer", "Plan", "Trip"] {
            let (state, follow_up) = state(subject, at_the_date);
            assert!(!follow_up && state != State::New, "{subject} does not come back: {state:?}");
        }
    }

    /// Task E19.2, decision 5: a mail of one's own in the conversation – in the sent folder, by any
    /// program – takes the mail off "Reply later", not off "Set aside"; a reply of someone else does
    /// not; a catch-up finds what the feed missed.
    #[tokio::test]
    async fn a_mail_of_ones_own_takes_the_conversation_off_reply_later() {
        use torro_mail::MailPile;
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        for subject in ["Offer", "Plan", "Trip", "Boarding"] {
            server.add("INBOX", &bernd(subject, 5), &["\\Seen"]);
        }
        let reply = |subject: &str, from: &str, to: &str| letter(subject, from, 1, "", Some(to));
        let own = "Anna <anna@example.org>";
        server.add("Sent", &reply("Re Offer mine", own, "Offer@example.org"), &["\\Seen"]);
        server.add("Archive", &reply("Re Plan", "Bernd <bernd@example.com>", "Plan@example.org"), &[]);
        server.add("Sent", &reply("Re Boarding mine", own, "Boarding@example.org"), &["\\Seen"]);
        server.add("Sent", &reply("Re Trip mine", own, "Trip@example.org"), &["\\Seen"]);
        let secrets = anna(dir.path(), &server, Notify::All);
        let inbox = Arc::new(inbox_in(dir.path()));
        let mail = Arc::new(service_in(dir.path(), &secrets));
        mail.reload().await;
        mail.sync_all(Reach::Suite).await.expect("synced");
        let store = mail.store(ANNA).await.expect("the store");
        let all = store.run(|store| store.search(&SearchQuery::default(), 50, 0)).await.expect("read");
        let of = |subject: &str| all.iter().find(|m| m.subject == subject).cloned().expect("synced");
        assert_eq!(of("Re Offer mine").thread_id, of("Offer").thread_id, "one conversation");

        // Put there four days ago, before every answer.
        let put = Utc::now() - TimeDelta::days(4);
        let pair = |subject: &str| (ANNA.to_string(), of(subject).id);
        let replying = [pair("Offer"), pair("Plan"), pair("Trip")];
        inbox.set_mail_pile(&replying, Some(MailPile::ReplyLater), put).expect("put");
        inbox.set_mail_pile(&[pair("Boarding")], Some(MailPile::SetAside), put).expect("put");
        let pile_of =
            |subject: &str| inbox.mail_piles(&[ANNA.to_string()]).expect("read").pile_of(ANNA, &of(subject).id);
        let change = |message: &torro_mail::MessageSummary, role| MailChange {
            account: ANNA.into(),
            folder: message.folder.clone(),
            role,
            new: vec![message.id.clone()],
            flags: Vec::new(),
            removed: Vec::new(),
            initial: false,
        };
        let feed = Feed { inbox: Arc::clone(&inbox), mail: Arc::clone(&mail), accounts_file: None, decide: None };

        feed.take(change(&of("Re Offer mine"), Some(Role::Sent)), Utc::now()).await;
        assert_eq!(pile_of("Offer"), None, "answered");
        feed.take(change(&of("Re Plan"), None), Utc::now()).await;
        assert_eq!(pile_of("Plan"), Some(MailPile::ReplyLater), "someone else's reply");
        feed.take(change(&of("Re Boarding mine"), Some(Role::Sent)), Utc::now()).await;
        assert_eq!(pile_of("Boarding"), Some(MailPile::SetAside), "Set aside stays");
        assert_eq!(pile_of("Trip"), Some(MailPile::ReplyLater), "its answer was missed");

        feed.catch_up(Utc::now()).await;
        assert_eq!(pile_of("Trip"), None, "the catch-up found the answer");
        assert_eq!(pile_of("Plan"), Some(MailPile::ReplyLater));
        assert_eq!(pile_of("Boarding"), Some(MailPile::SetAside));
        // Put on Reply later after the answer: it stays, a catch-up too.
        inbox.set_mail_pile(&[pair("Trip")], Some(MailPile::ReplyLater), Utc::now()).expect("put again");
        feed.take(change(&of("Re Trip mine"), Some(Role::Sent)), Utc::now()).await;
        feed.catch_up(Utc::now()).await;
        assert_eq!(pile_of("Trip"), Some(MailPile::ReplyLater), "answered before it was put there");
    }

    /// Review of #315: a pile forgets mail that is gone from the store – when its folder's change
    /// says it was removed, and on a catch-up –, but keeps mail the core moved (its id stays).
    #[tokio::test]
    async fn piles_forget_mail_that_is_gone() {
        use torro_mail::MailPile;
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        for subject in ["Deleted", "Missed", "Moved"] {
            server.add("INBOX", &bernd(subject, 2), &["\\Seen"]);
        }
        let secrets = anna(dir.path(), &server, Notify::All);
        let inbox = Arc::new(inbox_in(dir.path()));
        let mail = Arc::new(service_in(dir.path(), &secrets));
        mail.reload().await;
        mail.sync_all(Reach::Suite).await.expect("synced");
        let store = mail.store(ANNA).await.expect("the store");
        let all = store.run(|store| store.search(&SearchQuery::default(), 50, 0)).await.expect("read");
        let of = |subject: &str| all.iter().find(|m| m.subject == subject).cloned().expect("synced");
        let pairs: Vec<(String, String)> =
            ["Deleted", "Missed", "Moved"].iter().map(|subject| (ANNA.to_string(), of(subject).id)).collect();
        inbox.set_mail_pile(&pairs, Some(MailPile::SetAside), Utc::now()).expect("put");
        let piled = || inbox.mail_piles(&[ANNA.to_string()]).expect("read").all_of(ANNA);
        // Gone from the store: deleted, and one whose change the feed missed; one moved by the core.
        let archive = store.run(|store| store.folder_by_role(Role::Archive)).await.expect("read").expect("an archive");
        let (deleted, missed, moved) = (of("Deleted").id, of("Missed").id, of("Moved").id);
        let gone = [deleted.clone(), missed.clone()];
        let kept = moved.clone();
        store
            .run(move |store| {
                for id in &gone {
                    store.delete_message(id)?;
                }
                store.relocate_message(&kept, &archive.id, 14, 1)?;
                Ok(())
            })
            .await
            .expect("changed");
        let feed = Feed { inbox: Arc::clone(&inbox), mail: Arc::clone(&mail), accounts_file: None, decide: None };
        let removed = MailChange {
            account: ANNA.into(),
            folder: of("Deleted").folder,
            role: Some(Role::Inbox),
            new: Vec::new(),
            flags: Vec::new(),
            removed: vec![deleted.clone(), moved.clone()],
            initial: false,
        };
        feed.take(removed, Utc::now()).await;
        let mut left = piled();
        left.sort();
        let mut expected = vec![missed.clone(), moved.clone()];
        expected.sort();
        assert_eq!(left, expected, "the deleted one is forgotten, the moved one stays");
        feed.catch_up(Utc::now()).await;
        assert_eq!(piled(), [moved], "the catch-up forgets what the feed missed");
    }

    /// Review of #315: one change with several new mails reads them at once and does what each one
    /// asks – an answer of one's own takes mail off "Reply later", a reply ends a follow-up "only if
    /// no reply", other mail does nothing.
    #[tokio::test]
    async fn one_change_with_several_mails_notes_each() {
        use torro_mail::MailPile;
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        for subject in ["Offer", "Plan", "Other"] {
            server.add("INBOX", &bernd(subject, 5), &["\\Seen"]);
        }
        let reply = |subject: &str, from: &str, to: &str| letter(subject, from, 1, "", Some(to));
        server.add("INBOX", &reply("Re Offer mine", "Anna <anna@example.org>", "Offer@example.org"), &["\\Seen"]);
        server.add("INBOX", &reply("Re Plan", "Bernd <bernd@example.com>", "Plan@example.org"), &["\\Seen"]);
        let secrets = anna(dir.path(), &server, Notify::All);
        let inbox = Arc::new(inbox_in(dir.path()));
        let mail = Arc::new(service_in(dir.path(), &secrets));
        mail.reload().await;
        mail.sync_all(Reach::Suite).await.expect("synced");
        let store = mail.store(ANNA).await.expect("the store");
        let all = store.run(|store| store.search(&SearchQuery::default(), 50, 0)).await.expect("read");
        let of = |subject: &str| all.iter().find(|m| m.subject == subject).cloned().expect("synced");
        let since = Utc::now() - TimeDelta::days(4);
        inbox.set_mail_pile(&[(ANNA.to_string(), of("Offer").id)], Some(MailPile::ReplyLater), since).expect("put");
        let found = mail_to_snooze(&mail, Reach::Suite, &of("Plan").id).await.expect("the mail");
        let plan = inbox.file_mail(&found, Notify::All, since).expect("filed").expect("an entry");
        let until = Utc::now() + TimeDelta::hours(1);
        let request = Request::SnoozeItem { id: plan, until: Some(until), unless_reply: true, keep_open: false };
        assert!(matches!(inbox.handle(request, None, since), Response::Item { .. }));
        let feed = Feed { inbox: Arc::clone(&inbox), mail: Arc::clone(&mail), accounts_file: None, decide: None };
        let change = MailChange {
            account: ANNA.into(),
            folder: of("Offer").folder,
            role: Some(Role::Inbox),
            new: ["Re Offer mine", "Re Plan", "Other", "m:unknown"]
                .iter()
                .map(|s| if s.starts_with("m:") { s.to_string() } else { of(s).id })
                .collect(),
            flags: Vec::new(),
            removed: Vec::new(),
            initial: false,
        };
        feed.take(change, Utc::now()).await;
        assert!(inbox.mail_piles(&[ANNA.to_string()]).expect("read").all_of(ANNA).is_empty(), "answered");
        assert!(inbox.awaiting_reply(ANNA).expect("read").is_empty(), "the reply ended the follow-up");
    }

    /// Review of #315: a reply dated before the follow-up's time that the feed had not got to yet
    /// (in the store, never seen as a change) keeps a follow-up "only if no reply" from coming back;
    /// one without a reply comes back at its time. Until the feed has looked, a due one waits.
    #[tokio::test]
    async fn a_follow_up_checks_for_a_reply_before_it_wakes() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        for subject in ["Offer", "Plan"] {
            server.add("INBOX", &bernd(subject, 5), &["\\Seen"]);
        }
        let late = letter("Re Offer", "Bernd <bernd@example.com>", 2, "", Some("Offer@example.org"));
        server.add("Archive", &late, &[]);
        let secrets = anna(dir.path(), &server, Notify::All);
        let inbox = Arc::new(inbox_in(dir.path()));
        let mail = Arc::new(service_in(dir.path(), &secrets));
        mail.reload().await;
        mail.sync_all(Reach::Suite).await.expect("synced");
        let store = mail.store(ANNA).await.expect("the store");
        let all = store.run(|store| store.search(&SearchQuery::default(), 50, 0)).await.expect("read");
        let of = |subject: &str| all.iter().find(|m| m.subject == subject).cloned().expect("synced");
        // Set four days ago, due an hour ago; the reply is two days old.
        let (since, at) = (Utc::now() - TimeDelta::days(4), Utc::now() - TimeDelta::hours(1));
        let mut entries = BTreeMap::new();
        for subject in ["Offer", "Plan"] {
            let found = mail_to_snooze(&mail, Reach::Suite, &of(subject).id).await.expect("the mail");
            let entry = inbox.file_mail(&found, Notify::All, since).expect("filed").expect("an entry");
            let request =
                Request::SnoozeItem { id: entry.clone(), until: Some(at), unless_reply: true, keep_open: false };
            assert!(matches!(inbox.handle(request, None, since), Response::Item { .. }));
            entries.insert(subject, entry);
        }
        let entry =
            |subject: &str| match inbox.handle(Request::GetItem { id: entries[subject].clone() }, None, Utc::now()) {
                Response::Item { item } => item,
                other => panic!("the entry, got {other:?}"),
            };
        assert_eq!(entry("Offer").state, State::Snoozed { until: at }, "due, it waits for the feed's look");

        wake_follow_ups(&inbox, &mail, Utc::now()).await;
        let offer = entry("Offer");
        assert_eq!((offer.state, offer.follow_up), (State::Seen, None), "the late reply counts");
        let plan = entry("Plan");
        assert_eq!((plan.state, plan.updated), (State::New, at), "no reply: back at its time");
        let later = inbox.mail_later(&[ANNA.to_string()], Utc::now()).expect("read");
        assert_eq!(later.back_at(ANNA, &of("Plan").id), Some(at));
        assert_eq!(later.back_at(ANNA, &of("Offer").id), None, "not back");
    }

    /// Review of #315, decision 6: a mail of one's own filed into the conversation (Bcc to oneself,
    /// a list's echo) opens it but is no reply – the follow-up "only if no reply" stands until a
    /// reply of someone else comes.
    #[tokio::test]
    async fn own_mail_keeps_a_follow_up_unless_reply() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &bernd("Offer", 5), &["\\Seen"]);
        let reply = |subject: &str, from: &str| letter(subject, from, 1, "", Some("Offer@example.org"));
        server.add("INBOX", &reply("Re Offer bcc", "Anna <anna@example.org>"), &[]);
        server.add("Archive", &reply("Re Offer", "Bernd <bernd@example.com>"), &[]);
        let secrets = anna(dir.path(), &server, Notify::All);
        let inbox = Arc::new(inbox_in(dir.path()));
        let mail = Arc::new(service_in(dir.path(), &secrets));
        mail.reload().await;
        mail.sync_all(Reach::Suite).await.expect("synced");
        let store = mail.store(ANNA).await.expect("the store");
        let all = store.run(|store| store.search(&SearchQuery::default(), 50, 0)).await.expect("read");
        let of = |subject: &str| all.iter().find(|m| m.subject == subject).cloned().expect("synced");
        let (since, until) = (Utc::now() - TimeDelta::days(4), Utc::now() + TimeDelta::hours(1));
        let found = mail_to_snooze(&mail, Reach::Suite, &of("Offer").id).await.expect("the mail");
        let entry = inbox.file_mail(&found, Notify::All, since).expect("filed").expect("an entry");
        let request =
            Request::SnoozeItem { id: entry.clone(), until: Some(until), unless_reply: true, keep_open: false };
        assert!(matches!(inbox.handle(request, None, since), Response::Item { .. }));
        let held = || match inbox.handle(Request::GetItem { id: entry.clone() }, None, Utc::now()) {
            Response::Item { item } => (item.state, item.follow_up.map(|follow_up| follow_up.unless_reply)),
            other => panic!("the entry, got {other:?}"),
        };
        let change = |message: &torro_mail::MessageSummary, role| MailChange {
            account: ANNA.into(),
            folder: message.folder.clone(),
            role,
            new: vec![message.id.clone()],
            flags: Vec::new(),
            removed: Vec::new(),
            initial: false,
        };
        let accounts_file = Some(accounts_file(dir.path()));
        let feed = Feed { inbox: Arc::clone(&inbox), mail: Arc::clone(&mail), accounts_file, decide: None };

        feed.take(change(&of("Re Offer bcc"), Some(Role::Inbox)), Utc::now()).await;
        assert_eq!(inbox.messages_of(&entry).expect("read").len(), 2, "filed into the conversation");
        assert_eq!(held(), (State::New, Some(true)), "opened, the condition stands");
        feed.take(change(&of("Re Offer"), None), Utc::now()).await;
        assert_eq!(held(), (State::New, None), "a reply of someone else ends it");
    }

    /// Review of #315: a draft in the conversation is no answer of one's own – saved or a sending
    /// that failed – and no reply either, also one from another address (an alias); the mail
    /// stays on "Reply later" and the follow-up stands, a catch-up too. A sent mail takes it off.
    #[tokio::test]
    async fn a_draft_is_no_answer_and_no_reply() {
        use torro_mail::MailPile;
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        for subject in ["Offer", "Plan", "Trip"] {
            server.add("INBOX", &bernd(subject, 5), &["\\Seen"]);
        }
        let reply = |subject: &str, from: &str, to: &str| letter(subject, from, 1, "", Some(to));
        let own = "Anna <anna@example.org>";
        server.add("Drafts", &reply("Re Offer draft", own, "Offer@example.org"), &["\\Seen", "\\Draft"]);
        server.add("Drafts", &reply("Re Trip draft", own, "Trip@example.org"), &["\\Seen", "\\Draft"]);
        let alias = "Anna <anna@alias.example>";
        server.add("Drafts", &reply("Re Plan draft", alias, "Plan@example.org"), &["\\Seen", "\\Draft"]);
        server.add("Sent", &reply("Re Offer mine", own, "Offer@example.org"), &["\\Seen"]);
        let secrets = anna(dir.path(), &server, Notify::All);
        let inbox = Arc::new(inbox_in(dir.path()));
        let mail = Arc::new(service_in(dir.path(), &secrets));
        mail.reload().await;
        mail.sync_all(Reach::Suite).await.expect("synced");
        let store = mail.store(ANNA).await.expect("the store");
        let all = store.run(|store| store.search(&SearchQuery::default(), 50, 0)).await.expect("read");
        let of = |subject: &str| all.iter().find(|m| m.subject == subject).cloned().expect("synced");
        assert_eq!(of("Re Offer draft").thread_id, of("Offer").thread_id, "one conversation");

        let since = Utc::now() - TimeDelta::days(4);
        let pair = |subject: &str| (ANNA.to_string(), of(subject).id);
        inbox.set_mail_pile(&[pair("Offer"), pair("Trip")], Some(MailPile::ReplyLater), since).expect("put");
        let found = mail_to_snooze(&mail, Reach::Suite, &of("Plan").id).await.expect("the mail");
        let plan = inbox.file_mail(&found, Notify::All, since).expect("filed").expect("an entry");
        let until = Utc::now() + TimeDelta::hours(1);
        let request =
            Request::SnoozeItem { id: plan.clone(), until: Some(until), unless_reply: true, keep_open: false };
        assert!(matches!(inbox.handle(request, None, since), Response::Item { .. }));
        let pile_of =
            |subject: &str| inbox.mail_piles(&[ANNA.to_string()]).expect("read").pile_of(ANNA, &of(subject).id);
        let change = |message: &torro_mail::MessageSummary, role| MailChange {
            account: ANNA.into(),
            folder: message.folder.clone(),
            role,
            new: vec![message.id.clone()],
            flags: Vec::new(),
            removed: Vec::new(),
            initial: false,
        };
        let feed = Feed { inbox: Arc::clone(&inbox), mail: Arc::clone(&mail), accounts_file: None, decide: None };

        for draft in ["Re Offer draft", "Re Plan draft"] {
            feed.take(change(&of(draft), Some(Role::Drafts)), Utc::now()).await;
        }
        assert_eq!(pile_of("Offer"), Some(MailPile::ReplyLater), "a draft is no answer");
        assert_eq!(inbox.awaiting_reply(ANNA).expect("read").len(), 1, "a draft is no reply");
        feed.take(change(&of("Re Offer mine"), Some(Role::Sent)), Utc::now()).await;
        assert_eq!(pile_of("Offer"), None, "the sent mail is");
        feed.catch_up(Utc::now()).await;
        assert_eq!(pile_of("Trip"), Some(MailPile::ReplyLater), "nor in a catch-up");
        assert_eq!(inbox.awaiting_reply(ANNA).expect("read").len(), 1, "nor a reply in a catch-up");
    }

    /// Review of #315, decisions 6 and 7: a follow-up without the condition stands after the reply
    /// was read – the entry stays open while its mail is not archived – and brings the conversation
    /// back at the date; archived before, it ends.
    #[tokio::test]
    async fn a_follow_up_without_condition_outlasts_reading_the_reply() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let reply = |subject: &str, to: &str| letter(subject, "Bernd <bernd@example.com>", 1, "", Some(to));
        for subject in ["Offer", "Plan"] {
            server.add("INBOX", &bernd(subject, 5), &["\\Seen"]);
            server.add("INBOX", &reply(&format!("Re {subject}"), &format!("{subject}@example.org")), &[]);
        }
        let secrets = anna(dir.path(), &server, Notify::All);
        let inbox = Arc::new(inbox_in(dir.path()));
        let mail = Arc::new(service_in(dir.path(), &secrets));
        mail.reload().await;
        mail.sync_all(Reach::Suite).await.expect("synced");
        let store = mail.store(ANNA).await.expect("the store");
        let all = store.run(|store| store.search(&SearchQuery::default(), 50, 0)).await.expect("read");
        let of = |subject: &str| all.iter().find(|m| m.subject == subject).cloned().expect("synced");
        let (since, until) = (Utc::now() - TimeDelta::days(4), Utc::now() + TimeDelta::hours(1));
        let mut entries = BTreeMap::new();
        for subject in ["Offer", "Plan"] {
            let found = mail_to_snooze(&mail, Reach::Suite, &of(subject).id).await.expect("the mail");
            let entry = inbox.file_mail(&found, Notify::All, since).expect("filed").expect("an entry");
            let request =
                Request::SnoozeItem { id: entry.clone(), until: Some(until), unless_reply: false, keep_open: false };
            assert!(matches!(inbox.handle(request, None, since), Response::Item { .. }));
            // The reply comes and opens it, as the feed files it.
            let reply = mail_to_snooze(&mail, Reach::Suite, &of(&format!("Re {subject}")).id).await.expect("reply");
            assert_eq!(inbox.file_mail(&reply, Notify::All, Utc::now()).expect("filed"), Some(entry.clone()));
            entries.insert(subject, entry);
        }
        let entry =
            |subject: &str, now| match inbox.handle(Request::GetItem { id: entries[subject].clone() }, None, now) {
                Response::Item { item } => item,
                other => panic!("the entry, got {other:?}"),
            };
        assert_eq!(entry("Offer", Utc::now()).state, State::New, "the reply opened it");
        let feed = Feed { inbox: Arc::clone(&inbox), mail: Arc::clone(&mail), accounts_file: None, decide: None };

        // Both replies read: the follow-ups stand.
        let read: Vec<String> = ["Re Offer", "Re Plan"].iter().map(|subject| of(subject).id).collect();
        let marked = read.clone();
        store
            .run(move |store| {
                for id in &marked {
                    store.set_flags(id, &[Flag::Seen])?;
                }
                Ok(())
            })
            .await
            .expect("read");
        let flagged = MailChange {
            account: ANNA.into(),
            folder: of("Re Offer").folder,
            role: Some(Role::Inbox),
            new: Vec::new(),
            flags: read,
            removed: Vec::new(),
            initial: false,
        };
        feed.take(flagged, Utc::now()).await;
        assert!(entry("Offer", Utc::now()).state.is_open(), "read, the follow-up stands");
        assert!(entry("Plan", Utc::now()).state.is_open());

        // "Plan" archived: done, and it does not come back.
        let archive = store.run(|store| store.folder_by_role(Role::Archive)).await.expect("read").expect("an archive");
        let ids = [of("Plan").id, of("Re Plan").id];
        store
            .run(move |store| {
                for (uid, id) in (1..).zip(ids.iter()) {
                    store.relocate_message(id, &archive.id, 14, uid)?;
                }
                Ok(())
            })
            .await
            .expect("moved");
        feed.settle(&store, ANNA, None, Utc::now()).await;
        assert!(matches!(entry("Plan", Utc::now()).state, State::Done { .. }), "archived");

        let at_the_date = until + TimeDelta::minutes(1);
        let back = entry("Offer", at_the_date);
        assert_eq!((back.state, back.updated, back.follow_up), (State::New, until, None), "back at the date");
        let later = inbox.mail_later(&[ANNA.to_string()], at_the_date).expect("read");
        assert_eq!(later.back_at(ANNA, &of("Offer").id), Some(until));
        assert!(matches!(entry("Plan", at_the_date).state, State::Done { .. }), "archived does not come back");
    }

    /// Task E19.1: a follow-up waits while its mail lies anywhere but in the archive, the trash or
    /// spam – a sent mail followed up by its id is never in the inbox folder –, and ends when the
    /// mail is archived. Mail put off without a follow-up (before E19.1) still waits only in the
    /// inbox folder.
    #[tokio::test]
    async fn a_follow_up_waits_outside_the_inbox_folder_until_archived() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("Sent", &letter("Question", "Anna <anna@example.org>", 2, "", None), &["\\Seen"]);
        server.add("INBOX", &bernd("Put off before", 2), &["\\Seen"]);
        let secrets = anna(dir.path(), &server, Notify::All);
        let inbox = Arc::new(inbox_in(dir.path()));
        let mail = Arc::new(service_in(dir.path(), &secrets));
        mail.reload().await;
        mail.sync_all(Reach::Suite).await.expect("synced");
        let store = mail.store(ANNA).await.expect("the store");
        let all = store.run(|store| store.search(&SearchQuery::default(), 50, 0)).await.expect("read");
        let of = |subject: &str| all.iter().find(|m| m.subject == subject).cloned().expect("synced");
        let until = Utc::now() + TimeDelta::hours(1);
        let mut entries = BTreeMap::new();
        for subject in ["Question", "Put off before"] {
            let found = mail_to_snooze(&mail, Reach::Suite, &of(subject).id).await.expect("the mail");
            let entry = inbox.file_mail(&found, Notify::All, Utc::now()).expect("filed").expect("an entry");
            let request =
                Request::SnoozeItem { id: entry.clone(), until: Some(until), unless_reply: true, keep_open: false };
            assert!(matches!(inbox.handle(request, None, Utc::now()), Response::Item { .. }));
            entries.insert(subject, entry);
        }
        crate::inbox::tests::without_follow_up(&inbox, &entries["Put off before"]);
        let state =
            |subject: &str| match inbox.handle(Request::GetItem { id: entries[subject].clone() }, None, Utc::now()) {
                Response::Item { item } => item.state,
                other => panic!("the entry, got {other:?}"),
            };
        let feed = Feed { inbox: Arc::clone(&inbox), mail: Arc::clone(&mail), accounts_file: None, decide: None };

        // 1. In the sent folder it waits, after a catch-up and after a flag changed on it.
        feed.catch_up(Utc::now()).await;
        assert_eq!(state("Question"), State::Snoozed { until }, "a catch-up keeps it");
        let flagged = MailChange {
            account: ANNA.into(),
            folder: of("Question").folder,
            role: Some(Role::Sent),
            new: Vec::new(),
            flags: vec![of("Question").id],
            removed: Vec::new(),
            initial: false,
        };
        feed.take(flagged, Utc::now()).await;
        assert_eq!(state("Question"), State::Snoozed { until }, "a flag changed on it keeps it");
        assert_eq!(state("Put off before"), State::Snoozed { until }, "still in the inbox folder");

        // 2. and 3. Archived, both end: the follow-up and the mail put off before.
        let archive = store.run(|store| store.folder_by_role(Role::Archive)).await.expect("read").expect("an archive");
        let archive_id = archive.id.clone();
        let ids = [of("Question").id, of("Put off before").id];
        store
            .run(move |store| {
                for (uid, id) in (1..).zip(ids.iter()) {
                    store.relocate_message(id, &archive_id, 14, uid)?;
                }
                Ok(())
            })
            .await
            .expect("moved");
        feed.settle(&store, ANNA, None, Utc::now()).await;
        assert!(matches!(state("Question"), State::Done { .. }), "archived: no follow-up");
        assert!(matches!(state("Put off before"), State::Done { .. }), "out of the inbox folder: done as before");
    }

    /// Task E9.1: a read mail goes to "Later" as the feed would file it, and stays there when the
    /// feed looks at the account's mail again – read mail of an entry not put off is done.
    #[tokio::test]
    async fn read_mail_put_off_stays_in_later() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &bernd("Put off", 0), &["\\Seen"]);
        server.add("INBOX", &bernd("Left as it is", 0), &["\\Seen"]);
        let secrets = anna(dir.path(), &server, Notify::All);
        let (inbox, mail) = fed(dir.path(), &secrets).await;
        eventually("synced", || synced(&mail)).await;
        assert!(entries(&inbox).is_empty(), "read mail is not filed by itself");
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
            snoozed: None,
            categories: Vec::new(),
            pile: None,
        };
        let Response::MailMessages { messages, .. } =
            crate::mail::read::answer(&mail, &inbox, Reach::Suite, search).await
        else {
            panic!("a page of messages")
        };
        let id_of =
            |subject: &str| messages.iter().find(|m| m.subject == subject).map(|m| m.id.clone()).expect("synced");
        let (put_off, left) = (id_of("Put off"), id_of("Left as it is"));

        let mut rights = torro_ipc::api::RightsTemplate::FullAccess.rights();
        rights.mail_accounts = torro_ipc::api::Selection::Only(vec!["mail:bernd@example.com".into()]);
        assert_eq!(mail_to_snooze(&mail, Reach::Client(&rights), &put_off).await, None, "another account only");
        assert_eq!(mail_to_snooze(&mail, Reach::Suite, "m:nope").await, None);
        let mut filed = Vec::new();
        for id in [&put_off, &left] {
            let found = mail_to_snooze(&mail, Reach::Suite, id).await.expect("the mail");
            assert_eq!((found.account.as_str(), found.message.as_str()), (ANNA, id.as_str()));
            filed.push(inbox.file_mail(&found, Notify::All, Utc::now()).expect("filed").expect("an entry"));
        }
        let until = Utc::now() + TimeDelta::hours(1);
        inbox.handle(
            Request::SnoozeItem { id: filed[0].clone(), until: Some(until), unless_reply: false, keep_open: false },
            None,
            Utc::now(),
        );

        let feed = Feed { inbox: Arc::clone(&inbox), mail: Arc::clone(&mail), accounts_file: None, decide: None };
        let store = mail.store(ANNA).await.expect("the store");
        feed.settle(&store, ANNA, None, Utc::now()).await;
        let items = entries(&inbox);
        assert_eq!(entry(&items, "Put off").state, State::Snoozed { until }, "read, but put off");
        assert!(matches!(entry(&items, "Left as it is").state, State::Done { .. }), "read and not put off");
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
                categories: Vec::new(),
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
        let feed = Feed { inbox: Arc::clone(&inbox), mail: Arc::clone(&mail), accounts_file: file, decide: None };
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
        let feed = Feed { inbox: Arc::clone(&inbox), mail: Arc::clone(&mail), accounts_file: file, decide: None };

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

    // ----- mail filters on new mail (issue #339, step 3) -----

    /// Anna's account on `server`, synced once while no feed listened, and a feed to hand it
    /// changes by hand: every test decides itself which mail "arrived".
    async fn quiet(dir: &Path, server: &FakeImap) -> (Arc<Inbox>, Arc<MailService>, Feed) {
        let secrets = anna(dir, server, Notify::All);
        let inbox = Arc::new(inbox_in(dir));
        let mail = Arc::new(service_in(dir, &secrets));
        mail.reload().await;
        mail.sync_all(Reach::Suite).await.expect("synced");
        let file = Some(accounts_file(dir));
        let feed = Feed { inbox: Arc::clone(&inbox), mail: Arc::clone(&mail), accounts_file: file, decide: None };
        (inbox, mail, feed)
    }

    /// A filter made as the person makes it: the search `query`, for `account` (`None`: every one),
    /// doing what `set` says to a plain spec (on, no action yet).
    fn make_filter(
        inbox: &Inbox,
        mail: &MailService,
        query: &str,
        account: Option<&str>,
        set: impl FnOnce(&mut super::super::filters::FilterSpec),
    ) -> torro_mail::MailFilter {
        let mut spec = super::super::filters::FilterSpec {
            query: query.into(),
            account: account.map(str::to_string),
            file: None,
            mark_read: false,
            category: None,
            enabled: None,
        };
        set(&mut spec);
        let catalogue: Vec<torro_mail::Category> =
            inbox.catalogue().expect("the catalogue").into_iter().map(|def| def.category).collect();
        mail.filters().create(spec, &catalogue, Utc::now()).expect("a filter")
    }

    /// Every mail of Anna's store by its subject.
    async fn by_subject(mail: &MailService) -> BTreeMap<String, torro_mail::MessageSummary> {
        let store = mail.store(ANNA).await.expect("the store");
        let all = store.run(|store| store.search(&SearchQuery::default(), 100, 0)).await.expect("read");
        all.into_iter().map(|message| (message.subject.clone(), message)).collect()
    }

    /// The change of the inbox folder that brings `messages` as new mail, after its first sync.
    fn arrived(messages: &[&torro_mail::MessageSummary]) -> MailChange {
        MailChange {
            account: ANNA.into(),
            folder: messages.first().map(|message| message.folder.clone()).unwrap_or_default(),
            role: Some(Role::Inbox),
            new: messages.iter().map(|message| message.id.clone()).collect(),
            flags: Vec::new(),
            removed: Vec::new(),
            initial: false,
        }
    }

    fn titles(inbox: &Inbox) -> Vec<String> {
        let mut titles: Vec<String> = entries(inbox).into_iter().map(|item| item.title).collect();
        titles.sort();
        titles
    }

    fn from(name: &str, subject: &str) -> String {
        letter(subject, &format!("{name} <{}@example.com>", name.to_lowercase()), 0, "", None)
    }

    /// A filter that marks as read: the mail is read on the server and in the store, and gets no
    /// entry; mail no filter finds is filed as before.
    #[tokio::test]
    async fn a_filter_marks_new_mail_read_and_it_gets_no_entry() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let news = server.add("INBOX", &from("News", "Weekly news"), &[]);
        let offer = server.add("INBOX", &bernd("Offer", 0), &[]);
        let (inbox, mail, feed) = quiet(dir.path(), &server).await;
        make_filter(&inbox, &mail, "from:news@example.com", None, |spec| spec.mark_read = true);
        let all = by_subject(&mail).await;

        feed.take(arrived(&[&all["Weekly news"], &all["Offer"]]), Utc::now()).await;
        assert_eq!(titles(&inbox), ["Offer"], "read by the filter: no entry");
        assert!(server.flags("INBOX", news).contains(&"\\Seen".to_string()), "read on the server");
        assert!(server.flags("INBOX", offer).is_empty(), "only what the filter finds");
        assert!(by_subject(&mail).await["Weekly news"].flags.contains(&Flag::Seen), "and in the store");
    }

    /// A filter's category (decision 4): kept with the mail as a rule's, on the entry before the
    /// sorting rules, which still sort the mail no filter finds.
    #[tokio::test]
    async fn a_filters_category_comes_before_the_sorting_rules() {
        use torro_inbox::{SortCondition, SortField};
        use torro_mail::Category;
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &bernd("Bill", 0), &[]);
        server.add("INBOX", &from("Carla", "Hello"), &[]);
        let (inbox, mail, feed) = quiet(dir.path(), &server).await;
        let rule = Request::CreateSortRule {
            category: Category::Newsletter,
            conditions: vec![SortCondition { field: SortField::Domain, value: "example.com".into() }],
            exceptions: Vec::new(),
            position: None,
            enabled: true,
        };
        assert!(matches!(inbox.handle(rule, None, Utc::now()), Response::SortRule { .. }));
        make_filter(&inbox, &mail, "from:bernd", None, |spec| spec.category = Some(Category::Invoice));
        let all = by_subject(&mail).await;

        feed.take(arrived(&[&all["Bill"], &all["Hello"]]), Utc::now()).await;
        let items = entries(&inbox);
        assert_eq!(entry(&items, "Bill").category, Some(MailCategory::by_rule(Category::Invoice)), "the filter's");
        assert_eq!(entry(&items, "Hello").category, Some(MailCategory::by_rule(Category::Newsletter)), "the rule's");
        let kept = by_subject(&mail).await;
        assert_eq!(kept["Bill"].category, Some(MailCategory::by_rule(Category::Invoice)), "kept with the mail");
        assert_eq!(kept["Bill"].folder, all["Bill"].folder, "a category alone moves nothing");
        assert!(!kept["Bill"].flags.contains(&Flag::Seen));
    }

    /// The moves: to the archive, to a folder of the account, to the trash (a move, nothing is
    /// deleted for good). Moved mail gets no entry.
    #[tokio::test]
    async fn a_filter_moves_new_mail_to_the_archive_a_folder_or_the_trash() {
        use torro_mail::FilterFile;
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add_folder("Trash", "\\HasNoChildren \\Trash", 15);
        server.add_folder("Shop", "\\HasNoChildren", 16);
        for (name, subject) in [("Archie", "To archive"), ("Shop", "To the shop folder"), ("Spam", "To the trash")] {
            server.add("INBOX", &from(name, subject), &[]);
        }
        server.add("INBOX", &bernd("Offer", 0), &[]);
        let (inbox, mail, feed) = quiet(dir.path(), &server).await;
        let store = mail.store(ANNA).await.expect("the store");
        let folders = store.run(|store| store.folders()).await.expect("folders");
        let shop = folders.iter().find(|folder| folder.name == "Shop").expect("the shop folder").id.clone();
        make_filter(&inbox, &mail, "from:archie", None, |spec| spec.file = Some(FilterFile::Archive));
        make_filter(&inbox, &mail, "from:shop", Some(ANNA), |spec| spec.file = Some(FilterFile::Folder { id: shop }));
        make_filter(&inbox, &mail, "from:spam", None, |spec| spec.file = Some(FilterFile::Trash));
        let all = by_subject(&mail).await;
        let new: Vec<&torro_mail::MessageSummary> = all.values().collect();

        feed.take(arrived(&new), Utc::now()).await;
        assert_eq!(titles(&inbox), ["Offer"], "moved mail gets no entry");
        let lies_in = |folder: &str, subject: &str| {
            server.messages(folder).iter().any(|(_, _, raw)| raw.contains(&format!("Subject: {subject}\r\n")))
        };
        assert!(lies_in("Archive", "To archive") && lies_in("Shop", "To the shop folder"));
        assert!(lies_in("Trash", "To the trash"), "the trash, not deleted for good");
        assert_eq!(server.messages("INBOX").len(), 1, "only the offer stays");
        let kept = by_subject(&mail).await;
        let role_of =
            |subject: &str| folders.iter().find(|folder| folder.id == kept[subject].folder).and_then(|f| f.role);
        assert_eq!(role_of("To archive"), Some(Role::Archive), "the store keeps the id where it went");
        assert_eq!(role_of("To the trash"), Some(Role::Trash));
    }

    /// Only a category alone leaves an entry; with reading or a move there is none. Several filters
    /// together follow their order (decision 3).
    #[tokio::test]
    async fn only_mail_a_filter_just_sorts_gets_an_entry() {
        use torro_mail::{Category, FilterFile};
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        for (name, subject) in [("Sorted", "Sorted only"), ("Read", "Sorted and read"), ("Moved", "Sorted and moved")] {
            server.add("INBOX", &from(name, subject), &[]);
        }
        let (inbox, mail, feed) = quiet(dir.path(), &server).await;
        let invoice = |spec: &mut super::super::filters::FilterSpec| spec.category = Some(Category::Invoice);
        make_filter(&inbox, &mail, "from:example.com", None, invoice);
        make_filter(&inbox, &mail, "from:read", None, |spec| {
            spec.mark_read = true;
            spec.category = Some(Category::Newsletter);
        });
        make_filter(&inbox, &mail, "from:moved", None, |spec| spec.file = Some(FilterFile::Archive));
        let all = by_subject(&mail).await;
        let new: Vec<&torro_mail::MessageSummary> = all.values().collect();

        feed.take(arrived(&new), Utc::now()).await;
        assert_eq!(titles(&inbox), ["Sorted only"]);
        let items = entries(&inbox);
        assert_eq!(entry(&items, "Sorted only").category, Some(MailCategory::by_rule(Category::Invoice)));
        let kept = by_subject(&mail).await;
        let first = Some(MailCategory::by_rule(Category::Invoice));
        assert_eq!(kept["Sorted and read"].category, first, "the first filter's category");
        assert!(kept["Sorted and read"].flags.contains(&Flag::Seen));
        assert_ne!(kept["Sorted and moved"].folder, all["Sorted and moved"].folder, "moved");
    }

    /// A filter of another account, or one switched off, does nothing.
    #[tokio::test]
    async fn filters_of_another_account_or_switched_off_do_nothing() {
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let uid = server.add("INBOX", &bernd("Offer", 0), &[]);
        let (inbox, mail, feed) = quiet(dir.path(), &server).await;
        make_filter(&inbox, &mail, "from:bernd", Some("mail:bernd@example.com"), |spec| spec.mark_read = true);
        make_filter(&inbox, &mail, "from:bernd", None, |spec| {
            spec.mark_read = true;
            spec.enabled = Some(false);
        });
        let all = by_subject(&mail).await;

        feed.take(arrived(&[&all["Offer"]]), Utc::now()).await;
        assert_eq!(titles(&inbox), ["Offer"]);
        assert!(server.flags("INBOX", uid).is_empty());
    }

    /// A folder's first sync and a catch-up bring mail that was there before: no filter runs on it.
    /// New mail after the first sync is filtered by the running feed.
    #[tokio::test]
    async fn a_first_sync_and_a_catch_up_run_no_filters() {
        use torro_mail::FilterFile;
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let there = server.add("INBOX", &bernd("Offer", 0), &[]);
        let secrets = anna(dir.path(), &server, Notify::All);
        let inbox = Arc::new(inbox_in(dir.path()));
        let mail = Arc::new(service_in(dir.path(), &secrets));
        let archive = |spec: &mut super::super::filters::FilterSpec| spec.file = Some(FilterFile::Archive);
        make_filter(&inbox, &mail, "from:bernd", None, archive);
        start_with(Arc::clone(&inbox), Arc::clone(&mail), Some(accounts_file(dir.path())), None);
        mail.reload().await;

        eventually("the first sync filed", || async { entries(&inbox).len() == 1 }).await;
        eventually("synced", || synced(&mail)).await;
        assert!(server.messages("INBOX").iter().any(|(uid, _, _)| *uid == there), "the first sync moves nothing");

        eventually("the inbox watched", || async { idling(&server) }).await;
        server.add("INBOX", &bernd("Second offer", 0), &[]);
        eventually("archived as it came", || async {
            server.messages("Archive").iter().any(|(_, _, raw)| raw.contains("Subject: Second offer\r\n"))
        })
        .await;
        assert_eq!(titles(&inbox), ["Offer"], "no entry for the archived mail");

        // A catch-up files mail the feed missed, as it is.
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        let missed = server.add("INBOX", &bernd("Missed", 0), &[]);
        let (inbox, mail, feed) = quiet(dir.path(), &server).await;
        let read = |spec: &mut super::super::filters::FilterSpec| spec.mark_read = true;
        make_filter(&inbox, &mail, "from:bernd", None, read);
        feed.catch_up(Utc::now()).await;
        assert_eq!(titles(&inbox), ["Missed"]);
        assert!(server.flags("INBOX", missed).is_empty(), "not read");
    }

    /// Mail whose `Message-ID` the account holds in another folder came back, it was not delivered:
    /// no filter takes it out again. Copies in the archive (Gmail's "All Mail") and in sent mail
    /// (mail to oneself, a mailing list) do not count: that mail was delivered and is filtered.
    #[tokio::test]
    async fn mail_the_account_holds_in_another_folder_is_not_filtered() {
        for (other, filtered) in [("Archive", true), ("Sent", true), ("Junk", false), ("Projects", false)] {
            let dir = tempfile::tempdir().expect("temp dir");
            let server = FakeImap::start(PASSWORD).await;
            server.add_folder("Junk", "\\HasNoChildren \\Junk", 15);
            server.add_folder("Projects", "\\HasNoChildren", 16);
            server.add(other, &bernd("Offer", 0), &["\\Seen"]);
            let back = server.add("INBOX", &bernd("Offer", 0), &[]);
            let (inbox, mail, feed) = quiet(dir.path(), &server).await;
            make_filter(&inbox, &mail, "from:bernd", None, |spec| spec.mark_read = true);
            let store = mail.store(ANNA).await.expect("the store");
            let all = store.run(|store| store.search(&SearchQuery::default(), 10, 0)).await.expect("read");
            let in_inbox = store.run(|store| store.folder_by_role(Role::Inbox)).await.expect("read").expect("an inbox");
            let offer = all.iter().find(|message| message.folder == in_inbox.id).expect("the copy in the inbox");

            feed.take(arrived(&[offer]), Utc::now()).await;
            let read = server.flags("INBOX", back).iter().any(|flag| flag == "\\Seen");
            assert_eq!(read, filtered, "a copy in {other}");
            let expected: &[&str] = if filtered { &[] } else { &["Offer"] };
            assert_eq!(titles(&inbox), expected, "a copy in {other}: filed as any mail unless read by the filter");
        }
    }

    /// Decision 4: a category the person set stays – on the entry of the conversation, and with a
    /// mail that has one; a category no longer in the catalogue is skipped.
    #[tokio::test]
    async fn a_category_set_by_hand_stays_before_a_filters() {
        use torro_mail::Category;
        let dir = tempfile::tempdir().expect("temp dir");
        let server = FakeImap::start(PASSWORD).await;
        server.add("INBOX", &bernd("Offer", 1), &[]);
        server.add("INBOX", &letter("Re Offer", "Bernd <bernd@example.com>", 0, "", Some("Offer@example.org")), &[]);
        server.add("INBOX", &bernd("Plan", 0), &[]);
        let (inbox, mail, feed) = quiet(dir.path(), &server).await;
        let all = by_subject(&mail).await;
        // Filed before the filter was made, and sorted by hand.
        feed.take(arrived(&[&all["Offer"]]), Utc::now()).await;
        let id = entry(&entries(&inbox), "Offer").id.clone();
        let by_hand = Request::SetItemCategory { id, category: Category::Newsletter, remember: false };
        assert!(matches!(inbox.handle(by_hand, None, Utc::now()), Response::ItemCategory { .. }));
        let store = mail.store(ANNA).await.expect("the store");
        let plan = all["Plan"].id.clone();
        let person = MailCategory::by_person(Category::Newsletter);
        store.run(move |store| store.set_category(&plan, Some(person))).await.expect("kept");
        make_filter(&inbox, &mail, "from:bernd", None, |spec| spec.category = Some(Category::Invoice));

        feed.take(arrived(&[&all["Re Offer"], &all["Plan"]]), Utc::now()).await;
        let items = entries(&inbox);
        assert_eq!(entry(&items, "Re Offer").category, Some(person), "the entry keeps the person's");
        let kept = by_subject(&mail).await;
        assert_eq!(kept["Plan"].category, Some(person), "the mail keeps the person's");
        assert_eq!(kept["Re Offer"].category, Some(MailCategory::by_rule(Category::Invoice)), "a new mail gets it");

        let ids = vec![all["Offer"].id.clone()];
        let none = super::super::filters::on_arrival(&mail, &store, ANNA, &ids, &[]).await;
        assert!(none.is_empty(), "not in the catalogue: skipped");
        assert_eq!(by_subject(&mail).await["Offer"].category, None, "nothing set");
    }
}
