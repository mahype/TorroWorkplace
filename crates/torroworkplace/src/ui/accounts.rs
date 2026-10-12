//! Accounts (task S.5 in `docs/tasks/steuerzentrale.md`, several accounts since M1.2): the
//! accounts as rows of one settings group per provider (status dot, label on one line, status and
//! last sync below, chevron), "Add account" in the header bar – always, also with accounts; a
//! detail page per account with groups for the status ("Sync now"), the calendars and task lists
//! (a switch each for showing them in the apps, read-only ones marked) and the connection, and
//! "Remove account…" behind a confirmation that names the account and acts on it alone. A
//! sign-in in progress (an entry without an id) is a "Signing in…" row without a detail page.
//! Empty: a sentence about what to add. The add flow is `wizard.rs` (task S.6); it and the confirmation
//! are dialogs over the whole window ([`overlay`]). The shell hands the answers to requests to
//! [`answered`], so the dialog shows the outcome of its own requests.
//!
//! A GitHub account (task G.1) has no calendars or lists yet: its detail page shows what the
//! token's scopes allow and what is missing ([`github_permissions`]) in place of them.
//!
//! A CalDAV account (a mail provider's, task M7.2) shows "CalDAV" as its kind, the server and user
//! in the connection group, and "Change password…" there: a dialog over the window asks for the
//! new password and sends `ConnectCalDav` for the same server and user again (the core then
//! replaces only the password). A refused password stays in the dialog under its field.
//!
//! A mail account (task M5.2, `AccountInfo::mail`) has a page of its own ([`mail_detail`]): the
//! state of each service (mail, calendars and tasks) with its last sync and problem and "Sync now",
//! the question "Which folder holds sent mail?" while the server left a folder role open, the
//! sender name and signature (typed into a draft, saved with "Save"), "Show new mail in Inbox",
//! the services as switches, and the connection: "Server settings…" and "Change password…" open
//! dialogs that send `ConnectMail` again (the only way to change servers or password; the
//! password lives in the dialog as a [`Secret`] until it is sent). Everything else goes through
//! `UpdateMailAccount`, which changes settings only.
//!
//! Task M3.5: "Server settings" always has the calendar address (CalDAV), so an account connected
//! for mail only (a TorroMail import, for one) gets calendars and tasks later: their switches open
//! that dialog while the account has no address, and saving it switches the service on with the
//! address. The sender name and signature follow changes from outside while nothing is typed
//! into them ([`loaded`], called by the shell on every reload). Both mail servers are typed as in
//! the add-account dialog (`server_fields.rs`).
//!
//! Requests: `Sync`, `RemoveAccount { id }`, `ConnectCalDav` / `ConnectMail` for "Change
//! password…", `ConnectMail` for "Server settings…", `UpdateMailAccount` for the settings of a mail
//! account, and `SetCollectionShown { collection, shown }` for the switches. The pure parts
//! (relative time, status text, whether a collection is shown, which accounts are listed, what a
//! draft changes) are plain functions with unit tests.

use std::collections::BTreeMap;
use std::fmt;

use chrono::{DateTime, Utc};
use iced::widget::space as blank;
use iced::widget::text::{IntoFragment, Wrapping};
use iced::widget::{Space, column, container, pick_list, row, text, text_editor, toggler};
use iced::{Alignment, Element, Fill, Theme};
use torro_core::account::Secret;
use torro_core::i18n::{tr, trf};
use torro_core::settings::Settings;
use torro_gui::dialog::frame as dialog_frame;
use torro_gui::icon::Icon;
use torro_gui::layout::SizeClass;
use torro_gui::theme::{self, palette, size, space};
use torro_gui::widgets::{self, Status, collection_color, color_dot, empty_state};
use torro_gui::{style, typo};
use torro_ipc::{AccountInfo, MailView, OpenRoleView, Request, Response, ServiceState};
use torro_mail::{Notify, Role, Security, Services};
use torro_pim::{AccountState, Collection, CollectionKind};

use super::server_fields::{Server, ServerFields, imap_titles, server_group, smtp_titles};
use super::{Action, Data, Header, chevron, confirm, dialog, pressable, row_frame, setting_row, wizard};

#[derive(Debug, Default)]
pub struct State {
    /// The add-account dialog while it is open.
    pub wizard: Option<wizard::State>,
    /// The account whose detail page is open (its `AccountInfo::id`); `None` shows the list.
    pub selected: Option<String>,
    /// The "Remove this account?" confirmation is open.
    pub confirm_remove: bool,
    /// The "Change password" dialog of the open CalDAV or mail account.
    pub password: Option<PasswordChange>,
    /// The sender name and signature of the open mail account as they are being typed.
    pub mail: Option<MailDraft>,
    /// The "Server settings" dialog of the open mail account.
    pub servers: Option<ServerSettings>,
}

/// The "Change password" dialog: the new password, what went wrong, and whether the request is on
/// its way (then the button waits for the answer).
#[derive(Debug, Default)]
pub struct PasswordChange {
    password: Secret,
    error: Option<String>,
    sending: bool,
}

/// What is typed into the sender name and signature of a mail account's page. Both are saved
/// together with "Save" ([`MailDraft::changes`] names what differs from the stored values).
#[derive(Debug, Default)]
pub struct MailDraft {
    /// The account it belongs to (`AccountInfo::id`), so a draft never lands on another account.
    account: String,
    sender_name: String,
    signature: text_editor::Content,
    /// The sender name and signature stored when the draft was made (signature without trailing
    /// white space). While the draft still holds them, nothing was typed, and a change from
    /// outside replaces the draft ([`loaded`], task M3.5).
    stored: (String, String),
}

impl MailDraft {
    /// The stored values of `account`; `None` for an account that is not a mail account.
    fn of(account: &AccountInfo) -> Option<MailDraft> {
        let mail = account.mail.as_ref()?;
        let signature = mail.settings.signature.as_deref().unwrap_or_default();
        Some(MailDraft {
            account: account.id.clone(),
            sender_name: mail.sender_name.clone(),
            signature: text_editor::Content::with_text(signature),
            stored: (mail.sender_name.clone(), signature.trim_end().to_string()),
        })
    }

    /// The signature as sent: line breaks as `\n` whatever the platform, no trailing white space.
    fn signature_text(&self) -> String {
        self.signature.text().replace("\r\n", "\n").trim_end().to_string()
    }

    /// The draft holds `stored` (a sender name and signature as [`MailDraft::stored`] keeps them),
    /// as saving would compare them.
    fn holds(&self, (sender_name, signature): &(String, String)) -> bool {
        self.sender_name.trim() == sender_name.trim() && self.signature_text() == *signature
    }

    /// Only what differs from `mail`, so that saving asks for no more than the person changed.
    fn changes(&self, mail: &MailView) -> MailChange {
        let sender_name = self.sender_name.trim();
        let signature = self.signature_text();
        let stored_signature = mail.settings.signature.as_deref().unwrap_or_default();
        MailChange {
            sender_name: (sender_name != mail.sender_name.trim()).then(|| sender_name.to_string()),
            signature: (signature != stored_signature.trim_end()).then_some(signature),
            ..MailChange::default()
        }
    }
}

/// The fields of `UpdateMailAccount` that a click changes; the others stay as they are.
#[derive(Debug, Default, PartialEq, Eq)]
struct MailChange {
    sender_name: Option<String>,
    signature: Option<String>,
    notify: Option<Notify>,
    services: Option<Services>,
    folder_roles: Option<BTreeMap<Role, String>>,
}

impl MailChange {
    fn is_empty(&self) -> bool {
        *self == MailChange::default()
    }

    fn request(self, id: &str) -> Request {
        Request::UpdateMailAccount {
            id: id.to_string(),
            sender_name: self.sender_name,
            signature: self.signature,
            notify: self.notify,
            services: self.services,
            folder_roles: self.folder_roles,
        }
    }
}

/// The "Server settings" dialog: both mail servers as they are stored, the calendar address
/// (always, empty for an account without one, so calendars and tasks can be added later, task
/// M3.5), and the password, which the servers are checked with. The password is a [`Secret`], so
/// even a `Debug` of the dialog shows none, and it is taken when sent.
#[derive(Debug)]
pub struct ServerSettings {
    imap: ServerFields,
    smtp: ServerFields,
    /// The calendar address as typed; empty keeps the stored one (or none).
    caldav: String,
    /// The service whose switch opened the dialog because the account has no calendar address
    /// (task M3.5): it is switched on with the address. `None` for "Server settings…".
    adding: Option<MailService>,
    password: Secret,
    error: Option<String>,
    sending: bool,
}

impl ServerSettings {
    fn of(mail: &MailView) -> ServerSettings {
        ServerSettings {
            imap: ServerFields::of(&mail.imap),
            smtp: ServerFields::of(&mail.smtp),
            caldav: mail.caldav.clone().unwrap_or_default(),
            adding: None,
            password: Secret::default(),
            error: None,
            sending: false,
        }
    }

    fn server_mut(&mut self, server: Server) -> &mut ServerFields {
        match server {
            Server::Imap => &mut self.imap,
            Server::Smtp => &mut self.smtp,
        }
    }

    /// The calendar address to send; `None` when the field is empty.
    fn caldav_address(&self) -> Option<String> {
        Some(self.caldav.trim().to_string()).filter(|address| !address.is_empty())
    }

    /// Both servers are valid and the password is typed, and a service to add has its address:
    /// the servers can be checked.
    fn can_send(&self) -> bool {
        !self.sending
            && !self.password.is_blank()
            && self.imap.endpoint().is_some()
            && self.smtp.endpoint().is_some()
            && (self.adding.is_none() || self.caldav_address().is_some())
    }
}

impl State {
    /// A sign-in is running; the shell reloads more often.
    pub fn busy(&self) -> bool {
        self.wizard.as_ref().is_some_and(wizard::State::busy)
    }
}

#[derive(Debug, Clone)]
pub enum Message {
    AddAccount,
    Wizard(wizard::Message),
    /// Opens the detail page of an account.
    Open(String),
    /// Back from a detail page to the list.
    Back,
    SyncNow,
    /// The "Show in apps" switch of one calendar or task list.
    SetShown {
        collection: String,
        shown: bool,
    },
    AskRemove,
    CancelRemove,
    Remove,
    /// "Change password…" on a CalDAV or mail account's page.
    ChangePassword,
    /// The new password was typed. A [`Secret`], so that `Debug` of the message shows none.
    PasswordEdited(Secret),
    /// "Change password" in the dialog, or Enter in its field.
    SubmitPassword,
    ClosePassword,
    /// The sender name was typed.
    SenderNameEdited(String),
    SignatureEdited(text_editor::Action),
    /// "Save": sender name and signature, as far as they differ from the stored ones.
    SaveMail,
    /// "Show new mail in Inbox" was set.
    SetNotify(Notify),
    /// The switch of a service of the mail account.
    SetService(MailService, bool),
    /// A folder was picked for a role the server left open.
    PickFolder {
        role: Role,
        path: String,
    },
    /// "Server settings…" on a mail account's page.
    ChangeServers,
    ServerHost(Server, String),
    ServerPort(Server, String),
    ServerSecurity(Server, Security),
    CaldavEdited(String),
    /// The password the servers are checked with. A [`Secret`], so that `Debug` shows none.
    ServersPasswordEdited(Secret),
    SubmitServers,
    CloseServers,
}

/// What Escape means on this page right now: close the add-account dialog, the password or server
/// dialog or the remove confirmation.
pub fn escape(state: &State) -> Option<Message> {
    if state.wizard.is_some() {
        Some(Message::Wizard(wizard::Message::Close))
    } else if state.password.is_some() {
        Some(Message::ClosePassword)
    } else if state.servers.is_some() {
        Some(Message::CloseServers)
    } else if state.confirm_remove {
        Some(Message::CancelRemove)
    } else {
        None
    }
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        // The dialog asks for TorroMail's accounts as it opens (task M7.4).
        Message::AddAccount => {
            let (wizard, ask) = wizard::State::open();
            state.wizard = Some(wizard);
            ask
        }
        Message::Wizard(wizard::Message::Close) => {
            state.wizard = None;
            Action::None
        }
        Message::Wizard(message) => match state.wizard.as_mut() {
            Some(wizard) => wizard::update(wizard, message, data),
            None => Action::None,
        },
        Message::Open(id) => {
            state.selected = Some(id);
            state.confirm_remove = false;
            state.password = None;
            state.servers = None;
            state.mail = selected(state, data).and_then(MailDraft::of);
            Action::None
        }
        Message::Back => {
            state.selected = None;
            state.confirm_remove = false;
            state.password = None;
            state.servers = None;
            state.mail = None;
            Action::None
        }
        Message::SyncNow => Action::Send(Request::Sync),
        Message::SetShown { collection, shown } => Action::Send(Request::SetCollectionShown { collection, shown }),
        // Only an open detail page has an account to remove.
        Message::AskRemove => {
            state.confirm_remove = selected(state, data).is_some();
            Action::None
        }
        Message::CancelRemove => {
            state.confirm_remove = false;
            Action::None
        }
        Message::Remove => {
            state.confirm_remove = false;
            match state.selected.take() {
                Some(id) => Action::Send(Request::RemoveAccount { id }),
                None => Action::None,
            }
        }
        // Only a connected CalDAV account and a mail account have a password to change.
        Message::ChangePassword => {
            state.password =
                selected(state, data).filter(|a| can_change_password(a)).map(|_| PasswordChange::default());
            Action::None
        }
        Message::PasswordEdited(password) => {
            if let Some(change) = state.password.as_mut() {
                change.password = password;
                change.error = None;
            }
            Action::None
        }
        Message::SubmitPassword => submit_password(state, data),
        Message::ClosePassword => {
            state.password = None;
            Action::None
        }
        Message::SenderNameEdited(name) => {
            if let Some(draft) = draft(state, data) {
                draft.sender_name = name;
            }
            Action::None
        }
        Message::SignatureEdited(action) => {
            if let Some(draft) = draft(state, data) {
                draft.signature.perform(action);
            }
            Action::None
        }
        Message::SaveMail => save_mail(state, data),
        Message::SetNotify(notify) => with_mail(state, data, |mail| {
            (notify != mail.settings.notify).then(|| MailChange { notify: Some(notify), ..MailChange::default() })
        }),
        // Calendars and tasks of an account without a calendar address (task M3.5): the switch
        // asks for the address in "Server settings", which switches the service on with it.
        Message::SetService(which, true) if which.needs_caldav() && open_mail_lacks_caldav(state, data) => {
            state.password = None;
            state.servers = selected_mail(state, data)
                .map(|(_, mail)| ServerSettings { adding: Some(which), ..ServerSettings::of(mail) });
            Action::None
        }
        Message::SetService(which, on) => with_mail(state, data, |mail| {
            let mut services = mail.settings.services;
            match which {
                MailService::Mail => services.mail = on,
                MailService::Calendars => services.calendars = on,
                MailService::Tasks => services.tasks = on,
            }
            (services != mail.settings.services)
                .then(|| MailChange { services: Some(services), ..MailChange::default() })
        }),
        // Only a role the server left open, and only one of its candidates: the core refuses
        // anything else, so the page does not even ask.
        Message::PickFolder { role, path } => with_mail(state, data, |mail| {
            let offered = mail
                .open_roles
                .iter()
                .any(|open| open.role == role && open.candidates.iter().any(|folder| folder.path == path));
            offered.then(|| MailChange { folder_roles: Some(BTreeMap::from([(role, path)])), ..MailChange::default() })
        }),
        Message::ChangeServers => {
            state.password = None;
            state.servers = selected_mail(state, data).map(|(_, mail)| ServerSettings::of(mail));
            Action::None
        }
        Message::ServerHost(server, host) => {
            edit_servers(state, |dialog| dialog.server_mut(server).host = host);
            Action::None
        }
        Message::ServerPort(server, port) => {
            edit_servers(state, |dialog| dialog.server_mut(server).port = port);
            Action::None
        }
        Message::ServerSecurity(server, security) => {
            edit_servers(state, |dialog| dialog.server_mut(server).set_security(server, security));
            Action::None
        }
        Message::CaldavEdited(address) => {
            edit_servers(state, |dialog| dialog.caldav = address);
            Action::None
        }
        Message::ServersPasswordEdited(password) => {
            edit_servers(state, |dialog| dialog.password = password);
            Action::None
        }
        Message::SubmitServers => submit_servers(state, data),
        Message::CloseServers => {
            state.servers = None;
            Action::None
        }
    }
}

/// The open page's mail account with its [`MailView`]; `None` on every other page.
fn selected_mail<'a>(state: &State, data: &'a Data) -> Option<(&'a AccountInfo, &'a MailView)> {
    let account = selected(state, data)?;
    Some((account, account.mail.as_ref()?))
}

/// The open page is a mail account without a calendar address.
fn open_mail_lacks_caldav(state: &State, data: &Data) -> bool {
    selected_mail(state, data).is_some_and(|(_, mail)| mail.caldav.is_none())
}

/// The shell reloaded `data` (task M3.5): the sender name and signature of the open mail account
/// follow changes from outside (an agent, the API, another window) as long as nothing is typed
/// into them; what the person typed stays until they save it.
pub fn loaded(state: &mut State, data: &Data) {
    let Some(fresh) = selected(state, data).and_then(MailDraft::of) else { return };
    match state.mail.as_mut().filter(|draft| draft.account == fresh.account) {
        // Typed into: it stays, and once it is what is stored (saved), it follows again.
        Some(draft) if !draft.holds(&draft.stored) => {
            if draft.holds(&fresh.stored) {
                draft.stored = fresh.stored;
            }
        }
        // Untouched and up to date: the cursor stays where it is.
        Some(draft) if draft.stored == fresh.stored => {}
        _ => state.mail = Some(fresh),
    }
}

/// Sends the change `make` finds for the open mail account, if there is one.
fn with_mail(state: &State, data: &Data, make: impl FnOnce(&MailView) -> Option<MailChange>) -> Action {
    let Some((account, mail)) = selected_mail(state, data) else { return Action::None };
    make(mail)
        .filter(|change| !change.is_empty())
        .map_or(Action::None, |change| Action::Send(change.request(&account.id)))
}

/// The draft of the open mail account; made from the stored values when it is missing.
fn draft<'s>(state: &'s mut State, data: &Data) -> Option<&'s mut MailDraft> {
    let account = selected(state, data)?;
    if state.mail.as_ref().is_none_or(|draft| draft.account != account.id) {
        state.mail = MailDraft::of(account);
    }
    state.mail.as_mut()
}

/// Whether the draft differs from what is stored.
fn is_dirty(draft: &MailDraft, mail: &MailView) -> bool {
    !draft.changes(mail).is_empty()
}

/// "Save": what the draft changed, in one `UpdateMailAccount`.
fn save_mail(state: &State, data: &Data) -> Action {
    let Some((account, mail)) = selected_mail(state, data) else { return Action::None };
    let Some(draft) = state.mail.as_ref().filter(|draft| draft.account == account.id) else { return Action::None };
    let change = draft.changes(mail);
    if change.is_empty() { Action::None } else { Action::Send(change.request(&account.id)) }
}

/// Changes the open "Server settings" dialog and clears the refusal of the last try: it was about
/// what stood there before.
fn edit_servers(state: &mut State, change: impl FnOnce(&mut ServerSettings)) {
    if let Some(dialog) = state.servers.as_mut() {
        change(dialog);
        dialog.error = None;
    }
}

/// The user name the account signs in with: the one the core reports, else the address (an
/// account whose password is missing has no connection to ask).
fn login_name(account: &AccountInfo, mail: &MailView) -> String {
    match &account.state {
        AccountState::Connected { login_name, .. } => login_name.clone(),
        _ => mail.address.clone(),
    }
}

/// Sends the servers as typed with the password, which checks them (`ConnectMail` for an account
/// that exists keeps its settings and, with an empty sender name, its sender name). The calendar
/// address goes along when one is typed (an empty field keeps the stored one); the service the
/// dialog was opened for is switched on with it (task M3.5). The dialog keeps no copy of the
/// password.
fn submit_servers(state: &mut State, data: &Data) -> Action {
    let Some((account, mail)) = selected_mail(state, data) else { return Action::None };
    let Some(dialog) = state.servers.as_mut().filter(|dialog| dialog.can_send()) else { return Action::None };
    let (Some(imap), Some(smtp)) = (dialog.imap.endpoint(), dialog.smtp.endpoint()) else { return Action::None };
    dialog.sending = true;
    dialog.error = None;
    let services = mail.settings.services;
    Action::Send(Request::ConnectMail {
        address: mail.address.clone(),
        sender_name: String::new(),
        imap,
        smtp,
        login_name: login_name(account, mail),
        password: std::mem::take(&mut dialog.password),
        calendars: services.calendars || dialog.adding == Some(MailService::Calendars),
        tasks: services.tasks || dialog.adding == Some(MailService::Tasks),
        caldav: dialog.caldav_address(),
    })
}

/// The server and user name of a connected CalDAV account: what "Change password…" signs in at
/// again.
fn caldav_connection(account: &AccountInfo) -> Option<(&str, &str)> {
    match &account.state {
        AccountState::Connected { server, login_name, .. } if account.kind == "caldav" => {
            Some((server.as_str(), login_name.as_str()))
        }
        _ => None,
    }
}

/// Whether the account has a password to change: a connected CalDAV account, or a mail account
/// (also one whose password is missing: that is how it gets one again).
fn can_change_password(account: &AccountInfo) -> bool {
    account.mail.is_some() || caldav_connection(account).is_some()
}

/// Sends the new password: `ConnectCalDav` for a CalDAV account's own server and user, or
/// `ConnectMail` for a mail account's own servers and user. The core takes both as "replace the
/// password" (the services chosen then are ignored, hidden calendars stay, and an empty sender
/// name keeps the stored one). The dialog keeps no copy of the password.
fn submit_password(state: &mut State, data: &Data) -> Action {
    let Some(account) = selected(state, data) else { return Action::None };
    let Some(change) = state.password.as_mut().filter(|change| !change.sending && !change.password.is_blank()) else {
        return Action::None;
    };
    let Some(request) = password_request(account, std::mem::take(&mut change.password)) else { return Action::None };
    change.sending = true;
    change.error = None;
    Action::Send(request)
}

/// The request that signs `account` in again with `password`.
fn password_request(account: &AccountInfo, password: Secret) -> Option<Request> {
    if let Some(mail) = &account.mail {
        return Some(Request::ConnectMail {
            address: mail.address.clone(),
            sender_name: String::new(),
            imap: mail.imap.clone(),
            smtp: mail.smtp.clone(),
            login_name: login_name(account, mail),
            password,
            calendars: mail.settings.services.calendars,
            tasks: mail.settings.services.tasks,
            caldav: mail.caldav.clone(),
        });
    }
    let (server, login_name) = caldav_connection(account)?;
    Some(Request::ConnectCalDav {
        address: account.label.clone(),
        server: Some(server.to_string()),
        login_name: login_name.to_string(),
        password,
        calendars: true,
        tasks: true,
    })
}

/// The answer to a request the shell sent: the add-account dialog, the password dialog and the
/// server dialog take the answers to their own requests ([`wizard::answered`]). Returns whether
/// one did; then the shell shows no notice.
pub fn answered(state: &mut State, result: &Result<Response, String>) -> bool {
    if let Some(wizard) = state.wizard.as_mut() {
        return wizard::answered(wizard, result);
    }
    if let Some(change) = state.password.as_mut().filter(|change| change.sending) {
        match refusal(result) {
            None => state.password = None,
            Some(message) => {
                change.sending = false;
                change.error = Some(message);
            }
        }
        return true;
    }
    if let Some(dialog) = state.servers.as_mut().filter(|dialog| dialog.sending) {
        match refusal(result) {
            None => state.servers = None,
            Some(message) => {
                dialog.sending = false;
                dialog.error = Some(message);
            }
        }
        return true;
    }
    false
}

/// What went wrong with a request that signs an account in: `None` when the account connected.
fn refusal(result: &Result<Response, String>) -> Option<String> {
    match result {
        Ok(Response::Connected { .. } | Response::MailConnected { .. }) => None,
        Ok(Response::Error { message, .. }) | Err(message) => Some(message.clone()),
        Ok(_) => Some(tr("Torro Workplace did not answer as expected. Try again.").to_string()),
    }
}

pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let now = Utc::now();
    match selected(state, data) {
        Some(account) => detail(state, account, data, now),
        None => list(data, now),
    }
}

/// The page's dialog over the whole window: the add-account dialog, else the password dialog, else
/// the server dialog, else the "Remove this account?" confirmation; `None` while none is open.
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    if let Some(wizard) = &state.wizard {
        let content = wizard::view(wizard, data).map(Message::Wizard);
        return Some(dialog(content, size::FORM_MAX, Message::Wizard(wizard::Message::Close)));
    }
    if let Some(change) = &state.password {
        let account = selected(state, data)?;
        return Some(password_dialog(change, account));
    }
    if let Some(dialog) = &state.servers {
        let (account, mail) = selected_mail(state, data)?;
        return Some(servers_dialog(dialog, account, mail));
    }
    if !state.confirm_remove {
        return None;
    }
    // With several accounts the dialog says which one goes.
    let account = selected(state, data)?;
    Some(confirm(
        tr("Remove this account?"),
        trf(
            "This removes {account} from Torro Workplace. Your data stays on the server.",
            &[("account", &account.label)],
        ),
        tr("Remove account"),
        Message::Remove,
        Message::CancelRemove,
    ))
}

/// "Change password": who it is for, the masked field, a refusal under it, "Cancel" and the primary
/// button right. A click next to the dialog cancels.
fn password_dialog<'a>(change: &'a PasswordChange, account: &'a AccountInfo) -> Element<'a, Message> {
    let can_send = !change.sending && !change.password.is_blank();
    let input = wizard::field("", change.password.expose())
        .secure(true)
        .on_input(|password| Message::PasswordEdited(Secret::new(password)))
        .on_submit_maybe(can_send.then_some(Message::SubmitPassword));
    let mut body = column![
        typo::text(typo::TITLE, tr("Change password")),
        typo::text(typo::BODY, trf("Enter the new password for {account}.", &[("account", &account.label)]))
            .style(theme::text_2),
        widgets::group(tr("Password"), vec![row_frame(input, false)], None),
    ]
    .spacing(space::L);
    if let Some(error) = &change.error {
        body = body.push(
            typo::text(typo::BODY, error.as_str())
                .style(|t: &Theme| text::Style { color: Some(palette(t).error_text) }),
        );
    }
    let buttons = row![
        blank::horizontal(),
        widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(Message::ClosePassword)),
        widgets::text_button(
            None,
            tr("Change password"),
            style::button::primary,
            can_send.then_some(Message::SubmitPassword),
        ),
    ]
    .spacing(space::S)
    .align_y(Alignment::Center);
    dialog(body.push(buttons), widgets::POPOVER_WIDTH, Message::ClosePassword)
}

/// The header bar of the page: "Add account" on the list; on an account's detail its label with
/// a back button to the list.
pub fn header<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Header<'a, Message> {
    if let Some(account) = selected(state, data) {
        return Header { sub_page: Some((account.label.clone(), Message::Back)), actions: Vec::new() };
    }
    let add = match class {
        SizeClass::Wide | SizeClass::Medium => {
            widgets::text_button(Some(Icon::Plus), tr("Add account"), style::button::primary, Some(Message::AddAccount))
        }
        SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => {
            widgets::compact_button(Icon::Plus, tr("Add account"), style::button::primary, Some(Message::AddAccount))
        }
    };
    Header { sub_page: None, actions: vec![add] }
}

// ----- Pure parts (unit-tested) -------------------------------------------------------------

/// The accounts with a detail page. An empty id is a sign-in in progress ([`pending`]).
pub fn listed(accounts: &[AccountInfo]) -> impl Iterator<Item = &AccountInfo> {
    accounts.iter().filter(|account| !account.id.is_empty())
}

/// The sign-ins in progress: entries without an id. The list shows them as a "Signing in…" row;
/// there is no account yet to open or remove.
pub fn pending(accounts: &[AccountInfo]) -> impl Iterator<Item = &AccountInfo> {
    accounts.iter().filter(|account| account.id.is_empty())
}

/// The name of a sign-in in progress: the server without scheme and trailing slash.
fn pending_name(account: &AccountInfo) -> &str {
    let label = account.label.as_str();
    label.split_once("://").map_or(label, |(_, rest)| rest).trim_end_matches('/')
}

/// The provider's display name; unknown providers show their id.
pub fn provider_name(kind: &str) -> String {
    match kind {
        "nextcloud" => tr("Nextcloud").to_string(),
        "caldav" => tr("CalDAV").to_string(),
        "mail" => tr("Mail").to_string(),
        "github" => tr("GitHub").to_string(),
        other => other.to_string(),
    }
}

/// Word and tone of an account state. Status is never shown by colour alone: the word goes next
/// to the dot of the tone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusLine {
    pub text: String,
    pub tone: Tone,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Ok,
    /// Waiting for the user or for the server.
    Attention,
    Error,
}

impl Tone {
    /// The dot that goes with the word.
    fn status(self) -> Status {
        match self {
            Tone::Ok => Status::Ok,
            Tone::Attention => Status::Attention,
            Tone::Error => Status::Error,
        }
    }
}

pub fn status_line(state: &AccountState) -> StatusLine {
    match state {
        AccountState::NotConnected => StatusLine { text: tr("Needs sign-in").to_string(), tone: Tone::Attention },
        AccountState::SigningIn { .. } => StatusLine { text: tr("Signing in…").to_string(), tone: Tone::Attention },
        AccountState::Connected { problem: Some(problem), .. } => {
            StatusLine { text: trf("Problem: {problem}", &[("problem", problem)]), tone: Tone::Error }
        }
        AccountState::Connected { problem: None, .. } => {
            StatusLine { text: tr("Connected").to_string(), tone: Tone::Ok }
        }
    }
}

/// The line under an account's name in the list: its status, and for a healthy connection when it
/// last synced ("Connected · Last synced 5 min ago").
pub fn account_detail(state: &AccountState, now: DateTime<Utc>) -> String {
    let status = status_line(state).text;
    match state {
        AccountState::Connected { last_sync, problem: None, .. } => {
            format!("{status} · {}", last_synced_text(*last_sync, now))
        }
        _ => status,
    }
}

/// The providers of `accounts`, each once, in the order they first appear: one group per provider.
fn providers<'a>(accounts: &[&'a AccountInfo]) -> Vec<&'a str> {
    let mut kinds: Vec<&str> = Vec::new();
    for account in accounts {
        if !kinds.contains(&account.kind.as_str()) {
            kinds.push(&account.kind);
        }
    }
    kinds
}

/// "Calendar", "Task list · Read-only": the line under a collection's name.
fn collection_detail(collection: &Collection) -> String {
    let kind = kind_name(collection.kind);
    if collection.writable { kind.to_string() } else { format!("{kind} · {}", tr("Read-only")) }
}

/// "just now", "5 min ago", "3 h ago", "yesterday", "4 days ago". A time in the future
/// (clock skew) counts as just now.
pub fn relative_time(then: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let seconds = (now - then).num_seconds().max(0);
    let minutes = seconds / 60;
    let hours = minutes / 60;
    let days = hours / 24;
    if minutes < 1 {
        tr("just now").to_string()
    } else if minutes < 60 {
        trf("{n} min ago", &[("n", &minutes)])
    } else if hours < 24 {
        trf("{n} h ago", &[("n", &hours)])
    } else if days == 1 {
        tr("yesterday").to_string()
    } else {
        trf("{n} days ago", &[("n", &days)])
    }
}

/// "Last synced 5 min ago", or "Not synced yet" when the account never synced.
pub fn last_synced_text(last_sync: Option<DateTime<Utc>>, now: DateTime<Utc>) -> String {
    match last_sync {
        Some(at) => trf("Last synced {when}", &[("when", &relative_time(at, now))]),
        None => tr("Not synced yet").to_string(),
    }
}

/// Whether the apps show a collection: everything is shown unless it is hidden.
pub fn is_shown(settings: &Settings, collection: &str) -> bool {
    !settings.hidden_collections.iter().any(|hidden| hidden == collection)
}

fn kind_name(kind: CollectionKind) -> &'static str {
    match kind {
        CollectionKind::Events => tr("Calendar"),
        CollectionKind::Todos => tr("Task list"),
        CollectionKind::Both => tr("Both"),
    }
}

/// The account whose detail page is open, if it is still listed.
fn selected<'a>(state: &State, data: &'a Data) -> Option<&'a AccountInfo> {
    let id = state.selected.as_deref()?;
    listed(&data.accounts).find(|account| account.id == id)
}

// ----- Views --------------------------------------------------------------------------------

fn list<'a>(data: &'a Data, now: DateTime<Utc>) -> Element<'a, Message> {
    // The accounts first, then the sign-ins in progress, so a new row appears below the others.
    let entries: Vec<&AccountInfo> = listed(&data.accounts).chain(pending(&data.accounts)).collect();
    if entries.is_empty() {
        return empty_state(
            tr("No accounts yet."),
            tr("Add your Nextcloud or a mail provider to see calendars and tasks in the apps."),
        );
    }
    let groups = providers(&entries).into_iter().map(|kind| {
        let rows = entries
            .iter()
            .filter(|account| account.kind == kind)
            .map(|account| if account.id.is_empty() { pending_row(account) } else { account_row(account, now) });
        widgets::group(provider_name(kind), rows.collect(), None)
    });
    column(groups).spacing(space::XL).into()
}

/// A sign-in in progress: attention dot, the server and "Signing in…". Not pressable: there is
/// no account yet.
fn pending_row<'a>(account: &'a AccountInfo) -> Element<'a, Message> {
    let line = status_line(&account.state);
    let dot = widgets::status_dot(line.tone.status());
    setting_row(Some(dot), pending_name(account), Some(line.text), Space::new())
}

/// One account in the list: status dot, label, status and last sync, and a chevron; the whole row
/// opens the detail page.
fn account_row<'a>(account: &'a AccountInfo, now: DateTime<Utc>) -> Element<'a, Message> {
    let dot = widgets::status_dot(status_line(&account.state).tone.status());
    let row = setting_row(Some(dot), account.label.as_str(), Some(account_detail(&account.state, now)), chevron());
    pressable(row, Some(Message::Open(account.id.clone())))
}

/// The name of an account and its provider, at the top of its detail page.
fn heading(account: &AccountInfo) -> Element<'_, Message> {
    let name = typo::text(typo::HEADLINE, account.label.as_str()).wrapping(Wrapping::None);
    column![
        container(name).width(Fill).clip(true),
        typo::text(typo::META, provider_name(&account.kind)).style(theme::text_2),
    ]
    .spacing(space::XS)
    .into()
}

/// "Remove account…" at the end of a detail page.
fn remove_button<'a>() -> Element<'a, Message> {
    on_axis(widgets::text_button(None, tr("Remove account…"), style::button::danger, Some(Message::AskRemove)))
}

/// The detail page: the name, then the groups for status, calendars and lists and the connection,
/// and the removal at the end. A mail account has [`mail_detail`] instead.
fn detail<'a>(state: &'a State, account: &'a AccountInfo, data: &'a Data, now: DateTime<Utc>) -> Element<'a, Message> {
    if let Some(mail) = &account.mail {
        return mail_detail(state, account, mail, now);
    }
    // A GitHub account has no calendars or lists (yet); what its token may do matters instead.
    let middle = match account.kind.as_str() {
        "github" => permissions_group(account.scopes.as_deref()),
        _ => collections_group(account, &data.settings),
    };
    column![heading(account), status_group(account, now), middle]
        .push(connection_group(account))
        .push(remove_button())
        .spacing(space::XL)
        .into()
}

// ----- A mail account's page (task M5.2) ----------------------------------------------------

/// The services of a mail account, one switch and one status row each.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MailService {
    Mail,
    Calendars,
    Tasks,
}

impl MailService {
    const ALL: [MailService; 3] = [MailService::Mail, MailService::Calendars, MailService::Tasks];

    fn name(self) -> &'static str {
        match self {
            MailService::Mail => tr("Mail"),
            MailService::Calendars => tr("Calendars"),
            MailService::Tasks => tr("Tasks"),
        }
    }

    fn summary(self) -> &'static str {
        match self {
            MailService::Mail => tr("Read and send email"),
            MailService::Calendars => tr("Appointments and events"),
            MailService::Tasks => tr("To-dos and reminders"),
        }
    }

    fn is_on(self, services: Services) -> bool {
        match self {
            MailService::Mail => services.mail,
            MailService::Calendars => services.calendars,
            MailService::Tasks => services.tasks,
        }
    }

    /// Calendars and tasks come from the CalDAV address; mail does not.
    fn needs_caldav(self) -> bool {
        self != MailService::Mail
    }
}

/// How one service stands: the dot and the line under its name – "Off", the problem, when it last
/// synced, or that it never did. Never colour alone: the dot always goes with the words.
pub fn service_status(on: bool, state: &ServiceState, now: DateTime<Utc>) -> (Status, String) {
    match (on, &state.problem, state.last_sync) {
        (false, ..) => (Status::Inactive, tr("Off").to_string()),
        (true, Some(problem), _) => (Status::Error, trf("Problem: {problem}", &[("problem", problem)])),
        (true, None, Some(_)) => (Status::Ok, last_synced_text(state.last_sync, now)),
        (true, None, None) => (Status::Attention, tr("Not synced yet").to_string()),
    }
}

/// The name of the calendar part's row: what the account syncs of the CalDAV address; `None` when
/// it syncs neither.
fn calendar_part_name(services: Services) -> Option<&'static str> {
    match (services.calendars, services.tasks) {
        (true, true) => Some(tr("Calendars and tasks")),
        (true, false) => Some(MailService::Calendars.name()),
        (false, true) => Some(MailService::Tasks.name()),
        (false, false) => None,
    }
}

/// The question that goes with a folder role the server left open.
fn role_question(role: Role) -> &'static str {
    match role {
        Role::Inbox => tr("Which folder holds new mail?"),
        Role::Drafts => tr("Which folder holds drafts?"),
        Role::Sent => tr("Which folder holds sent mail?"),
        Role::Archive => tr("Which folder holds archived mail?"),
        Role::Junk => tr("Which folder holds spam?"),
        Role::Trash => tr("Which folder holds deleted mail?"),
    }
}

/// "Show new mail in Inbox" as a choice of a pick list.
#[derive(Clone, Copy, PartialEq, Eq)]
struct NotifyChoice(Notify);

const NOTIFY_CHOICES: [NotifyChoice; 3] =
    [NotifyChoice(Notify::All), NotifyChoice(Notify::ForMeOnly), NotifyChoice(Notify::Off)];

impl fmt::Display for NotifyChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self.0 {
            Notify::All => tr("All"),
            Notify::ForMeOnly => tr("Only \"For me\""),
            Notify::Off => tr("Off"),
        })
    }
}

/// The room of the signature field: four lines of text and the padding of a field.
const SIGNATURE_LINES: f32 = 4.0;

fn signature_height() -> f32 {
    SIGNATURE_LINES * typo::line_height(typo::BODY) + style::INPUT_PADDING.top + style::INPUT_PADDING.bottom
}

/// A mail account's page: the state of each service with "Sync now", the question for a folder
/// role the server left open, sender name and signature, what goes to the inbox, the services and
/// the connection, and the removal at the end.
fn mail_detail<'a>(
    state: &'a State,
    account: &'a AccountInfo,
    mail: &'a MailView,
    now: DateTime<Utc>,
) -> Element<'a, Message> {
    let mut page = column![heading(account), mail_status_group(account, mail, now)].spacing(space::XL);
    if !mail.open_roles.is_empty() {
        page = page.push(folders_group(&mail.open_roles));
    }
    // A page without a draft (opened by hand in a test) has no sender or signature to edit.
    if let Some(draft) = state.mail.as_ref().filter(|draft| draft.account == account.id) {
        page = page.push(sender_groups(draft, mail));
    }
    page.push(inbox_group(mail))
        .push(services_group(mail))
        .push(mail_connection_group(account, mail))
        .push(remove_button())
        .into()
}

/// One row per service that is in use, each with its dot, its last sync or its problem; "Sync now"
/// at the end of the first. An account that is not signed in has one row and nothing to sync.
fn mail_status_group<'a>(account: &'a AccountInfo, mail: &'a MailView, now: DateTime<Utc>) -> Element<'a, Message> {
    if !matches!(account.state, AccountState::Connected { .. }) {
        let line = status_line(&account.state);
        let row = setting_row(Some(widgets::status_dot(line.tone.status())), line.text, None, Space::new());
        return widgets::group(tr("Status"), vec![row], None);
    }
    let services = mail.settings.services;
    let sync = || {
        widgets::text_button(Some(Icon::RefreshCw), tr("Sync now"), style::button::secondary, Some(Message::SyncNow))
    };
    let mut rows =
        vec![service_row(MailService::Mail.name(), service_status(services.mail, &mail.mail, now), Some(sync()))];
    if let Some(name) = calendar_part_name(services) {
        let state = mail.calendars.clone().unwrap_or_default();
        rows.push(service_row(name, service_status(true, &state, now), None));
    }
    widgets::group(tr("Status"), rows, None)
}

/// A service with its dot, name and the line under it, which wraps (a problem can be long), and
/// an optional button at the end.
fn service_row<'a>(
    name: &'static str,
    (status, detail): (Status, String),
    control: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    wrapped_row(Some(widgets::status_dot(status)), name, detail, control)
}

/// A row of a group like [`setting_row`], but its second line wraps instead of being cut off: a
/// problem or a pair of server names can be longer than a narrow window. Grows with the text.
fn wrapped_row<'a>(
    mark: Option<Element<'a, Message>>,
    name: impl IntoFragment<'a>,
    detail: String,
    control: Option<Element<'a, Message>>,
) -> Element<'a, Message> {
    let lines = column![typo::text(typo::BODY, name), typo::text(typo::META, detail).style(theme::text_2)];
    container(row![].push(mark).push(lines.width(Fill)).push(control).spacing(space::M).align_y(Alignment::Center))
        .width(Fill)
        .padding([space::M, space::L])
        .into()
}

/// The question for each role the server left open, with the folders it could be: one press picks.
fn folders_group(roles: &[OpenRoleView]) -> Element<'_, Message> {
    let mut rows = Vec::new();
    for open in roles {
        let question = typo::text(typo::BODY_STRONG, role_question(open.role));
        rows.push(container(question).width(Fill).padding([space::M, space::L]).into());
        for folder in &open.candidates {
            let pick = widgets::text_button(
                None,
                tr("Use this folder"),
                style::button::secondary,
                Some(Message::PickFolder { role: open.role, path: folder.path.clone() }),
            );
            rows.push(wrapped_row(None, folder.name.as_str(), folder.path.clone(), Some(pick)));
        }
    }
    let footer = tr("The server has more than one folder that could hold it. Your choice is remembered.");
    widgets::group(tr("Folders"), rows, Some(footer.to_string()))
}

/// Sender name and signature, and "Save" under them: it is on only while they differ from what is
/// stored, and Enter in the name works like it.
fn sender_groups<'a>(draft: &'a MailDraft, mail: &MailView) -> Element<'a, Message> {
    let dirty = is_dirty(draft, mail);
    let name = wizard::field("Anna Example", &draft.sender_name)
        .on_input(Message::SenderNameEdited)
        .on_submit_maybe(dirty.then_some(Message::SaveMail));
    let signature = text_editor(&draft.signature)
        .on_action(Message::SignatureEdited)
        .font(typo::BODY.font)
        .size(typo::BODY.size)
        .line_height(typo::BODY.line_height)
        .padding(style::INPUT_PADDING)
        .wrapping(Wrapping::WordOrGlyph)
        .height(signature_height())
        .style(style::text_editor);
    let save_style: widgets::ButtonStyle = if dirty { style::button::primary } else { style::button::secondary };
    let save = widgets::text_button(None, tr("Save"), save_style, dirty.then_some(Message::SaveMail));
    let groups = column![
        widgets::group(
            tr("Sender name"),
            vec![row_frame(name, false)],
            Some(tr("Shown next to your address in the mail you send. Optional.").to_string()),
        ),
        widgets::group(
            tr("Signature"),
            vec![container(signature).width(Fill).padding([space::S, space::L]).into()],
            Some(tr("Added under new mail, replies and forwards.").to_string()),
        ),
    ]
    .spacing(space::XL);
    // "Save" belongs to both groups, so it sits close under them.
    column![groups, row![blank::horizontal(), save]].spacing(space::M).into()
}

/// "Show new mail in Inbox": all, only what the inbox's rules put into "For me", or none.
fn inbox_group(mail: &MailView) -> Element<'_, Message> {
    let choice =
        pick_list(NOTIFY_CHOICES, Some(NotifyChoice(mail.settings.notify)), |choice| Message::SetNotify(choice.0))
            .padding(style::PICK_LIST_PADDING)
            .text_size(typo::CAPTION.size)
            .text_line_height(typo::CAPTION.line_height)
            .font(typo::CAPTION.font)
            .style(style::pick_list)
            .menu_style(style::pick_list_menu);
    let row = setting_row(None, tr("Show new mail in Inbox"), None, choice);
    let footer = tr("Only \"For me\" leaves out newsletters and other automatic mail.");
    widgets::group(tr("Inbox"), vec![row], Some(footer.to_string()))
}

/// The services as switches. Calendars and tasks need the account's calendar address; without
/// one their switches open "Server settings" to ask for it (task M3.5), and the footer says so.
fn services_group(mail: &MailView) -> Element<'_, Message> {
    let rows = MailService::ALL
        .into_iter()
        .map(|which| {
            let switch = toggler(which.is_on(mail.settings.services))
                .size(style::CHECK_SIZE)
                .style(style::toggler)
                .on_toggle(move |on| Message::SetService(which, on));
            setting_row(None, which.name(), Some(which.summary().to_string()), switch)
        })
        .collect();
    let footer = mail.caldav.is_none().then(|| {
        tr("Calendars and tasks need the calendar address of this account. Switching them on asks for it.").to_string()
    });
    widgets::group(tr("Services"), rows, footer)
}

/// The servers, the user name and the password; the servers and the password change in dialogs
/// that sign in again.
fn mail_connection_group<'a>(account: &'a AccountInfo, mail: &'a MailView) -> Element<'a, Message> {
    let servers = format!("{} · {}", mail.imap.host, mail.smtp.host);
    let settings =
        widgets::text_button(None, tr("Server settings…"), style::button::secondary, Some(Message::ChangeServers));
    let password =
        widgets::text_button(None, tr("Change password…"), style::button::secondary, Some(Message::ChangePassword));
    let user = typo::text(typo::BODY, login_name(account, mail)).style(theme::text_2).wrapping(Wrapping::None);
    let rows = vec![
        wrapped_row(None, tr("Mail servers"), servers, Some(settings)),
        setting_row(None, tr("User"), None, user),
        setting_row(None, tr("Password"), None, password),
    ];
    let footer = tr("The password stays in the system keychain. Agents never see it.");
    widgets::group(tr("Connection"), rows, Some(footer.to_string()))
}

/// The sentence under the title of "Server settings": what the password is for. Opened by the
/// switch of calendars or tasks, it says that these are added.
fn servers_purpose(form: &ServerSettings, account: &AccountInfo) -> String {
    let account: [(&str, &dyn fmt::Display); 1] = [("account", &account.label)];
    match form.adding {
        Some(MailService::Calendars) => {
            trf("Enter the calendar address and the password of {account} to add its calendars.", &account)
        }
        Some(MailService::Tasks) => {
            trf("Enter the calendar address and the password of {account} to add its tasks.", &account)
        }
        _ => trf("Enter the password of {account} to check the servers.", &account),
    }
}

/// "Server settings": the two mail servers as stored, the calendar address (always: calendars and
/// tasks can be added later, task M3.5) and the password they are checked with. Opened by the
/// switch of calendars or tasks, the calendar address comes first. A refusal of the servers stands
/// in the foot.
fn servers_dialog<'a>(form: &'a ServerSettings, account: &'a AccountInfo, mail: &MailView) -> Element<'a, Message> {
    let submit = form.can_send().then_some(Message::SubmitServers);
    let head = column![
        typo::text(typo::TITLE, tr("Server settings")),
        typo::text(typo::BODY, servers_purpose(form, account)).style(theme::text_2),
    ]
    .spacing(space::XS);
    let password = wizard::field("", form.password.expose())
        .secure(true)
        .on_input(|password| Message::ServersPasswordEdited(Secret::new(password)))
        .on_submit_maybe(submit.clone());
    let server = |which: Server, fields, titles| {
        server_group(
            fields,
            titles,
            move |host| Message::ServerHost(which, host),
            move |port| Message::ServerPort(which, port),
            move |security| Message::ServerSecurity(which, security),
            submit.clone(),
        )
    };
    let caldav = wizard::field("https://dav.example.org", &form.caldav)
        .on_input(Message::CaldavEdited)
        .on_submit_maybe(submit.clone());
    // Without a stored address the field may stay empty: the account stays mail only.
    let caldav_footer = (mail.caldav.is_none() && form.adding.is_none())
        .then(|| tr("Leave empty to use this account for mail only.").to_string());
    let caldav = widgets::group(tr("Calendars and tasks (CalDAV)"), vec![row_frame(caldav, false)], caldav_footer);
    let servers = [server(Server::Imap, &form.imap, imap_titles()), server(Server::Smtp, &form.smtp, smtp_titles())];
    let mut body = column![].spacing(space::XL);
    body = match form.adding {
        Some(_) => body.push(caldav).extend(servers),
        None => body.extend(servers).push(caldav),
    };
    body = body.push(widgets::group(tr("Password"), vec![row_frame(password, false)], None));
    let error: Element<'_, Message> = match &form.error {
        Some(error) => typo::text(typo::BODY, error.as_str())
            .style(|t: &Theme| text::Style { color: Some(palette(t).error_text) })
            .width(Fill)
            .into(),
        None => blank::horizontal().into(),
    };
    let foot = row![
        error,
        widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(Message::CloseServers)),
        widgets::text_button(None, tr("Save"), style::button::primary, submit),
    ]
    .spacing(space::S)
    .align_y(Alignment::Center);
    dialog(dialog_frame(head, body, foot, space::XL), size::FORM_MAX, Message::CloseServers)
}

/// A text button moved left by its own padding, so its label stands on the page's content axis
/// (the name, the groups) instead of 10 to the right of it; the hover surface reaches into the margin.
/// The approvals page uses it for "Show details" (task M3.5).
pub(super) fn on_axis<'a, M: 'a>(button: Element<'a, M>) -> Element<'a, M> {
    row![Space::new(), button].spacing(-style::BUTTON_PADDING.left).into()
}

/// The status as a dot and a word, when it last synced, and "Sync now" for a connected account.
fn status_group<'a>(account: &'a AccountInfo, now: DateTime<Utc>) -> Element<'a, Message> {
    let line = status_line(&account.state);
    let (detail, sync) = match &account.state {
        AccountState::Connected { last_sync, .. } => (
            Some(last_synced_text(*last_sync, now)),
            Some(widgets::text_button(
                Some(Icon::RefreshCw),
                tr("Sync now"),
                style::button::secondary,
                Some(Message::SyncNow),
            )),
        ),
        AccountState::NotConnected | AccountState::SigningIn { .. } => (None, None),
    };
    let row = setting_row(Some(widgets::status_dot(line.tone.status())), line.text, detail, sync);
    widgets::group(tr("Status"), vec![row], None)
}

/// The calendars and task lists with their switches; before the first sync a sentence instead
/// (an empty group would be a bare frame).
fn collections_group<'a>(account: &'a AccountInfo, settings: &Settings) -> Element<'a, Message> {
    let title = tr("Calendars and lists");
    if account.collections.is_empty() {
        let hint = tr("No calendars or task lists yet. Use Sync now to fetch them from the server.");
        return column![widgets::section_title(title), typo::text(typo::BODY, hint).style(theme::text_2)]
            .spacing(space::S)
            .into();
    }
    let rows = account.collections.iter().map(|collection| collection_row(collection, settings)).collect();
    let footer = tr("Hidden calendars and lists stay on the server; the apps just don't show them.");
    widgets::group(title, rows, Some(footer.to_string()))
}

/// One calendar or task list: colour dot, name, kind and "Read-only", and the "Show in apps" switch.
fn collection_row<'a>(collection: &'a Collection, settings: &Settings) -> Element<'a, Message> {
    let id = collection.id.clone();
    let switch = toggler(is_shown(settings, &collection.id))
        .size(style::CHECK_SIZE)
        .style(style::toggler)
        .on_toggle(move |shown| Message::SetShown { collection: id.clone(), shown });
    let dot = color_dot(collection_color(collection.color.as_deref()), space::S);
    setting_row(Some(dot), collection.name.as_str(), Some(collection_detail(collection)), switch)
}

/// Server and user, and where the app password lives. Nothing to show without a server.
fn connection_group<'a>(account: &'a AccountInfo) -> Option<Element<'a, Message>> {
    let (server, user) = match &account.state {
        AccountState::Connected { server, login_name, .. } => (server, Some(login_name)),
        AccountState::SigningIn { server, .. } => (server, None),
        AccountState::NotConnected => return None,
    };
    let mut rows = vec![value_row(tr("Server"), server)];
    rows.extend(user.map(|user| value_row(tr("User"), user)));
    if account.kind == "caldav" && user.is_some() {
        let change =
            widgets::text_button(None, tr("Change password…"), style::button::secondary, Some(Message::ChangePassword));
        let stored = tr("Stored in the system keychain").to_string();
        rows.push(setting_row(None, tr("Password"), Some(stored), change));
    }
    let stored = match account.kind.as_str() {
        "github" => Some(tr("Token stored in the system keychain")),
        "caldav" => None,
        _ => Some(tr("App password stored in the system keychain")),
    };
    let footer = user.and(stored).map(str::to_string);
    Some(widgets::group(tr("Connection"), rows, footer))
}

/// What a GitHub token allows: a row per permission with a dot and a word (never colour alone),
/// and the scopes GitHub reported. A fine-grained token does not tell, so the group says what
/// it needs instead.
fn permissions_group<'a>(scopes: Option<&[String]>) -> Element<'a, Message> {
    let Some(scopes) = scopes else {
        let row = setting_row(
            None,
            tr("Fine-grained token"),
            Some(tr("GitHub does not report its permissions; they are set per repository.").to_string()),
            Space::new(),
        );
        let footer = tr("Closing issues needs Issues: Read and write. Pull requests need Pull requests: Read.");
        return widgets::group(tr("Permissions"), vec![row], Some(footer.to_string()));
    };
    let rows = github_permissions(scopes)
        .into_iter()
        .map(|permission| {
            let (status, detail) = match permission.allowed {
                true => (Status::Ok, tr("Allowed")),
                false => (Status::Attention, permission.missing),
            };
            setting_row(Some(widgets::status_dot(status)), permission.name, Some(detail.to_string()), Space::new())
        })
        .collect();
    let footer = match scopes {
        [] => tr("The token has no scopes. Change them on GitHub, then use Sync now.").to_string(),
        scopes => {
            trf("Token scopes: {scopes}. Change them on GitHub, then use Sync now.", &[("scopes", &scopes.join(", "))])
        }
    };
    widgets::group(tr("Permissions"), rows, Some(footer))
}

/// One thing a GitHub token allows or not, with the sentence that says what is missing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Permission {
    pub name: &'static str,
    pub allowed: bool,
    /// What it takes, shown when not allowed.
    pub missing: &'static str,
}

/// What a token with the OAuth `scopes` of a classic token or the GitHub CLI allows
/// (`docs/settings.md`, "GitHub: Rechte"): private repositories with `repo`, closing issues with
/// `repo` or `public_repo` (public repositories only), projects with `read:project` or `project`.
pub fn github_permissions(scopes: &[String]) -> Vec<Permission> {
    let has = |wanted: &[&str]| scopes.iter().any(|scope| wanted.contains(&scope.as_str()));
    vec![
        Permission {
            name: tr("Private repositories"),
            allowed: has(&["repo"]),
            missing: tr("Private repositories need the repo scope"),
        },
        Permission {
            name: tr("Close issues"),
            allowed: has(&["repo", "public_repo"]),
            missing: tr("Closing issues needs write access to issues"),
        },
        Permission {
            name: tr("Projects"),
            allowed: has(&["read:project", "project"]),
            missing: tr("Projects need the read:project permission"),
        },
    ]
}

/// A row with a value at the right in `text_2`, on one line.
fn value_row<'a>(label: &'a str, value: &'a str) -> Element<'a, Message> {
    setting_row(None, label, None, typo::text(typo::BODY, value).style(theme::text_2).wrapping(Wrapping::None))
}

#[cfg(test)]
mod tests {
    use chrono::TimeDelta;
    use torro_core::account::Secret;
    use torro_mail::Endpoint;

    use super::*;

    fn connected(last_sync: Option<DateTime<Utc>>, problem: Option<&str>) -> AccountState {
        AccountState::Connected {
            server: "https://cloud.example.org".into(),
            login_name: "anna".into(),
            last_sync,
            problem: problem.map(str::to_string),
        }
    }

    fn account(id: &str, state: AccountState) -> AccountInfo {
        AccountInfo {
            id: id.into(),
            kind: "nextcloud".into(),
            label: "anna@cloud.example.org".into(),
            state,
            collections: vec![],
            scopes: None,
            mail: None,
        }
    }

    #[test]
    fn relative_time_text() {
        let now = Utc::now();
        assert_eq!(relative_time(now, now), tr("just now"));
        assert_eq!(relative_time(now - TimeDelta::seconds(30), now), tr("just now"));
        assert_eq!(relative_time(now + TimeDelta::seconds(30), now), tr("just now"));
        assert_eq!(relative_time(now - TimeDelta::minutes(5), now), trf("{n} min ago", &[("n", &5)]));
        assert_eq!(relative_time(now - TimeDelta::hours(3), now), trf("{n} h ago", &[("n", &3)]));
        assert_eq!(relative_time(now - TimeDelta::hours(25), now), tr("yesterday"));
        assert_eq!(relative_time(now - TimeDelta::days(4), now), trf("{n} days ago", &[("n", &4)]));
    }

    #[test]
    fn last_synced_sentence() {
        let now = Utc::now();
        assert_eq!(last_synced_text(None, now), tr("Not synced yet"));
        let five_minutes = trf("{n} min ago", &[("n", &5)]);
        assert_eq!(
            last_synced_text(Some(now - TimeDelta::minutes(5)), now),
            trf("Last synced {when}", &[("when", &five_minutes)])
        );
    }

    #[test]
    fn status_text_per_state() {
        let connected_line = status_line(&connected(None, None));
        assert_eq!(connected_line.text, tr("Connected"));
        assert_eq!(connected_line.tone, Tone::Ok);
        assert_eq!(connected_line.tone.status(), Status::Ok);

        let problem = status_line(&connected(None, Some("Server unreachable")));
        assert_eq!(problem.text, trf("Problem: {problem}", &[("problem", &"Server unreachable")]));
        assert_eq!(problem.tone, Tone::Error);
        assert_eq!(problem.tone.status(), Status::Error);

        let signing_in = status_line(&AccountState::SigningIn {
            server: "https://cloud.example.org".into(),
            login_url: "https://cloud.example.org/login".into(),
        });
        assert_eq!(signing_in.text, tr("Signing in…"));
        assert_eq!(signing_in.tone, Tone::Attention);

        let not_connected = status_line(&AccountState::NotConnected);
        assert_eq!(not_connected.text, tr("Needs sign-in"));
        assert_eq!(not_connected.tone, Tone::Attention);
    }

    #[test]
    fn the_list_row_says_status_and_last_sync() {
        let now = Utc::now();
        let five_minutes = last_synced_text(Some(now - TimeDelta::minutes(5)), now);
        let healthy = connected(Some(now - TimeDelta::minutes(5)), None);
        assert_eq!(account_detail(&healthy, now), format!("{} · {five_minutes}", tr("Connected")));
        let broken = connected(Some(now), Some("Server unreachable"));
        assert_eq!(account_detail(&broken, now), status_line(&broken).text, "a problem needs the whole line");
        assert_eq!(account_detail(&AccountState::NotConnected, now), tr("Needs sign-in"));
    }

    #[test]
    fn one_group_per_provider_in_order_of_appearance() {
        let mut github = account("github:anna", connected(None, None));
        github.kind = "github".into();
        let (a, b) = (account("nextcloud:a", connected(None, None)), account("nextcloud:b", connected(None, None)));
        assert_eq!(providers(&[&a, &github, &b]), ["nextcloud", "github"]);
    }

    #[test]
    fn collections_say_kind_and_read_only() {
        let mut collection = Collection {
            id: "work".into(),
            name: "Work".into(),
            kind: CollectionKind::Events,
            color: None,
            writable: true,
            source: None,
        };
        assert_eq!(collection_detail(&collection), tr("Calendar"));
        collection.writable = false;
        assert_eq!(collection_detail(&collection), format!("{} · {}", tr("Calendar"), tr("Read-only")));
    }

    #[test]
    fn dialogs_are_window_layers() {
        let data = Data { accounts: vec![account("nextcloud:anna", connected(None, None))], ..Data::default() };
        let mut state = State::default();
        assert!(overlay(&state, &data).is_none());
        update(&mut state, Message::AskRemove, &data);
        assert!(overlay(&state, &data).is_none(), "nothing to remove without an open detail page");
        update(&mut state, Message::Open("nextcloud:anna".into()), &data);
        update(&mut state, Message::AskRemove, &data);
        assert!(overlay(&state, &data).is_some());
        update(&mut state, Message::CancelRemove, &data);
        update(&mut state, Message::AddAccount, &data);
        assert!(overlay(&state, &data).is_some());
        assert!(matches!(escape(&state), Some(Message::Wizard(wizard::Message::Close))));
    }

    #[test]
    fn pages_build_for_list_and_detail() {
        let now = Utc::now();
        let mut anna = account("nextcloud:anna@cloud.example.org", connected(Some(now), None));
        anna.collections = vec![Collection {
            id: "work".into(),
            name: "Work".into(),
            kind: CollectionKind::Todos,
            color: Some("#5B8CFF".into()),
            writable: false,
            source: None,
        }];
        let data =
            Data { accounts: vec![anna, account("nextcloud:ben", AccountState::NotConnected)], ..Data::default() };
        let mut state = State::default();
        drop(view(&state, &data));
        update(&mut state, Message::Open("nextcloud:anna@cloud.example.org".into()), &data);
        drop(view(&state, &data));
        drop(view(&State::default(), &Data::default()));
    }

    fn two_accounts() -> Data {
        Data {
            accounts: vec![
                account("nextcloud:anna@work.example.org", connected(None, None)),
                account("nextcloud:anna@home.example.org", connected(None, None)),
            ],
            ..Data::default()
        }
    }

    #[test]
    fn removing_acts_on_the_open_account_only() {
        let data = two_accounts();
        let mut state = State::default();
        update(&mut state, Message::Open("nextcloud:anna@home.example.org".into()), &data);
        update(&mut state, Message::AskRemove, &data);
        assert!(state.confirm_remove);
        let action = update(&mut state, Message::Remove, &data);
        assert!(
            matches!(action, Action::Send(Request::RemoveAccount { ref id }) if id == "nextcloud:anna@home.example.org"),
            "{action:?}"
        );
        assert!(state.selected.is_none() && !state.confirm_remove, "back on the list");
        let again = update(&mut state, Message::Remove, &data);
        assert!(matches!(again, Action::None), "no account open: nothing to remove");
    }

    #[test]
    fn adding_is_possible_with_accounts() {
        let data = two_accounts();
        let state = State::default();
        for class in [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow, SizeClass::Mini] {
            assert_eq!(header(&state, &data, class).actions.len(), 1, "{class:?} offers Add account");
        }
    }

    #[test]
    fn a_sign_in_in_progress_is_a_row_not_an_account() {
        let signing_in = AccountState::SigningIn {
            server: "https://cloud.example.org/".into(),
            login_url: "https://cloud.example.org/login".into(),
        };
        let mut waiting = account("", signing_in);
        waiting.label = "https://cloud.example.org/".into();
        let mut data = two_accounts();
        data.accounts.push(waiting.clone());
        assert_eq!(listed(&data.accounts).count(), 2);
        assert_eq!(pending(&data.accounts).count(), 1);
        assert_eq!(pending_name(&waiting), "cloud.example.org");
        assert_eq!(status_line(&waiting.state).text, tr("Signing in…"));
        drop(view(&State::default(), &data));
        // Only a sign-in: a row instead of the empty state.
        drop(view(&State::default(), &Data { accounts: vec![waiting], ..Data::default() }));
    }

    #[test]
    fn provider_names() {
        assert_eq!(provider_name("nextcloud"), tr("Nextcloud"));
        assert_eq!(provider_name("github"), tr("GitHub"));
        assert_eq!(provider_name("mail"), tr("Mail"), "a mail account is not shown as the raw kind");
        assert_eq!(provider_name("slack"), "slack");
    }

    #[test]
    fn github_permissions_follow_the_scopes() {
        let allowed = |scopes: &[&str]| -> Vec<bool> {
            let scopes: Vec<String> = scopes.iter().map(|s| s.to_string()).collect();
            github_permissions(&scopes).iter().map(|p| p.allowed).collect()
        };
        assert_eq!(allowed(&[]), [false, false, false]);
        assert_eq!(allowed(&["repo", "read:org"]), [true, true, false]);
        assert_eq!(allowed(&["public_repo"]), [false, true, false]);
        assert_eq!(allowed(&["repo", "read:project"]), [true, true, true]);
        let close = &github_permissions(&[])[1];
        assert_eq!(close.missing, tr("Closing issues needs write access to issues"));
    }

    #[test]
    fn the_github_detail_page_builds_with_and_without_scopes() {
        let mut github = account("github:mahype", connected(Some(Utc::now()), None));
        github.kind = "github".into();
        for scopes in [Some(vec!["repo".to_string()]), Some(vec![]), None] {
            github.scopes = scopes;
            let data = Data { accounts: vec![github.clone()], ..Data::default() };
            let state = State { selected: Some(github.id.clone()), ..State::default() };
            drop(view(&state, &data));
        }
    }

    #[test]
    fn the_dialog_takes_only_its_own_answers() {
        let mut state = State::default();
        assert!(!answered(&mut state, &Err("no template".into())), "no dialog: the shell shows it");
        let ask = update(&mut state, Message::AddAccount, &Data::default());
        assert!(matches!(ask, Action::Send(Request::TorroMailAccounts)), "it opens asking for TorroMail's accounts");
        assert!(answered(&mut state, &Ok(Response::TorroMailAccounts { accounts: Vec::new() })));
        assert!(!answered(&mut state, &Err("no template".into())), "the dialog waits for nothing else");
    }

    #[test]
    fn lists_only_accounts_with_an_id() {
        let accounts = vec![
            account("", AccountState::NotConnected),
            account("nextcloud:anna@cloud.example.org", connected(None, None)),
        ];
        let ids: Vec<&str> = listed(&accounts).map(|account| account.id.as_str()).collect();
        assert_eq!(ids, ["nextcloud:anna@cloud.example.org"]);
    }

    #[test]
    fn a_collection_is_shown_unless_hidden() {
        let base = Settings { hidden_collections: vec!["a".into()], ..Settings::default() };
        assert!(!is_shown(&base, "a"));
        assert!(is_shown(&base, "b"));
    }

    #[test]
    fn the_switch_sends_only_its_collection() {
        let data = Data::default();
        for shown in [true, false] {
            let message = Message::SetShown { collection: "a".into(), shown };
            let action = update(&mut State::default(), message, &data);
            assert!(
                matches!(action, Action::Send(Request::SetCollectionShown { ref collection, shown: s })
                    if collection == "a" && s == shown),
                "{action:?}"
            );
        }
    }

    // ----- A mail provider's account (task M7.2) -------------------------------------------------

    const CALDAV: &str = "caldav:anna@mailbox.org@dav.mailbox.org";

    fn caldav_data() -> Data {
        let mut anna = account(
            CALDAV,
            AccountState::Connected {
                server: "https://dav.mailbox.org/".into(),
                login_name: "anna@mailbox.org".into(),
                last_sync: None,
                problem: None,
            },
        );
        anna.kind = "caldav".into();
        anna.label = "anna@mailbox.org".into();
        Data { accounts: vec![anna], ..Data::default() }
    }

    #[test]
    fn a_caldav_account_is_called_caldav() {
        assert_eq!(provider_name("caldav"), tr("CalDAV"));
        let data = caldav_data();
        let mut state = State::default();
        drop(view(&state, &data));
        update(&mut state, Message::Open(CALDAV.into()), &data);
        drop(view(&state, &data));
    }

    #[test]
    fn change_password_asks_first_and_sends_the_same_server_and_user() {
        let data = caldav_data();
        let mut state = State::default();
        update(&mut state, Message::ChangePassword, &data);
        assert!(state.password.is_none(), "no account open");
        update(&mut state, Message::Open(CALDAV.into()), &data);
        update(&mut state, Message::ChangePassword, &data);
        assert!(state.password.is_some() && overlay(&state, &data).is_some());
        assert!(matches!(escape(&state), Some(Message::ClosePassword)));

        assert!(matches!(update(&mut state, Message::SubmitPassword, &data), Action::None), "no password yet");
        update(&mut state, Message::PasswordEdited(Secret::new("  ")), &data);
        assert!(matches!(update(&mut state, Message::SubmitPassword, &data), Action::None), "blank");
        update(&mut state, Message::PasswordEdited(Secret::new("new-secret")), &data);
        let action = update(&mut state, Message::SubmitPassword, &data);
        match &action {
            Action::Send(Request::ConnectCalDav { address, server, login_name, password, .. }) => {
                assert_eq!(address, "anna@mailbox.org");
                assert_eq!(server.as_deref(), Some("https://dav.mailbox.org/"));
                assert_eq!(login_name, "anna@mailbox.org");
                assert_eq!(password.expose(), "new-secret");
            }
            other => panic!("expected ConnectCalDav, got {other:?}"),
        }
        assert!(!format!("{action:?}").contains("new-secret") && !format!("{state:?}").contains("new-secret"));
        assert!(
            matches!(update(&mut state, Message::SubmitPassword, &data), Action::None),
            "once; the dialog keeps no copy to send twice"
        );
    }

    #[test]
    fn the_password_dialog_takes_its_answer_and_shows_a_refusal() {
        let data = caldav_data();
        let mut state = State::default();
        update(&mut state, Message::Open(CALDAV.into()), &data);
        update(&mut state, Message::ChangePassword, &data);
        assert!(!answered(&mut state, &Err("anything".into())), "it waits for nothing yet");

        update(&mut state, Message::PasswordEdited(Secret::new("wrong")), &data);
        update(&mut state, Message::SubmitPassword, &data);
        assert!(answered(&mut state, &Err(tr("Password was refused").into())));
        let change = state.password.as_ref().expect("the dialog stays open");
        assert_eq!((change.error.as_deref(), change.sending), (Some(tr("Password was refused")), false));
        drop(overlay(&state, &data));

        update(&mut state, Message::PasswordEdited(Secret::new("right")), &data);
        assert!(state.password.as_ref().is_some_and(|change| change.error.is_none()), "an edit clears it");
        update(&mut state, Message::SubmitPassword, &data);
        let connected = Response::Connected { account: data.accounts[0].clone(), calendars: true, tasks: true };
        assert!(answered(&mut state, &Ok(connected)));
        assert!(state.password.is_none(), "done: the dialog closes");
    }

    #[test]
    fn only_a_connected_caldav_account_has_a_password_to_change() {
        let mut data = two_accounts();
        let mut state = State::default();
        update(&mut state, Message::Open("nextcloud:anna@work.example.org".into()), &data);
        update(&mut state, Message::ChangePassword, &data);
        assert!(state.password.is_none(), "Nextcloud signs in in the browser");
        data.accounts.extend(caldav_data().accounts);
        update(&mut state, Message::Open(CALDAV.into()), &data);
        update(&mut state, Message::ChangePassword, &data);
        assert!(state.password.is_some());
        update(&mut state, Message::Back, &data);
        assert!(state.password.is_none(), "leaving the page closes it");
    }

    // ----- A mail account's page (task M5.2) -----------------------------------------------------

    /// The demo's mail accounts: the first with calendars, a signature and an open role, the second
    /// mail only with a problem.
    fn mail_data() -> Data {
        let mut data = super::super::launch::demo_data();
        data.accounts.retain(|account| account.kind == "mail");
        data
    }

    const MAILBOX: &str = "mail:anna@mailbox.org";

    fn open_mail(id: &str) -> (State, Data) {
        let data = mail_data();
        let mut state = State::default();
        update(&mut state, Message::Open(id.into()), &data);
        (state, data)
    }

    fn sent(action: Action) -> Request {
        match action {
            Action::Send(request) => request,
            other => panic!("expected a request, got {other:?}"),
        }
    }

    #[test]
    fn the_mail_page_builds_in_every_state() {
        for id in [MAILBOX, "mail:anna@work.example.net"] {
            let (mut state, data) = open_mail(id);
            drop(view(&state, &data));
            update(&mut state, Message::ChangePassword, &data);
            assert!(overlay(&state, &data).is_some());
            update(&mut state, Message::ChangeServers, &data);
            assert!(state.password.is_none() && overlay(&state, &data).is_some(), "one dialog at a time");
            assert!(matches!(escape(&state), Some(Message::CloseServers)));
        }
        // Not signed in: one status row, and the password can still be given.
        let (mut state, mut data) = open_mail(MAILBOX);
        data.accounts[0].state = AccountState::NotConnected;
        drop(view(&state, &data));
        update(&mut state, Message::ChangePassword, &data);
        assert!(state.password.is_some());
    }

    #[test]
    fn saving_sends_only_what_the_draft_changes() {
        let (mut state, data) = open_mail(MAILBOX);
        assert!(matches!(update(&mut state, Message::SaveMail, &data), Action::None), "nothing changed");
        update(&mut state, Message::SenderNameEdited("Anna Beispiel".into()), &data);
        let request = sent(update(&mut state, Message::SaveMail, &data));
        assert!(
            matches!(&request, Request::UpdateMailAccount { id, sender_name: Some(name), signature: None,
                notify: None, services: None, folder_roles: None } if id == MAILBOX && name == "Anna Beispiel"),
            "{request:?}"
        );
        // Emptying the signature removes it: the empty text is sent.
        update(&mut state, Message::SenderNameEdited("Anna Example".into()), &data);
        let draft = state.mail.as_mut().expect("a draft");
        draft.signature = text_editor::Content::with_text("");
        let request = sent(update(&mut state, Message::SaveMail, &data));
        assert!(
            matches!(&request, Request::UpdateMailAccount { sender_name: None, signature: Some(text), .. }
                if text.is_empty()),
            "{request:?}"
        );
        // Typing the stored values again is no change.
        let draft = state.mail.as_mut().expect("a draft");
        draft.signature = text_editor::Content::with_text("Anna Example\r\nExample GmbH\n");
        assert!(matches!(update(&mut state, Message::SaveMail, &data), Action::None));
    }

    #[test]
    fn a_draft_follows_the_account_and_never_lands_on_another() {
        let (mut state, data) = open_mail(MAILBOX);
        update(&mut state, Message::SenderNameEdited("Changed".into()), &data);
        update(&mut state, Message::Open("mail:anna@work.example.net".into()), &data);
        let draft = state.mail.as_ref().expect("a draft");
        assert_eq!((draft.account.as_str(), draft.sender_name.as_str()), ("mail:anna@work.example.net", ""));
        update(&mut state, Message::Back, &data);
        assert!(state.mail.is_none() && state.servers.is_none());
    }

    #[test]
    fn notify_and_services_send_one_setting_each() {
        let (mut state, data) = open_mail(MAILBOX);
        let request = sent(update(&mut state, Message::SetNotify(Notify::Off), &data));
        assert!(
            matches!(
                &request,
                Request::UpdateMailAccount {
                    notify: Some(Notify::Off),
                    sender_name: None,
                    signature: None,
                    services: None,
                    folder_roles: None,
                    ..
                }
            ),
            "{request:?}"
        );
        assert!(
            matches!(update(&mut state, Message::SetNotify(Notify::ForMeOnly), &data), Action::None),
            "as stored: nothing to send"
        );
        let request = sent(update(&mut state, Message::SetService(MailService::Tasks, false), &data));
        assert!(
            matches!(&request, Request::UpdateMailAccount { services: Some(services), notify: None, .. }
                if *services == Services { mail: true, calendars: true, tasks: false }),
            "{request:?}"
        );
        assert!(matches!(update(&mut state, Message::SetService(MailService::Mail, true), &data), Action::None));
    }

    #[test]
    fn only_an_offered_folder_is_picked() {
        let (mut state, data) = open_mail(MAILBOX);
        let pick = |role, path: &str| Message::PickFolder { role, path: path.into() };
        let request = sent(update(&mut state, pick(Role::Sent, "INBOX.Sent Items"), &data));
        assert!(
            matches!(&request, Request::UpdateMailAccount { folder_roles: Some(roles), .. }
                if roles.get(&Role::Sent).map(String::as_str) == Some("INBOX.Sent Items") && roles.len() == 1),
            "{request:?}"
        );
        assert!(matches!(update(&mut state, pick(Role::Sent, "INBOX.Other"), &data), Action::None), "not a candidate");
        assert!(matches!(update(&mut state, pick(Role::Trash, "INBOX.Sent"), &data), Action::None), "not an open role");
    }

    #[test]
    fn the_folder_question_names_the_role() {
        assert_eq!(role_question(Role::Sent), tr("Which folder holds sent mail?"));
        let roles = [Role::Inbox, Role::Drafts, Role::Sent, Role::Archive, Role::Junk, Role::Trash];
        let questions: std::collections::HashSet<&str> = roles.into_iter().map(role_question).collect();
        assert_eq!(questions.len(), roles.len(), "one question per role");
        let (state, mut data) = open_mail(MAILBOX);
        drop(view(&state, &data));
        // The question only stands while a role is open.
        data.accounts[0].mail.as_mut().expect("mail").open_roles.clear();
        drop(view(&state, &data));
    }

    #[test]
    fn change_password_of_a_mail_account_connects_again_and_keeps_the_rest() {
        let (mut state, data) = open_mail(MAILBOX);
        update(&mut state, Message::ChangePassword, &data);
        assert!(matches!(update(&mut state, Message::SubmitPassword, &data), Action::None), "no password yet");
        update(&mut state, Message::PasswordEdited(Secret::new("new-secret")), &data);
        let action = update(&mut state, Message::SubmitPassword, &data);
        assert!(!format!("{action:?}").contains("new-secret") && !format!("{state:?}").contains("new-secret"));
        match sent(action) {
            Request::ConnectMail {
                address,
                sender_name,
                imap,
                smtp,
                login_name,
                password,
                calendars,
                tasks,
                caldav,
            } => {
                assert_eq!(address, "anna@mailbox.org");
                assert!(sender_name.is_empty(), "empty keeps the stored name");
                assert_eq!((imap.host.as_str(), smtp.host.as_str()), ("imap.mailbox.org", "smtp.mailbox.org"));
                assert_eq!(login_name, "anna@mailbox.org");
                assert_eq!(password.expose(), "new-secret");
                assert!(calendars && tasks);
                assert_eq!(caldav.as_deref(), Some("https://dav.mailbox.org/"));
            }
            other => panic!("expected ConnectMail, got {other:?}"),
        }
        // The dialog closes with the account's answer.
        let done = Response::MailConnected {
            account: data.accounts[0].clone(),
            calendars: true,
            tasks: true,
            open_roles: Vec::new(),
        };
        assert!(answered(&mut state, &Ok(done)));
        assert!(state.password.is_none());
    }

    #[test]
    fn server_settings_send_the_edited_servers_with_the_password() {
        let (mut state, data) = open_mail(MAILBOX);
        update(&mut state, Message::ChangeServers, &data);
        assert!(matches!(update(&mut state, Message::SubmitServers, &data), Action::None), "no password yet");
        update(&mut state, Message::ServersPasswordEdited(Secret::new("pw")), &data);
        update(&mut state, Message::ServerHost(Server::Imap, " mail.example.org ".into()), &data);
        // STARTTLS brings the usual port along, a port typed by hand stays.
        update(&mut state, Message::ServerSecurity(Server::Imap, Security::StartTls), &data);
        update(&mut state, Message::ServerPort(Server::Smtp, "2525".into()), &data);
        update(&mut state, Message::ServerSecurity(Server::Smtp, Security::Tls), &data);
        update(&mut state, Message::CaldavEdited(" https://dav.example.org ".into()), &data);
        let action = update(&mut state, Message::SubmitServers, &data);
        assert!(!format!("{action:?}").contains("\"pw\"") && !format!("{state:?}").contains("\"pw\""));
        match sent(action) {
            Request::ConnectMail { imap, smtp, password, caldav, sender_name, login_name, .. } => {
                assert_eq!(imap, Endpoint { host: "mail.example.org".into(), port: 143, security: Security::StartTls });
                assert_eq!(smtp, Endpoint { host: "smtp.mailbox.org".into(), port: 2525, security: Security::Tls });
                assert_eq!(password.expose(), "pw");
                assert_eq!(caldav.as_deref(), Some("https://dav.example.org"));
                assert!(sender_name.is_empty());
                assert_eq!(login_name, "anna@mailbox.org");
            }
            other => panic!("expected ConnectMail, got {other:?}"),
        }
        assert!(matches!(update(&mut state, Message::SubmitServers, &data), Action::None), "once");
        // A refusal stays under the dialog; an edit clears it.
        assert!(answered(&mut state, &Err(tr("Password was refused").into())));
        let dialog = state.servers.as_ref().expect("the dialog stays");
        assert_eq!((dialog.error.as_deref(), dialog.sending), (Some(tr("Password was refused")), false));
        update(&mut state, Message::ServerHost(Server::Smtp, "smtp.example.org".into()), &data);
        assert!(state.servers.as_ref().is_some_and(|dialog| dialog.error.is_none()));
        drop(overlay(&state, &data));
    }

    #[test]
    fn server_settings_need_valid_servers_and_a_password() {
        let (mut state, data) = open_mail(MAILBOX);
        update(&mut state, Message::ChangeServers, &data);
        update(&mut state, Message::ServersPasswordEdited(Secret::new("pw")), &data);
        for (server, port) in [(Server::Imap, "0"), (Server::Imap, "abc"), (Server::Smtp, "")] {
            update(&mut state, Message::ServerPort(server, port.into()), &data);
            assert!(matches!(update(&mut state, Message::SubmitServers, &data), Action::None), "{port:?}");
            update(&mut state, Message::ServerPort(server, "993".into()), &data);
        }
        update(&mut state, Message::ServerHost(Server::Imap, "  ".into()), &data);
        assert!(matches!(update(&mut state, Message::SubmitServers, &data), Action::None), "no host");
        // A mail-only account has an empty CalDAV field (task M3.5) and sends none while it stays
        // empty, nor switches anything on.
        let mut state = State::default();
        update(&mut state, Message::Open("mail:anna@work.example.net".into()), &data);
        update(&mut state, Message::ChangeServers, &data);
        assert!(state.servers.as_ref().is_some_and(|dialog| dialog.caldav.is_empty() && dialog.adding.is_none()));
        drop(overlay(&state, &data));
        update(&mut state, Message::ServersPasswordEdited(Secret::new("pw")), &data);
        let request = sent(update(&mut state, Message::SubmitServers, &data));
        assert!(
            matches!(request, Request::ConnectMail { caldav: None, calendars: false, tasks: false, .. }),
            "{request:?}"
        );
    }

    /// Task M3.5: without a calendar address, switching calendars or tasks on opens "Server
    /// settings" for it instead of sending anything; saving sends the address with that service
    /// switched on, and only once an address is typed.
    #[test]
    fn a_switch_without_a_calendar_address_asks_for_it() {
        let work = "mail:anna@work.example.net";
        let (mut state, data) = open_mail(work);
        let off = update(&mut state, Message::SetService(MailService::Tasks, false), &data);
        assert!(matches!(off, Action::None) && state.servers.is_none(), "already off: nothing to do");
        let on = update(&mut state, Message::SetService(MailService::Tasks, true), &data);
        assert!(matches!(on, Action::None), "nothing is sent yet");
        assert_eq!(state.servers.as_ref().and_then(|dialog| dialog.adding), Some(MailService::Tasks));
        drop(overlay(&state, &data));
        update(&mut state, Message::ServersPasswordEdited(Secret::new("pw")), &data);
        assert!(matches!(update(&mut state, Message::SubmitServers, &data), Action::None), "no address yet");
        update(&mut state, Message::CaldavEdited(" https://dav.example.net/ ".into()), &data);
        match sent(update(&mut state, Message::SubmitServers, &data)) {
            Request::ConnectMail { calendars, tasks, caldav, .. } => {
                assert_eq!((calendars, tasks), (false, true), "the switch that was pressed");
                assert_eq!(caldav.as_deref(), Some("https://dav.example.net/"));
            }
            other => panic!("expected ConnectMail, got {other:?}"),
        }
        // With an address the switch changes the setting right away.
        let (mut state, data) = open_mail(MAILBOX);
        let request = sent(update(&mut state, Message::SetService(MailService::Calendars, false), &data));
        assert!(matches!(request, Request::UpdateMailAccount { services: Some(_), .. }), "{request:?}");
        assert!(state.servers.is_none());
    }

    /// Task M3.5: the sender name and signature follow a change from outside while nothing is
    /// typed into them; what the person typed stays, and follows again once it is saved.
    #[test]
    fn the_page_takes_changes_from_outside_while_nothing_is_typed() {
        let (mut state, mut data) = open_mail(MAILBOX);
        let renamed = |data: &mut Data, name: &str| {
            data.accounts[0].mail.as_mut().expect("mail").sender_name = name.into();
        };
        renamed(&mut data, "Anna from elsewhere");
        loaded(&mut state, &data);
        assert_eq!(state.mail.as_ref().map(|draft| draft.sender_name.as_str()), Some("Anna from elsewhere"));

        update(&mut state, Message::SenderNameEdited("Anna typed".into()), &data);
        renamed(&mut data, "Anna again from elsewhere");
        loaded(&mut state, &data);
        assert_eq!(state.mail.as_ref().map(|draft| draft.sender_name.as_str()), Some("Anna typed"), "typed stays");

        // Saved: the reload brings what was typed back, and from then on it follows again.
        renamed(&mut data, "Anna typed");
        loaded(&mut state, &data);
        renamed(&mut data, "Anna, later");
        loaded(&mut state, &data);
        assert_eq!(state.mail.as_ref().map(|draft| draft.sender_name.as_str()), Some("Anna, later"));
    }

    #[test]
    fn each_service_says_when_it_synced_and_what_went_wrong() {
        let now = Utc::now();
        let synced = ServiceState { last_sync: Some(now - TimeDelta::minutes(5)), problem: None };
        let (status, line) = service_status(true, &synced, now);
        assert_eq!((status, line), (Status::Ok, last_synced_text(synced.last_sync, now)));
        let broken = ServiceState { last_sync: synced.last_sync, problem: Some("Password was refused".into()) };
        let (status, line) = service_status(true, &broken, now);
        assert_eq!(status, Status::Error);
        assert_eq!(line, trf("Problem: {problem}", &[("problem", &"Password was refused")]));
        let (status, line) = service_status(true, &ServiceState::default(), now);
        assert_eq!((status, line.as_str()), (Status::Attention, tr("Not synced yet")));
        let (status, line) = service_status(false, &broken, now);
        assert_eq!((status, line.as_str()), (Status::Inactive, tr("Off")), "off wins over an old problem");
    }

    #[test]
    fn the_calendar_part_is_named_after_what_it_syncs() {
        let services = |calendars, tasks| Services { mail: true, calendars, tasks };
        assert_eq!(calendar_part_name(services(true, true)), Some(tr("Calendars and tasks")));
        assert_eq!(calendar_part_name(services(true, false)), Some(tr("Calendars")));
        assert_eq!(calendar_part_name(services(false, true)), Some(tr("Tasks")));
        assert_eq!(calendar_part_name(services(false, false)), None);
    }

    #[test]
    fn the_inbox_choices_read_as_the_task_says() {
        let labels: Vec<String> = NOTIFY_CHOICES.iter().map(ToString::to_string).collect();
        assert_eq!(labels, [tr("All"), tr("Only \"For me\""), tr("Off")]);
    }

    #[test]
    fn a_mail_account_builds_with_and_without_a_calendar_address() {
        let (state, mut data) = open_mail("mail:anna@work.example.net");
        assert!(data.accounts[1].mail.as_ref().is_some_and(|mail| mail.caldav.is_none()));
        drop(view(&state, &data));
        data.accounts[1].mail.as_mut().expect("mail").caldav = Some("https://dav.example.net/".into());
        drop(view(&state, &data));
    }
}
