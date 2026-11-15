//! The contact card (issue #789, `docs/inhaltskarten.md` "Kontakt"): a contact the assistant found, as the address
//! book holds it now.
//!
//! Filled by strand 2d of issue #784. Until then every contact card is [`Card::Unavailable`].

use torro_ipc::assistant::Card;

use super::Lookup;

/// What the contact `id` shows now: [`Card::Contact`] with the contact as `contacts.list_contacts` lists it (name,
/// company, the main address and number, its photo's reference for `contacts.get_photo`).
///
/// Must: look only in address books within [`Lookup::rights`] (`Rights::address_books`, the hidden ones excepted);
/// answer [`Card::Unavailable`] for a contact gone or out of reach. Logs no name.
pub async fn look(lookup: &Lookup<'_>, id: &str) -> Card {
    // Strand 2d (issue #789) reads the contact here.
    let _ = (lookup.contacts, lookup.reach(), id);
    Card::Unavailable
}
