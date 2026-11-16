//! The draft card (issue #797, `docs/inhaltskarten.md` "Mail-Entwurf"): a draft the assistant saved, as it is saved
//! now, with the open approval of sending exactly that state.
//!
//! The draft is read back as `mail.get_draft` reads it for the conversation's client: only one it wrote itself
//! (`crate::mail::store::DraftAuthor`), in an account within its rights. The approval is the open one of
//! `mail.send_draft` for this draft – its request names this draft's id, so a second draft that reads the same never
//! takes it – that this client asked for, and only while it still sums up the draft as it is
//! saved now: the core words the summary and its details from the stored draft ([`crate::approvals::Facts`], the
//! rule `draft_sent` in `approvals.rs`), so a draft saved again with other recipients, subject, text or attachments no
//! longer matches what the person was asked, and the card shows the new state without the approval. Should a change
//! leave the summary as it was (only the HTML), the approval still carries the fingerprint of the message the person
//! was shown, and the core sends nothing on it (`conflict`, `crate::mail::send::send_draft`).
//!
//! Nothing here logs a subject, a recipient or text.

use chrono::Utc;
use torro_ipc::api::Rights;
use torro_ipc::assistant::Card;
use torro_ipc::{ApprovalInfo, Request, Response};

use super::Lookup;
use crate::approvals::{Approvals, Facts};
use crate::mail::service::MailService;
use crate::mail::store::DraftAuthor;
use crate::pim::Reach;

/// What the draft `id` shows now: [`Card::Draft`] with the draft as `mail.get_draft` reads it back, and `approval`
/// set to the id of the open approval of `mail.send_draft` for it.
///
/// Finds only a draft the conversation's client wrote, in an account within [`Lookup::rights`], and only while the
/// client may read it back (`mail.get_draft`); takes `approval` only while the approval is pending and sums up this
/// state of the draft (see the module docs) – changed since, the card shows the new state without `approval`; answers
/// [`Card::Unavailable`] for a draft sent, deleted, out of reach or not readable now. Logs no subject, recipient or
/// text.
pub async fn look(lookup: &Lookup<'_>, id: &str) -> Card {
    draft_card(lookup.mail, lookup.approvals, &lookup.client.id, lookup.rights, id).await
}

/// [`look`] for the client `client` with `rights`, on `mail` and `approvals`.
async fn draft_card(mail: &MailService, approvals: &Approvals, client: &str, rights: &Rights, id: &str) -> Card {
    if !rights.allows_request(&Request::GetDraft { id: id.to_string() }) {
        return Card::Unavailable;
    }
    let (reach, author) = (Reach::Client(rights), DraftAuthor::Client(client.to_string()));
    let draft = match crate::mail::send::get_draft(mail, reach, &author, id).await {
        Response::MailDraft { draft } => draft,
        _ => return Card::Unavailable,
    };
    let approval = open_approval(mail, approvals, client, reach, id).await;
    Card::Draft { draft: Box::new(draft), approval }
}

/// The id of the pending approval of `mail.send_draft` for the draft `id` (its request names exactly this draft) that
/// `client` asked for and that sums up the draft as it is saved now ([`matching`]); `None` without one. Reads the
/// draft from the server a second time only while such an approval waits.
async fn open_approval(
    mail: &MailService,
    approvals: &Approvals,
    client: &str,
    reach: Reach<'_>,
    id: &str,
) -> Option<String> {
    let send = Request::SendDraft { id: id.to_string() };
    let now = Utc::now();
    let waiting = approvals.open_with_requests(Some(client), now).await.ok()?;
    if !waiting.iter().any(|(_, request)| *request == send) {
        return None;
    }
    // What the core would show the person for this draft now: summary and details from the stored draft, as
    // `Core::ask_approval` writes them when the approval is opened.
    let author = DraftAuthor::Client(client.to_string());
    let (facts, _) = crate::mail::send::approval(mail, reach, &author, id).await.ok()?;
    let facts = Facts { mail: Some(facts), ..Facts::default() };
    let shown = (crate::approvals::summary(&send, &facts), crate::approvals::details(&send, &facts));
    matching(&waiting, &send, &shown).map(|approval| approval.id.clone())
}

/// The newest of `waiting` (open approvals with their requests, newest first) that waits for a person on exactly
/// `send` (`mail.send_draft` of this draft's id, not merely the same command) and shows what `shown` (summary,
/// details) says: the approval of exactly this state of this draft.
fn matching<'a>(
    waiting: &'a [(ApprovalInfo, Request)],
    send: &Request,
    shown: &(String, Vec<String>),
) -> Option<&'a ApprovalInfo> {
    let fits = |(approval, request): &&(ApprovalInfo, Request)| {
        approval.state.is_pending() && request == send && approval.summary == shown.0 && approval.details == shown.1
    };
    waiting.iter().find(fits).map(|(approval, _)| approval)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use torro_ipc::api::{Area, AreaRights, Level, Outbound, Selection};
    use torro_mail_imap::smtp::Submitted;

    use super::*;
    use crate::account::github::tests::MemoryTokens;
    use crate::approvals::{Notifier, Origin};
    use crate::mail::send::tests::FakeSmtp;
    use crate::mail::service::tests::service_in;
    use crate::mail::sync::fake::FakeImap;
    use crate::mail::write::save_draft;
    use crate::mail::write::tests::{draft, mail_server, saved};

    const ASSISTANT: &str = "assistant";

    /// Anna's mail account on a fake IMAP server, synced, a fake SMTP server that accepts, and approvals in a
    /// temporary file.
    struct Room {
        dir: tempfile::TempDir,
        _server: FakeImap,
        mail: MailService,
        smtp: Arc<FakeSmtp>,
        approvals: Approvals,
    }

    async fn room() -> Room {
        let dir = tempfile::tempdir().expect("temp dir");
        let secrets = MemoryTokens::default();
        let server = mail_server(dir.path(), &secrets).await;
        let mail = service_in(dir.path(), &secrets);
        mail.reload().await;
        mail.sync_all(Reach::Suite).await.expect("synced");
        let smtp = FakeSmtp::answering(Submitted::Accepted);
        mail.outbox().await.expect("an outbox").use_fake(Arc::clone(&smtp));
        let approvals = Approvals::new(Some(dir.path().join("approvals.json")), Notifier::asking(|_| {}));
        Room { dir, _server: server, mail, smtp, approvals }
    }

    /// The assistant's rights with all of mail, sending with approval.
    fn rights() -> Rights {
        let mut rights =
            Rights { mail_accounts: Selection::All, outbound: Outbound::WithApproval, ..Rights::assistant() };
        rights.areas.insert(Area::Mail, AreaRights { level: Level::Full, admin: false });
        rights
    }

    /// The draft `subject` to Bernd saved by `author` (replacing `id` when given); its id.
    async fn saved_by(room: &Room, author: DraftAuthor, subject: &str, id: Option<String>) -> String {
        let draft = torro_mail::Draft { id, ..draft(subject) };
        saved(save_draft(&room.mail, Reach::Suite, draft, author).await).id.expect("an id")
    }

    /// Opens the approval of sending `id` for `client` as the core does (`Core::ask_approval`): summary and details
    /// from the stored draft, pinned to its fingerprint.
    async fn ask_to_send(room: &Room, client: &str, id: &str) -> ApprovalInfo {
        let send = Request::SendDraft { id: id.to_string() };
        let author = DraftAuthor::Client(client.to_string());
        let reach = Reach::Client(&rights());
        let (facts, pinned) = crate::mail::send::approval(&room.mail, reach, &author, id).await.expect("prepared");
        let facts = Facts { mail: Some(facts), ..Facts::default() };
        let shown = (crate::approvals::summary(&send, &facts), crate::approvals::details(&send, &facts));
        let kept = (None, Some(pinned), Origin::default());
        room.approvals.open_from(client, "Assistant", send, kept, shown, Utc::now()).await.expect("opened")
    }

    async fn card(room: &Room, client: &str, id: &str) -> Card {
        draft_card(&room.mail, &room.approvals, client, &rights(), id).await
    }

    fn approval_of(card: &Card) -> Option<&str> {
        match card {
            Card::Draft { approval, .. } => approval.as_deref(),
            other => panic!("a draft card, got {other:?}"),
        }
    }

    fn subject_of(card: &Card) -> &str {
        match card {
            Card::Draft { draft, .. } => &draft.subject,
            other => panic!("a draft card, got {other:?}"),
        }
    }

    /// Without an open approval the card has none, so it offers nothing that would send on the model's behalf; and
    /// looking it up sends nothing.
    #[tokio::test]
    async fn a_draft_without_an_approval_shows_without_one_and_nothing_is_sent() {
        let room = room().await;
        let id = saved_by(&room, DraftAuthor::Client(ASSISTANT.into()), "Lunch", None).await;
        let shown = card(&room, ASSISTANT, &id).await;
        assert_eq!(subject_of(&shown), "Lunch");
        assert_eq!(approval_of(&shown), None);
        assert!(room.smtp.sent().is_empty(), "nothing went out");
    }

    /// The open approval of exactly this state is on the card; saved again with another subject, the card shows
    /// the new state without it. A denied approval and another client's are never on it.
    #[tokio::test]
    async fn the_approval_holds_for_the_state_it_was_asked_for() {
        let room = room().await;
        let author = DraftAuthor::Client(ASSISTANT.into());
        let id = saved_by(&room, author.clone(), "Lunch", None).await;
        let asked = ask_to_send(&room, ASSISTANT, &id).await;
        assert_eq!(approval_of(&card(&room, ASSISTANT, &id).await), Some(asked.id.as_str()));

        saved_by(&room, author.clone(), "Lunch, and bring Eve", Some(id.clone())).await;
        let changed = card(&room, ASSISTANT, &id).await;
        assert_eq!(subject_of(&changed), "Lunch, and bring Eve", "the new state");
        assert_eq!(approval_of(&changed), None, "the approval was for the old state");

        // Asked again for the new state, then denied: gone from the card.
        let again = ask_to_send(&room, ASSISTANT, &id).await;
        assert_eq!(approval_of(&card(&room, ASSISTANT, &id).await), Some(again.id.as_str()));
        room.approvals.decide(&again.id, false, Utc::now()).await.expect("denied");
        assert_eq!(approval_of(&card(&room, ASSISTANT, &id).await), None, "denied");
        assert!(room.smtp.sent().is_empty(), "nothing went out");
    }

    /// Issue #797: two drafts that read the same each get only the approval of their own id, never the other's.
    #[tokio::test]
    async fn the_approval_belongs_to_one_draft_even_when_another_reads_the_same() {
        let room = room().await;
        let author = DraftAuthor::Client(ASSISTANT.into());
        let first = saved_by(&room, author.clone(), "Lunch", None).await;
        let second = saved_by(&room, author, "Lunch", None).await;
        assert_ne!(first, second);
        let asked = ask_to_send(&room, ASSISTANT, &first).await;
        assert_eq!(approval_of(&card(&room, ASSISTANT, &first).await), Some(asked.id.as_str()));
        assert_eq!(approval_of(&card(&room, ASSISTANT, &second).await), None, "the other draft's approval");
        let theirs = ask_to_send(&room, ASSISTANT, &second).await;
        assert_eq!(approval_of(&card(&room, ASSISTANT, &second).await), Some(theirs.id.as_str()));
        assert_eq!(approval_of(&card(&room, ASSISTANT, &first).await), Some(asked.id.as_str()), "still its own");
        assert!(room.smtp.sent().is_empty(), "nothing went out");
    }

    /// A draft of the person or of another client is no card of this client's, and neither is a draft gone.
    #[tokio::test]
    async fn another_writers_draft_or_one_gone_is_unavailable() {
        let room = room().await;
        let persons = saved_by(&room, DraftAuthor::App, "The person's", None).await;
        assert_eq!(card(&room, ASSISTANT, &persons).await, Card::Unavailable);
        let others = saved_by(&room, DraftAuthor::Client("bot".into()), "The bot's", None).await;
        assert_eq!(card(&room, ASSISTANT, &others).await, Card::Unavailable);
        assert_eq!(card(&room, ASSISTANT, "m:999").await, Card::Unavailable);
        let mut no_mail = rights();
        no_mail.areas.remove(&Area::Mail);
        let own = saved_by(&room, DraftAuthor::Client(ASSISTANT.into()), "Mine", None).await;
        let unread = draft_card(&room.mail, &room.approvals, ASSISTANT, &no_mail, &own).await;
        assert_eq!(unread, Card::Unavailable, "without mail rights");
        assert!(room.dir.path().exists());
    }
}
