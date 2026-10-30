//! The mail commands on the wire (task M2.1, `docs/tasks/mail.md`): what the `mail.*` requests and
//! answers carry besides the types of `torro-mail`, the limits every mail request keeps
//! ([`limit_refusal`]), and which account and message ids a request names
//! ([`Request::mail_account`], [`Request::message_ids`]), for the core's reach checks and its
//! audit log.
//!
//! Rights in short (`docs/api.md`): the area `mail` grants a level; the client's
//! `Rights::mail_accounts` names the accounts it sees (others are `not-found` for the whole call,
//! checked before anything is read); `Rights::outbound` decides whether `mail.send_draft` runs,
//! waits for a person, or is `forbidden`.

use chrono::{DateTime, NaiveDate, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use torro_core::i18n::tr;
use torro_mail::{DraftAttachment, Folder, Role};

use crate::{Request, Response};

/// Which complete original messages a backup reads. `Local` never fetches missing bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BackupScope {
    Server,
    Local,
}

/// What the background backup job is currently doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupPhase {
    Queued,
    Counting,
    Reading,
    Verifying,
    Complete,
    Incomplete,
    /// Cancellation was requested and the task is still stopping; the account stays busy.
    Cancelling,
    Cancelled,
}

impl BackupPhase {
    /// The job has ended (complete, incomplete or cancelled) and no longer keeps its account busy.
    pub fn is_final(self) -> bool {
        matches!(self, Self::Complete | Self::Incomplete | Self::Cancelled)
    }
}

/// Progress and final result of one backup job; `archive` is an opaque name in the allowed folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupJob {
    pub id: String,
    pub account: String,
    pub source: BackupScope,
    pub phase: BackupPhase,
    pub folders: u64,
    pub messages: u64,
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub archive: Option<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// Offline verification result for an archive. Counts include folder occurrences of messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackupVerification {
    pub source: BackupScope,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since: Option<NaiveDate>,
    pub started_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<DateTime<Utc>>,
    pub complete: bool,
    pub folders: u64,
    pub messages: u64,
    pub bytes: u64,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// Read-only estimate of what a restore would do at the time of the test run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestorePlan {
    pub id: String,
    pub account: String,
    /// The archive this plan was made from, as the caller named it.
    #[serde(default)]
    pub archive: String,
    pub complete_archive: bool,
    pub folders_to_create: u64,
    pub messages_to_add: u64,
    pub already_present: u64,
    pub uncertain: u64,
    /// Uploads of an earlier restore that were never confirmed and are absent from the target;
    /// `mail.restore_apply` repeats them only with `retry_unconfirmed`.
    #[serde(default)]
    pub unconfirmed: u64,
    pub bytes_to_add: u64,
    /// Exact archive-to-target folder mapping and counts for the review table.
    #[serde(default)]
    pub folders: Vec<RestoreFolderPlan>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// One folder row of a read-only restore test run. Paths are descriptive data, never ZIP paths.
///
/// `archive_path` and `target_path` are exact IMAP mailbox names as they go on the wire: modified
/// UTF-7 (RFC 3501 5.1.3), so `Entwürfe` is `Entw&APw-rfe`. They identify the folder; compare and
/// pass them on as they are, never show them. For people there are `archive_display` and
/// `target_display` ([`RestoreFolderPlan::archive_shown`], [`RestoreFolderPlan::target_shown`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreFolderPlan {
    /// The folder's mailbox name in the archive, as the source server listed it.
    pub archive_path: String,
    /// The mailbox name on the target the restore uses (it may differ from `archive_path`: `INBOX`
    /// as the target spells it, a collection folder the target fills by itself).
    pub target_path: String,
    /// `archive_path` decoded for people (`Entwürfe`). Empty in answers of cores before #267's
    /// review; show `archive_path` then.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub archive_display: String,
    /// `target_path` decoded for people. Empty in answers of older cores; show `target_path` then.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub target_display: String,
    pub create: bool,
    pub messages_to_add: u64,
    pub already_present: u64,
    pub uncertain: u64,
    pub bytes_to_add: u64,
    /// A virtual aggregate folder (`\All`, Gmail's starred mail) that a target folder fills by
    /// itself too (`target_path`): only mail found in no ordinary folder is added here. Where the
    /// target has no such folder, and always for Gmail's important mail, the archive's copies go
    /// up as ordinary mail and this is `false`.
    #[serde(default)]
    pub aggregate: bool,
    /// Archived mail of an aggregate folder that is a copy of mail in an ordinary folder; the
    /// server shows it here again by itself, so it is never appended.
    #[serde(default)]
    pub covered: u64,
}

impl RestoreFolderPlan {
    /// The archive folder as people read it: `archive_display`, or `archive_path` when an older
    /// core sent none.
    pub fn archive_shown(&self) -> &str {
        if self.archive_display.is_empty() { &self.archive_path } else { &self.archive_display }
    }

    /// The target folder as people read it: `target_display`, or `target_path` when an older core
    /// sent none.
    pub fn target_shown(&self) -> &str {
        if self.target_display.is_empty() { &self.target_path } else { &self.target_display }
    }
}

/// Verified outcome of a real restore; uncertain entries must be investigated before retrying.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestoreResult {
    pub added: u64,
    pub skipped: u64,
    pub uncertain: u64,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// At most this many messages in one call (`ids` of `mail.set_flags`, `mail.move_messages`,
/// `mail.delete_messages`).
pub const MAX_IDS: usize = 100;

/// At most this many messages in one page of `mail.search_messages`.
pub const MAX_LIMIT: u32 = 100;

/// At most this many characters in the `query` of `mail.search_messages`.
pub const MAX_QUERY_CHARS: usize = 500;

/// The largest file a draft may attach (`DraftAttachment::File`, security rule 1): 25 MiB.
pub const MAX_FILE_BYTES: u64 = 25 * 1024 * 1024;

/// Where `mail.move_messages` puts the messages: `{"folder": "<id>"}` or `{"role": "archive"}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MoveTarget {
    /// A folder by its id from `mail.list_folders`, of the same account as the messages. A folder
    /// whose role is `trash` counts as deleting, as `{"role": "trash"}` does: the core checks the
    /// folder's role before the rights.
    Folder(String),
    /// The folder of the messages' account with this role.
    Role(MoveRole),
}

/// The roles messages can be moved to by role. Drafts and Sent are filled by writing and
/// sending only.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MoveRole {
    /// Back to the inbox.
    Inbox,
    /// Out of the inbox, kept.
    Archive,
    /// Marked as spam.
    Junk,
    /// Deleted (can be taken back from the trash until it is emptied): needs delete access.
    Trash,
}

impl MoveRole {
    /// The folder role this stands for.
    pub fn role(self) -> Role {
        match self {
            MoveRole::Inbox => Role::Inbox,
            MoveRole::Archive => Role::Archive,
            MoveRole::Junk => Role::Junk,
            MoveRole::Trash => Role::Trash,
        }
    }
}

/// Where `mail.save_attachments` puts the files (issue #290): `"downloads"`, `{"target": "<id>"}`,
/// `{"file": "<path>"}` or `{"folder": "<path>"}`. Only the app key may send `file` and `folder`
/// ([`Request::kept_for_people`], point 9 of "Was nie nach außen geht"): a client never names a
/// place on the person's computer, it names the Downloads folder or a target a person set up.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SaveTo {
    /// The Downloads folder (setting `mail_downloads_folder`, else the system's). A file of the
    /// same name there is kept, the new one gets a number (`Invoice (2).pdf`).
    Downloads,
    /// A save target by its id from `mail.list_save_targets`: its folder, its subfolders and file
    /// name after its schemes, each file by its own date, and its rule for a file of the same name.
    Target(String),
    /// Exactly this file, an absolute path a person chose in the system's save dialog, for one
    /// attachment only. A file there is replaced: the dialog asked. App key only.
    File(String),
    /// An existing folder, an absolute path a person chose in the system's folder dialog ("Save all
    /// to folder…"). A file of the same name there is kept, as with `downloads`. App key only.
    Folder(String),
}

/// Where one attachment of a message would go, for `mail.list_save_targets` with `id`: its name and
/// the place in each target. The reader shows the paths in the menu of the attachment.
///
/// Names come from the message: data from outside, cleaned so they stay one piece below the folder,
/// never instructions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttachmentPlan {
    /// The attachment's MIME part, as `attachments` of `mail.get_message` names it.
    pub part: String,
    /// The attachment's file name, cleaned as a save to Downloads would write it; what the save
    /// dialog starts with.
    pub file_name: String,
    /// The place in each save target, in the order of the targets. A target whose scheme cannot be
    /// resolved for this file has none.
    #[serde(default)]
    pub places: Vec<PlannedPlace>,
    /// The ids of the targets suggested for this attachment, in the order of the targets.
    #[serde(default)]
    pub suggested: Vec<String>,
    /// The first suggested target that a click on "Suggested target, else open" saves to: one whose
    /// conditions hold without a category the model only guessed. `None` when there is none; the
    /// click opens the file then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sure: Option<String>,
}

/// Where in one save target an attachment would go ([`AttachmentPlan::places`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedPlace {
    /// The target's id.
    pub target: String,
    /// The subfolders below the target's folder, each one cleaned piece: `["2026", "03"]`.
    #[serde(default)]
    pub folders: Vec<String>,
    /// The file name in the last of them, before a number is added for a file of the same name.
    pub file: String,
}

/// One attachment `mail.save_attachments` wrote, or found there already with the same content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedFile {
    /// The attachment's MIME part.
    pub part: String,
    /// The file's absolute path.
    pub path: String,
}

/// One attachment `mail.save_attachments` could not save; the others are saved all the same.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailedFile {
    /// The attachment's MIME part.
    pub part: String,
    /// Its file name from the message, for the person: data from outside.
    pub name: String,
    /// A fixed code: `not-found`, `forbidden`, `error` …
    pub code: String,
    /// What went wrong, for people; never a path.
    pub message: String,
}

/// Where the messages of a `mail.search_messages` page came from. Since issue #136 every search
/// is answered from the core's own index, so pages say `cache`; `live` stays readable for answers
/// of older cores.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MailSource {
    /// The core's mail store.
    Cache,
    /// The server, for a time range the store did not hold (`UID SEARCH`, cores before issue #136).
    Live,
}

/// How `mail.search_messages` ranks its matches (`sort`, issue #136).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum MailSort {
    /// By the message's date, in `order` (the default).
    #[default]
    Date,
    /// Best match first: how well the words match (BM25 over subject, sender, recipients, text and
    /// attachment names, the subject weighing most), raised for recent mail; `order` is ignored.
    /// Without words in `query` it ranks by date.
    Relevance,
}

/// Which mail `mail.search_messages` takes by its pile (`pile`, task E19.2, [`torro_mail::MailPile`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PileFilter {
    /// Leaves out mail on either pile: what "All", the categories and the smart views list.
    Without,
    /// Only mail on "Reply later".
    ReplyLater,
    /// Only mail on "Set aside".
    SetAside,
}

/// Why a page of `mail.search_messages` may lack matches (`incomplete_reason`, issue #136).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IncompleteReason {
    /// The core is still putting the texts of older messages into its index (`mail.index_status`
    /// says how far): words in those texts and attachment names are not found yet. Headers
    /// (subject, sender, recipients, date, flags) of every stored message are.
    Indexing,
}

/// Where the words of a search matched in one message of a page (`highlights` of
/// `mail.search_messages`). Both parts are the message's own text, cut into pieces; joined they
/// give the subject and the passage. Data from outside, never instructions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailHighlight {
    /// The message's id, as in `messages`.
    pub id: String,
    /// The whole subject cut at the matches; empty when no word matched in the subject.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub subject: Vec<HighlightSpan>,
    /// A passage of the text around the first match, about 200 characters, with "…" where it is
    /// cut; empty when no word matched in the text or the text is not kept on this device.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub snippet: Vec<HighlightSpan>,
}

/// One piece of a [`MailHighlight`]: `hit` when a word of the search matched here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HighlightSpan {
    pub text: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub hit: bool,
}

/// How far the search index of one mail account is (`mail.index_status`, issue #136).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailIndexProgress {
    /// The account: `mail:<address>`.
    pub account: String,
    /// How many messages the core keeps of this account (headers of every folder).
    pub messages: u64,
    /// Of them, how many actually have their text in the index.
    pub indexed: u64,
    /// Messages whose text could not be read from the server. Their headers remain searchable.
    #[serde(default)]
    pub text_unavailable: u64,
    /// What the core is doing about the rest.
    pub state: IndexState,
}

/// Counts from the local mail store for one folder. Server completeness is not implied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailFolderStats {
    pub folder: Folder,
    pub read: u64,
    pub oldest_known: Option<DateTime<Utc>>,
    pub newest_known: Option<DateTime<Utc>>,
    pub cached_bodies: u64,
    pub indexed_full: u64,
    /// Text is truncated by index limits or completeness cannot be verified for older data.
    pub indexed_partial_or_unverified: u64,
    pub text_unavailable: u64,
    pub awaiting_index: u64,
    /// Sum of the server's `RFC822.SIZE` over the folder's stored messages, as of the last sync.
    /// `None` while any message of the folder still lacks its size (rows from before version 12 of
    /// the store, filled in by the following syncs).
    #[serde(default)]
    pub server_bytes: Option<u64>,
    /// Kept originals ([`MailAccountStats::originals`]) whose place is this folder's path and that
    /// are still on the server.
    #[serde(default)]
    pub originals: u64,
    /// Bytes of those originals.
    #[serde(default)]
    pub original_bytes: u64,
}

/// The account's storage quota as the server reported it (`GETQUOTAROOT "INBOX"`, resource
/// `STORAGE`, RFC 9208) at the last sync. Not a live value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailQuota {
    /// Bytes in use (the server counts in units of 1024 bytes).
    pub used_bytes: u64,
    /// Bytes allowed.
    pub limit_bytes: u64,
    /// When the sync asked the server.
    pub checked: DateTime<Utc>,
}

/// How many stored messages of an account are dated in one month (UTC, by the message's date)
/// and how many of them have their reading text kept on this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailMonthCount {
    pub year: i32,
    /// 1 to 12.
    pub month: u32,
    pub messages: u64,
    /// Of `messages`, those readable offline.
    pub cached_bodies: u64,
}

/// A mail account's locally known inventory. Dates are deliberately only *known* dates:
/// synchronization does not currently persist proof that every server folder is complete.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailAccountStats {
    pub account: String,
    pub folders: Vec<MailFolderStats>,
    pub messages: u64,
    /// Sum of the server-reported sizes of the known messages. `None` until every known message
    /// has a size. This is an inventory size, never an IMAP quota or local disk usage.
    #[serde(default)]
    pub known_mail_bytes: Option<u64>,
    /// Server-reported sizes of messages whose reading text or original is on this device.
    /// Uses the same unit and message set as `known_mail_bytes` for a meaningful share bar.
    #[serde(default)]
    pub offline_mail_bytes: Option<u64>,
    /// Server-reported sizes of messages selected by the account's offline policy.
    #[serde(default)]
    pub planned_mail_bytes: Option<u64>,
    /// Server-reported sizes of locally readable messages whose full text is indexed.
    #[serde(default)]
    pub indexed_offline_mail_bytes: Option<u64>,
    pub read: u64,
    pub unread: u64,
    pub oldest_known: Option<DateTime<Utc>>,
    pub newest_known: Option<DateTime<Utc>>,
    pub cached_bodies: u64,
    /// Oldest message whose complete reading text is kept locally.
    #[serde(default)]
    pub oldest_cached_body: Option<DateTime<Utc>>,
    /// Approximate bytes occupied by reading texts; separate from the search index.
    #[serde(default)]
    pub body_cache_bytes: u64,
    /// SQLite pages used by the word and trigram search indexes, when `dbstat` is available.
    #[serde(default)]
    pub search_index_bytes: Option<u64>,
    #[serde(default)]
    pub last_index_run: Option<DateTime<Utc>>,
    pub indexed_full: u64,
    pub indexed_partial_or_unverified: u64,
    pub text_unavailable: u64,
    pub awaiting_index: u64,
    pub database_bytes: u64,
    /// Fetched attachments that the cache can attribute to this account.
    pub cached_attachments: u64,
    /// Older untagged cache entries cannot be attributed and expire within 24 hours.
    pub attachment_cache_bytes: Option<u64>,
    pub last_sync: Option<DateTime<Utc>>,
    pub sync_problem: Option<String>,
    /// The storage quota from the last sync; `None` when the server has no `QUOTA` capability,
    /// names no `STORAGE` limit, or was not synced yet.
    #[serde(default)]
    pub quota: Option<MailQuota>,
    /// Stored messages per month, oldest month first; only months with mail.
    #[serde(default)]
    pub months: Vec<MailMonthCount>,
    /// Kept RFC-822 originals (all-offline mode) of mail still on the server.
    #[serde(default)]
    pub originals: u64,
    /// Bytes of those originals.
    #[serde(default)]
    pub original_bytes: u64,
    /// When the counts above – the attachments included – were computed (issue #395): the service
    /// answers from the last computation, which can be older than a minute; then it computes anew
    /// in the background for the next call. `last_sync` and `sync_problem` are always current.
    /// `None` from a core before that issue.
    #[serde(default)]
    pub as_of: Option<DateTime<Utc>>,
}

/// What `mail.overview` counted (issue #409): how much mail came in over a span of local days, of
/// which kind, from whom and how much of it was opened, and what lies in the inbox now. Counted from
/// the core's store of every reachable account (or of one): every folder but Sent and Drafts, a
/// message kept in two folders or accounts (the same `Message-ID`) once. Opened is what the server
/// marks as seen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailOverview {
    /// The first local day counted.
    pub from: NaiveDate,
    /// The last local day counted: today.
    pub until: NaiveDate,
    /// When the numbers were counted.
    pub as_of: DateTime<Utc>,
    /// Every day from `from` to `until`, also those without mail.
    pub days: Vec<OverviewDay>,
    /// The categories of what came in over the span, the most first. Mail without a category (not
    /// sorted yet) counts under `category: null`.
    pub categories: Vec<OverviewCategory>,
    /// The senders with the most mail over the span, the most first, at most
    /// [`OVERVIEW_SENDERS`].
    pub senders: Vec<OverviewSender>,
    /// The inbox folders now, per category, by age.
    pub inbox: Vec<OverviewInbox>,
}

/// How many senders `mail.overview` names at most.
pub const OVERVIEW_SENDERS: usize = 6;

/// What came in on one local day, per main category ([`MailOverview::days`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverviewDay {
    pub day: NaiveDate,
    /// Only groups with mail; `group: null` is mail without a category.
    pub groups: Vec<OverviewGroupCount>,
}

/// How much mail of one main category came in on a day.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverviewGroupCount {
    /// `None`: mail without a category, or of a category without a group.
    pub group: Option<torro_mail::CategoryGroup>,
    pub count: u32,
}

/// One category of what came in ([`MailOverview::categories`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverviewCategory {
    /// `None`: mail that has no category yet.
    pub category: Option<torro_mail::Category>,
    /// Its main category in the catalogue now.
    pub group: Option<torro_mail::CategoryGroup>,
    pub count: u32,
    /// Of `count`, the mail marked as seen.
    pub opened: u32,
}

/// One of the senders with the most mail ([`MailOverview::senders`]). Name, address and `latest`
/// come from the mail and are foreign text (`untrusted`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverviewSender {
    /// The name of the sender's newest mail.
    pub name: Option<String>,
    /// The address, in lower case.
    pub address: String,
    pub count: u32,
    /// Of `count`, the mail marked as seen.
    pub opened: u32,
    /// The category most of the sender's mail has; `None` when most has none.
    pub category: Option<torro_mail::Category>,
    /// The sender's newest mail outside the trash (the newest of all when every one is there).
    pub latest: torro_mail::MessageSummary,
}

/// The mail of one category in the inbox folders by age ([`MailOverview::inbox`]): today, the six
/// days before, older. Local days.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverviewInbox {
    /// `None`: mail that has no category yet.
    pub category: Option<torro_mail::Category>,
    pub group: Option<torro_mail::CategoryGroup>,
    pub today: u32,
    pub week: u32,
    pub older: u32,
}

/// How many message bodies an account keeps for offline reading. Search text is independent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[repr(i64)]
pub enum MailCacheMode {
    HeadersOnly = 0,
    #[default]
    Recent90Days = 1,
    AllOffline = 2,
    SinceDate = 3,
    MaxSize = 4,
}

/// Per-account local storage choice, persisted with the mail store.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MailCachePolicy {
    pub account: String,
    pub mode: MailCacheMode,
    /// Inclusive ISO calendar date for `SinceDate`; all known headers remain available.
    #[serde(default)]
    pub since_date: Option<String>,
    /// Maximum mail content in MiB for `MaxSize`, with newest messages selected first.
    #[serde(default)]
    pub max_local_mb: Option<u64>,
    /// Legacy wire field. The service ignores it and always returns `false`: attachment files
    /// are fetched only when opened, then kept according to `attachment_retention_days`.
    #[serde(default)]
    pub prefetch_attachments: bool,
    /// Attachment files are removed after this many days; 1 by default.
    #[serde(default = "default_attachment_days")]
    pub attachment_retention_days: u32,
}

fn default_attachment_days() -> u32 {
    1
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MailCacheAction {
    ApplyPolicy,
    FetchMissingBodies,
    FetchAttachments,
    RebuildIndex,
    CleanCache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum MailCacheJobState {
    Running,
    Paused,
    Complete,
    Failed,
}

/// A durable, resumable maintenance operation on one mail account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailCacheJob {
    pub id: String,
    pub account: String,
    pub action: MailCacheAction,
    pub state: MailCacheJobState,
    pub total: u64,
    pub processed: u64,
    /// Last inspected store sequence for a resumable body fetch.
    #[serde(default)]
    pub cursor: Option<i64>,
    /// Estimated bytes affected before starting; actual storage may differ.
    pub estimated_bytes: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Dry-run counts for a cache choice or maintenance job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MailCacheEstimate {
    pub account: String,
    pub current_bodies: u64,
    pub bodies_to_fetch: u64,
    pub bodies_to_remove: u64,
    pub estimated_download_bytes: u64,
    pub estimated_reclaim_bytes: u64,
}

/// What the core does about the texts not yet in an account's index ([`MailIndexProgress`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexState {
    /// Every message has either indexed text or a known unavailable text; inspect both counts.
    Complete,
    /// The core is fetching the missing texts from the server, newest first.
    Indexing,
    /// Texts are missing and the core waits: for the first sync, or for the server to be reached
    /// again. It goes on by itself.
    Waiting,
}

/// What became of a sent message (`mail.send_draft`, the send rule of task M2.4). A server that
/// refused it is an error, not a delivery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Delivery {
    /// The server took it.
    Accepted,
    /// The connection broke after the message was handed over: it may or may not have gone out.
    /// It is never sent again by itself; look into the sent folder or ask the recipient.
    Unknown,
}

/// At most this many recipients in a draft, `to`, `cc` and `bcc` together.
pub const MAX_RECIPIENTS: usize = 100;

/// At most this many attachments in a draft.
pub const MAX_ATTACHMENTS: usize = 20;

/// The longest draft text: 1 MiB.
pub const MAX_BODY_BYTES: usize = 1024 * 1024;

/// The longest id a mail request may carry (account, folder, message, draft, attachment part,
/// cursor): enough for `mail:` and the longest address, and far more than the core's own ids.
pub const MAX_ID_BYTES: usize = 300;

/// The `error` answer for a mail request that breaks a limit, before anything is looked at: `ids`
/// empty or over [`MAX_IDS`], more than [`MAX_ATTACHMENTS`] `parts` to save, a search `limit` of 0
/// or over [`MAX_LIMIT`], a `query` over [`MAX_QUERY_CHARS`] characters (a mail filter's over
/// [`torro_mail::FILTER_QUERY_MAX`], issue #339), a draft with more than
/// [`MAX_RECIPIENTS`] recipients, more than [`MAX_ATTACHMENTS`] attachments or a text over
/// [`MAX_BODY_BYTES`], and any id, folder, cursor or attachment part over [`MAX_ID_BYTES`]. `None`
/// for every request within them and for every other request. The core calls it before rights,
/// approvals and handlers, so nobody is asked to approve what would be refused anyway.
pub fn limit_refusal(request: &Request) -> Option<Response> {
    let refuse = |message: String| Some(Response::Error { code: "error".into(), message });
    if mail_ids(request).any(|id| id.len() > MAX_ID_BYTES) {
        return refuse(format!("an id, folder, cursor or attachment part may hold at most {MAX_ID_BYTES} bytes"));
    }
    match request {
        Request::MailBackupCreate { file_name: Some(file_name), .. } if !valid_backup_filename(file_name) => {
            refuse(tr("Backup file name must be a plain .zip name or absolute app path.").into())
        }
        // The piles (task E19.2) name mail as `mail.set_flags` does.
        Request::SetMailFlags { ids, .. }
        | Request::MoveMail { ids, .. }
        | Request::DeleteMail { ids, .. }
        | Request::SetMailPile { ids, .. } => {
            if ids.is_empty() {
                return refuse("name at least one message in ids".into());
            }
            if ids.len() > MAX_IDS {
                return refuse(format!("at most {MAX_IDS} messages per call, not {}", ids.len()));
            }
            None
        }
        Request::SearchMail { limit, query, .. } => {
            if *limit == 0 || *limit > MAX_LIMIT {
                return refuse(format!("limit must be from 1 to {MAX_LIMIT}"));
            }
            if query.as_deref().is_some_and(|query| query.chars().count() > MAX_QUERY_CHARS) {
                return refuse(format!("the query may hold at most {MAX_QUERY_CHARS} characters"));
            }
            None
        }
        // Mail filters (issue #339): a search as long as one of `mail.search_messages`.
        Request::CreateMailFilter { query, .. }
        | Request::UpdateMailFilter { query, .. }
        | Request::PreviewMailFilter { query, .. }
            if query.chars().count() > torro_mail::FILTER_QUERY_MAX =>
        {
            refuse(format!("the search of a filter may hold at most {} characters", torro_mail::FILTER_QUERY_MAX))
        }
        Request::SaveMailAttachments { parts, .. } if parts.len() > MAX_ATTACHMENTS => {
            refuse(format!("at most {MAX_ATTACHMENTS} attachments per call, or none for all"))
        }
        Request::SaveDraft { draft } => {
            if draft.to.len() + draft.cc.len() + draft.bcc.len() > MAX_RECIPIENTS {
                return refuse(format!("a draft may have at most {MAX_RECIPIENTS} recipients"));
            }
            if draft.attachments.len() > MAX_ATTACHMENTS {
                return refuse(format!("a draft may have at most {MAX_ATTACHMENTS} attachments"));
            }
            if draft.body.len() > MAX_BODY_BYTES {
                return refuse("the text of a draft may hold at most 1 MiB".into());
            }
            None
        }
        _ => None,
    }
}

fn valid_backup_filename(file_name: &str) -> bool {
    let path = std::path::Path::new(file_name);
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else { return false };
    if !path.is_absolute() && name != file_name {
        return false;
    }
    !name.starts_with('.')
        && name.len() <= 120
        && name.ends_with(".zip")
        && name.bytes().all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// Every id-like text of a mail request, for [`MAX_ID_BYTES`]: its account, messages, folder,
/// cursor and attachment part.
fn mail_ids(request: &Request) -> impl Iterator<Item = &str> {
    let more: Vec<&str> = match request {
        Request::UpdateMailAccount { id, .. } | Request::ReconfigureMail { id, .. } => vec![id],
        Request::SearchMail { folder, cursor, .. } => folder.iter().chain(cursor).map(String::as_str).collect(),
        Request::MoveMail { to: MoveTarget::Folder(folder), .. } => vec![folder],
        Request::GetMailAttachment { attachment, .. } => vec![attachment],
        // Save targets (issue #290): their ids and the parts, as short as the core's own.
        Request::UpdateSaveTarget { id, .. } | Request::RemoveSaveTarget { id } => vec![id],
        Request::SaveMailAttachments { parts, to, .. } => {
            let target = match to {
                SaveTo::Target(target) => Some(target.as_str()),
                _ => None,
            };
            parts.iter().map(String::as_str).chain(target).collect()
        }
        // Mail filters (issue #339): their ids and the folder they move to.
        Request::CreateMailFilter { file, .. } => filter_folder(file.as_ref()).into_iter().collect(),
        Request::UpdateMailFilter { id, file, .. } => {
            std::iter::once(id.as_str()).chain(filter_folder(file.as_ref())).collect()
        }
        Request::RemoveMailFilter { id } | Request::ApplyMailFilter { id } => vec![id],
        Request::MailBackupCreate { file_name, .. } => file_name.iter().map(String::as_str).collect(),
        Request::MailBackupStatus { job, .. } | Request::MailBackupCancel { job, .. } => vec![job],
        Request::MailBackupVerify { file, .. } | Request::MailRestorePlan { file, .. } => vec![file],
        Request::MailRestoreApply { plan, .. } => vec![plan],
        Request::SaveDraft { draft } => draft
            .attachments
            .iter()
            .filter_map(|attachment| match attachment {
                DraftAttachment::MailPart { part, .. } => Some(part.as_str()),
                DraftAttachment::File { .. } => None,
            })
            .collect(),
        _ => Vec::new(),
    };
    request.mail_account().into_iter().chain(request.message_ids()).chain(more)
}

/// The folder a filter moves its mail to, if it names one.
fn filter_folder(file: Option<&torro_mail::FilterFile>) -> Option<&str> {
    match file {
        Some(torro_mail::FilterFile::Folder { id }) => Some(id.as_str()),
        _ => None,
    }
}

impl Request {
    /// The mail account a request names itself: `account` of `mail.list_folders`, `mail.overview`,
    /// `mail.search_messages`, `mail.index_status` and the mail filters (`mail.create_filter`,
    /// `mail.update_filter`, `mail.preview_filter`, issue #339), the draft's `account` of `mail.save_draft`. `None` for every other
    /// request, and when the field is left out (then the core takes every account the caller
    /// reaches). Messages and folders name their account only through the core's store
    /// ([`Request::message_ids`]).
    pub fn mail_account(&self) -> Option<&str> {
        match self {
            Request::MailFolders { account }
            | Request::MailAccountStats { account }
            | Request::MailOverview { account, .. }
            | Request::SearchMail { account, .. }
            | Request::MailIndexStatus { account }
            // A mail filter for one account (issue #339); without one it runs in every account.
            | Request::CreateMailFilter { account, .. }
            | Request::UpdateMailFilter { account, .. }
            | Request::PreviewMailFilter { account, .. } => account.as_deref(),
            Request::GetMailCachePolicy { account }
            | Request::PreviewMailCache { account, .. }
            | Request::StartMailCacheJob { account, .. }
            | Request::MailCacheJobStatus { account }
            | Request::ControlMailCacheJob { account, .. } => Some(account),
            Request::SetMailCachePolicy { policy } => Some(&policy.account),
            Request::MailBackupCreate { account, .. }
            | Request::MailBackupStatus { account, .. }
            | Request::MailBackupCancel { account, .. }
            | Request::MailBackupVerify { account, .. }
            | Request::MailRestorePlan { account, .. }
            | Request::MailRestoreApply { account, .. } => Some(account.as_str()),
            Request::SaveDraft { draft } => Some(draft.account.as_str()),
            _ => None,
        }
    }

    /// Every message id a request names, in order: `id`/`ids` (also of `mail.list_save_targets` and
    /// `mail.save_attachments`), a draft's own id, the message it answers and the messages it
    /// forwards parts of, the mail put on a pile (`inbox.set_mail_pile`, task E19.2). Empty for every
    /// other request. If one of them is in an account the caller does not reach, the whole call is `not-found` (checked
    /// before anything is read).
    pub fn message_ids(&self) -> Vec<&str> {
        match self {
            Request::GetMail { id, .. }
            | Request::GetMailThread { id }
            | Request::GetMailAttachment { id, .. }
            | Request::SaveMailAttachments { id, .. }
            | Request::ListSaveTargets { id: Some(id) }
            | Request::SendDraft { id } => vec![id.as_str()],
            Request::SetMailFlags { ids, .. }
            | Request::MoveMail { ids, .. }
            | Request::DeleteMail { ids, .. }
            | Request::SetMailPile { ids, .. } => ids.iter().map(String::as_str).collect(),
            Request::SaveDraft { draft } => {
                let parts = draft.attachments.iter().filter_map(|attachment| match attachment {
                    DraftAttachment::MailPart { message, .. } => Some(message.as_str()),
                    DraftAttachment::File { .. } => None,
                });
                draft.id.as_deref().into_iter().chain(draft.in_reply_to.as_deref()).chain(parts).collect()
            }
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use torro_mail::Draft;

    use super::*;

    fn search(limit: u32, query: Option<String>) -> Request {
        Request::SearchMail {
            account: None,
            folder: None,
            query,
            unread: None,
            flagged: None,
            since: None,
            before: None,
            limit,
            cursor: None,
            order: None,
            sort: None,
            snoozed: None,
            categories: Vec::new(),
            pile: None,
        }
    }

    fn refused(request: &Request) -> bool {
        matches!(limit_refusal(request), Some(Response::Error { code, .. }) if code == "error")
    }

    /// Issue #290: `to` is a word for Downloads and an object for the rest; plans and results read
    /// back as written, with `fileName` in camelCase.
    #[test]
    fn save_types_round_trip() {
        for (to, wire) in [
            (SaveTo::Downloads, serde_json::json!("downloads")),
            (SaveTo::Target("0a1b2c3d".into()), serde_json::json!({"target": "0a1b2c3d"})),
            (SaveTo::File("/Users/anna/Rechnung.pdf".into()), serde_json::json!({"file": "/Users/anna/Rechnung.pdf"})),
            (SaveTo::Folder("/Users/anna/Ablage".into()), serde_json::json!({"folder": "/Users/anna/Ablage"})),
        ] {
            assert_eq!(serde_json::to_value(&to).expect("writes"), wire);
            assert_eq!(serde_json::from_value::<SaveTo>(wire).expect("reads"), to);
        }
        assert!(serde_json::from_value::<SaveTo>(serde_json::json!({"path": "/tmp"})).is_err());

        let plan = AttachmentPlan {
            part: "2".into(),
            file_name: "Rechnung_2026-03-28_4711.pdf".into(),
            places: vec![PlannedPlace {
                target: "0a1b2c3d".into(),
                folders: vec!["2026".into(), "03".into()],
                file: "Rechnung_2026-03-28_4711.pdf".into(),
            }],
            suggested: vec!["0a1b2c3d".into()],
            sure: Some("0a1b2c3d".into()),
        };
        let json = serde_json::to_value(&plan).expect("writes");
        assert_eq!(json["fileName"], "Rechnung_2026-03-28_4711.pdf");
        assert_eq!(json["places"][0]["folders"], serde_json::json!(["2026", "03"]));
        assert_eq!(serde_json::from_value::<AttachmentPlan>(json).expect("reads"), plan);
        let unsure = serde_json::json!({"part": "3", "fileName": "a.pdf"});
        let read: AttachmentPlan = serde_json::from_value(unsure).expect("lists may be left out");
        assert!(read.places.is_empty() && read.suggested.is_empty() && read.sure.is_none());

        let saved = Response::SavedAttachments {
            saved: vec![SavedFile { part: "2".into(), path: "/Users/anna/Rechnungen/2026/03/a.pdf".into() }],
            skipped: vec![SavedFile { part: "3".into(), path: "/Users/anna/Rechnungen/2026/03/b.pdf".into() }],
            failed: vec![FailedFile {
                part: "4".into(),
                name: "c.pdf".into(),
                code: "error".into(),
                message: "too large".into(),
            }],
        };
        let json = serde_json::to_value(&saved).expect("writes");
        assert_eq!(json["result"], "saved-attachments");
        assert_eq!(serde_json::from_value::<Response>(json).expect("reads"), saved);

        let target = torro_mail::SaveTarget {
            id: "0a1b2c3d".into(),
            spec: torro_mail::SaveTargetSpec::new("Invoices", "/Users/anna/Rechnungen"),
        };
        let listed = Response::SaveTargets {
            targets: vec![target.clone()],
            downloads: "/Users/anna/Downloads".into(),
            files: vec![plan],
        };
        let json = serde_json::to_value(&listed).expect("writes");
        assert_eq!(json["result"], "save-targets");
        assert_eq!(serde_json::from_value::<Response>(json).expect("reads"), listed);
        let removed = Response::SaveTargets { targets: Vec::new(), downloads: "/d".into(), files: Vec::new() };
        let json = serde_json::to_value(&removed).expect("writes");
        assert!(json.get("files").is_none(), "no files without an id: {json}");
        assert_eq!(serde_json::from_value::<Response>(json).expect("reads"), removed);
        let one = Response::SaveTarget { target };
        let json = serde_json::to_value(&one).expect("writes");
        assert_eq!(json["result"], "save-target");
        assert_eq!(json["target"]["fileName"], "{name}.{ext}");
        assert_eq!(serde_json::from_value::<Response>(json).expect("reads"), one);
    }

    /// Issue #290: saving names its message (for the reach check), its parts and target are ids
    /// within the limits, and at most [`MAX_ATTACHMENTS`] parts go in one call.
    #[test]
    fn saving_attachments_names_its_message_and_keeps_the_limits() {
        let save = |parts: Vec<String>, to| Request::SaveMailAttachments { id: "m:1".into(), parts, to };
        assert_eq!(save(Vec::new(), SaveTo::Downloads).message_ids(), ["m:1"]);
        assert_eq!(Request::ListSaveTargets { id: Some("m:2".into()) }.message_ids(), ["m:2"]);
        assert!(Request::ListSaveTargets { id: None }.message_ids().is_empty());
        assert!(!refused(&save(vec!["2".into(); MAX_ATTACHMENTS], SaveTo::Downloads)));
        assert!(refused(&save(vec!["2".into(); MAX_ATTACHMENTS + 1], SaveTo::Downloads)));
        let long = "x".repeat(MAX_ID_BYTES + 1);
        assert!(refused(&save(vec![long.clone()], SaveTo::Downloads)));
        assert!(refused(&save(Vec::new(), SaveTo::Target(long.clone()))));
        assert!(refused(&Request::RemoveSaveTarget { id: long }));
    }

    /// Issue #339: a mail filter names its account for the reach check, its search keeps the length
    /// of a search, and its id and folder the length of an id.
    #[test]
    fn mail_filters_name_their_account_and_keep_the_limits() {
        let create = |query: String, account: Option<&str>, folder: &str| Request::CreateMailFilter {
            query,
            account: account.map(str::to_string),
            file: Some(torro_mail::FilterFile::Folder { id: folder.into() }),
            mark_read: false,
            category: None,
            enabled: true,
        };
        let anna = Some("mail:anna@example.org");
        assert_eq!(create("from:x".into(), anna, "f:1").mail_account(), anna);
        assert_eq!(create("from:x".into(), None, "f:1").mail_account(), None);
        let preview = Request::PreviewMailFilter { query: "from:x".into(), account: Some("mail:b@example.org".into()) };
        assert_eq!(preview.mail_account(), Some("mail:b@example.org"));
        assert!(Request::ApplyMailFilter { id: "f1".into() }.mail_account().is_none());

        let longest = "x".repeat(torro_mail::FILTER_QUERY_MAX);
        assert!(!refused(&create(longest.clone(), anna, "f:1")));
        assert!(refused(&create(format!("{longest}y"), anna, "f:1")));
        let long = "x".repeat(MAX_ID_BYTES + 1);
        assert!(refused(&create("from:x".into(), anna, &long)));
        assert!(refused(&Request::RemoveMailFilter { id: long.clone() }));
        assert!(refused(&Request::ApplyMailFilter { id: long }));
        assert!(!refused(&Request::ApplyMailFilter { id: "f12".into() }));
    }

    /// DS.1 (#314): an answer of a core from before (no quota, sizes, months or originals) reads
    /// with the new parts empty, and the new parts read back as written.
    #[test]
    fn account_stats_of_an_older_core_read_without_the_new_parts() {
        let folder = MailFolderStats {
            folder: Folder {
                id: "f:1".into(),
                account: "mail:anna@example.org".into(),
                name: "INBOX".into(),
                path: "INBOX".into(),
                role: Some(Role::Inbox),
                unread: 1,
                total: 2,
            },
            read: 1,
            oldest_known: None,
            newest_known: None,
            cached_bodies: 1,
            indexed_full: 2,
            indexed_partial_or_unverified: 0,
            text_unavailable: 0,
            awaiting_index: 0,
            server_bytes: Some(4096),
            originals: 1,
            original_bytes: 2048,
        };
        let checked = DateTime::from_timestamp(1_790_000_000, 0).expect("a date");
        let stats = MailAccountStats {
            account: "mail:anna@example.org".into(),
            folders: vec![folder],
            messages: 2,
            known_mail_bytes: Some(4096),
            offline_mail_bytes: Some(2048),
            planned_mail_bytes: Some(4096),
            indexed_offline_mail_bytes: Some(2048),
            read: 1,
            unread: 1,
            oldest_known: None,
            newest_known: None,
            cached_bodies: 1,
            oldest_cached_body: None,
            body_cache_bytes: 0,
            search_index_bytes: None,
            last_index_run: None,
            indexed_full: 2,
            indexed_partial_or_unverified: 0,
            text_unavailable: 0,
            awaiting_index: 0,
            database_bytes: 0,
            cached_attachments: 0,
            attachment_cache_bytes: None,
            last_sync: None,
            sync_problem: None,
            quota: Some(MailQuota { used_bytes: 2475 * 1024, limit_bytes: 10_485_760 * 1024, checked }),
            months: vec![MailMonthCount { year: 2026, month: 10, messages: 2, cached_bodies: 1 }],
            originals: 1,
            original_bytes: 2048,
            as_of: Some(checked),
        };
        let json = serde_json::to_value(&stats).expect("writes");
        assert_eq!(json["quota"]["limit_bytes"], 10_737_418_240_u64);
        assert_eq!(serde_json::from_value::<MailAccountStats>(json.clone()).expect("reads"), stats);

        let mut older = json;
        for key in ["quota", "months", "originals", "original_bytes", "as_of"] {
            older.as_object_mut().expect("an object").remove(key);
        }
        for key in ["server_bytes", "originals", "original_bytes"] {
            older["folders"][0].as_object_mut().expect("an object").remove(key);
        }
        let read = serde_json::from_value::<MailAccountStats>(older).expect("an older answer reads");
        assert_eq!((read.quota, read.months.len(), read.originals, read.original_bytes), (None, 0, 0, 0));
        assert_eq!(read.as_of, None, "a core before #395 says nothing about when");
        let folder = &read.folders[0];
        assert_eq!((folder.server_bytes, folder.originals, folder.original_bytes), (None, 0, 0));
    }

    #[test]
    fn restore_folder_rows_are_optional_for_older_plan_responses() {
        let old = serde_json::json!({
            "id": "plan-1", "account": "mail:anna@example.org", "complete_archive": true,
            "folders_to_create": 0, "messages_to_add": 1, "already_present": 0,
            "uncertain": 0, "bytes_to_add": 57, "warnings": []
        });
        let plan: RestorePlan = serde_json::from_value(old).expect("older plan response");
        assert!(plan.folders.is_empty());
    }

    #[test]
    fn folder_rows_show_the_decoded_name_and_fall_back_to_the_wire_name() {
        let older = serde_json::json!({
            "archive_path": "Entw&APw-rfe", "target_path": "Entw&APw-rfe", "create": true,
            "messages_to_add": 1, "already_present": 0, "uncertain": 0, "bytes_to_add": 9
        });
        let row: RestoreFolderPlan = serde_json::from_value(older).expect("row of an older core");
        assert_eq!((row.archive_shown(), row.target_shown()), ("Entw&APw-rfe", "Entw&APw-rfe"));
        let row = RestoreFolderPlan { archive_display: "Entwürfe".into(), target_display: "Entwürfe".into(), ..row };
        assert_eq!((row.archive_shown(), row.target_shown()), ("Entwürfe", "Entwürfe"));
        let json = serde_json::to_value(&row).expect("json");
        assert_eq!(
            (json["archive_path"].as_str(), json["target_display"].as_str()),
            (Some("Entw&APw-rfe"), Some("Entwürfe"))
        );
    }

    #[test]
    fn limits_hold_for_ids_pages_and_queries() {
        let ids = |n: usize| (0..n).map(|i| format!("m:{i}")).collect::<Vec<_>>();
        let delete = |n| Request::DeleteMail { ids: ids(n), permanent: false };
        assert!(refused(&delete(0)) && refused(&delete(MAX_IDS + 1)));
        assert_eq!(limit_refusal(&delete(MAX_IDS)), None);
        let flags = Request::SetMailFlags { ids: ids(101), add: Vec::new(), remove: Vec::new() };
        assert!(refused(&flags));
        let to = MoveTarget::Role(MoveRole::Archive);
        assert!(refused(&Request::MoveMail { ids: Vec::new(), to }));
        // Task E19.2: the piles alike, also without a pile (taking off).
        let pile = |ids| Request::SetMailPile { ids, pile: None, since: None };
        assert!(refused(&pile(ids(0))) && refused(&pile(ids(MAX_IDS + 1))));
        assert_eq!(limit_refusal(&pile(ids(MAX_IDS))), None);
        assert!(refused(&pile(vec!["m:".to_string() + &"x".repeat(MAX_ID_BYTES)])));
        assert_eq!(pile(ids(2)).message_ids(), ["m:0", "m:1"]);

        assert!(refused(&search(0, None)) && refused(&search(MAX_LIMIT + 1, None)));
        assert_eq!(limit_refusal(&search(MAX_LIMIT, Some("ä".repeat(MAX_QUERY_CHARS)))), None, "characters, not bytes");
        assert!(refused(&search(10, Some("x".repeat(MAX_QUERY_CHARS + 1)))));
        assert_eq!(limit_refusal(&Request::Ping), None);

        for file_name in ["../escape.zip", "sub/archive.zip", "sub\\archive.zip", ".hidden.zip", "archive.txt"] {
            assert!(refused(&Request::MailBackupCreate {
                account: "mail:anna@example.org".into(),
                source: BackupScope::Server,
                since: None,
                file_name: Some(file_name.into()),
            }));
        }
        let absolute = std::env::temp_dir().join("backup.zip").to_string_lossy().into_owned();
        assert_eq!(
            limit_refusal(&Request::MailBackupCreate {
                account: "mail:anna@example.org".into(),
                source: BackupScope::Server,
                since: None,
                file_name: Some(absolute),
            }),
            None
        );

        let long = "m".repeat(MAX_ID_BYTES + 1);
        assert!(refused(&Request::GetMail { id: long.clone(), include_body: true, include_html: false }));
        assert!(refused(&Request::GetMailAttachment { id: "m:1".into(), attachment: long.clone() }));
        let mut paged = search(10, None);
        if let Request::SearchMail { cursor, .. } = &mut paged {
            *cursor = Some(long);
        }
        assert!(refused(&paged));
    }

    #[test]
    fn drafts_keep_to_recipients_attachments_and_text_size() {
        let to = |n: usize| {
            (0..n).map(|i| torro_mail::MailAddress { name: None, address: format!("p{i}@example.org") }).collect()
        };
        let draft = |recipients: usize, attachments: usize, body: usize| Request::SaveDraft {
            draft: Draft {
                html: None,
                id: None,
                account: "mail:anna@example.org".into(),
                to: to(recipients),
                cc: Vec::new(),
                bcc: to(1),
                subject: String::new(),
                body: "x".repeat(body),
                in_reply_to: None,
                attachments: (0..attachments).map(|i| DraftAttachment::File { path: format!("/tmp/{i}") }).collect(),
            },
        };
        assert_eq!(limit_refusal(&draft(MAX_RECIPIENTS - 1, MAX_ATTACHMENTS, MAX_BODY_BYTES)), None);
        assert!(refused(&draft(MAX_RECIPIENTS, 0, 0)), "bcc counts");
        assert!(refused(&draft(1, MAX_ATTACHMENTS + 1, 0)));
        assert!(refused(&draft(1, 0, MAX_BODY_BYTES + 1)));
    }

    #[test]
    fn requests_name_their_account_and_messages() {
        let draft = Draft {
            html: None,
            id: Some("m:draft".into()),
            account: "mail:anna@example.org".into(),
            to: Vec::new(),
            cc: Vec::new(),
            bcc: Vec::new(),
            subject: String::new(),
            body: String::new(),
            in_reply_to: Some("m:asked".into()),
            attachments: vec![
                DraftAttachment::MailPart { message: "m:forwarded".into(), part: "2".into() },
                DraftAttachment::File { path: "/tmp/x".into() },
            ],
        };
        let save = Request::SaveDraft { draft };
        assert_eq!(save.mail_account(), Some("mail:anna@example.org"));
        assert_eq!(save.message_ids(), ["m:draft", "m:asked", "m:forwarded"]);
        let folders = Request::MailFolders { account: Some("mail:anna@example.org".into()) };
        assert_eq!((folders.mail_account(), folders.message_ids().len()), (Some("mail:anna@example.org"), 0));
        assert_eq!(search(10, None).mail_account(), None);
        let status = Request::MailIndexStatus { account: Some("mail:anna@example.org".into()) };
        assert_eq!((status.mail_account(), status.message_ids().len()), (Some("mail:anna@example.org"), 0));
        let archive = Request::MailBackupVerify { file: "backup.zip".into(), account: "mail:anna@example.org".into() };
        assert_eq!((archive.mail_account(), archive.message_ids().len()), (Some("mail:anna@example.org"), 0));
        let get = Request::GetMailAttachment { id: "m:1".into(), attachment: "2".into() };
        assert_eq!(get.message_ids(), ["m:1"]);
        assert!(Request::Accounts.message_ids().is_empty() && Request::Accounts.mail_account().is_none());
    }

    #[test]
    fn search_answers_read_and_write_their_new_parts() {
        let sort: MailSort = serde_json::from_str(r#""relevance""#).expect("a sort");
        assert_eq!((sort, MailSort::default()), (MailSort::Relevance, MailSort::Date));
        let highlight = MailHighlight {
            id: "m:1".into(),
            subject: vec![
                HighlightSpan { text: "Ihre ".into(), hit: false },
                HighlightSpan { text: "Stromrechnung".into(), hit: true },
            ],
            snippet: Vec::new(),
        };
        let json = serde_json::to_value(&highlight).expect("json");
        assert_eq!(
            json,
            serde_json::json!({"id": "m:1", "subject": [{"text": "Ihre "}, {"text": "Stromrechnung", "hit": true}]})
        );
        assert_eq!(serde_json::from_value::<MailHighlight>(json).expect("parses"), highlight);
        let progress = MailIndexProgress {
            account: "mail:a@b.c".into(),
            messages: 10,
            indexed: 4,
            text_unavailable: 0,
            state: IndexState::Waiting,
        };
        let json = serde_json::to_value(&progress).expect("json");
        assert_eq!(json["state"], "waiting");
        assert_eq!(serde_json::to_value(IncompleteReason::Indexing).expect("json"), "indexing");
    }

    #[test]
    fn move_targets_are_a_folder_or_a_role() {
        let to: MoveTarget = serde_json::from_str(r#"{"role":"trash"}"#).expect("a role");
        assert_eq!(to, MoveTarget::Role(MoveRole::Trash));
        assert_eq!(MoveRole::Trash.role(), Role::Trash);
        let to: MoveTarget = serde_json::from_str(r#"{"folder":"f:1"}"#).expect("a folder");
        assert_eq!(to, MoveTarget::Folder("f:1".into()));
        assert!(serde_json::from_str::<MoveTarget>(r#"{"role":"sent"}"#).is_err(), "sent is filled by sending");
    }
}
