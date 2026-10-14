//! Bots on the wire (task B.1, `docs/assistent.md` "Bots (Stufe 2)"): what the `bots.*` requests and
//! answers carry – a bot whole ([`Bot`]), in a list ([`BotSummary`]), its skills ([`Skill`]) – and
//! the limits every `bots.*` request keeps ([`limit_refusal`]).
//!
//! A bot is an assistant of its own, cut to one field ("Travel planner"): a profile like the
//! assistant's (name, description, picture, personality), instructions and skills as text, the
//! engine and model it answers with, and a client of its own of the kind [`crate::api::BOT_CLIENT`].
//! The logic is the core's (`crates/torroworkplace/src/bots`): it keeps every bot in
//! `<config>/assistant/bots/<id>/` and runs its answers as conversations of the assistant marked
//! with the bot ([`crate::assistant::ConversationSummary::bot`]).
//!
//! Rights in short: a new bot's client holds nothing ([`crate::api::Rights::none`]); only a person
//! grants it more (`clients.update_client`, app key only). A bot's client may use nothing of the
//! areas `bots` and `assistant` ([`crate::api::loops_back`]): no chains, no loop. The area `bots` is
//! off in every template, like `assistant` ([`crate::api::Area::in_templates`]).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::assistant::{AVATARS, MAX_ID_BYTES, MAX_TEXT_CHARS};
use crate::{Request, Response};

/// At most this many characters in a bot's name: one line.
pub const MAX_NAME_CHARS: usize = 40;

/// At most this many characters in a bot's short description: one line.
pub const MAX_DESCRIPTION_CHARS: usize = 200;

/// At most this many characters in a bot's personality ([`Bot::personality`]).
pub const MAX_PERSONALITY_CHARS: usize = 4_000;

/// At most this many characters in a bot's instructions ([`Bot::instructions`]).
pub const MAX_INSTRUCTIONS_CHARS: usize = 8_000;

/// At most this many characters in the text of one skill ([`Skill::text`]).
pub const MAX_SKILL_CHARS: usize = 8_000;

/// At most this many characters in the name of one skill ([`Skill::name`]): one line.
pub const MAX_SKILL_NAME_CHARS: usize = 40;

/// At most this many skills per bot.
pub const MAX_SKILLS: usize = 8;

/// At most this many bots; `bots.save_bot` for a new one beyond it is `error`.
pub const MAX_BOTS: usize = 50;

/// One bot whole (`bots.get_bot`, the answer of `bots.save_bot`). Everything in it but `id`,
/// `client` and `engine` is the person's text: data, never instructions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Bot {
    /// 8 hex digits, made by the core: what `bots.get_bot`, `bots.ask_bot` and
    /// `assistant.send_message`'s `bot` take.
    pub id: String,
    /// What the bot is called ("Travel planner"): one line, at most [`MAX_NAME_CHARS`] characters.
    pub name: String,
    /// What it is for, one line of at most [`MAX_DESCRIPTION_CHARS`] characters; may be empty.
    pub description: String,
    /// Its picture: one of [`AVATARS`] (`letter` for the first letter of its name). Clients must
    /// show a picture they do not know as the letter.
    pub avatar: String,
    /// The engine it answers with when it is asked (`bots.ask_bot`): an id from
    /// `assistant.list_engines` (`ai:ollama@localhost:11434`, `claude-code`).
    pub engine: String,
    /// The model of that engine; `None` lets the engine pick one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// How it speaks and behaves, at most [`MAX_PERSONALITY_CHARS`] characters; empty for none. It
    /// comes first in the bot's prompt and shapes its tone only.
    pub personality: String,
    /// What it does and how, at most [`MAX_INSTRUCTIONS_CHARS`] characters; empty for none. It comes
    /// after the fixed rules in the bot's prompt, which win where they disagree.
    pub instructions: String,
    /// What it knows, as text, at most [`MAX_SKILLS`]; after the instructions in its prompt.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<Skill>,
    /// The id of the bot's own client (`clients.list_clients`): its rights are that client's, set by
    /// a person with `clients.update_client`.
    pub client: String,
}

/// One bot without its long texts (`bots.list_bots`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct BotSummary {
    /// As [`Bot::id`].
    pub id: String,
    pub name: String,
    pub description: String,
    pub avatar: String,
    pub engine: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// As [`Bot::client`].
    pub client: String,
}

impl Bot {
    /// The bot as `bots.list_bots` lists it.
    pub fn summary(&self) -> BotSummary {
        BotSummary {
            id: self.id.clone(),
            name: self.name.clone(),
            description: self.description.clone(),
            avatar: self.avatar.clone(),
            engine: self.engine.clone(),
            model: self.model.clone(),
            client: self.client.clone(),
        }
    }
}

/// One skill of a bot: knowledge it reads in every answer, as text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Skill {
    /// What the skill is called ("Packing lists"): one line, at most [`MAX_SKILL_NAME_CHARS`]
    /// characters.
    pub name: String,
    /// The skill itself (Markdown or plain text), at most [`MAX_SKILL_CHARS`] characters.
    pub text: String,
}

/// The `error` answer for a `bots.*` request that breaks a limit, before anything is looked at:
/// an id over [`MAX_ID_BYTES`], for `bots.save_bot` a name that is blank, more than one line or
/// over [`MAX_NAME_CHARS`], a description of more than one line or over
/// [`MAX_DESCRIPTION_CHARS`], a picture not among [`AVATARS`], a blank engine, a personality, the
/// instructions or a skill beyond their lengths, more than [`MAX_SKILLS`] skills or a skill whose
/// name is blank or longer than one line of [`MAX_SKILL_NAME_CHARS`]; for `bots.ask_bot` a
/// message that is blank or over [`MAX_TEXT_CHARS`]. Lengths count characters after trimming.
/// `None` for every request within them and for every other request. The core calls it before
/// rights, approvals and handlers, so nobody is asked to approve what would be refused anyway.
/// `bots.save_bot` beyond [`MAX_BOTS`] bots is the core's to refuse: only it knows how many there
/// are.
pub fn limit_refusal(request: &Request) -> Option<Response> {
    let refuse = |message: String| Some(Response::Error { code: "error".into(), message });
    if bot_ids(request).into_iter().any(|id| id.len() > MAX_ID_BYTES) {
        return refuse(format!("an id may hold at most {MAX_ID_BYTES} bytes"));
    }
    match request {
        Request::SaveBot { name, description, avatar, engine, personality, instructions, skills, .. } => {
            let draft =
                Draft { name, description, avatar: avatar.as_deref(), engine, personality, instructions, skills };
            draft.refusal().and_then(refuse)
        }
        Request::AskBot { text, .. } if text.trim().is_empty() => refuse("the message is empty".into()),
        Request::AskBot { text, .. } if text.chars().count() > MAX_TEXT_CHARS => {
            refuse(format!("a message may hold at most {MAX_TEXT_CHARS} characters"))
        }
        _ => None,
    }
}

/// The fields of a `bots.save_bot`, for [`limit_refusal`].
struct Draft<'a> {
    name: &'a str,
    description: &'a str,
    avatar: Option<&'a str>,
    engine: &'a str,
    personality: &'a str,
    instructions: &'a str,
    skills: &'a [Skill],
}

impl Draft<'_> {
    /// Why the bot is refused, for people; `None` when it keeps every limit.
    fn refusal(&self) -> Option<String> {
        let chars = |text: &str| text.trim().chars().count();
        let one_line = |text: &str| text.trim().lines().count() <= 1;
        if self.name.trim().is_empty() {
            return Some("the name is empty".into());
        }
        if !one_line(self.name) || chars(self.name) > MAX_NAME_CHARS {
            return Some(format!("a name is one line of at most {MAX_NAME_CHARS} characters"));
        }
        if !one_line(self.description) || chars(self.description) > MAX_DESCRIPTION_CHARS {
            return Some(format!("a description is one line of at most {MAX_DESCRIPTION_CHARS} characters"));
        }
        if let Some(avatar) = self.avatar
            && !AVATARS.contains(&avatar)
        {
            return Some(format!("no avatar {avatar:?}; one of: {}", AVATARS.join(", ")));
        }
        if self.engine.trim().is_empty() {
            return Some("a bot needs an engine (an id from assistant.list_engines)".into());
        }
        if chars(self.personality) > MAX_PERSONALITY_CHARS {
            return Some(format!("a personality may hold at most {MAX_PERSONALITY_CHARS} characters"));
        }
        if chars(self.instructions) > MAX_INSTRUCTIONS_CHARS {
            return Some(format!("the instructions may hold at most {MAX_INSTRUCTIONS_CHARS} characters"));
        }
        if self.skills.len() > MAX_SKILLS {
            return Some(format!("a bot has at most {MAX_SKILLS} skills"));
        }
        for skill in self.skills {
            if skill.name.trim().is_empty() || !one_line(&skill.name) || chars(&skill.name) > MAX_SKILL_NAME_CHARS {
                return Some(format!("a skill's name is one line of 1 to {MAX_SKILL_NAME_CHARS} characters"));
            }
            if chars(&skill.text) > MAX_SKILL_CHARS {
                return Some(format!("a skill may hold at most {MAX_SKILL_CHARS} characters"));
            }
        }
        None
    }
}

impl Request {
    /// The bot a request names, for the core's checks and its audit log: `bots.get_bot`,
    /// `bots.delete_bot` and a `bots.save_bot` that changes one by their `id`, `bots.ask_bot` and
    /// `assistant.send_message` and `assistant.confirm_bot` by their `bot`. `None` for every other request and for a
    /// `bots.save_bot` that makes a new one.
    pub fn bot(&self) -> Option<&str> {
        match self {
            Request::GetBot { id } | Request::DeleteBot { id } | Request::SaveBot { id: Some(id), .. } => Some(id),
            Request::AskBot { bot, .. } | Request::ConfirmBot { bot, .. } => Some(bot),
            Request::SendMessage { bot, .. } => bot.as_deref(),
            _ => None,
        }
    }
}

/// Every id a `bots.*` request carries, for [`limit_refusal`].
fn bot_ids(request: &Request) -> Vec<&str> {
    let mut ids: Vec<&str> = match request {
        Request::GetBot { .. } | Request::DeleteBot { .. } | Request::SaveBot { .. } | Request::AskBot { .. } => {
            request.bot().into_iter().collect()
        }
        _ => Vec::new(),
    };
    match request {
        Request::SaveBot { engine, model, .. } => {
            ids.push(engine);
            ids.extend(model.as_deref());
        }
        Request::AskBot { conversation, .. } => ids.extend(conversation.as_deref()),
        _ => {}
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(response: Option<Response>) -> Option<String> {
        response.map(|response| match response {
            Response::Error { code, .. } => code,
            other => panic!("expected an error, got {other:?}"),
        })
    }

    /// A `bots.save_bot` of a new bot with `name`, `skills` and the rest within the limits.
    fn save(name: &str, skills: Vec<Skill>) -> Request {
        Request::SaveBot {
            id: None,
            name: name.into(),
            description: "Plans trips.".into(),
            avatar: None,
            engine: "ai:ollama@localhost:11434".into(),
            model: None,
            personality: String::new(),
            instructions: "Suggest trains before flights.".into(),
            skills,
        }
    }

    fn skill(name: &str, text: &str) -> Skill {
        Skill { name: name.into(), text: text.into() }
    }

    #[test]
    fn a_bot_keeps_its_limits() {
        assert_eq!(code(limit_refusal(&save("Travel planner", vec![skill("Packing", "Pack light.")]))), None);
        assert_eq!(code(limit_refusal(&save(" ", Vec::new()))), Some("error".into()), "blank name");
        assert_eq!(code(limit_refusal(&save("Travel\nplanner", Vec::new()))), Some("error".into()), "one line");
        assert_eq!(code(limit_refusal(&save(&"a".repeat(MAX_NAME_CHARS), Vec::new()))), None);
        assert_eq!(code(limit_refusal(&save(&"a".repeat(MAX_NAME_CHARS + 1), Vec::new()))), Some("error".into()));

        let with = |change: &dyn Fn(&mut Request)| {
            let mut request = save("Travel planner", Vec::new());
            change(&mut request);
            code(limit_refusal(&request))
        };
        let refused = Some("error".to_string());
        assert_eq!(
            with(&|r| if let Request::SaveBot { description, .. } = r {
                *description = "a".repeat(MAX_DESCRIPTION_CHARS + 1)
            }),
            refused
        );
        assert_eq!(
            with(&|r| if let Request::SaveBot { description, .. } = r {
                *description = "one\ntwo".into()
            }),
            refused
        );
        assert_eq!(
            with(&|r| if let Request::SaveBot { avatar, .. } = r {
                *avatar = Some("robot".into())
            }),
            refused
        );
        for known in AVATARS {
            assert_eq!(
                with(&|r| if let Request::SaveBot { avatar, .. } = r {
                    *avatar = Some(known.to_string())
                }),
                None
            );
        }
        assert_eq!(
            with(&|r| if let Request::SaveBot { engine, .. } = r {
                *engine = " ".into()
            }),
            refused
        );
        let personality = |n: usize| {
            with(&move |r| {
                if let Request::SaveBot { personality, .. } = r {
                    *personality = "a".repeat(n)
                }
            })
        };
        assert_eq!(personality(MAX_PERSONALITY_CHARS), None);
        assert_eq!(personality(MAX_PERSONALITY_CHARS + 1), refused);
        let instructions = |n: usize| {
            with(&move |r| {
                if let Request::SaveBot { instructions, .. } = r {
                    *instructions = "a".repeat(n)
                }
            })
        };
        assert_eq!(instructions(MAX_INSTRUCTIONS_CHARS), None);
        assert_eq!(instructions(MAX_INSTRUCTIONS_CHARS + 1), refused);

        let many = (0..=MAX_SKILLS).map(|n| skill(&format!("Skill {n}"), "x")).collect::<Vec<_>>();
        assert_eq!(code(limit_refusal(&save("Bot", many[..MAX_SKILLS].to_vec()))), None);
        assert_eq!(code(limit_refusal(&save("Bot", many))), refused, "too many skills");
        assert_eq!(code(limit_refusal(&save("Bot", vec![skill(" ", "x")]))), refused, "a skill needs a name");
        assert_eq!(code(limit_refusal(&save("Bot", vec![skill("a\nb", "x")]))), refused);
        let long = "a".repeat(MAX_SKILL_CHARS + 1);
        assert_eq!(code(limit_refusal(&save("Bot", vec![skill("Long", &long)]))), refused);
        let fits = "a".repeat(MAX_SKILL_CHARS);
        assert_eq!(code(limit_refusal(&save("Bot", vec![skill("Long", &fits)]))), None);
    }

    #[test]
    fn asking_a_bot_and_ids_keep_their_limits() {
        let ask = |text: &str, conversation: Option<String>| Request::AskBot {
            bot: "1a2b3c4d".into(),
            text: text.into(),
            conversation,
        };
        assert_eq!(code(limit_refusal(&ask("Plan my trip", None))), None);
        assert_eq!(code(limit_refusal(&ask(" \n", None))), Some("error".into()));
        assert_eq!(code(limit_refusal(&ask(&"a".repeat(MAX_TEXT_CHARS + 1), None))), Some("error".into()));
        let long = "a".repeat(MAX_ID_BYTES + 1);
        assert_eq!(code(limit_refusal(&ask("Hi", Some(long.clone())))), Some("error".into()));
        assert_eq!(code(limit_refusal(&Request::GetBot { id: long.clone() })), Some("error".into()));
        assert_eq!(code(limit_refusal(&Request::DeleteBot { id: long })), Some("error".into()));
        assert_eq!(code(limit_refusal(&Request::ListBots)), None);
        assert_eq!(code(limit_refusal(&Request::Ping)), None, "other requests keep no limit here");
    }

    #[test]
    fn requests_name_their_bot() {
        assert_eq!(Request::GetBot { id: "b1".into() }.bot(), Some("b1"));
        assert_eq!(Request::DeleteBot { id: "b1".into() }.bot(), Some("b1"));
        assert_eq!(Request::AskBot { bot: "b1".into(), text: "Hi".into(), conversation: None }.bot(), Some("b1"));
        assert_eq!(save("New", Vec::new()).bot(), None, "a new bot has no id yet");
        assert_eq!(Request::ListBots.bot(), None);
    }

    /// The wire format apps and agents rely on: skills left out when there are none, a model only
    /// when one is set.
    #[test]
    fn bots_read_as_documented() {
        let bot: Bot = serde_json::from_value(serde_json::json!({
            "id": "1a2b3c4d", "name": "Travel planner", "description": "", "avatar": "letter",
            "engine": "test", "personality": "", "instructions": "", "client": "9f8e7d6c"
        }))
        .expect("a bot");
        assert!(bot.skills.is_empty() && bot.model.is_none());
        let json = serde_json::to_value(&bot).expect("serializes");
        assert!(json.get("skills").is_none() && json.get("model").is_none(), "{json}");
        assert_eq!(bot.summary().client, "9f8e7d6c");
    }
}
