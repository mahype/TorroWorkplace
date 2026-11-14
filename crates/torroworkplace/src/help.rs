//! The suite's built-in help (issue #702, `docs/assistent.md` "Hilfe zur Suite"): short English
//! articles for people, one per topic, built into the program from `assets/help/*.md`, and the
//! answer of `system.get_help`.
//!
//! The texts are written for people who use the suite, not translated from `docs/` (which are German
//! concepts for developers). They name ways through the interface as its English labels say them,
//! and hold no paths, no names of commands kept from agents and nothing about where keys are kept
//! (the tests check it), so every signed-in client, the assistant and its bots may read them.

use torro_ipc::Response;
use torro_ipc::help::{HelpTopic, HelpTopicSummary};

/// At most this many characters in one topic's text: enough for one area, short enough for a
/// model to read whole.
#[cfg(test)]
const MAX_TOPIC_CHARS: usize = 4_000;

/// One topic: its id, title, one sentence on what it covers, and its text.
struct Topic {
    id: &'static str,
    title: &'static str,
    summary: &'static str,
    text: &'static str,
}

/// Every topic, in the order the list shows them.
const TOPICS: &[Topic] = &[
    Topic {
        id: "overview",
        title: "Overview",
        summary: "What Torro Workplace is, its apps, the tray icon and where the settings are.",
        text: include_str!("../assets/help/overview.md"),
    },
    Topic {
        id: "workspaces",
        title: "Workspaces",
        summary: "Saving and loading workspaces of the window manager as templates, and their keys.",
        text: include_str!("../assets/help/workspaces.md"),
    },
    Topic {
        id: "accounts",
        title: "Accounts",
        summary: "Adding and removing accounts: Nextcloud, mail, GitHub and AI providers.",
        text: include_str!("../assets/help/accounts.md"),
    },
    Topic {
        id: "mail",
        title: "Mail",
        summary: "Reading and writing mail, several mail accounts, filters, signatures and backups.",
        text: include_str!("../assets/help/mail.md"),
    },
    Topic {
        id: "calendar-and-tasks",
        title: "Calendar and tasks",
        summary: "Calendars and task lists from Nextcloud, GitHub issues as tasks, reminders.",
        text: include_str!("../assets/help/calendar-and-tasks.md"),
    },
    Topic {
        id: "inbox",
        title: "Inbox",
        summary: "The inbox that gathers mail, notifications and more, its rules and muting.",
        text: include_str!("../assets/help/inbox.md"),
    },
    Topic {
        id: "contacts",
        title: "Contacts",
        summary: "Address books from Nextcloud and how the suite uses them.",
        text: include_str!("../assets/help/contacts.md"),
    },
    Topic {
        id: "chat",
        title: "Chat",
        summary: "Chatting through Nextcloud Talk and your status.",
        text: include_str!("../assets/help/chat.md"),
    },
    Topic {
        id: "assistant",
        title: "Assistant",
        summary: "The assistant: models, what it may use, its profile and memory, voice, bots and approvals.",
        text: include_str!("../assets/help/assistant.md"),
    },
    Topic {
        id: "agents-and-mcp",
        title: "Agents and MCP",
        summary: "Letting other programs and AI agents use the suite through the API and MCP, with rights you set.",
        text: include_str!("../assets/help/agents-and-mcp.md"),
    },
];

/// The topics, without their texts.
pub fn list() -> Vec<HelpTopicSummary> {
    TOPICS
        .iter()
        .map(|topic| HelpTopicSummary { id: topic.id.into(), title: topic.title.into(), summary: topic.summary.into() })
        .collect()
}

/// The topic `id` with its text; `None` for an id no topic has.
pub fn get(id: &str) -> Option<HelpTopic> {
    TOPICS.iter().find(|topic| topic.id == id.trim()).map(|topic| HelpTopic {
        id: topic.id.into(),
        title: topic.title.into(),
        text: topic.text.trim().into(),
    })
}

/// The answer of `system.get_help`: the list without `topic`, the topic with it, `not-found` for an
/// unknown one. The message does not repeat what was asked: it goes to the log of the caller.
pub fn answer(topic: Option<&str>) -> Response {
    match topic {
        None => Response::HelpTopics { topics: list() },
        Some(id) => match get(id) {
            Some(topic) => Response::HelpTopic { topic },
            None => Response::Error {
                code: "not-found".into(),
                message: "no such help topic; system.get_help without a topic lists them".into(),
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn every_topic_has_a_unique_id_a_title_and_a_text_within_its_length() {
        let ids: BTreeSet<&str> = TOPICS.iter().map(|topic| topic.id).collect();
        assert_eq!(ids.len(), TOPICS.len(), "ids are unique");
        for topic in TOPICS {
            assert!(
                !topic.id.is_empty() && topic.id.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
                "{}",
                topic.id
            );
            assert!(!topic.title.trim().is_empty() && !topic.summary.trim().is_empty(), "{}", topic.id);
            let text = topic.text.trim();
            assert!(text.starts_with("# "), "{}: a text starts with its heading", topic.id);
            assert!(text.chars().count() > 300, "{}: not empty", topic.id);
            assert!(text.chars().count() <= MAX_TOPIC_CHARS, "{}: {} characters", topic.id, text.chars().count());
        }
    }

    /// Erledigt 5 of issue #702: no paths to secrets and nothing of "Was nie nach außen geht".
    #[test]
    fn no_text_names_paths_secrets_or_what_never_goes_out() {
        let forbidden = [
            "clients.",
            "clients_",
            "approvals.decide",
            "approvals_decide",
            "system.quit",
            "system_quit",
            "~/",
            "/home/",
            "/Users/",
            "<config>",
            "<data>",
            "<state>",
            ".json",
            ".toml",
            ".key",
            "keychain",
            "keyring",
            "app key",
            "torro_app_",
            "torro_cli_",
        ];
        for topic in TOPICS {
            let text = format!("{} {} {}", topic.title, topic.summary, topic.text).to_lowercase();
            for word in forbidden {
                assert!(!text.contains(&word.to_lowercase()), "{} names {word:?}", topic.id);
            }
        }
    }

    #[test]
    fn the_command_lists_the_topics_and_answers_one() {
        match answer(None) {
            Response::HelpTopics { topics } => {
                assert_eq!(topics.len(), TOPICS.len());
                assert_eq!(topics[0].id, "overview");
            }
            other => panic!("the list, got {other:?}"),
        }
        match answer(Some("mail")) {
            Response::HelpTopic { topic } => {
                assert_eq!((topic.id.as_str(), topic.title.as_str()), ("mail", "Mail"));
                assert!(topic.text.starts_with("# Mail"), "{}", topic.text);
            }
            other => panic!("the topic, got {other:?}"),
        }
        match answer(Some("passwords-and-keys")) {
            Response::Error { code, message } => {
                assert_eq!(code, "not-found");
                assert!(!message.contains("passwords"), "the message does not repeat the input");
            }
            other => panic!("not-found, got {other:?}"),
        }
    }
}
