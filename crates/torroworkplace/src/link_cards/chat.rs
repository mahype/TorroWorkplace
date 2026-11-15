//! Messages of the own server's chat (issue #790, `docs/inhaltskarten.md` "Zitat einer Chat-Nachricht"): their links,
//! and – with strand 2c of issue #784 – their cards (author, conversation, time, the start of the message), only when
//! the reader may read that conversation; otherwise no card and no text.

use super::LinkKind;

/// What `url` on the own server `own` names as a chat message: `/call/<token>#message_<id>` (also below
/// `/index.php/`); `None` for any other address. `url` is on `own` already ([`super::classify`] checked).
///
/// Strand 2c (issue #790) fills it in; until then no address is a message.
pub fn parse(url: &url::Url, own: &str) -> Option<LinkKind> {
    let _ = (url, own);
    None
}
