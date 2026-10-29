//! The sections behind the gear of the inbox's mail window. Account setup, reading and writing,
//! attachments, and automatic categorization each have one place.
//!
//! ```text
//! Accounts        identity and connection, offline content, folders and backup per account
//! Reading         display, default sender, compose format and formatted signature
//! Attachments     default click, Downloads folder and save targets (task M12.7, [`attachments`])
//! Categorization  rules before model sorting, plus editable categories
//! Filters         saved searches that move, mark or file new mail in the inbox (issue #339, [`filters`])
//! ```
//!
//! Passwords stay in the keychain; the page only sends a newly typed secret to the connection
//! command. Notification Hub source choices remain in [`crate::inbox::sources`].

use iced::Element;

use torro_core::i18n::tr;
use torro_gui::icon::Icon;

use crate::{Action, Data, Page, Section};

pub mod attachments;
mod backup;
mod editor;
pub mod filters;

/// The sections, in the order of the list. [`Section::id`] is what `--settings <id>` takes.
pub const SECTIONS: [Section; 5] = [
    // Accounts is first: the gear lands on who the mail comes from.
    Section { id: ACCOUNTS_SECTION, icon: Icon::Mail, label: "Accounts" },
    Section { id: "reading", icon: Icon::MailOpen, label: "Reading & writing" },
    // What a click on an attachment does and where it is saved (task M12.7, issue #290); "Manage
    // save targets…" in the menu of an attachment opens it ([`ATTACHMENTS_SECTION`]).
    Section { id: ATTACHMENTS_SECTION, icon: Icon::Paperclip, label: "Attachments" },
    // What incoming mail is sorted into (task E12.10, `docs/kategorien.md`).
    Section { id: "categorization", icon: Icon::Tag, label: "Categorization" },
    // Saved searches with what to do with new mail (issue #339); "Create filter…" at a mail and
    // at the search field open a new one here ([`new_filter`]).
    Section { id: FILTERS_SECTION, icon: Icon::Filter, label: "Filters" },
];

/// The id of the section "Attachments": what `--settings attachments` takes and where "Manage save
/// targets…" leads.
pub const ATTACHMENTS_SECTION: &str = "attachments";

/// The id of the section "Filters" (issue #339): what `--settings filters` takes and where "Create
/// filter…" leads.
pub const FILTERS_SECTION: &str = "filters";

/// The message that opens a new filter's page in "Filters" with `query` and `account` filled in
/// (`None`: every account) and asks what it finds – what torroinbox sends after opening the
/// section for "Create filter…" (`from:<address>`) and "Create filter from search…".
pub fn new_filter(query: String, account: Option<String>) -> Message {
    Message::Filters(filters::Message::New { query, account })
}

/// The id of the section "Accounts", whose account page has a menu of its own (task DS.6).
pub const ACCOUNTS_SECTION: &str = "accounts";

/// What only the mail window can do with these sections ([`crate::Action::App`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppAction {
    /// "Change…" at the Downloads folder or "Choose…" at a save target's folder (Attachments): the
    /// window opens the system's folder chooser and, once a folder is chosen, hands it back with
    /// [`folder_chosen`] as a message of the page; cancelled, nothing. Only a person sets a path
    /// (point 9 of "Was nie nach außen geht"), which is why only the window's chooser does.
    PickFolder(FolderFor),
}

/// What a folder from the chooser is for ([`AppAction::PickFolder`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderFor {
    /// The Downloads folder of "Save to Downloads": the page sends it as `mail_downloads_folder`.
    Downloads,
    /// The folder of the save target in the open editor: it goes into the editor's field.
    Target,
}

/// The message that hands the folder `path` picked for `purpose` back to the section "Attachments"
/// (after [`AppAction::PickFolder`]).
pub fn folder_chosen(purpose: FolderFor, path: std::path::PathBuf) -> Message {
    Message::Attachments(attachments::Message::FolderChosen(purpose, path))
}

/// What one load of these sections needs: the suite-wide settings (Reading) and every account with
/// its mail settings (Accounts).
pub fn wanted() -> Vec<torro_ipc::Request> {
    vec![
        torro_ipc::Request::Settings,
        torro_ipc::Request::Accounts,
        // "Attachments" (task M12.7): the save targets and the Downloads folder.
        torro_ipc::Request::ListSaveTargets { id: None },
        torro_ipc::Request::MailAccountStats { account: None },
        // "Categorization" (task E12.10): what is assigned and how far the backlog has come.
        torro_ipc::Request::AiAssignments,
        torro_ipc::Request::GetCategorization,
        torro_ipc::Request::PreviewCategoryJob { scope: torro_ipc::inbox::CategoryJobScope::default() },
        torro_ipc::Request::GetCategoryJob { id: None },
        torro_ipc::Request::PreviewClearCategories,
        // The catalogue (task E15.2): what the categories are called and where they stand now.
        torro_ipc::Request::ListCategories { account: None },
        // The sorting rules (task E15.5), listed and edited under Categorization.
        torro_ipc::Request::ListSortRules,
        // "Filters" (issue #339): the filters, and the folders a filter can move mail into.
        torro_ipc::Request::ListMailFilters,
        torro_ipc::Request::MailFolders { account: None },
    ]
}

/// The state of every section that keeps some; the app owns one of these next to its host.
#[derive(Debug, Default)]
pub struct State {
    /// Accounts: which account's page is open and what is typed into it. Reading keeps nothing.
    pub accounts: accounts::State,
    pub reading: reading::State,
    /// Categorization: the page of a category or a sorting rule that is open (task E15.5).
    pub categorization: categorization::State,
    pub storage: storage::State,
    /// Attachments: the editor of a save target while it is open (task M12.7).
    pub attachments: attachments::State,
    /// Filters: the open filter's page and the question of "Apply" (issue #339).
    pub filters: filters::State,
}

/// What the sections report; the app wraps it in its own message.
#[derive(Debug, Clone)]
pub enum Message {
    Accounts(accounts::Message),
    Reading(reading::Message),
    Categorization(categorization::Message),
    Storage(storage::Message),
    Attachments(attachments::Message),
    Filters(filters::Message),
}

/// What a section's widget reported, and what the host is to do about it.
pub fn update(state: &mut State, message: Message, data: &Data) -> Action<AppAction> {
    match message {
        Message::Accounts(accounts::Message::Storage(message)) => {
            let Some(account) = accounts::current_account(&state.accounts) else { return Action::None };
            storage::select_account(&mut state.storage, account);
            storage::update(&mut state.storage, message, data)
        }
        Message::Attachments(message) => attachments::update(&mut state.attachments, message, data),
        Message::Accounts(message) => {
            let action = accounts::update(&mut state.accounts, message, data);
            if let Some(account) = accounts::current_account(&state.accounts) {
                storage::select_account(&mut state.storage, account);
            }
            action
        }
        Message::Reading(message) => reading::update(&mut state.reading, message, data),
        Message::Categorization(message) => categorization::update(&mut state.categorization, message, data),
        // "Show account" opens another section's page, so it is taken before the section sees it.
        Message::Storage(storage::Message::ShowAccount(id)) => {
            accounts::show(&mut state.accounts, &id, data);
            Action::Open(ACCOUNTS_SECTION)
        }
        Message::Storage(message) => storage::update(&mut state.storage, message, data),
        Message::Filters(message) => filters::update(&mut state.filters, message, data),
    }
}

/// The page of the section with this id, as the host takes it. An id no section has shows
/// Accounts, the section the gear lands on; the host only ever asks for one of [`SECTIONS`].
pub fn page<'a>(state: &'a State, data: &'a Data, section: &str) -> Page<'a, Message> {
    let page = match section {
        "reading" => wrap(Page::new(reading::view(&state.reading, data)), Message::Reading),
        "categorization" => wrap(categorization::page(&state.categorization, data), Message::Categorization),
        FILTERS_SECTION => wrap(filters::page(&state.filters, data), Message::Filters),
        ATTACHMENTS_SECTION => wrap(Page::new(attachments::view(&state.attachments, data)), Message::Attachments),
        _ => wrap(accounts::page_with_storage(&state.accounts, &state.storage, data), Message::Accounts),
    };
    page.max_width(1200.0)
}

/// The dialog of the section with this id over the window – the editor of a save target with its
/// open menu, or the question before it is deleted (task M12.7) – while one is open; `class` is the
/// window's size class. The window shows it as its top layer.
pub fn overlay<'a>(
    state: &'a State,
    data: &'a Data,
    section: &str,
    class: torro_gui::layout::SizeClass,
) -> Option<Element<'a, Message>> {
    match section {
        ATTACHMENTS_SECTION => {
            attachments::overlay(&state.attachments, data, class).map(|dialog| dialog.map(Message::Attachments))
        }
        ACCOUNTS_SECTION => accounts::overlay(&state.accounts).map(|menu| menu.map(Message::Accounts)),
        _ => None,
    }
}

/// What Escape means while "Attachments" shows: close its menu, its question, then its dialog
/// (task M12.7). `None` without a dialog, which lets the window close the settings.
pub fn attachments_escape(state: &State) -> Option<Message> {
    attachments::escape(&state.attachments).map(Message::Attachments)
}

/// A send of the host came back (the window calls it on every `Sent`, not on a load): what was
/// being saved is saved or failed, so the save bar of an account's page takes clicks again.
pub fn sent(state: &mut State, success: bool) {
    accounts::sent(&mut state.accounts, success);
    reading::sent(&mut state.reading);
}

/// A fresh host load initializes the signature editor from the selected mail account. A draft
/// already being typed stays in place while unrelated settings refresh.
pub fn loaded(state: &mut State, data: &Data) {
    reading::loaded(&mut state.reading, data);
    editor::loaded(&mut state.categorization.editor, data);
}

/// A send of the host came back with `result` – the data loaded after it, or why it failed –: a
/// filter's page waiting for its save closes once the filter is stored, or stays with what was typed
/// (issue #339, [`filters::sent`]). The window calls it on every `Sent`, before the host takes it.
pub fn filters_sent(state: &mut State, result: &Result<Data, String>) {
    filters::sent(&mut state.filters, result);
}

/// What Escape means while "Accounts" shows: close the "…" menu of an account's head (task DS.6).
/// `None` without one, which lets the window close the settings.
pub fn accounts_escape(state: &State) -> Option<Message> {
    accounts::escape(&state.accounts).map(Message::Accounts)
}

/// What Escape means in these sections: a page of the categories' editor goes back to its list
/// (task E15.5), asking first when something typed would go. `None` leaves it to the window,
/// which closes the settings.
pub fn escape(state: &State) -> Option<Message> {
    editor::escape(&state.categorization.editor)
        .map(|message| Message::Categorization(categorization::Message::Editor(message)))
}

/// What Escape means while "Filters" shows: a filter's page goes back to the list, asking first when
/// something typed would go (issue #339). `None` on the list, which lets the window close the
/// settings.
pub fn filters_escape(state: &State) -> Option<Message> {
    filters::escape(&state.filters).map(Message::Filters)
}

/// The answer of a one-off request: a preview of the selected sorting scope, or a filter's preview
/// or run (issue #339). A filter's answer goes to "Filters" only.
pub fn answered(state: &mut State, answer: &Result<torro_ipc::Response, String>) {
    if filters::answered(&mut state.filters, answer) {
        return;
    }
    accounts::answered(&mut state.accounts, answer);
    categorization::answered(&mut state.categorization, answer);
    storage::answered(&mut state.storage, answer);
    editor::answered(&mut state.categorization.editor, answer);
}

/// Backup answers carry their originating account, so another section's pending answer cannot
/// consume a backup job and a job remains visible after switching between mail accounts. The
/// window passes the core's errors as `Ok(Response::Error)` with their code (`not-found` for a job
/// the core no longer knows), not as `Err`.
pub fn backup_answered(state: &mut State, account: &str, answer: &Result<torro_ipc::Response, String>) {
    accounts::backup_answered(&mut state.accounts, account, answer);
}

/// The status request of the open account's running backup job, for the window's ten-second tick;
/// `None` when no job runs or a request is on its way.
pub fn poll_request(state: &mut State) -> Option<torro_ipc::Request> {
    accounts::poll_request(&mut state.accounts)
}

pub use backup::{Chooser as BackupChooser, Scene as BackupScene};

/// The system chooser `message` asks the window to open ("Choose…" next to the backup folder or the
/// archive), `None` for every other message. The sections never open a dialog; the window does and
/// answers with [`backup_chosen`].
pub fn backup_chooser(message: &Message) -> Option<BackupChooser> {
    match message {
        Message::Accounts(accounts::Message::Backup(backup::Message::Choose(chooser))) => Some(*chooser),
        _ => None,
    }
}

/// The message that hands the path picked in `chooser` back to the open account's page.
pub fn backup_chosen(chooser: BackupChooser, path: std::path::PathBuf) -> Message {
    Message::Accounts(accounts::Message::Backup(backup::Message::Chosen(chooser, path)))
}

/// Puts the demo page `scene` on `account` (`--demo --open backup…` in the inbox): it shows once the
/// account's page is opened ([`accounts::Message::Open`]), or at once if it is open.
pub fn stage_backup(state: &mut State, account: &str, scene: BackupScene) {
    accounts::stage_backup(&mut state.accounts, account, backup::State::scene(scene, account));
}

/// What the window keeps of these sections while the settings are closed: only Accounts, with each
/// account's backup page and the requests on their way, whose answers still arrive. The other
/// sections start afresh, so a preview that was on its way never stays "Calculating…".
pub fn kept(state: State) -> State {
    State { accounts: state.accounts, ..State::default() }
}

/// One section's page with its messages wrapped in [`Message`].
fn wrap<'a, M: 'a>(page: Page<'a, M>, into: fn(M) -> Message) -> Page<'a, Message> {
    Page {
        body: page.body.map(into),
        sub_page: page.sub_page.map(|(name, back)| (name, into(back))),
        actions: page.actions.into_iter().map(|action| action.map(into)).collect(),
        save_bar: page.save_bar.map(|bar| bar.map(into)),
        max_width: page.max_width,
    }
}

/// "Accounts": one row per mail account, and below it the account's own identity, connection,
/// offline content, folder and backup pages. Signature and compose format are in Reading & writing.
///
/// The sender name and the signature are typed into a draft and saved with "Save"
/// (`accounts.update_mail_account`); everything else is sent the moment it is pressed, like every
/// other settings row. The draft measures itself against the stored account of the fresh [`Data`],
/// so "Save" greys out by itself once a change is stored and what is still typed stays typed.
pub mod accounts {
    use std::collections::BTreeMap;

    use chrono::{DateTime, Utc};
    use iced::Alignment;
    use iced::Length;
    use iced::widget::{column, container, responsive, row, space as blank, text_editor};
    use torro_core::i18n::trf;
    use torro_gui::datetime::format_moment;
    use torro_gui::theme::{self, size, space};
    use torro_gui::units::{format_bytes, format_count};
    use torro_gui::widgets::{
        Align, Anchor, HeadActions, MenuItem, MenuSection, PageAction, Placement, Status, TileFace, Trail,
    };
    use torro_gui::{layout, style, typo, widgets};
    use torro_ipc::mail::{MailAccountStats, MailFolderStats};
    use torro_ipc::{AccountInfo, MailView, OpenRoleView, Request, Secret};
    use torro_mail::{Endpoint, MailFormat, Role, Security};

    use super::*;
    use crate::parts::{chevron, field, pressable, row_frame, setting_row};

    /// Section state: the open account's draft, which is also what says that its page is open –
    /// one holds the other, so the page never stands without something to type into.
    #[derive(Debug, Default)]
    pub struct State {
        /// What is typed into the open account's sender name and signature; `None` on the list of
        /// accounts.
        draft: Option<Draft>,
        /// The open tab of the account's page; every account opens on "Overview".
        tab: Tab,
        /// The head's "…" menu is open, and where its button is.
        more: bool,
        more_anchor: Anchor,
        backup: backup::State,
        backup_account: Option<String>,
        backup_others: BTreeMap<String, backup::State>,
        /// What is typed into accounts that are not open, kept while it differs from what is stored:
        /// "Back" or another account loses nothing, and the save bar is there again on return.
        drafts_others: BTreeMap<String, Draft>,
        /// "Save" sent the name and the signature and the answer has not come yet: the save bar
        /// says "Saving…" and takes no second click.
        saving: bool,
        /// One test or save of an account's server credentials, including when its page is closed.
        pending_connection: Option<String>,
        save_connection: Option<String>,
    }

    /// The parts of an account's page (task DS.6), "Overview" first.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub enum Tab {
        #[default]
        Overview,
        Writing,
        Storage,
        Folders,
        Backup,
    }

    impl Tab {
        /// The tabs in their order.
        pub const ALL: [Tab; 5] = [Tab::Overview, Tab::Writing, Tab::Storage, Tab::Folders, Tab::Backup];

        fn label(self) -> &'static str {
            match self {
                Tab::Overview => tr("Overview"),
                Tab::Writing => tr("Settings"),
                Tab::Storage => tr("Storage & search"),
                Tab::Folders => tr("Folders"),
                Tab::Backup => tr("Backup"),
            }
        }

        /// Where the tab stands in the row: what [`Message::Tab`] takes.
        pub fn index(self) -> usize {
            Self::ALL.iter().position(|tab| *tab == self).unwrap_or(0)
        }

        fn at(index: usize) -> Tab {
            Self::ALL.get(index).copied().unwrap_or_default()
        }
    }

    /// What a row of this section reports.
    #[derive(Debug, Clone)]
    pub enum Message {
        /// A mail account's row: open its page.
        Open(String),
        /// "‹ Back" from an account's page to the list.
        Back,
        /// A tab of the account's page, by its index ([`Tab::index`]).
        Tab(usize),
        /// "Sync now" in the head: every account syncs (`accounts.sync`).
        Sync,
        /// The head's "…": open or close its menu.
        More,
        /// A click beside the open "…" menu, or Escape.
        CloseMore,
        /// "Storage & index" from the "…" menu or a link of the overview.
        OpenStorage,
        /// "Server settings": the account in the control centre, where servers and passwords are.
        ServerSettings,
        /// "Discard" in the save bar: the draft holds what is stored again.
        Discard,
        /// Typing in the sender name.
        SenderName(String),
        LoginName(String),
        Password(Secret),
        ManualServer(bool),
        ServerHost(ServerKind, String),
        ServerPort(ServerKind, String),
        ServerSecurity(ServerKind, Security),
        TestConnection,
        SaveConnection,
        /// Typing in the signature.
        Signature(text_editor::Action),
        /// "Save" in the save bar: the sender name and the signature, as far as they differ from the
        /// stored ones.
        Save,
        /// "Write new mail as" was set (issue #125).
        Format(MailFormat),
        /// "Use this folder" for a role the server left open.
        PickFolder {
            role: Role,
            path: String,
        },
        /// A row of the account's groups "Backup" and "Restore".
        Backup(backup::Message),
        /// A storage choice on this account's own page.
        Storage(storage::Message),
    }

    /// The sender name and the signature of one account as they are being typed.
    #[derive(Debug)]
    struct Draft {
        account: String,
        sender_name: String,
        signature: text_editor::Content,
        connection: ConnectionDraft,
    }

    #[derive(Debug, Clone, Copy)]
    pub enum ServerKind {
        Imap,
        Smtp,
    }

    impl ServerKind {
        fn default_port(self, security: Security) -> u16 {
            match (self, security) {
                (Self::Imap, Security::Tls) => 993,
                (Self::Imap, Security::StartTls) => 143,
                (Self::Smtp, Security::Tls) => 465,
                (Self::Smtp, Security::StartTls) => 587,
            }
        }
    }

    #[derive(Debug)]
    struct ServerDraft {
        host: String,
        port: String,
        security: Security,
    }

    impl ServerDraft {
        fn of(endpoint: &Endpoint) -> Self {
            Self { host: endpoint.host.clone(), port: endpoint.port.to_string(), security: endpoint.security }
        }
        fn endpoint(&self) -> Option<Endpoint> {
            let port = self.port.trim().parse::<u16>().ok().filter(|port| *port > 0)?;
            let host = self.host.trim();
            (!host.is_empty()).then(|| Endpoint { host: host.to_string(), port, security: self.security })
        }
        fn set_security(&mut self, kind: ServerKind, security: Security) {
            if self.port.trim() == kind.default_port(self.security).to_string() {
                self.port = kind.default_port(security).to_string();
            }
            self.security = security;
        }
    }

    #[derive(Debug)]
    struct ConnectionDraft {
        login_name: String,
        password: Secret,
        imap: ServerDraft,
        smtp: ServerDraft,
        manual: bool,
        tried: bool,
        pending: bool,
        status: Option<String>,
    }

    impl ConnectionDraft {
        fn new(mail: &MailView, login_name: &str) -> Self {
            Self {
                login_name: login_name.to_string(),
                password: Secret::new(""),
                imap: ServerDraft::of(&mail.imap),
                smtp: ServerDraft::of(&mail.smtp),
                manual: false,
                tried: false,
                pending: false,
                status: None,
            }
        }
        fn server_mut(&mut self, kind: ServerKind) -> &mut ServerDraft {
            match kind {
                ServerKind::Imap => &mut self.imap,
                ServerKind::Smtp => &mut self.smtp,
            }
        }
        fn changed(&self, mail: &MailView, login_name: &str) -> bool {
            self.login_name.trim() != login_name
                || !self.password.is_blank()
                || self.imap.endpoint().as_ref() != Some(&mail.imap)
                || self.smtp.endpoint().as_ref() != Some(&mail.smtp)
        }
        fn request(&self, id: &str, save: bool) -> Option<Request> {
            Some(Request::ReconfigureMail {
                id: id.to_string(),
                imap: self.imap.endpoint()?,
                smtp: self.smtp.endpoint()?,
                login_name: self.login_name.trim().to_string(),
                password: (!self.password.is_blank()).then(|| self.password.clone()),
                save,
            })
        }
    }

    impl Draft {
        fn new(account: &str, mail: &MailView, login_name: &str) -> Draft {
            Draft {
                account: account.to_string(),
                sender_name: mail.sender_name.clone(),
                signature: text_editor::Content::with_text(mail.settings.signature.as_deref().unwrap_or_default()),
                connection: ConnectionDraft::new(mail, login_name),
            }
        }

        /// The signature as it would be sent: line breaks as `\n` whatever the platform, no white
        /// space at the end.
        fn signature_text(&self) -> String {
            self.signature.text().replace("\r\n", "\n").trim_end().to_string()
        }

        /// The sender name and the signature as they would be sent, each only where it differs
        /// from the stored account.
        fn changed(&self, mail: &MailView) -> (Option<String>, Option<String>) {
            let sender_name = self.sender_name.trim();
            let signature = self.signature_text();
            let stored_signature = mail.settings.signature.as_deref().unwrap_or_default().trim_end();
            let changed_name = (sender_name != mail.sender_name.trim()).then(|| sender_name.to_string());
            let changed_signature = (signature != stored_signature).then_some(signature);
            (changed_name, changed_signature)
        }

        /// What the save bar says changed; `None` when nothing did (then there is no save bar).
        fn summary(&self, mail: &MailView) -> Option<&'static str> {
            match self.changed(mail) {
                (Some(_), Some(_)) => Some(tr("Sender name and signature changed")),
                (Some(_), None) => Some(tr("Sender name changed")),
                (None, Some(_)) => Some(tr("Signature changed")),
                (None, None) => None,
            }
        }

        /// What changed against the stored account, as `accounts.update_mail_account` with only
        /// those fields; `None` when nothing did (then there is no save bar).
        fn changes(&self, mail: &MailView) -> Option<Request> {
            let (changed_name, changed_signature) = self.changed(mail);
            if changed_name.is_none() && changed_signature.is_none() {
                return None;
            }
            Some(Request::UpdateMailAccount {
                id: self.account.clone(),
                sender_name: changed_name,
                signature: changed_signature,
                signature_format: None,
                notify: None,
                services: None,
                folder_roles: None,
                format: None,
            })
        }
    }

    pub fn update<A>(state: &mut State, message: Message, data: &Data) -> Action<A> {
        match message {
            Message::Open(id) => {
                // Without the account in the loaded data there is nothing to show or type into;
                // the list stays, and a press after the next load opens it.
                if let Some(mail) = mail_of(data, &id) {
                    if state.backup_account.as_deref() != Some(id.as_str()) {
                        if let Some(previous) = state.backup_account.replace(id.clone()) {
                            state.backup_others.insert(previous, std::mem::take(&mut state.backup));
                        }
                        state.backup = state
                            .backup_others
                            .remove(&id)
                            .unwrap_or_else(|| backup::State::from_preferences(&mail.settings.backup));
                    }
                    keep_draft(state, data);
                    let draft = state
                        .drafts_others
                        .remove(&id)
                        .unwrap_or_else(|| Draft::new(&id, mail, login_of(data, &id, mail)));
                    state.draft = Some(draft);
                    state.tab = Tab::Overview;
                    state.more = false;
                }
                Action::None
            }
            Message::Back => {
                keep_draft(state, data);
                state.more = false;
                Action::None
            }
            // A tab only changes what the page shows; nothing goes to the daemon.
            Message::Tab(index) => {
                state.tab = Tab::at(index);
                state.more = false;
                Action::None
            }
            Message::Sync => {
                state.more = false;
                Action::send(Request::Sync)
            }
            Message::More => {
                state.more = !state.more;
                Action::None
            }
            Message::CloseMore => {
                state.more = false;
                Action::None
            }
            Message::OpenStorage => {
                state.more = false;
                state.tab = Tab::Storage;
                Action::None
            }
            Message::ServerSettings => {
                state.more = false;
                state.tab = Tab::Writing;
                Action::None
            }
            Message::Discard => {
                if let Some(draft) = &state.draft
                    && let Some(mail) = mail_of(data, &draft.account)
                {
                    state.draft = Some(Draft::new(&draft.account, mail, login_of(data, &draft.account, mail)));
                }
                Action::None
            }
            Message::SenderName(name) => {
                if let Some(draft) = &mut state.draft {
                    draft.sender_name = name;
                }
                Action::None
            }
            Message::LoginName(name) => {
                if let Some(draft) = &mut state.draft {
                    draft.connection.login_name = name;
                    draft.connection.status = None;
                }
                Action::None
            }
            Message::Password(password) => {
                if let Some(draft) = &mut state.draft {
                    draft.connection.password = password;
                    draft.connection.status = None;
                }
                Action::None
            }
            Message::ManualServer(manual) => {
                if let Some(draft) = &mut state.draft {
                    draft.connection.manual = manual;
                }
                Action::None
            }
            Message::ServerHost(kind, host) => {
                if let Some(draft) = &mut state.draft {
                    draft.connection.server_mut(kind).host = host;
                    draft.connection.status = None;
                }
                Action::None
            }
            Message::ServerPort(kind, port) => {
                if let Some(draft) = &mut state.draft {
                    draft.connection.server_mut(kind).port = port;
                    draft.connection.status = None;
                }
                Action::None
            }
            Message::ServerSecurity(kind, security) => {
                if let Some(draft) = &mut state.draft {
                    draft.connection.server_mut(kind).set_security(kind, security);
                    draft.connection.status = None;
                }
                Action::None
            }
            Message::TestConnection | Message::SaveConnection
                if state.pending_connection.is_some() || state.save_connection.is_some() =>
            {
                Action::None
            }
            Message::TestConnection | Message::SaveConnection => {
                let save = matches!(message, Message::SaveConnection);
                let Some(draft) = &mut state.draft else { return Action::None };
                draft.connection.tried = true;
                if draft.connection.login_name.trim().is_empty() {
                    draft.connection.status = Some(tr("Enter the user name.").into());
                    return Action::None;
                }
                let Some(request) = draft.connection.request(&draft.account, save) else {
                    draft.connection.status =
                        Some(tr("Enter a server name and a valid port for IMAP and SMTP.").into());
                    return Action::None;
                };
                draft.connection.status = None;
                draft.connection.pending = true;
                if save {
                    state.save_connection = Some(draft.account.clone());
                    Action::send(request)
                } else {
                    state.pending_connection = Some(draft.account.clone());
                    Action::Ask(request)
                }
            }
            Message::Signature(action) => {
                if let Some(draft) = &mut state.draft {
                    draft.signature.perform(action);
                }
                Action::None
            }
            // A second "Save" (Enter in the name) while the first is on its way sends nothing.
            Message::Save if state.saving => Action::None,
            Message::Save => {
                let Some((draft, mail)) = open_draft(state, data) else { return Action::None };
                let Some(request) = draft.changes(mail) else { return Action::None };
                state.saving = true;
                Action::send(request)
            }
            Message::Format(format) => change(state, data, |mail| {
                (format != mail.settings.format).then(|| MailChange { format: Some(format), ..MailChange::default() })
            }),
            Message::PickFolder { role, path } => change(state, data, |_| {
                Some(MailChange { folder_roles: Some(BTreeMap::from([(role, path)])), ..MailChange::default() })
            }),
            Message::Backup(message) => state
                .draft
                .as_ref()
                .map_or(Action::None, |draft| backup::update(&mut state.backup, &draft.account, message)),
            Message::Storage(_) => Action::None,
        }
    }

    /// Takes the open draft off the page, kept for its account while it holds something unsaved.
    fn keep_draft(state: &mut State, data: &Data) {
        let Some(draft) = state.draft.take() else { return };
        if mail_of(data, &draft.account).is_some_and(|mail| {
            draft.changes(mail).is_some() || draft.connection.changed(mail, login_of(data, &draft.account, mail))
        }) {
            state.drafts_others.insert(draft.account.clone(), draft);
        }
    }

    /// Opens `id`'s page at "Overview" ("Show account" in Storage & index, task DS.7); an account
    /// that is open already keeps what is typed into it.
    pub(super) fn show(state: &mut State, id: &str, data: &Data) {
        if state.draft.as_ref().is_some_and(|draft| draft.account == id) {
            state.tab = Tab::Overview;
            state.more = false;
        } else {
            let _: Action<()> = update(state, Message::Open(id.to_string()), data);
        }
    }

    /// A send came back: "Save" is done, the save bar shows what is still different.
    pub(super) fn sent(state: &mut State, success: bool) {
        state.saving = false;
        if let Some(id) = state.save_connection.take() {
            let draft = if state.draft.as_ref().is_some_and(|draft| draft.account == id) {
                state.draft.as_mut()
            } else {
                state.drafts_others.get_mut(&id)
            };
            if let Some(draft) = draft {
                draft.connection.pending = false;
                if success {
                    draft.connection.password = Secret::new("");
                    draft.connection.status = Some(tr("Connection saved and tested.").into());
                } else {
                    draft.connection.status =
                        Some(tr("The connection could not be saved. Check the account error above.").into());
                }
            }
        }
    }

    /// The answer of a backup request for `account`, to its page whether it is open or not.
    pub fn backup_answered(state: &mut State, account: &str, answer: &Result<torro_ipc::Response, String>) {
        if state.backup_account.as_deref() == Some(account) {
            backup::answered(&mut state.backup, answer);
        } else if let Some(saved) = state.backup_others.get_mut(account) {
            backup::answered(saved, answer);
        }
    }

    pub fn poll_request(state: &mut State) -> Option<Request> {
        let draft = state.draft.as_ref()?;
        backup::poll_request(&mut state.backup, &draft.account)
    }

    /// The backup folder typed or chosen on the open account's page.
    #[cfg(test)]
    pub(super) fn backup_destination(state: &State) -> Option<&str> {
        state.draft.as_ref().map(|_| state.backup.destination.as_str())
    }

    /// Replaces `account`'s backup page by `backup`, open or kept for later.
    pub(super) fn stage_backup(state: &mut State, account: &str, backup: backup::State) {
        if state.backup_account.as_deref() == Some(account) {
            state.backup = backup;
        } else {
            state.backup_others.insert(account.to_string(), backup);
        }
    }

    /// The fields of `accounts.update_mail_account` that one press changes; the others stay as they
    /// are, so two windows never overwrite each other's settings.
    #[derive(Debug, Default)]
    struct MailChange {
        format: Option<MailFormat>,
        folder_roles: Option<BTreeMap<Role, String>>,
    }

    /// `make` says what the press changes about the open account, or nothing when it would change
    /// nothing; the request names only those fields.
    fn change<A>(state: &State, data: &Data, make: impl FnOnce(&MailView) -> Option<MailChange>) -> Action<A> {
        let Some((draft, mail)) = open_draft(state, data) else { return Action::None };
        let Some(change) = make(mail) else { return Action::None };
        Action::send(Request::UpdateMailAccount {
            id: draft.account.clone(),
            sender_name: None,
            signature: None,
            signature_format: None,
            notify: None,
            services: None,
            folder_roles: change.folder_roles,
            format: change.format,
        })
    }

    /// Every mail account of the fresh data, in the order the daemon lists them.
    pub fn mail_accounts(data: &Data) -> Vec<(&AccountInfo, &MailView)> {
        data.accounts.iter().filter_map(|account| Some((account, account.mail.as_ref()?))).collect()
    }

    /// The mail account with this id, as far as the data holds it.
    fn mail_of<'a>(data: &'a Data, id: &str) -> Option<&'a MailView> {
        data.accounts.iter().find(|account| account.id == id).and_then(|account| account.mail.as_ref())
    }

    fn login_of<'a>(data: &'a Data, id: &str, mail: &'a MailView) -> &'a str {
        data.accounts
            .iter()
            .find(|account| account.id == id)
            .and_then(|account| {
                if let torro_pim::AccountState::Connected { login_name, .. } = &account.state {
                    Some(login_name.as_str())
                } else {
                    None
                }
            })
            .unwrap_or(&mail.address)
    }

    pub(super) fn answered(state: &mut State, answer: &Result<torro_ipc::Response, String>) {
        let Some(id) = state.pending_connection.as_deref() else { return };
        match answer {
            Ok(torro_ipc::Response::MailConnectionChecked { id: answered }) if answered != id => return,
            Ok(torro_ipc::Response::MailConnected { account, .. }) if account.id != id => return,
            Ok(
                torro_ipc::Response::MailConnectionChecked { .. }
                | torro_ipc::Response::MailConnected { .. }
                | torro_ipc::Response::Error { .. },
            )
            | Err(_) => {}
            _ => return,
        }
        let id = state.pending_connection.take().expect("checked above");
        let draft = if state.draft.as_ref().is_some_and(|draft| draft.account == id) {
            state.draft.as_mut()
        } else {
            state.drafts_others.get_mut(&id)
        };
        let Some(draft) = draft else { return };
        draft.connection.pending = false;
        draft.connection.status = Some(match answer {
            Ok(torro_ipc::Response::MailConnectionChecked { .. }) => tr("Connection successful.").into(),
            Ok(torro_ipc::Response::MailConnected { .. }) => {
                draft.connection.password = Secret::new("");
                tr("Connection saved and tested.").into()
            }
            Ok(torro_ipc::Response::Error { message, .. }) | Err(message) => message.clone(),
            _ => return,
        });
    }

    /// The open draft with the stored account it measures itself against; `None` while the list is
    /// shown or the account is gone.
    fn open_draft<'a>(state: &'a State, data: &'a Data) -> Option<(&'a Draft, &'a MailView)> {
        let draft = state.draft.as_ref()?;
        Some((draft, mail_of(data, &draft.account)?))
    }

    /// The section's page: the list of accounts, or the open account's page below it.
    pub fn page<'a>(state: &'a State, data: &'a Data) -> Page<'a, Message> {
        page_with_optional_storage(state, None, data)
    }

    pub(super) fn page_with_storage<'a>(
        state: &'a State,
        storage: &'a storage::State,
        data: &'a Data,
    ) -> Page<'a, Message> {
        page_with_optional_storage(state, Some(storage), data)
    }

    fn page_with_optional_storage<'a>(
        state: &'a State,
        storage: Option<&'a storage::State>,
        data: &'a Data,
    ) -> Page<'a, Message> {
        match open_draft(state, data) {
            Some((draft, mail)) => {
                let label = account_label(data, &draft.account);
                let stats = data.mail_stats.iter().find(|stats| stats.account == draft.account);
                let page =
                    Page::new(account_page(state, draft, mail, stats, storage, data)).sub_page(label, Message::Back);
                // A page that does not save itself: the bar stands while the name or the signature
                // differs from what is stored, on every tab.
                match draft.summary(mail) {
                    Some(summary) => {
                        let idle = !state.saving;
                        let bar = widgets::save_bar(
                            summary,
                            idle.then_some(Message::Discard),
                            idle.then_some(Message::Save),
                            state.saving,
                        );
                        page.save_bar(bar)
                    }
                    None => page,
                }
            }
            None => Page::new(list(data)),
        }
    }

    /// How the path names the open account: its address.
    fn account_label(data: &Data, id: &str) -> String {
        data.accounts
            .iter()
            .find(|account| account.id == id)
            .map_or_else(|| tr("Account").to_string(), |account| account.label.clone())
    }

    pub(super) fn current_account(state: &State) -> Option<&str> {
        state.draft.as_ref().map(|draft| draft.account.as_str())
    }

    /// A compact inventory per account. Every bar compares RFC822.SIZE values from the same
    /// message inventory; the database's physical size is shown only on the storage detail.
    fn list(data: &Data) -> Element<'_, Message> {
        let accounts = mail_accounts(data);
        if accounts.is_empty() {
            return widgets::empty_state(
                tr("No mail account"),
                tr("Add a mail account in Torro Workplace, under Accounts."),
            );
        }
        let now = Utc::now();
        let rows = accounts
            .into_iter()
            .map(|(account, mail)| {
                let stats = data.mail_stats.iter().find(|stats| stats.account == account.id);
                let name = mail.sender_name.trim().to_string();
                let address = account.label.clone();
                let id = account.id.clone();
                let sync = mail.mail.last_sync.or_else(|| stats.and_then(|stats| stats.last_sync));
                let sync_label = if sync_problem(mail, stats).is_some() {
                    tr("Sync problem").to_string()
                } else {
                    sync.map_or_else(|| tr("Not synced yet").to_string(), |at| format_moment(at, now))
                };
                let stats = stats.cloned();
                let entry = responsive(move |room| {
                    let inventory = inventory_content(stats.as_ref());
                    let name_col = column![
                        typo::text(typo::body_strong(), address.clone()),
                        typo::text(typo::meta(), name.clone()).style(theme::text_2),
                    ]
                    .spacing(space::xs());
                    let sync_col = typo::text(typo::meta(), sync_label.clone()).style(theme::text_2);
                    if room.width < 680.0 {
                        container(column![name_col, inventory, sync_col].spacing(space::m()))
                            .width(Length::Fill)
                            .padding([space::m(), space::l()])
                            .into()
                    } else {
                        container(
                            row![
                                name_col.width(Length::FillPortion(3)),
                                container(inventory).width(Length::FillPortion(5)),
                                sync_col.width(Length::FillPortion(2)),
                                chevron(),
                            ]
                            .spacing(space::l())
                            .align_y(Alignment::Center),
                        )
                        .width(Length::Fill)
                        .padding([space::m(), space::l()])
                        .into()
                    }
                });
                pressable(entry.into(), Some(Message::Open(id)))
            })
            .collect();
        column![widgets::group(tr("Email accounts"), rows, None), widgets::mail_coverage_legend(),]
            .spacing(space::m())
            .into()
    }

    fn inventory_content<'a>(stats: Option<&MailAccountStats>) -> Element<'a, Message> {
        let Some(stats) = stats else {
            return typo::text(typo::meta(), tr("Statistics are not available yet.")).style(theme::text_2).into();
        };
        let online = stats.known_mail_bytes.map(format_bytes);
        let local = stats.offline_mail_bytes.map(format_bytes);
        let heading = match (local, online) {
            (Some(local), Some(online)) => trf("{local} of {online} online", &[("local", &local), ("online", &online)]),
            _ => tr("Mail sizes are being collected").to_string(),
        };
        let mut content = column![
            typo::text(typo::body_strong(), heading),
            typo::text(typo::meta(), trf("{n} emails total", &[("n", &format_count(stats.messages))]))
                .style(theme::text_2),
        ]
        .spacing(space::xs());
        if let (Some(total), Some(actual), Some(planned), Some(indexed)) = (
            stats.known_mail_bytes,
            stats.offline_mail_bytes,
            stats.planned_mail_bytes,
            stats.indexed_offline_mail_bytes,
        ) {
            content = content.push(widgets::mail_coverage_bars(actual, planned, indexed, total));
        }
        content.into()
    }

    /// One account's page (task DS.6): the head with the address, how it syncs and "Sync now", the
    /// tabs, and the tab that is open.
    fn account_page<'a>(
        state: &'a State,
        draft: &'a Draft,
        mail: &'a MailView,
        stats: Option<&'a MailAccountStats>,
        storage: Option<&'a storage::State>,
        data: &'a Data,
    ) -> Element<'a, Message> {
        let now = Utc::now();
        let labels = Tab::ALL
            .iter()
            .map(|tab| {
                let count = (*tab == Tab::Folders).then(|| stats.map(|stats| stats.folders.len() as u64)).flatten();
                (tab.label().to_string(), count)
            })
            .collect();
        let body = match state.tab {
            Tab::Overview => overview(mail, stats, now),
            Tab::Writing => account_settings(draft, mail, data),
            Tab::Storage => storage.map_or_else(
                || widgets::empty_state(tr("Storage is loading"), tr("Try again shortly.")),
                |state| storage::account_view(state, data, &draft.account).map(Message::Storage),
            ),
            Tab::Folders => folders(mail, stats),
            Tab::Backup => backup::view(&state.backup, &draft.account).map(Message::Backup),
        };
        column![head(state, mail, stats, now), widgets::tabs(labels, state.tab.index(), Message::Tab), body]
            .spacing(layout::section_gap())
            .into()
    }

    /// The sync problem of the account: what the account says, else what the statistics say.
    fn sync_problem<'a>(mail: &'a MailView, stats: Option<&'a MailAccountStats>) -> Option<&'a str> {
        mail.mail.problem.as_deref().or_else(|| stats.and_then(|stats| stats.sync_problem.as_deref()))
    }

    /// The head: letter tile, address, "Name · IMAP host:port · SMTP host:port", how the last sync
    /// went, "Sync now" and the "…" with Storage & index and the server settings.
    fn head<'a>(
        state: &'a State,
        mail: &'a MailView,
        stats: Option<&'a MailAccountStats>,
        now: DateTime<Utc>,
    ) -> Element<'a, Message> {
        let mut meta = Vec::new();
        if !mail.sender_name.trim().is_empty() {
            meta.push(mail.sender_name.trim().to_string());
        }
        let server = |host: &str, port: u16| format!("{host}:{port}");
        meta.push(trf("IMAP {server}", &[("server", &server(&mail.imap.host, mail.imap.port))]));
        meta.push(trf("SMTP {server}", &[("server", &server(&mail.smtp.host, mail.smtp.port))]));
        let last_sync = mail.mail.last_sync.or_else(|| stats.and_then(|stats| stats.last_sync));
        let status = match (sync_problem(mail, stats), last_sync) {
            (Some(_), _) => Some((Status::Error, tr("Sync problem").to_string())),
            (None, Some(at)) => Some((Status::Ok, trf("Synced {when}", &[("when", &format_moment(at, now))]))),
            (None, None) => None,
        };
        let sync = PageAction::new(tr("Sync now"), Some(Message::Sync))
            .icon(Icon::RefreshCw)
            .tip(tr("Syncs all accounts, not only this one"));
        let actions = HeadActions { action: Some(sync), more: Some((state.more_anchor.clone(), Message::More)) };
        widgets::detail_head(TileFace::letter(&mail.address), mail.address.clone(), meta.join(" · "), status, actions)
    }

    /// The menu of the head's "…" while it is open.
    pub(super) fn overlay(state: &State) -> Option<Element<'_, Message>> {
        if !state.more || state.draft.is_none() {
            return None;
        }
        let items = vec![
            MenuItem::new(Icon::Archive, tr("Storage & index"), Message::OpenStorage),
            MenuItem::new(Icon::Settings, tr("Server settings"), Message::ServerSettings)
                .trail(Trail::Icon(Icon::ArrowUpRight)),
        ];
        let menu = widgets::menu(vec![MenuSection::new(items)]);
        Some(widgets::floating(&state.more_anchor, Placement::Below(Align::End), menu, Message::CloseMore))
    }

    /// Escape closes the head's "…" menu while it is open.
    pub(super) fn escape(state: &State) -> Option<Message> {
        if state.tab == Tab::Backup
            && let Some(message) = backup::escape(&state.backup)
        {
            return Some(Message::Backup(message));
        }
        state.more.then_some(Message::CloseMore)
    }

    /// The tab "Overview": the open questions, the figures, storage, mail over time, the search
    /// index and the largest folders.
    fn overview<'a>(
        mail: &'a MailView,
        stats: Option<&'a MailAccountStats>,
        _now: DateTime<Utc>,
    ) -> Element<'a, Message> {
        let mut page = column![].spacing(layout::section_gap());
        let mut questions: Vec<Element<'a, Message>> = mail
            .open_roles
            .iter()
            .map(|open| {
                let choose = PageAction::new(tr("Choose folders"), Some(Message::Tab(Tab::Folders.index())));
                widgets::question_card(Icon::Folder, role_question(open.role), tr(ROLE_DETAIL), vec![choose])
            })
            .collect();
        if let Some(problem) = sync_problem(mail, stats) {
            let settings = PageAction::new(tr("Server settings"), Some(Message::ServerSettings));
            questions.push(widgets::question_card(
                Icon::CircleAlert,
                tr("The last sync failed"),
                problem.to_string(),
                vec![settings],
            ));
        }
        if !questions.is_empty() {
            page = page.push(column(questions).spacing(size::card_gap()));
        }
        let Some(stats) = stats else { return page.push(no_statistics()).into() };
        page.push(storage_card(stats)).into()
    }

    /// What stands where the numbers would before the mail service has them.
    fn no_statistics<'a>() -> Element<'a, Message> {
        widgets::empty_state(
            tr("Statistics are not available yet."),
            tr("The mail service supplies these numbers from its local store."),
        )
    }

    /// The sentence under the question for a folder role.
    const ROLE_DETAIL: &str = "The server has more than one folder that could hold it. Your choice is remembered.";

    /// What a cell shows for a value that is not known yet.
    const UNKNOWN: &str = "–";

    /// "Storage": the server and the device as strips, the attachments under them, and the way to
    /// the page that manages them.
    fn storage_card<'a>(stats: &MailAccountStats) -> Element<'a, Message> {
        let body = column![
            inventory_content(Some(stats)),
            widgets::mail_coverage_legend(),
            typo::text(
                typo::meta(),
                trf("Local database incl. headers and index: {size}", &[("size", &format_bytes(stats.database_bytes))]),
            )
            .style(theme::text_2),
        ]
        .spacing(space::s());
        let manage =
            widgets::text_button(None, tr("Storage & search"), style::button::ghost, Some(Message::OpenStorage));
        widgets::chart_card(tr("Mail inventory"), "", body.push(row![blank::horizontal(), manage]))
    }

    /// Identity is kept beside access details. Sending preferences and the signature live in
    /// Reading & writing, where they apply to the chosen sender account.
    fn account_settings<'a>(draft: &'a Draft, mail: &'a MailView, data: &'a Data) -> Element<'a, Message> {
        let dirty = draft.changes(mail).is_some();
        let connection_changed = draft.connection.changed(mail, login_of(data, &draft.account, mail));
        let value = |label: &'static str, content: String| -> Element<'a, Message> {
            row_frame(
                column![typo::text(typo::meta(), tr(label)).style(theme::text_2), typo::text(typo::body(), content)]
                    .spacing(space::xs()),
                false,
            )
        };
        responsive(move |room| {
            let identity = widgets::group(
                tr("Account details"),
                vec![
                    row_frame(
                        column![
                            typo::text(typo::meta(), tr("Display name")).style(theme::text_2),
                            field("Anna Example", &draft.sender_name)
                                .on_input(Message::SenderName)
                                .on_submit_maybe(dirty.then_some(Message::Save)),
                        ]
                        .spacing(space::xs()),
                        false,
                    ),
                    value("Email address", mail.address.clone()),
                    row_frame(
                        column![
                            typo::text(typo::meta(), tr("Username")).style(theme::text_2),
                            field(tr("Username"), &draft.connection.login_name).on_input(Message::LoginName),
                        ]
                        .spacing(space::xs()),
                        false,
                    ),
                    row_frame(
                        column![
                            typo::text(typo::meta(), tr("Password")).style(theme::text_2),
                            field(tr("Leave empty to keep the saved password"), draft.connection.password.expose())
                                .secure(true)
                                .on_input(|typed| Message::Password(Secret::new(typed))),
                        ]
                        .spacing(space::xs()),
                        false,
                    ),
                ],
                None,
            );
            let mut server_rows: Vec<Element<'a, Message>> = vec![];
            if draft.connection.manual {
                server_rows.push(server_form(ServerKind::Imap, &draft.connection.imap));
                server_rows.push(server_form(ServerKind::Smtp, &draft.connection.smtp));
            } else {
                let server = |label: &'static str, fields: &ServerDraft| {
                    let security = match fields.security {
                        Security::Tls => "TLS",
                        Security::StartTls => "STARTTLS",
                    };
                    value(label, format!("{} · {} · {security}", fields.host, fields.port))
                };
                server_rows.push(server("IMAP server", &draft.connection.imap));
                server_rows.push(server("SMTP server", &draft.connection.smtp));
            }
            server_rows.push(row_frame(
                widgets::text_button(
                    None,
                    if draft.connection.manual { tr("Hide manual server fields") } else { tr("Edit servers manually") },
                    style::button::secondary,
                    Some(Message::ManualServer(!draft.connection.manual)),
                ),
                false,
            ));
            server_rows.push(
                responsive(move |room| {
                    let test = widgets::text_button(
                        None,
                        if draft.connection.pending { tr("Checking…") } else { tr("Test credentials") },
                        style::button::secondary,
                        (!draft.connection.pending).then_some(Message::TestConnection),
                    );
                    let save = widgets::text_button(
                        None,
                        tr("Save connection"),
                        style::button::primary,
                        (connection_changed && !draft.connection.pending).then_some(Message::SaveConnection),
                    );
                    if room.width < 440.0 {
                        container(column![test, save].spacing(space::s()))
                            .width(Length::Fill)
                            .padding([space::s(), space::l()])
                            .into()
                    } else {
                        row_frame(row![test, save].spacing(space::s()), false)
                    }
                })
                .height(Length::Shrink)
                .into(),
            );
            if let Some(status) = &draft.connection.status {
                server_rows.push(row_frame(typo::text(typo::body(), status.clone()), false));
            }
            let connection = widgets::group(tr("Connection"), server_rows, None);
            let folders = system_folders(mail, data.mail_stats.iter().find(|stats| stats.account == draft.account));
            let right = column![connection, folders].spacing(layout::section_gap());
            if room.width >= 700.0 {
                row![container(identity).width(Length::FillPortion(1)), container(right).width(Length::FillPortion(1))]
                    .spacing(space::l())
                    .align_y(Alignment::Start)
                    .into()
            } else {
                column![identity, right].spacing(layout::section_gap()).into()
            }
        })
        .height(Length::Shrink)
        .into()
    }

    /// Show the resolved IMAP roles beside the server settings. Ambiguous roles link to the
    /// existing folder picker; a missing first sync is reported instead of guessed.
    fn system_folders<'a>(mail: &'a MailView, stats: Option<&'a MailAccountStats>) -> Element<'a, Message> {
        let roles = [
            (Role::Inbox, "Inbox"),
            (Role::Drafts, "Drafts"),
            (Role::Sent, "Sent"),
            (Role::Archive, "Archive"),
            (Role::Junk, "Spam"),
            (Role::Trash, "Trash"),
        ];
        let rows = roles
            .into_iter()
            .map(|(role, label)| {
                let mapped = stats
                    .and_then(|stats| stats.folders.iter().find(|folder| folder.folder.role == Some(role)))
                    .map(|folder| folder.folder.name.as_str())
                    .or_else(|| mail.settings.folder_roles.get(&role).map(String::as_str));
                let pending = mail.open_roles.iter().any(|open| open.role == role);
                let detail = if pending { tr("Selection needed") } else { mapped.unwrap_or(tr("Not determined yet")) };
                let control = widgets::text_button(
                    None,
                    tr("Choose folders"),
                    style::button::secondary,
                    pending.then_some(Message::Tab(Tab::Folders.index())),
                );
                if pending {
                    setting_row(None, tr(label), Some(detail.to_string()), control)
                } else {
                    row_frame(
                        row![
                            typo::text(typo::body(), tr(label)),
                            blank::horizontal(),
                            widgets::ellipsized(typo::body(), detail.to_string(), theme::text_2),
                        ]
                        .spacing(space::s())
                        .align_y(Alignment::Center),
                        false,
                    )
                }
            })
            .collect();
        widgets::group(tr("System folders"), rows, None)
    }

    fn server_form<'a>(kind: ServerKind, server: &'a ServerDraft) -> Element<'a, Message> {
        let title = match kind {
            ServerKind::Imap => tr("Incoming mail (IMAP)"),
            ServerKind::Smtp => tr("Outgoing mail (SMTP)"),
        };
        let host = field(tr("Server"), &server.host)
            .on_input(move |value| Message::ServerHost(kind, value))
            .width(Length::FillPortion(3));
        let port = field(tr("Port"), &server.port)
            .on_input(move |value| Message::ServerPort(kind, value))
            .width(Length::FillPortion(1));
        widgets::group(
            title,
            vec![
                row_frame(row![host, port].spacing(space::s()), false),
                setting_row(
                    None,
                    tr("Security"),
                    None,
                    widgets::segmented(
                        [
                            ("TLS", Message::ServerSecurity(kind, Security::Tls)),
                            ("STARTTLS", Message::ServerSecurity(kind, Security::StartTls)),
                        ],
                        usize::from(server.security == Security::StartTls),
                    ),
                ),
            ],
            None,
        )
    }

    /// The tab "Folders": the roles the server left open, one press picks (task M5.2), then every
    /// folder with its numbers and a total.
    fn folders<'a>(mail: &'a MailView, stats: Option<&'a MailAccountStats>) -> Element<'a, Message> {
        let mut page = column![].spacing(layout::section_gap());
        let questions: Vec<Element<'a, Message>> = mail.open_roles.iter().map(role_card).collect();
        if !questions.is_empty() {
            page = page.push(column(questions).spacing(size::card_gap()));
        }
        let Some(stats) = stats else { return page.push(no_statistics()).into() };
        // Narrower than a form the seven columns would run into each other: then only the name,
        // the counts and the size on the server.
        let grid = iced::widget::responsive(move |room| {
            let (head, rows, total) = folder_table(stats);
            let keep = |cells: Vec<String>| -> Vec<String> {
                if room.width >= size::form_max() {
                    return cells;
                }
                cells.into_iter().enumerate().filter(|(i, _)| NARROW_COLUMNS.contains(i)).map(|(_, c)| c).collect()
            };
            widgets::data_table(keep(head), rows.into_iter().map(keep).collect(), Some(keep(total)))
        })
        .height(iced::Length::Shrink);
        let mut table = column![grid].spacing(space::s());
        if stats.folders.iter().any(|folder| folder.server_bytes.is_none()) {
            let later = tr("Sizes on the server come with the next sync.");
            table = table.push(typo::text(typo::meta(), later).style(theme::text_2));
        }
        page.push(table).into()
    }

    /// The question for a role the server left open, with one button per folder it could be.
    fn role_card(open: &OpenRoleView) -> Element<'_, Message> {
        let picks = open
            .candidates
            .iter()
            .map(|folder| {
                let pick = Message::PickFolder { role: open.role, path: folder.path.clone() };
                PageAction::new(trf("Use {folder}", &[("folder", &folder.name)]), Some(pick)).tip(folder.path.clone())
            })
            .collect();
        widgets::question_card(Icon::Folder, role_question(open.role), tr(ROLE_DETAIL), picks)
    }

    /// The columns of the folder table that a narrow page keeps: folder, mail, unread, on server.
    const NARROW_COLUMNS: [usize; 4] = [0, 1, 2, 5];

    /// The table of the tab "Folders": head, one row per folder, and the total.
    pub(super) fn folder_table(stats: &MailAccountStats) -> (Vec<String>, Vec<Vec<String>>, Vec<String>) {
        let head = [
            tr("Folder"),
            tr("Mail"),
            tr("Unread"),
            tr("Offline texts"),
            tr("Indexed"),
            tr("On the server"),
            tr("Originals"),
        ]
        .map(str::to_string)
        .to_vec();
        let size = |bytes: Option<u64>| bytes.map_or_else(|| UNKNOWN.to_string(), format_bytes);
        let rows = stats
            .folders
            .iter()
            .map(|folder| {
                vec![
                    folder.folder.name.clone(),
                    format_count(u64::from(folder.folder.total)),
                    format_count(u64::from(folder.folder.unread)),
                    format_count(folder.cached_bodies),
                    format_count(folder.indexed_full),
                    size(folder.server_bytes),
                    format_bytes(folder.original_bytes),
                ]
            })
            .collect();
        let server: Option<u64> = stats.folders.iter().map(|folder| folder.server_bytes).sum();
        let sum = |value: fn(&MailFolderStats) -> u64| stats.folders.iter().map(value).sum::<u64>();
        let total = vec![
            tr("All folders").to_string(),
            format_count(sum(|f| u64::from(f.folder.total))),
            format_count(sum(|f| u64::from(f.folder.unread))),
            format_count(sum(|f| f.cached_bodies)),
            format_count(sum(|f| f.indexed_full)),
            size(server),
            format_bytes(sum(|f| f.original_bytes)),
        ];
        (head, rows, total)
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

    #[cfg(test)]
    pub(super) mod tests {
        use super::*;
        use crate::Action as Act;

        /// A mail account as `accounts.list_accounts` answers it.
        pub(in crate::mail) fn account(address: &str, sender_name: &str, signature: Option<&str>) -> AccountInfo {
            serde_json::from_value(serde_json::json!({
                "id": format!("mail:{address}"),
                "kind": "mail",
                "label": address,
                "state": {"state": "connected", "server": "imap.example.org", "login_name": address},
                "collections": [],
                "mail": {
                    "address": address,
                    "sender_name": sender_name,
                    "imap": {"host": "imap.example.org", "port": 993, "security": "tls"},
                    "smtp": {"host": "smtp.example.org", "port": 465, "security": "tls"},
                    "settings": {"signature": signature, "format": "html"},
                    "open_roles": [{
                        "role": "archive",
                        "candidates": [
                            {"name": "Archive", "path": "INBOX.Archive"},
                            {"name": "All Mail", "path": "INBOX.All"}
                        ]
                    }],
                    "mail": {}
                }
            }))
            .expect("a mail account")
        }

        fn data() -> Data {
            Data {
                accounts: vec![account("anna@example.org", "Anna Example", Some("Kind regards\nAnna"))],
                ..Data::default()
            }
        }

        /// A row opens its account's page, "‹ Back" leads to the list again.
        #[test]
        fn a_row_opens_its_account_and_back_closes_it() {
            let (data, mut state) = (data(), State::default());
            assert!(page(&state, &data).sub_page.is_none(), "the list first");
            let _: Action<()> = update(&mut state, Message::Open("mail:anna@example.org".into()), &data);
            let opened = page(&state, &data).sub_page.map(|(name, _)| name);
            assert_eq!(opened, Some("anna@example.org".to_string()));
            let _: Action<()> = update(&mut state, Message::Back, &data);
            assert!(page(&state, &data).sub_page.is_none());
        }

        #[test]
        fn connection_can_be_tested_without_saving_and_saved_with_separate_server_fields() {
            let data = data();
            let mut state = opened(&data);
            let test: Action<()> = update(&mut state, Message::TestConnection, &data);
            assert!(matches!(test, Action::Ask(Request::ReconfigureMail { save: false, password: None, .. })));
            answered(
                &mut state,
                &Ok(torro_ipc::Response::MailConnectionChecked { id: "mail:anna@example.org".into() }),
            );
            assert!(state.pending_connection.is_none());
            let _: Action<()> = update(&mut state, Message::ManualServer(true), &data);
            let _: Action<()> =
                update(&mut state, Message::ServerHost(ServerKind::Imap, "mail.example.org".into()), &data);
            let _: Action<()> =
                update(&mut state, Message::ServerSecurity(ServerKind::Smtp, Security::StartTls), &data);
            let _: Action<()> = update(&mut state, Message::Password(Secret::new("new-secret")), &data);
            let save: Action<()> = update(&mut state, Message::SaveConnection, &data);
            let Action::Send(mut requests) = save else { panic!("edited connection must be sent") };
            let Request::ReconfigureMail { imap, smtp, password, save: true, .. } = requests.remove(0) else {
                panic!("edited connection must be sent")
            };
            assert_eq!((imap.host.as_str(), imap.port), ("mail.example.org", 993));
            assert_eq!((smtp.port, smtp.security), (587, Security::StartTls));
            assert_eq!(password, Some(Secret::new("new-secret")));
        }

        #[test]
        fn invalid_server_port_stays_on_the_page_and_a_failed_test_can_be_retried() {
            let data = data();
            let mut state = opened(&data);
            let _: Action<()> = update(&mut state, Message::ServerPort(ServerKind::Imap, "abc".into()), &data);
            let invalid: Action<()> = update(&mut state, Message::TestConnection, &data);
            assert_eq!(invalid, Action::None);
            assert!(state.pending_connection.is_none());
            let _: Action<()> = update(&mut state, Message::ServerPort(ServerKind::Imap, "143".into()), &data);
            let sent: Action<()> = update(&mut state, Message::TestConnection, &data);
            assert!(matches!(sent, Action::Ask(Request::ReconfigureMail { .. })));
            answered(&mut state, &Ok(torro_ipc::Response::Error { code: "auth".into(), message: "Bad login".into() }));
            assert_eq!(state.draft.as_ref().unwrap().connection.status.as_deref(), Some("Bad login"));
            assert!(state.pending_connection.is_none());
        }

        /// "Save" sends the sender name and the signature, each only when it differs; nothing
        /// while they hold what is stored.
        #[test]
        fn save_sends_only_what_was_changed() {
            let (data, mut state) = (data(), State::default());
            let _: Action<()> = update(&mut state, Message::Open("mail:anna@example.org".into()), &data);
            let nothing: Action<()> = update(&mut state, Message::Save, &data);
            assert_eq!(nothing, Action::None, "the draft holds what is stored");

            let _: Action<()> = update(&mut state, Message::SenderName("Anna B.".into()), &data);
            let saved: Action<()> = update(&mut state, Message::Save, &data);
            let Act::Send(requests) = saved else { panic!("expected Send") };
            assert_eq!(
                requests,
                vec![Request::UpdateMailAccount {
                    id: "mail:anna@example.org".into(),
                    sender_name: Some("Anna B.".into()),
                    signature: None,
                    signature_format: None,
                    notify: None,
                    services: None,
                    folder_roles: None,
                    format: None,
                }]
            );
        }

        /// "Write new mail as" sends only `format`, and nothing when it is already chosen.
        #[test]
        fn the_format_sets_only_its_field() {
            let (data, mut state) = (data(), State::default());
            let _: Action<()> = update(&mut state, Message::Open("mail:anna@example.org".into()), &data);
            let same: Action<()> = update(&mut state, Message::Format(MailFormat::Html), &data);
            assert_eq!(same, Action::None, "it is formatted already");
            let changed: Action<()> = update(&mut state, Message::Format(MailFormat::Plain), &data);
            assert_eq!(
                changed,
                Action::send(Request::UpdateMailAccount {
                    id: "mail:anna@example.org".into(),
                    sender_name: None,
                    signature: None,
                    signature_format: None,
                    notify: None,
                    services: None,
                    folder_roles: None,
                    format: Some(MailFormat::Plain),
                })
            );
        }

        /// "Use this folder" sends only that role with its path.
        #[test]
        fn a_folder_sets_only_its_role() {
            let (data, mut state) = (data(), State::default());
            let _: Action<()> = update(&mut state, Message::Open("mail:anna@example.org".into()), &data);
            let picked: Action<()> =
                update(&mut state, Message::PickFolder { role: Role::Archive, path: "INBOX.Archive".into() }, &data);
            assert_eq!(
                picked,
                Action::send(Request::UpdateMailAccount {
                    id: "mail:anna@example.org".into(),
                    sender_name: None,
                    signature: None,
                    signature_format: None,
                    notify: None,
                    services: None,
                    folder_roles: Some(BTreeMap::from([(Role::Archive, "INBOX.Archive".to_string())])),
                    format: None,
                })
            );
        }

        /// A press without an open account changes nothing (a window whose account was removed).
        #[test]
        fn nothing_happens_without_an_open_account() {
            let mut state = State::default();
            let data = data();
            for message in [
                Message::Save,
                Message::Format(MailFormat::Plain),
                Message::PickFolder { role: Role::Sent, path: "Sent".into() },
            ] {
                let action: Action<()> = update(&mut state, message, &data);
                assert_eq!(action, Action::None);
            }
            // An account the data does not hold opens no page and no draft.
            let _: Action<()> = update(&mut state, Message::Open("mail:gone@example.org".into()), &data);
            assert!(page(&state, &data).sub_page.is_none(), "back to the list");
        }

        /// The statistics of `anna@example.org` with one folder per `(name, total, unread, server
        /// bytes)`, and a quota of `(used, limit)` when given.
        pub(in crate::mail) fn stats(
            folders: &[(&str, u32, u32, Option<u64>)],
            quota: Option<(u64, u64)>,
        ) -> MailAccountStats {
            let folders: Vec<serde_json::Value> = folders
                .iter()
                .map(|(name, total, unread, server_bytes)| {
                    serde_json::json!({
                        "folder": {"id": format!("f:{name}"), "account": "mail:anna@example.org", "name": name,
                                   "path": name, "unread": unread, "total": total},
                        "read": total - unread, "oldest_known": null, "newest_known": null, "cached_bodies": 1,
                        "indexed_full": 1, "indexed_partial_or_unverified": 0, "text_unavailable": 0,
                        "awaiting_index": 0, "server_bytes": server_bytes
                    })
                })
                .collect();
            let quota = quota.map(|(used, limit)| {
                serde_json::json!({"used_bytes": used, "limit_bytes": limit, "checked": "2026-10-26T12:00:00Z"})
            });
            serde_json::from_value(serde_json::json!({
                "account": "mail:anna@example.org", "folders": folders, "messages": 40, "read": 30, "unread": 10,
                "oldest_known": "2020-10-05T09:00:00Z", "newest_known": "2026-10-26T08:00:00Z",
                "cached_bodies": 20, "indexed_full": 30, "indexed_partial_or_unverified": 4,
                "text_unavailable": 1, "awaiting_index": 5, "database_bytes": 1_048_576,
                "body_cache_bytes": 262_144, "search_index_bytes": 131_072, "cached_attachments": 2,
                "attachment_cache_bytes": 4096, "last_sync": "2026-10-26T08:00:00Z", "sync_problem": null,
                "quota": quota,
                "months": [{"year": 2025, "month": 11, "messages": 10, "cached_bodies": 2},
                           {"year": 2026, "month": 2, "messages": 30, "cached_bodies": 18}]
            }))
            .expect("statistics")
        }

        fn opened(data: &Data) -> State {
            let mut state = State::default();
            let _: Action<()> = update(&mut state, Message::Open("mail:anna@example.org".into()), data);
            state
        }

        /// A tab only changes what the page shows: nothing goes to the daemon, and every tab builds
        /// with and without statistics.
        #[test]
        fn a_tab_sends_nothing_and_every_tab_builds() {
            let mut with = data();
            with.mail_stats = vec![stats(&[("INBOX", 30, 8, Some(4096)), ("Sent", 10, 2, None)], None)];
            for data in [data(), with] {
                let mut state = opened(&data);
                for tab in Tab::ALL {
                    let action: Action<()> = update(&mut state, Message::Tab(tab.index()), &data);
                    assert_eq!(action, Action::None, "{tab:?}");
                    assert_eq!(state.tab, tab);
                    let _ = page(&state, &data).body;
                }
            }
        }

        /// What is typed stays when the page is left: "Back" and another account keep it, and the
        /// save bar is there again when the account is opened again; "Discard" lets it go.
        #[test]
        fn unsaved_typing_survives_leaving_the_page() {
            let mut data = data();
            data.accounts.push(account("ben@example.org", "Ben", None));
            let mut state = opened(&data);
            let _: Action<()> = update(&mut state, Message::SenderName("Anna B.".into()), &data);
            let _: Action<()> = update(&mut state, Message::Back, &data);
            assert!(page(&state, &data).sub_page.is_none(), "back on the list");
            let _: Action<()> = update(&mut state, Message::Open("mail:anna@example.org".into()), &data);
            assert!(page(&state, &data).save_bar.is_some(), "the name is still typed");
            let _: Action<()> = update(&mut state, Message::Open("mail:ben@example.org".into()), &data);
            assert!(page(&state, &data).save_bar.is_none(), "Ben has nothing typed");
            let _: Action<()> = update(&mut state, Message::Open("mail:anna@example.org".into()), &data);
            let _: Action<()> = update(&mut state, Message::Discard, &data);
            let _: Action<()> = update(&mut state, Message::Back, &data);
            let _: Action<()> = update(&mut state, Message::Open("mail:anna@example.org".into()), &data);
            assert!(page(&state, &data).save_bar.is_none(), "discarded stays discarded");
        }

        /// "Save" makes the save bar busy until the host's answer is back; a second click sends
        /// nothing.
        #[test]
        fn save_waits_for_its_answer() {
            let data = data();
            let mut state = opened(&data);
            let _: Action<()> = update(&mut state, Message::SenderName("Anna B.".into()), &data);
            let saved: Action<()> = update(&mut state, Message::Save, &data);
            assert!(matches!(saved, Act::Send(_)));
            assert!(state.saving);
            let again: Action<()> = update(&mut state, Message::Save, &data);
            assert_eq!(again, Action::None, "one save at a time");
            sent(&mut state, true);
            assert!(!state.saving);
        }

        /// "Sync now" sends exactly `accounts.sync`.
        #[test]
        fn sync_now_sends_the_sync() {
            let data = data();
            let mut state = opened(&data);
            let action: Action<()> = update(&mut state, Message::Sync, &data);
            assert_eq!(action, Action::send(Request::Sync));
        }

        /// The save bar stands only while something is changed; "Discard" sends nothing and takes it
        /// away.
        #[test]
        fn discard_sends_nothing_and_drops_the_change() {
            let data = data();
            let mut state = opened(&data);
            assert!(page(&state, &data).save_bar.is_none(), "nothing changed");
            let _: Action<()> = update(&mut state, Message::SenderName("Anna B.".into()), &data);
            assert!(page(&state, &data).save_bar.is_some());
            let action: Action<()> = update(&mut state, Message::Discard, &data);
            assert_eq!(action, Action::None);
            assert!(page(&state, &data).save_bar.is_none());
            let nothing: Action<()> = update(&mut state, Message::Save, &data);
            assert_eq!(nothing, Action::None, "the stored name is back");
        }

        /// The folder table has a total; a size the server has not told yet shows as unknown.
        #[test]
        fn the_folder_table_totals_its_columns() {
            let (head, rows, total) =
                folder_table(&stats(&[("INBOX", 30, 8, Some(4096)), ("Sent", 10, 2, None)], None));
            assert_eq!(head.len(), 7);
            assert_eq!(rows.len(), 2);
            assert_eq!(rows[1][5], UNKNOWN);
            assert_eq!(total[1], format_count(40));
            assert_eq!(total[5], UNKNOWN, "not all sizes are known");
        }

        /// The "…" menu opens and closes, and its entries lead to this account's own tabs.
        #[test]
        fn the_more_menu_leads_on() {
            let data = data();
            let mut state = opened(&data);
            assert!(overlay(&state).is_none());
            let _: Action<()> = update(&mut state, Message::More, &data);
            assert!(overlay(&state).is_some());
            assert!(matches!(escape(&state), Some(Message::CloseMore)));
            let storage: Action<()> = update(&mut state, Message::OpenStorage, &data);
            assert_eq!(storage, Action::None);
            assert_eq!(state.tab, Tab::Storage);
            assert!(overlay(&state).is_none());
            let server: Action<()> = update(&mut state, Message::ServerSettings, &data);
            assert_eq!(server, Action::None);
            assert_eq!(state.tab, Tab::Writing);
        }

        /// A storage choice from an account tab sends that account's policy only after its value
        /// is valid. Invalid dates never reach the daemon.
        #[test]
        fn storage_selection_validates_the_date_on_the_open_account() {
            use torro_ipc::mail::MailCacheMode;
            let mut data = data();
            data.mail_stats = vec![stats(&[("INBOX", 30, 8, Some(4096))], None)];
            let mut state = crate::mail::State::default();
            let _ = crate::mail::update(
                &mut state,
                crate::mail::Message::Accounts(Message::Open("mail:anna@example.org".into())),
                &data,
            );
            let choice = |message| crate::mail::Message::Accounts(Message::Storage(message));
            let _ = crate::mail::update(&mut state, choice(storage::Message::Select(MailCacheMode::SinceDate)), &data);
            let _ = crate::mail::update(&mut state, choice(storage::Message::Date("wrong".into())), &data);
            assert_eq!(crate::mail::update(&mut state, choice(storage::Message::Apply), &data), Action::None);
            let _ = crate::mail::update(&mut state, choice(storage::Message::Date("01.01.2024".into())), &data);
            let action = crate::mail::update(&mut state, choice(storage::Message::Apply), &data);
            let Action::Send(requests) = action else { panic!("date policy sent") };
            let Request::SetMailCachePolicy { policy } = &requests[0] else { panic!("policy first") };
            assert_eq!(policy.account, "mail:anna@example.org");
            assert_eq!(policy.mode, MailCacheMode::SinceDate);
            assert_eq!(policy.since_date.as_deref(), Some("2024-01-01"));
        }

        #[test]
        fn storage_saves_attachment_retention_with_the_offline_strategy() {
            use torro_ipc::mail::MailCacheMode;

            let data = data();
            let mut state = crate::mail::State::default();
            let _: Action<AppAction> = crate::mail::update(
                &mut state,
                crate::mail::Message::Accounts(Message::Open("mail:anna@example.org".into())),
                &data,
            );
            let choice = |message| crate::mail::Message::Accounts(Message::Storage(message));
            assert_eq!(crate::mail::update(&mut state, choice(storage::Message::Retention(7)), &data), Action::None);
            let _: Action<AppAction> =
                crate::mail::update(&mut state, choice(storage::Message::Select(MailCacheMode::AllOffline)), &data);
            let Action::Send(requests) = crate::mail::update(&mut state, choice(storage::Message::Apply), &data) else {
                panic!("policy and cache job sent together");
            };
            assert_eq!(requests.len(), 2);
            let Request::SetMailCachePolicy { policy } = &requests[0] else { panic!("policy first") };
            assert_eq!(policy.mode, MailCacheMode::AllOffline);
            assert_eq!(policy.attachment_retention_days, 7);
            assert!(matches!(requests[1], Request::StartMailCacheJob { .. }));
        }

        /// DS.7: "Show account" in Storage & index opens the account at "Overview"; an account
        /// already open keeps what is typed and only goes back to "Overview".
        #[test]
        fn show_account_lands_on_the_accounts_overview() {
            let mut data = data();
            data.mail_stats = vec![stats(&[("INBOX", 30, 8, Some(4096))], None)];
            let mut state = crate::mail::State::default();
            let show = || {
                crate::mail::Message::Storage(crate::mail::storage::Message::ShowAccount(
                    "mail:anna@example.org".into(),
                ))
            };
            let action = crate::mail::update(&mut state, show(), &data);
            assert_eq!(action, Action::Open(crate::mail::ACCOUNTS_SECTION));
            assert_eq!(page(&state.accounts, &data).sub_page.map(|(name, _)| name), Some("anna@example.org".into()));
            assert_eq!(state.accounts.tab, Tab::Overview);

            let _: Action<()> = update(&mut state.accounts, Message::Tab(Tab::Writing.index()), &data);
            let _: Action<()> = update(&mut state.accounts, Message::SenderName("Anna B.".into()), &data);
            let _ = crate::mail::update(&mut state, show(), &data);
            assert_eq!(state.accounts.tab, Tab::Overview);
            assert!(page(&state.accounts, &data).save_bar.is_some(), "the typed name stays");
        }

        /// Every role has its question, and every page builds – with and without an account.
        #[test]
        fn the_pages_build_with_their_questions() {
            for role in [Role::Inbox, Role::Drafts, Role::Sent, Role::Archive, Role::Junk, Role::Trash] {
                assert!(!role_question(role).is_empty(), "{role:?}");
            }
            let mut state = State::default();
            let _ = page(&state, &Data::default()).body;
            let data = data();
            let _: Action<()> = update(&mut state, Message::Open("mail:anna@example.org".into()), &data);
            let _ = page(&state, &data).body;
        }
    }
}

/// "Reading": how a mail shows in the reader and its colours there (task M9.8). Only the mail's
/// content follows "Mail colors", never the rest of the window.
pub mod reading {
    use std::sync::Arc;

    use iced::Length;
    use iced::widget::text::Wrapping;
    use iced::widget::text_editor::{Edit, Motion};
    use iced::widget::{column, container, row, text_editor};
    use torro_core::settings::{MailAppearance, MailFormat};
    use torro_gui::theme::space;
    use torro_gui::{layout, style, typo, widgets};
    use torro_ipc::{Request, SettingKey};
    use torro_mail::MailFormat as ComposeFormat;

    use super::*;
    use crate::parts::{picker, setting_row};

    const SIGNATURE_LINES: f32 = 8.0;

    fn signature_height() -> f32 {
        SIGNATURE_LINES * typo::line_height(typo::body()) + style::INPUT_PADDING.top + style::INPUT_PADDING.bottom
    }

    #[derive(Debug, Default)]
    pub struct State {
        account: Option<String>,
        signature: text_editor::Content,
        signature_format: bool,
        dirty: bool,
        saving: bool,
    }

    fn account<'a>(state: &State, data: &'a Data) -> Option<&'a torro_ipc::AccountInfo> {
        data.accounts.iter().find(|account| state.account.as_deref() == Some(account.id.as_str()))
    }

    fn signature_text(state: &State) -> String {
        state.signature.text().replace("\r\n", "\n").trim_end().to_string()
    }

    pub fn loaded(state: &mut State, data: &Data) {
        let chosen = state
            .account
            .as_ref()
            .and_then(|id| data.accounts.iter().find(|account| &account.id == id && account.mail.is_some()))
            .or_else(|| data.settings.mail_sender.as_ref().and_then(|id| data.accounts.iter().find(|a| &a.id == id)))
            .or_else(|| data.accounts.iter().find(|account| account.mail.is_some()));
        let Some(chosen) = chosen else { return };
        let Some(mail) = &chosen.mail else { return };
        let stored = mail.settings.signature.as_deref().unwrap_or_default().trim_end();
        if state.account.as_deref() != Some(chosen.id.as_str()) || !state.dirty || signature_text(state) == stored {
            state.account = Some(chosen.id.clone());
            state.signature = text_editor::Content::with_text(stored);
            state.signature_format = mail.settings.signature_format;
            state.dirty = false;
        }
    }

    pub fn sent(state: &mut State) {
        state.saving = false;
    }

    /// What a row of this section changes; each is one `settings.set_setting`.
    #[derive(Debug, Clone)]
    pub enum Message {
        /// "Show mail as": how a mail with HTML first shows in the reader.
        Show(MailFormat),
        /// "Mail colors": the colours of a mail's content in the reader.
        Colors(MailAppearance),
        /// Account used for a new message without an explicit account choice.
        Sender(String),
        Compose(ComposeFormat),
        Signature(text_editor::Action),
        FormatSignature(SignatureStyle),
        SaveSignature,
        DiscardSignature,
    }

    pub fn update<A>(state: &mut State, message: Message, data: &Data) -> Action<A> {
        match message {
            Message::Show(format) => Action::send(Request::SetSetting {
                key: SettingKey::MailView,
                value: serde_json::to_value(format).unwrap_or_default(),
            }),
            Message::Colors(colors) => Action::send(Request::SetSetting {
                key: SettingKey::MailAppearance,
                value: serde_json::to_value(colors).unwrap_or_default(),
            }),
            Message::Sender(id) => {
                if state.dirty || state.saving {
                    return Action::None;
                }
                state.account = Some(id.clone());
                if let Some(mail) = account(state, data).and_then(|account| account.mail.as_ref()) {
                    state.signature =
                        text_editor::Content::with_text(mail.settings.signature.as_deref().unwrap_or_default());
                    state.signature_format = mail.settings.signature_format;
                }
                Action::send(Request::SetSetting { key: SettingKey::MailSender, value: serde_json::json!(id) })
            }
            Message::Compose(format) => {
                let Some(account) = account(state, data) else { return Action::None };
                Action::send(Request::UpdateMailAccount {
                    id: account.id.clone(),
                    sender_name: None,
                    signature: None,
                    signature_format: None,
                    notify: None,
                    services: None,
                    folder_roles: None,
                    format: Some(format),
                })
            }
            Message::Signature(action) => {
                state.signature.perform(action);
                state.dirty = account(state, data).and_then(|account| account.mail.as_ref()).is_some_and(|mail| {
                    signature_text(state) != mail.settings.signature.as_deref().unwrap_or_default().trim_end()
                        || state.signature_format != mail.settings.signature_format
                });
                Action::None
            }
            Message::FormatSignature(style) => {
                if !state.signature_format {
                    let plain = signature_text(state);
                    let escaped = escape_markup(&plain);
                    state.signature = text_editor::Content::with_text(&escaped);
                    state.signature.perform(text_editor::Action::Move(Motion::DocumentEnd));
                    state.signature_format = true;
                }
                apply_signature_style(&mut state.signature, style);
                state.dirty = true;
                Action::None
            }
            Message::SaveSignature if state.dirty && !state.saving => {
                let Some(account) = account(state, data) else { return Action::None };
                let id = account.id.clone();
                let signature = signature_text(state);
                state.saving = true;
                Action::send(Request::UpdateMailAccount {
                    id,
                    sender_name: None,
                    signature: Some(signature),
                    signature_format: Some(state.signature_format),
                    notify: None,
                    services: None,
                    folder_roles: None,
                    format: None,
                })
            }
            Message::SaveSignature => Action::None,
            Message::DiscardSignature => {
                if let Some(mail) = account(state, data).and_then(|account| account.mail.as_ref()) {
                    state.signature =
                        text_editor::Content::with_text(mail.settings.signature.as_deref().unwrap_or_default());
                    state.signature_format = mail.settings.signature_format;
                    state.dirty = false;
                }
                Action::None
            }
        }
    }

    pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
        let settings = &data.settings;
        let show = picker(SHOW_CHOICES, ShowChoice(settings.mail_view), |choice: ShowChoice| Message::Show(choice.0));
        let colors = picker(COLOR_CHOICES, ColorChoice(settings.mail_appearance), |choice: ColorChoice| {
            Message::Colors(choice.0)
        });
        let group = widgets::group(
            tr("Reading"),
            vec![setting_row(None, tr("Show mail as"), None, show), setting_row(None, tr("Mail colors"), None, colors)],
            None,
        );
        let mut page = column![group].spacing(layout::section_gap());
        let accounts: Vec<SenderChoice> = data
            .accounts
            .iter()
            .filter_map(|account| {
                let mail = account.mail.as_ref().filter(|mail| mail.settings.services.mail)?;
                Some(SenderChoice { id: account.id.clone(), label: mail.address.clone() })
            })
            .collect();
        if !accounts.is_empty() {
            let chosen = state
                .account
                .as_ref()
                .or(data.settings.mail_sender.as_ref())
                .and_then(|id| accounts.iter().find(|account| &account.id == id))
                .cloned()
                .unwrap_or_else(|| accounts[0].clone());
            let sender = picker(accounts, chosen, |choice: SenderChoice| Message::Sender(choice.id));
            let mail = account(state, data).and_then(|account| account.mail.as_ref());
            let compose = mail.map(|mail| {
                picker(COMPOSE_CHOICES, ComposeChoice(mail.settings.format), |choice: ComposeChoice| {
                    Message::Compose(choice.0)
                })
            });
            let signature = text_editor(&state.signature)
                .on_action(Message::Signature)
                .font(typo::body().font)
                .size(typo::body().size)
                .line_height(typo::body().line_height)
                .padding(style::INPUT_PADDING)
                .wrapping(Wrapping::WordOrGlyph)
                .height(signature_height())
                .style(style::text_editor);
            let toolbar = row(SignatureStyle::ALL
                .map(|style| widgets::icon_button(style.icon(), style.name(), Some(Message::FormatSignature(style)))))
            .spacing(space::xs());
            let controls = row![
                widgets::text_button(
                    None,
                    tr("Discard"),
                    style::button::secondary,
                    state.dirty.then_some(Message::DiscardSignature)
                ),
                widgets::text_button(
                    None,
                    tr("Save signature"),
                    style::button::primary,
                    (state.dirty && !state.saving).then_some(Message::SaveSignature)
                ),
            ]
            .spacing(space::s());
            let mut rows = vec![setting_row(None, tr("Default sender address"), None, sender)];
            if let Some(compose) = compose {
                rows.push(setting_row(None, tr("Write new mail as"), None, compose));
                rows.push(
                    container(
                        column![typo::text(typo::body_strong(), tr("Signature")), toolbar, signature, controls]
                            .spacing(space::s()),
                    )
                    .width(Length::Fill)
                    .padding([space::s(), space::l()])
                    .into(),
                );
            }
            page = page.push(widgets::group(tr("Writing"), rows, None));
        }
        page.into()
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct SenderChoice {
        id: String,
        label: String,
    }

    impl std::fmt::Display for SenderChoice {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(&self.label)
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct ComposeChoice(ComposeFormat);

    const COMPOSE_CHOICES: [ComposeChoice; 2] =
        [ComposeChoice(ComposeFormat::Html), ComposeChoice(ComposeFormat::Plain)];

    impl std::fmt::Display for ComposeChoice {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(match self.0 {
                ComposeFormat::Html => tr("Formatted (HTML)"),
                ComposeFormat::Plain => tr("Plain text"),
            })
        }
    }

    #[derive(Debug, Clone, Copy)]
    pub enum SignatureStyle {
        Bold,
        Italic,
        Underline,
        Link,
        Bullets,
        Numbers,
    }

    impl SignatureStyle {
        const ALL: [Self; 6] = [Self::Bold, Self::Italic, Self::Underline, Self::Link, Self::Bullets, Self::Numbers];

        fn icon(self) -> Icon {
            match self {
                Self::Bold => Icon::Bold,
                Self::Italic => Icon::Italic,
                Self::Underline => Icon::Underline,
                Self::Link => Icon::Link,
                Self::Bullets => Icon::List,
                Self::Numbers => Icon::ListOrdered,
            }
        }

        fn name(self) -> &'static str {
            match self {
                Self::Bold => tr("Bold"),
                Self::Italic => tr("Italic"),
                Self::Underline => tr("Underline"),
                Self::Link => tr("Link"),
                Self::Bullets => tr("Bulleted list"),
                Self::Numbers => tr("Numbered list"),
            }
        }
    }

    fn escape_markup(plain: &str) -> String {
        let mut escaped = String::with_capacity(plain.len());
        for character in plain.chars() {
            if "\\*_[]()>-#`".contains(character) {
                escaped.push('\\');
            }
            escaped.push(character);
        }
        escaped
    }

    fn apply_signature_style(content: &mut text_editor::Content, style: SignatureStyle) {
        let selected = content.selection().unwrap_or_default();
        let (replacement, back) = match style {
            SignatureStyle::Bold => (format!("**{selected}**"), selected.is_empty().then_some(2)),
            SignatureStyle::Italic => (format!("*{selected}*"), selected.is_empty().then_some(1)),
            SignatureStyle::Underline => (format!("__{selected}__"), selected.is_empty().then_some(2)),
            SignatureStyle::Link => (format!("[{selected}]()"), Some(if selected.is_empty() { 3 } else { 1 })),
            SignatureStyle::Bullets => (
                if selected.is_empty() {
                    "- ".to_string()
                } else {
                    selected.lines().map(|line| format!("- {line}")).collect::<Vec<_>>().join("\n")
                },
                None,
            ),
            SignatureStyle::Numbers => (
                if selected.is_empty() {
                    "1. ".to_string()
                } else {
                    selected
                        .lines()
                        .enumerate()
                        .map(|(at, line)| format!("{}. {line}", at + 1))
                        .collect::<Vec<_>>()
                        .join("\n")
                },
                None,
            ),
        };
        content.perform(text_editor::Action::Edit(Edit::Paste(Arc::new(replacement))));
        for _ in 0..back.unwrap_or(0) {
            content.perform(text_editor::Action::Move(Motion::Left));
        }
    }

    /// "Show mail as" as a choice of a pick list.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct ShowChoice(MailFormat);

    const SHOW_CHOICES: [ShowChoice; 2] = [ShowChoice(MailFormat::Html), ShowChoice(MailFormat::Text)];

    impl std::fmt::Display for ShowChoice {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(match self.0 {
                MailFormat::Html => tr("Formatted (HTML)"),
                MailFormat::Text => tr("Plain text"),
            })
        }
    }

    /// "Mail colors" as a choice of a pick list.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct ColorChoice(MailAppearance);

    const COLOR_CHOICES: [ColorChoice; 3] =
        [ColorChoice(MailAppearance::App), ColorChoice(MailAppearance::Light), ColorChoice(MailAppearance::Dark)];

    impl std::fmt::Display for ColorChoice {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(match self.0 {
                MailAppearance::App => tr("Like the app"),
                MailAppearance::Light => tr("Light"),
                MailAppearance::Dark => tr("Dark"),
            })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// Each row sends `set_setting` with its own key and nothing else.
        #[test]
        fn each_row_sets_only_its_setting() {
            let show: Action<()> = update(&mut State::default(), Message::Show(MailFormat::Text), &Data::default());
            assert_eq!(
                show,
                Action::send(Request::SetSetting { key: SettingKey::MailView, value: serde_json::json!("text") })
            );
            let colors: Action<()> =
                update(&mut State::default(), Message::Colors(MailAppearance::Dark), &Data::default());
            assert_eq!(
                colors,
                Action::send(Request::SetSetting { key: SettingKey::MailAppearance, value: serde_json::json!("dark") })
            );
        }

        /// The choices read as the control centre's group "Mail" had them.
        #[test]
        fn the_choices_read_as_before() {
            let shows: Vec<String> = SHOW_CHOICES.iter().map(ToString::to_string).collect();
            assert_eq!(shows, [tr("Formatted (HTML)"), tr("Plain text")]);
            let colors: Vec<String> = COLOR_CHOICES.iter().map(ToString::to_string).collect();
            assert_eq!(colors, [tr("Like the app"), tr("Light"), tr("Dark")]);
        }

        #[test]
        fn the_page_builds() {
            let _: Element<'_, Message> = view(&State::default(), &Data::default());
        }
    }
}

pub mod storage {
    use iced::Element;
    use iced::Length;
    use iced::widget::{column, container, radio, responsive, row, space as blank};
    use torro_core::i18n::{tr, trf};
    use torro_gui::theme::{self, space};
    use torro_gui::units::{format_bytes, format_count};
    use torro_gui::{layout, style, typo, widgets};
    use torro_ipc::mail::{MailCacheAction, MailCacheEstimate, MailCacheJobState, MailCacheMode, MailCachePolicy};
    use torro_ipc::{Request, Response};

    use super::*;
    use crate::parts::{field, picker, row_frame};

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Retention {
        None,
        OneDay,
        OneWeek,
        OneMonth,
        All,
        Other(u32),
    }

    impl Retention {
        const ALL: [Self; 5] = [Self::None, Self::OneDay, Self::OneWeek, Self::OneMonth, Self::All];

        fn from_days(days: u32) -> Self {
            match days {
                0 => Self::None,
                1 => Self::OneDay,
                7 => Self::OneWeek,
                30 => Self::OneMonth,
                u32::MAX => Self::All,
                days => Self::Other(days),
            }
        }

        fn days(self) -> u32 {
            match self {
                Self::None => 0,
                Self::OneDay => 1,
                Self::OneWeek => 7,
                Self::OneMonth => 30,
                Self::All => u32::MAX,
                Self::Other(days) => days,
            }
        }
    }

    impl std::fmt::Display for Retention {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            let label = match self {
                Self::None => tr("None"),
                Self::OneDay => tr("1 day"),
                Self::OneWeek => tr("7 days"),
                Self::OneMonth => tr("30 days"),
                Self::All => tr("All"),
                Self::Other(days) => return write!(formatter, "{}", trf("{days} days", &[("days", &days)])),
            };
            formatter.write_str(label)
        }
    }

    #[derive(Debug, Default)]
    pub struct State {
        pub(super) account: Option<String>,
        pending_mode: Option<MailCacheMode>,
        pending_retention: Option<u32>,
        date_input: Option<String>,
        size_input: Option<String>,
        estimate: Option<MailCacheEstimate>,
        cleanup_preview: Option<MailCacheEstimate>,
        previewing_cleanup: bool,
        error: Option<String>,
    }

    #[derive(Debug, Clone)]
    pub enum Message {
        Account(String),
        Select(MailCacheMode),
        Date(String),
        Size(String),
        Apply,
        Retention(u32),
        Job(MailCacheAction),
        PreviewCleanup,
        Pause(String),
        Resume(String),
        Refresh,
        /// "Show account": the account's page under Accounts, at its tab "Overview" (task DS.7);
        /// [`super::update`] takes it, since it opens another section.
        ShowAccount(String),
    }

    fn selected<'a>(state: &'a State, data: &'a Data) -> Option<&'a str> {
        state
            .account
            .as_deref()
            .filter(|id| data.accounts.iter().any(|account| account.id == *id && account.mail.is_some()))
            .or_else(|| data.accounts.iter().find(|account| account.mail.is_some()).map(|account| account.id.as_str()))
    }

    pub(super) fn select_account(state: &mut State, account: &str) {
        if state.account.as_deref() != Some(account) {
            state.account = Some(account.to_string());
            state.pending_mode = None;
            state.pending_retention = None;
            state.date_input = None;
            state.size_input = None;
            state.error = None;
        }
    }

    pub fn update<A>(state: &mut State, message: Message, data: &Data) -> Action<A> {
        match message {
            Message::Account(account) => {
                select_account(state, &account);
                state.estimate = None;
                state.cleanup_preview = None;
                Action::None
            }
            Message::Select(mode) => {
                state.pending_mode = Some(mode);
                state.estimate = None;
                state.cleanup_preview = None;
                state.error = None;
                state.previewing_cleanup = false;
                Action::None
            }
            Message::Date(value) => {
                state.date_input = Some(value);
                state.error = None;
                Action::None
            }
            Message::Size(value) => {
                state.size_input = Some(value);
                state.error = None;
                Action::None
            }
            Message::Apply => {
                let Some(account) = selected(state, data) else { return Action::None };
                let current = policy(data, account);
                let current_mode = current.mode;
                let Some(mode) = state.pending_mode.or(match current.mode {
                    MailCacheMode::HeadersOnly => None,
                    MailCacheMode::Recent90Days => Some(MailCacheMode::SinceDate),
                    mode => Some(mode),
                }) else {
                    return Action::None;
                };
                if data
                    .mail_cache_jobs
                    .iter()
                    .any(|job| job.account == account && job.state == MailCacheJobState::Running)
                {
                    return Action::None;
                }
                let mut policy = current;
                policy.mode = mode;
                policy.attachment_retention_days = state.pending_retention.unwrap_or(policy.attachment_retention_days);
                policy.prefetch_attachments = false;
                if mode == MailCacheMode::SinceDate {
                    let legacy =
                        (chrono::Utc::now().date_naive() - chrono::Duration::days(90)).format("%Y-%m-%d").to_string();
                    let typed = state
                        .date_input
                        .as_deref()
                        .or(policy.since_date.as_deref())
                        .unwrap_or_else(|| if current_mode == MailCacheMode::Recent90Days { &legacy } else { "" });
                    let parsed = chrono::NaiveDate::parse_from_str(typed, "%d.%m.%Y")
                        .or_else(|_| chrono::NaiveDate::parse_from_str(typed, "%Y-%m-%d"));
                    let Ok(date) = parsed else {
                        state.error = Some(tr("Enter a valid date (DD.MM.YYYY).").to_string());
                        return Action::None;
                    };
                    policy.since_date = Some(date.format("%Y-%m-%d").to_string());
                }
                if mode == MailCacheMode::MaxSize {
                    let mb = match state.size_input.as_deref() {
                        Some(typed) => typed.trim().parse::<u64>().ok(),
                        None => policy.max_local_mb,
                    };
                    let Some(mb) = mb.filter(|mb| (1..=1_000_000).contains(mb)) else {
                        state.error = Some(tr("Enter a size in MiB from 1 to 1000000.").to_string());
                        return Action::None;
                    };
                    policy.max_local_mb = Some(mb);
                }
                let account = account.to_string();
                state.pending_mode = None;
                state.pending_retention = None;
                state.estimate = None;
                state.cleanup_preview = None;
                Action::Send(vec![
                    Request::SetMailCachePolicy { policy },
                    Request::StartMailCacheJob { account, action: MailCacheAction::ApplyPolicy },
                ])
            }
            Message::Retention(days) => {
                state.pending_retention = Some(days);
                state.error = None;
                Action::None
            }
            Message::Job(action) => {
                let Some(account) = selected(state, data).map(str::to_string) else { return Action::None };
                if action == MailCacheAction::CleanCache
                    && state.cleanup_preview.as_ref().is_none_or(|estimate| estimate.account != account)
                {
                    return Action::None;
                }
                state.cleanup_preview = None;
                Action::send(Request::StartMailCacheJob { account, action })
            }
            Message::PreviewCleanup => {
                let Some(account) = selected(state, data).map(str::to_string) else { return Action::None };
                let mode = policy(data, &account).mode;
                state.previewing_cleanup = true;
                state.cleanup_preview = None;
                state.error = None;
                Action::Ask(Request::PreviewMailCache { account, mode })
            }
            Message::Pause(id) => {
                let Some(account) = selected(state, data) else { return Action::None };
                Action::send(Request::ControlMailCacheJob { account: account.to_string(), id, resume: false })
            }
            Message::Resume(id) => {
                let Some(account) = selected(state, data) else { return Action::None };
                Action::send(Request::ControlMailCacheJob { account: account.to_string(), id, resume: true })
            }
            Message::Refresh => Action::Send(Vec::new()),
            Message::ShowAccount(_) => Action::None,
        }
    }

    pub fn answered(state: &mut State, answer: &Result<Response, String>) {
        match answer {
            Ok(Response::MailCacheEstimate { estimate }) => {
                if state.previewing_cleanup {
                    state.cleanup_preview = Some(estimate.clone());
                } else {
                    state.estimate = Some(estimate.clone());
                }
                state.previewing_cleanup = false;
                state.error = None;
            }
            Err(error) if state.pending_mode.is_some() || state.previewing_cleanup => {
                state.error = Some(error.clone());
                state.previewing_cleanup = false;
            }
            _ => {}
        }
    }

    fn policy(data: &Data, account: &str) -> MailCachePolicy {
        data.mail_cache_policies.iter().find(|policy| policy.account == account).cloned().unwrap_or(MailCachePolicy {
            account: account.to_string(),
            mode: MailCacheMode::Recent90Days,
            since_date: None,
            max_local_mb: None,
            prefetch_attachments: false,
            attachment_retention_days: 1,
        })
    }

    fn button<'a>(label: &'static str, message: Option<Message>) -> Element<'a, Message> {
        widgets::text_button(None, tr(label), style::button::secondary, message)
    }

    /// The account's own storage tab; no account switcher or repeated account statistics.
    pub(super) fn account_view<'a>(state: &'a State, data: &'a Data, account: &'a str) -> Element<'a, Message> {
        responsive(move |room| {
            let summary = inventory_card(data, account);
            let choices = offline_choices(state, data, account);
            if room.width < 850.0 {
                column![summary, choices].spacing(layout::section_gap()).into()
            } else {
                row![
                    container(summary).width(Length::FillPortion(3)),
                    container(choices).width(Length::FillPortion(2)),
                ]
                .spacing(layout::section_gap())
                .align_y(iced::Alignment::Start)
                .into()
            }
        })
        .into()
    }

    fn inventory_card<'a>(data: &Data, account: &str) -> Element<'a, Message> {
        let Some(stats) = data.mail_stats.iter().find(|stats| stats.account == account) else {
            return widgets::empty_state(
                tr("Statistics are not available yet."),
                tr("The mail service supplies these numbers from its local store."),
            );
        };
        let online = stats.known_mail_bytes.map_or_else(|| tr("Not known").to_string(), format_bytes);
        let local = stats.offline_mail_bytes.map_or_else(|| tr("Not known").to_string(), format_bytes);
        let numbers = row![
            column![
                typo::text(typo::meta(), tr("Online")).style(theme::text_2),
                typo::text(typo::title(), online),
                typo::text(typo::meta(), trf("{n} emails total", &[("n", &format_count(stats.messages))]))
                    .style(theme::text_2),
            ]
            .spacing(space::xs())
            .width(Length::Fill),
            column![
                typo::text(typo::meta(), tr("On this device")).style(theme::text_2),
                typo::text(typo::title(), local),
                typo::text(typo::meta(), trf("{n} emails offline", &[("n", &format_count(stats.cached_bodies))]))
                    .style(theme::text_2),
            ]
            .spacing(space::xs())
            .width(Length::Fill),
        ]
        .spacing(space::l());
        let mut body = column![numbers].spacing(space::m());
        if let (Some(total), Some(actual), Some(planned), Some(indexed)) = (
            stats.known_mail_bytes,
            stats.offline_mail_bytes,
            stats.planned_mail_bytes,
            stats.indexed_offline_mail_bytes,
        ) {
            body = body
                .push(widgets::mail_coverage_bars(actual, planned, indexed, total))
                .push(widgets::mail_coverage_legend());
        } else {
            body = body.push(typo::text(typo::meta(), tr("Mail sizes are being collected")).style(theme::text_2));
        }
        body = body.push(
            typo::text(
                typo::meta(),
                trf("Local database incl. headers and index: {size}", &[("size", &format_bytes(stats.database_bytes))]),
            )
            .style(theme::text_2),
        );
        widgets::chart_card(tr("Online and on this device"), "", body)
    }

    fn offline_choices<'a>(state: &'a State, data: &'a Data, account: &'a str) -> Element<'a, Message> {
        let current = policy(data, account);
        let chosen = state.pending_mode.unwrap_or(match current.mode {
            MailCacheMode::Recent90Days => MailCacheMode::SinceDate,
            mode => mode,
        });
        let date = state.date_input.clone().unwrap_or_else(|| {
            current
                .since_date
                .as_deref()
                .and_then(|value| chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").ok())
                .map_or_else(
                    || {
                        if current.mode == MailCacheMode::Recent90Days {
                            (chrono::Utc::now().date_naive() - chrono::Duration::days(90))
                                .format("%d.%m.%Y")
                                .to_string()
                        } else {
                            String::new()
                        }
                    },
                    |date| date.format("%d.%m.%Y").to_string(),
                )
        });
        let size = state
            .size_input
            .clone()
            .unwrap_or_else(|| current.max_local_mb.map_or_else(String::new, |mb| mb.to_string()));
        let choice = |label, mode| {
            row_frame(
                radio(label, mode, Some(chosen), Message::Select).size(style::CHECK_SIZE).style(style::radio),
                false,
            )
        };
        let mut rows = vec![
            choice(tr("All emails"), MailCacheMode::AllOffline),
            choice(tr("Mail from date"), MailCacheMode::SinceDate),
            choice(tr("Limit local size"), MailCacheMode::MaxSize),
        ];
        if current.mode == MailCacheMode::HeadersOnly && state.pending_mode.is_none() {
            rows.insert(
                0,
                container(
                    typo::text(
                        typo::meta(),
                        tr("This account still uses the previous header-only setting. Choose one of the three offline options."),
                    )
                    .style(theme::text_2),
                )
                .width(Length::Fill)
                .padding([space::m(), space::l()])
                .into(),
            );
        }
        if chosen == MailCacheMode::SinceDate {
            rows.push(
                responsive(move |room| {
                    let label = typo::text(typo::body(), tr("From date"));
                    let input = field(tr("DD.MM.YYYY"), &date).on_input(Message::Date);
                    if room.width < 460.0 {
                        container(column![label, input].spacing(space::s()))
                            .width(Length::Fill)
                            .padding([space::m(), space::l()])
                            .into()
                    } else {
                        row_frame(row![label, input].spacing(space::m()).align_y(iced::Alignment::Center), false)
                    }
                })
                .height(Length::Shrink)
                .into(),
            );
        }
        if chosen == MailCacheMode::MaxSize {
            rows.push(
                responsive(move |room| {
                    let label = typo::text(typo::body(), tr("Maximum"));
                    let input = field(tr("MiB"), &size).on_input(Message::Size);
                    let unit = typo::text(typo::meta(), tr("MiB")).style(theme::text_2);
                    if room.width < 460.0 {
                        container(column![label, row![input, unit].spacing(space::s())].spacing(space::s()))
                            .width(Length::Fill)
                            .padding([space::m(), space::l()])
                            .into()
                    } else {
                        row_frame(row![label, input, unit].spacing(space::m()).align_y(iced::Alignment::Center), false)
                    }
                })
                .height(Length::Shrink)
                .into(),
            );
        }
        if let Some(error) = &state.error {
            rows.push(
                container(typo::text(typo::meta(), error.clone()).style(theme::text_2))
                    .width(Length::Fill)
                    .padding([space::m(), space::l()])
                    .into(),
            );
        }
        let running =
            data.mail_cache_jobs.iter().any(|job| job.account == account && job.state == MailCacheJobState::Running);
        let retention = Retention::from_days(state.pending_retention.unwrap_or(current.attachment_retention_days));
        let mut retention_options = Retention::ALL.to_vec();
        if matches!(retention, Retention::Other(_)) {
            retention_options.push(retention);
        }
        rows.push(
            responsive(move |room| {
                let label = typo::text(typo::body(), tr("Keep downloaded attachments"));
                let select = picker(retention_options.clone(), retention, |choice| Message::Retention(choice.days()));
                if room.width < 480.0 {
                    container(column![label, select].spacing(space::s()))
                        .width(Length::Fill)
                        .padding([space::m(), space::l()])
                        .into()
                } else {
                    row_frame(
                        row![label, blank::horizontal(), select].spacing(space::m()).align_y(iced::Alignment::Center),
                        false,
                    )
                }
            })
            .height(Length::Shrink)
            .into(),
        );
        rows.push(row_frame(
            row![
                blank::horizontal(),
                button(
                    "Save offline selection",
                    (!running && chosen != MailCacheMode::HeadersOnly).then_some(Message::Apply)
                )
            ],
            false,
        ));
        let note = if chosen == MailCacheMode::MaxSize {
            tr(
                "The limit includes offline mail and downloaded attachments; headers and the search index use additional space.",
            )
        } else {
            tr("Headers of all known emails stay on this device.")
        };
        let strategy = widgets::group(tr("Which content stays offline?"), rows, Some(note.to_string()));
        let mut content = column![strategy].spacing(layout::section_gap());
        if let Some(job) =
            data.mail_cache_jobs.iter().find(|job| job.account == account && job.state == MailCacheJobState::Running)
        {
            content = content.push(widgets::group(
                tr("Applying offline selection"),
                vec![row_frame(
                    row![
                        widgets::progress_bar(job.processed as usize, job.total as usize),
                        typo::text(
                            typo::meta(),
                            format!("{} / {}", format_count(job.processed), format_count(job.total)),
                        )
                        .style(theme::text_2),
                    ]
                    .spacing(space::m())
                    .align_y(iced::Alignment::Center),
                    false,
                )],
                None,
            ));
        }
        content.into()
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::Host;
    use crate::demo::Demo;

    /// Both sections build their page, with and without a mail account.
    #[test]
    fn every_section_builds_with_and_without_an_account() {
        let mut state = State::default();
        let data = Data {
            accounts: vec![accounts::tests::account("anna@example.org", "Anna Example", None)],
            ..Data::default()
        };
        for data in [&data, &Data::default()] {
            for section in SECTIONS {
                let page = page(&state, data, section.id);
                let _ = (page.body, page.sub_page, page.actions);
            }
            // An id no section has is Accounts, not an empty page.
            let _ = page(&state, data, "shortcuts").body;
        }
        // The page below Accounts, with the account in the path.
        let _: Action<AppAction> =
            update(&mut state, Message::Accounts(accounts::Message::Open("mail:anna@example.org".into())), &data);
        assert!(page(&state, &data, "accounts").sub_page.is_some());
    }

    #[test]
    fn backup_answer_returns_to_the_account_that_started_the_job() {
        use torro_ipc::mail::{BackupJob, BackupPhase, BackupScope};

        let anna = "mail:anna@example.org";
        let bernd = "mail:bernd@example.org";
        let data = Data {
            accounts: vec![
                accounts::tests::account("anna@example.org", "Anna", None),
                accounts::tests::account("bernd@example.org", "Bernd", None),
            ],
            ..Data::default()
        };
        let mut state = State::default();
        let _: Action<AppAction> = update(&mut state, Message::Accounts(accounts::Message::Open(anna.into())), &data);
        let action: Action<AppAction> =
            update(&mut state, Message::Accounts(accounts::Message::Backup(backup::Message::Create)), &data);
        assert!(matches!(action, Action::Ask(torro_ipc::Request::MailBackupCreate { .. })));
        let _: Action<AppAction> = update(&mut state, Message::Accounts(accounts::Message::Open(bernd.into())), &data);
        let job = BackupJob {
            id: "job-anna".into(),
            account: anna.into(),
            source: BackupScope::Server,
            phase: BackupPhase::Reading,
            folders: 1,
            messages: 2,
            bytes: 32,
            archive: None,
            warnings: Vec::new(),
        };
        backup_answered(&mut state, anna, &Ok(torro_ipc::Response::MailBackupJob { job }));
        assert!(accounts::poll_request(&mut state.accounts).is_none(), "Bernd's page has no Anna job");
        let _: Action<AppAction> = update(&mut state, Message::Accounts(accounts::Message::Open(anna.into())), &data);
        assert!(
            matches!(accounts::poll_request(&mut state.accounts), Some(torro_ipc::Request::MailBackupStatus { job, .. }) if job == "job-anna")
        );
        assert_eq!(
            update(&mut state, Message::Accounts(accounts::Message::Backup(backup::Message::Create)), &data),
            Action::None
        );
    }

    /// Closing the settings keeps each account's backup page (`kept`), and a staged demo page shows
    /// when its account opens; "Choose…" is the window's, and the path it picked comes back.
    #[test]
    fn backup_pages_outlive_closed_settings_and_choosers_round_trip() {
        let anna = "mail:anna@example.org";
        let data =
            Data { accounts: vec![accounts::tests::account("anna@example.org", "Anna", None)], ..Data::default() };
        let mut state = State::default();
        stage_backup(&mut state, anna, BackupScene::Running);
        let mut state = kept(state);
        let _: Action<AppAction> = update(&mut state, Message::Accounts(accounts::Message::Open(anna.into())), &data);
        assert!(
            matches!(poll_request(&mut state), Some(torro_ipc::Request::MailBackupStatus { job, .. }) if job == "demo-job")
        );
        let choose = Message::Accounts(accounts::Message::Backup(backup::Message::Choose(BackupChooser::Folder)));
        assert_eq!(backup_chooser(&choose), Some(BackupChooser::Folder));
        assert_eq!(backup_chooser(&Message::Accounts(accounts::Message::Back)), None);
        let folder = std::env::temp_dir();
        let _: Action<AppAction> = update(&mut state, backup_chosen(BackupChooser::Folder, folder.clone()), &data);
        assert_eq!(accounts::backup_destination(&state.accounts), Some(folder.to_string_lossy().as_ref()));
    }

    /// The sections' ids and names, as the list and `--settings <section>` use them. "Keyboard
    /// shortcuts" of task E10.3 is not among them yet.
    #[test]
    fn the_sections_are_reachable_by_their_words() {
        let ids: Vec<&str> = SECTIONS.iter().map(|section| section.id).collect();
        assert_eq!(ids, ["accounts", "reading", "attachments", "categorization", "filters"]);
        assert_eq!(SECTIONS[0].name(), tr("Accounts"));
        assert_eq!(SECTIONS[2].name(), tr("Attachments"), "task M12.7");
        assert_eq!(SECTIONS[3].name(), tr("Categorization"), "task E12.10");
        assert_eq!(SECTIONS[4].name(), tr("Filters"), "issue #339");
        let (mut host, _) = Host::new(Arc::new(Demo::new()), SECTIONS.to_vec(), wanted());
        for id in ids {
            assert!(host.open(id), "{id}");
        }
        assert!(!host.open("sources"), "a section of the Notification Hub is not one of these");
    }

    /// One load asks for what the sections show, and for nothing else.
    #[test]
    fn the_load_asks_for_what_the_sections_show() {
        assert_eq!(
            wanted(),
            vec![
                torro_ipc::Request::Settings,
                torro_ipc::Request::Accounts,
                torro_ipc::Request::ListSaveTargets { id: None },
                torro_ipc::Request::MailAccountStats { account: None },
                torro_ipc::Request::AiAssignments,
                torro_ipc::Request::GetCategorization,
                torro_ipc::Request::PreviewCategoryJob { scope: torro_ipc::inbox::CategoryJobScope::default() },
                torro_ipc::Request::GetCategoryJob { id: None },
                torro_ipc::Request::PreviewClearCategories,
                torro_ipc::Request::ListCategories { account: None },
                torro_ipc::Request::ListSortRules,
                torro_ipc::Request::ListMailFilters,
                torro_ipc::Request::MailFolders { account: None },
            ]
        );
    }

    /// Task E12.10: a category switched off is stored as hidden, one switched on is taken out
    /// again – only the hidden ones are kept, so a category added later starts shown.
    #[test]
    fn switching_a_category_off_stores_it_as_hidden() {
        use torro_mail::Category;

        let mut data = Data::default();
        let hide = categorization::update::<()>(
            &mut Default::default(),
            categorization::Message::Shown(Category::Spam, false),
            &data,
        );
        let Action::Send(requests) = hide else { panic!("a request") };
        let [torro_ipc::Request::SetSetting { key, value }] = requests.as_slice() else { panic!("one setting") };
        assert_eq!(*key, torro_ipc::SettingKey::HiddenCategories);
        assert_eq!(value, &serde_json::json!(["spam"]));

        data.settings.hidden_categories = vec![Category::Spam, Category::Promotion];
        let show = categorization::update::<()>(
            &mut Default::default(),
            categorization::Message::Shown(Category::Spam, true),
            &data,
        );
        let Action::Send(requests) = show else { panic!("a request") };
        let [torro_ipc::Request::SetSetting { value, .. }] = requests.as_slice() else { panic!("one setting") };
        assert_eq!(value, &serde_json::json!(["promotion"]), "only the ones still hidden");
    }

    /// The slot Sorting with `model` of the local Ollama (task MW.3).
    fn slot(model: &str) -> torro_core::ai::SlotModel {
        torro_core::ai::SlotModel {
            engine: "ai:ollama@localhost:11434".into(),
            model: Some(model.into()),
            effort: None,
        }
    }

    /// The exception `inbox.categorize` with `model` of the local Ollama, on or off (task MW.3).
    fn exception(model: &str, enabled: bool) -> Vec<torro_ipc::AiAssignmentInfo> {
        use torro_core::ai::{AiAssignment, AiFunction};
        vec![torro_ipc::AiAssignmentInfo {
            assignment: AiAssignment {
                function: AiFunction::InboxCategorize,
                engine: Some("ai:ollama@localhost:11434".into()),
                model: Some(model.into()),
                effort: None,
                enabled,
            },
            label: "Sort mail into categories".into(),
            needs_decision: true,
        }]
    }

    /// What `message` sends on the Categorization page over `data`, one request after the other.
    fn sent(message: categorization::Message, data: &Data) -> Vec<torro_ipc::Request> {
        match categorization::update::<()>(&mut Default::default(), message, data) {
            Action::Send(requests) => requests,
            Action::None => Vec::new(),
            _ => panic!("a request or nothing"),
        }
    }

    /// `ai.set_slot decide` with `model` of `engine`.
    fn set_slot(engine: &str, model: &str) -> torro_ipc::Request {
        torro_ipc::Request::SetAiSlot {
            slot: torro_core::ai::AiSlot::Decide,
            engine: Some(engine.into()),
            model: Some(model.into()),
            effort: None,
        }
    }

    /// The exception taken back (`engine: null`).
    fn no_exception() -> torro_ipc::Request {
        torro_ipc::Request::SetAiAssignment {
            function: torro_core::ai::AiFunction::InboxCategorize,
            engine: None,
            model: None,
            effort: None,
            enabled: false,
        }
    }

    /// Issue #325 (task MW.3): picking a model sets the slot Sorting – no new exception after the
    /// slots took the old assignment over –, and takes an exception back after the slot; the switch
    /// "off" is the exception switched off with what sorts now, "on" takes it back, and fills an
    /// empty slot with what sorted last.
    #[test]
    fn the_model_goes_into_the_slot_and_the_switch_is_the_exception() {
        use categorization::Message;
        let ollama = "ai:ollama@localhost:11434";
        let mut by_slot = Data::default();
        by_slot.settings.ai_slots.decide = Some(slot("nimble"));
        assert!(categorization::in_effect(&by_slot).is_ready(), "the slot sorts");

        assert_eq!(sent(Message::Model("ai:x".into(), "tev1".into()), &by_slot), [set_slot("ai:x", "tev1")]);
        let off = sent(Message::Enabled(false), &by_slot);
        let [torro_ipc::Request::SetAiAssignment { engine, model, enabled, .. }] = off.as_slice() else {
            panic!("the exception switched off: {off:?}")
        };
        assert_eq!((engine.as_deref(), model.as_deref(), *enabled), (Some(ollama), Some("nimble"), false));

        // Switched off by the exception: the page says so, "on" takes the exception back.
        let switched_off = Data { assignments: exception("nimble", false), ..by_slot.clone() };
        assert!(!categorization::in_effect(&switched_off).is_ready());
        assert_eq!(sent(Message::Enabled(true), &switched_off), [no_exception()]);
        assert_eq!(
            sent(Message::Model("ai:x".into(), "tev1".into()), &switched_off),
            [no_exception(), set_slot("ai:x", "tev1")],
            "the exception goes first, then the slot"
        );
        // Without a slot, "on" puts what sorted last into it before the exception goes.
        let no_slot = Data { assignments: exception("nimble", false), ..Data::default() };
        assert_eq!(sent(Message::Enabled(true), &no_slot), [set_slot(ollama, "nimble"), no_exception()]);
        // Nothing sorts: nothing to switch off.
        assert!(sent(Message::Enabled(false), &Data::default()).is_empty());
    }

    /// The daemon of [`a_refused_take_back_sets_no_slot`]: refuses taking the exception back and notes
    /// every request it gets.
    struct Refusing(std::sync::Mutex<Vec<torro_ipc::Request>>);

    impl crate::Backend for Refusing {
        fn call(&self, request: torro_ipc::Request) -> crate::Answer {
            let refused = matches!(request, torro_ipc::Request::SetAiAssignment { engine: None, .. });
            self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(request);
            Box::pin(async move {
                match refused {
                    true => Err("Settings cannot be written.".to_string()),
                    false => Ok(torro_ipc::Response::Ok),
                }
            })
        }
    }

    /// Review of #325 (R6): picking a model while an exception stands sends the take-back first; the
    /// daemon refusing it stops there, so the slot is not set and nothing sorts in between.
    #[tokio::test]
    async fn a_refused_take_back_sets_no_slot() {
        use categorization::Message;
        let mut data = Data { assignments: exception("nimble", true), ..Data::default() };
        data.settings.ai_slots.decide = Some(slot("nimble"));
        let requests = sent(Message::Model("ai:x".into(), "tev1".into()), &data);
        let backend = std::sync::Arc::new(Refusing(std::sync::Mutex::new(Vec::new())));
        let failed = crate::send(backend.clone(), requests, Vec::new()).await;
        assert_eq!(failed.err().as_deref(), Some("Settings cannot be written."));
        let got = backend.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone();
        assert_eq!(got, [no_exception()], "no ai.set_slot after the refusal");
    }

    /// Task S.26 (and MW.3): only models turned on are offered for sorting mail – switching it on
    /// with nothing set puts the first one that is on into the slot, and with none on there is
    /// nothing to switch on.
    #[test]
    fn sorting_mail_offers_only_models_that_are_on() {
        use torro_core::ai::AiModel;

        let decider = |id: &str, enabled: bool| {
            let model = AiModel { decision: Some(true), enabled, remote: Some(false), ..AiModel::named(id) };
            ("ai:ollama@localhost:11434".to_string(), model)
        };
        let data = Data { decider_models: vec![decider("off", false), decider("on", true)], ..Data::default() };
        let on = sent(categorization::Message::Enabled(true), &data);
        assert_eq!(on, [set_slot("ai:ollama@localhost:11434", "on")], "the model that is off is not offered");
        let _ = categorization::view(&Default::default(), &data);

        let none = Data { decider_models: vec![decider("off", false)], ..Data::default() };
        assert!(sent(categorization::Message::Enabled(true), &none).is_empty(), "nothing on, nothing to switch on");
        let _ = categorization::view(&Default::default(), &none);
    }

    /// Task MW.3: chat models with tools are offered too, after nothing – a model that can do neither
    /// is not –; while sorted mail stays here only those that compute here, judged by the slot's own
    /// permission (`decide_mail_to_cloud`), not the chat's.
    #[test]
    fn chat_models_are_offered_for_sorting_and_only_here_while_mail_stays_here() {
        use torro_core::ai::AiModel;

        let ollama = "ai:ollama@localhost:11434";
        let openrouter = "ai:openrouter@openrouter.ai";
        let model = |engine: &str, id: &str, decision: Option<bool>, tools: Option<bool>, remote: Option<bool>| {
            (engine.to_string(), AiModel { decision, tools, remote, enabled: true, ..AiModel::named(id) })
        };
        let mut data = Data {
            decider_models: vec![
                model(ollama, "nimble", Some(true), Some(false), Some(false)),
                model(ollama, "qwen3:8b", Some(false), Some(true), Some(false)),
                model(ollama, "nomic-embed-text", Some(false), Some(false), Some(false)),
                model(ollama, "glm:cloud", Some(false), Some(true), Some(true)),
                model(openrouter, "qwen/qwen3-14b", None, Some(true), None),
                model("ai:lm_studio@localhost:1234", "gemma", None, None, None),
                model("ai:openai_compatible@127.0.0.1:8000", "mine", None, None, None),
                model("ai:openai_compatible@gpu.example.org", "far", None, None, None),
            ],
            ..Data::default()
        };
        let ids = |data: &Data| -> Vec<String> {
            categorization::offered(data).into_iter().map(|(_, model)| model.id).collect()
        };
        data.settings.mail_to_cloud = true;
        assert_eq!(ids(&data), ["nimble", "qwen3:8b", "gemma", "mine"], "the chat's permission opens nothing");
        data.settings.decide_mail_to_cloud = Some(true);
        assert_eq!(ids(&data), ["nimble", "qwen3:8b", "glm:cloud", "qwen/qwen3-14b", "gemma", "mine", "far"]);
        data.settings.ai_slots.decide = Some(slot("qwen3:8b"));
        let _ = categorization::view(&Default::default(), &data);
    }

    /// Task MW.3: an exception with a model of its own is what sorts, and the page shows it as such.
    #[test]
    fn an_exception_with_its_own_model_sorts_instead_of_the_slot() {
        use torro_core::ai::AiModel;

        let ollama = "ai:ollama@localhost:11434";
        let mut data = Data {
            assignments: exception("tev1", true),
            decider_models: ["nimble", "tev1"]
                .into_iter()
                .map(|id| (ollama.to_string(), AiModel { decision: Some(true), enabled: true, ..AiModel::named(id) }))
                .collect(),
            ..Data::default()
        };
        data.settings.ai_slots.decide = Some(slot("nimble"));
        let held = categorization::in_effect(&data);
        assert_eq!((held.model.as_deref(), held.enabled), (Some("tev1"), true));
        let _ = categorization::view(&Default::default(), &data);
    }

    /// PR review #242: a model assigned for sorting mail that is turned off in Accounts is not shown
    /// as sorting – the core sorts nothing then: its row says so, and "Manage models…" leads to the
    /// accounts. Not while the sorting is off, the model on, or the account's models not in yet.
    #[test]
    fn a_sorting_model_turned_off_says_so() {
        use torro_core::ai::{AiAssignment, AiFunction, AiModel};

        let engine = "ai:ollama@localhost:11434";
        let assigned = |enabled: bool| {
            vec![torro_ipc::AiAssignmentInfo {
                assignment: AiAssignment {
                    function: AiFunction::InboxCategorize,
                    engine: Some(engine.into()),
                    model: Some("nimble".into()),
                    effort: None,
                    enabled,
                },
                label: "Sort mail into categories".into(),
                needs_decision: true,
            }]
        };
        let decider = |id: &str, enabled: bool| {
            let name = format!("{id} (decides)");
            (engine.to_string(), AiModel { decision: Some(true), enabled, name, ..AiModel::named(id) })
        };
        let off = Data {
            assignments: assigned(true),
            decider_models: vec![decider("nimble", false), decider("other", true)],
            ..Data::default()
        };
        assert_eq!(categorization::assigned_off(&off), Some((engine.to_string(), "nimble (decides)".to_string())));
        let _ = categorization::view(&Default::default(), &off);
        let Action::ControlCentre(section) =
            categorization::update::<()>(&mut Default::default(), categorization::Message::ManageModels, &off)
        else {
            panic!("the control centre")
        };
        assert_eq!(section, crate::assistant::AI_SECTION);

        let gone = Data { decider_models: vec![decider("other", true)], ..off.clone() };
        assert_eq!(categorization::assigned_off(&gone), Some((engine.to_string(), "nimble".to_string())), "gone");
        let _ = categorization::view(&Default::default(), &gone);
        let on = Data { decider_models: vec![decider("nimble", true)], ..off.clone() };
        assert_eq!(categorization::assigned_off(&on), None);
        let unloaded = Data { decider_models: Vec::new(), ..off.clone() };
        assert_eq!(categorization::assigned_off(&unloaded), None, "not known yet");
        let switched_off = Data { assignments: assigned(false), ..off };
        assert_eq!(categorization::assigned_off(&switched_off), None, "sorting is off anyway");
    }

    /// Task E12.10: "Catch up now" starts the run, "Cancel" stops it.
    #[test]
    fn catching_up_starts_and_stops() {
        let data = Data::default();
        for (message, cancel) in
            [(categorization::Message::CatchUp(false), false), (categorization::Message::CatchUp(true), true)]
        {
            let Action::Send(requests) = categorization::update::<()>(&mut Default::default(), message, &data) else {
                panic!("a request")
            };
            assert_eq!(requests, vec![torro_ipc::Request::CatchUpCategories { cancel, again: false }]);
        }
        // Task E15.5: "Sort again" goes through the whole window anew.
        let message = categorization::Message::SortAgain;
        let Action::Send(requests) = categorization::update::<()>(&mut Default::default(), message, &data) else {
            panic!("a request")
        };
        assert_eq!(requests, vec![torro_ipc::Request::CatchUpCategories { cancel: false, again: true }]);
    }

    /// Task E15.5: the editor's page replaces the list, and "‹ Back" brings the list back.
    #[test]
    fn a_category_page_opens_over_the_list() {
        let categories = torro_mail::CategoryDef::defaults()
            .iter()
            .map(|def| torro_ipc::inbox::CategoryInfo::of(def, 0, 0))
            .collect();
        let data = Data { categories, ..Data::default() };
        let mut state = State::default();
        let first = data.categories.first().map(|info| info.category).expect("a category");
        let open =
            Message::Categorization(categorization::Message::Editor(super::editor::Message::EditCategory(first)));
        let _ = update(&mut state, open, &data);
        assert!(page(&state, &data, "categorization").sub_page.is_some());
        let back = Message::Categorization(categorization::Message::Editor(super::editor::Message::Back));
        let _ = update(&mut state, back, &data);
        assert!(page(&state, &data, "categorization").sub_page.is_none());
    }
}

/// "Categorization" (task E12.10, concept `docs/kategorien.md`): what incoming mail is sorted
/// into, and by which model.
///
/// ```text
/// Sort mail into categories   on/off
/// Model                       the models of the AI accounts that can sort: decision models, and
///                             chat models that answer in a fixed format (task MW.3)
/// Catch up                    how far the backlog has come, "Catch up now" / "Cancel"
/// Show in the sidebar         one switch per category
/// Rules ↗                     the kept senders, in the Notification Hub
/// ```
///
/// **No setting for the moment of sorting or the confidence threshold**: both are fixed
/// (`docs/kategorien.md`, "decisions instead of options").
///
/// **The slot and its exception** (issue #325, task MW.3): what the page shows is what sorts – the
/// exception `inbox.categorize` while one is stored, else the slot Sorting ([`in_effect`]). Picking a
/// model sets the slot (`ai.set_slot decide`) and takes an exception back, so no new exception is
/// made. The switch "off" stores the exception switched off, with the account and model in effect;
/// "on" takes it back (`engine: null`), and the slot sorts again. An exception with a model of its
/// own says so at its row ("Only for sorting mail").
///
/// Models that do not run on this computer are **not offered** while sorted mail may not leave it
/// (the permission of the slot Sorting, `decide_mail_to_cloud`; `docs/ki.md`, principle 4); a quiet
/// line says so. The core refuses such a model as well (task E12.5), so the rule holds even if this
/// list were wrong.
///
/// **A model turned off** (PR review #242): only models turned on in the control centre's
/// AI section are offered (task S.26). The assigned one turned off since stands on top of "Model"
/// with "Turned off in Settings › AI" and an alert in the attention colour – the core sorts
/// nothing then ([`assigned_off`]) –, and "Manage models…" ends the group, the way to turn it on.
pub mod categorization {
    use iced::Theme;
    use iced::widget::text::Wrapping;
    use iced::widget::{column, svg};
    use torro_core::ai::{AiAssignment, AiFunction, AiModel, AiSlot};
    use torro_core::i18n::trf;
    use torro_gui::icon;
    use torro_gui::theme::palette;
    use torro_gui::{layout, style, theme, typo, widgets};
    use torro_ipc::inbox::{CategoryJobControl, CategoryJobPreview, CategoryJobScope, CategoryJobState};
    use torro_ipc::{Request, Response};
    use torro_mail::Category;

    use super::*;
    use crate::parts::{choice, pressable, row_frame, setting_row, switch};

    /// Form choices remain local; the actual job and its progress always come from the service.
    #[derive(Debug, Default)]
    pub struct State {
        pub(super) editor: super::editor::State,
        tab: Tab,
        all_accounts: bool,
        accounts: Vec<String>,
        all_folders: bool,
        folders: Vec<String>,
        period: Period,
        again: bool,
        /// The bulk operation stays compact until the person chooses to set its scope.
        job_expanded: bool,
        preview: Option<CategoryJobPreview>,
        preview_error: Option<String>,
        pending_scope: Option<CategoryJobScope>,
        asking_clear: bool,
        /// `None` means the recommended choice: switch future automatic sorting off.
        disable_automatic_after_clear: Option<bool>,
    }

    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    enum Tab {
        #[default]
        Automatic,
        Categories,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
    pub enum Period {
        #[default]
        All,
        Days30,
        Days90,
        Year,
    }

    /// What a row of this section reports.
    #[derive(Debug, Clone)]
    pub enum Message {
        /// Switch between automatic sorting and the category catalogue.
        Tab(usize),
        /// The switch: sorting mail on or off.
        Enabled(bool),
        /// A model picked for sorting mail.
        Model(String, String),
        /// "Catch up now" (`false`) or "Cancel" (`true`).
        CatchUp(bool),
        /// "Sort again" (task E15.5): the whole window anew, after new rules or descriptions.
        SortAgain,
        ShowJob(bool),
        /// Change the scope of the explicit sorting job and refresh its preview.
        AllAccounts,
        ToggleAccount(String),
        AllFolders,
        ToggleFolder(String),
        Period(Period),
        Again(bool),
        StartJob,
        ControlJob(String, CategoryJobControl),
        AskClear,
        ConfirmClear,
        CloseClear,
        DisableAutomaticAfterClear(bool),
        ControlClear(String, CategoryJobControl),
        /// The editor of the categories and the sorting rules (task E15.5).
        Editor(super::editor::Message),
        /// One category shown in the sidebar or not.
        Shown(Category, bool),
        /// "Manage models…": which models are on is set in the control centre's accounts.
        ManageModels,
    }

    pub fn update<A>(state: &mut State, message: Message, data: &Data) -> Action<A> {
        match message {
            Message::Tab(index) => {
                if index == 1 && state.editor.is_rule_open() {
                    let action = super::editor::update(&mut state.editor, super::editor::Message::Back, data);
                    if state.editor.is_rule_open() {
                        return action;
                    }
                }
                state.tab = if index == 1 { Tab::Categories } else { Tab::Automatic };
                Action::None
            }
            Message::Editor(message) => {
                let action = super::editor::update(&mut state.editor, message, data);
                if state.editor.is_rule_open() {
                    state.tab = Tab::Automatic;
                }
                action
            }
            Message::AllAccounts => {
                state.all_accounts = true;
                state.accounts.clear();
                preview_action(state)
            }
            Message::ToggleAccount(account) => {
                state.all_accounts = false;
                toggle(&mut state.accounts, account);
                if state.accounts.is_empty() {
                    state.all_accounts = true;
                }
                preview_action(state)
            }
            Message::AllFolders => {
                state.all_folders = true;
                state.folders.clear();
                preview_action(state)
            }
            Message::ToggleFolder(folder) => {
                state.all_folders = false;
                toggle(&mut state.folders, folder);
                if state.folders.is_empty() {
                    state.all_folders = true;
                }
                preview_action(state)
            }
            Message::Period(period) => {
                state.period = period;
                preview_action(state)
            }
            Message::Again(again) => {
                state.again = again;
                preview_action(state)
            }
            Message::StartJob => Action::send(Request::StartCategoryJob { scope: scope(state) }),
            Message::ControlJob(id, action) => Action::send(Request::ControlCategoryJob { id, action }),
            Message::AskClear => {
                state.asking_clear = true;
                Action::None
            }
            Message::CloseClear => {
                state.asking_clear = false;
                Action::None
            }
            Message::DisableAutomaticAfterClear(on) => {
                state.disable_automatic_after_clear = Some(on);
                Action::None
            }
            Message::ConfirmClear => {
                state.asking_clear = false;
                Action::send(Request::ClearCategories {
                    all_accounts: true,
                    disable_automatic: state.disable_automatic_after_clear.unwrap_or(true),
                })
            }
            Message::ControlClear(id, action) => Action::send(Request::ControlClearCategories { id, action }),
            Message::SortAgain => Action::send(Request::CatchUpCategories { cancel: false, again: true }),
            Message::ShowJob(show) => {
                state.job_expanded = show;
                Action::None
            }
            Message::Enabled(true) => switched_on(data),
            // Off is the exception switched off, with what sorts now: it goes before the slot and
            // keeps sorting off, also while the slot changes. Nothing sorts, nothing to switch off.
            Message::Enabled(false) => {
                let held = in_effect(data);
                match held.engine {
                    Some(engine) => Action::send(Request::SetAiAssignment {
                        function: AiFunction::InboxCategorize,
                        engine: Some(engine),
                        model: held.model,
                        effort: held.effort,
                        enabled: false,
                    }),
                    None => Action::None,
                }
            }
            // Picking a model switches the sorting on: that is what picking one means. It goes into
            // the slot; an exception would stand before it, so it is taken back – first (review of
            // #325): setting the slot can let Sorting's mail out, and meanwhile an exception the person
            // stopped with "Keep mail here" would sort again. The requests go one after the other and
            // stop at the first refused ([`crate::send`]), so a refused take-back sets no slot.
            Message::Model(engine, model) => {
                let effort = data.settings.ai_slots.decide.as_ref().and_then(|slot| slot.effort);
                let mut requests: Vec<Request> = stored_exception(data).map(|_| no_exception()).into_iter().collect();
                requests.push(set_slot(engine, Some(model), effort));
                Action::Send(requests)
            }
            Message::CatchUp(cancel) => Action::send(Request::CatchUpCategories { cancel, again: false }),
            Message::Shown(category, shown) => Action::send(super::editor::shown_request(data, category, shown)),
            Message::ManageModels => Action::ControlCentre(crate::assistant::AI_SECTION.to_string()),
        }
    }

    /// The exception stored for sorting mail (`ai.set_assignment inbox.categorize`), on or off; `None`
    /// when none is stored – the core lists the function with empty fields then.
    fn stored_exception(data: &Data) -> Option<AiAssignment> {
        data.assignments
            .iter()
            .find(|info| info.assignment.function == AiFunction::InboxCategorize)
            .map(|info| info.assignment.clone())
            .filter(|held| held.engine.is_some())
    }

    /// What sorts mail now (issue #325), as the core weighs it: the exception while one is stored –
    /// one switched off keeps sorting off –, else the slot Sorting as an assignment that is on; nothing
    /// assigned when neither is set.
    pub fn in_effect(data: &Data) -> AiAssignment {
        let function = AiFunction::InboxCategorize;
        if let Some(own) = stored_exception(data) {
            return own;
        }
        match data.settings.ai_slots.get(AiSlot::Decide) {
            Some(slot) => AiAssignment {
                function,
                engine: Some(slot.engine.clone()),
                model: slot.model.clone(),
                effort: slot.effort,
                enabled: true,
            },
            None => AiAssignment::none(function),
        }
    }

    /// The exception with a model of its own: stored and on, so it sorts instead of the slot.
    fn own_exception(data: &Data) -> Option<AiAssignment> {
        stored_exception(data).filter(|held| held.enabled)
    }

    /// The switch "on": the exception switched off is taken back, so the slot sorts again. Without a
    /// slot, it gets what sorted last – the exception's model – or the first model offered; nothing
    /// to sort with, nothing to switch on. The slot first, so a refused one takes nothing back.
    fn switched_on<A>(data: &Data) -> Action<A> {
        let exception = stored_exception(data);
        let mut requests = Vec::new();
        if data.settings.ai_slots.decide.is_none() {
            let from_exception = exception
                .as_ref()
                .and_then(|held| held.engine.clone().map(|engine| set_slot(engine, held.model.clone(), held.effort)));
            let first =
                || offered(data).into_iter().next().map(|(engine, model)| set_slot(engine, Some(model.id), None));
            match from_exception.or_else(first) {
                Some(request) => requests.push(request),
                None => return Action::None,
            }
        }
        requests.extend(exception.map(|_| no_exception()));
        match requests.is_empty() {
            true => Action::None,
            false => Action::Send(requests),
        }
    }

    /// `ai.set_slot decide` with `engine` and `model`.
    fn set_slot(engine: String, model: Option<String>, effort: Option<torro_core::ai::Effort>) -> Request {
        Request::SetAiSlot { slot: AiSlot::Decide, engine: Some(engine), model, effort }
    }

    /// `ai.set_assignment inbox.categorize` with `engine: null`: the exception taken back.
    fn no_exception() -> Request {
        Request::SetAiAssignment {
            function: AiFunction::InboxCategorize,
            engine: None,
            model: None,
            effort: None,
            enabled: false,
        }
    }

    /// The section's page: the list, or the open page of a category or a sorting rule with
    /// "‹ Back" (task E15.5).
    pub fn page<'a>(state: &'a State, data: &'a Data) -> Page<'a, Message> {
        if state.editor.is_rule_open() {
            return Page::new(view(state, data));
        }
        match super::editor::page(&state.editor, data) {
            Some((name, body, save)) => {
                let back = Message::Editor(super::editor::Message::Back);
                Page::new(body.map(Message::Editor)).sub_page(name, back).actions(vec![save.map(Message::Editor)])
            }
            None => Page::new(view(state, data)),
        }
    }

    pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
        let tabs = widgets::tabs(
            vec![(tr("Automatic sorting").to_string(), None), (tr("Categories").to_string(), None)],
            usize::from(state.tab == Tab::Categories),
            Message::Tab,
        );
        if state.tab == Tab::Categories {
            let mut page = column![tabs].spacing(layout::section_gap());
            for group in super::editor::categories_groups(&state.editor, data) {
                page = page.push(group.map(Message::Editor));
            }
            return page.into();
        }
        let held = in_effect(data);
        let on = held.is_ready();
        let switch_row = setting_row(None, tr("Sort mail into categories"), None, switch(on, Message::Enabled));
        // The rules first: they decide before the model (the mockup's order, `docs/kategorien.md`).
        // "Always for this sender?" keeps its senders among them (task E15.3).
        let sorting_rules = super::editor::rules_group(&state.editor, data).map(Message::Editor);
        let mut model_choices = vec![switch_row];
        model_choices.extend(model_rows(data, &held));
        let cloud = match data.settings.mail_to_cloud_in(AiSlot::Decide) {
            true => tr(
                "The chosen model sorts mail. Mail may leave this computer; no message is moved or deleted, and categories can be changed by hand.",
            ),
            false => tr(
                "A model on your computer says what each mail is. No mail leaves this computer, nothing is hidden, moved or deleted, and you can change any category by hand.",
            ),
        };
        let note = format!("{cloud} {}", model_note(data));
        let models = widgets::group(tr("2. Model sorting"), model_choices, Some(note));
        column![tabs, sorting_rules, models, job_group(state, data)].spacing(layout::section_gap()).into()
    }

    fn toggle(selected: &mut Vec<String>, id: String) {
        if let Some(position) = selected.iter().position(|held| held == &id) {
            selected.remove(position);
        } else {
            selected.push(id);
        }
    }

    fn scope(state: &State) -> CategoryJobScope {
        let days = match state.period {
            Period::All => None,
            Period::Days30 => Some(30),
            Period::Days90 => Some(90),
            Period::Year => Some(365),
        };
        CategoryJobScope {
            accounts: if state.all_accounts { Vec::new() } else { state.accounts.clone() },
            folders: if state.all_folders { Vec::new() } else { state.folders.clone() },
            since: days.map(|days| chrono::Utc::now() - chrono::Duration::days(days)),
            before: None,
            again: state.again,
            engine: None,
            model: None,
        }
    }

    fn preview_action<A>(state: &mut State) -> Action<A> {
        state.preview = None;
        state.preview_error = None;
        let scope = scope(state);
        state.pending_scope = Some(scope.clone());
        Action::Ask(Request::PreviewCategoryJob { scope })
    }

    /// The host delivers the answer of the currently selected scope without replacing all
    /// settings data. The service alone counts; the form stores no derived totals.
    pub fn answered(state: &mut State, answer: &Result<Response, String>) {
        match answer {
            Ok(Response::CategoryJobPreview { preview }) if preview.request_scope == state.pending_scope => {
                state.preview = Some(preview.clone());
                state.preview_error = None;
                state.pending_scope = None;
            }
            Err(problem) => {
                state.preview_error = Some(problem.clone());
                state.pending_scope = None;
            }
            _ => {}
        }
    }

    fn job_group<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
        use iced::widget::column;
        if !state.job_expanded {
            let status = data.category_job.as_ref().map(|job| job_state_label(job.state).to_string());
            let title = status.map_or_else(
                || tr("Sort existing emails").to_string(),
                |status| trf("Sort existing emails · {status}", &[("status", &status)]),
            );
            let detail = data.category_job.as_ref().map_or_else(
                || tr("Choose time span, accounts and folders when needed.").to_string(),
                |job| {
                    let done: u64 = job.accounts.iter().map(|account| account.processed).sum();
                    let total: u64 = job.accounts.iter().map(|account| account.eligible).sum();
                    trf("{done} of {total}", &[("done", &done), ("total", &total)])
                },
            );
            let compact = widgets::group(
                tr("Existing emails"),
                vec![setting_row(
                    None,
                    title,
                    Some(detail),
                    widgets::text_button(None, tr("Set up…"), style::button::secondary, Some(Message::ShowJob(true))),
                )],
                None,
            );
            return column![compact, clear_group(state, data)].spacing(layout::section_gap()).into();
        }
        let all_accounts = state.all_accounts || state.accounts.is_empty();
        let mut account_rows: Vec<Element<'a, Message>> =
            vec![choice(tr("All mail accounts").to_string(), all_accounts, Message::AllAccounts)];
        for stats in &data.mail_stats {
            account_rows.push(choice(
                account_label(data, &stats.account),
                !all_accounts && state.accounts.contains(&stats.account),
                Message::ToggleAccount(stats.account.clone()),
            ));
        }
        let accounts = widgets::group(
            tr("Mail accounts"),
            account_rows,
            Some(tr("Choose one or more accounts, or all accounts.").to_string()),
        );

        let all_folders = state.all_folders || state.folders.is_empty();
        let mut folder_rows: Vec<Element<'a, Message>> =
            vec![choice(tr("All folders").to_string(), all_folders, Message::AllFolders)];
        for stats in &data.mail_stats {
            if !all_accounts && !state.accounts.contains(&stats.account) {
                continue;
            }
            for folder in &stats.folders {
                folder_rows.push(choice(
                    format!("{} · {}", account_label(data, &stats.account), folder.folder.name),
                    !all_folders && state.folders.contains(&folder.folder.id),
                    Message::ToggleFolder(folder.folder.id.clone()),
                ));
            }
        }
        let folders = widgets::group(tr("Folders to sort"), folder_rows, None);
        let period = widgets::group(
            tr("Time span"),
            vec![
                choice(tr("All emails").to_string(), state.period == Period::All, Message::Period(Period::All)),
                choice(tr("Last 30 days").to_string(), state.period == Period::Days30, Message::Period(Period::Days30)),
                choice(tr("Last 90 days").to_string(), state.period == Period::Days90, Message::Period(Period::Days90)),
                choice(tr("Last year").to_string(), state.period == Period::Year, Message::Period(Period::Year)),
                setting_row(
                    None,
                    tr("Recheck model categories"),
                    Some(tr("Manual choices and sorting rules remain protected.").to_string()),
                    switch(state.again, Message::Again),
                ),
            ],
            None,
        );
        let preview = if state.pending_scope.is_some() || state.preview_error.is_some() {
            None
        } else {
            state.preview.as_ref().or(data.category_job_preview.as_ref())
        };
        let preview_words = match (preview, state.preview_error.as_ref()) {
            (_, Some(problem)) => problem.clone(),
            (Some(preview), _) => trf(
                "{eligible} to sort · {unclassified} without a category · {automatic} automatic · {protected} protected · {total} total",
                &[
                    ("eligible", &preview.eligible),
                    ("unclassified", &preview.unclassified),
                    ("automatic", &preview.automatic),
                    ("protected", &preview.protected),
                    ("total", &preview.total),
                ],
            ),
            (None, _) => tr("Calculating selection…").to_string(),
        };
        let clear_incomplete = data.categorization.clear_job.as_ref().is_some_and(|job| {
            matches!(job.state, CategoryJobState::Running | CategoryJobState::Paused | CategoryJobState::Failed)
        });
        let running =
            clear_incomplete || data.category_job.as_ref().is_some_and(|job| job.state == CategoryJobState::Running);
        let start = widgets::text_button(
            None,
            tr("Start sorting"),
            style::button::secondary,
            (!running && preview.is_some_and(|preview| preview.eligible > 0)).then_some(Message::StartJob),
        );
        let summary = widgets::group(
            tr("Preview"),
            vec![
                row_frame(
                    column![
                        typo::text(typo::body_strong(), tr("Selected mail")),
                        typo::text(typo::body(), preview_words).wrapping(Wrapping::WordOrGlyph),
                    ]
                    .spacing(theme::space::xs()),
                    false,
                ),
                row_frame(start, false),
                setting_row(None, tr("Model location"), Some(job_model_note(data)), iced::widget::space().width(0)),
            ],
            Some(
                tr("The preview and progress count all selected mail. Sidebar badges count unread mail only.")
                    .to_string(),
            ),
        );
        let close =
            widgets::text_button(None, tr("Close selection"), style::button::ghost, Some(Message::ShowJob(false)));
        let mut page =
            column![row_frame(close, false), accounts, folders, period, summary].spacing(layout::section_gap());
        if let Some(job) = &data.category_job {
            let total: u64 = job.accounts.iter().map(|account| account.eligible).sum();
            let processed: u64 = job.accounts.iter().map(|account| account.processed).sum();
            let mut rows = vec![setting_row(
                None,
                tr("Sorting progress"),
                Some(trf("{done} of {total}", &[("done", &processed), ("total", &total)])),
                typo::text(typo::body_strong(), job_state_label(job.state)),
            )];
            for account in &job.accounts {
                rows.push(setting_row(
                    None,
                    account_label(data, &account.account),
                    Some(trf("{done} of {total}", &[("done", &account.processed), ("total", &account.eligible)])),
                    typo::text(typo::body(), account.changed.to_string()),
                ));
            }
            for error in &job.errors {
                rows.push(setting_row(None, tr("Error"), Some(error.clone()), iced::widget::space().width(0)));
            }
            let controls = match job.state {
                CategoryJobState::Running => vec![
                    widgets::text_button(
                        None,
                        tr("Pause"),
                        style::button::secondary,
                        Some(Message::ControlJob(job.id.clone(), CategoryJobControl::Pause)),
                    ),
                    widgets::text_button(
                        None,
                        tr("Cancel"),
                        style::button::ghost,
                        Some(Message::ControlJob(job.id.clone(), CategoryJobControl::Cancel)),
                    ),
                ],
                CategoryJobState::Paused | CategoryJobState::Failed | CategoryJobState::Cancelled => vec![
                    widgets::text_button(
                        None,
                        tr("Resume"),
                        style::button::secondary,
                        Some(Message::ControlJob(job.id.clone(), CategoryJobControl::Resume)),
                    ),
                    widgets::text_button(
                        None,
                        tr("Cancel"),
                        style::button::ghost,
                        Some(Message::ControlJob(job.id.clone(), CategoryJobControl::Cancel)),
                    ),
                ],
                CategoryJobState::Complete => Vec::new(),
            };
            if !controls.is_empty() {
                rows.push(iced::widget::row(controls).spacing(theme::space::m()).into());
            }
            page = page.push(widgets::group(tr("Current sorting job"), rows, None));
        }
        page = page.push(clear_group(state, data));
        page.into()
    }

    fn clear_group<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
        let mut rows: Vec<Element<'a, Message>> = Vec::new();
        let count = data.category_clear_preview.as_ref().map(|preview| preview.total).unwrap_or(0);
        let incomplete = data.categorization.clear_job.as_ref().is_some_and(|job| {
            matches!(job.state, CategoryJobState::Running | CategoryJobState::Paused | CategoryJobState::Failed)
        });
        let ask = widgets::text_button(
            None,
            tr("Remove all categories…"),
            style::button::secondary,
            (count > 0 && !incomplete).then_some(Message::AskClear),
        );
        rows.push(setting_row(
            None,
            tr("Categories on mail"),
            Some(trf(
                "{count} assignments across all accounts and folders",
                &[("count", &torro_gui::units::format_count(count))],
            )),
            ask,
        ));
        if state.asking_clear {
            if let Some(preview) = &data.category_clear_preview {
                for account in &preview.accounts {
                    rows.push(setting_row(
                        None,
                        account_label(data, &account.account),
                        Some(trf("{count} categories", &[("count", &account.categorized)])),
                        iced::widget::space().width(0),
                    ));
                }
            }
            let disable = state.disable_automatic_after_clear.unwrap_or(true);
            rows.push(setting_row(
                None,
                tr("Turn off automatic categorization afterwards"),
                Some(if disable {
                    tr("New mail stays uncategorized until you turn sorting on again.").to_string()
                } else {
                    tr("New mail may receive categories again after this reset.").to_string()
                }),
                switch(disable, Message::DisableAutomaticAfterClear),
            ));
            rows.push(row_frame(
                typo::text(
                    typo::body(),
                    tr("This removes manual, rule and model categories and model answers. Mail, folders and sorting rules remain."),
                )
                .wrapping(Wrapping::WordOrGlyph),
                false,
            ));
            rows.push(row_frame(
                iced::widget::row![
                    widgets::text_button(None, tr("Cancel"), style::button::ghost, Some(Message::CloseClear)),
                    widgets::text_button(
                        None,
                        tr("Remove categories from all mail"),
                        style::button::secondary,
                        Some(Message::ConfirmClear)
                    ),
                ]
                .spacing(theme::space::m()),
                false,
            ));
        }
        if let Some(job) = &data.categorization.clear_job {
            let removed: u64 = job.accounts.iter().map(|account| account.removed).sum();
            let selected: u64 = job.accounts.iter().map(|account| account.categorized).sum();
            rows.push(setting_row(
                None,
                tr("Reset progress"),
                Some(trf("{done} of {total}", &[("done", &removed), ("total", &selected)])),
                typo::text(typo::body_strong(), job_state_label(job.state)),
            ));
            for error in &job.errors {
                rows.push(setting_row(None, tr("Error"), Some(error.clone()), iced::widget::space().width(0)));
            }
            let controls = match job.state {
                CategoryJobState::Running => vec![
                    widgets::text_button(
                        None,
                        tr("Pause"),
                        style::button::secondary,
                        Some(Message::ControlClear(job.id.clone(), CategoryJobControl::Pause)),
                    ),
                    widgets::text_button(
                        None,
                        tr("Cancel"),
                        style::button::ghost,
                        Some(Message::ControlClear(job.id.clone(), CategoryJobControl::Cancel)),
                    ),
                ],
                CategoryJobState::Paused | CategoryJobState::Failed | CategoryJobState::Cancelled => vec![
                    widgets::text_button(
                        None,
                        tr("Resume"),
                        style::button::secondary,
                        Some(Message::ControlClear(job.id.clone(), CategoryJobControl::Resume)),
                    ),
                    widgets::text_button(
                        None,
                        tr("Cancel"),
                        style::button::ghost,
                        Some(Message::ControlClear(job.id.clone(), CategoryJobControl::Cancel)),
                    ),
                ],
                CategoryJobState::Complete => Vec::new(),
            };
            if !controls.is_empty() {
                rows.push(iced::widget::row(controls).spacing(theme::space::m()).into());
            }
        }
        widgets::group(
            tr("Reset categorization"),
            rows,
            Some(tr("Only category assignments and model answers are removed; no mail is deleted.").to_string()),
        )
    }

    fn job_state_label(state: CategoryJobState) -> &'static str {
        match state {
            CategoryJobState::Running => tr("Running"),
            CategoryJobState::Paused => tr("Paused"),
            CategoryJobState::Cancelled => tr("Cancelled"),
            CategoryJobState::Complete => tr("Complete"),
            CategoryJobState::Failed => tr("Failed"),
        }
    }

    fn job_model_note(data: &Data) -> String {
        if let Some(cloud) = data.category_job.as_ref().and_then(|job| job.model_in_cloud) {
            return match cloud {
                true => tr("Cloud model: mail content leaves this computer.").to_string(),
                false => tr("Local model: mail content stays on this computer.").to_string(),
            };
        }
        let held = in_effect(data);
        let here = data
            .decider_models
            .iter()
            .find(|(engine, model)| {
                held.engine.as_deref() == Some(engine.as_str()) && held.model.as_deref() == Some(model.id.as_str())
            })
            .map(|(engine, model)| stays_here(engine, model));
        match here {
            Some(false) => tr("Cloud model: mail content leaves this computer.").to_string(),
            Some(true) => tr("Local model: mail content stays on this computer.").to_string(),
            None => tr("Choose a model above.").to_string(),
        }
    }

    /// The models that may sort mail: every AI account's models that can – decision models, and
    /// chat models with tools through the fallback (task MW.3, `AiModel::can_decide`) – and are
    /// turned on for the suite (task S.26, `AiModel::enabled`), and – while sorted mail may not leave
    /// this computer – only those that run here (`docs/ki.md`, principle 4). The core refuses the
    /// others as well (task E12.5), so the rule holds even if this list were wrong.
    pub(super) fn offered(data: &Data) -> Vec<(String, AiModel)> {
        deciders(data).into_iter().filter(|(_, model)| model.enabled).collect()
    }

    /// The models that can sort mail and may see it, turned on or not.
    fn deciders(data: &Data) -> Vec<(String, AiModel)> {
        let local_only = !data.settings.mail_to_cloud_in(AiSlot::Decide);
        data.decider_models
            .iter()
            .filter(|(_, model)| model.can_decide())
            .filter(|(engine, model)| !local_only || stays_here(engine, model))
            .cloned()
            .collect()
    }

    /// Whether `model` of the AI account `engine` (`ai:<provider>@<host>`) computes on this computer,
    /// as far as the list tells: what its server says (`AiModel::remote`, Ollama and Strata), else
    /// by the account – LM Studio, or a server of one's own at an address of this computer. A cloud
    /// provider says nothing and counts as elsewhere. Only for what is offered; the core judges again.
    fn stays_here(engine: &str, model: &AiModel) -> bool {
        if let Some(remote) = model.remote {
            return !remote;
        }
        let (provider, host) = engine.strip_prefix("ai:").and_then(|rest| rest.split_once('@')).unwrap_or_default();
        let lm_studio = torro_core::ai::AiProvider::LmStudio.id();
        let own = torro_core::ai::AiProvider::OpenAiCompatible.id();
        provider == lm_studio || (provider == own && torro_core::ai::is_this_computer(host))
    }

    /// The model assigned for sorting mail when the sorting is on and that model of its AI account
    /// is turned off (PR review #242): the account's models are in and have it off, or lack it. The
    /// core sorts nothing then (task S.24). Its account and name as listed – the id where the list
    /// lacks it. `None` while the sorting is off, the model is on, "Automatic" is assigned, or the
    /// account's models are not in yet.
    pub fn assigned_off(data: &Data) -> Option<(String, String)> {
        let held = in_effect(data);
        let (engine, model) = (held.engine.filter(|_| held.enabled)?, held.model?);
        let mut listed = data.decider_models.iter().filter(|(account, _)| *account == engine).peekable();
        listed.peek()?;
        match listed.find(|(_, known)| known.id == model) {
            Some((_, known)) if known.enabled => None,
            Some((_, known)) => Some((engine, known.name.clone())),
            None => Some((engine, model)),
        }
    }

    /// One row per model that may sort mail, the one in effect checked; picking one switches the
    /// sorting on. A chat model says that it answers in a fixed format (the fallback, task MW.3), the
    /// model of an exception that it is only for sorting mail. Without any, one row says so instead:
    /// where to turn one on when there are models that can sort but none is on (task S.26), else where
    /// to add one. The model in effect turned off since comes first ([`assigned_off`]), "Manage
    /// models…" last.
    fn model_rows<'a>(data: &'a Data, held: &AiAssignment) -> Vec<Element<'a, Message>> {
        let mut offered = offered(data);
        offered.sort_by(|(a_engine, a_model), (b_engine, b_model)| {
            account_label(data, a_engine)
                .cmp(&account_label(data, b_engine))
                .then_with(|| a_model.name.cmp(&b_model.name))
        });
        let off = assigned_off(data);
        // Which account a model is from matters only where more than one offers models.
        let mut engines: std::collections::BTreeSet<&str> = offered.iter().map(|(engine, _)| engine.as_str()).collect();
        engines.extend(off.as_ref().map(|(engine, _)| engine.as_str()));
        let several = engines.len() > 1;
        let label = |engine: &str, name: &str| match several {
            true => format!("{} · {}", account_label(data, engine), name),
            false => name.to_string(),
        };
        let mut rows: Vec<Element<'a, Message>> =
            off.iter().map(|(engine, name)| off_row(label(engine, name))).collect();
        if offered.is_empty() && off.is_none() {
            let detail = match deciders(data).is_empty() {
                true => tr("Add a local AI such as Ollama under AI and load a model that decides."),
                false => tr("Turn on models in Settings › AI."),
            };
            let none = iced::widget::space().width(0);
            rows.push(setting_row(None, tr("No model that can sort mail"), Some(detail.to_owned()), none));
        }
        let own = own_exception(data);
        let mut last_provider = None;
        for (engine, model) in offered {
            if several && last_provider.as_deref() != Some(engine.as_str()) {
                rows.push(row_frame(typo::text(typo::body_strong(), account_label(data, &engine)), false));
                last_provider = Some(engine.clone());
            }
            let is = |held: &AiAssignment| {
                held.engine.as_deref() == Some(engine.as_str()) && held.model.as_deref() == Some(model.id.as_str())
            };
            let chosen = held.is_ready() && is(held);
            let notes: Vec<&str> = [
                own.as_ref().filter(|own| is(own)).map(|_| tr("Only for sorting mail")),
                model.decides_by_chat().then(|| tr("Answers in a fixed format, slower")),
            ]
            .into_iter()
            .flatten()
            .collect();
            let detail = (!notes.is_empty()).then(|| notes.join(" · "));
            // Under a provider's heading the provider is not repeated in the name.
            let name = if several { model.name.clone() } else { label(&engine, &model.name) };
            rows.push(model_choice(name, detail, chosen, Message::Model(engine, model.id)));
        }
        // Which models are offered is set where the accounts are (task S.26), the whole suite at once.
        let arrow = icon::icon(Icon::ArrowUpRight, icon::SIZE)
            .style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) });
        rows.push(pressable(setting_row(None, tr("Manage models…"), None, arrow), Some(Message::ManageModels)));
        rows
    }

    /// One model to pick, like [`choice`], with a line under its name where it has one: the row is
    /// the button, a check at the end marks the one that sorts.
    fn model_choice<'a>(
        label: String,
        detail: Option<String>,
        chosen: bool,
        on_press: Message,
    ) -> Element<'a, Message> {
        let check: Element<'a, Message> = match chosen {
            true => icon::icon(Icon::Check, icon::SIZE)
                .style(|t: &Theme, _| svg::Style { color: Some(palette(t).text) })
                .into(),
            false => iced::widget::space().width(icon::SIZE).into(),
        };
        pressable(setting_row(None, label, detail, check), Some(on_press))
    }

    /// The assigned model turned off (PR review #242), as a row of "Model": an alert in the attention
    /// colour before its name – the mail goes unsorted, which is worth noticing, not only reading –,
    /// why, and the check of the stored choice. No button: it is turned on under "Manage models…".
    fn off_row<'a>(label: String) -> Element<'a, Message> {
        let alert = icon::icon(Icon::CircleAlert, icon::SIZE)
            .style(|t: &Theme, _| svg::Style { color: Some(palette(t).attention_text) });
        let check =
            icon::icon(Icon::Check, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text) });
        let detail = tr("Turned off in Settings › AI").to_string();
        setting_row(Some(alert.into()), label, Some(detail), check)
    }

    /// What the account `id` is called, as the control centre calls it; the id itself when it is
    /// gone from the list.
    fn account_label(data: &Data, id: &str) -> String {
        data.accounts
            .iter()
            .find(|account| account.id == id)
            .map(|account| account.label.clone())
            .unwrap_or_else(|| id.to_string())
    }

    /// Under the models: why the list may be short.
    fn model_note(data: &Data) -> String {
        match data.settings.mail_to_cloud_in(AiSlot::Decide) {
            true => tr("Mail may go to an AI elsewhere for sorting; every model that can sort it is offered.").to_owned(),
            false => tr(
                "Only models on this computer are offered: mail stays here unless you choose a model elsewhere for Sorting in Settings › AI.",
            )
            .to_owned(),
        }
    }
}
