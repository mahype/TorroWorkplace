//! The link card of the assistant (issue #785, #790, `docs/inhaltskarten.md` "Webseite", "Datenschutz"): a web
//! address in an answer, as the own Nextcloud resolves it, or the question whether its provider's content may load.
//!
//! Filled by strand 2b of issue #784 (web pages, Settings › Privacy, "Show once") with the kinds of strand 2c (files
//! of the own server, quoted chat messages, GitHub), all through [`crate::link_cards`]. Until then a link card stays
//! a link ([`Card::Link`] with neither `card` nor `ask`), unless a person had it shown once.

use std::collections::HashMap;
use std::sync::Mutex;

use torro_ipc::assistant::{Card, LinkCard, LinkOrigin};

use super::Lookup;

/// The link cards a person had shown once (`assistant.show_card`, "Show once"), by conversation and entry number:
/// in memory only, so after a restart the question comes again. The core holds one ([`Lookup::shown`]); Settings ›
/// Privacy is never changed by it.
#[derive(Debug, Default)]
pub struct Shown {
    cards: Mutex<HashMap<(String, u64), LinkCard>>,
}

impl Shown {
    /// The card shown once for the entry `seq` of `conversation`, if any. Strand 2b adds keeping one (with a limit
    /// of how many are held) for `assistant.show_card`.
    pub fn get(&self, conversation: &str, seq: u64) -> Option<LinkCard> {
        let cards = self.cards.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        cards.get(&(conversation.to_owned(), seq)).cloned()
    }
}

/// What the link card of the entry `seq` for the address `url` shows now ([`Card::Link`]).
///
/// Must: a card shown once ([`Shown`]) shows as it was fetched. Otherwise the address is classified
/// ([`crate::link_cards::classify`] with the own Nextcloud of [`Lookup::chat`]): a link to the own server loads by
/// itself, without Settings › Privacy, within [`Lookup::rights`] (strand 2c; out of reach it is [`Card::Unavailable`]
/// for a quote, a web page otherwise); a page of another server loads by itself only when `from` is
/// [`LinkOrigin::Person`] or [`LinkOrigin::Read`] and Settings › Privacy decides `Load`; `Ask` gives `ask` with the
/// provider, and so does every address with [`LinkOrigin::Model`], whatever the level ("Always show" too); `Skip`
/// (blocked, "Never show") and a page the server knows no card for give neither. Without a Nextcloud account it
/// stays a link. Never fetches the page itself, logs no address.
pub async fn look(lookup: &Lookup<'_>, seq: u64, url: &str, from: LinkOrigin) -> Card {
    if let Some(card) = lookup.shown.get(lookup.conversation, seq) {
        return Card::Link { url: url.to_owned(), card: Some(Box::new(card)), ask: None };
    }
    // Strand 2b (issue #785) resolves it here, strand 2c (issue #790) the kinds of the own server and GitHub.
    let _ = (lookup.chat, lookup.github, lookup.reach(), from);
    Card::Link { url: url.to_owned(), card: None, ask: None }
}
