//! The sections behind the gear of the inbox's mail window (task S.20 in
//! `docs/tasks/steuerzentrale.md`, concept `docs/settings.md`): what belongs to writing and
//! reading mail.
//!
//! ```text
//! Accounts  one row per mail account; its page holds the sender name, the signature,
//!           "Write new mail as" and the folder questions the server left open
//! Reading   "Show mail as" and "Mail colors" for the reader
//! Attachments  what a click on an attachment does, the Downloads folder, the save targets
//!              (task M12.7, [`attachments`])
//! ```
//!
//! Task E10.3 adds a third section, "Keyboard shortcuts"; it is not built here.
//!
//! **What is not here:** everything about the account itself – servers, password, services,
//! removing it – and which new mail becomes an inbox entry. Servers and passwords stay in the
//! control centre, where the keychain is ([`crate::Action::ControlCentre`] leads there); "Show new
//! mail" belongs to the Notification Hub's sources ([`crate::inbox::sources`]), so it has one
//! place only.
//!
//! Ported from the control centre's mail account page and its group "Mail" under Appearance, which
//! task S.21 removes.

use iced::Element;

use torro_core::i18n::tr;
use torro_gui::icon::Icon;

use crate::{Action, Data, Page, Section};

pub mod attachments;
mod backup;
mod editor;

/// The sections, in the order of the list. [`Section::id`] is what `--settings <id>` takes.
pub const SECTIONS: [Section; 5] = [
    // Accounts is first: the gear lands on who the mail comes from.
    Section { id: ACCOUNTS_SECTION, icon: Icon::Mail, label: "Accounts" },
    Section { id: "reading", icon: Icon::MailOpen, label: "Reading" },
    // What a click on an attachment does and where it is saved (task M12.7, issue #290); "Manage
    // save targets…" in the menu of an attachment opens it ([`ATTACHMENTS_SECTION`]).
    Section { id: ATTACHMENTS_SECTION, icon: Icon::Paperclip, label: "Attachments" },
    // What incoming mail is sorted into (task E12.10, `docs/kategorien.md`).
    Section { id: "categorization", icon: Icon::Tag, label: "Categorization" },
    Section { id: "storage", icon: Icon::Archive, label: "Storage & index" },
];

/// The id of the section "Attachments": what `--settings attachments` takes and where "Manage save
/// targets…" leads.
pub const ATTACHMENTS_SECTION: &str = "attachments";

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
    ]
}

/// The state of every section that keeps some; the app owns one of these next to its host.
#[derive(Debug, Default)]
pub struct State {
    /// Accounts: which account's page is open and what is typed into it. Reading keeps nothing.
    pub accounts: accounts::State,
    /// Categorization: the page of a category or a sorting rule that is open (task E15.5).
    pub categorization: categorization::State,
    pub storage: storage::State,
    /// Attachments: the editor of a save target while it is open (task M12.7).
    pub attachments: attachments::State,
}

/// What the sections report; the app wraps it in its own message.
#[derive(Debug, Clone)]
pub enum Message {
    Accounts(accounts::Message),
    Reading(reading::Message),
    Categorization(categorization::Message),
    Storage(storage::Message),
    Attachments(attachments::Message),
}

/// What a section's widget reported, and what the host is to do about it.
pub fn update(state: &mut State, message: Message, data: &Data) -> Action<AppAction> {
    match message {
        Message::Attachments(message) => attachments::update(&mut state.attachments, message, data),
        Message::Accounts(message) => accounts::update(&mut state.accounts, message, data),
        Message::Reading(message) => reading::update(message),
        Message::Categorization(message) => categorization::update(&mut state.categorization, message, data),
        // "Show account" opens another section's page, so it is taken before the section sees it.
        Message::Storage(storage::Message::ShowAccount(id)) => {
            accounts::show(&mut state.accounts, &id, data);
            Action::Open(ACCOUNTS_SECTION)
        }
        Message::Storage(message) => storage::update(&mut state.storage, message, data),
    }
}

/// The page of the section with this id, as the host takes it. An id no section has shows
/// Accounts, the section the gear lands on; the host only ever asks for one of [`SECTIONS`].
pub fn page<'a>(state: &'a State, data: &'a Data, section: &str) -> Page<'a, Message> {
    match section {
        "reading" => wrap(Page::new(reading::view(data)), Message::Reading),
        "categorization" => wrap(categorization::page(&state.categorization, data), Message::Categorization),
        "storage" => wrap(storage::page(&state.storage, data), Message::Storage),
        ATTACHMENTS_SECTION => wrap(Page::new(attachments::view(&state.attachments, data)), Message::Attachments),
        _ => wrap(accounts::page(&state.accounts, data), Message::Accounts),
    }
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

/// A change or a load of the host came back (the window calls it on every one): what was being
/// saved is saved or failed, so the save bar of an account's page takes clicks again.
pub fn sent(state: &mut State) {
    accounts::sent(&mut state.accounts);
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

/// The answer of a one-off preview request for the currently selected sorting scope.
pub fn answered(state: &mut State, answer: &Result<torro_ipc::Response, String>) {
    categorization::answered(&mut state.categorization, answer);
    storage::answered(&mut state.storage, answer);
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
    }
}

/// "Accounts": one row per mail account, and below it the account's own page – the sender name,
/// the signature, how new mail is written and the folder questions the server left open.
///
/// The sender name and the signature are typed into a draft and saved with "Save"
/// (`accounts.update_mail_account`); everything else is sent the moment it is pressed, like every
/// other settings row. The draft measures itself against the stored account of the fresh [`Data`],
/// so "Save" greys out by itself once a change is stored and what is still typed stays typed.
pub mod accounts {
    use std::collections::BTreeMap;

    use chrono::{DateTime, Utc};
    use iced::Alignment;
    use iced::widget::text::Wrapping;
    use iced::widget::{column, row, space as blank, text_editor};
    use torro_core::i18n::trf;
    use torro_gui::datetime::{format_exact, format_moment, month_name, month_name_short};
    use torro_gui::theme::{self, ChartColor, size, space};
    use torro_gui::units::{format_bytes, format_count};
    use torro_gui::widgets::{
        Align, Anchor, ChartColumn, ChartSeries, HeadActions, MenuItem, MenuSection, PageAction, Placement, SharePart,
        StatFigure, Status, TileFace, Trail,
    };
    use torro_gui::{layout, style, typo, widgets};
    use torro_ipc::mail::{MailAccountStats, MailFolderStats, MailMonthCount};
    use torro_ipc::{AccountInfo, MailView, OpenRoleView, Request};
    use torro_mail::{MailFormat, Role};

    use super::*;
    use crate::parts::{chevron, field, picker, pressable, setting_row};

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
        /// "Save" sent the name and the signature and the answer has not come yet: the save bar
        /// says "Saving…" and takes no second click.
        saving: bool,
    }

    /// The parts of an account's page (task DS.6), "Overview" first.
    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub enum Tab {
        #[default]
        Overview,
        Writing,
        Folders,
        Backup,
    }

    impl Tab {
        /// The tabs in their order.
        pub const ALL: [Tab; 4] = [Tab::Overview, Tab::Writing, Tab::Folders, Tab::Backup];

        fn label(self) -> &'static str {
            match self {
                Tab::Overview => tr("Overview"),
                Tab::Writing => tr("Writing"),
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
        /// Typing in the signature.
        Signature(text_editor::Action),
        /// "Save" in the save bar: the sender name and the signature, as far as they differ from the
        /// stored ones.
        Save,
        /// "Write new mail as" was set (issue #125).
        Format(MailFormat),
        /// "Use this folder" for a role the server left open.
        PickFolder { role: Role, path: String },
        /// A row of the account's groups "Backup" and "Restore".
        Backup(backup::Message),
    }

    /// The sender name and the signature of one account as they are being typed.
    #[derive(Debug)]
    struct Draft {
        account: String,
        sender_name: String,
        signature: text_editor::Content,
    }

    impl Draft {
        fn new(account: &str, mail: &MailView) -> Draft {
            Draft {
                account: account.to_string(),
                sender_name: mail.sender_name.clone(),
                signature: text_editor::Content::with_text(mail.settings.signature.as_deref().unwrap_or_default()),
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
                    state.draft = Some(Draft::new(&id, mail));
                    state.tab = Tab::Overview;
                    state.more = false;
                }
                Action::None
            }
            Message::Back => {
                state.draft = None;
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
                Action::Open("storage")
            }
            Message::ServerSettings => {
                state.more = false;
                Action::ControlCentre("accounts".to_string())
            }
            Message::Discard => {
                if let Some(draft) = &state.draft
                    && let Some(mail) = mail_of(data, &draft.account)
                {
                    state.draft = Some(Draft::new(&draft.account, mail));
                }
                Action::None
            }
            Message::SenderName(name) => {
                if let Some(draft) = &mut state.draft {
                    draft.sender_name = name;
                }
                Action::None
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

    /// A change or a load came back: "Save" is done, the save bar shows what is still different.
    pub(super) fn sent(state: &mut State) {
        state.saving = false;
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

    /// The open draft with the stored account it measures itself against; `None` while the list is
    /// shown or the account is gone.
    fn open_draft<'a>(state: &'a State, data: &'a Data) -> Option<(&'a Draft, &'a MailView)> {
        let draft = state.draft.as_ref()?;
        Some((draft, mail_of(data, &draft.account)?))
    }

    /// The section's page: the list of accounts, or the open account's page below it.
    pub fn page<'a>(state: &'a State, data: &'a Data) -> Page<'a, Message> {
        match open_draft(state, data) {
            Some((draft, mail)) => {
                let label = account_label(data, &draft.account);
                let stats = data.mail_stats.iter().find(|stats| stats.account == draft.account);
                let page = Page::new(account_page(state, draft, mail, stats)).sub_page(label, Message::Back);
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

    /// One row per mail account, each opening its page; a quiet hint without any.
    fn list(data: &Data) -> Element<'_, Message> {
        let accounts = mail_accounts(data);
        if accounts.is_empty() {
            return widgets::empty_state(
                tr("No mail account"),
                tr("Add a mail account in Torro Workplace, under Accounts."),
            );
        }
        let rows = accounts
            .into_iter()
            .map(|(account, mail)| {
                let name = Some(mail.sender_name.clone()).filter(|name| !name.trim().is_empty());
                let row = setting_row(None, account.label.as_str(), name, chevron());
                pressable(row, Some(Message::Open(account.id.clone())))
            })
            .collect();
        let footer = tr("The name and signature of each account, and how its mail is written.");
        widgets::group(tr("Accounts"), rows, Some(footer.to_string()))
    }

    /// One account's page (task DS.6): the head with the address, how it syncs and "Sync now", the
    /// tabs, and the tab that is open.
    fn account_page<'a>(
        state: &'a State,
        draft: &'a Draft,
        mail: &'a MailView,
        stats: Option<&'a MailAccountStats>,
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
            Tab::Writing => writing(draft, mail),
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
        state.more.then_some(Message::CloseMore)
    }

    /// The tab "Overview": the open questions, the figures, storage, mail over time, the search
    /// index and the largest folders.
    fn overview<'a>(
        mail: &'a MailView,
        stats: Option<&'a MailAccountStats>,
        now: DateTime<Utc>,
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
        page.push(widgets::stat_grid(figures(stats)))
            .push(storage_card(stats))
            .push(time_card(stats, now))
            .push(index_card(stats, now))
            .push(largest_card(stats))
            .into()
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

    /// The four figures of the overview: mail, on the server, on this device, searchable.
    pub(super) fn figures(stats: &MailAccountStats) -> Vec<StatFigure> {
        vec![
            StatFigure::new(
                tr("Mail"),
                format_count(stats.messages),
                trf("{n} unread", &[("n", &format_count(stats.unread))]),
            ),
            server_figure(stats),
            StatFigure::new(
                tr("On this device"),
                format_bytes(stats.database_bytes),
                trf("{n} texts offline", &[("n", &format_count(stats.cached_bodies))]),
            ),
            StatFigure::new(
                tr("Searchable"),
                widgets::share_percent(stats.indexed_full, stats.messages),
                trf("{n} with full text", &[("n", &format_count(stats.indexed_full))]),
            ),
        ]
    }

    /// "On the server": the quota with its limit where the server tells it, else the sum of the
    /// folders' sizes without "of …" (decided in the planning of #314).
    pub(super) fn server_figure(stats: &MailAccountStats) -> StatFigure {
        let label = tr("On the server");
        if let Some(quota) = &stats.quota {
            let note = trf(
                "of {total} · {share}",
                &[
                    ("total", &format_bytes(quota.limit_bytes)),
                    ("share", &widgets::share_percent(quota.used_bytes, quota.limit_bytes)),
                ],
            );
            return StatFigure::new(label, format_bytes(quota.used_bytes), note);
        }
        let known: Vec<u64> = stats.folders.iter().filter_map(|folder| folder.server_bytes).collect();
        if known.is_empty() {
            return StatFigure::new(label, UNKNOWN, tr("Sizes come with the next sync"));
        }
        let note = if known.len() < stats.folders.len() {
            tr("Some sizes come with the next sync").to_string()
        } else {
            trf("in {n} folders", &[("n", &known.len())])
        };
        StatFigure::new(label, format_bytes(known.iter().sum()), note)
    }

    /// What a cell shows for a value that is not known yet.
    const UNKNOWN: &str = "–";

    /// The server's strip: the three largest folders, then "{n} other folders", and the free room
    /// of the quota. The three keep the first three colours in their order: what the strip shows
    /// is which folders are largest.
    pub(super) fn server_parts(stats: &MailAccountStats) -> (Vec<SharePart>, Option<(String, u64)>) {
        let mut sized: Vec<(&str, u64)> = stats
            .folders
            .iter()
            .filter_map(|folder| Some((folder.folder.name.as_str(), folder.server_bytes?)))
            .collect();
        sized.sort_by_key(|(_, bytes)| std::cmp::Reverse(*bytes));
        let shown = sized.len().min(LARGEST_IN_STRIP);
        let mut parts: Vec<SharePart> = sized[..shown]
            .iter()
            .enumerate()
            .map(|(slot, (name, bytes))| SharePart::new(*name, *bytes, format_bytes(*bytes), ChartColor::Series(slot)))
            .collect();
        let others = &sized[shown..];
        if !others.is_empty() {
            let bytes = others.iter().map(|(_, bytes)| bytes).sum();
            // One folder left over keeps its own name; several are counted.
            let label = match others {
                [(name, _)] => (*name).to_string(),
                _ => trf("{n} other folders", &[("n", &others.len())]),
            };
            parts.push(SharePart::new(label, bytes, format_bytes(bytes), ChartColor::Other));
        }
        // With a quota the strip is the quota: what is used but no folder accounts for (sizes still
        // coming, labels counted twice by Gmail) is a part of its own, so "Free" is the free share.
        if let Some(quota) = &stats.quota {
            let counted: u64 = sized.iter().map(|(_, bytes)| bytes).sum();
            let rest = quota.used_bytes.saturating_sub(counted);
            if rest > 0 {
                parts.push(SharePart::new(tr("Not counted by folder"), rest, format_bytes(rest), ChartColor::Other));
            }
        }
        let free = stats
            .quota
            .as_ref()
            .map(|quota| (tr("Free").to_string(), quota.limit_bytes.saturating_sub(quota.used_bytes)));
        (parts, free)
    }

    /// How many folders the server's strip names before the rest is "other folders".
    const LARGEST_IN_STRIP: usize = 3;

    /// The device's strip: search index, mail texts, originals and the rest of the database; never
    /// below 0. Without the size of the index the rest says that it holds the index too.
    pub(super) fn device_parts(stats: &MailAccountStats) -> Vec<SharePart> {
        let part =
            |label: &str, bytes: u64, slot| SharePart::new(label, bytes, format_bytes(bytes), ChartColor::Series(slot));
        let mut parts = Vec::new();
        if let Some(index) = stats.search_index_bytes {
            parts.push(part(tr("Search index"), index, 0));
        }
        parts.push(part(tr("Mail texts"), stats.body_cache_bytes, 1));
        parts.push(part(tr("Originals"), stats.original_bytes, 2));
        let counted = stats.search_index_bytes.unwrap_or(0) + stats.body_cache_bytes + stats.original_bytes;
        let rest = stats.database_bytes.saturating_sub(counted);
        let label =
            if stats.search_index_bytes.is_some() { tr("Headers & folders") } else { tr("Index, headers & folders") };
        parts.push(part(label, rest, 3));
        parts
    }

    /// "Storage": the server and the device as strips, the attachments under them, and the way to
    /// the page that manages them.
    fn storage_card<'a>(stats: &MailAccountStats) -> Element<'a, Message> {
        let caption = |text: &'static str| typo::text(typo::caption(), text).style(theme::text_3);
        let (server, free) = server_parts(stats);
        let server: Element<'a, Message> = if server.is_empty() && free.is_none() {
            typo::text(typo::meta(), tr("Sizes on the server come with the next sync.")).style(theme::text_2).into()
        } else {
            widgets::share_strip(server, free)
        };
        let mut body = column![
            column![caption(tr("On the server")), server].spacing(space::s()),
            column![caption(tr("On this device")), widgets::share_strip(device_parts(stats), None)].spacing(space::s()),
        ]
        .spacing(space::l());
        if stats.cached_attachments > 0 {
            let attachments = trf(
                "Attachments on this device: {n} · {size}",
                &[
                    ("n", &format_count(stats.cached_attachments)),
                    ("size", &format_bytes(stats.attachment_cache_bytes.unwrap_or(0))),
                ],
            );
            body = body.push(typo::text(typo::meta(), attachments).style(theme::text_2));
        }
        let manage = widgets::text_button(
            None,
            tr("Manage in Storage & index"),
            style::button::ghost,
            Some(Message::OpenStorage),
        );
        // Every follow-up of a card stands at its bottom right, like "Change" and "All folders".
        widgets::chart_card(tr("Storage"), "", body.push(row![blank::horizontal(), manage]))
    }

    /// "Mail over time": the mail per year – per month when it all lies within two years –, readable
    /// offline or headers only, then the oldest and newest mail and the offline window.
    fn time_card<'a>(stats: &MailAccountStats, now: DateTime<Utc>) -> Element<'a, Message> {
        let mut body = column![].spacing(space::m());
        let columns = time_columns(&stats.months);
        if columns.is_empty() {
            body = body.push(typo::text(typo::meta(), tr("No mail yet.")).style(theme::text_2));
        } else {
            let series = vec![
                ChartSeries { name: tr("Readable offline").to_string(), color: ChartColor::Series(0) },
                ChartSeries { name: tr("Headers only").to_string(), color: ChartColor::Series(1) },
            ];
            body = body.push(widgets::chart_legend(&series)).push(widgets::column_chart(series, columns));
        }
        let moment =
            |at: Option<DateTime<Utc>>| at.map_or_else(|| tr("Not known").to_string(), |at| format_moment(at, now));
        let range = trf(
            "Oldest {oldest} · newest {newest}",
            &[("oldest", &moment(stats.oldest_known)), ("newest", &moment(stats.newest_known))],
        );
        body = body.push(dated(typo::text(typo::meta(), range).style(theme::text_2), stats.oldest_known));
        let window = match stats.oldest_cached_body {
            Some(at) => trf("Readable offline from {moment}", &[("moment", &format_moment(at, now))]),
            None => tr("No mail texts on this device yet.").to_string(),
        };
        let change = widgets::text_button(None, tr("Change"), style::button::ghost, Some(Message::OpenStorage));
        let window = row![
            dated(typo::text(typo::meta(), window).style(theme::text_2), stats.oldest_cached_body),
            blank::horizontal(),
            change
        ]
        .spacing(space::s())
        .align_y(Alignment::Center);
        widgets::chart_card(tr("Mail over time"), "", body.push(window))
    }

    /// `content` with the exact moment `at` as its tooltip, when there is one.
    fn dated<'a>(content: impl Into<Element<'a, Message>>, at: Option<DateTime<Utc>>) -> Element<'a, Message> {
        match at {
            Some(at) => widgets::with_tooltip(content, format_exact(at)),
            None => content.into(),
        }
    }

    /// The columns of "Mail over time": one per year from the first to the last with mail, or one per
    /// month when they all lie within two years; gaps count as 0.
    pub(super) fn time_columns(months: &[MailMonthCount]) -> Vec<ChartColumn> {
        let (Some(first), Some(last)) =
            (months.iter().min_by_key(|m| (m.year, m.month)), months.iter().max_by_key(|m| (m.year, m.month)))
        else {
            return Vec::new();
        };
        let values = |count: (u64, u64)| {
            let offline = u32::try_from(count.1).unwrap_or(u32::MAX);
            let headers = u32::try_from(count.0.saturating_sub(count.1)).unwrap_or(u32::MAX);
            vec![offline, headers]
        };
        if last.year - first.year <= 1 {
            let mut columns = Vec::new();
            let (mut year, mut month) = (first.year, first.month);
            while (year, month) <= (last.year, last.month) {
                let count = months
                    .iter()
                    .filter(|m| m.year == year && m.month == month)
                    .fold((0, 0), |sum, m| (sum.0 + m.messages, sum.1 + m.cached_bodies));
                columns.push(ChartColumn {
                    label: month_name_short(month).to_string(),
                    title: trf("{month} {year}", &[("month", &month_name(month)), ("year", &year)]),
                    values: values(count),
                });
                (year, month) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
            }
            return columns;
        }
        (first.year..=last.year)
            .map(|year| {
                let count = months
                    .iter()
                    .filter(|m| m.year == year)
                    .fold((0, 0), |sum, m| (sum.0 + m.messages, sum.1 + m.cached_bodies));
                ChartColumn { label: year.to_string(), title: year.to_string(), values: values(count) }
            })
            .collect()
    }

    /// "Search index": how far the mail is searchable, and when the index last ran.
    fn index_card<'a>(stats: &MailAccountStats, now: DateTime<Utc>) -> Element<'a, Message> {
        let part = |label: &str, count: u64, colour| SharePart::new(label, count, format_count(count), colour);
        let parts = vec![
            part(tr("Full text"), stats.indexed_full, ChartColor::Series(0)),
            part(tr("Partial or unverified"), stats.indexed_partial_or_unverified, ChartColor::Series(1)),
            part(tr("Awaiting"), stats.awaiting_index, ChartColor::Series(2)),
            part(tr("Text unavailable"), stats.text_unavailable, ChartColor::Series(3)),
        ];
        let note = stats
            .last_index_run
            .map_or_else(String::new, |at| trf("Last run {moment}", &[("moment", &format_moment(at, now))]));
        widgets::chart_card(tr("Search index"), note, widgets::share_strip(parts, None))
    }

    /// "Largest folders": the five largest as a table, and the way to all of them.
    fn largest_card<'a>(stats: &MailAccountStats) -> Element<'a, Message> {
        let mut folders: Vec<&MailFolderStats> = stats.folders.iter().collect();
        folders.sort_by(|a, b| {
            (b.server_bytes.unwrap_or(0), b.folder.total).cmp(&(a.server_bytes.unwrap_or(0), a.folder.total))
        });
        let rows = folders
            .iter()
            .take(LARGEST_IN_TABLE)
            .map(|folder| {
                vec![
                    folder.folder.name.clone(),
                    format_count(u64::from(folder.folder.total)),
                    format_count(u64::from(folder.folder.unread)),
                    widgets::share_percent(u64::from(folder.folder.total), stats.messages),
                    folder.server_bytes.map_or_else(|| UNKNOWN.to_string(), format_bytes),
                ]
            })
            .collect();
        let head = [tr("Folder"), tr("Mail"), tr("Unread"), tr("Of all mail"), tr("On the server")]
            .map(str::to_string)
            .to_vec();
        let all = widgets::text_button(
            None,
            trf("All {n} folders", &[("n", &stats.folders.len())]),
            style::button::ghost,
            Some(Message::Tab(Tab::Folders.index())),
        );
        let body = column![widgets::data_table(head, rows, None), row![blank::horizontal(), all]].spacing(space::s());
        widgets::chart_card(tr("Largest folders"), "", body)
    }

    /// How many folders the overview's table shows.
    const LARGEST_IN_TABLE: usize = 5;

    /// The tab "Writing": sender name, signature and "Write new mail as" as pairs with what they are
    /// for. Name and signature save through the save bar; the format is sent at once.
    fn writing<'a>(draft: &'a Draft, mail: &'a MailView) -> Element<'a, Message> {
        let dirty = draft.changes(mail).is_some();
        let name = widgets::setting_pair(
            tr("Sender name"),
            tr("Shown next to your address in the mail you send. Optional."),
            move || {
                field("Anna Example", &draft.sender_name)
                    .on_input(Message::SenderName)
                    .on_submit_maybe(dirty.then_some(Message::Save))
                    .into()
            },
        );
        let signature =
            widgets::setting_pair(tr("Signature"), tr("Added under new mail, replies and forwards."), move || {
                text_editor(&draft.signature)
                    .on_action(Message::Signature)
                    .font(typo::body().font)
                    .size(typo::body().size)
                    .line_height(typo::body().line_height)
                    .padding(style::INPUT_PADDING)
                    .wrapping(Wrapping::WordOrGlyph)
                    .height(signature_height())
                    .style(style::text_editor)
                    .into()
            });
        let format = mail.settings.format;
        let format = widgets::setting_pair(
            tr("Write new mail as"),
            tr("Formatted mail can hold bold text, lists, links and quotes; a plain text copy goes along."),
            move || picker(FORMAT_CHOICES, FormatChoice(format), |choice: FormatChoice| Message::Format(choice.0)),
        );
        widgets::setting_pairs(vec![name, signature, format])
    }

    /// The room of the signature field: four lines of text and the padding of a field.
    const SIGNATURE_LINES: f32 = 4.0;

    fn signature_height() -> f32 {
        SIGNATURE_LINES * typo::line_height(typo::body()) + style::INPUT_PADDING.top + style::INPUT_PADDING.bottom
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

    /// "Write new mail as" as a choice of a pick list.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct FormatChoice(MailFormat);

    const FORMAT_CHOICES: [FormatChoice; 2] = [FormatChoice(MailFormat::Html), FormatChoice(MailFormat::Plain)];

    impl std::fmt::Display for FormatChoice {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(match self.0 {
                MailFormat::Html => tr("Formatted (HTML)"),
                MailFormat::Plain => tr("Plain text"),
            })
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
            sent(&mut state);
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

        /// "On the server" with a quota names its limit and share; without one it is the sum of the
        /// folders, and without sizes it waits for the next sync.
        #[test]
        fn the_server_figure_with_and_without_a_quota() {
            const GIB: u64 = 1024 * 1024 * 1024;
            let quota = stats(&[("INBOX", 30, 8, Some(1024))], Some((GIB * 236 / 100, 10 * GIB)));
            let figure = server_figure(&quota);
            assert_eq!(figure.value, format_bytes(GIB * 236 / 100));
            let share = widgets::share_percent(GIB * 236 / 100, 10 * GIB);
            assert_eq!(
                figure.note,
                trf("of {total} · {share}", &[("total", &format_bytes(10 * GIB)), ("share", &share)])
            );
            let sum = server_figure(&stats(&[("INBOX", 30, 8, Some(3072)), ("Sent", 10, 2, Some(1024))], None));
            assert_eq!((sum.value, sum.note), (format_bytes(4096), trf("in {n} folders", &[("n", &2)])));
            let unknown = server_figure(&stats(&[("INBOX", 30, 8, None)], None));
            assert_eq!(unknown.value, UNKNOWN);
            let (parts, free) = server_parts(&quota);
            assert_eq!(parts.len(), 2, "the folder and what no folder accounts for");
            assert_eq!(parts[1].label, tr("Not counted by folder"));
            let whole: u64 = parts.iter().map(|part| part.value).sum::<u64>() + free.as_ref().map_or(0, |(_, b)| *b);
            assert_eq!(whole, 10 * GIB, "the strip is the quota");
            assert_eq!(free.map(|(_, bytes)| bytes), Some(10 * GIB - GIB * 236 / 100));
            assert!(server_parts(&stats(&[("INBOX", 30, 8, Some(1))], None)).1.is_none(), "no quota, no free room");
        }

        /// The server's strip names the three largest folders and puts the rest together from the
        /// fourth folder on.
        #[test]
        fn folders_past_the_third_become_other_folders() {
            let folders = [("INBOX", 1, 0, Some(400)), ("Sent", 1, 0, Some(300)), ("Archive", 1, 0, Some(500))];
            let (three, _) = server_parts(&stats(&folders, None));
            assert_eq!(three.iter().map(|p| p.label.as_str()).collect::<Vec<_>>(), ["Archive", "INBOX", "Sent"]);
            let mut four = folders.to_vec();
            four.push(("Junk", 1, 0, Some(10)));
            let (parts, _) = server_parts(&stats(&four, None));
            assert_eq!(parts.len(), 4);
            assert_eq!((parts[3].label.as_str(), parts[3].value), ("Junk", 10));
            assert_eq!(parts[3].color, ChartColor::Other);
            four.push(("Trash", 1, 0, Some(20)));
            let (parts, _) = server_parts(&stats(&four, None));
            assert_eq!((parts[3].label.clone(), parts[3].value), (trf("{n} other folders", &[("n", &2)]), 30));
        }

        /// The device's strip never goes below 0 and names the rest by what it holds.
        #[test]
        fn the_device_strip_adds_up_to_the_database() {
            let mut stats = stats(&[("INBOX", 30, 8, None)], None);
            let parts = device_parts(&stats);
            assert_eq!(parts.iter().map(|p| p.value).sum::<u64>(), stats.database_bytes);
            assert_eq!(parts.last().map(|p| p.label.as_str()), Some(tr("Headers & folders")));
            stats.search_index_bytes = None;
            stats.database_bytes = 1;
            let parts = device_parts(&stats);
            assert_eq!(parts.last().map(|p| (p.label.as_str(), p.value)), Some((tr("Index, headers & folders"), 0)));
        }

        /// Mail over time is counted per month within two years, else per year, gaps as 0.
        #[test]
        fn mail_over_time_per_month_or_per_year() {
            let month = |year, month, messages, cached_bodies| MailMonthCount { year, month, messages, cached_bodies };
            let months = time_columns(&[month(2025, 11, 10, 2), month(2026, 2, 30, 18)]);
            assert_eq!(months.len(), 4, "Nov, Dec, Jan, Feb");
            assert_eq!(months[0].values, vec![2, 8]);
            assert_eq!(months[1].values, vec![0, 0]);
            let years = time_columns(&[month(2020, 3, 5, 0), month(2026, 2, 30, 18)]);
            assert_eq!(
                years.iter().map(|c| c.label.as_str()).collect::<Vec<_>>(),
                ["2020", "2021", "2022", "2023", "2024", "2025", "2026"]
            );
            assert!(time_columns(&[]).is_empty());
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
            assert_eq!(figures(&stats(&[], None)).len(), 4);
        }

        /// The "…" menu opens and closes, and its entries lead to Storage & index and the control
        /// centre.
        #[test]
        fn the_more_menu_leads_on() {
            let data = data();
            let mut state = opened(&data);
            assert!(overlay(&state).is_none());
            let _: Action<()> = update(&mut state, Message::More, &data);
            assert!(overlay(&state).is_some());
            assert!(matches!(escape(&state), Some(Message::CloseMore)));
            let storage: Action<()> = update(&mut state, Message::OpenStorage, &data);
            assert_eq!(storage, Action::Open("storage"));
            assert!(overlay(&state).is_none());
            let server: Action<()> = update(&mut state, Message::ServerSettings, &data);
            assert_eq!(server, Action::ControlCentre("accounts".into()));
        }

        /// DS.7: Storage & index shows none of the account's numbers, only estimates and the job: its
        /// page reads no field of the statistics (the account's page is where they stand).
        #[test]
        fn storage_shows_none_of_the_accounts_numbers() {
            let source = include_str!("mail.rs");
            let start = source.find("pub mod storage {").expect("the storage section");
            let page = &source[start..];
            let page = &page[page.find("pub fn page<'a>").expect("its page")..];
            let page = &page[..page.find("\n    }\n").expect("the end of its page")];
            for field in [
                "cached_bodies",
                "body_cache_bytes",
                "search_index_bytes",
                "last_index_run",
                "oldest_cached_body",
                "indexed_full",
                "indexed_partial_or_unverified",
                "text_unavailable",
                "awaiting_index",
                "database_bytes",
                "cached_attachments",
                "attachment_cache_bytes",
                "messages",
                "unread",
                "quota",
            ] {
                assert!(!page.contains(&format!("stats.{field}")), "Storage & index shows {field}");
            }
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
            let _ = crate::mail::page(&state, &data, "storage").body;
        }

        /// Every role has its question, and every page builds – with and without an account.
        #[test]
        fn the_pages_build_with_their_questions() {
            for role in [Role::Inbox, Role::Drafts, Role::Sent, Role::Archive, Role::Junk, Role::Trash] {
                assert!(!role_question(role).is_empty(), "{role:?}");
            }
            let words: Vec<String> = FORMAT_CHOICES.iter().map(ToString::to_string).collect();
            assert_eq!(words, [tr("Formatted (HTML)"), tr("Plain text")]);
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
    use iced::widget::column;
    use torro_core::settings::{MailAppearance, MailFormat};
    use torro_gui::{layout, widgets};
    use torro_ipc::{Request, SettingKey};

    use super::*;
    use crate::parts::{picker, setting_row};

    /// What a row of this section changes; each is one `settings.set_setting`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum Message {
        /// "Show mail as": how a mail with HTML first shows in the reader.
        Show(MailFormat),
        /// "Mail colors": the colours of a mail's content in the reader.
        Colors(MailAppearance),
    }

    pub fn update<A>(message: Message) -> Action<A> {
        let (key, value) = match message {
            Message::Show(format) => (SettingKey::MailView, serde_json::to_value(format).unwrap_or_default()),
            Message::Colors(colors) => (SettingKey::MailAppearance, serde_json::to_value(colors).unwrap_or_default()),
        };
        Action::send(Request::SetSetting { key, value })
    }

    pub fn view(data: &Data) -> Element<'_, Message> {
        let settings = &data.settings;
        let show = picker(SHOW_CHOICES, ShowChoice(settings.mail_view), |choice: ShowChoice| Message::Show(choice.0));
        let colors = picker(COLOR_CHOICES, ColorChoice(settings.mail_appearance), |choice: ColorChoice| {
            Message::Colors(choice.0)
        });
        let group = widgets::group(
            tr("Reading"),
            vec![
                setting_row(None, tr("Show mail as"), None, show),
                setting_row(None, tr("Mail colors"), None, colors),
            ],
            Some(
                tr("Formatted shows mail the way the sender designed it. Dark recolors mail for a dark background; some newsletters look worse that way.")
                    .to_string(),
            ),
        );
        column![group].spacing(layout::section_gap()).into()
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
            let show: Action<()> = update(Message::Show(MailFormat::Text));
            assert_eq!(
                show,
                Action::send(Request::SetSetting { key: SettingKey::MailView, value: serde_json::json!("text") })
            );
            let colors: Action<()> = update(Message::Colors(MailAppearance::Dark));
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
            let _: Element<'_, Message> = view(&Data::default());
        }
    }
}

pub mod storage {
    use iced::Element;
    use iced::widget::{column, row, space as blank};
    use torro_core::i18n::{tr, trf};
    use torro_gui::theme::space;
    use torro_gui::units::{format_bytes, format_count};
    use torro_gui::{layout, style, typo, widgets};
    use torro_ipc::mail::{MailCacheAction, MailCacheEstimate, MailCacheJobState, MailCacheMode, MailCachePolicy};
    use torro_ipc::{Request, Response};

    use super::*;
    use crate::parts::row_frame;

    #[derive(Debug, Default)]
    pub struct State {
        account: Option<String>,
        pending_mode: Option<MailCacheMode>,
        estimate: Option<MailCacheEstimate>,
        cleanup_preview: Option<MailCacheEstimate>,
        previewing_cleanup: bool,
        error: Option<String>,
    }

    #[derive(Debug, Clone)]
    pub enum Message {
        Account(String),
        Preview(MailCacheMode),
        Apply,
        Retention(u32),
        Prefetch(bool),
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
            .filter(|id| data.mail_stats.iter().any(|stats| stats.account == *id))
            .or_else(|| data.mail_stats.first().map(|stats| stats.account.as_str()))
    }

    pub fn update<A>(state: &mut State, message: Message, data: &Data) -> Action<A> {
        match message {
            Message::Account(account) => {
                state.account = Some(account);
                state.pending_mode = None;
                state.estimate = None;
                state.cleanup_preview = None;
                Action::None
            }
            Message::Preview(mode) => {
                let Some(account) = selected(state, data).map(str::to_string) else { return Action::None };
                state.pending_mode = Some(mode);
                state.estimate = None;
                state.cleanup_preview = None;
                state.error = None;
                state.previewing_cleanup = false;
                Action::Ask(Request::PreviewMailCache { account, mode })
            }
            Message::Apply => {
                let (Some(account), Some(mode), Some(estimate)) =
                    (selected(state, data), state.pending_mode, &state.estimate)
                else {
                    return Action::None;
                };
                if estimate.account != account {
                    return Action::None;
                }
                if data
                    .mail_cache_jobs
                    .iter()
                    .any(|job| job.account == account && job.state == MailCacheJobState::Running)
                {
                    return Action::None;
                }
                let mut policy = policy(data, account);
                policy.mode = mode;
                let account = account.to_string();
                state.pending_mode = None;
                state.estimate = None;
                state.cleanup_preview = None;
                Action::Send(vec![
                    Request::SetMailCachePolicy { policy },
                    Request::StartMailCacheJob { account, action: MailCacheAction::ApplyPolicy },
                ])
            }
            Message::Retention(days) => {
                let Some(account) = selected(state, data) else { return Action::None };
                let mut policy = policy(data, account);
                policy.attachment_retention_days = days;
                Action::send(Request::SetMailCachePolicy { policy })
            }
            Message::Prefetch(enabled) => {
                let Some(account) = selected(state, data) else { return Action::None };
                let mut policy = policy(data, account);
                policy.prefetch_attachments = enabled;
                Action::send(Request::SetMailCachePolicy { policy })
            }
            Message::Job(action) => {
                let Some(account) = selected(state, data).map(str::to_string) else { return Action::None };
                if action == MailCacheAction::CleanCache
                    && !state.cleanup_preview.as_ref().is_some_and(|estimate| estimate.account == account)
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
            prefetch_attachments: false,
            attachment_retention_days: 1,
        })
    }

    fn button<'a>(label: &'static str, message: Option<Message>) -> Element<'a, Message> {
        widgets::text_button(None, tr(label), style::button::secondary, message)
    }

    fn line<'a>(label: &'static str, value: String) -> Element<'a, Message> {
        row_frame(
            row![typo::text(typo::body(), tr(label)), blank::horizontal(), typo::text(typo::body_strong(), value)],
            false,
        )
    }

    pub fn page<'a>(state: &'a State, data: &'a Data) -> Page<'a, Message> {
        let Some(account) = selected(state, data) else {
            return Page::new(widgets::empty_state(
                tr("No mail account"),
                tr("Add a mail account in Torro Workplace, under Accounts."),
            ));
        };
        let current = policy(data, account);
        let mut account_rows = Vec::new();
        for stats in &data.mail_stats {
            let label = data
                .accounts
                .iter()
                .find(|held| held.id == stats.account)
                .map_or(stats.account.as_str(), |held| held.label.as_str());
            account_rows.push(row_frame(
                row![
                    typo::text(typo::body_strong(), label.to_string()),
                    blank::horizontal(),
                    button(
                        if stats.account == account { "Selected" } else { "Open" },
                        (stats.account != account).then(|| Message::Account(stats.account.clone()))
                    )
                ],
                false,
            ));
        }
        // The account's numbers stand once, on its own page (task DS.7): here only the way there.
        account_rows.push(row_frame(
            row![
                typo::text(typo::body(), tr("Numbers of this account")),
                blank::horizontal(),
                button("Show account", Some(Message::ShowAccount(account.to_string())))
            ]
            .align_y(iced::Alignment::Center),
            false,
        ));
        let mut page = column![widgets::group(tr("Mail account"), account_rows, None)].spacing(layout::section_gap());
        let mut policy_rows = vec![
            line(
                "Current strategy",
                match current.mode {
                    MailCacheMode::HeadersOnly => tr("Headers only"),
                    MailCacheMode::Recent90Days => tr("Last 90 days"),
                    MailCacheMode::AllOffline => tr("All mail offline"),
                }
                .to_string(),
            ),
            row_frame(button("Headers only", Some(Message::Preview(MailCacheMode::HeadersOnly))), false),
            row_frame(button("Last 90 days", Some(Message::Preview(MailCacheMode::Recent90Days))), false),
            row_frame(button("All mail offline", Some(Message::Preview(MailCacheMode::AllOffline))), false),
        ];
        if let Some(estimate) = state.estimate.as_ref().filter(|estimate| estimate.account == account) {
            policy_rows.push(line(
                "To download (estimate)",
                format!(
                    "{} · {}",
                    format_count(estimate.bodies_to_fetch),
                    format_bytes(estimate.estimated_download_bytes)
                ),
            ));
            policy_rows.push(line(
                "To remove (estimate)",
                format!(
                    "{} · {}",
                    format_count(estimate.bodies_to_remove),
                    format_bytes(estimate.estimated_reclaim_bytes)
                ),
            ));
            policy_rows.push(row_frame(
                row![
                    blank::horizontal(),
                    button(
                        "Apply strategy",
                        (!data
                            .mail_cache_jobs
                            .iter()
                            .any(|job| job.account == account && job.state == MailCacheJobState::Running))
                        .then_some(Message::Apply)
                    )
                ],
                false,
            ));
        }
        if let Some(error) = &state.error {
            policy_rows.push(row_frame(typo::text(typo::body(), error.clone()), false));
        }
        page = page.push(widgets::group(tr("Offline strategy"), policy_rows, Some(tr("Estimates use the average size of local mail texts; actual downloads may differ. Search text remains independent of offline reading texts.").to_string())));

        page = page.push(widgets::group(tr("Attachment cache"), vec![
            line("Keep downloaded attachments", match current.attachment_retention_days { 1 => tr("1 day").to_string(), days => trf("{days} days", &[("days", &days)]) }),
            row_frame(row![button("1 day", Some(Message::Retention(1))), button("7 days", Some(Message::Retention(7))), button("30 days", Some(Message::Retention(30)))].spacing(space::s()), false),
            line("Prefetch attachments", if current.prefetch_attachments { tr("On") } else { tr("Off") }.to_string()),
            row_frame(row![button("Turn on", (!current.prefetch_attachments).then_some(Message::Prefetch(true))), button("Turn off", current.prefetch_attachments.then_some(Message::Prefetch(false)))].spacing(space::s()), false),
        ], Some(tr("Newly fetched mail can also download its attachment parts. Existing mail can be fetched by a maintenance job. Files have separate retention and can be large.").to_string())));
        let job = data.mail_cache_jobs.iter().find(|job| job.account == account);
        let running = job.is_some_and(|job| job.state == MailCacheJobState::Running);
        let mut job_rows = vec![
            row_frame(
                button(
                    "Download missing mail texts",
                    (!running).then_some(Message::Job(MailCacheAction::FetchMissingBodies)),
                ),
                false,
            ),
            row_frame(
                button("Download attachments", (!running).then_some(Message::Job(MailCacheAction::FetchAttachments))),
                false,
            ),
            row_frame(
                button("Rebuild search index", (!running).then_some(Message::Job(MailCacheAction::RebuildIndex))),
                false,
            ),
            row_frame(button("Preview cleanup", (!running).then_some(Message::PreviewCleanup)), false),
        ];
        if let Some(estimate) = state.cleanup_preview.as_ref().filter(|estimate| estimate.account == account) {
            job_rows.push(line(
                "To remove (estimate)",
                format!(
                    "{} · {}",
                    format_count(estimate.bodies_to_remove),
                    format_bytes(estimate.estimated_reclaim_bytes)
                ),
            ));
            job_rows.push(row_frame(
                button("Clean up cache", (!running).then_some(Message::Job(MailCacheAction::CleanCache))),
                false,
            ));
        }
        if let Some(job) = job {
            let state_label = match job.state {
                MailCacheJobState::Running => tr("Running"),
                MailCacheJobState::Paused => tr("Paused"),
                MailCacheJobState::Complete => tr("Complete"),
                MailCacheJobState::Failed => tr("Failed"),
            };
            job_rows.push(line(
                "Current job",
                format!("{} · {} / {}", state_label, format_count(job.processed), format_count(job.total)),
            ));
            if let Some(error) = &job.error {
                job_rows.push(row_frame(typo::text(typo::body(), error.clone()), false));
            }
            job_rows.push(row_frame(
                row![
                    button("Refresh", Some(Message::Refresh)),
                    button("Pause", running.then(|| Message::Pause(job.id.clone()))),
                    button(
                        "Resume",
                        matches!(job.state, MailCacheJobState::Paused | MailCacheJobState::Failed)
                            .then(|| Message::Resume(job.id.clone()))
                    ),
                ]
                .spacing(space::s()),
                false,
            ));
        }
        Page::new(page.push(widgets::group(tr("Maintenance"), job_rows, Some(tr("Jobs keep their progress after a restart. Mail, drafts, folders, and server copies are not deleted.").to_string()))))
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
        assert_eq!(ids, ["accounts", "reading", "attachments", "categorization", "storage"]);
        assert_eq!(SECTIONS[0].name(), tr("Accounts"));
        assert_eq!(SECTIONS[2].name(), tr("Attachments"), "task M12.7");
        assert_eq!(SECTIONS[3].name(), tr("Categorization"), "task E12.10");
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

    /// Task E12.10: switching the sorting on keeps the model that is assigned, and picking a
    /// model switches it on – that is what picking one means.
    #[test]
    fn the_switch_and_the_model_go_together() {
        use torro_core::ai::{AiAssignment, AiFunction};

        let assignments = vec![torro_ipc::AiAssignmentInfo {
            assignment: AiAssignment {
                function: AiFunction::InboxCategorize,
                engine: Some("ai:ollama@localhost:11434".into()),
                model: Some("nimble".into()),
                effort: None,
                enabled: false,
            },
            label: "Sort mail into categories".into(),
            needs_decision: true,
        }];
        let data = Data { assignments, ..Data::default() };
        let on = categorization::update::<()>(&mut Default::default(), categorization::Message::Enabled(true), &data);
        let Action::Send(requests) = on else { panic!("a request") };
        let [torro_ipc::Request::SetAiAssignment { engine, model, enabled, .. }] = requests.as_slice() else {
            panic!("one assignment")
        };
        assert_eq!(engine.as_deref(), Some("ai:ollama@localhost:11434"), "the model assigned stays");
        assert_eq!(model.as_deref(), Some("nimble"));
        assert!(enabled);

        let pick = categorization::update::<()>(
            &mut Default::default(),
            categorization::Message::Model("ai:x".into(), "tev1".into()),
            &data,
        );
        let Action::Send(requests) = pick else { panic!("a request") };
        let [torro_ipc::Request::SetAiAssignment { engine, model, enabled, .. }] = requests.as_slice() else {
            panic!("one assignment")
        };
        assert_eq!((engine.as_deref(), model.as_deref(), *enabled), (Some("ai:x"), Some("tev1"), true));
    }

    /// Task S.26: only models turned on are offered for sorting mail – switching it on takes the
    /// first one that is on, and with none on there is nothing to switch on.
    #[test]
    fn sorting_mail_offers_only_models_that_are_on() {
        use torro_core::ai::AiModel;

        let decider = |id: &str, enabled: bool| {
            ("ai:ollama@localhost:11434".to_string(), AiModel { decision: Some(true), enabled, ..AiModel::named(id) })
        };
        let data = Data { decider_models: vec![decider("off", false), decider("on", true)], ..Data::default() };
        let on = categorization::update::<()>(&mut Default::default(), categorization::Message::Enabled(true), &data);
        let Action::Send(requests) = on else { panic!("a request") };
        let [torro_ipc::Request::SetAiAssignment { model, .. }] = requests.as_slice() else { panic!("one assignment") };
        assert_eq!(model.as_deref(), Some("on"), "the model that is off is not offered");
        let _ = categorization::view(&Default::default(), &data);

        let none = Data { decider_models: vec![decider("off", false)], ..Data::default() };
        let nothing =
            categorization::update::<()>(&mut Default::default(), categorization::Message::Enabled(true), &none);
        assert!(matches!(nothing, Action::None), "nothing on, nothing to switch on");
        let _ = categorization::view(&Default::default(), &none);
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
/// Model                       the decision models of the AI accounts, local first
/// Catch up                    how far the backlog has come, "Catch up now" / "Cancel"
/// Show in the sidebar         one switch per category
/// Rules ↗                     the kept senders, in the Notification Hub
/// ```
///
/// **No setting for the moment of sorting or the confidence threshold**: both are fixed
/// (`docs/kategorien.md`, "decisions instead of options").
///
/// Models that do not run on this computer are **not offered** while mail may not leave it
/// (`docs/ki.md`, principle 4); a quiet line says so. The core refuses such a model as well
/// (task E12.5), so the rule holds even if this list were wrong.
///
/// **A model turned off** (PR review #242): only models turned on in the control centre's
/// AI section are offered (task S.26). The assigned one turned off since stands on top of "Model"
/// with "Turned off in Settings › AI" and an alert in the attention colour – the core sorts
/// nothing then ([`assigned_off`]) –, and "Manage models…" ends the group, the way to turn it on.
pub mod categorization {
    use iced::Theme;
    use iced::widget::text::Wrapping;
    use iced::widget::{column, svg};
    use torro_core::ai::{AiAssignment, AiFunction, AiModel};
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
        all_accounts: bool,
        accounts: Vec<String>,
        all_folders: bool,
        folders: Vec<String>,
        period: Period,
        again: bool,
        preview: Option<CategoryJobPreview>,
        preview_error: Option<String>,
        pending_scope: Option<CategoryJobScope>,
        asking_clear: bool,
        /// `None` means the recommended choice: switch future automatic sorting off.
        disable_automatic_after_clear: Option<bool>,
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
        /// The switch: sorting mail on or off.
        Enabled(bool),
        /// A model picked for sorting mail.
        Model(String, String),
        /// "Catch up now" (`false`) or "Cancel" (`true`).
        CatchUp(bool),
        /// "Sort again" (task E15.5): the whole window anew, after new rules or descriptions.
        SortAgain,
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
        let held = assignment(data);
        match message {
            Message::Editor(message) => super::editor::update(&mut state.editor, message, data),
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
            // Switching on without a model named would store an assignment the core throws away
            // (no account, nothing to ask): the first model that can decide is taken, which is
            // what the switch means. Nothing offered, nothing to switch on.
            Message::Enabled(true) if held.engine.is_none() => match offered(data).first() {
                Some((engine, model)) => Action::send(Request::SetAiAssignment {
                    function: AiFunction::InboxCategorize,
                    engine: Some(engine.clone()),
                    model: Some(model.id.clone()),
                    effort: None,
                    enabled: true,
                }),
                None => Action::None,
            },
            Message::Enabled(on) => Action::send(Request::SetAiAssignment {
                function: AiFunction::InboxCategorize,
                engine: held.engine.clone(),
                model: held.model.clone(),
                effort: held.effort,
                enabled: on,
            }),
            Message::Model(engine, model) => Action::send(Request::SetAiAssignment {
                function: AiFunction::InboxCategorize,
                engine: Some(engine),
                model: Some(model),
                effort: held.effort,
                // Picking a model switches the sorting on: that is what picking one means.
                enabled: true,
            }),
            Message::CatchUp(cancel) => Action::send(Request::CatchUpCategories { cancel, again: false }),
            Message::Shown(category, shown) => Action::send(super::editor::shown_request(data, category, shown)),
            Message::ManageModels => Action::ControlCentre(crate::assistant::AI_SECTION.to_string()),
        }
    }

    /// What is assigned for sorting mail, or nothing assigned.
    fn assignment(data: &Data) -> AiAssignment {
        data.assignments
            .iter()
            .find(|info| info.assignment.function == AiFunction::InboxCategorize)
            .map(|info| info.assignment.clone())
            .unwrap_or_else(|| AiAssignment::none(AiFunction::InboxCategorize))
    }

    /// The section's page: the list, or the open page of a category or a sorting rule with
    /// "‹ Back" (task E15.5).
    pub fn page<'a>(state: &'a State, data: &'a Data) -> Page<'a, Message> {
        match super::editor::page(&state.editor, data) {
            Some((name, body, save)) => {
                let back = Message::Editor(super::editor::Message::Back);
                Page::new(body.map(Message::Editor)).sub_page(name, back).actions(vec![save.map(Message::Editor)])
            }
            None => Page::new(view(state, data)),
        }
    }

    pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
        let held = assignment(data);
        let on = held.is_ready();
        let switch_row = setting_row(None, tr("Sort mail into categories"), None, switch(on, Message::Enabled));
        let sorting = widgets::group(
            tr("Categorization"),
            vec![switch_row],
            Some(match data.settings.mail_to_cloud {
                true => tr("The chosen model sorts mail. Mail may leave this computer; no message is moved or deleted, and categories can be changed by hand.").to_string(),
                false => tr("A model on your computer says what each mail is. No mail leaves this computer, nothing is hidden, moved or deleted, and you can change any category by hand.").to_string(),
            }),
        );
        let models = widgets::group(tr("Model"), model_rows(data, &held), Some(model_note(data)));
        let sorting_rules = super::editor::rules_group(data).map(Message::Editor);
        // The rules before the long list of categories: they change more often. "Always for this
        // sender?" keeps its senders among them (task E15.3), so no link to the Notification Hub.
        // Whether the sidebar shows a category is on its page (task E15.5), not a second list.
        let mut page = column![sorting, models, job_group(state, data), sorting_rules].spacing(layout::section_gap());
        for group in super::editor::categories_groups(data) {
            page = page.push(group.map(Message::Editor));
        }
        page.into()
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
                choice(tr("All known mail").to_string(), state.period == Period::All, Message::Period(Period::All)),
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
        let mut page = column![accounts, folders, period, summary].spacing(layout::section_gap());
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
            Some(trf("{count} assignments across all accounts and folders", &[("count", &count)])),
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
        let held = assignment(data);
        let remote = data
            .decider_models
            .iter()
            .find(|(engine, model)| {
                held.engine.as_deref() == Some(engine.as_str()) && held.model.as_deref() == Some(model.id.as_str())
            })
            .and_then(|(_, model)| model.remote);
        match remote {
            Some(true) => tr("Cloud model: mail content leaves this computer.").to_string(),
            Some(false) => tr("Local model: mail content stays on this computer.").to_string(),
            None => tr("Choose a decision model above.").to_string(),
        }
    }

    /// The models that may sort mail: every AI account's models that decide and are turned on for
    /// the suite (task S.26, `AiModel::enabled`), and – while mail may not leave this computer –
    /// only those that run here (`docs/ki.md`, principle 4). The core refuses the others as well
    /// (task E12.5), so the rule holds even if this list were wrong.
    fn offered(data: &Data) -> Vec<(String, AiModel)> {
        deciders(data).into_iter().filter(|(_, model)| model.enabled).collect()
    }

    /// The models that decide and may see mail, turned on or not.
    fn deciders(data: &Data) -> Vec<(String, AiModel)> {
        let local_only = !data.settings.mail_to_cloud;
        data.decider_models
            .iter()
            .filter(|(_, model)| model.decision == Some(true))
            .filter(|(_, model)| !local_only || model.remote != Some(true))
            .cloned()
            .collect()
    }

    /// The model assigned for sorting mail when the sorting is on and that model of its AI account
    /// is turned off (PR review #242): the account's models are in and have it off, or lack it. The
    /// core sorts nothing then (task S.24). Its account and name as listed – the id where the list
    /// lacks it. `None` while the sorting is off, the model is on, "Automatic" is assigned, or the
    /// account's models are not in yet.
    pub fn assigned_off(data: &Data) -> Option<(String, String)> {
        let held = assignment(data);
        let (engine, model) = (held.engine.filter(|_| held.enabled)?, held.model?);
        let mut listed = data.decider_models.iter().filter(|(account, _)| *account == engine).peekable();
        listed.peek()?;
        match listed.find(|(_, known)| known.id == model) {
            Some((_, known)) if known.enabled => None,
            Some((_, known)) => Some((engine, known.name.clone())),
            None => Some((engine, model)),
        }
    }

    /// One row per model that may sort mail, the assigned one checked; picking one switches the
    /// sorting on. Without any, one row says so instead: where to turn one on when there are
    /// models that decide but none is on (task S.26), else where to add one. The assigned model
    /// turned off since comes first ([`assigned_off`]), "Manage models…" last.
    fn model_rows<'a>(data: &'a Data, held: &AiAssignment) -> Vec<Element<'a, Message>> {
        let offered = offered(data);
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
        rows.extend(offered.into_iter().map(|(engine, model)| {
            let chosen = held.is_ready()
                && held.engine.as_deref() == Some(engine.as_str())
                && held.model.as_deref() == Some(model.id.as_str());
            choice(label(&engine, &model.name), chosen, Message::Model(engine, model.id))
        }));
        // Which models are offered is set where the accounts are (task S.26), the whole suite at once.
        let arrow = icon::icon(Icon::ArrowUpRight, icon::SIZE)
            .style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) });
        rows.push(pressable(setting_row(None, tr("Manage models…"), None, arrow), Some(Message::ManageModels)));
        rows
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
        match data.settings.mail_to_cloud {
            true => tr("Mail may go to an AI elsewhere; every model that decides is offered.").to_owned(),
            false => {
                tr("Only models on this computer are offered: mail stays here unless you allow more in the control centre.")
                    .to_owned()
            }
        }
    }
}
