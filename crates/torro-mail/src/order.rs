//! In which order a list comes by arrival (task E8.15, issue #116): newest or oldest first. Shared
//! by `mail.search_messages` (by a message's date) and `inbox.list_items` (by an entry's last
//! change), so the mail window and the inbox sort the same way. It lives here, the lowest crate
//! both build on; `torro-inbox` and `torro-ipc` re-export it.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The order of a list by arrival. On the wire `newest_first` / `oldest_first`; a request without
/// it lists newest first, as before the field existed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Order {
    /// The most recent first: what just came stands at the top.
    #[default]
    NewestFirst,
    /// The oldest first: the order things came in, to work through them.
    OldestFirst,
}

impl Order {
    /// Both orders with their word, as `ui.json` stores them ([`Order::word`]).
    pub const ALL: [(&'static str, Order); 2] =
        [("newest-first", Order::NewestFirst), ("oldest-first", Order::OldestFirst)];

    /// The other order: what the switch in a list's header turns to.
    pub fn toggled(self) -> Order {
        match self {
            Order::NewestFirst => Order::OldestFirst,
            Order::OldestFirst => Order::NewestFirst,
        }
    }

    /// The word of the order (`newest-first`, `oldest-first`), for files that remember it.
    pub fn word(self) -> &'static str {
        match self {
            Order::NewestFirst => "newest-first",
            Order::OldestFirst => "oldest-first",
        }
    }

    /// The order of `word` ([`Order::word`]); `None` for any other word.
    pub fn parse(word: &str) -> Option<Order> {
        Order::ALL.iter().find(|(name, _)| *name == word).map(|(_, order)| *order)
    }

    /// `newest` with this order applied to a comparison made newest first: reversed for
    /// [`Order::OldestFirst`]. Ties stay ties, so a caller's tie-break keeps its direction.
    pub fn apply(self, newest: std::cmp::Ordering) -> std::cmp::Ordering {
        match self {
            Order::NewestFirst => newest,
            Order::OldestFirst => newest.reverse(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cmp::Ordering;

    use super::*;

    #[test]
    fn the_order_is_newest_first_unless_asked() {
        assert_eq!(Order::default(), Order::NewestFirst);
        assert_eq!(serde_json::to_string(&Order::OldestFirst).expect("serializes"), "\"oldest_first\"");
        let read: Order = serde_json::from_str("\"newest_first\"").expect("deserializes");
        assert_eq!(read, Order::NewestFirst);
    }

    #[test]
    fn words_toggles_and_comparisons() {
        for (word, order) in Order::ALL {
            assert_eq!(Order::parse(word), Some(order));
            assert_eq!(order.word(), word);
            assert_eq!(order.toggled().toggled(), order);
        }
        assert_eq!(Order::parse("newest"), None);
        assert_eq!(Order::NewestFirst.apply(Ordering::Less), Ordering::Less);
        assert_eq!(Order::OldestFirst.apply(Ordering::Less), Ordering::Greater);
        assert_eq!(Order::OldestFirst.apply(Ordering::Equal), Ordering::Equal);
    }
}
