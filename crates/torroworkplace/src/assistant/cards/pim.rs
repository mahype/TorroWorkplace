//! The event and task cards (issue #797, `docs/inhaltskarten.md` "Termin", "Aufgabe"): an item the assistant saved
//! or found, as the calendar and the task lists hold it now.
//!
//! Filled by strand 2e of issue #784. Until then every event and task card is [`Card::Unavailable`].

use torro_ipc::assistant::Card;
use torro_pim::EventTime;

use super::Lookup;

/// What the event `uid` (one occurrence of a series with `recurrence_id`) shows now: [`Card::Event`] with the event,
/// its calendar's name and colour, and at most two other events at the same time in the calendars the conversation's
/// client may read (`overlaps`, the earliest first), the rest counted in `more_overlaps`.
///
/// Must: look only in calendars within [`Lookup::rights`] (a calendar out of reach is not there, its events neither
/// as the event nor as an overlap); without `recurrence_id` for a series the next occurrence from now; answer
/// [`Card::Unavailable`] for an event gone or out of reach. Logs no title.
pub async fn look_event(lookup: &Lookup<'_>, uid: &str, recurrence_id: Option<&EventTime>) -> Card {
    // Strand 2e (issue #797) reads the event here.
    let _ = (lookup.pim, lookup.reach(), uid, recurrence_id);
    Card::Unavailable
}

/// What the task `uid` shows now: [`Card::Todo`] with the task and its list's name and colour.
///
/// Must: look only in task lists within [`Lookup::rights`]; answer [`Card::Unavailable`] for a task gone or out of
/// reach. Logs no title.
pub async fn look_todo(lookup: &Lookup<'_>, uid: &str) -> Card {
    // Strand 2e (issue #797) reads the task here.
    let _ = (lookup.pim, lookup.reach(), uid);
    Card::Unavailable
}
