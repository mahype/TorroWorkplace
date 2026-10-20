//! The assistant asks a bot (task B.2, `docs/assistent.md` "Bots"): only when the person asked for
//! it, and the bot then works with what both may.
//!
//! - **Only on the person's word** ([`Assistant::delegate`]): the last message in the assistant's
//!   conversation is the person's own (sent with the app key, not by an agent with access to the
//!   assistant) and names the bot ([`crate::bots::names_bot`]), or the person confirmed the
//!   assistant's proposal there (`assistant.confirm_bot`, app key only, [`Assistant::confirm_bot`];
//!   it counts for one ask and lives in memory only). Otherwise `forbidden`, with a word for the
//!   model to propose the bot by name instead.
//! - **What both may**: the bot runs as [`Actor::Delegated`]: its client with the rights it and the
//!   assistant's client have in common, without the areas switched off in the assistant's
//!   conversation (and those an agent's key was lent without). The core computes them anew for every
//!   tool call and keeps the narrowing in the approvals the bot asks for (`crate::approvals::Origin`).
//! - **Where**: in the bot's own conversation, one per assistant conversation and bot (kept in
//!   memory; after a restart a new one starts). The assistant's conversation gets one tool line
//!   `bots.ask_bot` with "Asked {bot} · {n} steps" and the bot's conversation as `conversation` in
//!   its input ([`with_conversation`]), so a window can open it; the model reads the bot's answer
//!   ([`Asked::for_model`]).
//! - **Data rule**: a conversation whose model read mail asks no bot whose model runs elsewhere (the
//!   message could carry the mail; the model the bot really answers with, its "Automatic" resolved
//!   as for any answer, task S.23, and a model turned off replaced by one of the same account that is
//!   on, task S.24, both by [`super::Host::model`]); when the bot read mail, its answer is withheld from a model
//!   elsewhere; when it read data from outside the suite, its answer is marked so, and the
//!   assistant's conversation counts as having read it (notes then wait for a person, task C.11).
//! - **Stopping** the assistant's answer stops the bots it asked (`assistant.cancel`).
//! - **No chains**: a bot's client never sends a `bots` or `assistant` command
//!   (`torro_ipc::api::loops_back`), and a bot's conversation never asks one.

use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use serde_json::{Value, json};
use torro_core::i18n::trf;
use torro_ipc::api::Area;
use torro_ipc::assistant::{EngineKind, Entry, EntryContent};
use torro_ipc::bots::Bot;
use torro_ipc::{Request, Response};

use super::data_rule::{self, DataRule, Place, Reader, WITHHELD};
use super::model::{Actor, Delegation};
use super::{Assistant, Message, bot_refusal, failure, tools, unknown};
use crate::account::ai::AiAccounts;

/// How often [`Assistant::delegate`] looks whether the bot's run is done.
const WAIT_STEP: Duration = Duration::from_millis(50);

impl Assistant {
    /// `assistant.confirm_bot` (app key only): the person's yes to the assistant's proposal to ask
    /// `bot` in its conversation `conversation`. Allows one ask ([`Assistant::delegate`]); kept in
    /// memory only, so a restart forgets it. `not-found` for an unknown bot or conversation, `error`
    /// for a conversation with a bot.
    pub async fn confirm_bot(&self, conversation: &str, bot: &str) -> Response {
        if self.bots.get(bot).await.is_none() {
            return crate::bots::no_bot(bot);
        }
        let mut state = self.state().await;
        let Some(held) = state.conversations.get(conversation) else { return unknown(conversation) };
        if let Some(refused) = bot_refusal(held, None) {
            return refused;
        }
        state.confirmed.entry(conversation.to_string()).or_default().insert(bot.to_string());
        Response::Ok
    }

    /// Whether the assistant may ask `bot` in its conversation `main` now
    /// ([`crate::bots::assistant_may_ask`]): the last message there is the person's own
    /// ([`super::State::said`]) and names it, or the person confirmed it – a confirmation is used up
    /// here. `Err` with the answer: `not-found` for no such
    /// conversation, `forbidden` for one with a bot (no chains) or when the person did not ask.
    async fn may_ask(&self, main: &str, bot: &Bot) -> Result<(), Response> {
        let mut state = self.state().await;
        let Some(held) = state.conversations.get(main) else { return Err(unknown(main)) };
        if held.bot.is_some() {
            return Err(forbidden("a bot cannot ask another bot"));
        }
        // The last message counts only when the person wrote it (the app key), not an agent with
        // access to the assistant.
        let last = held.entries.iter().rev().find_map(|entry| match &entry.content {
            EntryContent::Message { text, .. } => Some((entry.seq, text.clone())),
            _ => None,
        });
        let said = last.filter(|(seq, _)| state.said.get(main) == Some(seq)).map(|(_, text)| text);
        if crate::bots::assistant_may_ask(said.as_deref(), &bot.name, false) {
            return Ok(());
        }
        let confirmed = state.confirmed.get_mut(main).is_some_and(|bots| bots.remove(&bot.id));
        match crate::bots::assistant_may_ask(None, &bot.name, confirmed) {
            true => Ok(()),
            false => Err(not_asked(&bot.name)),
        }
    }

    /// `bots.ask_bot` of the assistant's own client while it answers in its conversation (task
    /// B.2, [`Asking`]), with the areas switched off there (and for an agent's lent key what it was
    /// lent without) taken out. Checks that the person asked for the bot ([`Assistant::may_ask`]),
    /// runs it as [`Actor::Delegated`] in its conversation for the assistant's (new on the first
    /// ask) and waits until the run is done. A conversation that read mail asks no bot whose model
    /// runs elsewhere (`forbidden`, the data rule). Answers [`Response::Conversation`]
    /// with the bot's conversation and the entries of this run, or the error answer: `forbidden`
    /// when the person did not ask, `not-found` for an unknown bot, `error` for a `conversation`
    /// other than the one kept for `main`, and what [`Assistant::refusal`] refuses (`busy`, an engine
    /// that does not answer). The core calls it after it checked the assistant's client.
    pub async fn delegate(self: &Arc<Self>, ai: &AiAccounts, from: &Asking<'_>, request: Request) -> Response {
        let Asking { conversation: main, assistant, without, read_mail, mode } = *from;
        if let Some(refused) = torro_ipc::bots::limit_refusal(&request) {
            return refused;
        }
        let Request::AskBot { bot, text, conversation } = request else {
            return failure("only bots.ask_bot asks a bot");
        };
        let Some(bot) = self.bots.get(&bot).await else { return crate::bots::no_bot(&bot) };
        let kept = self.kept_for(main, &bot.id).await;
        if conversation.as_ref().is_some_and(|given| Some(given) != kept.as_ref()) {
            return failure("leave out `conversation`: the core keeps your conversation with this bot");
        }
        let delegation = Delegation {
            bot: bot.id.clone(),
            client: bot.client.clone(),
            assistant: assistant.to_string(),
            without: without.to_vec(),
            mode,
        };
        let asking = Request::AskBot { bot: bot.id.clone(), text: text.clone(), conversation: kept.clone() };
        if let Some(refused) = self.refusal(ai, &asking).await {
            return refused;
        }
        let kind = match self.engine(ai, &bot.engine, &Actor::Delegated(delegation.clone())).await {
            Ok(kind) => kind,
            Err(refused) => return refused,
        };
        // The data rule: what the assistant's model read may be in the message, and what the rule
        // keeps home stays on the person's computers (`read_mail` is what it keeps, task C.17).
        let mut model = bot.model.clone();
        if read_mail {
            let (place, checked) = self.place_of(kind, &bot.engine, bot.model.as_deref()).await;
            if place == Place::Remote {
                return forbidden(
                    "this conversation read what the person's data rule keeps from models outside this computer \
                     (mail unless the person let it out, or messages in the inbox), and the bot answers with such a \
                     model; do not ask it now, tell the person instead",
                );
            }
            // The model checked answers, not one "Automatic" might pick anew (task S.23).
            model = checked.or(model);
        }
        // Last, as it uses up a confirmation: nothing else refuses the ask after this.
        if let Err(refused) = self.may_ask(main, &bot).await {
            return refused;
        }
        let message = Message {
            conversation: kept,
            engine: bot.engine.clone(),
            model,
            effort: None,
            text,
            without: without.to_vec(),
            // The bot's client is narrowed through the delegation already (its mode included).
            mode: torro_ipc::assistant::Mode::Default,
            bot: Some(bot.clone()),
            on_behalf: Some(delegation),
            person: false,
        };
        let started = self.send(message, kind, Utc::now()).await;
        let Response::Conversation { conversation: summary, entries } = &started else { return started };
        let (id, before) = (summary.id.clone(), entries.first().map_or(summary.last, |entry| entry.seq - 1));
        self.state().await.asked.insert((main.to_string(), bot.id), id.clone());
        while self.state().await.running.contains_key(&id) {
            tokio::time::sleep(WAIT_STEP).await;
        }
        self.get(&id, Some(before)).await
    }

    /// Where the model `model` of `engine` (driven as `kind`) runs, for the data rule: the test
    /// engine here, an agent elsewhere, an AI account where the core says for that model (task
    /// S.23; for `None` the model "Automatic" picks) – elsewhere when it cannot tell. With the
    /// model of the AI account that was judged, so the bot answers with that one.
    async fn place_of(&self, kind: EngineKind, engine: &str, model: Option<&str>) -> (Place, Option<String>) {
        match kind {
            EngineKind::Test => (Place::Local, None),
            EngineKind::Agent => (Place::Remote, None),
            EngineKind::Ai => match self.host() {
                Some(host) => host
                    .model(engine, model)
                    .await
                    .map_or((Place::Remote, None), |access| (access.place, Some(access.model))),
                None => (Place::Remote, None),
            },
        }
    }

    /// The bot's conversation kept for the assistant's conversation `main` and `bot`, while it is
    /// still there.
    async fn kept_for(&self, main: &str, bot: &str) -> Option<String> {
        let state = self.state().await;
        let id = state.asked.get(&(main.to_string(), bot.to_string()))?;
        state.conversations.contains_key(id).then(|| id.clone())
    }
}

/// Where the assistant asks a bot from (task B.2, [`Assistant::delegate`]).
#[derive(Debug, Clone, Copy)]
pub struct Asking<'a> {
    /// The assistant's conversation, where the person must have asked for the bot.
    pub conversation: &'a str,
    /// The assistant's client id: its rights bound the bot's run.
    pub assistant: &'a str,
    /// The areas switched off there (and for an agent's lent key those it was lent without): the
    /// bot runs without them.
    pub without: &'a [Area],
    /// The assistant's model read mail in this conversation: a bot whose model runs elsewhere is
    /// not asked (the data rule).
    pub read_mail: bool,
    /// The permission mode of the assistant's answer (issue #132): the bot runs in it as well.
    pub mode: torro_ipc::assistant::Mode,
}

/// What a bot's run for the assistant brought (task B.2), from the entries of the run.
#[derive(Debug, Clone, PartialEq)]
pub struct Asked {
    /// The bot's conversation.
    pub conversation: String,
    /// How many tools the bot ran.
    pub steps: usize,
    /// The bot's answers, joined; `None` without one.
    pub answer: Option<String>,
    /// The code of the error the run ended with (also `stopped`); `None` when it answered.
    pub error: Option<String>,
    /// Its message for people.
    message: Option<String>,
    /// A tool of the run read mail ([`data_rule::reads_mail`]): the answer may retell it.
    pub read_mail: bool,
    /// A tool of the run read the inbox or asked a bot: the answer may retell entries of Nextcloud
    /// or Talk, which stay home even with mail let out (task C.17, [`Asked::kept`]).
    pub read_messages: bool,
    /// A tool of the run may have read data from outside the suite ([`tools::may_have_read_untrusted`]).
    pub untrusted: bool,
}

impl Asked {
    /// The run in `response`, the answer of [`Assistant::delegate`]; `None` for an error answer.
    pub fn of(response: &Response) -> Option<Asked> {
        let Response::Conversation { conversation, entries } = response else { return None };
        Some(Asked::from_entries(&conversation.id, entries))
    }

    /// The run of the bot's conversation `conversation` from its `entries`.
    fn from_entries(conversation: &str, entries: &[Entry]) -> Asked {
        let mut asked = Asked {
            conversation: conversation.to_string(),
            steps: 0,
            answer: None,
            error: None,
            message: None,
            read_mail: false,
            read_messages: false,
            untrusted: false,
        };
        let mut answers = Vec::new();
        for entry in entries {
            match &entry.content {
                EntryContent::Tool { name, error, .. } => {
                    asked.steps += 1;
                    // What failed read nothing.
                    if error.is_none() {
                        asked.read_mail |= data_rule::reads_mail(name);
                        asked.read_messages |= data_rule::kept_from(name, DataRule { mail_to_cloud: true });
                        asked.untrusted |= tools::may_have_read_untrusted(name);
                    }
                }
                EntryContent::Answer { text, .. } => answers.push(text.clone()),
                EntryContent::Error { code, message } => {
                    asked.error = Some(code.clone());
                    asked.message = Some(message.clone());
                }
                EntryContent::Stopped => asked.error = Some("stopped".into()),
                EntryContent::Message { .. } | EntryContent::Approval { .. } | EntryContent::Card { .. } => {}
            }
        }
        asked.answer = (!answers.is_empty()).then(|| answers.join("\n\n"));
        asked
    }

    /// The tool line's words for people: "Asked Travel planner · 3 steps".
    pub fn summary(&self, bot: &str) -> String {
        match self.steps {
            1 => trf("Asked {bot} · 1 step", &[("bot", &bot)]),
            steps => trf("Asked {bot} · {steps} steps", &[("bot", &bot), ("steps", &steps)]),
        }
    }

    /// Whether the run read what `rule` keeps from models elsewhere: mail unless it is let out, the
    /// inbox and other bots always.
    pub fn kept(&self, rule: DataRule) -> bool {
        self.read_messages || (self.read_mail && !rule.mail_to_cloud)
    }

    /// What the assistant's model read as `reader` reads: the bot's conversation, its steps and its
    /// answer (or why it ended), shortened. A model elsewhere reads [`WITHHELD`] for an answer after
    /// what the rule keeps home ([`Asked::kept`]); an answer after data from outside the suite is
    /// marked so.
    pub fn for_model(&self, reader: impl Into<Reader>) -> String {
        let reader = reader.into();
        let mut value = json!({"result": "bot-answer", "conversation": self.conversation, "steps": self.steps});
        let withheld = reader.place == Place::Remote && self.kept(reader.rule);
        if withheld {
            value["answer"] = json!(WITHHELD);
        } else if let Some(answer) = &self.answer {
            value["answer"] = json!(answer);
        }
        if let Some(code) = &self.error {
            value["error"] = json!({"code": code, "message": self.message.clone().unwrap_or_default()});
        }
        if self.untrusted && !withheld {
            value = json!({"untrusted": true, "note": tools::UNTRUSTED_NOTE, "data": value});
        }
        tools::shorten(value).to_string()
    }
}

/// The tool line's `input` with the bot's conversation as `conversation`, so the line refers to it:
/// that is what the request ran with. A model's input that is no object becomes `{"input": …}`.
pub fn with_conversation(input: Value, conversation: &str) -> Value {
    let mut object = match input {
        Value::Object(object) => object,
        other => serde_json::Map::from_iter([("input".to_string(), other)]),
    };
    object.insert("conversation".into(), json!(conversation));
    Value::Object(object)
}

/// The `forbidden` answer for a bot the person did not ask for, with what the model should do.
/// For the model, so English.
fn not_asked(name: &str) -> Response {
    forbidden(&format!(
        "the person has not asked for the bot {name:?} in this conversation. Do not ask it; propose it by name \
         instead (\"Shall I ask {name}?\"). It can be asked once the person names it in their message or confirms \
         your proposal."
    ))
}

/// The `forbidden` answer with `message`.
fn forbidden(message: &str) -> Response {
    Response::Error { code: "forbidden".into(), message: message.into() }
}

#[cfg(test)]
mod tests;
