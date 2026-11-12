//! What the chat window decides apart from drawing, kept pure so it can be tested on its own:
//!
//! - [`Transcript`], the engine order and the default choice ([`ordered`], [`tool_models`],
//!   [`default_choice`], [`preferred_choice`]) and the words of a tool line ([`tool_title`],
//!   [`tool_outcome`], [`tool_line`], [`answer_line`]) live in `torro_gui::chat` since issue #605,
//!   shared with the mini chat about a mail in the mail window; they are used from there.
//! - The models the picker offers for an engine ([`engine_models`]) and the effort beside them,
//!   offered only for a model that thinks ([`can_reason`], issue #131).
//! - The areas the person can switch off for one conversation ([`switchable_areas`], the
//!   `without` of `assistant.send_message`), the permission mode ([`mode_name`], issue #132) and
//!   the line under the composer that says what the assistant may do now ([`access_line`]).
//! - The requests the window sends for a click ([`send_request`], [`decide_request`]), and under a
//!   refused tool why, with "Change access" only where that helps ([`refusal_hint`], issue #133).
//! - The sidebar's two groups, pinned and recent, filtered by the search ([`sidebar_groups`]).
//! - The file card (task WZ.8): what it says of a file ([`file_meta`]) and the request of each of
//!   its buttons ([`open_file_request`], [`save_file_request`], [`mail_file_request`]).

use std::path::Path;

use torro_core::AppId;
use torro_core::ai::{AiModel, Effort};
use torro_core::i18n::{tr, trc, trf};
pub use torro_gui::chat::{
    Choice, Transcript, answer_line, default_choice, ordered, preferred_choice, tool_line, tool_models, tool_outcome,
    tool_title,
};
use torro_ipc::Request;
use torro_ipc::api::{Access, Area, CATALOGUE, Lack, Level, Rights};
use torro_ipc::assistant::{ConversationSummary, Engine, EngineKind, Mode, MyAccess};
use torro_ipc::tools::{FileInfo, PdfTemplateId};

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
        about: None,
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

/// The name of an area as the control centre writes it ("Calendar", "Mail"); the clients' area by
/// where they are managed since issue #231 ("API and MCP").
pub fn area_name(area: Area) -> &'static str {
    match area {
        Area::System => tr("System"),
        Area::Workspaces => tr("Workspaces"),
        Area::Calendar => tr("Calendar"),
        Area::Tasks => tr("Tasks"),
        Area::Contacts => tr("Contacts"),
        Area::Github => tr("GitHub"),
        Area::Inbox => tr("Notification Hub"),
        Area::Mail => tr("Mail"),
        Area::Chat => tr("Chat (Talk)"),
        Area::Presence => tr("Presence"),
        Area::Accounts => tr("Accounts"),
        Area::Ai => tr("AI"),
        Area::Assistant => tr("Assistant"),
        Area::Bots => tr("Bots"),
        Area::Workflows => tr("Workflows"),
        Area::Tools => tr("Tools"),
        Area::Extensions => tr("MCP servers"),
        Area::Settings => tr("Settings"),
        Area::Apps => tr("Apps"),
        Area::Clients => tr("API and MCP"),
        Area::Approvals => tr("Approvals"),
    }
}

/// The app "Open …" under an unfolded tool line of `area` leads to ([`app_of`]), while it is
/// `installed` (issue #408); `None` without it, and the button is left out.
pub fn open_in(area: Area, installed: impl Fn(AppId) -> bool) -> Option<AppId> {
    app_of(area).filter(|app| installed(*app))
}

/// Whether a file card offers "Send by mail" ([`mail_file_request`]): the new mail opens in the
/// Notification Hub's compose window, so only while it is `installed` (issue #408).
pub fn can_mail_files(installed: impl Fn(AppId) -> bool) -> bool {
    installed(AppId::INBOX)
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

/// Who answers in the chat a tool line shows in (#231): the assistant, or a bot – by its name where
/// the window knows it, `None` before the bots are listed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answerer<'a> {
    Assistant,
    Bot(Option<&'a str>),
}

/// The sentence under a `forbidden` tool line of `area` and whether "Change access" goes with it
/// (issue #133): the reason the core gave (`lack`), and the button only where changing the
/// assistant's rights lifts it – not for what an agent never gets, not for an area switched off in
/// this conversation, not for what only the person may do. Without a reason (an older line, a
/// refusal of the core's own) the general sentence and the button, as before. A command the person
/// turned off names who answers (`who`, #231): in a bot's chat the bot, not the assistant.
pub fn refusal_hint(area: Option<Area>, lack: Option<Lack>, who: Answerer<'_>) -> (String, bool) {
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
        Lack::Denied { area } => match who {
            Answerer::Assistant => trf("You turned this off for the assistant in {area}.", &[("area", &name(area))]),
            Answerer::Bot(Some(bot)) => {
                trf("You turned this off for {bot} in {area}.", &[("bot", &bot), ("area", &name(area))])
            }
            Answerer::Bot(None) => trf("You turned this off in {area}.", &[("area", &name(area))]),
        },
        Lack::ToolOff { tool } => {
            trf("The tool {tool} is switched off in the control centre under MCP servers.", &[("tool", &tool.title())])
        }
        Lack::Tool { tool } => trf("The assistant may not use the tool {tool}.", &[("tool", &tool.title())]),
        // Issue #302: the level for the server, or the tool switched off; both are its rights.
        Lack::Extension => tr("The assistant may not use this tool of the MCP server.").to_owned(),
    };
    (text, lack.in_rights())
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

/// "Open in Mail" on a suggestion's card (issue #605): `apps.open_app` with `message:<account>:<id>`, the
/// mail window on that mail, whose cards accept the suggestion. The core checks that the mail is there
/// and may be read.
pub fn open_mail_request(account: &str, id: &str) -> Request {
    Request::OpenApp { app: AppId::INBOX_MAIL.window_class.to_owned(), target: Some(format!("message:{account}:{id}")) }
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
    use torro_ipc::assistant::{Entry, EntryContent};

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
                about: None,
            })
        );
        let none: (&[Area], Mode) = (&[], Mode::Default);
        assert_eq!(send_request(None, Some(&choice), None, " \n ", none, None), None, "blank");
        assert_eq!(send_request(None, None, None, "Hi", none, None), None, "no engine");
        // Task B.3: a message to a bot names it; the core runs it with the bot's client.
        let to_bot = send_request(None, Some(&choice), None, "Plan Lisbon", none, Some("b0000001"));
        assert!(matches!(to_bot, Some(Request::SendMessage { bot: Some(ref bot), .. }) if bot == "b0000001"));
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
                from: None,
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
    fn tool_lines_lead_to_the_app_that_shows_their_area() {
        assert_eq!(area_of("mail.search_messages"), Some(Area::Mail));
        assert_eq!(area_of("find_tools"), None);
        assert_eq!(app_of(Area::Mail), Some(AppId::INBOX));
        // Issue #408: no "Open …" into an app that is not installed, and no "Send by mail" without
        // the Notification Hub.
        assert_eq!(open_in(Area::Mail, |_| true), Some(AppId::INBOX));
        assert_eq!(open_in(Area::Mail, |app| app != AppId::INBOX), None);
        assert_eq!(open_in(Area::Calendar, |app| app == AppId::CALENDAR), Some(AppId::CALENDAR));
        assert!(can_mail_files(|_| true) && !can_mail_files(|app| app != AppId::INBOX));
    }

    /// Issue #133: a refused tool says why; "Change access" only where the rights lift it.
    #[test]
    fn a_refusal_says_why_and_offers_a_change_only_where_it_helps() {
        let mail = Some(Area::Mail);
        let hint = |lack| refusal_hint(mail, Some(lack), Answerer::Assistant);
        let named = |text: &'static str| trf(text, &[("area", &tr("Mail"))]);
        let older = refusal_hint(mail, None, Answerer::Assistant);
        assert_eq!(older, (named("The assistant has no access to {area}."), true), "older lines");
        // #231: a command turned off names who answers.
        let denied = Some(Lack::Denied { area: Area::Mail });
        assert_eq!(
            hint(Lack::Denied { area: Area::Mail }).0,
            named("You turned this off for the assistant in {area}.")
        );
        let travel = refusal_hint(mail, denied, Answerer::Bot(Some("Travel planner"))).0;
        assert_eq!(
            travel,
            trf("You turned this off for {bot} in {area}.", &[("bot", &"Travel planner"), ("area", &tr("Mail"))])
        );
        let unnamed = refusal_hint(mail, denied, Answerer::Bot(None)).0;
        assert_eq!(unnamed, named("You turned this off in {area}."));
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
        // Issue #225: a tool switched off is the person's switch under MCP servers (task MA.7), a
        // tool not granted its rights.
        let pdf = torro_ipc::tools::ToolId::Pdf;
        let tool = |text: &'static str| trf(text, &[("tool", &"PDF")]);
        let off = hint(Lack::ToolOff { tool: pdf });
        assert_eq!(off, (tool("The tool {tool} is switched off in the control centre under MCP servers."), false));
        let not_granted = hint(Lack::Tool { tool: pdf });
        assert_eq!(not_granted, (tool("The assistant may not use the tool {tool}."), true));
        assert_eq!(area_name(Area::Tools), tr("Tools"));
        // Issue #302: a server's tool refused is the assistant's rights.
        let server = hint(Lack::Extension);
        assert_eq!(server, (tr("The assistant may not use this tool of the MCP server.").to_owned(), true));
        assert_eq!(area_name(Area::Extensions), tr("MCP servers"));
    }

    /// Task WZ.8: the tool lines of the PDF tool say what was done; a file card says its template
    /// and size, and each of its buttons builds its request.
    #[test]
    fn the_file_card_says_what_the_file_is_and_each_button_its_request() {
        assert_eq!(tool_title("tools.create_pdf"), tr("Created PDF"));
        assert_eq!(tool_title("tools.list_tools"), tr("Listed tools"));
        assert_eq!(tool_title("tools.open_file"), tr("Opened file"));
        // Issue #305: the web's lines name what happened, never the search words or the address.
        assert_eq!(tool_title("tools.web_search"), tr("Searched the web"));
        assert_eq!(tool_title("tools.fetch_page"), tr("Read web page"));
        assert_eq!(tool_title("tools.list_search_providers"), tr("Listed search providers"));
        // Issue #304: the tool `files`.
        for (name, title) in [
            ("tools.list_folder", "Listed folder"),
            ("tools.find_files", "Found files"),
            ("tools.create_folder", "Made folder"),
            ("tools.copy_file", "Copied file"),
            ("tools.move_file", "Moved file"),
            ("tools.trash_file", "Moved to trash"),
        ] {
            assert_eq!(tool_title(name), tr(title), "{name}");
        }
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
        assert_eq!(
            open_mail_request("mail:anna@example.org", "m:w2"),
            Request::OpenApp {
                app: "torro-inbox-mail".into(),
                target: Some("message:mail:anna@example.org:m:w2".into())
            }
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
}
