//! The short notes the voice chat says while an answer takes a moment (issue #837): "One moment, I'm
//! checking your calendar." They name only the part of the suite a tool belongs to, never what it reads
//! (no mail, event or name of an extension), and are only spoken, never stored in the conversation.

use torro_core::i18n::tr;

/// What an answer does while it is announced: the part of the suite its tool belongs to, or thinking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Doing {
    Calendar,
    Mail,
    Tasks,
    Contacts,
    /// Any other tool: of the suite, the system, the web or an extension.
    Tool,
    /// No tool: the model thinks or writes.
    Thinking,
}

impl Doing {
    /// The part of the catalogue name `tool` before its first `.`; an extension's tool (`ext_…`) has
    /// none and counts as any other.
    fn of(tool: Option<&str>) -> Doing {
        let Some(tool) = tool else { return Doing::Thinking };
        match tool.split_once('.').map(|(area, _)| area) {
            Some("calendar") => Doing::Calendar,
            Some("mail" | "inbox") => Doing::Mail,
            Some("tasks") => Doing::Tasks,
            Some("contacts") => Doing::Contacts,
            _ => Doing::Tool,
        }
    }

    /// Three ways to say it, short enough that an answer coming meanwhile does not wait long.
    fn versions(self) -> [&'static str; 3] {
        match self {
            Doing::Calendar => [
                tr("One moment, I'm checking your calendar."),
                tr("Let me look at your calendar."),
                tr("Checking your calendar."),
            ],
            Doing::Mail => {
                [tr("One moment, I'm checking your mail."), tr("Let me look at your mail."), tr("Checking your mail.")]
            }
            Doing::Tasks => [
                tr("One moment, I'm checking your tasks."),
                tr("Let me look at your tasks."),
                tr("Checking your tasks."),
            ],
            Doing::Contacts => [
                tr("One moment, I'm checking your contacts."),
                tr("Let me look at your contacts."),
                tr("Checking your contacts."),
            ],
            Doing::Tool => [tr("One moment, I'm looking that up."), tr("Let me check that."), tr("I'm on it.")],
            Doing::Thinking => [tr("Let me think."), tr("One moment, please."), tr("Give me a second.")],
        }
    }
}

/// The note for an answer that uses `tool` (its catalogue name), or thinks with `None`; `turn` counts
/// the notes said so far, so one follows the other in turn instead of the same one each time.
pub fn announcement(tool: Option<&str>, turn: usize) -> &'static str {
    let versions = Doing::of(tool).versions();
    versions[turn % versions.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each part of the suite has its own notes, three different ones in turn; any other tool, also an
    /// extension's, the general ones, and no tool the ones for thinking.
    #[test]
    fn each_part_has_three_notes_in_turn() {
        let calendar = announcement(Some("calendar.list_events"), 0);
        assert_eq!(calendar, tr("One moment, I'm checking your calendar."));
        assert_eq!(announcement(Some("mail.search"), 1), tr("Let me look at your mail."));
        assert_eq!(announcement(Some("inbox.list_entries"), 2), tr("Checking your mail."));
        assert_eq!(announcement(Some("tasks.list_todos"), 0), tr("One moment, I'm checking your tasks."));
        assert_eq!(announcement(Some("contacts.search"), 0), tr("One moment, I'm checking your contacts."));
        for tool in ["ext_x_y", "tools.web_search", "system.ping", "calendar"] {
            assert_eq!(announcement(Some(tool), 0), tr("One moment, I'm looking that up."), "{tool}");
        }
        assert_eq!(announcement(None, 0), tr("Let me think."));
        for tool in [Some("calendar.x"), Some("mail.x"), Some("tasks.x"), Some("contacts.x"), Some("ext_x"), None] {
            let notes: std::collections::BTreeSet<_> = (0..3).map(|turn| announcement(tool, turn)).collect();
            assert_eq!(notes.len(), 3, "{tool:?}: three different ones");
            assert_eq!(announcement(tool, 3), announcement(tool, 0), "{tool:?}: then from the start");
        }
    }
}
