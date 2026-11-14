//! The chat with the assistant apart from drawing, shared by every window that shows one (issue
//! #605): the assistant's own windows (`torroassistant`) and the mini chat about a mail in the mail
//! window (`torroinbox`, "Ask AI…"). Kept pure so it can be tested on its own:
//!
//! - [`Transcript`]: the entries of the open conversation, merged from every answer of
//!   `assistant.get_conversation` – one entry per number, and the answer the model is still writing
//!   (numbered `last + 1`, see `torro_ipc::assistant`) replaced each time it comes again.
//! - What answers: engines in the order of Open First ([`ordered`], [`is_local`]), only models that
//!   can call tools ([`tool_models`]), and what is chosen before the person picks
//!   ([`default_choice`], first the default of the control centre: [`preferred_choice`]).
//! - The words of a tool line ([`tool_title`], [`tool_outcome`], [`tool_line`]) and an answer as
//!   one plain line ([`answer_line`]).

use std::collections::HashSet;

use torro_core::ai::{AiModel, Effort, is_this_computer};
use torro_core::i18n::{tr, trf};
use torro_core::settings::AssistantDefault;
use torro_ipc::api::Area;
use torro_ipc::assistant::{ConversationSummary, Engine, EngineKind, Entry, EntryContent, Mode};

/// The entries of one conversation as the window has them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Transcript {
    /// The conversation's head as last answered; `None` before the first answer.
    pub summary: Option<ConversationSummary>,
    /// In order of their numbers, one per number; the last may be the answer the model is still
    /// writing (numbered after `summary.last`).
    pub entries: Vec<Entry>,
}

impl Transcript {
    /// A transcript from a full answer of `assistant.get_conversation` (without `after`).
    pub fn new(summary: ConversationSummary, mut entries: Vec<Entry>) -> Transcript {
        entries.sort_by_key(|entry| entry.seq);
        entries.dedup_by_key(|entry| entry.seq);
        Transcript { summary: Some(summary), entries }
    }

    /// The number of the newest stored entry: what the next `assistant.get_conversation` passes as
    /// `after`. 0 before the first answer.
    pub fn last(&self) -> u64 {
        self.summary.as_ref().map_or(0, |summary| summary.last)
    }

    /// Takes an answer of `assistant.get_conversation` with `after` = [`Transcript::last`], or of
    /// `assistant.send_message`: every entry replaces the one with its number or joins in order,
    /// and an entry after the new `last` that the answer no longer has is dropped – a growing
    /// answer that has since become something else, or was stopped.
    ///
    /// An answer older than what the window has (its `last` lower, from a request that overtook
    /// another) is ignored and `false` returned, so the window never goes back.
    pub fn merge(&mut self, summary: ConversationSummary, entries: Vec<Entry>) -> bool {
        if let Some(have) = &self.summary {
            if have.id != summary.id {
                *self = Transcript::new(summary, entries);
                return true;
            }
            if summary.last < have.last {
                return false;
            }
        }
        let fresh: HashSet<u64> = entries.iter().map(|entry| entry.seq).collect();
        for entry in entries {
            match self.entries.binary_search_by_key(&entry.seq, |have| have.seq) {
                Ok(at) => self.entries[at] = entry,
                Err(at) => self.entries.insert(at, entry),
            }
        }
        let last = summary.last;
        self.entries.retain(|entry| entry.seq <= last || fresh.contains(&entry.seq));
        self.summary = Some(summary);
        true
    }

    /// The answer the model is still writing: the entry after [`Transcript::last`].
    pub fn growing(&self) -> Option<&Entry> {
        let last = self.last();
        self.entries.last().filter(|entry| entry.seq > last)
    }

    /// An answer runs in the conversation (the core says so).
    pub fn running(&self) -> bool {
        self.summary.as_ref().is_some_and(|summary| summary.running)
    }

    /// The ids of the approvals the conversation waits for or waited for, in order.
    pub fn approvals(&self) -> Vec<&str> {
        self.entries
            .iter()
            .filter_map(|entry| match &entry.content {
                EntryContent::Approval { id } => Some(id.as_str()),
                _ => None,
            })
            .collect()
    }

    /// The numbers of the card entries (task WZ.8), in order: what `assistant.get_cards` resolves.
    pub fn cards(&self) -> Vec<u64> {
        self.entries
            .iter()
            .filter(|entry| matches!(entry.content, EntryContent::Card { .. }))
            .map(|entry| entry.seq)
            .collect()
    }

    /// The number of the card that comes right after the entry `seq` (a tool that made a file, task
    /// WZ.8); `None` when the next entry is no card or there is none.
    pub fn card_after(&self, seq: u64) -> Option<u64> {
        let next = self.entries.iter().find(|entry| entry.seq > seq)?;
        matches!(next.content, EntryContent::Card { .. }).then_some(next.seq)
    }

    /// The newest answer, the one still being written included: what the mini tile shows.
    pub fn last_answer(&self) -> Option<&Entry> {
        self.entries.iter().rev().find(|entry| matches!(entry.content, EntryContent::Answer { .. }))
    }

    /// The text of [`Transcript::last_answer`].
    pub fn last_answer_text(&self) -> Option<&str> {
        self.last_answer().and_then(|entry| match &entry.content {
            EntryContent::Answer { text, .. } => Some(text.as_str()),
            _ => None,
        })
    }

    /// The areas switched off for the newest message: a conversation opened again keeps them.
    pub fn last_without(&self) -> Option<Vec<Area>> {
        self.entries.iter().rev().find_map(|entry| match &entry.content {
            EntryContent::Message { without, .. } => Some(without.clone()),
            _ => None,
        })
    }

    /// The permission mode of the newest message (issue #132): a conversation opened again keeps it.
    pub fn last_mode(&self) -> Option<Mode> {
        self.entries.iter().rev().find_map(|entry| match &entry.content {
            EntryContent::Message { mode, .. } => Some(*mode),
            _ => None,
        })
    }
}

/// What answers the next message: an engine (`assistant.list_engines`) and, for an AI account, its
/// model (`ai.list_models`); `None` lets the engine pick one that can call tools.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub engine: String,
    pub model: Option<String>,
}

/// Whether `engine` is an AI account on this computer (Ollama, LM Studio, a server on
/// `localhost`): its id is `ai:<provider>@<host>`.
pub fn is_local(engine: &Engine) -> bool {
    engine.kind == EngineKind::Ai && engine.id.split_once('@').is_some_and(|(_, authority)| is_this_computer(authority))
}

/// `engines` in the order the picker lists them (Open First): AI accounts first, those on this
/// computer on top, then coding agents (none are offered since issue #219, the order stays for the
/// kind), the test engine last.
pub fn ordered(mut engines: Vec<Engine>) -> Vec<Engine> {
    engines.sort_by_key(|engine| match engine.kind {
        EngineKind::Ai if is_local(engine) => 0,
        EngineKind::Ai => 1,
        EngineKind::Agent => 2,
        EngineKind::Test => 3,
    });
    engines
}

/// The models the picker offers for an AI account: those that can call tools, which the assistant
/// needs to do anything in the suite. A model the server says nothing about (LM Studio, vLLM do
/// not tell) stays, as `ai.list_models` keeps it for open weights: only a model known to have no
/// tools is left out. The order stays.
pub fn tool_models(models: &[AiModel]) -> Vec<AiModel> {
    models.iter().filter(|model| model.tools != Some(false)).cloned().collect()
}

/// What answers before the person picks: the engine and model of the newest conversation if that
/// engine is still there ("the last one used"), else the first engine of [`ordered`] with its
/// first model that can call tools (`models` gives an engine's listed models, `None` while they
/// are not in), else nothing (no engine at all).
pub fn default_choice<'a>(
    conversations: &[ConversationSummary],
    engines: &[Engine],
    models: impl Fn(&str) -> Option<&'a [AiModel]>,
) -> Option<Choice> {
    let newest = conversations.iter().max_by_key(|conversation| conversation.updated);
    if let Some(conversation) = newest.filter(|c| engines.iter().any(|engine| engine.id == c.engine)) {
        return Some(Choice { engine: conversation.engine.clone(), model: conversation.model.clone() });
    }
    let engine = ordered(engines.to_vec()).into_iter().next()?;
    let model = models(&engine.id).and_then(|models| tool_models(models).into_iter().next()).map(|model| model.id);
    Some(Choice { engine: engine.id, model })
}

/// What answers before the person picks, with its effort (issue #131): the default set in the
/// control centre (`default`, `settings.ai_slots.chat`) while its engine is offered, else
/// [`default_choice`] with the effort of the newest conversation that engine answered in.
pub fn preferred_choice<'a>(
    default: Option<&AssistantDefault>,
    conversations: &[ConversationSummary],
    engines: &[Engine],
    models: impl Fn(&str) -> Option<&'a [AiModel]>,
) -> Option<(Choice, Option<Effort>)> {
    if let Some(default) = default.filter(|default| engines.iter().any(|engine| engine.id == default.engine)) {
        return Some((Choice { engine: default.engine.clone(), model: default.model.clone() }, default.effort));
    }
    let choice = default_choice(conversations, engines, models)?;
    let effort = conversations
        .iter()
        .filter(|conversation| conversation.engine == choice.engine && conversation.model == choice.model)
        .max_by_key(|conversation| conversation.updated)
        .and_then(|conversation| conversation.effort);
    Some((choice, effort))
}

/// What a tool did, in the past tense, for its collapsed line ("Searched mail"): known commands by
/// name, any other by its catalogue name, so nothing is hidden.
pub fn tool_title(name: &str) -> String {
    let known = match name {
        "calendar.list_calendars" => tr("Looked up calendars"),
        "calendar.list_events" => tr("Looked up events"),
        "calendar.save_event" => tr("Saved event"),
        "calendar.delete_event" => tr("Deleted event"),
        "tasks.list_lists" => tr("Looked up task lists"),
        "tasks.list_todos" => tr("Looked up tasks"),
        "tasks.save_todo" => tr("Saved task"),
        "tasks.delete_todo" => tr("Deleted task"),
        "tasks.move_todo" => tr("Moved task"),
        "inbox.list_items" => tr("Looked at the Notification Hub"),
        "inbox.get_item" => tr("Read a Notification Hub entry"),
        "inbox.post_item" => tr("Posted to the Notification Hub"),
        "inbox.set_item_done" => tr("Marked entry done"),
        "inbox.create_task_from_item" => tr("Made a task"),
        "mail.list_folders" => tr("Looked up mail folders"),
        "mail.search_messages" => tr("Searched mail"),
        "mail.get_message" => tr("Read mail"),
        "mail.get_thread" => tr("Read conversation"),
        // Issue #605: the chat about a mail reads a suggestion out of it.
        "mail.extract_action" => tr("Read a suggestion out of the mail"),
        "mail.save_draft" => tr("Saved draft"),
        "mail.send_draft" => tr("Sent mail"),
        "mail.get_draft" => tr("Read draft"),
        "mail.move_messages" => tr("Moved mail"),
        "mail.delete_messages" => tr("Deleted mail"),
        "workspaces.list_templates" => tr("Looked up templates"),
        "workspaces.load_template" => tr("Loaded template"),
        "workspaces.save_template" => tr("Saved template"),
        "bots.list_bots" => tr("Looked up bots"),
        "assistant.my_access" => tr("Looked up its access"),
        "bots.save_bot" => tr("Suggested a bot"),
        "tools.create_pdf" => tr("Created PDF"),
        "tools.list_tools" => tr("Listed tools"),
        "tools.open_file" => tr("Opened file"),
        "tools.web_search" => tr("Searched the web"),
        "tools.fetch_page" => tr("Read web page"),
        "tools.list_search_providers" => tr("Listed search providers"),
        "tools.list_folder" => tr("Listed folder"),
        "tools.find_files" => tr("Found files"),
        "tools.create_folder" => tr("Made folder"),
        "tools.copy_file" => tr("Copied file"),
        "tools.move_file" => tr("Moved file"),
        "tools.trash_file" => tr("Moved to trash"),
        // Issue #705: the tool `file_contents`, never the path or the text searched for.
        "tools.read_file" => tr("Read file"),
        "tools.search_in_files" => tr("Searched in files"),
        "tools.write_file" => tr("Wrote file"),
        "tools.edit_file" => tr("Changed file"),
        "find_tools" => tr("Looked for tools"),
        other => return other.to_owned(),
    };
    known.to_owned()
}

/// What came of a tool, after its title: the core's few words ("12 results"), or for an error
/// code a word people read ("No access", "Denied"). The core's summary repeats the error then, so
/// only the word shows.
pub fn tool_outcome(summary: &str, error: Option<&str>) -> String {
    match error {
        None => summary.to_owned(),
        Some("forbidden") => tr("No access").to_owned(),
        Some("not-found") => tr("Not found").to_owned(),
        Some("denied") => tr("Denied").to_owned(),
        Some("expired") => tr("Expired").to_owned(),
        Some("offline") => tr("Offline").to_owned(),
        Some(_) => tr("Failed").to_owned(),
    }
}

/// The collapsed line of a tool: "Searched mail · 12 results".
pub fn tool_line(name: &str, summary: &str, error: Option<&str>) -> String {
    let outcome = tool_outcome(summary, error);
    if outcome.is_empty() {
        tool_title(name)
    } else {
        trf("{tool} · {outcome}", &[("tool", &tool_title(name)), ("outcome", &outcome)])
    }
}

/// An answer as one plain line, for a mini tile with room for no more: its lines joined, the marks
/// of Markdown left out (`**`, `` ` ``, `#`, list and quote marks, a link's address), so the caller
/// can cut it with "…" (`typo::ellipsize`).
pub fn answer_line(markdown: &str) -> String {
    let lines = markdown.lines().map(|line| {
        let line = line.trim_start().trim_start_matches(['#', '>']).trim_start();
        let line = line.strip_prefix("- ").or_else(|| line.strip_prefix("* ")).unwrap_or(line);
        line.trim()
    });
    let joined = lines.filter(|line| !line.is_empty()).collect::<Vec<_>>().join(" ");
    let mut plain = String::with_capacity(joined.len());
    let mut rest = joined.as_str();
    // "[text](address)" keeps its text.
    while let Some(open) = rest.find("](") {
        let close = rest[open..].find(')').map(|at| open + at);
        let start = rest[..open].rfind('[');
        match (start, close) {
            (Some(start), Some(close)) => {
                plain.push_str(&rest[..start]);
                plain.push_str(&rest[start + 1..open]);
                rest = &rest[close + 1..];
            }
            _ => break,
        }
    }
    plain.push_str(rest);
    plain.chars().filter(|c| !matches!(c, '*' | '`')).collect()
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeZone, Utc};
    use torro_ipc::assistant::TEST_ENGINE;

    use super::*;

    fn at(minute: u32) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 14, 9, minute, 0).single().expect("a time")
    }

    fn summary(id: &str, last: u64, running: bool) -> ConversationSummary {
        ConversationSummary {
            id: id.into(),
            title: "Plan for tomorrow".into(),
            engine: "ai:ollama@localhost:11434".into(),
            model: Some("qwen3:8b".into()),
            effort: None,
            created: at(0),
            updated: at(1),
            pinned: false,
            running,
            last,
            bot: None,
            bot_deleted: false,
            about: None,
        }
    }

    fn answer(seq: u64, text: &str) -> Entry {
        Entry { seq, time: at(2), content: EntryContent::Answer { text: text.into(), engine: "e".into(), model: None } }
    }

    fn message(seq: u64, text: &str) -> Entry {
        Entry {
            seq,
            time: at(2),
            content: EntryContent::Message {
                text: text.into(),
                engine: "e".into(),
                model: None,
                effort: None,
                without: Vec::new(),
                mode: torro_ipc::assistant::Mode::Default,
                from: None,
            },
        }
    }

    fn tool(seq: u64) -> Entry {
        Entry {
            seq,
            time: at(2),
            content: EntryContent::Tool {
                name: "mail.search_messages".into(),
                input: serde_json::json!({}),
                summary: "12 results".into(),
                error: None,
                lack: None,
            },
        }
    }

    fn texts(transcript: &Transcript) -> Vec<(u64, String)> {
        transcript
            .entries
            .iter()
            .map(|entry| {
                let text = match &entry.content {
                    EntryContent::Answer { text, .. } | EntryContent::Message { text, .. } => text.clone(),
                    EntryContent::Tool { name, .. } => name.clone(),
                    other => format!("{other:?}"),
                };
                (entry.seq, text)
            })
            .collect()
    }

    fn engine(id: &str, kind: EngineKind) -> Engine {
        Engine { id: id.into(), kind, name: id.into(), problem: None, models: Vec::new() }
    }

    fn model(id: &str, tools: Option<bool>) -> AiModel {
        AiModel {
            id: id.into(),
            name: id.into(),
            tools,
            open_weights: None,
            context: None,
            parameters: None,
            quantization: None,
            size: None,
            reasoning: None,
            remote: None,
            decision: None,
            enabled: false,
        }
    }

    #[test]
    fn a_growing_answer_is_replaced_by_its_number() {
        let mut transcript = Transcript::new(summary("c1", 1, true), vec![message(1, "What is on?")]);
        assert!(transcript.merge(summary("c1", 1, true), vec![answer(2, "Tomorrow")]));
        assert_eq!(transcript.growing().map(|entry| entry.seq), Some(2));
        assert!(transcript.merge(summary("c1", 1, true), vec![answer(2, "Tomorrow you have")]));
        assert_eq!(texts(&transcript), [(1, "What is on?".into()), (2, "Tomorrow you have".into())], "one entry");
        // Stored now: `last` reaches it and it comes once more, final.
        assert!(transcript.merge(summary("c1", 2, false), vec![answer(2, "Tomorrow you have four events.")]));
        assert_eq!(transcript.growing(), None);
        assert_eq!(transcript.last_answer_text(), Some("Tomorrow you have four events."));
        assert!(!transcript.running());
    }

    #[test]
    fn a_growing_answer_that_became_a_tool_or_went_is_dropped() {
        let mut transcript = Transcript::new(summary("c1", 1, true), vec![message(1, "Hi")]);
        transcript.merge(summary("c1", 1, true), vec![answer(2, "Let me")]);
        // The model called a tool instead: number 2 is the tool now, 3 grows.
        transcript.merge(summary("c1", 2, true), vec![tool(2), answer(3, "You have")]);
        assert_eq!(texts(&transcript), [(1, "Hi".into()), (2, "mail.search_messages".into()), (3, "You have".into())]);
        // Stopped before anything was stored of it: the partial text is gone from the answer.
        transcript.merge(summary("c1", 2, false), Vec::new());
        assert_eq!(transcript.entries.len(), 2, "the stale growing answer is dropped");
    }

    #[test]
    fn polled_entries_join_in_order_and_old_answers_are_ignored() {
        let mut transcript = Transcript::new(summary("c1", 2, false), vec![answer(2, "b"), message(1, "a")]);
        assert_eq!(transcript.entries.iter().map(|entry| entry.seq).collect::<Vec<_>>(), [1, 2], "sorted");
        assert!(transcript.merge(summary("c1", 4, false), vec![tool(3), answer(4, "d")]));
        assert_eq!(transcript.last(), 4);
        assert!(!transcript.merge(summary("c1", 3, false), vec![answer(3, "old")]), "an older answer");
        assert_eq!(transcript.entries.len(), 4);
        assert_eq!(texts(&transcript)[2], (3, "mail.search_messages".into()), "kept");
        // Another conversation's answer replaces everything.
        assert!(transcript.merge(summary("c2", 1, false), vec![message(1, "other")]));
        assert_eq!(texts(&transcript), [(1, "other".into())]);
    }

    #[test]
    fn the_picker_lists_local_accounts_first_and_only_tool_models() {
        let engines = vec![
            engine(TEST_ENGINE, EngineKind::Test),
            engine("ai:openrouter@openrouter.ai", EngineKind::Ai),
            engine("ai:ollama@localhost:11434", EngineKind::Ai),
            engine("ai:lm_studio@127.0.0.1:1234", EngineKind::Ai),
        ];
        let ids: Vec<String> = ordered(engines).into_iter().map(|engine| engine.id).collect();
        assert_eq!(
            ids,
            ["ai:ollama@localhost:11434", "ai:lm_studio@127.0.0.1:1234", "ai:openrouter@openrouter.ai", TEST_ENGINE]
        );

        let models = [model("qwen3:8b", Some(true)), model("gemma3:4b", Some(false)), model("local", None)];
        let offered: Vec<String> = tool_models(&models).into_iter().map(|model| model.id).collect();
        assert_eq!(offered, ["qwen3:8b", "local"], "a model known to have no tools is left out");
    }

    #[test]
    fn the_last_engine_used_comes_first_else_a_local_tool_model() {
        let engines = vec![
            engine("ai:openrouter@openrouter.ai", EngineKind::Ai),
            engine("ai:ollama@localhost:11434", EngineKind::Ai),
        ];
        let models = vec![model("gemma3:4b", Some(false)), model("qwen3:8b", Some(true))];
        let listed = |id: &str| (id == "ai:ollama@localhost:11434").then_some(models.as_slice());
        let first = default_choice(&[], &engines, listed);
        assert_eq!(first, Some(Choice { engine: "ai:ollama@localhost:11434".into(), model: Some("qwen3:8b".into()) }));

        let mut older = summary("c1", 1, false);
        older.engine = "ai:openrouter@openrouter.ai".into();
        older.model = Some("qwen/qwen3-32b".into());
        let mut gone = summary("c2", 1, false);
        gone.engine = "ai:removed@example.com".into();
        gone.updated = at(0);
        assert_eq!(
            default_choice(&[gone.clone(), older], &engines, listed),
            Some(Choice { engine: "ai:openrouter@openrouter.ai".into(), model: Some("qwen/qwen3-32b".into()) })
        );
        assert_eq!(
            default_choice(&[gone], &engines, |_| None),
            Some(Choice { engine: "ai:ollama@localhost:11434".into(), model: None }),
            "the newest conversation's engine is gone; models not in yet: the engine picks"
        );
        assert_eq!(default_choice(&[], &[], listed), None);
    }

    /// Issue #131: the control centre's default wins while its engine is there; else the last one
    /// used, with the effort it had.
    #[test]
    fn the_default_of_the_control_centre_comes_first() {
        let engines =
            vec![engine("ai:ollama@localhost:11434", EngineKind::Ai), engine("claude-code", EngineKind::Agent)];
        let default =
            AssistantDefault { engine: "claude-code".into(), model: Some("opus".into()), effort: Some(Effort::High) };
        let (choice, effort) = preferred_choice(Some(&default), &[], &engines, |_| None).expect("a choice");
        assert_eq!(
            (choice.engine.as_str(), choice.model.as_deref(), effort),
            ("claude-code", Some("opus"), Some(Effort::High))
        );

        let mut last = summary("c1", 1, false);
        last.effort = Some(Effort::Low);
        let gone = AssistantDefault { engine: "ai:removed@example.com".into(), ..default };
        let (choice, effort) = preferred_choice(Some(&gone), &[last], &engines, |_| None).expect("a choice");
        assert_eq!((choice.model.as_deref(), effort), (Some("qwen3:8b"), Some(Effort::Low)), "the one used last");
        assert_eq!(preferred_choice(None, &[], &[], |_| None), None);
    }

    #[test]
    fn tool_lines_say_what_was_done_and_how_it_went() {
        assert_eq!(
            tool_line("mail.search_messages", "12 results", None),
            trf("{tool} · {outcome}", &[("tool", &tr("Searched mail")), ("outcome", &"12 results")])
        );
        assert_eq!(tool_title("calendar.frobnicate"), "calendar.frobnicate", "unknown names show as they are");
        assert_eq!(tool_title("assistant.my_access"), tr("Looked up its access"), "issue #134");
        assert_eq!(tool_outcome("forbidden: no access to mail", Some("forbidden")), tr("No access"));
        assert_eq!(tool_line("find_tools", "", None), tr("Looked for tools"));
        assert_eq!(tool_title("mail.extract_action"), tr("Read a suggestion out of the mail"), "issue #605");
    }

    /// Fixes of the design review of WZ.8: the card right after a tool is found (its line then
    /// leaves the file's name to the card), and an answer becomes one plain line for the mini tile.
    #[test]
    fn a_card_after_a_tool_and_an_answer_on_one_line() {
        let card = |seq| Entry {
            seq,
            time: at(2),
            content: EntryContent::Card { card: torro_ipc::assistant::CardRef::File { id: "5eedf11e".into() } },
        };
        let transcript = Transcript::new(
            summary("c1", 5, false),
            vec![message(1, "Make a PDF"), tool(2), card(3), tool(4), answer(5, "Done")],
        );
        assert_eq!(transcript.card_after(2), Some(3));
        assert_eq!(transcript.card_after(4), None, "an answer follows");
        assert_eq!(transcript.card_after(5), None, "nothing follows");

        let markdown =
            "## Done\n\n- **Release checklist-2.pdf** is on your [desktop](file:///home/anna/Desktop).\n> `84 KB`";
        assert_eq!(answer_line(markdown), "Done Release checklist-2.pdf is on your desktop. 84 KB");
        assert_eq!(answer_line("A [half link and (more)"), "A [half link and (more)");
    }
}
