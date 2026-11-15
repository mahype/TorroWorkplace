//! The mail card (issue #786, `docs/inhaltskarten.md` "E-Mail"): a mail the assistant found or read, as the mail
//! cache holds it now.
//!
//! Filled by strand 2d of issue #784. Until then every mail card is [`Card::Unavailable`].

use torro_ipc::assistant::Card;

use super::Lookup;

/// What the mail `id` of the account `account` shows now: [`Card::Mail`] with the message as the mail cache lists it
/// (`MailService`, the same summary `mail.get_message` starts from) and `untrusted: true`, never a picture from the
/// net.
///
/// Must: read nothing unless the conversation's client may read mail of `account` now (`crate::core::reads_mail`
/// with [`Lookup::reach`]); answer [`Card::Unavailable`] for a mail that is gone, moved out of reach, of an account the
/// client may not read, or that cannot be read now – without saying why. Logs no subject, sender or text.
pub async fn look(lookup: &Lookup<'_>, account: &str, id: &str) -> Card {
    // Strand 2d (issue #786) reads the mail here.
    let _ = (lookup.mail, lookup.reach(), account, id);
    Card::Unavailable
}
