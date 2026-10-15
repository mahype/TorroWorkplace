//! What the window does with bots (task B.3, see the module documentation of [`crate::app`]): the
//! list, a bot's chat, the editor, deleting, and the way a `--bot` window finds its conversation.
//! Every change is a request (`bots.*`, `assistant.open_window`).
//!
//! The editor's section "Access" (task B.5) reads the bot's rights and what exists each time it
//! opens ([`App::read_access`]). "Save" is `bots.save_bot` first, whose answer gives the editor the
//! bot's id and client at once, so a second "Save" never makes a second bot; then, only when a row
//! of "Access" changed, `clients.list_clients` again and `clients.update_client` with the changed
//! rows laid onto what the core holds now ([`crate::bots::BotAccess::onto`]). Only then does the
//! editor close. When the access cannot be set, the bot is saved anyway: the editor stays open and
//! says so, and "Save" tries again.

use iced::advanced::widget::operation::{self, Outcome, scrollable::AbsoluteOffset};
use iced::advanced::widget::{Id, Operation};
use iced::{Rectangle, Task, Vector};
use torro_core::i18n::tr;
use torro_gui::theme::space;
use torro_ipc::bots::{Bot, BotSummary};
use torro_ipc::{ClientInfo, Request, Response};

use super::{App, Dialog, Message, Window};
use crate::backend::Refusal;
use crate::bots::{self, AccessNote, BotAccess, BotDraft, Known, Listed, Proposal};
use crate::demo;
use crate::launch::Open;

impl App {
    /// `bots.list_bots`; the forced states of `--open` keep loading for good.
    pub(super) fn load_bots(&mut self) -> Task<Message> {
        if self.launch.open == Some(Open::Loading) {
            return Task::none();
        }
        self.ask(Request::ListBots, |result| Message::Bots(result.and_then(bots_of)))
    }

    /// The bots came: the list, the engine of a new chat with a bot, and the demo states that need
    /// them. An old core without bots answers with an error: the window shows none.
    pub(super) fn bots_listed(&mut self, result: Result<Vec<BotSummary>, Refusal>) -> Task<Message> {
        match result {
            Ok(bots) => {
                self.bots = bots;
                self.bots_loaded = true;
            }
            Err(refusal) => {
                eprintln!("cannot list the bots: {}", refusal.message);
                return Task::none();
            }
        }
        self.choose_default();
        self.open_launched_bots()
    }

    /// The bot that answers in the open chat: the open conversation's, else the one a new chat
    /// goes to; `None` for the assistant.
    pub(crate) fn chat_bot(&self) -> Option<&str> {
        let summary = self.current.as_ref().and_then(|id| {
            self.transcript
                .summary
                .as_ref()
                .filter(|summary| &summary.id == id)
                .or_else(|| self.conversations.iter().find(|c| &c.id == id))
        });
        match summary {
            Some(summary) => summary.bot.as_deref(),
            None => self.bot.as_deref(),
        }
    }

    /// The listed bot `id`.
    pub(crate) fn listed_bot(&self, id: &str) -> Option<&BotSummary> {
        bots::find(&self.bots, id)
    }

    /// The name of the bot `id`; "Deleted bot" once the bots are listed without it, "Bot" before.
    pub(crate) fn bot_name(&self, id: &str) -> String {
        match self.listed_bot(id) {
            Some(bot) => bot.name.clone(),
            None if self.bots_loaded => tr("Deleted bot").to_owned(),
            None => tr("Bot").to_owned(),
        }
    }

    /// The open chat's bot is gone: its conversation can be read, not continued, and a `--bot`
    /// window of a deleted bot has nothing to send to.
    pub(crate) fn bot_gone(&self) -> bool {
        let deleted = self.transcript.summary.as_ref().is_some_and(|summary| summary.bot_deleted);
        let unlisted = self.bots_loaded && self.chat_bot().is_some_and(|id| self.listed_bot(id).is_none());
        deleted || unlisted
    }

    /// A `--bot` window, once the conversations are listed: the bot's newest conversation, else a
    /// new chat with it. Once only, so a conversation the person left stays left.
    pub(super) fn resolve_bot_window(&mut self) -> Task<Message> {
        let Window::Bot(id) = self.launch.window.clone() else { return Task::none() };
        if std::mem::replace(&mut self.bot_resolved, true) || self.current.is_some() {
            return Task::none();
        }
        match bots::newest_conversation(&self.conversations, &id).map(|c| c.id.clone()) {
            Some(newest) => {
                self.reset_chat(Some(newest));
                self.open_current()
            }
            None => Task::none(),
        }
    }

    /// The bot's chat in this window: its newest conversation or a new one.
    pub(super) fn open_bot(&mut self, id: String) -> Task<Message> {
        match bots::newest_conversation(&self.conversations, &id).map(|c| c.id.clone()) {
            Some(newest) => self.select(newest),
            None => self.new_bot_chat(id),
        }
    }

    /// A new chat with the bot `id`.
    pub(super) fn new_bot_chat(&mut self, id: String) -> Task<Message> {
        self.close_menus();
        self.bot_draft = None;
        self.bot = Some(id);
        self.reset_chat(None);
        if self.chosen != super::Chosen::Person {
            self.chosen = super::Chosen::Default;
        }
        self.choose_default();
        iced::widget::operation::focus(super::EDITOR)
    }

    /// "New bot": the editor of a new bot, answering with the window's engine.
    pub(super) fn new_bot(&mut self) -> Task<Message> {
        self.close_menus();
        self.bot_draft = Some(BotDraft::new(self.choice.clone()));
        self.read_access()
    }

    /// "Edit bot": the whole bot first (`bots.get_bot`), with its long texts.
    pub(super) fn edit_bot(&mut self, id: String) -> Task<Message> {
        self.close_menus();
        self.ask(Request::GetBot { id }, |result| Message::BotLoaded(result.and_then(bot_of)))
    }

    pub(super) fn bot_loaded(&mut self, result: Result<Bot, Refusal>) -> Task<Message> {
        match result {
            Ok(bot) => {
                self.bot_draft = Some(BotDraft::of(&bot));
                let read = self.read_access();
                if self.launch.open != Some(Open::BotAccess) {
                    return read;
                }
                // The demo state for screenshots: the form at its section "Access".
                Task::batch([read, scroll_to_access()])
            }
            Err(refusal) => {
                self.note_failure(tr("Couldn't open the bot."), &refusal);
                Task::none()
            }
        }
    }

    /// The editor opened: the clients (the bot's rights) and what exists, read anew each time, so
    /// the section "Access" never shows an old state.
    fn read_access(&mut self) -> Task<Message> {
        self.known = Known::default();
        let clients = self.ask(Request::ListClients, |result| Message::BotClients(result.and_then(clients_of)));
        let calendars = self
            .ask(Request::CalendarCollections, |result| Message::Known(collection_ids(result).map(Listed::Calendars)));
        let lists =
            self.ask(Request::TaskCollections, |result| Message::Known(collection_ids(result).map(Listed::TaskLists)));
        let sources = self.ask(Request::ListSources, |result| {
            Message::Known(result.and_then(|response| match response {
                Response::Sources { sources } => Ok(Listed::Sources(sources)),
                other => Err(Refusal::unexpected(&other)),
            }))
        });
        let accounts = self.ask(Request::Accounts, |result| {
            Message::Known(result.and_then(|response| match response {
                Response::Accounts { accounts } => Ok(Listed::Accounts(accounts)),
                other => Err(Refusal::unexpected(&other)),
            }))
        });
        Task::batch([clients, calendars, lists, sources, accounts])
    }

    /// The clients the editor read when it opened: the section "Access" starts from the bot's
    /// rights (a new bot's from none). Taken once per opening; without them the section says it
    /// cannot show the access, and saving leaves it alone.
    pub(super) fn bot_clients(&mut self, result: Result<Vec<ClientInfo>, Refusal>) {
        let clients = match result {
            Ok(clients) => clients,
            Err(refusal) => {
                eprintln!("cannot read the bot's access: {}", refusal.message);
                Vec::new()
            }
        };
        let Some(draft) = self.bot_draft.as_mut().filter(|draft| draft.access.is_none()) else { return };
        draft.access = BotAccess::of(draft.client.as_deref(), &clients);
        draft.access_note = draft.access.is_none().then_some(AccessNote::Unreadable);
        self.known.clients = clients;
    }

    /// "Save": `bots.save_bot` once the bot keeps its limits; else the problem shows from now on.
    /// After a save that stored the bot but not its access, it goes again with the bot's id.
    pub(super) fn save_bot(&mut self) -> Task<Message> {
        let Some(draft) = self.bot_draft.as_mut() else { return Task::none() };
        draft.tried = true;
        if draft.saving {
            return Task::none();
        }
        if draft.access_note == Some(AccessNote::NotSet) {
            draft.access_note = None;
        }
        let Some(request) = draft.request() else {
            // The problem shows at the top of the form, which goes there; a problem of the name also
            // takes the focus into it.
            let name = draft.name.trim();
            let at_name = name.is_empty() || name.chars().count() > torro_ipc::bots::MAX_NAME_CHARS;
            let top = iced::widget::operation::snap_to(super::BOT_FORM, iced::widget::operation::RelativeOffset::START);
            return if at_name { Task::batch([top, iced::widget::operation::focus(super::BOT_NAME)]) } else { top };
        };
        draft.saving = true;
        self.ask(request, |result| Message::BotSaved(result.and_then(bot_of)))
    }

    /// The bot is saved: the editor takes its id and client at once. When a row of "Access" changed,
    /// the access follows ([`App::access_now`]); else the editor closes ([`App::bot_done`]).
    pub(super) fn bot_saved(&mut self, result: Result<Bot, Refusal>) -> Task<Message> {
        match result {
            Ok(bot) => {
                self.bots.retain(|listed| listed.id != bot.id);
                self.bots.push(bot.summary());
                let Some(draft) = self.bot_draft.as_mut() else { return self.load_bots() };
                draft.id = Some(bot.id.clone());
                draft.client = Some(bot.client.clone());
                if !draft.access.as_ref().is_some_and(BotAccess::changed) {
                    return self.bot_done(bot.id);
                }
                let again = self.ask(Request::ListClients, |result| Message::AccessNow(result.and_then(clients_of)));
                Task::batch([again, self.load_bots()])
            }
            Err(refusal) => {
                if let Some(draft) = self.bot_draft.as_mut() {
                    draft.saving = false;
                }
                self.note_failure(tr("Couldn't save the bot."), &refusal);
                Task::none()
            }
        }
    }

    /// The clients as the core holds them now, after the save: the changed rows of "Access" laid
    /// onto the bot's client's rights go out as `clients.update_client`, unless they change
    /// nothing there.
    pub(super) fn access_now(&mut self, result: Result<Vec<ClientInfo>, Refusal>) -> Task<Message> {
        let Some(draft) = self.bot_draft.as_ref().filter(|draft| draft.saving) else { return Task::none() };
        let (Some(id), Some(client), Some(access)) = (draft.id.clone(), draft.client.clone(), draft.access.as_ref())
        else {
            return Task::none();
        };
        let clients = match result {
            Ok(clients) => clients,
            Err(refusal) => return self.access_failed(&refusal),
        };
        let Some(current) = clients.iter().find(|info| info.id == client).map(|info| info.rights.clone()) else {
            return self.access_failed(&Refusal::new("not-found", format!("no client {client:?}")));
        };
        let rights = access.onto(&current, &self.known.present());
        self.known.clients = clients;
        if rights == current {
            return self.bot_done(id);
        }
        self.ask(Request::UpdateClient { id: client, rights }, |result| Message::AccessSet(result.map(|_| ())))
    }

    /// `clients.update_client` answered: done, or the bot is saved without its access.
    pub(super) fn access_set(&mut self, result: Result<(), Refusal>) -> Task<Message> {
        let Some(id) = self.bot_draft.as_ref().filter(|draft| draft.saving).and_then(|draft| draft.id.clone()) else {
            return Task::none();
        };
        match result {
            Ok(()) => self.bot_done(id),
            Err(refusal) => self.access_failed(&refusal),
        }
    }

    /// Everything is saved: the editor closes; a new bot's chat opens, so the person can talk to it
    /// at once.
    fn bot_done(&mut self, id: String) -> Task<Message> {
        let new = self.bot_draft.take().is_some_and(|draft| draft.new);
        self.note(tr("Bot saved").to_owned());
        let chat = if new { self.new_bot_chat(id) } else { Task::none() };
        Task::batch([chat, self.load_bots()])
    }

    /// The bot is saved, its access not: the editor stays open with the note at its top, and the
    /// next "Save" tries again with the bot's id.
    fn access_failed(&mut self, refusal: &Refusal) -> Task<Message> {
        eprintln!("cannot set the bot's access: {}", refusal.message);
        if let Some(draft) = self.bot_draft.as_mut() {
            draft.saving = false;
            draft.access_note = Some(AccessNote::NotSet);
        }
        let top = iced::widget::operation::snap_to(super::BOT_FORM, iced::widget::operation::RelativeOffset::START);
        Task::batch([top, self.load_bots()])
    }

    /// "Delete bot…": asks first.
    pub(super) fn ask_delete_bot(&mut self, id: String) {
        self.close_menus();
        let name = self.bot_name(&id);
        self.dialog = Some(Dialog::DeleteBot { id, name });
    }

    pub(super) fn delete_bot(&mut self) -> Task<Message> {
        let Some(Dialog::DeleteBot { id, .. }) = self.dialog.take() else { return Task::none() };
        let request = Request::DeleteBot { id: id.clone() };
        self.ask(request, move |result| Message::BotDeleted(id.clone(), result.map(|_| ())))
    }

    /// Deleted: its editor closes; the main window goes back to a new chat with the assistant when
    /// the bot's chat was open. Its conversations stay, as of a deleted bot.
    pub(super) fn bot_deleted(&mut self, id: String, result: Result<(), Refusal>) -> Task<Message> {
        if let Err(refusal) = result {
            self.note_failure(tr("Couldn't delete the bot."), &refusal);
            return Task::none();
        }
        self.bots.retain(|bot| bot.id != id);
        if self.bot_draft.as_ref().is_some_and(|draft| draft.id.as_deref() == Some(id.as_str())) {
            self.bot_draft = None;
        }
        self.note(tr("Bot deleted").to_owned());
        let back = if !self.chat_only && self.chat_bot() == Some(id.as_str()) { self.new_chat() } else { Task::none() };
        Task::batch([back, self.load_bots(), self.load_list(), self.follow()])
    }

    /// "Ask {bot}" on a proposal card: `assistant.confirm_bot` – only for a person's click, once at a
    /// time.
    pub(super) fn confirm_bot(&mut self, proposal: Proposal) -> Task<Message> {
        if self.confirming.is_some() || self.sending || self.transcript.running() {
            return Task::none();
        }
        self.confirming = Some(proposal.clone());
        let request = bots::confirm_request(&proposal);
        self.ask(request, move |result| Message::BotConfirmed(proposal.clone(), result.map(|_| ())))
    }

    /// Confirmed: the person's "Yes, ask Travel planner." follows in the conversation, so the
    /// assistant goes on and asks the bot – with the window's engine, as any message.
    pub(super) fn bot_confirmed(&mut self, proposal: Proposal, result: Result<(), Refusal>) -> Task<Message> {
        self.confirming = None;
        if let Err(refusal) = result {
            self.note_failure(tr("Couldn't confirm the bot."), &refusal);
            return Task::none();
        }
        if self.current.as_deref() != Some(proposal.conversation.as_str()) || self.sending {
            return Task::none();
        }
        let text = bots::confirm_text(&self.bot_name(&proposal.bot));
        let effort = self.offered_effort();
        let Some(request) = crate::chat::send_request(
            self.current.as_deref(),
            self.choice.as_ref(),
            effort,
            &text,
            (&self.without, self.mode),
            None,
        ) else {
            return Task::none();
        };
        self.sending = true;
        self.ask(request, |result| Message::Sent(super::conversation_of(result)))
    }

    /// The states of `--open` that need the bots listed: the editor, a new bot with its problem,
    /// and the question before deleting.
    fn open_launched_bots(&mut self) -> Task<Message> {
        if std::mem::replace(&mut self.opened_bots, true) {
            return Task::none();
        }
        match self.launch.open {
            Some(Open::BotEditor | Open::BotAccess) => self.edit_bot(demo::TRAVEL.into()),
            Some(Open::BotNew) => {
                let read = self.new_bot();
                Task::batch([read, self.save_bot()])
            }
            Some(Open::BotDelete) => {
                self.ask_delete_bot(demo::TRAVEL.into());
                Task::none()
            }
            _ => Task::none(),
        }
    }
}

/// The bots of an answer of `bots.list_bots`.
fn bots_of(response: Response) -> Result<Vec<BotSummary>, Refusal> {
    match response {
        Response::Bots { bots } => Ok(bots),
        other => Err(Refusal::unexpected(&other)),
    }
}

/// Scrolls the bot form so its section "Access" ([`super::BOT_ACCESS`]) stands at the top, with
/// the form's own margin above it: for the demo state `--open bot-access`. Iced scrolls only by
/// offsets, so the operation finds the form and the section in the layout first.
pub(super) fn scroll_to_access() -> Task<Message> {
    iced::advanced::widget::operate(ToAccess { form: None, access: None })
}

/// The operation of [`scroll_to_access`]: the top of the form's content and of the section.
struct ToAccess {
    form: Option<f32>,
    access: Option<f32>,
}

impl<T: 'static> Operation<T> for ToAccess {
    fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation<T>)) {
        operate(self);
    }

    fn container(&mut self, id: Option<&Id>, bounds: Rectangle) {
        if id == Some(&Id::new(super::BOT_ACCESS)) {
            self.access = Some(bounds.y);
        }
    }

    fn scrollable(
        &mut self,
        id: Option<&Id>,
        _bounds: Rectangle,
        content: Rectangle,
        _translation: Vector,
        _state: &mut dyn operation::Scrollable,
    ) {
        if id == Some(&Id::new(super::BOT_FORM)) {
            self.form = Some(content.y);
        }
    }

    fn finish(&self) -> Outcome<T> {
        let (Some(form), Some(access)) = (self.form, self.access) else { return Outcome::None };
        // The children of a scrollable are laid out unscrolled, so the distance is the offset.
        let y = (access - form - space::xl()).max(0.0);
        let to = operation::scrollable::scroll_to(Id::new(super::BOT_FORM), AbsoluteOffset { x: None, y: Some(y) });
        Outcome::Chain(Box::new(to))
    }
}

/// The clients of an answer of `clients.list_clients`.
fn clients_of(response: Response) -> Result<Vec<ClientInfo>, Refusal> {
    match response {
        Response::Clients { clients } => Ok(clients),
        other => Err(Refusal::unexpected(&other)),
    }
}

/// The ids of the collections of an answer of `calendar.list_calendars` or `tasks.list_lists`.
fn collection_ids(result: Result<Response, Refusal>) -> Result<Vec<String>, Refusal> {
    match result? {
        Response::Collections { collections } => Ok(collections.into_iter().map(|c| c.id).collect()),
        other => Err(Refusal::unexpected(&other)),
    }
}

/// The bot of an answer of `bots.get_bot` or `bots.save_bot`.
pub(super) fn bot_of(response: Response) -> Result<Bot, Refusal> {
    match response {
        Response::Bot { bot } => Ok(bot),
        other => Err(Refusal::unexpected(&other)),
    }
}
