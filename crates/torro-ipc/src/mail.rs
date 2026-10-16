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

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use torro_mail::{DraftAttachment, Role};

use crate::{Request, Response};

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
    /// Of them, how many have their text in the index (or are known to have none to read).
    pub indexed: u64,
    /// What the core is doing about the rest.
    pub state: IndexState,
}

/// What the core does about the texts not yet in an account's index ([`MailIndexProgress`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexState {
    /// Every message is indexed.
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

/// The `error` answer for a mail request that breaks a limit, before anything is looked at:
/// `ids` empty or over [`MAX_IDS`], a search `limit` of 0 or over [`MAX_LIMIT`], a `query` over
/// [`MAX_QUERY_CHARS`] characters, a draft with more than [`MAX_RECIPIENTS`] recipients, more than
/// [`MAX_ATTACHMENTS`] attachments or a text over [`MAX_BODY_BYTES`], and any id, folder, cursor or
/// attachment part over [`MAX_ID_BYTES`]. `None` for every request within them and for every other
/// request. The core calls it before rights, approvals and handlers, so nobody is asked to approve
/// what would be refused anyway.
pub fn limit_refusal(request: &Request) -> Option<Response> {
    let refuse = |message: String| Some(Response::Error { code: "error".into(), message });
    if mail_ids(request).any(|id| id.len() > MAX_ID_BYTES) {
        return refuse(format!("an id, folder, cursor or attachment part may hold at most {MAX_ID_BYTES} bytes"));
    }
    match request {
        Request::SetMailFlags { ids, .. } | Request::MoveMail { ids, .. } | Request::DeleteMail { ids, .. } => {
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

/// Every id-like text of a mail request, for [`MAX_ID_BYTES`]: its account, messages, folder,
/// cursor and attachment part.
fn mail_ids(request: &Request) -> impl Iterator<Item = &str> {
    let more: Vec<&str> = match request {
        Request::UpdateMailAccount { id, .. } => vec![id],
        Request::SearchMail { folder, cursor, .. } => folder.iter().chain(cursor).map(String::as_str).collect(),
        Request::MoveMail { to: MoveTarget::Folder(folder), .. } => vec![folder],
        Request::GetMailAttachment { attachment, .. } => vec![attachment],
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

impl Request {
    /// The mail account a request names itself: `account` of `mail.list_folders`,
    /// `mail.search_messages` and `mail.index_status`, the draft's `account` of `mail.save_draft`. `None` for every other
    /// request, and when the field is left out (then the core takes every account the caller
    /// reaches). Messages and folders name their account only through the core's store
    /// ([`Request::message_ids`]).
    pub fn mail_account(&self) -> Option<&str> {
        match self {
            Request::MailFolders { account }
            | Request::SearchMail { account, .. }
            | Request::MailIndexStatus { account } => account.as_deref(),
            Request::SaveDraft { draft } => Some(draft.account.as_str()),
            _ => None,
        }
    }

    /// Every message id a request names, in order: `id`/`ids`, a draft's own id, the message it
    /// answers and the messages it forwards parts of. Empty for every other request. If one of
    /// them is in an account the caller does not reach, the whole call is `not-found` (checked
    /// before anything is read).
    pub fn message_ids(&self) -> Vec<&str> {
        match self {
            Request::GetMail { id, .. }
            | Request::GetMailThread { id }
            | Request::GetMailAttachment { id, .. }
            | Request::SendDraft { id } => vec![id.as_str()],
            Request::SetMailFlags { ids, .. } | Request::MoveMail { ids, .. } | Request::DeleteMail { ids, .. } => {
                ids.iter().map(String::as_str).collect()
            }
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
        }
    }

    fn refused(request: &Request) -> bool {
        matches!(limit_refusal(request), Some(Response::Error { code, .. }) if code == "error")
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

        assert!(refused(&search(0, None)) && refused(&search(MAX_LIMIT + 1, None)));
        assert_eq!(limit_refusal(&search(MAX_LIMIT, Some("ä".repeat(MAX_QUERY_CHARS)))), None, "characters, not bytes");
        assert!(refused(&search(10, Some("x".repeat(MAX_QUERY_CHARS + 1)))));
        assert_eq!(limit_refusal(&Request::Ping), None);

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
        let progress =
            MailIndexProgress { account: "mail:a@b.c".into(), messages: 10, indexed: 4, state: IndexState::Waiting };
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
