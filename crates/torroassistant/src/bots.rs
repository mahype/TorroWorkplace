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
//!
//! The window reads and writes nothing itself: the bots come from `bots.list_bots` and
//! `bots.get_bot`, every change is a request. **What a bot may use is not written here** (task
//! S.18): a bot's rights are its client's, and `clients.*` is the app key's alone (point 2 of "Was
//! nie nach außen geht" in `docs/api.md`). The editor only points at the bot's page in the control
//! centre (`torroworkplace ui --section bot:<id>`, [`access_section`]); `bots.save_bot` carries no
//! rights, so the assistant's proposals never grant anything.

use iced::widget::text_editor::{Action, Content};
use torro_core::i18n::{tr, trf};
use torro_ipc::Request;
use torro_ipc::assistant::{ConversationSummary, Entry, EntryContent, LETTER_AVATAR};
use torro_ipc::bots::{
    Bot, BotSummary, MAX_DESCRIPTION_CHARS, MAX_INSTRUCTIONS_CHARS, MAX_NAME_CHARS, MAX_PERSONALITY_CHARS,
    MAX_SKILL_CHARS, MAX_SKILL_NAME_CHARS, MAX_SKILLS, Skill,
};

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
    /// `bots.save_bot` runs: "Save" takes no second click.
    pub saving: bool,
    /// The editor made a new bot: its chat opens once it is saved.
    pub new: bool,
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
}
