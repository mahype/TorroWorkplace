//! The bots of the core (task B.1, `docs/assistent.md` "Bots (Stufe 2)"): assistants of their own,
//! cut to one field ("Travel planner"), kept in `<config>/assistant/bots/<id>/` ([`store`]) and
//! answered with the assistant's machinery.
//!
//! - **Storage** ([`Bots`]): one folder per bot (0700, files 0600); at most
//!   [`torro_ipc::bots::MAX_BOTS`]. Read from disk on every use: there are few, and a person may
//!   edit the text files by hand.
//! - **A client of its own** ([`torro_ipc::api::BOT_CLIENT`]): the first `bots.save_bot` makes it
//!   with [`torro_ipc::api::Rights::none`]; only a person grants it more (`clients.update_client`,
//!   app key only). A rename of the bot renames its client, so approvals name it as the person
//!   knows it; a client the person removed is made anew, again without rights, on the next save.
//!   `bots.delete_bot` removes the client first (its key stops at once), then the folder.
//! - **Chats** are the assistant's (`crate::assistant`): a conversation started with a bot keeps
//!   its id, runs with the bot's client, its prompt and its engine; `bots.ask_bot` and
//!   `assistant.send_message` with `bot` both go there. Conversations of a deleted bot stay and
//!   are marked so.
//! - **Rights** are the core's (`crate::core::Core::handle_client`): a bot's client never sends a
//!   `bots` or `assistant` command (`torro_ipc::api::loops_back`); the assistant's own client lists
//!   bots, proposes a new one (always with a person's approval) and asks one only when
//!   [`assistant_may_ask`] says so (task B.2): then the bot runs with what both clients may.
//!
//! What goes to a client never names a path; the core's log does.

mod store;

use std::io;
use std::path::PathBuf;

use tokio::sync::Mutex;
use torro_ipc::api::{BOT_CLIENT, Rights};
use torro_ipc::assistant::LETTER_AVATAR;
use torro_ipc::bots::{
    Bot, MAX_BOTS, MAX_DESCRIPTION_CHARS, MAX_INSTRUCTIONS_CHARS, MAX_NAME_CHARS, MAX_PERSONALITY_CHARS,
    MAX_SKILL_CHARS, MAX_SKILL_NAME_CHARS, Skill,
};
use torro_ipc::{Request, Response};

use crate::assistant::Assistant;
use crate::assistant::profile::{clean_line, clean_text};
use crate::clients::{Clients, ClientsError};

/// The folder of the bots in the assistant's config folder: `<config>/assistant/bots`.
pub fn folder(assistant_config: &std::path::Path) -> PathBuf {
    assistant_config.join("bots")
}

/// Whether the assistant may ask the bot named `name` now (task B.2): only when the person's last
/// message in the assistant's conversation (`last_message`; `None` when it has none) names the bot
/// ([`names_bot`]), or the person confirmed the assistant's proposal of this bot in that
/// conversation (`confirmed`, from `assistant.confirm_bot`, app key only, counted once by the
/// caller). Otherwise the assistant must propose the bot by name instead.
///
/// Only the assistant calls it ([`crate::assistant::Assistant::delegate`]), for a `bots.ask_bot` of
/// its own client that the core admitted with the conversation it answers in. Without that
/// conversation (the assistant's key over the API) the core refuses the ask anyway.
pub fn assistant_may_ask(last_message: Option<&str>, name: &str, confirmed: bool) -> bool {
    confirmed || last_message.is_some_and(|text| names_bot(text, name))
}

/// The articles a bot's name may start with or be named with ("the travel planner", "den
/// Reiseplaner"): left out of the name before it is looked for.
const ARTICLES: &[&str] =
    &["the", "a", "an", "der", "die", "das", "den", "dem", "des", "ein", "eine", "einen", "einem", "einer", "eines"];

/// The endings a word of the name may have in the text ("des Reiseplaners", "the travel planners",
/// "dem Reiseplaner"): German and English inflection, kept simple.
const ENDINGS: &[&str] = &["", "s", "n", "e", "en", "es", "er", "ns"];

/// Whether `text` names the bot `name`: its words (without a leading article) follow each other in
/// `text`, ignoring case and punctuation, each as it is or with a simple ending ([`ENDINGS`]). A
/// name of nothing but an article or punctuation is never named.
pub fn names_bot(text: &str, name: &str) -> bool {
    let mut wanted = words(name);
    if wanted.len() > 1 && ARTICLES.contains(&wanted[0].as_str()) {
        wanted.remove(0);
    }
    if wanted.is_empty() || (wanted.len() == 1 && ARTICLES.contains(&wanted[0].as_str())) {
        return false;
    }
    let said = words(text);
    said.windows(wanted.len()).any(|window| {
        window
            .iter()
            .zip(&wanted)
            .all(|(word, part)| word.strip_prefix(part.as_str()).is_some_and(|ending| ENDINGS.contains(&ending)))
    })
}

/// The words of `text` in lower case: runs of letters and digits.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric()).filter(|word| !word.is_empty()).map(str::to_lowercase).collect()
}

/// The bots, on disk.
pub struct Bots {
    /// `<config>/assistant/bots`; `None` without a home directory and in tests that pass none:
    /// then there are no bots and saving one is `error`.
    dir: Option<PathBuf>,
    /// Held while a bot is saved or deleted, so two changes never lose one another and the number
    /// of bots stays within [`MAX_BOTS`].
    changing: Mutex<()>,
}

/// What `bots.save_bot` asks for, past its limits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Draft {
    pub id: Option<String>,
    pub name: String,
    pub description: String,
    pub avatar: Option<String>,
    pub engine: String,
    pub model: Option<String>,
    pub personality: String,
    pub instructions: String,
    pub skills: Vec<Skill>,
}

impl Draft {
    /// The draft of a `bots.save_bot`; `None` for every other request.
    pub fn of(request: Request) -> Option<Draft> {
        let Request::SaveBot { id, name, description, avatar, engine, model, personality, instructions, skills } =
            request
        else {
            return None;
        };
        Some(Draft { id, name, description, avatar, engine, model, personality, instructions, skills })
    }

    /// The bot it makes, cleaned as it is kept: names and the description on one line, the texts
    /// without control characters, each within its limit; a blank model is none, a missing picture
    /// the letter. `id` and `client` as given.
    fn into_bot(self, id: String, client: String) -> Bot {
        Bot {
            id,
            name: clean_line(&self.name, MAX_NAME_CHARS),
            description: clean_line(&self.description, MAX_DESCRIPTION_CHARS),
            avatar: self.avatar.unwrap_or_else(|| LETTER_AVATAR.to_string()),
            engine: self.engine.trim().to_string(),
            model: self.model.map(|model| model.trim().to_string()).filter(|model| !model.is_empty()),
            personality: clean_text(&self.personality, MAX_PERSONALITY_CHARS),
            instructions: clean_text(&self.instructions, MAX_INSTRUCTIONS_CHARS),
            skills: self
                .skills
                .into_iter()
                .map(|skill| Skill {
                    name: clean_line(&skill.name, MAX_SKILL_NAME_CHARS),
                    text: clean_text(&skill.text, MAX_SKILL_CHARS),
                })
                .collect(),
            client,
        }
    }
}

impl Bots {
    /// The bots in `dir` (`<config>/assistant/bots`, [`folder`]); none for `None`. Reads nothing yet.
    pub fn new(dir: Option<PathBuf>) -> Bots {
        Bots { dir, changing: Mutex::new(()) }
    }

    /// Every bot, by name (ignoring case), then id. None when they cannot be read (the core's log
    /// says why).
    pub async fn list(&self) -> Vec<Bot> {
        let Some(dir) = self.dir.clone() else { return Vec::new() };
        let mut bots = blocking(move || store::load_all(&dir)).await.unwrap_or_else(|err| {
            eprintln!("torroworkplace: cannot read the bots: {err}");
            Vec::new()
        });
        bots.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()).then(a.id.cmp(&b.id)));
        bots
    }

    /// The bot `id`; `None` for none, also one that cannot be read (the core's log says why).
    pub async fn get(&self, id: &str) -> Option<Bot> {
        let dir = self.dir.clone()?;
        let wanted = id.to_string();
        blocking(move || store::load(&dir, &wanted)).await.unwrap_or_else(|err| {
            eprintln!("torroworkplace: cannot read the bot {id}: {err}");
            None
        })
    }

    /// The ids of every bot; none when they cannot be read.
    pub async fn ids(&self) -> Vec<String> {
        let Some(dir) = self.dir.clone() else { return Vec::new() };
        blocking(move || store::ids(&dir)).await.unwrap_or_else(|err| {
            eprintln!("torroworkplace: cannot read the bots: {err}");
            Vec::new()
        })
    }

    /// What a `bots.*` request (but `bots.ask_bot`, which the assistant checks) would be refused
    /// with anyway, checked before anyone is asked to approve it and again by [`answer`]: its
    /// limits ([`torro_ipc::bots::limit_refusal`]), a bot that is not there (`not-found`), a new bot
    /// beyond [`MAX_BOTS`] (`error`). `None` when it may run and for every other request.
    pub async fn refusal(&self, request: &Request) -> Option<Response> {
        if let Some(refused) = torro_ipc::bots::limit_refusal(request) {
            return Some(refused);
        }
        match request {
            Request::GetBot { id } | Request::DeleteBot { id } | Request::SaveBot { id: Some(id), .. } => {
                self.get(id).await.is_none().then(|| no_bot(id))
            }
            Request::SaveBot { id: None, .. } if self.ids().await.len() >= MAX_BOTS => Some(full()),
            _ => None,
        }
    }

    /// `bots.save_bot` once its limits were checked: a new bot with a client of its own without
    /// rights, or the bot `draft.id` replaced whole (its client kept, renamed with it, or made anew
    /// without rights when the person removed it). Answers with the bot as stored. A client made
    /// here is taken back when the bot cannot be stored.
    pub async fn save(&self, clients: &Clients, draft: Draft) -> Response {
        let Some(dir) = self.dir.clone() else { return failure(NO_PLACE) };
        let _changing = self.changing.lock().await;
        let mut made = None;
        let bot = match draft.id.clone() {
            Some(id) => {
                let Some(old) = self.get(&id).await else { return no_bot(&id) };
                let mut bot = draft.into_bot(id, old.client.clone());
                if clients.list().iter().any(|info| info.id == old.client && info.kind == BOT_CLIENT) {
                    if old.name != bot.name
                        && let Err(err) = clients.rename(&old.client, &bot.name).await
                    {
                        // The bot is what the person changed; its client keeps the old name until the
                        // next save.
                        eprintln!("torroworkplace: cannot rename the client of the bot {}: {err}", bot.id);
                    }
                } else {
                    // The person removed its client: a new one, again without any rights.
                    match new_client(clients, &bot.name).await {
                        Ok(client) => bot.client = client.clone(),
                        Err(refused) => return refused,
                    }
                    made = Some(bot.client.clone());
                }
                bot
            }
            None => {
                let ids = self.ids().await;
                if ids.len() >= MAX_BOTS {
                    return full();
                }
                let id = match store::new_id(&ids) {
                    Ok(id) => id,
                    Err(err) => return failure(&err),
                };
                let name = clean_line(&draft.name, MAX_NAME_CHARS);
                let client = match new_client(clients, &name).await {
                    Ok(client) => client,
                    Err(refused) => return refused,
                };
                made = Some(client.clone());
                draft.into_bot(id, client)
            }
        };
        let saved = bot.clone();
        match blocking(move || store::save(&dir, &saved)).await {
            Ok(()) => Response::Bot { bot },
            Err(err) => {
                eprintln!("torroworkplace: cannot store the bot {}: {err}", bot.id);
                if let Some(client) = made
                    && let Err(err) = clients.remove(&client).await
                {
                    eprintln!("torroworkplace: cannot take back the client of the bot {}: {err}", bot.id);
                }
                failure("the bot cannot be stored")
            }
        }
    }

    /// `bots.delete_bot`: removes the bot's client (its key stops working at once; one the person
    /// removed already is fine), then its folder. Answers the bot as it was, or the error answer.
    pub async fn delete(&self, clients: &Clients, id: &str) -> Result<Bot, Response> {
        let Some(dir) = self.dir.clone() else { return Err(no_bot(id)) };
        let _changing = self.changing.lock().await;
        let Some(bot) = self.get(id).await else { return Err(no_bot(id)) };
        // Only a bot's own client goes with it: `bot.json` may have been edited by hand to name
        // another one, which stays.
        let own = clients.list().iter().any(|info| info.id == bot.client && info.kind == BOT_CLIENT);
        let removed = match own {
            true => clients.remove(&bot.client).await,
            false => Ok(()),
        };
        match removed {
            Ok(()) | Err(ClientsError::NotFound(_)) => {}
            Err(err) => {
                eprintln!("torroworkplace: cannot remove the client of the bot {id}: {err}");
                return Err(failure("the bot's access cannot be removed; try again"));
            }
        }
        let gone = id.to_string();
        if let Err(err) = blocking(move || store::remove(&dir, &gone)).await {
            eprintln!("torroworkplace: cannot remove the bot {id}: {err}");
            return Err(failure("the bot cannot be deleted; try again"));
        }
        Ok(bot)
    }
}

/// Answers `bots.list_bots`, `bots.get_bot`, `bots.save_bot` and `bots.delete_bot`, for the app key
/// and for a client the core admitted (`crate::core::Core::handle_from`). `bots.ask_bot` is the
/// assistant's (`crate::assistant::answer`). What would be refused anyway comes first
/// ([`Bots::refusal`]); the handlers check again under their lock.
pub async fn answer(assistant: &Assistant, clients: &Clients, request: Request) -> Response {
    let bots = assistant.bots();
    if let Some(refused) = bots.refusal(&request).await {
        return refused;
    }
    match request {
        Request::ListBots => Response::Bots { bots: bots.list().await.iter().map(Bot::summary).collect() },
        Request::GetBot { id } => match bots.get(&id).await {
            Some(bot) => Response::Bot { bot },
            None => no_bot(&id),
        },
        request @ Request::SaveBot { .. } => match Draft::of(request) {
            Some(draft) => bots.save(clients, draft).await,
            None => failure("no bot to save"),
        },
        Request::DeleteBot { id } => match bots.delete(clients, &id).await {
            Ok(_) => {
                // Its client is gone, so its tools fail anyway; the answers stop here as well.
                assistant.stop_bot(&id).await;
                Response::Ok
            }
            Err(refused) => refused,
        },
        other => failure(&format!("{} is no command of the bots", other.operation().name())),
    }
}

/// Makes the client of a bot named `name`: kind [`BOT_CLIENT`], no rights. Its key is made and
/// kept in the keychain as for every client, but nobody is given it: the core runs the bot as its
/// client, and engine B lends a key of its own for each run.
async fn new_client(clients: &Clients, name: &str) -> Result<String, Response> {
    match clients.add_client(name, BOT_CLIENT, None, Rights::none()).await {
        Ok((info, _key)) => Ok(info.id),
        Err(err) => {
            eprintln!("torroworkplace: cannot make a client for the bot {name:?}: {err}");
            Err(failure("the bot's access cannot be set up"))
        }
    }
}

/// Runs `work` on a blocking thread; a thread that panicked is an I/O error.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> io::Result<T> + Send + 'static) -> io::Result<T> {
    tokio::task::spawn_blocking(work).await.unwrap_or_else(|err| Err(io::Error::other(err)))
}

/// Why a bot cannot be saved without a home directory.
const NO_PLACE: &str = "there is no place to keep bots (no home directory)";

/// The `not-found` answer for bot `id`.
pub fn no_bot(id: &str) -> Response {
    Response::Error { code: "not-found".into(), message: format!("no bot {id:?}") }
}

/// The `error` answer for a new bot beyond [`MAX_BOTS`].
fn full() -> Response {
    failure(&format!("there are {MAX_BOTS} bots already; delete one first"))
}

/// The `error` answer.
fn failure(message: &str) -> Response {
    Response::Error { code: "error".into(), message: message.into() }
}

#[cfg(test)]
mod tests;
