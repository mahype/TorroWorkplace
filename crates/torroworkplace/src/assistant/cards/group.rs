//! The group card (issue #786, `assistant.show_items`, "Grenzen" in `docs/inhaltskarten.md`): several items a
//! command of the conversation found, each shown as it would be alone.
//!
//! Strand 2f of issue #784 adds what `assistant.show_items` checks and writes; looking a group up is already whole.

use torro_ipc::api::Area;
use torro_ipc::assistant::{Card, ItemRef, MAX_SHOWN_ITEMS};

use super::Lookup;

/// What the group of `items` of the area `area` shows now: [`Card::Group`] with each item looked up alone
/// ([`super::look_item`]; one out of reach is [`Card::Unavailable`], a kind that may not stand in a group as well), at
/// most [`MAX_SHOWN_ITEMS`] in their order, and `total` as it was stored (the core set it from the search, never a
/// model).
pub async fn look(lookup: &Lookup<'_>, items: &[ItemRef], area: Area, total: Option<u64>) -> Card {
    let mut shown = Vec::with_capacity(items.len().min(MAX_SHOWN_ITEMS));
    for item in items.iter().take(MAX_SHOWN_ITEMS) {
        shown.push(super::look_item(lookup, item).await);
    }
    Card::Group { items: shown, area, total }
}
