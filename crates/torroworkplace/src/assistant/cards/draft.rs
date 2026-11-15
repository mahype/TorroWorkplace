//! The draft card (issue #797, `docs/inhaltskarten.md` "Mail-Entwurf"): a draft the assistant saved, as it is saved
//! now, with the open approval of sending exactly that state.
//!
//! Filled by strand 2e of issue #784. Until then every draft card is [`Card::Unavailable`].

use torro_ipc::assistant::Card;

use super::Lookup;

/// What the draft `id` shows now: [`Card::Draft`] with the draft as `mail.get_draft` reads it back, and `approval`
/// set to the id of the open approval of `mail.send_draft` for it.
///
/// Must: find only a draft the conversation's client wrote (`crate::mail::store::DraftAuthor`, as `mail.get_draft`
/// for that client) in an account within [`Lookup::rights`]; take `approval` only while the approval is pending and
/// was asked for this state of the draft (`crate::approvals::Facts`, the rule `draft_sent` in `approvals.rs`) –
/// changed since, the card shows the new state without `approval`; answer [`Card::Unavailable`] for a draft sent,
/// deleted or out of reach. Logs no subject, recipient or text.
pub async fn look(lookup: &Lookup<'_>, id: &str) -> Card {
    // Strand 2e (issue #797) reads the draft and its approval here.
    let _ = (lookup.mail, lookup.approvals, lookup.client, lookup.reach(), id);
    Card::Unavailable
}
