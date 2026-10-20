//! What the chat window decides apart from drawing, kept pure so it can be tested on its own:
//!
//! - [`Transcript`]: the entries of the open conversation, merged from every answer of
//!   `assistant.get_conversation` – one entry per number, and the answer the model is still writing
//!   (numbered `last + 1`, see `torro_ipc::assistant`) replaced each time it comes again.
//! - The engine and model picker: engines in the order of Open First ([`ordered`]), only models
//!   that can call tools ([`tool_models`]), and what is chosen before the person picks
//!   ([`default_choice`], first the default of the control centre: [`preferred_choice`]). Beside
//!   it the effort, offered only for a model that thinks ([`can_reason`], issue #131).
//! - The areas the person can switch off for one conversation ([`switchable_areas`], the
//!   `without` of `assistant.send_message`), the permission mode ([`mode_name`], issue #132) and
//!   the line under the composer that says what the assistant may do now ([`access_line`]).
//! - The requests the window sends for a click ([`send_request`], [`decide_request`]) and the words
//!   of a tool line ([`tool_title`], [`tool_outcome`]), and under a refused one why, with "Change
//!   access" only where that helps ([`refusal_hint`], issue #133).
//! - The sidebar's two groups, pinned and recent, filtered by the search ([`sidebar_groups`]).
//! - The file card (task WZ.8): what it says of a file ([`file_meta`]) and the request of each of
//!   its buttons ([`open_file_request`], [`save_file_request`], [`mail_file_request`]).

use std::collections::HashSet;
use std::path::Path;

use torro_core::AppId;
use torro_core::ai::{AiModel, Effort, is_this_computer};
use torro_core::i18n::{tr, trc, trf};
use torro_core::settings::AssistantDefault;
use torro_ipc::Request;
use torro_ipc::api::{Access, Area, CATALOGUE, Lack, Level, Rights};
use torro_ipc::assistant::{ConversationSummary, Engine, EngineKind, Entry, EntryContent, Mode, MyAccess};
use torro_ipc::tools::{FileInfo, PdfTemplateId};

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
    #[cfg(test)]
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
/// control centre (`default`, `settings.assistant_default`) while its engine is offered, else
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

/// The models the picker offers for `engine`: an agent's fixed ones ([`Engine::models`]), an AI
/// account's `listed` ones that can call tools ([`tool_models`]); `None` while an AI account's are
/// not listed, and for the test engine.
pub fn engine_models(engine: &Engine, listed: Option<&[AiModel]>) -> Option<Vec<AiModel>> {
    match engine.kind {
        EngineKind::Agent => Some(engine.models.clone()),
        EngineKind::Ai => listed.map(tool_models),
        EngineKind::Test => None,
    }
}

/// Whether the composer offers an effort for `choice` (issue #131): a model is picked and is not
/// known to lack thinking ([`AiModel::reasoning`], from `models`, the engine's offered models). An
/// agent left to pick its own model takes an effort too; an AI account left to pick does not, as
/// nobody knows which model it takes.
pub fn can_reason(engine: Option<&Engine>, choice: &Choice, models: Option<&[AiModel]>) -> bool {
    let Some(engine) = engine else { return false };
    match (&choice.model, engine.kind) {
        (_, EngineKind::Test) | (None, EngineKind::Ai) => false,
        (None, EngineKind::Agent) => true,
        (Some(id), _) => models
            .and_then(|models| models.iter().find(|model| &model.id == id))
            .is_some_and(|model| model.reasoning != Some(false)),
    }
}

/// The word for `effort` in the composer and its menu: "Automatic" for `None`, else "Low",
/// "Medium", "High".
pub fn effort_name(effort: Option<Effort>) -> &'static str {
    match effort {
        None => tr("Automatic"),
        Some(Effort::Low) => tr("Low"),
        Some(Effort::Medium) => tr("Medium"),
        Some(Effort::High) => tr("High"),
    }
}

/// The areas the composer shows, in this order.
const SHOWN: [Area; 6] = [Area::Mail, Area::Calendar, Area::Tasks, Area::Inbox, Area::Workspaces, Area::Github];

/// The areas the composer offers to switch off, in this order: those of the assistant's own
/// client that it may use at all (`rights`, from `clients.list_clients`). Without a client, none:
/// the assistant can use no tool then.
pub fn switchable_areas(rights: Option<&Rights>) -> Vec<Area> {
    let Some(rights) = rights else { return Vec::new() };
    SHOWN.into_iter().filter(|area| rights.area(*area).level != Level::NoAccess).collect()
}

/// The name of a permission mode in the composer and its menu (issue #132): "Read only", "Ask
/// before changes", "Default access".
pub fn mode_name(mode: Mode) -> &'static str {
    match mode {
        Mode::ReadOnly => tr("Read only"),
        Mode::Ask => tr("Ask before changes"),
        Mode::Default => tr("Default access"),
    }
}

/// The line under the composer (issue #132): what the assistant may do with the next message, as
/// `assistant.my_access` would answer it ([`MyAccess`], issue #134) – the stored `rights` of its
/// client without the areas `without`, in the mode `mode`. Each area it may use with its level, then
/// when it asks: "Mail: Read · Calendar: Write · asks before changes".
pub fn access_line(rights: &Rights, without: &[Area], mode: Mode) -> String {
    let access = MyAccess::of(rights.clone(), None).switching_off(without).in_mode(mode);
    let mut parts: Vec<String> = SHOWN
        .into_iter()
        .filter_map(|area| {
            let level = match access.rights.area(area).level {
                Level::NoAccess => return None,
                Level::Read => tr("Read"),
                Level::Write => tr("Write"),
                Level::Full => tr("Full"),
            };
            Some(trf("{area}: {level}", &[("area", &area_name(area)), ("level", &level)]))
        })
        .collect();
    if parts.is_empty() {
        parts.push(tr("No access to your data").to_owned());
    }
    parts.push(
        match (mode, access.rights.confirm) {
            (Mode::ReadOnly, _) => tr("changes nothing"),
            (_, torro_ipc::api::Confirm::OnEveryChange) => tr("asks before changes"),
            (_, torro_ipc::api::Confirm::OnDelete) => tr("asks before deleting"),
            (_, torro_ipc::api::Confirm::Never) => tr("asks only for settings"),
        }
        .to_owned(),
    );
    parts.join(" · ")
}

/// The request of "Send": `assistant.send_message` with the text as typed (trailing blank lines
/// dropped), to `conversation` or a new one, with `effort` (issue #131; `None` where the model takes
/// none, [`can_reason`]), to the bot `bot` (task B.3) or the assistant; `None` for a text that is
/// only blank or without an engine, which the core would refuse anyway.
pub fn send_request(
    conversation: Option<&str>,
    choice: Option<&Choice>,
    effort: Option<Effort>,
    text: &str,
    (without, mode): (&[Area], Mode),
    bot: Option<&str>,
) -> Option<Request> {
    let text = text.trim_end();
    let choice = choice?;
    if text.trim().is_empty() {
        return None;
    }
    Some(Request::SendMessage {
        conversation: conversation.map(str::to_owned),
        engine: choice.engine.clone(),
        model: choice.model.clone(),
        effort,
        text: text.to_owned(),
        without: without.to_vec(),
        mode,
        bot: bot.map(str::to_owned),
    })
}

/// The request of the approval card's "Allow" (`allow`) or "Deny": `approvals.decide_approval`.
/// Only ever sent for a person's click; the model has no way to it.
pub fn decide_request(approval: &str, allow: bool) -> Request {
    Request::DecideApproval { id: approval.to_owned(), allow }
}

/// The area of the catalogue command `name` (`calendar.list_events` → calendar); `None` for a name
/// the catalogue does not know (the assistant's own `find_tools`).
pub fn area_of(name: &str) -> Option<Area> {
    CATALOGUE.iter().find(|command| command.name == name).map(|command| command.area)
}

/// The name of an area as the control centre writes it ("Calendar", "Mail").
pub fn area_name(area: Area) -> &'static str {
    match area {
        Area::System => tr("System"),
        Area::Workspaces => tr("Workspaces"),
        Area::Calendar => tr("Calendar"),
        Area::Tasks => tr("Tasks"),
        Area::Github => tr("GitHub"),
        Area::Inbox => tr("Notification Hub"),
        Area::Mail => tr("Mail"),
        Area::Accounts => tr("Accounts"),
        Area::Ai => tr("AI"),
        Area::Assistant => tr("Assistant"),
        Area::Bots => tr("Bots"),
        Area::Tools => tr("Tools"),
        Area::Settings => tr("Settings"),
        Area::Apps => tr("Apps"),
        Area::Clients => tr("Agents & API"),
        Area::Approvals => tr("Approvals"),
    }
}

/// The app that shows what a tool of `area` looked at or changed, for "Open …" under an unfolded
/// tool line: the calendar, the tasks app (GitHub filter lists are task lists there), the inbox
/// (mail too). `None` for the other areas.
pub fn app_of(area: Area) -> Option<AppId> {
    match area {
        Area::Calendar => Some(AppId::CALENDAR),
        Area::Tasks | Area::Github => Some(AppId::TASKS),
        Area::Inbox | Area::Mail => Some(AppId::INBOX),
        _ => None,
    }
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
        "mail.save_draft" => tr("Saved draft"),
        "mail.send_draft" => tr("Sent mail"),
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

/// The sentence under a `forbidden` tool line of `area` and whether "Change access" goes with it
/// (issue #133): the reason the core gave (`lack`), and the button only where changing the
/// assistant's rights lifts it – not for what an agent never gets, not for an area switched off in
/// this conversation, not for what only the person may do. Without a reason (an older line, a
/// refusal of the core's own) the general sentence and the button, as before.
pub fn refusal_hint(area: Option<Area>, lack: Option<Lack>) -> (String, bool) {
    let name = |area: Area| area_name(area);
    let Some(lack) = lack else {
        let text = match area {
            Some(area) => trf("The assistant has no access to {area}.", &[("area", &name(area))]),
            None => tr("The assistant has no access to this.").to_owned(),
        };
        return (text, true);
    };
    let text = match lack {
        Lack::Area { area } => trf("The assistant has no access to {area}.", &[("area", &name(area))]),
        Lack::Level { area, needs: Access::Delete } => {
            trf("The assistant needs full access to {area} for this.", &[("area", &name(area))])
        }
        Lack::Level { area, .. } => {
            trf("The assistant needs write access to {area} for this.", &[("area", &name(area))])
        }
        Lack::Admin { area } => trf("The assistant needs admin rights for {area}.", &[("area", &name(area))]),
        Lack::Outbound => tr("Sending mail on your behalf is switched off for the assistant.").to_owned(),
        Lack::ForPeople => tr("Only you can do this, in Torro Workplace.").to_owned(),
        Lack::SwitchedOff { area } => trf("You switched off {area} for this conversation.", &[("area", &name(area))]),
        Lack::ReadOnly => {
            tr("This conversation is read only. Pick another mode under the message field to let it change things.")
                .to_owned()
        }
        Lack::NeverToAgents { area } => trf(
            "{area} never goes to coding agents like Claude Code: your data rule keeps it on your computers. Choose an AI account on this computer for it.",
            &[("area", &name(area))],
        ),
        Lack::ToolOff { tool } => {
            trf("The tool {tool} is switched off in the assistant's settings.", &[("tool", &tool.title())])
        }
        Lack::Tool { tool } => trf("The assistant may not use the tool {tool}.", &[("tool", &tool.title())]),
    };
    (text, lack.in_rights())
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

/// The name of a PDF template as the assistant's settings write it ("Report").
pub fn template_name(template: PdfTemplateId) -> &'static str {
    match template {
        PdfTemplateId::Letter => trc("template", "Letter"),
        PdfTemplateId::Report => trc("template", "Report"),
        PdfTemplateId::Note => trc("template", "Note"),
    }
}

/// A size in bytes as people read it: "860 B", "3 KB", "12 MB" (as the mail window writes it).
pub fn size_text(bytes: u64) -> String {
    const KB: u64 = 1024;
    if bytes < KB {
        trf("{n} B", &[("n", &bytes)])
    } else if bytes < KB * KB {
        trf("{n} KB", &[("n", &bytes.div_ceil(KB))])
    } else {
        trf("{n} MB", &[("n", &bytes.div_ceil(KB * KB))])
    }
}

/// The quiet line under a file card's name: its template and size ("Report · 84 KB").
pub fn file_meta(file: &FileInfo) -> String {
    format!("{} · {}", template_name(file.template), size_text(file.size))
}

/// "Open" on a file card: `tools.open_file`, the system's program for it on the person's screen.
pub fn open_file_request(file: &str) -> Request {
    Request::OpenFile { file: file.to_owned() }
}

/// "Save as…" on a file card, once the person chose `path` in the system's save dialog:
/// `tools.save_file_as`. `None` for a path that is not text, which the core could not take.
pub fn save_file_request(file: &str, path: &Path) -> Option<Request> {
    let path = path.to_str()?;
    Some(Request::SaveFileAs { file: file.to_owned(), path: path.to_owned() })
}

/// "Send by mail" on a file card: `apps.open_app` with `file:<id>`, a new mail in the inbox's
/// compose window with the file attached – the person sees what goes out before it goes.
pub fn mail_file_request(file: &str) -> Request {
    Request::OpenApp { app: AppId::INBOX.binary.to_owned(), target: Some(format!("file:{file}")) }
}

/// The sidebar's conversations: pinned ones, then the rest, each newest first, both narrowed to
/// the titles that contain `query` (any case; all of them for a blank query).
pub fn sidebar_groups<'a>(
    conversations: &'a [ConversationSummary],
    query: &str,
) -> (Vec<&'a ConversationSummary>, Vec<&'a ConversationSummary>) {
    let query = query.trim().to_lowercase();
    let mut shown: Vec<&ConversationSummary> = conversations
        .iter()
        .filter(|conversation| query.is_empty() || conversation.title.to_lowercase().contains(&query))
        .collect();
    shown.sort_by_key(|conversation| std::cmp::Reverse(conversation.updated));
    shown.into_iter().partition(|conversation| conversation.pinned)
}

#[cfg(test)]
mod tests {
    use chrono::{DateTime, TimeZone, Utc};
    use torro_ipc::api::AreaRights;
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
            reasoning: None,
            remote: None,
            decision: None,
        }
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

    #[test]
    fn send_builds_the_message_request() {
        let choice = Choice { engine: "ai:ollama@localhost:11434".into(), model: Some("qwen3:8b".into()) };
        assert_eq!(
            send_request(
                Some("c1"),
                Some(&choice),
                Some(Effort::High),
                "What is on tomorrow?\n\n",
                (&[Area::Mail], Mode::Ask),
                None
            ),
            Some(Request::SendMessage {
                conversation: Some("c1".into()),
                engine: "ai:ollama@localhost:11434".into(),
                model: Some("qwen3:8b".into()),
                effort: Some(Effort::High),
                text: "What is on tomorrow?".into(),
                without: vec![Area::Mail],
                mode: Mode::Ask,
                bot: None,
            })
        );
        let none: (&[Area], Mode) = (&[], Mode::Default);
        assert_eq!(send_request(None, Some(&choice), None, " \n ", none, None), None, "blank");
        assert_eq!(send_request(None, None, None, "Hi", none, None), None, "no engine");
        // Task B.3: a message to a bot names it; the core runs it with the bot's client.
        let to_bot = send_request(None, Some(&choice), None, "Plan Lisbon", none, Some("b0000001"));
        assert!(matches!(to_bot, Some(Request::SendMessage { bot: Some(ref bot), .. }) if bot == "b0000001"));
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

    /// Issue #131: an effort only for a model that thinks; an agent's models are its own.
    #[test]
    fn an_effort_is_offered_only_where_the_model_takes_one() {
        let mut claude = engine("claude-code", EngineKind::Agent);
        claude.models = vec![
            AiModel { reasoning: Some(true), ..model("opus", Some(true)) },
            AiModel { reasoning: Some(false), ..model("haiku", Some(true)) },
        ];
        let ollama = engine("ai:ollama@localhost:11434", EngineKind::Ai);
        let listed = [
            AiModel { reasoning: Some(true), ..model("qwen3:8b", Some(true)) },
            model("local", None),
            model("gemma3:4b", Some(false)),
        ];
        let offered = engine_models(&claude, None).expect("fixed");
        assert_eq!(offered.len(), 2);
        assert_eq!(engine_models(&ollama, Some(&listed)).map(|models| models.len()), Some(2), "tool models only");
        assert_eq!(engine_models(&ollama, None), None);
        let pick =
            |engine: &str, model: Option<&str>| Choice { engine: engine.into(), model: model.map(str::to_owned) };
        assert!(can_reason(Some(&claude), &pick("claude-code", Some("opus")), Some(&offered)));
        assert!(!can_reason(Some(&claude), &pick("claude-code", Some("haiku")), Some(&offered)));
        assert!(can_reason(Some(&claude), &pick("claude-code", None), Some(&offered)), "the agent picks");
        assert!(can_reason(Some(&ollama), &pick("x", Some("qwen3:8b")), Some(&listed)));
        assert!(can_reason(Some(&ollama), &pick("x", Some("local")), Some(&listed)), "not known to lack it");
        assert!(!can_reason(Some(&ollama), &pick("x", None), Some(&listed)), "the account picks: unknown");
        assert!(!can_reason(None, &pick("x", Some("qwen3:8b")), Some(&listed)));
        assert_eq!(effort_name(None), tr("Automatic"));
        assert_eq!(effort_name(Some(Effort::High)), tr("High"));
    }

    /// Issue #132: the line under the composer says what the assistant may do with the next message,
    /// narrowed by the switched-off areas and the mode, never more than the rights.
    #[test]
    fn the_access_line_says_what_the_next_message_may_do() {
        let mut rights = Rights::assistant();
        rights.areas.insert(Area::Mail, AreaRights { level: Level::Read, admin: false });
        rights.areas.insert(Area::Calendar, AreaRights { level: Level::Write, admin: false });
        let part = |area: &'static str, level: &'static str| {
            trf("{area}: {level}", &[("area", &tr(area)), ("level", &tr(level))])
        };
        let line = access_line(&rights, &[], Mode::Ask);
        assert!(line.starts_with(&part("Mail", "Read")), "{line}");
        assert!(line.contains(&part("Calendar", "Write")), "{line}");
        assert!(line.ends_with(tr("asks before changes")), "{line}");
        let read_only = access_line(&rights, &[Area::Mail], Mode::ReadOnly);
        assert!(!read_only.contains(&part("Mail", "Read")), "switched off: {read_only}");
        assert!(read_only.contains(&part("Calendar", "Read")), "capped at read: {read_only}");
        assert!(read_only.ends_with(tr("changes nothing")), "{read_only}");
        assert!(access_line(&rights, &[], Mode::Default).ends_with(tr("asks before deleting")));
        assert!(access_line(&Rights::none(), &[], Mode::Default).starts_with(tr("No access to your data")));
        assert_eq!(mode_name(Mode::Ask), tr("Ask before changes"));

        let mut transcript = Transcript::new(summary("c1", 1, false), vec![message(1, "Hi")]);
        assert_eq!(transcript.last_mode(), Some(Mode::Default));
        let asked = Entry {
            seq: 2,
            time: at(3),
            content: EntryContent::Message {
                text: "Look only".into(),
                engine: "e".into(),
                model: None,
                effort: None,
                without: Vec::new(),
                mode: Mode::ReadOnly,
            },
        };
        transcript.merge(summary("c1", 2, false), vec![asked]);
        assert_eq!(transcript.last_mode(), Some(Mode::ReadOnly));
    }

    #[test]
    fn the_approval_card_decides_with_the_app_request() {
        assert_eq!(
            decide_request("9c1f0a7e3b2d4c58", true),
            Request::DecideApproval { id: "9c1f0a7e3b2d4c58".into(), allow: true }
        );
        assert_eq!(
            decide_request("9c1f0a7e3b2d4c58", false),
            Request::DecideApproval { id: "9c1f0a7e3b2d4c58".into(), allow: false }
        );
    }

    #[test]
    fn only_areas_the_assistant_may_use_can_be_switched_off() {
        assert!(switchable_areas(None).is_empty());
        let mut rights = Rights::assistant();
        assert_eq!(switchable_areas(Some(&rights)), [Area::Calendar, Area::Tasks, Area::Inbox]);
        rights.areas.insert(Area::Mail, AreaRights { level: Level::Read, admin: false });
        assert_eq!(switchable_areas(Some(&rights)), [Area::Mail, Area::Calendar, Area::Tasks, Area::Inbox]);
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
        assert_eq!(area_of("mail.search_messages"), Some(Area::Mail));
        assert_eq!(area_of("find_tools"), None);
        assert_eq!(app_of(Area::Mail), Some(AppId::INBOX));
    }

    /// Issue #133: a refused tool says why; "Change access" only where the rights lift it.
    #[test]
    fn a_refusal_says_why_and_offers_a_change_only_where_it_helps() {
        let mail = Some(Area::Mail);
        let hint = |lack| refusal_hint(mail, Some(lack));
        let named = |text: &'static str| trf(text, &[("area", &tr("Mail"))]);
        assert_eq!(refusal_hint(mail, None), (named("The assistant has no access to {area}."), true), "older lines");
        assert_eq!(hint(Lack::Area { area: Area::Mail }), (named("The assistant has no access to {area}."), true));
        let write = hint(Lack::Level { area: Area::Mail, needs: Access::Write });
        assert_eq!(write, (named("The assistant needs write access to {area} for this."), true));
        let full = hint(Lack::Level { area: Area::Mail, needs: Access::Delete });
        assert_eq!(full, (named("The assistant needs full access to {area} for this."), true));
        assert!(hint(Lack::Outbound).1, "sending is a switch of the rights");
        let off = hint(Lack::SwitchedOff { area: Area::Mail });
        assert_eq!(off, (named("You switched off {area} for this conversation."), false));
        let (never, change) = hint(Lack::NeverToAgents { area: Area::Mail });
        assert!(!change && never.contains("Claude Code"), "{never}");
        assert!(!hint(Lack::ForPeople).1);
        // Issue #225: a tool switched off is the assistant's settings, a tool not granted its rights.
        let pdf = torro_ipc::tools::ToolId::Pdf;
        let tool = |text: &'static str| trf(text, &[("tool", &"PDF")]);
        let off = hint(Lack::ToolOff { tool: pdf });
        assert_eq!(off, (tool("The tool {tool} is switched off in the assistant's settings."), false));
        let not_granted = hint(Lack::Tool { tool: pdf });
        assert_eq!(not_granted, (tool("The assistant may not use the tool {tool}."), true));
        assert_eq!(area_name(Area::Tools), tr("Tools"));
    }

    /// Task WZ.8: the tool lines of the PDF tool say what was done; a file card says its template
    /// and size, and each of its buttons builds its request.
    #[test]
    fn the_file_card_says_what_the_file_is_and_each_button_its_request() {
        assert_eq!(tool_title("tools.create_pdf"), tr("Created PDF"));
        assert_eq!(tool_title("tools.list_tools"), tr("Listed tools"));
        assert_eq!(tool_title("tools.open_file"), tr("Opened file"));
        let file = FileInfo {
            id: "1a2b3c4d".into(),
            name: "Offer.pdf".into(),
            path: "/home/anna/Desktop/Offer.pdf".into(),
            title: "Offer".into(),
            template: PdfTemplateId::Report,
            size: 86_000,
            created: Utc::now(),
        };
        let size = trf("{n} KB", &[("n", &84)]);
        assert_eq!(file_meta(&file), format!("{} · {size}", trc("template", "Report")));
        assert_eq!(size_text(860), trf("{n} B", &[("n", &860)]));
        assert_eq!(size_text(3 * 1024 * 1024), trf("{n} MB", &[("n", &3)]));
        assert_eq!(open_file_request("1a2b3c4d"), Request::OpenFile { file: "1a2b3c4d".into() });
        let path = Path::new("/home/anna/Offer copy.pdf");
        assert_eq!(
            save_file_request("1a2b3c4d", path),
            Some(Request::SaveFileAs { file: "1a2b3c4d".into(), path: "/home/anna/Offer copy.pdf".into() })
        );
        assert_eq!(
            mail_file_request("1a2b3c4d"),
            Request::OpenApp { app: "torroinbox".into(), target: Some("file:1a2b3c4d".into()) }
        );
    }

    #[test]
    fn the_sidebar_puts_pinned_first_newest_first_and_searches_titles() {
        let mut list = Vec::new();
        for (minute, title, pinned) in
            [(1, "Trip to Lisbon", false), (5, "Plan for tomorrow", true), (3, "Release", false)]
        {
            let mut conversation = summary(title, 1, false);
            conversation.title = title.into();
            conversation.updated = at(minute);
            conversation.pinned = pinned;
            list.push(conversation);
        }
        let titles = |group: Vec<&ConversationSummary>| group.iter().map(|c| c.title.clone()).collect::<Vec<_>>();
        let (pinned, recent) = sidebar_groups(&list, "");
        assert_eq!(
            (titles(pinned), titles(recent)),
            (vec!["Plan for tomorrow".to_owned()], vec!["Release".to_owned(), "Trip to Lisbon".to_owned()])
        );
        let (pinned, recent) = sidebar_groups(&list, "  LISBON ");
        assert_eq!((titles(pinned).len(), titles(recent)), (0, vec!["Trip to Lisbon".to_owned()]));
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
