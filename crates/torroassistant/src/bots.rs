//! Bots in the assistant's window (task B.3, `docs/assistent.md` "Bots"), the parts that decide
//! without drawing, kept pure so they can be tested on their own:
//!
//! - [`BotDraft`]: a bot as it is typed in the editor – profile, personality, instructions, skills,
//!   engine and model –, the problems that keep "Save" back ([`BotDraft::problem`], within the
//!   limits of `torro_ipc::bots`) and the `bots.save_bot` it sends ([`BotDraft::request`]).
//! - [`Template`]: the starting points of a bot's personality, the same as the assistant's.
//! - Which conversation a bot's chat opens ([`newest_conversation`]), what a tool line of the
//!   assistant that asked a bot points to ([`asked_bot`]), and the assistant's proposals to ask one
//!   ([`open_proposal`], [`confirm_request`]).
//! - The editor's section "Access" (task B.5, [`BotAccess`]): a level per area a bot can be given
//!   and "Ask first", the line under each area that says what applies ([`detail`]), and the rights
//!   a save lays onto what the core holds then ([`BotAccess::onto`]). What exists now – calendars,
//!   task lists, inbox sources, mail accounts – comes from [`Known`], read each time the editor
//!   opens, as [`Present`].
//!
//! The window reads and writes nothing itself: the bots come from `bots.list_bots` and
//! `bots.get_bot`, every change is a request. A bot's rights are its client's: the editor reads them
//! with `clients.list_clients` and sets them with `clients.update_client` (app key only, like the
//! control centre); `bots.save_bot` carries none, so the assistant's proposals never grant anything.
//! The rest of a bot's rights – which calendars, accounts and repositories, sending mail, admin
//! commands – stays in the control centre at the bot's page (`torroworkplace ui --section bot:<id>`).

use std::collections::BTreeMap;

use iced::widget::text_editor::{Action, Content};
use torro_core::i18n::{tr, trf};
use torro_inbox::{SourceInfo, SourceRef};
use torro_ipc::api::{self, Area, BOT_CLIENT, Confirm, Level, Outbound, Rights, Selection};
use torro_ipc::assistant::{ConversationSummary, Entry, EntryContent, LETTER_AVATAR};
use torro_ipc::bots::{
    Bot, BotSummary, MAX_DESCRIPTION_CHARS, MAX_INSTRUCTIONS_CHARS, MAX_NAME_CHARS, MAX_PERSONALITY_CHARS,
    MAX_SKILL_CHARS, MAX_SKILL_NAME_CHARS, MAX_SKILLS, Skill,
};
use torro_ipc::{AccountInfo, ClientInfo, Request};

use crate::chat::Choice;

/// The catalogue name of asking a bot: the assistant's tool line for it opens the bot's chat.
pub const ASK_BOT: &str = "bots.ask_bot";

/// The control centre's page of the bot `id` (`torroworkplace ui --section bot:<id>`): its rights.
pub fn access_section(id: &str) -> String {
    format!("bot:{id}")
}

/// A starting point for a bot's personality: the assistant's templates (task C.10), so both read
/// the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    Calm,
    Friendly,
    Formal,
}

impl Template {
    pub const ALL: [Template; 3] = [Template::Calm, Template::Friendly, Template::Formal];

    pub fn label(self) -> &'static str {
        match self {
            Template::Calm => tr("Calm and brief"),
            Template::Friendly => tr("Friendly"),
            Template::Formal => tr("Formal"),
        }
    }

    /// The personality it fills in, in the language of the suite: the person reads and changes it.
    pub fn text(self) -> &'static str {
        match self {
            Template::Calm => tr(
                "You are calm and brief. Answer in a few clear sentences, without small talk. Say what you did and what comes next. Ask only when you really need to know something.",
            ),
            Template::Friendly => tr(
                "You are warm and friendly, like a helpful colleague. Speak casually and keep answers short. When it helps, suggest a next step.",
            ),
            Template::Formal => tr(
                "You are polite and formal. Write complete sentences without slang or emoji. Be precise, and give dates and times in full.",
            ),
        }
    }
}

/// One skill as it is typed: its name and its text.
#[derive(Debug)]
pub struct SkillDraft {
    pub name: String,
    pub text: Content,
}

/// A bot as it is typed in the editor: a new one (`id: None`) or the bot `id` as `bots.get_bot`
/// gave it.
#[derive(Debug)]
pub struct BotDraft {
    /// The bot it changes; `None` for a new one.
    pub id: Option<String>,
    pub name: String,
    pub description: String,
    /// One of `torro_ipc::assistant::AVATARS`.
    pub avatar: String,
    pub personality: Content,
    pub instructions: Content,
    pub skills: Vec<SkillDraft>,
    /// The engine and model it answers with when asked; `None` while there is no engine.
    pub choice: Option<Choice>,
    /// "Save" was pressed once: problems show from now on (design skill, "Formulare").
    pub tried: bool,
    /// Saving runs – `bots.save_bot`, then the access (`clients.list_clients`,
    /// `clients.update_client`) –: "Save" takes no second click.
    pub saving: bool,
    /// The editor made a new bot. It stays `true` after the first save stored the bot but not its
    /// access, so its chat still opens once everything is saved.
    pub new: bool,
    /// The bot's own client (`Bot::client`), whose rights the section "Access" sets; `None` for a
    /// new bot until its first save.
    pub client: Option<String>,
    /// The section "Access"; `None` while the bot's rights are read, and when they could not be
    /// ([`BotDraft::access_note`]).
    pub access: Option<BotAccess>,
    /// What went wrong with the access, for the line that says so.
    pub access_note: Option<AccessNote>,
}

/// What went wrong with a bot's access in the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessNote {
    /// Its rights could not be read when the editor opened: the section shows no levels, and
    /// saving leaves the access alone.
    Unreadable,
    /// The bot was saved, its access not: "Save" tries again, "Access…" opens the control centre.
    NotSet,
}

/// A change in the bot editor, one per field and button.
#[derive(Debug, Clone)]
pub enum DraftEdit {
    Name(String),
    Description(String),
    /// A picture of `torro_ipc::assistant::AVATARS`.
    Avatar(String),
    Personality(Action),
    /// A template replaces the personality.
    Template(Template),
    Instructions(Action),
    /// "Add skill": an empty one at the end, while there are fewer than [`MAX_SKILLS`].
    AddSkill,
    /// "Remove skill" at the skill with this place.
    RemoveSkill(usize),
    SkillName(usize, String),
    SkillText(usize, Action),
    /// From the engine picker.
    Engine(Choice),
    /// A segment of an area's level in the section "Access".
    Level(Area, Level),
    /// A segment of "Ask first".
    Confirm(Confirm),
}

/// What a text editor holds as it would be sent: line breaks as `\n`, no white space at the ends.
pub fn editor_text(content: &Content) -> String {
    content.text().replace("\r\n", "\n").trim().to_owned()
}

/// The number of characters `text` counts with in the core's limits (after trimming).
fn chars(text: &str) -> usize {
    text.trim().chars().count()
}

impl BotDraft {
    /// A new bot answering with `choice` (the window's engine), the first letter as its picture.
    pub fn new(choice: Option<Choice>) -> BotDraft {
        BotDraft {
            id: None,
            name: String::new(),
            description: String::new(),
            avatar: LETTER_AVATAR.to_owned(),
            personality: Content::new(),
            instructions: Content::new(),
            skills: Vec::new(),
            choice,
            tried: false,
            saving: false,
            new: true,
            client: None,
            access: None,
            access_note: None,
        }
    }

    /// The bot `bot` to change, as it is stored.
    pub fn of(bot: &Bot) -> BotDraft {
        BotDraft {
            id: Some(bot.id.clone()),
            name: bot.name.clone(),
            description: bot.description.clone(),
            avatar: bot.avatar.clone(),
            personality: Content::with_text(&bot.personality),
            instructions: Content::with_text(&bot.instructions),
            skills: bot
                .skills
                .iter()
                .map(|skill| SkillDraft { name: skill.name.clone(), text: Content::with_text(&skill.text) })
                .collect(),
            choice: Some(Choice { engine: bot.engine.clone(), model: bot.model.clone() }),
            tried: false,
            saving: false,
            new: false,
            client: Some(bot.client.clone()),
            access: None,
            access_note: None,
        }
    }

    /// Applies `edit`; a skill that is not there and a skill beyond [`MAX_SKILLS`] change nothing.
    pub fn apply(&mut self, edit: DraftEdit) {
        match edit {
            DraftEdit::Name(name) => self.name = name,
            DraftEdit::Description(description) => self.description = description,
            DraftEdit::Avatar(avatar) => self.avatar = avatar,
            DraftEdit::Personality(action) => self.personality.perform(action),
            DraftEdit::Template(template) => self.personality = Content::with_text(template.text()),
            DraftEdit::Instructions(action) => self.instructions.perform(action),
            DraftEdit::AddSkill if self.can_add_skill() => {
                self.skills.push(SkillDraft { name: String::new(), text: Content::new() });
            }
            DraftEdit::AddSkill => {}
            DraftEdit::RemoveSkill(at) if at < self.skills.len() => {
                self.skills.remove(at);
            }
            DraftEdit::RemoveSkill(_) => {}
            DraftEdit::SkillName(at, name) => {
                if let Some(skill) = self.skills.get_mut(at) {
                    skill.name = name;
                }
            }
            DraftEdit::SkillText(at, action) => {
                if let Some(skill) = self.skills.get_mut(at) {
                    skill.text.perform(action);
                }
            }
            DraftEdit::Engine(choice) => self.choice = Some(choice),
            DraftEdit::Level(area, level) => {
                if let Some(access) = self.access.as_mut() {
                    access.set_level(area, level);
                }
            }
            DraftEdit::Confirm(confirm) => {
                if let Some(access) = self.access.as_mut() {
                    access.set_confirm(confirm);
                }
            }
        }
    }

    /// Whether "Add skill" can add one more: at most [`MAX_SKILLS`].
    pub fn can_add_skill(&self) -> bool {
        self.skills.len() < MAX_SKILLS
    }

    /// The first thing that keeps "Save" from sending, for people, in the order of the form; `None`
    /// when the bot keeps every limit of `bots.save_bot` (`torro_ipc::bots::limit_refusal`) and has
    /// an engine. The window shows it only after the first "Save" ([`BotDraft::tried`]).
    pub fn problem(&self) -> Option<String> {
        let too_long = |what: &'static str, count: usize, max: usize| trf(what, &[("count", &count), ("max", &max)]);
        let name = self.name.trim();
        if name.is_empty() {
            return Some(tr("Give the bot a name.").to_owned());
        }
        if name.chars().count() > MAX_NAME_CHARS {
            return Some(too_long("The name is too long: {count} of {max} characters.", chars(name), MAX_NAME_CHARS));
        }
        if chars(&self.description) > MAX_DESCRIPTION_CHARS {
            let count = chars(&self.description);
            return Some(too_long(
                "The description is too long: {count} of {max} characters.",
                count,
                MAX_DESCRIPTION_CHARS,
            ));
        }
        let personality = chars(&editor_text(&self.personality));
        if personality > MAX_PERSONALITY_CHARS {
            return Some(too_long(
                "The personality is too long: {count} of {max} characters.",
                personality,
                MAX_PERSONALITY_CHARS,
            ));
        }
        let instructions = chars(&editor_text(&self.instructions));
        if instructions > MAX_INSTRUCTIONS_CHARS {
            return Some(too_long(
                "The instructions are too long: {count} of {max} characters.",
                instructions,
                MAX_INSTRUCTIONS_CHARS,
            ));
        }
        for skill in &self.skills {
            let name = skill.name.trim();
            if name.is_empty() {
                return Some(tr("Give every skill a name.").to_owned());
            }
            if name.chars().count() > MAX_SKILL_NAME_CHARS {
                return Some(trf(
                    "A skill's name is at most {max} characters: “{name}”.",
                    &[("max", &MAX_SKILL_NAME_CHARS), ("name", &name)],
                ));
            }
            let count = chars(&editor_text(&skill.text));
            if count > MAX_SKILL_CHARS {
                return Some(trf(
                    "The skill “{name}” is too long: {count} of {max} characters.",
                    &[("name", &name), ("count", &count), ("max", &MAX_SKILL_CHARS)],
                ));
            }
        }
        if self.choice.is_none() {
            return Some(tr("Add an AI account first: the bot needs an engine to answer with.").to_owned());
        }
        None
    }

    /// The `bots.save_bot` of "Save": the whole bot (a changed one replaces the stored one), texts
    /// trimmed; `None` while there is a [`BotDraft::problem`].
    pub fn request(&self) -> Option<Request> {
        if self.problem().is_some() {
            return None;
        }
        let choice = self.choice.as_ref()?;
        Some(Request::SaveBot {
            id: self.id.clone(),
            name: self.name.trim().to_owned(),
            description: self.description.trim().to_owned(),
            avatar: Some(self.avatar.clone()),
            engine: choice.engine.clone(),
            model: choice.model.clone(),
            personality: editor_text(&self.personality),
            instructions: editor_text(&self.instructions),
            skills: self
                .skills
                .iter()
                .map(|skill| Skill { name: skill.name.trim().to_owned(), text: editor_text(&skill.text) })
                .collect(),
        })
    }
}

/// What exists now, as the core listed it when the bot editor opened (task B.5): the answers of
/// `calendar.list_calendars`, `tasks.list_lists` (ids only), `inbox.list_sources`,
/// `accounts.list_accounts` and `clients.list_clients`. Each comes on its own ([`Listed`]); what
/// did not come stays empty.
#[derive(Debug, Clone, Default)]
pub struct Known {
    pub calendars: Vec<String>,
    pub task_lists: Vec<String>,
    pub sources: Vec<SourceInfo>,
    pub accounts: Vec<AccountInfo>,
    pub clients: Vec<ClientInfo>,
}

/// One answer for [`Known`].
#[derive(Debug, Clone)]
pub enum Listed {
    /// The ids of the calendars.
    Calendars(Vec<String>),
    /// The ids of the task lists, GitHub filter lists included.
    TaskLists(Vec<String>),
    Sources(Vec<SourceInfo>),
    Accounts(Vec<AccountInfo>),
}

impl Known {
    /// Takes the answer `listed`.
    pub fn take(&mut self, listed: Listed) {
        match listed {
            Listed::Calendars(ids) => self.calendars = ids,
            Listed::TaskLists(ids) => self.task_lists = ids,
            Listed::Sources(sources) => self.sources = sources,
            Listed::Accounts(accounts) => self.accounts = accounts,
        }
    }

    /// What exists now per selection, as the control centre offers it: the inbox sources by
    /// `api::present_sources`, the signed-in mail accounts by `api::signed_in_mail`.
    pub fn present(&self) -> Present {
        Present {
            calendars: self.calendars.clone(),
            task_lists: self.task_lists.clone(),
            inbox_sources: api::present_sources(&self.sources, &self.clients, &self.accounts)
                .iter()
                .map(SourceRef::key)
                .collect(),
            mail_accounts: api::signed_in_mail(&self.accounts).map(|account| account.id.clone()).collect(),
        }
    }
}

/// The ids of what exists now, per selection of a bot's rights: what switching an area on gives it
/// (`Rights::set_level`), and the "of {m}" of a line under an area ([`detail`]).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Present {
    pub calendars: Vec<String>,
    pub task_lists: Vec<String>,
    /// By their key (`agent:claude-code`, `mail:anna@example.org`).
    pub inbox_sources: Vec<String>,
    pub mail_accounts: Vec<String>,
}

impl Present {
    /// The ids for the selection of `area`; empty for GitHub, which has no list of every
    /// repository, and for an area without a selection.
    pub fn of(&self, area: Area) -> &[String] {
        match area {
            Area::Calendar => &self.calendars,
            Area::Tasks => &self.task_lists,
            Area::Inbox => &self.inbox_sources,
            Area::Mail => &self.mail_accounts,
            _ => &[],
        }
    }
}

/// The section "Access" of the bot editor (task B.5): the level of every area a bot can be given
/// and "Ask first", changed only by the person's clicks ([`DraftEdit::Level`],
/// [`DraftEdit::Confirm`]) – never by a proposal of the assistant or anything in a chat.
///
/// It keeps the rights the editor read when it opened and the rows the person set. A save lays only the rows that differ from what was read onto the rights the core
/// holds then ([`BotAccess::onto`]): what changed meanwhile in the control centre – the selections,
/// sending, the admin switches, other rows – stays.
#[derive(Debug, Clone, PartialEq)]
pub struct BotAccess {
    stored: Rights,
    levels: BTreeMap<Area, Level>,
    confirm: Option<Confirm>,
}

impl BotAccess {
    /// The section for a bot whose client holds `stored`.
    pub fn new(stored: Rights) -> BotAccess {
        BotAccess { stored, levels: BTreeMap::new(), confirm: None }
    }

    /// The section for the bot whose client is `client`, from the answer of `clients.list_clients`:
    /// a new bot (no client yet) starts with no rights at all (`Rights::none`), as the core gives
    /// it; `None` when the client is not listed – then its rights are unknown.
    pub fn of(client: Option<&str>, clients: &[ClientInfo]) -> Option<BotAccess> {
        match client {
            None => Some(BotAccess::new(Rights::none())),
            Some(id) => clients.iter().find(|info| info.id == id).map(|info| BotAccess::new(info.rights.clone())),
        }
    }

    /// The areas the section shows, in the catalogue's order: every grantable one but
    /// `assistant` and `bots`, which the core never opens to a bot (`api::shows_area`).
    pub fn areas() -> impl Iterator<Item = Area> {
        Area::ALL.into_iter().filter(|&area| api::shows_area(area, BOT_CLIENT))
    }

    /// The person chose `level` for `area`; an area the section does not show changes nothing.
    pub fn set_level(&mut self, area: Area, level: Level) {
        if api::shows_area(area, BOT_CLIENT) {
            self.levels.insert(area, level);
        }
    }

    /// The person chose `confirm` for "Ask first".
    pub fn set_confirm(&mut self, confirm: Confirm) {
        self.confirm = Some(confirm);
    }

    /// The rows whose level the person set to something else than was read.
    fn changed_levels(&self) -> impl Iterator<Item = (Area, Level)> + '_ {
        self.levels
            .iter()
            .map(|(&area, &level)| (area, level))
            .filter(|&(area, level)| self.stored.area(area).level != level)
    }

    /// "Ask first", when the person set it to something else than was read.
    fn changed_confirm(&self) -> Option<Confirm> {
        self.confirm.filter(|&confirm| confirm != self.stored.confirm)
    }

    /// Whether a row differs from what was read: only then does a save touch the access.
    pub fn changed(&self) -> bool {
        self.changed_levels().next().is_some() || self.changed_confirm().is_some()
    }

    /// `current` – the client's rights as the core holds them at saving – with the changed rows
    /// laid on: each changed level through `Rights::set_level` (switching an area on with an empty
    /// selection gives it what is in `present`; GitHub stays empty) and a changed "Ask first".
    /// Everything else of `current` stays.
    pub fn onto(&self, current: &Rights, present: &Present) -> Rights {
        let mut rights = current.clone();
        for (area, level) in self.changed_levels() {
            rights.set_level(area, level, || present.of(area).to_vec());
        }
        if let Some(confirm) = self.changed_confirm() {
            rights.confirm = confirm;
        }
        rights
    }

    /// The rights the section shows: what was read with the person's rows laid on.
    pub fn shown(&self, present: &Present) -> Rights {
        self.onto(&self.stored, present)
    }
}

/// What an area holds, under its name while it is off; the control centre's words.
fn area_detail(area: Area) -> &'static str {
    match area {
        Area::Workspaces => tr("Templates of your windows"),
        Area::Calendar => tr("Calendars and events"),
        Area::Tasks => tr("Task lists and tasks"),
        Area::Github => tr("Issues and pull requests in filter lists"),
        Area::Inbox => tr("Entries of agents and scripts"),
        Area::Mail => tr("Folders, messages and drafts"),
        Area::Accounts => tr("Connected accounts and sync"),
        Area::Ai => tr("Models of your AI accounts"),
        Area::Assistant => tr("Chats with the assistant, which acts with its own rights"),
        Area::Bots => tr("Bots and asking them, which act with their own rights"),
        Area::Settings => tr("Settings of the suite"),
        Area::Apps => tr("Opening the suite's apps"),
        Area::System | Area::Clients | Area::Approvals => "",
    }
}

/// The line under an area of the section "Access" in `rights`: what applies.
///
/// - off: what the area holds ("Calendars and events"), or "Admin commands allowed" while its
///   admin switch is on in the control centre (the editor keeps it when the level changes),
/// - a selection: "All calendars", "2 of 3 calendars" (counting what exists in `present`), "No
///   calendar"; GitHub "3 repositories" or "No repositories · choose in Torro Workplace",
/// - mail from "Write" on adds sending: "1 of 2 mail accounts · sending: never",
/// - an area without a selection: what it holds.
pub fn detail(area: Area, rights: &Rights, present: &Present) -> String {
    let held = rights.area(area);
    if held.level == Level::NoAccess {
        return if held.admin { tr("Admin commands allowed") } else { area_detail(area) }.to_owned();
    }
    let Some(selection) = rights.selection(area) else { return area_detail(area).to_owned() };
    let reach = match (area, selection) {
        (Area::Github, Selection::All) => tr("All repositories").to_owned(),
        (Area::Github, Selection::Only(names)) => match names.len() {
            0 => tr("No repositories · choose in Torro Workplace").to_owned(),
            1 => tr("1 repository").to_owned(),
            n => trf("{n} repositories", &[("n", &n)]),
        },
        (_, selection) => counted(area, selection, present.of(area)),
    };
    if area != Area::Mail || held.level < Level::Write {
        return reach;
    }
    let sending = match rights.outbound {
        Outbound::Never => tr("sending: never"),
        Outbound::WithApproval => tr("sending: asks each time"),
        Outbound::Free => tr("sending: always"),
    };
    format!("{reach} · {sending}")
}

/// "All calendars", "No calendar" or "{n} of {m} calendars" for the selection of `area`, `n`
/// counting what of it exists in `present`.
fn counted(area: Area, selection: &Selection, present: &[String]) -> String {
    let (all, none, some) = match area {
        Area::Calendar => (tr("All calendars"), tr("No calendar"), "{n} of {m} calendars"),
        Area::Tasks => (tr("All task lists"), tr("No task list"), "{n} of {m} task lists"),
        Area::Inbox => (tr("All sources"), tr("No source"), "{n} of {m} sources"),
        _ => (tr("All mail accounts"), tr("No mail account"), "{n} of {m} mail accounts"),
    };
    let Selection::Only(ids) = selection else { return all.to_owned() };
    match present.iter().filter(|id| ids.contains(id)).count() {
        0 => none.to_owned(),
        n => trf(some, &[("n", &n), ("m", &present.len())]),
    }
}

/// The conversation a bot's chat opens (`--bot <id>`, a click on the bot in the sidebar): its
/// newest; `None` starts a new one.
pub fn newest_conversation<'a>(conversations: &'a [ConversationSummary], bot: &str) -> Option<&'a ConversationSummary> {
    conversations.iter().filter(|c| c.bot.as_deref() == Some(bot)).max_by_key(|c| c.updated)
}

/// The bot `id` as listed.
pub fn find<'a>(bots: &'a [BotSummary], id: &str) -> Option<&'a BotSummary> {
    bots.iter().find(|bot| bot.id == id)
}

/// What a tool line of the assistant that asked a bot points to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asked {
    /// The bot's id, from the tool's input.
    pub bot: String,
    /// The bot's conversation, when the tool's input names one; else the bot's chat opens its
    /// newest.
    pub conversation: Option<String>,
}

/// For a tool line `name` with `input`: the bot it asked, when it is `bots.ask_bot` with a `bot`
/// in its input. The input is the model's, so only an id-like word is taken.
pub fn asked_bot(name: &str, input: &serde_json::Value) -> Option<Asked> {
    if name != ASK_BOT {
        return None;
    }
    let word = |key: &str| input.get(key).and_then(serde_json::Value::as_str).filter(|id| is_id(id)).map(str::to_owned);
    Some(Asked { bot: word("bot")?, conversation: word("conversation") })
}

/// A word that can be an id: not empty, within the core's limit, letters, digits and `-`, `_`.
fn is_id(word: &str) -> bool {
    !word.is_empty()
        && word.len() <= torro_ipc::assistant::MAX_ID_BYTES
        && word.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

/// A proposal of the assistant to ask a bot, which the person confirms in the chat.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Proposal {
    /// The assistant's conversation.
    pub conversation: String,
    /// The bot it would ask.
    pub bot: String,
}

/// The proposal in `entry` of the assistant's conversation `conversation`: a `bots.ask_bot` the core
/// refused (`forbidden`) because the person had not asked for the bot (task B.2). `None` for every
/// other entry.
pub fn proposal(conversation: &str, entry: &Entry) -> Option<Proposal> {
    let EntryContent::Tool { name, input, error: Some(error), .. } = &entry.content else { return None };
    if error != "forbidden" {
        return None;
    }
    let asked = asked_bot(name, input)?;
    Some(Proposal { conversation: conversation.to_owned(), bot: asked.bot })
}

/// The proposal the person can still confirm in the assistant's conversation `conversation`, with
/// the number of its entry: the newest refused `bots.ask_bot` ([`proposal`]) that no message of
/// the person and no answer of the bot came after. Once the person wrote again – or the bot was
/// asked – the card goes. `None` without such an entry.
pub fn open_proposal(conversation: &str, entries: &[Entry]) -> Option<(u64, Proposal)> {
    for entry in entries.iter().rev() {
        if let Some(found) = proposal(conversation, entry) {
            return Some((entry.seq, found));
        }
        match &entry.content {
            EntryContent::Message { .. } => return None,
            EntryContent::Tool { name, error: None, .. } if name == ASK_BOT => return None,
            _ => {}
        }
    }
    None
}

/// The request of "Ask {bot}" on a proposal card: `assistant.confirm_bot { conversation, bot }`
/// (task B.2). Only the app key may send it, and only for a person's click: it lets the assistant
/// ask that bot once in that conversation.
pub fn confirm_request(proposal: &Proposal) -> Request {
    Request::ConfirmBot { conversation: proposal.conversation.clone(), bot: proposal.bot.clone() }
}

/// The message that follows a confirmation, so the assistant goes on: "Yes, ask Travel planner."
/// It names the bot, as the person would.
pub fn confirm_text(bot: &str) -> String {
    trf("Yes, ask {bot}.", &[("bot", &bot)])
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use torro_ipc::api::AreaRights;
    use torro_ipc::assistant::AVATARS;

    use super::*;

    fn bot() -> Bot {
        Bot {
            id: "b0000001".into(),
            name: "Travel planner".into(),
            description: "Plans trips by train".into(),
            avatar: "sun".into(),
            engine: "ai:ollama@localhost:11434".into(),
            model: Some("qwen3:14b".into()),
            personality: "Calm.".into(),
            instructions: "Trains first.".into(),
            skills: vec![Skill { name: "Packing".into(), text: "Pack light.".into() }],
            client: "c0000001".into(),
        }
    }

    fn tried(mut draft: BotDraft) -> Option<String> {
        draft.tried = true;
        draft.problem()
    }

    #[test]
    fn a_stored_bot_is_saved_whole_with_its_id() {
        let draft = BotDraft::of(&bot());
        assert_eq!(draft.problem(), None);
        assert_eq!(
            draft.request(),
            Some(Request::SaveBot {
                id: Some("b0000001".into()),
                name: "Travel planner".into(),
                description: "Plans trips by train".into(),
                avatar: Some("sun".into()),
                engine: "ai:ollama@localhost:11434".into(),
                model: Some("qwen3:14b".into()),
                personality: "Calm.".into(),
                instructions: "Trains first.".into(),
                skills: vec![Skill { name: "Packing".into(), text: "Pack light.".into() }],
            })
        );
    }

    #[test]
    fn a_new_bot_is_saved_without_id_and_trimmed() {
        let choice = Choice { engine: "test".into(), model: None };
        let mut draft = BotDraft::new(Some(choice));
        assert_eq!(draft.avatar, LETTER_AVATAR);
        assert_eq!(tried(BotDraft::new(None)), Some(tr("Give the bot a name.").to_owned()));
        draft.name = "  Release helper ".into();
        draft.skills.push(SkillDraft { name: " Steps ".into(), text: Content::with_text(" Tag, build. \n") });
        let Some(Request::SaveBot { id, name, skills, engine, avatar, .. }) = draft.request() else {
            panic!("a save");
        };
        assert_eq!(
            (id, name.as_str(), engine.as_str(), avatar.as_deref()),
            (None, "Release helper", "test", Some("letter"))
        );
        assert_eq!(skills, vec![Skill { name: "Steps".into(), text: "Tag, build.".into() }]);
    }

    #[test]
    fn the_editor_keeps_the_limits_of_the_core() {
        let with = |change: &dyn Fn(&mut BotDraft)| {
            let mut draft = BotDraft::of(&bot());
            change(&mut draft);
            (tried(BotDraft { ..draft_clone(&draft) }), draft.request().is_none())
        };
        assert_eq!(with(&|_| {}), (None, false));
        let refused = |result: (Option<String>, bool)| result.0.is_some() && result.1;
        assert!(refused(with(&|d| d.name = " ".into())));
        assert!(refused(with(&|d| d.name = "a".repeat(MAX_NAME_CHARS + 1))));
        assert_eq!(with(&|d| d.name = "a".repeat(MAX_NAME_CHARS)).0, None);
        assert!(refused(with(&|d| d.description = "a".repeat(MAX_DESCRIPTION_CHARS + 1))));
        assert!(refused(with(&|d| d.personality = Content::with_text(&"a".repeat(MAX_PERSONALITY_CHARS + 1)))));
        assert_eq!(with(&|d| d.personality = Content::with_text(&"a".repeat(MAX_PERSONALITY_CHARS))).0, None);
        assert!(refused(with(&|d| d.instructions = Content::with_text(&"a".repeat(MAX_INSTRUCTIONS_CHARS + 1)))));
        assert!(refused(with(&|d| d.skills[0].name = String::new())));
        assert!(refused(with(&|d| d.skills[0].name = "a".repeat(MAX_SKILL_NAME_CHARS + 1))));
        assert!(refused(with(&|d| d.skills[0].text = Content::with_text(&"a".repeat(MAX_SKILL_CHARS + 1)))));
        assert!(refused(with(&|d| d.choice = None)), "no engine");
        assert_eq!(
            with(&|d| d.instructions = Content::with_text(&"a".repeat(MAX_INSTRUCTIONS_CHARS + 1))).0,
            Some(trf(
                "The instructions are too long: {count} of {max} characters.",
                &[("count", &(MAX_INSTRUCTIONS_CHARS + 1)), ("max", &MAX_INSTRUCTIONS_CHARS)]
            ))
        );

        let mut full = BotDraft::of(&bot());
        for _ in 0..=MAX_SKILLS {
            full.apply(DraftEdit::AddSkill);
        }
        assert_eq!(full.skills.len(), MAX_SKILLS, "never more than the core takes");
        full.apply(DraftEdit::RemoveSkill(MAX_SKILLS));
        full.apply(DraftEdit::RemoveSkill(0));
        assert_eq!(full.skills.len(), MAX_SKILLS - 1);
        full.apply(DraftEdit::SkillName(99, "nowhere".into()));
        full.apply(DraftEdit::Template(Template::Formal));
        assert_eq!(editor_text(&full.personality), Template::Formal.text());
        full.apply(DraftEdit::Engine(Choice { engine: "test".into(), model: None }));
        assert_eq!(full.choice.map(|choice| choice.engine), Some("test".into()));
    }

    /// A copy of `draft` (the editor's text fields cannot be cloned).
    fn draft_clone(draft: &BotDraft) -> BotDraft {
        BotDraft {
            id: draft.id.clone(),
            name: draft.name.clone(),
            description: draft.description.clone(),
            avatar: draft.avatar.clone(),
            personality: Content::with_text(&draft.personality.text()),
            instructions: Content::with_text(&draft.instructions.text()),
            skills: draft
                .skills
                .iter()
                .map(|skill| SkillDraft { name: skill.name.clone(), text: Content::with_text(&skill.text.text()) })
                .collect(),
            choice: draft.choice.clone(),
            tried: draft.tried,
            saving: draft.saving,
            new: draft.new,
            client: draft.client.clone(),
            access: draft.access.clone(),
            access_note: draft.access_note,
        }
    }

    #[test]
    fn templates_are_the_assistants_and_fit() {
        for template in Template::ALL {
            assert!(!template.label().is_empty() && template.text().chars().count() <= MAX_PERSONALITY_CHARS);
        }
        assert!(AVATARS.contains(&LETTER_AVATAR));
    }

    fn summary(id: &str, bot: Option<&str>, minute: u32) -> ConversationSummary {
        let at = Utc.with_ymd_and_hms(2026, 10, 14, 9, minute, 0).single().expect("a time");
        ConversationSummary {
            id: id.into(),
            title: id.into(),
            engine: "test".into(),
            model: None,
            effort: None,
            created: at,
            updated: at,
            pinned: false,
            running: false,
            last: 1,
            bot: bot.map(str::to_owned),
            bot_deleted: false,
        }
    }

    #[test]
    fn a_bots_chat_opens_its_newest_conversation() {
        let list = [
            summary("a", Some("b1"), 1),
            summary("b", Some("b1"), 5),
            summary("c", None, 9),
            summary("d", Some("b2"), 7),
        ];
        assert_eq!(newest_conversation(&list, "b1").map(|c| c.id.as_str()), Some("b"));
        assert_eq!(newest_conversation(&list, "b3"), None, "none yet: a new one");
        assert_eq!(access_section("b1"), "bot:b1");
    }

    fn tool(name: &str, input: serde_json::Value, error: Option<&str>) -> Entry {
        let time = Utc.with_ymd_and_hms(2026, 10, 14, 9, 0, 0).single().expect("a time");
        Entry {
            seq: 2,
            time,
            content: EntryContent::Tool {
                name: name.into(),
                input,
                summary: "3 steps".into(),
                error: error.map(str::to_owned),
                lack: None,
            },
        }
    }

    #[test]
    fn a_tool_line_that_asked_a_bot_points_to_its_chat() {
        let input = serde_json::json!({"bot": "b1", "text": "Plan it", "conversation": "c9"});
        assert_eq!(asked_bot(ASK_BOT, &input), Some(Asked { bot: "b1".into(), conversation: Some("c9".into()) }));
        let input = serde_json::json!({"bot": "b1", "text": "Plan it"});
        assert_eq!(asked_bot(ASK_BOT, &input), Some(Asked { bot: "b1".into(), conversation: None }));
        assert_eq!(asked_bot(ASK_BOT, &serde_json::json!({"bot": "../x y"})), None, "the model's text is no id");
        assert_eq!(asked_bot("mail.search_messages", &serde_json::json!({"bot": "b1"})), None);
    }

    #[test]
    fn a_refused_ask_is_a_proposal_until_the_person_answers() {
        let refused = tool(ASK_BOT, serde_json::json!({"bot": "b1", "text": "Plan it"}), Some("forbidden"));
        let found = proposal("c1", &refused);
        assert_eq!(found, Some(Proposal { conversation: "c1".into(), bot: "b1".into() }));
        assert_eq!(
            found.as_ref().map(confirm_request),
            Some(Request::ConfirmBot { conversation: "c1".into(), bot: "b1".into() })
        );
        assert_eq!(proposal("c1", &tool(ASK_BOT, serde_json::json!({"bot": "b1"}), None)), None, "it ran");
        assert_eq!(proposal("c1", &tool(ASK_BOT, serde_json::json!({"bot": "b1"}), Some("busy"))), None);

        let time = Utc.with_ymd_and_hms(2026, 10, 14, 9, 0, 0).single().expect("a time");
        let said = |seq: u64| Entry {
            seq,
            time,
            content: EntryContent::Message {
                text: "No".into(),
                engine: "test".into(),
                model: None,
                effort: None,
                without: Vec::new(),
                mode: torro_ipc::assistant::Mode::Default,
            },
        };
        let answer = |seq: u64| Entry {
            seq,
            time,
            content: EntryContent::Answer { text: "Shall I ask it?".into(), engine: "test".into(), model: None },
        };
        let asked = |seq: u64| Entry { seq, ..tool(ASK_BOT, serde_json::json!({"bot": "b1"}), None) };
        let refused = Entry { seq: 2, ..refused };
        let open = open_proposal("c1", &[said(1), refused.clone(), answer(3)]);
        assert_eq!(open.map(|(seq, proposal)| (seq, proposal.bot)), Some((2, "b1".into())), "the card waits");
        assert_eq!(open_proposal("c1", &[said(1), refused.clone(), answer(3), said(4)]), None, "answered");
        assert_eq!(open_proposal("c1", &[said(1), refused, asked(3)]), None, "asked meanwhile");
        assert_eq!(confirm_text("Travel planner"), trf("Yes, ask {bot}.", &[("bot", &"Travel planner")]));
    }

    // -- The section "Access" (task B.5) ---------------------------------------------------------

    fn present() -> Present {
        Present {
            calendars: vec!["acc/work".into(), "acc/home".into(), "acc/family".into()],
            task_lists: vec!["acc/todo".into()],
            inbox_sources: vec!["agent:claude-code".into(), "script:backup".into()],
            mail_accounts: Vec::new(),
        }
    }

    fn read() -> AreaRights {
        AreaRights { level: Level::Read, admin: false }
    }

    #[test]
    fn a_new_bot_starts_with_nothing_and_without_a_change_sends_no_access() {
        let mut access = BotAccess::of(None, &[]).expect("a new bot has no client yet");
        assert_eq!(access.shown(&present()), Rights::none());
        assert!(!access.changed());
        access.set_level(Area::Calendar, Level::NoAccess);
        access.set_confirm(Confirm::OnDelete);
        assert!(!access.changed(), "the same as read is no change");
        access.set_level(Area::Calendar, Level::Read);
        access.set_level(Area::Calendar, Level::NoAccess);
        assert!(!access.changed(), "back where it was");
        assert_eq!(BotAccess::of(Some("c9"), &[]), None, "a client that is not listed: unknown rights");
    }

    #[test]
    fn only_the_changed_rows_land_on_the_rights_the_core_holds_now() {
        let mut stored = Rights::none();
        stored.areas.insert(Area::Calendar, read());
        stored.calendars = Selection::Only(vec!["acc/work".into()]);
        let mut access = BotAccess::new(stored);
        access.set_level(Area::Calendar, Level::Read);
        access.set_level(Area::Tasks, Level::Write);
        access.set_confirm(Confirm::OnEveryChange);
        assert!(access.changed());

        // Meanwhile, in the control centre: another calendar, sending, a higher calendar level.
        let mut current = Rights::none();
        current.areas.insert(Area::Calendar, AreaRights { level: Level::Write, admin: false });
        current.calendars = Selection::Only(vec!["acc/work".into(), "acc/home".into()]);
        current.outbound = Outbound::WithApproval;
        let saved = access.onto(&current, &present());
        assert_eq!(saved.area(Area::Calendar).level, Level::Write, "a row the person left stays");
        assert_eq!(saved.calendars, current.calendars, "a selection changed meanwhile stays");
        assert_eq!(saved.outbound, Outbound::WithApproval, "sending stays");
        assert_eq!(saved.area(Area::Tasks).level, Level::Write);
        assert_eq!(saved.task_lists, Selection::Only(vec!["acc/todo".into()]), "switched on: what exists");
        assert_eq!(saved.confirm, Confirm::OnEveryChange);
    }

    #[test]
    fn switching_an_area_on_gives_what_exists_and_says_so() {
        let mut access = BotAccess::new(Rights::none());
        for area in [Area::Calendar, Area::Github, Area::Inbox, Area::Mail] {
            access.set_level(area, Level::Read);
        }
        let present = present();
        let shown = access.shown(&present);
        assert_eq!(shown.calendars, Selection::Only(present.calendars.clone()), "never All");
        assert_eq!(shown.inbox_sources, Selection::Only(present.inbox_sources.clone()));
        assert_eq!(shown.mail_accounts, Selection::Only(Vec::new()), "no mail account to give");
        assert_eq!(shown.github_repos, Selection::Only(Vec::new()), "no list of every repository");
        let line = |area| detail(area, &shown, &present);
        assert_eq!(line(Area::Calendar), trf("{n} of {m} calendars", &[("n", &3), ("m", &3)]));
        assert_eq!(line(Area::Mail), tr("No mail account"));
        assert_eq!(line(Area::Github), tr("No repositories · choose in Torro Workplace"));
        assert_eq!(line(Area::Inbox), trf("{n} of {m} sources", &[("n", &2), ("m", &2)]));
        assert_eq!(line(Area::Tasks), tr("Task lists and tasks"), "off: what it holds");

        // A selection that names something stays as it is.
        let mut stored = Rights::none();
        stored.calendars = Selection::Only(vec!["acc/home".into()]);
        let mut access = BotAccess::new(stored);
        access.set_level(Area::Calendar, Level::Read);
        let shown = access.shown(&present);
        assert_eq!(shown.calendars, Selection::Only(vec!["acc/home".into()]));
        assert_eq!(detail(Area::Calendar, &shown, &present), trf("{n} of {m} calendars", &[("n", &1), ("m", &3)]));
    }

    #[test]
    fn the_assistant_and_the_bots_never_appear() {
        assert!(BotAccess::areas().all(|area| !matches!(area, Area::Assistant | Area::Bots)));
        assert!(BotAccess::areas().any(|area| area == Area::Calendar));
        let mut access = BotAccess::new(Rights::none());
        access.set_level(Area::Bots, Level::Full);
        access.set_level(Area::Assistant, Level::Read);
        assert!(!access.changed());
        let saved = access.onto(&Rights::none(), &present());
        assert_eq!(saved.area(Area::Bots).level, Level::NoAccess);
        assert_eq!(saved.area(Area::Assistant).level, Level::NoAccess);
    }

    #[test]
    fn a_row_that_is_off_with_its_admin_switch_on_says_so() {
        let mut stored = Rights::none();
        stored.areas.insert(Area::Inbox, AreaRights { level: Level::NoAccess, admin: true });
        let present = present();
        assert_eq!(detail(Area::Inbox, &stored, &present), tr("Admin commands allowed"));
        let mut access = BotAccess::new(stored);
        access.set_level(Area::Inbox, Level::Read);
        assert!(access.shown(&present).area(Area::Inbox).admin, "the switch stays, as in the control centre");
    }

    #[test]
    fn the_lines_say_what_applies() {
        let present = present();
        let mut rights = Rights::none();
        rights.areas.insert(Area::Calendar, read());
        rights.calendars = Selection::All;
        rights.areas.insert(Area::Mail, AreaRights { level: Level::Write, admin: false });
        rights.mail_accounts = Selection::Only(vec!["mail:a@example.org".into()]);
        rights.areas.insert(Area::Github, read());
        rights.github_repos = Selection::Only(vec!["anna/torro".into(), "acme".into()]);
        rights.areas.insert(Area::Settings, read());
        let present =
            Present { mail_accounts: vec!["mail:a@example.org".into(), "mail:b@example.org".into()], ..present };
        assert_eq!(detail(Area::Calendar, &rights, &present), tr("All calendars"));
        assert_eq!(
            detail(Area::Mail, &rights, &present),
            format!("{} · {}", trf("{n} of {m} mail accounts", &[("n", &1), ("m", &2)]), tr("sending: never"))
        );
        assert_eq!(detail(Area::Github, &rights, &present), trf("{n} repositories", &[("n", &2)]));
        assert_eq!(detail(Area::Settings, &rights, &present), tr("Settings of the suite"));
        rights.areas.insert(Area::Mail, read());
        assert_eq!(
            detail(Area::Mail, &rights, &present),
            trf("{n} of {m} mail accounts", &[("n", &1), ("m", &2)]),
            "reading sends nothing"
        );
    }

    #[test]
    fn the_editor_takes_levels_only_from_its_section() {
        let mut draft = BotDraft::new(None);
        draft.apply(DraftEdit::Level(Area::Calendar, Level::Read));
        assert!(draft.access.is_none(), "nothing to change before the rights came");
        draft.access = BotAccess::of(None, &[]);
        draft.apply(DraftEdit::Level(Area::Calendar, Level::Read));
        draft.apply(DraftEdit::Confirm(Confirm::Never));
        let access = draft.access.as_ref().expect("the section");
        assert!(access.changed());
        assert_eq!(access.shown(&present()).confirm, Confirm::Never);
    }
}
