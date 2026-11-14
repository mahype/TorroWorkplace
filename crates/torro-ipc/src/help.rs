//! The suite's built-in help on the wire (issue #702): what `system.get_help` answers.
//!
//! The help texts are short English articles for people, one per topic, built into the core
//! (`crates/torroworkplace/assets/help`). Without a topic the command lists them
//! ([`HelpTopicSummary`]), with one it answers its text ([`HelpTopic`]). They hold no paths and
//! nothing from "Was nie nach außen geht" in `docs/api.md`, so any signed-in client may read them.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One help topic in the list of `system.get_help` without a topic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HelpTopicSummary {
    /// The topic's id, as `system.get_help` takes it, e.g. `mail`.
    pub id: String,
    /// The topic's title, e.g. "Mail".
    pub title: String,
    /// One sentence on what the topic covers.
    pub summary: String,
}

/// One help topic with its text, the answer of `system.get_help` with a topic.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct HelpTopic {
    /// The topic's id, e.g. `mail`.
    pub id: String,
    /// The topic's title, e.g. "Mail".
    pub title: String,
    /// The topic's text, Markdown in English.
    pub text: String,
}
