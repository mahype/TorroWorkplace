//! Accounts (task S.5 in `docs/tasks/steuerzentrale.md`, several accounts since M1.2, fixed groups
//! since S.30, providers beside their accounts since S.37): the providers in this order and also
//! without an account – Nextcloud, Mail (with the CalDAV accounts of a mail provider), GitHub,
//! ntfy. AI has its own permanent section and reuses this module ([`Group`]); an account of an
//! otherwise unknown kind gets a provider of its own at the end.
//! They stand in a column at the left edge of the content ([`provider_column`]: with names and
//! counts where the list keeps its reading width beside them, else only icons) or as chips above the
//! list (narrow), the icon with a dot in a status colour where an account needs the person. The list shows one provider ([`chosen`]): its name and the
//! sentence of what belongs there, then its accounts as rows (status dot, label on one line, status
//! and last sync below, chevron) and its "Add" ([`Message::Add`]), which opens the add-account
//! dialog (`wizard.rs`, task S.6) at that provider's first step; the header bar has no button of
//! its own. Mail also offers "Import from TorroMail…" ([`Message::ImportTorroMail`]) while
//! TorroMail has an account to take over: the page asks for TorroMail's accounts when it opens
//! ([`opened`]) and after the dialog closes, not with every reload. Under AI stands the data rule
//! "Chat may read mail in the cloud" (`settings.set_setting mail_to_cloud`, task C.17), for the
//! assistant and all its bots.
//!
//! **The two slots (issue #325, task MW.8).** Above the data rule the AI section has the group
//! "Models" ([`slots_group`]): the computer as measured, then *Chat* and *Sorting*, each with the
//! model chosen as a pick list of the models turned on that serve the slot ("None" empties it,
//! `ai.set_slot`), and under it what Torro suggests (`ai.recommend`) with "Use suggestion". That button
//! runs a chain ([`Using`]): a local model the account does not list yet is downloaded first
//! (`ai.pull_model`, then `ai.list_pulls` every second while it runs, "Cancel" with `ai.cancel_pull`),
//! then the account's models are read again (`ai.list_models`), the model turned on if it is off
//! (`ai.set_model_enabled`) and put in the slot (`ai.set_slot`). A step that fails stops the chain and
//! shows why with "Retry". These requests go with [`Action::AskAi`] and come back tagged ([`Asked`],
//! [`ai_answered`]), so an answer is never taken for another one's. Where *Sorting* sends mail to a
//! model elsewhere – chosen or suggested –, the group says so; the data rule then shows that
//! permission with "Keep mail here" (`settings.set_setting decide_mail_to_cloud false`).
//!
//! A detail page per account has groups for the status ("Sync now"), the calendars and task lists
//! (a switch each for showing them in the apps, read-only ones marked) and the connection, and
//! "Remove account…" behind a confirmation that names the account and acts on it alone. A
//! sign-in in progress (an entry without an id) is a "Signing in…" row without a detail page.
//! The dialog and the confirmation are dialogs over the whole window ([`overlay`]). The shell
//! hands the answers to requests to [`answered`], so the dialog shows the outcome of its own
//! requests.
//!
//! A GitHub account (task G.1) has no calendars or lists yet: its detail page shows what the
//! token's scopes allow and what is missing ([`github_permissions`]) in place of them.
//!
//! An AI account (task KI.1a, kind `ai`) has neither: its row says how it stands, its page shows
//! the status without "Sync now" (nothing of it syncs), the server's address and where the key
//! lives. Between status and connection stands the group "Models" (task S.31, `docs/settings.md` ›
//! KI): a row per model with what it can, where it computes ([`model_place`]) and a switch
//! (`ai.set_model_enabled`), faded and without a switch where it can't use tools, by name (task
//! S.36); a search over them, at OpenRouter with "Show closed models". The page asks for the models
//! as it opens (`ai.list_models` with `all`), not with every reload; a switch shows at once and
//! turns back when the core refuses. The account's row counts the models on from the last list ("3
//! models on").
//!
//! A CalDAV account (a mail provider's, task M7.2) shows "CalDAV" as its kind, the server and user
//! in the connection group, and "Change password…" there: a dialog over the window asks for the
//! new password and sends `ConnectCalDav` for the same server and user again (the core then
//! replaces only the password). A refused password stays in the dialog under its field.
//!
//! A mail account (task M5.2, `AccountInfo::mail`) has a page of its own ([`mail_detail`]): the
//! state of each service (mail, calendars and tasks) with its last sync and problem and "Sync now",
//! where its mail settings are now ([`moved_group`]), the services as switches, and the connection:
//! "Server settings…" and "Change password…" open dialogs that send `ConnectMail` again (the only
//! way to change servers or password; the password lives in the dialog as a [`Secret`] until it is
//! sent). The services go through `UpdateMailAccount`, which changes settings only.
//!
//! Task M11.4: when a server shows a certificate the system does not trust
//! (`Response::UntrustedCertificate`), both dialogs ask the question of the add-account dialog
//! ([`wizard::certificate_question`]) in place of their form, under the title the account's name,
//! the fingerprint above the foot, and their last button becomes "Trust certificate"
//! ([`wizard::trust_button`]: not the red primary button for a host on the public internet), which
//! sends `ConnectMail` again with every certificate trusted in the dialog. The question stays while
//! that request is out, with "Signing in…" in place of the button. Only for this question a dialog
//! keeps the password; any other answer, "Back" or closing clears it. A group "Trusted
//! certificates" below the connection lists the certificates the account trusts, each fingerprint
//! in the two lines of the question (`MailView::trusted_certificates`); it is left out while there
//! are none.
//!
//! Task M3.5: "Server settings" always has the calendar address (CalDAV), so an account connected
//! for mail only (a TorroMail import, for one) gets calendars and tasks later: their switches open
//! that dialog while the account has no address, and saving it switches the service on with the
//! address. Both mail servers are typed as in the add-account dialog (`server_fields.rs`).
//!
//! **What moved into the apps (task S.21).** An account page is about the account itself – the
//! sign-in, what syncs, the servers, the keychain. What a window shows of it is set in that window,
//! behind its gear: which calendars and lists the suite shows (Calendar and Tasks), which
//! notifications a GitHub or Nextcloud account sends and whether new mail shows up (Notification
//! Hub), and a mail account's sender name, signature, writing format and folder questions (Mail).
//! In their place stands one group per window with the sentence where it is now and a button that
//! opens that window ([`moved_group`], `apps.open_app` with the window class).
//!
//! An ntfy account (task E5.14, `AccountInfo::ntfy`) has a page of its own ([`ntfy_detail`]): its
//! topics by label with their names, each opening a dialog to rename it or "Unsubscribe…" (behind
//! a confirmation), "Add topic…" (a dialog for the name and label), and "Change login…" (a dialog
//! with the login form of the add-account dialog, the secret a [`Secret`] until it is sent). They
//! send `accounts.set_ntfy_topic` and `accounts.set_ntfy_login`; the dialogs wait for the answer and
//! show a refusal in their foot.
//!
//! Requests: `Sync`, `RemoveAccount { id }`, `ConnectCalDav` / `ConnectMail` for "Change
//! password…", `ConnectMail` for "Server settings…", `UpdateMailAccount` for the services of a mail
//! account, `OpenApp` for the buttons that open a window, `TorroMailAccounts` for the import,
//! `SetSetting { key: MailToCloud | DecideMailToCloud }` for the data rule, `AiModels` /
//! `SetModelEnabled` for an AI account's models, and for the slots `AiRecommend`, `PullAiModel`,
//! `AiPulls`, `CancelAiPull`, `SetAiSlot` and – before a model goes into Sorting – `AiAssignments` and
//! `SetAiAssignment`, which take a stored exception of sorting back. The pure parts (relative time, status text,
//! whether a collection is shown, which accounts are listed, the groups) are plain functions with
//! unit tests.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fmt;
use std::time::Duration;

use chrono::{DateTime, Utc};
use iced::widget::space as blank;
use iced::widget::text::{IntoFragment, Wrapping};
use iced::widget::{Space, column, container, row, scrollable, svg, text, toggler};
use iced::{Alignment, Element, Fill, Subscription, Theme};
use torro_core::AppId;
use torro_core::account::Secret;
use torro_core::ai::hardware::Hardware;
use torro_core::ai::{AiFunction, AiModel, AiProvider, AiSlot, Effort, Seat};
use torro_core::i18n::{tr, trf};
use torro_core::settings::Settings;
use torro_gui::dialog::frame as dialog_frame;
use torro_gui::icon::{self, Icon};
use torro_gui::layout::{self, SizeClass};
use torro_gui::shell::{self, SidebarMode};
use torro_gui::theme::{self, Palette, palette, size, space};
use torro_gui::widgets::{self, Mark, Status, Trail};
use torro_gui::{style, typo};
use torro_inbox::item::SourceHealth;
use torro_inbox::{SourceKind, SourceRef};
use torro_ipc::{
    AccountInfo, AiPull, AiPullState, AiRecommendation, MailView, NtfyTopicView, NtfyView, Request, Response,
    ServiceState, SettingKey, SlotSuggestion, TopicName, TorroMailAccount,
};
use torro_mail::{MailFormat, Notify, Role, Security, Services};
use torro_pim::AccountState;

use torro_settings_ui::parts::{picker, picker_fill};

use super::server_fields::{Server, ServerFields, imap_titles, server_group, smtp_titles};
use super::{Action, Data, Header, chevron, confirm, dialog, notice, pressable, row_frame, setting_row, wizard};
use crate::account::ntfy::TOPICS_MAX;
use crate::assistant::{Place, place_of};

#[derive(Debug, Default)]
pub struct State {
    /// The add-account dialog while it is open.
    pub wizard: Option<wizard::State>,
    /// The account whose detail page is open (its `AccountInfo::id`); `None` shows the list.
    pub selected: Option<String>,
    /// The provider the list shows (task S.37), by the key of its group ([`Listed::key`]); `None`
    /// until one is chosen, then [`chosen`] takes the first with an account. Kept while the window
    /// runs, not in `ui.json`: the accounts are what makes a provider worth opening first.
    pub provider: Option<String>,
    /// The "Remove this account?" confirmation is open.
    pub confirm_remove: bool,
    /// The "Change password" dialog of the open CalDAV or mail account.
    pub password: Option<PasswordChange>,
    /// The "Server settings" dialog of the open mail account.
    pub servers: Option<ServerSettings>,
    /// The topic dialog of the open ntfy account (task E5.14): a new topic, or one to rename or
    /// unsubscribe.
    pub topic: Option<TopicEdit>,
    /// The "Change login" dialog of the open ntfy account.
    pub login: Option<LoginChange>,
    /// TorroMail's accounts as the last answer listed them (task S.30): whether the group Mail
    /// offers "Import from TorroMail…" ([`wizard::can_import`]). `None` until an answer came; a
    /// list that could not be read counts as empty.
    pub torromail: Option<Vec<TorroMailAccount>>,
    /// `TorroMailAccounts` was sent by the page and has not answered.
    asking_torromail: bool,
    /// The models of each AI account as `ai.list_models` last answered (task S.31), by account id,
    /// with the switches pressed since: the open page lists them, the account's row in the list
    /// counts those that are on. Asked when an AI account's page opens, never with a reload.
    pub models: BTreeMap<String, Vec<AiModel>>,
    /// What the open AI account's page shows of its models besides the list (task S.31).
    pub models_page: ModelsPage,
    /// The model switches sent and not answered yet, oldest first. The core answers each with `ok`
    /// or an error; a refusal turns the oldest one back ([`answered`]). Kept when the page closes,
    /// so a late refusal still corrects the count in the list.
    switching: VecDeque<ModelSwitch>,
    /// The group "Models" of the AI section (issue #325, task MW.8): the suggestion, the downloads and
    /// what "Use suggestion" or a slot's list is doing.
    pub slots: SlotsPage,
}

/// The state of the group "Models" in the AI section (task MW.8) beside the models themselves
/// ([`State::models`]). What was asked is forgotten when the section closes ([`leave`]), so it asks
/// again when it opens; a chain under way and the downloads stay, since they run in the core.
#[derive(Debug, Default)]
pub struct SlotsPage {
    /// The last answer to `ai.recommend`; `None` until one came.
    pub recommendation: Option<AiRecommendation>,
    /// `ai.recommend` was sent and has not answered: the first answer can take a while (the core
    /// measures the computer and asks every account for its models), so the row waits quietly.
    pub recommending: bool,
    /// Why `ai.recommend` failed, in the core's words: a row with "Retry".
    pub recommend_error: Option<String>,
    /// `ai.recommend` and `ai.list_pulls` were asked since the section opened.
    asked: bool,
    /// The AI accounts whose models were asked for since the section opened, answered or not: a
    /// list that cannot be read is not asked for again with every reload.
    asked_models: BTreeSet<String>,
    /// The downloads as the core last listed them (`ai.pull_model`, `ai.list_pulls`,
    /// `ai.cancel_pull`), oldest first.
    pub pulls: Vec<AiPull>,
    /// `ai.list_pulls` is out: the next tick of [`subscription`] waits for it.
    polling: bool,
    /// "Use suggestion" or a choice in a slot's list under way; at most one at a time.
    pub using: Option<Using>,
    /// Why the last step failed: a row under its slot with "Retry".
    pub failed: Option<SlotFailure>,
}

/// A model on its way into a slot (task MW.8): "Use suggestion" or a choice in the slot's list. The
/// chain runs [`UseStep`] by [`UseStep`]; each answer starts the next request ([`ai_answered`]).
#[derive(Debug, Clone)]
pub struct Using {
    pub slot: AiSlot,
    /// The account (`ai:…`); `None` empties the slot.
    pub engine: Option<String>,
    /// The model; once the account listed it, its id as listed (`nimble:latest` for `nimble`).
    pub model: Option<String>,
    /// How hard it thinks: the chat keeps the effort it has, sorting takes none.
    effort: Option<Effort>,
    /// The exception of `inbox.categorize` stored when the chain started, for Sorting (review of #325):
    /// before `ai.set_slot decide` it is taken back ([`UseStep::TakeBack`]), as Settings › Categorization
    /// does, so the slot sorts – one switched off would keep sorting off, one elsewhere would sort
    /// instead. Taken back first, so meanwhile no exception the person stopped sorts again. Whether it
    /// still stands is asked at that step ([`UseStep::Check`]): one the person changed or set while a
    /// download ran is theirs and stays.
    displaces: Option<torro_core::ai::AiAssignment>,
    /// `ai.list_pulls` was out when `ai.pull_model` went out: its answer may predate the start and
    /// lack the new download, so it is dropped (review of #325) – never a reason to stop the chain.
    stale_poll: bool,
    pub step: UseStep,
    /// What "Retry" sends after a step failed: the press that started the chain.
    retry: Box<Message>,
}

/// Where a [`Using`] stands: the request that is out (or, while downloading, the download it waits
/// for).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UseStep {
    /// `ai.pull_model` is out (`None`) or the download with this id runs; `ai.list_pulls` follows it.
    Pull(Option<String>),
    /// `ai.pull_model` answered `busy` (the sentence): `ai.list_pulls` is out to find the download of
    /// the model that runs already (another window, an agent), which the chain then follows by its id
    /// (review of #325); without one it stops with that sentence.
    Join(String),
    /// `ai.list_models` for the account: the model's id as listed and whether it is on.
    List,
    /// `ai.set_model_enabled` turns the model on.
    Enable,
    /// `ai.list_assignments`: whether the exception the chain started with still stands as it was
    /// ([`Using::displaces`]), just before the slot is set.
    Check,
    /// `ai.set_assignment inbox.categorize` with `engine: null` takes the stored exception back, so the
    /// slot Sorting sorts ([`Using::displaces`]); `ai.set_slot` follows.
    TakeBack,
    /// `ai.set_slot` puts it in the slot.
    Set,
}

impl UseStep {
    /// The chain waits for a download: one it starts, joins or follows.
    fn downloading(&self) -> bool {
        matches!(self, UseStep::Pull(_) | UseStep::Join(_))
    }
}

/// A step of a [`Using`] (or "Cancel") that failed: the sentence under the slot and what "Retry"
/// sends.
#[derive(Debug, Clone)]
pub struct SlotFailure {
    pub slot: AiSlot,
    pub message: String,
    retry: Box<Message>,
}

/// What a request of the group "Models" was for ([`Action::AskAi`]), so its answer finds its way back
/// ([`ai_answered`]) and is never taken for another one's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asked {
    /// `ai.recommend`.
    Recommend,
    /// `ai.list_pulls`, every second while a download runs.
    Pulls,
    /// `ai.list_models` of this account, for the lists of the slots.
    Models(String),
    /// A step of the chain of [`SlotsPage::using`] for this slot.
    Slot(AiSlot),
    /// "Cancel" of the download with this id shown under this slot: "Retry" after a refusal aims at
    /// the same download, never at another one that runs.
    Cancel(AiSlot, String),
}

impl Asked {
    /// The answer may have changed the settings (a slot, `decideMailToCloud`, a model turned on): the
    /// shell reloads after it. Reading answers do not, so polling every second stays light.
    pub fn changes(&self) -> bool {
        matches!(self, Asked::Slot(_))
    }
}

/// The state of the group "Models" on an AI account's page (task S.31) beside the models
/// themselves ([`State::models`]): whether they are being asked for, why that failed, why the
/// core refused the last switch, the search, and "Show closed models" of OpenRouter. A new
/// page starts with a fresh one.
#[derive(Debug, Default)]
pub struct ModelsPage {
    /// `ai.list_models` was sent for the open page and has not answered: "Loading models…".
    pub loading: bool,
    /// Why the models could not be listed, in the core's words: a sentence in the group.
    pub error: Option<String>,
    /// Why the core refused the last switch: a sentence under the group, until the next switch.
    pub refused: Option<String>,
    /// What the search field over the models holds; matched against name and id, any case.
    pub search: String,
    /// OpenRouter lists its models with closed weights too ([`shown_models`]).
    pub show_closed: bool,
}

/// One model switch on its way to the core: which account's model, and what it was switched to.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ModelSwitch {
    account: String,
    model: String,
    enabled: bool,
}

/// The topic dialog of an ntfy account's page (task E5.14). For a new topic the name and label are
/// typed; for one the account has, the name is fixed and the label can change, and "Unsubscribe…"
/// asks before it goes. The name is a [`TopicName`], so `Debug` never shows it.
#[derive(Debug)]
pub struct TopicEdit {
    name: TopicName,
    /// The label the account keeps for the topic; `None` for a new topic.
    stored: Option<String>,
    label: String,
    /// "Subscribe" or "Save" was pressed: from then on the dialog says what is wrong.
    tried: bool,
    /// "Unsubscribe…" was pressed: the confirmation shows instead of the dialog.
    confirm_unsubscribe: bool,
    error: Option<String>,
    sending: bool,
}

impl TopicEdit {
    /// A new topic, nothing typed.
    fn new() -> TopicEdit {
        TopicEdit {
            name: TopicName::new(String::new()),
            stored: None,
            label: String::new(),
            tried: false,
            confirm_unsubscribe: false,
            error: None,
            sending: false,
        }
    }

    /// A topic the account has; `None` when the answer held no name (only the app key gets one).
    fn of(topic: &NtfyTopicView) -> Option<TopicEdit> {
        let name = topic.topic.as_deref()?;
        Some(TopicEdit {
            name: TopicName::new(name),
            stored: Some(topic.label.clone()),
            label: topic.label.clone(),
            ..TopicEdit::new()
        })
    }

    /// What keeps the topic from being sent, in the core's words.
    fn problem(&self) -> Option<String> {
        let name = self.stored.is_none().then(|| wizard::topic_problem(self.name.as_str())).flatten();
        name.or_else(|| wizard::label_problem(&self.label))
    }
}

/// The "Change login" dialog of an ntfy account (task E5.14): the login form of the add-account
/// dialog, what went wrong and whether the request is on its way.
#[derive(Debug, Default)]
pub struct LoginChange {
    login: wizard::NtfyLogin,
    error: Option<String>,
    sending: bool,
}

impl LoginChange {
    /// The form as the account signs in now: its user name, or the token way when it has a login
    /// without one. The secret is never filled in: the core keeps it.
    fn of(view: &NtfyView) -> LoginChange {
        let token = view.login && view.user.is_none();
        let login = wizard::NtfyLogin { token, user: view.user.clone().unwrap_or_default(), ..Default::default() };
        LoginChange { login, ..LoginChange::default() }
    }
}

/// The "Change password" dialog: the new password, what went wrong, and whether the request is on
/// its way (then the button waits for the answer).
#[derive(Debug, Default)]
pub struct PasswordChange {
    password: Secret,
    error: Option<String>,
    sending: bool,
    /// The certificates trusted in this dialog and the one it asks about (task M11.4).
    certificates: wizard::Certificates,
}

/// The fields of `UpdateMailAccount` that a click changes; the others stay as they are.
#[derive(Debug, Default, PartialEq, Eq)]
struct MailChange {
    sender_name: Option<String>,
    signature: Option<String>,
    notify: Option<Notify>,
    services: Option<Services>,
    folder_roles: Option<BTreeMap<Role, String>>,
    format: Option<MailFormat>,
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
            format: self.format,
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
    /// The certificates trusted in this dialog and the one it asks about (task M11.4).
    certificates: wizard::Certificates,
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
            certificates: wizard::Certificates::default(),
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
    /// The "Add" of a group (task S.30): the add-account dialog at that provider's first step.
    Add(wizard::Provider),
    /// "Import from TorroMail…" of the group Mail (task S.30): the dialog at TorroMail's accounts.
    ImportTorroMail,
    /// The switch of the data rule under AI (task C.17, moved here with S.30): whether cloud models
    /// read the mail the assistant and its bots may read – a setting of the suite, not a right.
    MailToCloud(bool),
    Wizard(wizard::Message),
    /// A provider in the column or the chips of the list (task S.37), by the key of its group
    /// ([`Listed::key`]): the list shows its accounts.
    ShowProvider(String),
    /// Opens the detail page of an account.
    Open(String),
    /// Back from a detail page to the list.
    Back,
    SyncNow,
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
    /// The switch of a service of the mail account.
    SetService(MailService, bool),
    /// "Open Calendar", "Open Tasks", "Open Notification Hub" or "Open Mail" of a group that says
    /// where a setting lives now (task S.21): the window with this class.
    OpenApp(&'static str),
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
    /// "Back" from the question about a certificate in "Server settings" or "Change password"
    /// (task M11.4): the form again, the password to be typed again.
    CertificateBack,
    /// "Add topic…" on an ntfy account's page.
    AddTopic,
    /// A topic's row, by its place in the account's list.
    EditTopic(usize),
    /// The new topic's name was typed. A [`TopicName`], so that `Debug` shows none.
    TopicNameEdited(TopicName),
    TopicLabelEdited(String),
    /// "Subscribe" or "Save" in the topic dialog, or Enter in a field.
    SaveTopic,
    /// "Unsubscribe…" in the topic dialog.
    AskUnsubscribe,
    CancelUnsubscribe,
    Unsubscribe,
    CloseTopic,
    /// "Change login…" on an ntfy account's page.
    ChangeLogin,
    /// "User and password" (`false`) or "Access token" (`true`).
    LoginKind(bool),
    LoginUserEdited(String),
    /// The password or token. A [`Secret`], so that `Debug` shows none.
    LoginSecretEdited(Secret),
    SubmitLogin,
    CloseLogin,
    /// The switch of a model on an AI account's page (task S.31), by the model's id: sends
    /// `ai.set_model_enabled` and shows the new state at once. A model that can't use tools has no
    /// switch, so it does nothing for one.
    SetModel(String, bool),
    /// "Retry" after the models could not be listed: asks the server again.
    LoadModels,
    /// The search field over an AI account's models was typed in.
    ModelSearch(String),
    /// "Show closed models" of OpenRouter, on or off.
    ShowClosed(bool),
    /// A model chosen in a slot's list (task MW.8), or "None": `ai.set_slot`. Ignored while another
    /// choice or suggestion is under way, and for what the slot has already.
    PickSlot(AiSlot, SlotChoice),
    /// "Use suggestion" of a slot: the chain of [`Using`] for the suggestion of `ai.recommend`.
    UseSuggestion(AiSlot),
    /// "Cancel" of a download shown under a slot, by the download's id: `ai.cancel_pull`.
    CancelPull(AiSlot, String),
    /// The tick of [`subscription`] while a download runs: `ai.list_pulls`.
    PollPulls,
    /// "Retry" after `ai.recommend` failed.
    Recommend,
    /// "Retry" after a step failed: what started it, again.
    RetrySlot,
    /// "Keep mail here" of the permission of *Sorting* in the data rule: `decide_mail_to_cloud` off.
    StopSortingMail,
}

/// A choice in a slot's list (task MW.8): a model of an AI account, or "None" (both `None`). Two
/// choices are the same when account and model are; `label` is what the list shows.
#[derive(Debug, Clone, Eq)]
pub struct SlotChoice {
    pub engine: Option<String>,
    pub model: Option<String>,
    pub label: String,
}

impl PartialEq for SlotChoice {
    fn eq(&self, other: &Self) -> bool {
        self.engine == other.engine && self.model == other.model
    }
}

impl fmt::Display for SlotChoice {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.label)
    }
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
    } else if let Some(edit) = &state.topic {
        Some(if edit.confirm_unsubscribe { Message::CancelUnsubscribe } else { Message::CloseTopic })
    } else if state.login.is_some() {
        Some(Message::CloseLogin)
    } else if state.confirm_remove {
        Some(Message::CancelRemove)
    } else {
        None
    }
}

/// The page opens (task S.30): it asks for TorroMail's accounts, so the group Mail knows whether to
/// offer the import. The shell sends the returned request when Accounts is chosen or the window
/// starts on it; the answer comes back through [`answered`].
pub fn opened(state: &mut State) -> Action {
    state.asking_torromail = true;
    Action::Send(Request::TorroMailAccounts)
}

/// The page has neither TorroMail's accounts nor asked for them: a window that started on Accounts
/// before the core answered asks with its first good load.
pub fn unlisted(state: &State) -> bool {
    state.torromail.is_none() && !state.asking_torromail
}

/// A section change leaves no account detail or add dialog behind for the other section.
pub fn leave(state: &mut State) {
    state.wizard = None;
    state.selected = None;
    state.models_page = ModelsPage::default();
    close_dialogs(state);
    // The section asks again when it opens; a chain under way and the downloads go on in the core.
    state.slots.asked = false;
    state.slots.asked_models.clear();
}

pub fn update(state: &mut State, message: Message, data: &Data) -> Action {
    match message {
        // The AI providers ask for the servers on this computer as they open.
        // The list then shows that provider, so the new account appears where it was added.
        Message::Add(provider) => {
            if let Some(group) = Group::ALL.into_iter().find(|group| group.provider() == provider) {
                state.provider = Some(group.kinds()[0].to_string());
            }
            let (wizard, ask) = wizard::State::at(provider, data);
            state.wizard = Some(wizard);
            ask
        }
        // The step asks for TorroMail's accounts as it opens (task M7.4).
        Message::ImportTorroMail => {
            state.provider = Some(Group::Mail.kinds()[0].to_string());
            let (wizard, ask) = wizard::State::torromail();
            state.wizard = Some(wizard);
            ask
        }
        // The app key's: only a person lets mail out (task C.17, `Request::kept_for_people`).
        Message::MailToCloud(on) => {
            Action::Send(Request::SetSetting { key: SettingKey::MailToCloud, value: serde_json::Value::Bool(on) })
        }
        // An account may have come from TorroMail or gone: the import asks again.
        Message::Wizard(wizard::Message::Close) => {
            state.wizard = None;
            opened(state)
        }
        Message::Wizard(message) => match state.wizard.as_mut() {
            Some(wizard) => wizard::update(wizard, message, data),
            None => Action::None,
        },
        Message::ShowProvider(key) => {
            state.provider = Some(key);
            Action::None
        }
        // "Back" leads to the account's provider, wherever the list stood before.
        Message::Open(id) => {
            if let Some(account) = listed(&data.accounts).find(|account| account.id == id) {
                state.provider = Some(key_of(&account.kind).to_string());
            }
            state.selected = Some(id);
            close_dialogs(state);
            state.models_page = ModelsPage::default();
            load_models(state, data)
        }
        Message::Back => {
            state.selected = None;
            close_dialogs(state);
            state.models_page = ModelsPage::default();
            Action::None
        }
        Message::SetModel(model, enabled) => set_model(state, data, model, enabled),
        Message::LoadModels => load_models(state, data),
        Message::ModelSearch(search) => {
            state.models_page.search = search;
            Action::None
        }
        Message::ShowClosed(on) => {
            state.models_page.show_closed = on;
            Action::None
        }
        Message::PickSlot(slot, choice) => pick_slot(state, data, slot, choice),
        Message::UseSuggestion(slot) => use_suggestion(state, data, slot),
        Message::CancelPull(slot, id) => ask_ai(Asked::Cancel(slot, id.clone()), Request::CancelAiPull { id }),
        Message::PollPulls => poll_pulls(state),
        Message::Recommend => recommend(state),
        Message::RetrySlot => match state.slots.failed.take() {
            Some(failure) => update(state, *failure.retry, data),
            None => Action::None,
        },
        // Only a person lets mail out; anyone may stop it (`Request::kept_for_people`).
        Message::StopSortingMail => Action::Send(Request::SetSetting {
            key: SettingKey::DecideMailToCloud,
            value: serde_json::Value::Bool(false),
        }),
        Message::SyncNow => Action::Send(Request::Sync),
        Message::OpenApp(app) => Action::Send(Request::OpenApp { app: app.to_string(), target: None }),
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
        Message::CertificateBack => {
            if let Some(change) = state.password.as_mut().filter(|change| !change.sending) {
                change.certificates.dismiss();
                change.password = Secret::default();
            }
            if let Some(dialog) = state.servers.as_mut().filter(|dialog| !dialog.sending) {
                dialog.certificates.dismiss();
                dialog.password = Secret::default();
            }
            Action::None
        }
        Message::AddTopic => {
            let room = selected_ntfy(state, data).is_some_and(|(_, view)| view.topics.len() < TOPICS_MAX);
            state.topic = room.then(TopicEdit::new);
            Action::None
        }
        Message::EditTopic(at) => {
            state.topic = selected_ntfy(state, data).and_then(|(_, view)| view.topics.get(at)).and_then(TopicEdit::of);
            Action::None
        }
        // A topic the account has keeps its name; only a new one is typed.
        Message::TopicNameEdited(name) => {
            edit_topic(state, |edit| {
                if edit.stored.is_none() {
                    edit.name = name;
                }
            });
            Action::None
        }
        Message::TopicLabelEdited(label) => {
            edit_topic(state, |edit| edit.label = label);
            Action::None
        }
        Message::SaveTopic => save_topic(state, data),
        Message::AskUnsubscribe => {
            if let Some(edit) = state.topic.as_mut().filter(|edit| edit.stored.is_some() && !edit.sending) {
                edit.confirm_unsubscribe = true;
            }
            Action::None
        }
        Message::CancelUnsubscribe => {
            if let Some(edit) = state.topic.as_mut() {
                edit.confirm_unsubscribe = false;
            }
            Action::None
        }
        // The confirmation is the last step: a refusal is the shell's notice.
        Message::Unsubscribe => {
            let Some((account, _)) = selected_ntfy(state, data) else { return Action::None };
            if !state.topic.as_ref().is_some_and(|edit| edit.confirm_unsubscribe) {
                return Action::None;
            }
            match state.topic.take() {
                Some(edit) => Action::Send(Request::SetNtfyTopic {
                    account: account.id.clone(),
                    topic: edit.name,
                    label: None,
                    subscribed: false,
                }),
                None => Action::None,
            }
        }
        Message::CloseTopic => {
            state.topic = None;
            Action::None
        }
        Message::ChangeLogin => {
            state.login = selected_ntfy(state, data).map(|(_, view)| LoginChange::of(view));
            Action::None
        }
        Message::LoginKind(token) => {
            edit_login(state, |login| login.token = token);
            Action::None
        }
        Message::LoginUserEdited(user) => {
            edit_login(state, |login| login.user = user);
            Action::None
        }
        Message::LoginSecretEdited(secret) => {
            edit_login(state, |login| login.secret = secret);
            Action::None
        }
        Message::SubmitLogin => submit_login(state, data),
        Message::CloseLogin => {
            state.login = None;
            Action::None
        }
    }
}

/// The open page's AI account; `None` on every other page.
fn selected_ai<'a>(state: &State, data: &'a Data) -> Option<&'a AccountInfo> {
    selected(state, data).filter(|account| account.kind == AI_KIND)
}

/// Asks the open AI account's server for all its models (task S.31), closed ones too: OpenRouter's
/// page filters them itself ([`shown_models`]). Nothing for any other page.
fn load_models(state: &mut State, data: &Data) -> Action {
    let Some(account) = selected_ai(state, data) else { return Action::None };
    state.models_page.loading = true;
    state.models_page.error = None;
    Action::Send(Request::AiModels { account: account.id.clone(), all: true })
}

/// A model's switch on the open AI account's page: the new state shows at once and goes to the
/// core, which may refuse it ([`answered`] turns it back then). Nothing for a model that can't use
/// tools (it has no switch), one the page does not list, or a state it already has.
fn set_model(state: &mut State, data: &Data, model: String, enabled: bool) -> Action {
    let Some(account) = selected_ai(state, data).map(|account| account.id.clone()) else { return Action::None };
    let Some(listed) = state.models.get_mut(&account).and_then(|models| models.iter_mut().find(|m| m.id == model))
    else {
        return Action::None;
    };
    if !has_switch(listed) || listed.enabled == enabled {
        return Action::None;
    }
    listed.enabled = enabled;
    state.models_page.refused = None;
    state.switching.push_back(ModelSwitch { account: account.clone(), model: model.clone(), enabled });
    Action::Send(Request::SetModelEnabled { account, model, enabled })
}

/// The answers the AI accounts' models wait for (task S.31), after their lists: the oldest switch
/// on its way takes `ok` or an error, a refused one is turned back and its reason shown on the
/// open page; else an error while the open page loads is why its models could not be listed.
/// Returns whether one of them took the answer.
fn models_answered(state: &mut State, result: &Result<Response, String>) -> bool {
    let refusal = match result {
        Ok(Response::Error { message, .. }) | Err(message) => Some(message.clone()),
        _ => None,
    };
    if !state.switching.is_empty() && (refusal.is_some() || matches!(result, Ok(Response::Ok))) {
        let Some(switch) = state.switching.pop_front() else { return false };
        if let Some(message) = refusal {
            let models = state.models.get_mut(&switch.account).into_iter().flatten();
            for model in models.filter(|model| model.id == switch.model) {
                model.enabled = !switch.enabled;
            }
            if state.selected.as_deref() == Some(switch.account.as_str()) {
                state.models_page.refused = Some(message);
            }
        }
        return true;
    }
    match refusal {
        Some(message) if state.models_page.loading => {
            state.models_page.loading = false;
            state.models_page.error = Some(message);
            true
        }
        _ => false,
    }
}

// ----- The slots (issue #325, task MW.8) ---------------------------------------------------------

/// One request of the group "Models", tagged with what it is for.
fn ask_ai(asked: Asked, request: Request) -> Action {
    Action::AskAi(vec![(asked, request)])
}

/// The AI accounts the section lists (not a sign-in in progress), in the order of the list.
fn ai_accounts(data: &Data) -> impl Iterator<Item = &AccountInfo> {
    listed(&data.accounts).filter(|account| account.kind == AI_KIND && !account.id.is_empty())
}

/// The AI section is shown (chosen, or reloaded while open): asks for what the group "Models" still
/// lacks – the suggestion and the downloads once per opening, and the models of every AI account
/// not asked for yet (`all`, as an account's page asks, so both share [`State::models`]). The shell
/// calls it when the section is chosen, with every good reload while it is open, and at a start on
/// it. Without an AI account there is no group, so nothing is asked.
pub fn ai_shown(state: &mut State, data: &Data) -> Action {
    let accounts: Vec<String> = ai_accounts(data).map(|account| account.id.clone()).collect();
    if accounts.is_empty() {
        return Action::None;
    }
    let mut asks = Vec::new();
    if !state.slots.asked {
        state.slots.asked = true;
        if !state.slots.recommending {
            state.slots.recommending = true;
            asks.push((Asked::Recommend, Request::AiRecommend));
        }
        if !state.slots.polling {
            state.slots.polling = true;
            asks.push((Asked::Pulls, Request::AiPulls));
        }
    }
    for account in accounts {
        if !state.models.contains_key(&account) && state.slots.asked_models.insert(account.clone()) {
            asks.push((Asked::Models(account.clone()), Request::AiModels { account, all: true }));
        }
    }
    if asks.is_empty() { Action::None } else { Action::AskAi(asks) }
}

/// "Retry" after `ai.recommend` failed, and the fresh suggestion after a slot changed.
fn recommend(state: &mut State) -> Action {
    if state.slots.recommending {
        return Action::None;
    }
    state.slots.recommending = true;
    ask_ai(Asked::Recommend, Request::AiRecommend)
}

/// A tick while a download runs: `ai.list_pulls`, unless the last one has not answered.
fn poll_pulls(state: &mut State) -> Action {
    if state.slots.polling || !running(&state.slots.pulls) {
        return Action::None;
    }
    state.slots.polling = true;
    ask_ai(Asked::Pulls, Request::AiPulls)
}

/// A download runs: [`subscription`] ticks.
fn running(pulls: &[AiPull]) -> bool {
    pulls.iter().any(|pull| pull.state == AiPullState::Running)
}

/// Every second while a download runs (task MW.8, `ai.list_pulls`), else nothing: the page does not
/// tick while it waits for nothing.
pub fn subscription(state: &State) -> Subscription<Message> {
    match running(&state.slots.pulls) {
        true => iced::time::every(Duration::from_secs(1)).map(|_| Message::PollPulls),
        false => Subscription::none(),
    }
}

/// `name` with its tag, as Ollama lists it: `nimble` → `nimble:latest`. Only the part after the last
/// `/` holds a tag, since a registry host may name a port (`host:5000/x`).
fn with_tag(name: &str) -> String {
    let last = name.rsplit('/').next().unwrap_or(name);
    if last.contains(':') { name.to_string() } else { format!("{name}:latest") }
}

/// Two names are one model: equal once a name without a tag has the tag `latest`. The suggestion and a
/// download name a model as the list of recommended models does (`nimble`), the account by the id it
/// lists (`nimble:latest`).
pub fn same_model(a: &str, b: &str) -> bool {
    a == b || with_tag(a) == with_tag(b)
}

/// The suggestion of `ai.recommend` for `slot`, if there is one.
fn suggestion_of(state: &State, slot: AiSlot) -> Option<&SlotSuggestion> {
    state.slots.recommendation.as_ref()?.slots.iter().find(|suggestion| suggestion.slot == slot)
}

/// The latest download of `model` into `account` the core listed.
fn pull_of<'a>(pulls: &'a [AiPull], account: &str, model: &str) -> Option<&'a AiPull> {
    pulls.iter().rev().find(|pull| pull.account == account && same_model(&pull.model, model))
}

/// The effort a model put in `slot` keeps: the chat keeps the one it has (as Assistant › General
/// does), sorting takes none – a decision model ignores it.
fn kept_effort(data: &Data, slot: AiSlot) -> Option<Effort> {
    match slot {
        AiSlot::Chat => data.settings.ai_slots.chat.as_ref().and_then(|chosen| chosen.effort),
        AiSlot::Decide => None,
    }
}

/// The slot has this account and model already (the model by [`same_model`]); `None` and `None` for an
/// empty slot.
fn holds(data: &Data, slot: AiSlot, engine: Option<&str>, model: Option<&str>) -> bool {
    let chosen = data.settings.ai_slots.get(slot);
    match (chosen, engine) {
        (None, None) => true,
        (Some(chosen), Some(engine)) => {
            chosen.engine == engine
                && match (chosen.model.as_deref(), model) {
                    (None, None) => true,
                    (Some(a), Some(b)) => same_model(a, b),
                    _ => false,
                }
        }
        _ => false,
    }
}

/// The exception of sorting a chain for `slot` takes back before `ai.set_slot` ([`Using::displaces`]):
/// the one stored, on or off, for Sorting only – while it is, the slot Sorting does not sort. The
/// settings hold it as the core weighs it (`assignment_in_effect`).
fn displaced(data: &Data, slot: AiSlot) -> Option<torro_core::ai::AiAssignment> {
    let stored = data.settings.ai_assignments.get(&AiFunction::InboxCategorize);
    stored.filter(|held| slot == AiSlot::Decide && held.engine.is_some()).cloned()
}

/// A choice in a slot's list: `ai.set_slot` at once – for Sorting the stored exception taken back first,
/// from the data of the press, which is now –, with "Retry" sending the same choice. Nothing while
/// another chain runs, or for what the slot has already while no exception stands in its way.
fn pick_slot(state: &mut State, data: &Data, slot: AiSlot, choice: SlotChoice) -> Action {
    let displaces = displaced(data, slot);
    let held = holds(data, slot, choice.engine.as_deref(), choice.model.as_deref());
    if state.slots.using.is_some() || (held && displaces.is_none()) {
        return Action::None;
    }
    state.slots.failed = None;
    let take_back = displaces.is_some();
    let mut using = Using {
        slot,
        engine: choice.engine.clone(),
        model: choice.model.clone(),
        effort: kept_effort(data, slot),
        displaces,
        stale_poll: false,
        step: UseStep::Set,
        retry: Box::new(Message::PickSlot(slot, choice)),
    };
    let request = match take_back {
        true => {
            using.step = UseStep::TakeBack;
            no_exception()
        }
        false => set_slot_request(&using),
    };
    state.slots.using = Some(using);
    ask_ai(Asked::Slot(slot), request)
}

/// "Use suggestion": a model to download starts with `ai.pull_model` – or waits for a download of it
/// that runs already (another window, an agent; only one that is `running`; one the core knows of but
/// the page not yet is joined when the start answers `busy`, [`UseStep::Join`]) –, any other with
/// `ai.list_models`. For Sorting a stored exception is taken back before the slot is set, if it still
/// stands then ([`Using::displaces`]).
/// Nothing while another chain runs, without a suggestion or for one without an account (it shows why
/// instead).
fn use_suggestion(state: &mut State, data: &Data, slot: AiSlot) -> Action {
    if state.slots.using.is_some() {
        return Action::None;
    }
    let Some(suggestion) = suggestion_of(state, slot).cloned() else { return Action::None };
    let Some(engine) = suggestion.engine.clone() else { return Action::None };
    state.slots.failed = None;
    let mut using = Using {
        slot,
        engine: Some(engine.clone()),
        model: suggestion.model.clone(),
        effort: kept_effort(data, slot),
        displaces: displaced(data, slot),
        stale_poll: false,
        step: UseStep::List,
        retry: Box::new(Message::UseSuggestion(slot)),
    };
    let download = suggestion.download_bytes.and(suggestion.model.clone());
    let action = match download {
        Some(model) => match pull_of(&state.slots.pulls, &engine, &model).filter(|p| p.state == AiPullState::Running) {
            Some(pull) => {
                using.step = UseStep::Pull(Some(pull.id.clone()));
                Action::None
            }
            None => {
                using.step = UseStep::Pull(None);
                using.stale_poll = state.slots.polling;
                ask_ai(Asked::Slot(slot), Request::PullAiModel { account: engine, model })
            }
        },
        None => ask_ai(Asked::Slot(slot), Request::AiModels { account: engine, all: true }),
    };
    state.slots.using = Some(using);
    action
}

/// `ai.set_slot` for the chain's account and model.
fn set_slot_request(using: &Using) -> Request {
    Request::SetAiSlot {
        slot: using.slot,
        engine: using.engine.clone(),
        model: using.model.clone(),
        effort: using.effort,
    }
}

/// The model is ready for the slot: `ai.set_slot` – for Sorting, while the chain started with an
/// exception, first `ai.list_assignments`, whose answer says whether that one still stands
/// ([`UseStep::Check`]).
fn toward_slot(using: &mut Using) -> Action {
    let slot = using.slot;
    if using.displaces.is_some() {
        using.step = UseStep::Check;
        return ask_ai(Asked::Slot(slot), Request::AiAssignments);
    }
    using.step = UseStep::Set;
    ask_ai(Asked::Slot(slot), set_slot_request(using))
}

/// The chain stops at a step that failed: the sentence under its slot with "Retry".
fn stop(state: &mut State, message: String) -> Action {
    if let Some(using) = state.slots.using.take() {
        state.slots.failed = Some(SlotFailure { slot: using.slot, message, retry: using.retry });
    }
    Action::None
}

/// The answer to a request of the group "Models" ([`Action::AskAi`]), by what it was for. The chain
/// of [`SlotsPage::using`] goes on with the next step or stops with the reason; a list of downloads
/// may finish the download it waits for. Returns the next request, if any.
pub fn ai_answered(state: &mut State, asked: Asked, result: &Result<Response, String>) -> Action {
    match asked {
        Asked::Recommend => {
            state.slots.recommending = false;
            match result {
                Ok(Response::AiRecommendation { recommendation }) => {
                    state.slots.recommendation = Some(recommendation.clone());
                    state.slots.recommend_error = None;
                }
                _ => state.slots.recommend_error = failure(result, |_| false),
            }
            Action::None
        }
        Asked::Pulls => {
            state.slots.polling = false;
            match result {
                Ok(Response::AiPulls { pulls }) => polled(state, pulls.clone()),
                // A list that could not be read leaves the last one; the next tick asks again. It was
                // the poll that went out before a start, if one did: the next answer is no older.
                _ => {
                    if let Some(using) = state.slots.using.as_mut() {
                        using.stale_poll = false;
                    }
                    Action::None
                }
            }
        }
        // An account whose models cannot be read offers none in the lists; asked once per opening.
        Asked::Models(_) => {
            if let Ok(Response::AiModels { account, models }) = result {
                state.models.insert(account.clone(), models.clone());
            }
            Action::None
        }
        Asked::Cancel(slot, id) => match result {
            Ok(Response::AiPulls { pulls }) => pulls_answered(state, pulls.clone()),
            _ => {
                let message = failure(result, |_| false).unwrap_or_default();
                // The download may still run: the chain waiting for it stays, and "Retry" cancels the
                // same one again – never another that runs (review of #325).
                let still = state.slots.pulls.iter().any(|pull| pull.id == id && pull.state == AiPullState::Running);
                let retry = Box::new(match still {
                    true => Message::CancelPull(slot, id),
                    false => Message::PollPulls,
                });
                state.slots.failed = Some(SlotFailure { slot, message, retry });
                Action::None
            }
        },
        Asked::Slot(slot) => step_answered(state, slot, result),
    }
}

/// The answer to the step of the chain under `slot`; a stale one (the chain has stopped) is dropped.
fn step_answered(state: &mut State, slot: AiSlot, result: &Result<Response, String>) -> Action {
    let Some(step) = state.slots.using.as_ref().filter(|using| using.slot == slot).map(|using| using.step.clone())
    else {
        return Action::None;
    };
    match step {
        // Another window or an agent downloads the model already: the chain joins that download.
        UseStep::Pull(None) if is_busy(result) => {
            let message = failure(result, |_| false).unwrap_or_default();
            if let Some(using) = state.slots.using.as_mut() {
                using.step = UseStep::Join(message);
            }
            ask_ai(Asked::Slot(slot), Request::AiPulls)
        }
        UseStep::Pull(_) => match failure(result, |response| matches!(response, Response::AiPulls { .. })) {
            Some(message) => stop(state, message),
            None => match result {
                Ok(Response::AiPulls { pulls }) => started(state, pulls.clone()),
                _ => Action::None,
            },
        },
        UseStep::Join(busy) => match (failure(result, |response| matches!(response, Response::AiPulls { .. })), result)
        {
            (None, Ok(Response::AiPulls { pulls })) => joined(state, pulls.clone(), busy),
            (message, _) => stop(state, message.unwrap_or(busy)),
        },
        UseStep::List => match (failure(result, |response| matches!(response, Response::AiModels { .. })), result) {
            (None, Ok(Response::AiModels { account, models })) => {
                state.models.insert(account.clone(), models.clone());
                model_listed(state, models)
            }
            (message, _) => stop(state, message.unwrap_or_default()),
        },
        UseStep::Enable => match failure(result, |response| matches!(response, Response::Ok)) {
            Some(message) => stop(state, message),
            None => {
                let Some(using) = state.slots.using.as_mut() else { return Action::None };
                let (engine, model) = (using.engine.clone().unwrap_or_default(), using.model.clone());
                let next = toward_slot(using);
                let models = state.models.get_mut(&engine).into_iter().flatten();
                for entry in models.filter(|entry| Some(&entry.id) == model.as_ref()) {
                    entry.enabled = true;
                }
                next
            }
        },
        UseStep::Check => {
            match (failure(result, |response| matches!(response, Response::AiAssignments { .. })), result) {
                (None, Ok(Response::AiAssignments { assignments, .. })) => {
                    let stored = assignments
                        .iter()
                        .map(|info| &info.assignment)
                        .find(|held| held.function == AiFunction::InboxCategorize && held.engine.is_some());
                    let Some(using) = state.slots.using.as_mut() else { return Action::None };
                    // Only the exception the chain started with, as it was: one changed or set meanwhile is
                    // the person's newer choice and stays.
                    match stored.is_some() && stored == using.displaces.as_ref() {
                        true => {
                            using.step = UseStep::TakeBack;
                            ask_ai(Asked::Slot(slot), no_exception())
                        }
                        false => {
                            using.step = UseStep::Set;
                            ask_ai(Asked::Slot(slot), set_slot_request(using))
                        }
                    }
                }
                (message, _) => stop(state, message.unwrap_or_default()),
            }
        }
        // Taken back: now the slot. Refused: the chain stops, the slot stays as it was.
        UseStep::TakeBack => match failure(result, |response| matches!(response, Response::AiAssignments { .. })) {
            Some(message) => stop(state, message),
            None => {
                let Some(using) = state.slots.using.as_mut() else { return Action::None };
                using.step = UseStep::Set;
                ask_ai(Asked::Slot(slot), set_slot_request(using))
            }
        },
        UseStep::Set => match failure(result, |response| matches!(response, Response::AiAssignments { .. })) {
            Some(message) => stop(state, message),
            None => finished(state),
        },
    }
}

/// The core refused with `busy`: for `ai.pull_model`, the same model downloads to the same account.
fn is_busy(result: &Result<Response, String>) -> bool {
    matches!(result, Ok(Response::Error { code, .. }) if code == "busy")
}

/// The list of downloads after a start that was `busy` ([`UseStep::Join`]): the newest download of the
/// chain's model that runs is the one it follows from now on, by its id; without one it stops with the
/// core's sentence `busy`.
fn joined(state: &mut State, pulls: Vec<AiPull>, busy: String) -> Action {
    let Some(using) = state.slots.using.as_mut() else { return Action::None };
    let (engine, model) = (using.engine.clone().unwrap_or_default(), using.model.clone().unwrap_or_default());
    let runs = pulls
        .iter()
        .rev()
        .find(|pull| pull.state == AiPullState::Running && pull.account == engine && same_model(&pull.model, &model));
    match runs {
        Some(pull) => {
            using.step = UseStep::Pull(Some(pull.id.clone()));
            pulls_answered(state, pulls)
        }
        None => {
            state.slots.pulls = pulls;
            stop(state, busy)
        }
    }
}

/// The chain is through: nothing more to send but a fresh suggestion, which may have changed with it (a
/// model downloaded, the slot taken).
fn finished(state: &mut State) -> Action {
    state.slots.using = None;
    recommend(state)
}

/// `ai.set_assignment inbox.categorize` with `engine: null`: the exception taken back, as Settings ›
/// Categorization sends it.
fn no_exception() -> Request {
    Request::SetAiAssignment {
        function: AiFunction::InboxCategorize,
        engine: None,
        model: None,
        effort: None,
        enabled: false,
    }
}

/// The account listed its models for the chain: the model by the id it lists (`nimble:latest`), turned
/// on first if it is off, then put in the slot. One the account does not list stops the chain; an
/// account left to pick a model (`model: None`) goes straight to `ai.set_slot`.
fn model_listed(state: &mut State, models: &[AiModel]) -> Action {
    let Some(using) = state.slots.using.as_mut() else { return Action::None };
    let slot = using.slot;
    let Some(wanted) = using.model.clone() else { return toward_slot(using) };
    let Some(model) = models.iter().find(|model| same_model(&model.id, &wanted)) else {
        return stop(state, trf("The account does not list {model}.", &[("model", &wanted)]));
    };
    using.model = Some(model.id.clone());
    if model.enabled {
        return toward_slot(using);
    }
    using.step = UseStep::Enable;
    let account = using.engine.clone().unwrap_or_default();
    ask_ai(Asked::Slot(slot), Request::SetModelEnabled { account, model: model.id.clone(), enabled: true })
}

/// The answer to `ai.pull_model`: the downloads of the run, the newest of the chain's model the one it
/// started – from here on the chain follows that id alone ([`pulls_answered`]).
fn started(state: &mut State, pulls: Vec<AiPull>) -> Action {
    let Some(using) = state.slots.using.as_mut().filter(|using| using.step == UseStep::Pull(None)) else {
        return pulls_answered(state, pulls);
    };
    let (engine, model) = (using.engine.clone().unwrap_or_default(), using.model.clone().unwrap_or_default());
    match pull_of(&pulls, &engine, &model) {
        Some(pull) => {
            using.step = UseStep::Pull(Some(pull.id.clone()));
            pulls_answered(state, pulls)
        }
        None => {
            state.slots.pulls = pulls;
            stop(state, tr("The download stopped. Try again.").to_string())
        }
    }
}

/// The answer to a tick's `ai.list_pulls` (review of #325). Dropped while the chain's own start is out
/// (it does not know its download yet, and an older one of the same model must never stand in) and when
/// it was asked before that start went out (it may lack the new download). Else as every list.
fn polled(state: &mut State, pulls: Vec<AiPull>) -> Action {
    if let Some(using) = state.slots.using.as_mut()
        && (std::mem::take(&mut using.stale_poll) || using.step == UseStep::Pull(None))
    {
        return Action::None;
    }
    pulls_answered(state, pulls)
}

/// The core listed its downloads: they replace the last list, and the download the chain waits for
/// (by its id, [`started`]) moves it on – `done` to `ai.list_models`, `failed` stops it with the core's
/// reason, `cancelled` ends it quietly (the person pressed "Cancel"). A download the list no longer has
/// (the core started again) stops it too.
fn pulls_answered(state: &mut State, pulls: Vec<AiPull>) -> Action {
    state.slots.pulls = pulls;
    let Some(using) = state.slots.using.as_mut() else { return Action::None };
    let UseStep::Pull(Some(id)) = &using.step else { return Action::None };
    let engine = using.engine.clone().unwrap_or_default();
    let Some(pull) = state.slots.pulls.iter().find(|pull| &pull.id == id).cloned() else {
        return stop(state, tr("The download stopped. Try again.").to_string());
    };
    match pull.state {
        AiPullState::Running => Action::None,
        AiPullState::Done => {
            using.step = UseStep::List;
            ask_ai(Asked::Slot(using.slot), Request::AiModels { account: engine, all: true })
        }
        AiPullState::Failed => {
            stop(state, pull.error.unwrap_or_else(|| tr("The download stopped. Try again.").to_string()))
        }
        AiPullState::Cancelled => {
            state.slots.using = None;
            Action::None
        }
    }
}

/// Every dialog and confirmation of a detail page closed: another page opens.
fn close_dialogs(state: &mut State) {
    state.confirm_remove = false;
    state.password = None;
    state.servers = None;
    state.topic = None;
    state.login = None;
}

/// The open page's ntfy account with its [`NtfyView`]; `None` on every other page.
fn selected_ntfy<'a>(state: &State, data: &'a Data) -> Option<(&'a AccountInfo, &'a NtfyView)> {
    let account = selected(state, data)?;
    Some((account, account.ntfy.as_ref()?))
}

/// Changes the open topic dialog and clears the refusal of the last try.
fn edit_topic(state: &mut State, change: impl FnOnce(&mut TopicEdit)) {
    if let Some(edit) = state.topic.as_mut().filter(|edit| !edit.sending) {
        change(edit);
        edit.error = None;
    }
}

/// Changes the open login dialog and clears the refusal of the last try.
fn edit_login(state: &mut State, change: impl FnOnce(&mut wizard::NtfyLogin)) {
    if let Some(dialog) = state.login.as_mut().filter(|dialog| !dialog.sending) {
        change(&mut dialog.login);
        dialog.error = None;
    }
}

/// "Subscribe" or "Save" in the topic dialog: from now on it says what is wrong; once nothing is,
/// sends `SetNtfyTopic` and waits. A label left as it was changes nothing, so nothing is sent.
fn save_topic(state: &mut State, data: &Data) -> Action {
    let Some((account, _)) = selected_ntfy(state, data) else { return Action::None };
    let Some(edit) = state.topic.as_mut().filter(|edit| !edit.sending) else { return Action::None };
    edit.tried = true;
    if edit.problem().is_some() {
        return Action::None;
    }
    let label = edit.label.trim().to_string();
    if edit.stored.as_deref().is_some_and(|stored| stored == label) {
        state.topic = None;
        return Action::None;
    }
    edit.sending = true;
    edit.error = None;
    let topic = TopicName::new(edit.name.as_str().trim());
    Action::Send(Request::SetNtfyTopic { account: account.id.clone(), topic, label: Some(label), subscribed: true })
}

/// "Save" in "Change login": sends the login for the account's own server and waits; the topics
/// stay as they are. The dialog keeps no copy of the secret.
fn submit_login(state: &mut State, data: &Data) -> Action {
    let Some((_, view)) = selected_ntfy(state, data) else { return Action::None };
    let Some(dialog) = state.login.as_mut().filter(|dialog| !dialog.sending && dialog.login.is_ready()) else {
        return Action::None;
    };
    let (user, secret) = dialog.login.take();
    dialog.sending = true;
    dialog.error = None;
    Action::Send(Request::SetNtfyLogin { server: view.server.clone(), topics: Vec::new(), user, secret })
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

/// Sends the change `make` finds for the open mail account, if there is one.
fn with_mail(state: &State, data: &Data, make: impl FnOnce(&MailView) -> Option<MailChange>) -> Action {
    let Some((account, mail)) = selected_mail(state, data) else { return Action::None };
    make(mail)
        .filter(|change| !change.is_empty())
        .map_or(Action::None, |change| Action::Send(change.request(&account.id)))
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
/// dialog was opened for is switched on with it (task M3.5). On the question about a certificate
/// it trusts that one first, and every certificate trusted in the dialog goes along (task M11.4).
/// The request gets a copy of the password, which [`answered`] clears with every answer but that
/// question.
fn submit_servers(state: &mut State, data: &Data) -> Action {
    let Some((account, mail)) = selected_mail(state, data) else { return Action::None };
    let Some(dialog) = state.servers.as_mut().filter(|dialog| dialog.can_send()) else { return Action::None };
    let (Some(imap), Some(smtp)) = (dialog.imap.endpoint(), dialog.smtp.endpoint()) else { return Action::None };
    dialog.certificates.trust();
    dialog.sending = true;
    dialog.error = None;
    let services = mail.settings.services;
    Action::Send(Request::ConnectMail {
        address: mail.address.clone(),
        sender_name: String::new(),
        imap,
        smtp,
        login_name: login_name(account, mail),
        password: dialog.password.clone(),
        calendars: services.calendars || dialog.adding == Some(MailService::Calendars),
        tasks: services.tasks || dialog.adding == Some(MailService::Tasks),
        caldav: dialog.caldav_address(),
        trusted_certificates: dialog.certificates.trusted(),
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
/// name keeps the stored one). `ConnectCalDav` takes the password, so the dialog keeps no copy;
/// `ConnectMail` gets a copy, which [`answered`] clears with every answer but the question about a
/// certificate (task M11.4). On that question it trusts the certificate first, and every
/// certificate trusted in the dialog goes along.
fn submit_password(state: &mut State, data: &Data) -> Action {
    let Some(account) = selected(state, data) else { return Action::None };
    let Some(change) = state.password.as_mut().filter(|change| !change.sending && !change.password.is_blank()) else {
        return Action::None;
    };
    change.certificates.trust();
    let password = match account.mail {
        Some(_) => change.password.clone(),
        None => std::mem::take(&mut change.password),
    };
    let Some(request) = password_request(account, password, change.certificates.trusted()) else {
        return Action::None;
    };
    change.sending = true;
    change.error = None;
    Action::Send(request)
}

/// The request that signs `account` in again with `password`; a mail account's with the
/// certificates `trusted` in the dialog (a CalDAV account has none).
fn password_request(account: &AccountInfo, password: Secret, trusted: Vec<String>) -> Option<Request> {
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
            trusted_certificates: trusted,
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
/// server dialog take the answers to their own requests ([`wizard::answered`]), the page TorroMail's
/// accounts it asked for ([`opened`]) and an AI account's models and switches (task S.31). Returns
/// whether one did; then the shell shows no notice.
pub fn answered(state: &mut State, result: &Result<Response, String>) -> bool {
    // An AI account's models (task S.31) name their account: whoever asked, they are the latest.
    if let Ok(Response::AiModels { account, models }) = result {
        if state.selected.as_deref() == Some(account.as_str()) {
            state.models_page.loading = false;
            state.models_page.error = None;
        }
        state.models.insert(account.clone(), models.clone());
        return true;
    }
    // The list is the page's whoever asked; an open step TorroMail gets it too.
    let listed = match result {
        Ok(Response::TorroMailAccounts { accounts }) => {
            state.torromail = Some(accounts.clone());
            state.asking_torromail = false;
            true
        }
        _ => false,
    };
    if let Some(wizard) = state.wizard.as_mut() {
        return wizard::answered(wizard, result) || listed;
    }
    if listed || models_answered(state, result) {
        return true;
    }
    // Both dialogs keep the password only while a certificate is asked about (task M11.4).
    if let Some(change) = state.password.as_mut().filter(|change| change.sending) {
        change.sending = false;
        if change.certificates.ask(result) {
            return true;
        }
        change.password = Secret::default();
        match refusal(result) {
            None => state.password = None,
            Some(message) => change.error = Some(message),
        }
        return true;
    }
    if let Some(dialog) = state.servers.as_mut().filter(|dialog| dialog.sending) {
        dialog.sending = false;
        if dialog.certificates.ask(result) {
            return true;
        }
        dialog.password = Secret::default();
        match refusal(result) {
            None => state.servers = None,
            Some(message) => dialog.error = Some(message),
        }
        return true;
    }
    if let Some(edit) = state.topic.as_mut().filter(|edit| edit.sending) {
        match failure(result, |response| matches!(response, Response::Ok)) {
            None => state.topic = None,
            Some(message) => {
                edit.sending = false;
                edit.error = Some(message);
            }
        }
        return true;
    }
    if let Some(dialog) = state.login.as_mut().filter(|dialog| dialog.sending) {
        match failure(result, |response| matches!(response, Response::Account { .. })) {
            None => state.login = None,
            Some(message) => {
                dialog.sending = false;
                dialog.error = Some(message);
            }
        }
        return true;
    }
    // Nothing else of the page waits: an error is the answer to TorroMail's list. The import is a
    // shortcut, so a list that cannot be read only leaves the button out, quietly.
    if state.asking_torromail && matches!(result, Err(_) | Ok(Response::Error { .. })) {
        state.asking_torromail = false;
        state.torromail.get_or_insert_default();
        return true;
    }
    false
}

/// What went wrong with a request whose good answer `is_done` knows; `None` when it went through.
fn failure(result: &Result<Response, String>, is_done: impl Fn(&Response) -> bool) -> Option<String> {
    match result {
        Ok(Response::Error { message, .. }) | Err(message) => Some(message.clone()),
        Ok(response) if is_done(response) => None,
        Ok(_) => Some(tr("Torro Workplace did not answer as expected. Try again.").to_string()),
    }
}

/// What went wrong with a request that signs an account in: `None` when the account connected.
fn refusal(result: &Result<Response, String>) -> Option<String> {
    match result {
        Ok(Response::Connected { .. } | Response::MailConnected { .. }) => None,
        Ok(Response::Error { message, .. }) | Err(message) => Some(message.clone()),
        Ok(_) => Some(tr("Torro Workplace did not answer as expected. Try again.").to_string()),
    }
}

/// The page in `class`: an account's detail page, else the list of the chosen provider – in the
/// narrow class with the providers as chips above it, in the others beside [`provider_column`].
/// `palette` colours a provider whose accounts need the person ([`provider_mark`]).
pub fn view<'a>(state: &'a State, data: &'a Data, class: SizeClass, palette: &Palette) -> Element<'a, Message> {
    let now = Utc::now();
    match selected(state, data) {
        Some(account) => detail(state, account, data, now),
        None => list(state, data, now, !beside(class), palette, false, false),
    }
}

/// The dedicated AI section uses the same account details, model switches and add dialog; in the
/// narrow `class` the lists of the slots stand under their names (task MW.8).
pub fn ai_view<'a>(state: &'a State, data: &'a Data, class: SizeClass, palette: &Palette) -> Element<'a, Message> {
    let now = Utc::now();
    match selected(state, data) {
        Some(account) => detail(state, account, data, now),
        None => list(state, data, now, false, palette, true, !beside(class)),
    }
}

/// The providers as a column at the left edge of the content (task S.37), for the shell to set
/// beside the scrolling page: with names and counts where the content beside the sidebar (`content`,
/// [`shell::content_width`]) leaves the list its reading width beside them, else only their icons,
/// names and counts as tooltips ([`column_mode`], issue #248). `None` on a detail page and in the
/// narrow class, where the list has chips instead.
pub fn provider_column<'a>(
    state: &'a State,
    data: &'a Data,
    class: SizeClass,
    content: f32,
    palette: &Palette,
) -> Option<Element<'a, Message>> {
    if selected(state, data).is_some() {
        return None;
    }
    let mode = column_mode(class, content)?;
    let width = column_width(mode);
    let entries = entries(data);
    let groups = account_providers(&entries);
    let shown = chosen(state.provider.as_deref(), &groups, &entries);
    let items = groups.iter().map(|&group| {
        let count = accounts_in(group, &entries);
        widgets::sidebar_item_in(
            mode,
            provider_mark(group, &entries, palette),
            group.title(),
            if count > 0 { Trail::Count(count) } else { Trail::None },
            group == shown,
            Message::ShowProvider(group.key().to_string()),
        )
    });
    // Scrolls on its own, so a low window still reaches the last provider.
    let items = container(column(items).spacing(space::xs()).width(Fill)).padding(space::s());
    let items =
        scrollable(items).direction(scrollable::Direction::Vertical(style::scrollbar())).style(style::scrollable);
    Some(container(items).width(width).height(Fill).into())
}

/// How the providers' column is drawn beside a content `content` wide in `class`: with names from
/// [`layout::wide_column_from`] on, as a rail below it, none in the narrow class and in tiles. The
/// content width decides and not the class, so the list does not get narrower when the window grows
/// past the medium class (issue #248).
fn column_mode(class: SizeClass, content: f32) -> Option<SidebarMode> {
    match class {
        SizeClass::Wide | SizeClass::Medium if content >= layout::wide_column_from() => Some(SidebarMode::Expanded),
        SizeClass::Wide | SizeClass::Medium => Some(SidebarMode::Rail),
        SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip => None,
    }
}

/// The width of the providers' column in `mode`: the sidebar's or the rail's.
fn column_width(mode: SidebarMode) -> f32 {
    match mode {
        SidebarMode::Expanded => size::sidebar(),
        SidebarMode::Rail => shell::rail(),
    }
}

/// Whether the providers stand in a column beside the list ([`provider_column`]): wide and medium.
fn beside(class: SizeClass) -> bool {
    matches!(class, SizeClass::Wide | SizeClass::Medium)
}

/// The accounts, then the sign-ins in progress, so a new row appears below the others.
fn entries(data: &Data) -> Vec<&AccountInfo> {
    listed(&data.accounts).chain(pending(&data.accounts)).collect()
}

/// The page's dialog over the whole window: the add-account dialog, else the password dialog, else
/// the server dialog, else the "Remove this account?" confirmation; `None` while none is open.
pub fn overlay<'a>(state: &'a State, data: &'a Data) -> Option<Element<'a, Message>> {
    if let Some(wizard) = &state.wizard {
        let content = wizard::view(wizard, data).map(Message::Wizard);
        return Some(dialog(content, size::form_max(), Message::Wizard(wizard::Message::Close)));
    }
    if let Some(change) = &state.password {
        let account = selected(state, data)?;
        return Some(password_dialog(change, account));
    }
    if let Some(dialog) = &state.servers {
        let (account, mail) = selected_mail(state, data)?;
        return Some(servers_dialog(dialog, account, mail));
    }
    if let Some(edit) = &state.topic {
        let (account, _) = selected_ntfy(state, data)?;
        return Some(if edit.confirm_unsubscribe { unsubscribe_confirm(edit) } else { topic_dialog(edit, account) });
    }
    if let Some(change) = &state.login {
        let (_, view) = selected_ntfy(state, data)?;
        return Some(login_dialog(change, view));
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
    if let Some(asked) = change.certificates.shown() {
        return certificate_dialog(asked, account, change.sending, Message::SubmitPassword, Message::ClosePassword);
    }
    let can_send = !change.sending && !change.password.is_blank();
    let input = wizard::field("", change.password.expose())
        .secure(true)
        .on_input(|password| Message::PasswordEdited(Secret::new(password)))
        .on_submit_maybe(can_send.then_some(Message::SubmitPassword));
    let mut body = column![
        typo::text(typo::title(), tr("Change password")),
        typo::text(typo::body(), trf("Enter the new password for {account}.", &[("account", &account.label)]))
            .style(theme::text_2),
        widgets::group(tr("Password"), vec![row_frame(input, false)], None),
    ]
    .spacing(space::l());
    if let Some(error) = &change.error {
        body = body.push(
            typo::text(typo::body(), error.as_str())
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
    .spacing(space::s())
    .align_y(Alignment::Center);
    dialog(body.push(buttons), widgets::POPOVER_WIDTH, Message::ClosePassword)
}

/// The header bar of the page: on an account's detail its label with a back button to the list.
/// The list has no button of its own since task S.30: each group carries its "Add", so there is
/// no primary button in every size class.
pub fn header<'a>(state: &'a State, data: &'a Data, _class: SizeClass) -> Header<'a, Message> {
    let sub_page = selected(state, data).map(|account| (account.label.clone(), Message::Back));
    Header { sub_page, actions: Vec::new() }
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
        "ai" => tr("AI").to_string(),
        NTFY_KIND => "ntfy".to_string(),
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

/// How an account stands, from fine to failing: the order is what [`group_tone`] takes the worst
/// of.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
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

/// A fixed group of the list (task S.30, `docs/settings.md` › Accounts): always shown, also
/// without an account, in the order of [`Group::ALL`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Nextcloud,
    /// Mail accounts and the CalDAV accounts of a mail provider (kinds `mail` and `caldav`).
    Mail,
    GitHub,
    Ai,
    Ntfy,
}

impl Group {
    const ALL: [Group; 5] = [Group::Nextcloud, Group::Mail, Group::GitHub, Group::Ai, Group::Ntfy];

    /// The kinds of account (`AccountInfo::kind`) the group lists.
    fn kinds(self) -> &'static [&'static str] {
        match self {
            Group::Nextcloud => &["nextcloud"],
            Group::Mail => &["mail", "caldav"],
            Group::GitHub => &["github"],
            Group::Ai => &[AI_KIND],
            Group::Ntfy => &[NTFY_KIND],
        }
    }

    /// The provider the group's "Add" opens the dialog at.
    fn provider(self) -> wizard::Provider {
        match self {
            Group::Nextcloud => wizard::Provider::Nextcloud,
            Group::Mail => wizard::Provider::Mail,
            Group::GitHub => wizard::Provider::GitHub,
            Group::Ai => wizard::Provider::Ai,
            Group::Ntfy => wizard::Provider::Ntfy,
        }
    }

    fn title(self) -> String {
        provider_name(self.kinds()[0])
    }

    /// The sentence under the provider's name: what belongs here.
    fn about(self) -> &'static str {
        match self {
            Group::Nextcloud => tr("Calendars and tasks from your Nextcloud."),
            Group::Mail => tr("Mailboxes, and the calendars and tasks of the same provider."),
            Group::GitHub => tr("Issues, pull requests and projects from GitHub."),
            Group::Ai => tr("Models for the assistant and its bots, on this computer or in the cloud."),
            Group::Ntfy => tr("Notifications from your servers and scripts, by topic."),
        }
    }

    /// The words of the group's "Add" button.
    fn add_label(self) -> &'static str {
        match self {
            Group::Nextcloud => tr("Add Nextcloud…"),
            Group::Mail => tr("Add mail account…"),
            Group::GitHub => tr("Add GitHub account…"),
            Group::Ai => tr("Add AI provider…"),
            Group::Ntfy => tr("Add ntfy server…"),
        }
    }
}

/// One group of the list: a fixed [`Group`], or a kind of account none of them holds, as a group
/// of its own (named by [`provider_name`], without "Add").
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Listed<'a> {
    Fixed(Group),
    Other(&'a str),
}

impl<'a> Listed<'a> {
    fn holds(self, kind: &str) -> bool {
        match self {
            Listed::Fixed(group) => group.kinds().contains(&kind),
            Listed::Other(other) => other == kind,
        }
    }

    /// What [`State::provider`] keeps of the group (task S.37): a fixed group's first kind, else
    /// the kind itself.
    fn key(self) -> &'a str {
        match self {
            Listed::Fixed(group) => group.kinds()[0],
            Listed::Other(kind) => kind,
        }
    }

    fn title(self) -> String {
        match self {
            Listed::Fixed(group) => group.title(),
            Listed::Other(kind) => provider_name(kind),
        }
    }

    /// The provider's icon in the column and the chips: the ones the suite uses for these
    /// services elsewhere (Nextcloud a cloud, AI the sparkles, ntfy the bell of notifications).
    fn icon(self) -> Icon {
        match self {
            Listed::Fixed(Group::Nextcloud) => Icon::Cloud,
            Listed::Fixed(Group::Mail) => Icon::Mail,
            Listed::Fixed(Group::GitHub) => Icon::Github,
            Listed::Fixed(Group::Ai) => Icon::Sparkles,
            Listed::Fixed(Group::Ntfy) => Icon::Bell,
            Listed::Other(_) => Icon::Folder,
        }
    }
}

/// The key of the group an account of `kind` stands in ([`Listed::key`]): a fixed group's, else the
/// kind's own.
fn key_of(kind: &str) -> &str {
    Group::ALL.into_iter().find(|group| group.kinds().contains(&kind)).map_or(kind, |group| group.kinds()[0])
}

/// The group the list shows (task S.37): the one `choice` names while it is among `groups`, else
/// the first that holds one of `entries` (accounts and sign-ins in progress), else the first of
/// all. A kind of its own leaves `groups` with its last account, so its choice falls back too.
fn chosen<'a>(choice: Option<&str>, groups: &[Listed<'a>], entries: &[&AccountInfo]) -> Listed<'a> {
    let named = choice.and_then(|key| groups.iter().copied().find(|group| group.key() == key));
    let busy = || groups.iter().copied().find(|group| entries.iter().any(|entry| group.holds(&entry.kind)));
    named.or_else(busy).unwrap_or(Listed::Fixed(Group::Nextcloud))
}

/// How many accounts `group` has: the count beside the provider. A sign-in in progress is no
/// account yet ([`pending`]).
fn accounts_in(group: Listed<'_>, entries: &[&AccountInfo]) -> usize {
    entries.iter().filter(|entry| !entry.id.is_empty() && group.holds(&entry.kind)).count()
}

/// The worst way an entry of `group` stands, sign-ins in progress included (they wait for the
/// person); `None` without any.
fn group_tone(group: Listed<'_>, entries: &[&AccountInfo]) -> Option<Tone> {
    entries.iter().filter(|entry| group.holds(&entry.kind)).map(|entry| status_line(&entry.state).tone).max()
}

/// The provider's icon, with a dot in the colour of a status where one of its accounts needs the
/// person or fails (issue #248): the dot is a shape, so the state shows without colour too, and the
/// icon stays the provider's own, so two providers in need stay apart in the rail. The words stand on
/// the account's row. The text colours (`attention_text`, `error_text`): the status colours are too
/// light for a small dot on light surfaces.
fn provider_mark(group: Listed<'_>, entries: &[&AccountInfo], palette: &Palette) -> Mark {
    match group_tone(group, entries) {
        Some(Tone::Attention) => Mark::Badged(group.icon(), palette.attention_text),
        Some(Tone::Error) => Mark::Badged(group.icon(), palette.error_text),
        Some(Tone::Ok) | None => Mark::Icon(group.icon()),
    }
}

/// The overview's "Open accounts" of the account `id` (task S.37): as the list shows one provider,
/// it shows that account's, and leaves an open account's page for the list. An account that is
/// gone by then changes no choice.
pub fn show_provider_of(state: &mut State, data: &Data, id: &str) {
    if let Some(account) = listed(&data.accounts).find(|account| account.id == id) {
        state.provider = Some(key_of(&account.kind).to_string());
    }
    state.selected = None;
    close_dialogs(state);
    state.models_page = ModelsPage::default();
}

/// The groups of the list: every fixed group in its order, accounts or not, then each kind of
/// `accounts` that no fixed group holds, once, in the order it first appears.
fn providers<'a>(accounts: &[&'a AccountInfo]) -> Vec<Listed<'a>> {
    let mut groups: Vec<Listed<'a>> = Group::ALL.into_iter().map(Listed::Fixed).collect();
    for account in accounts {
        if !groups.iter().any(|group| group.holds(&account.kind)) {
            groups.push(Listed::Other(&account.kind));
        }
    }
    groups
}

/// AI has its own section; the Accounts provider picker contains the other kinds only.
fn account_providers<'a>(accounts: &[&'a AccountInfo]) -> Vec<Listed<'a>> {
    providers(accounts).into_iter().filter(|group| *group != Listed::Fixed(Group::Ai)).collect()
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

/// The account whose detail page is open, if it is still listed.
fn selected<'a>(state: &State, data: &'a Data) -> Option<&'a AccountInfo> {
    let id = state.selected.as_deref()?;
    listed(&data.accounts).find(|account| account.id == id)
}

// ----- Views --------------------------------------------------------------------------------

/// The chosen provider ([`chosen`], task S.37): its name with the sentence of what belongs there
/// above its accounts, then the group "Accounts" with them, the sign-ins in progress and its "Add"
/// ("No account yet." before it while there is none), and under AI the data rule. A kind of
/// account none of the fixed groups holds has no "Add". With `chips` (narrow) the providers stand
/// as chips above it all.
fn list<'a>(
    state: &'a State,
    data: &'a Data,
    now: DateTime<Utc>,
    chips: bool,
    palette: &Palette,
    ai: bool,
    stacked: bool,
) -> Element<'a, Message> {
    let entries = entries(data);
    let groups = account_providers(&entries);
    let shown = if ai { Listed::Fixed(Group::Ai) } else { chosen(state.provider.as_deref(), &groups, &entries) };
    let mut page = column![].spacing(layout::section_gap());
    if chips {
        page = page.push(provider_chips(&groups, shown, &entries, palette));
    }
    page = page.push(provider_heading(shown));
    let mut rows: Vec<Element<'a, Message>> = entries
        .iter()
        .filter(|account| shown.holds(&account.kind))
        .map(|account| match account.id.is_empty() {
            true => pending_row(account),
            false => account_row(account, models_on(state, &account.id), now),
        })
        .collect();
    if let Listed::Fixed(fixed) = shown {
        if rows.is_empty() {
            rows.push(quiet_row(tr("No account yet.")));
        }
        rows.push(add_row(fixed, fixed == Group::Mail && offers_import(state)));
    }
    page = page.push(widgets::group(tr("Accounts"), rows, None));
    if shown == Listed::Fixed(Group::Ai) {
        page = page.push(slots_group(state, data, stacked));
        page = page.push(data_rule_group(data, &state.models));
    }
    page.into()
}

/// The providers as chips above the list in the narrow class (task S.37), the shown one chosen.
fn provider_chips<'a>(
    groups: &[Listed<'_>],
    shown: Listed<'_>,
    entries: &[&AccountInfo],
    palette: &Palette,
) -> Element<'a, Message> {
    let chips = groups.iter().map(|&group| {
        widgets::choice_chip(
            group.title(),
            Some(provider_mark(group, entries, palette)),
            group == shown,
            Some(Message::ShowProvider(group.key().to_string())),
        )
    });
    widgets::chip_row(chips.collect())
}

/// The shown provider's name and, for a fixed group, the sentence of what belongs there – above
/// its accounts, as the name of an account heads its detail page ([`heading`]).
fn provider_heading<'a>(group: Listed<'_>) -> Element<'a, Message> {
    let name = typo::text(typo::headline(), group.title()).wrapping(Wrapping::None);
    let mut heading = column![container(name).width(Fill).clip(true)].spacing(space::xs());
    if let Listed::Fixed(fixed) = group {
        heading = heading.push(typo::text(typo::meta(), fixed.about()).style(theme::text_2));
    }
    heading.into()
}

/// The group Mail offers "Import from TorroMail…": the last list TorroMail gave has an account the
/// step TorroMail can take over ([`wizard::can_import`]).
fn offers_import(state: &State) -> bool {
    state.torromail.as_deref().is_some_and(wizard::can_import)
}

/// The last row of a fixed group: its "Add", and in Mail "Import from TorroMail…" next to it while
/// TorroMail has an account to take over. Ghost buttons on the content axis like "Add topic…": the
/// page has no primary button, and the groups are many.
fn add_row<'a>(group: Group, import: bool) -> Element<'a, Message> {
    let add = widgets::text_button(
        Some(Icon::Plus),
        group.add_label(),
        style::button::ghost,
        Some(Message::Add(group.provider())),
    );
    let import = import.then(|| {
        // The suite has no download icon; the envelope says where the accounts come from.
        widgets::text_button(
            Some(Icon::Mail),
            tr("Import from TorroMail…"),
            style::button::ghost,
            Some(Message::ImportTorroMail),
        )
    });
    let buttons = row![add].push(import).spacing(space::s()).align_y(Alignment::Center);
    row_frame(on_axis(buttons.into()), false)
}

/// The data rule in the AI section (task C.17, here since S.30; the permission per slot since issue
/// #325): "Chat may read mail in the cloud" – off, the chat reads mail only with AI on the person's own
/// computers; on, with models elsewhere too. The sentence under the group says that the switch holds
/// for the assistant and every bot. While *Sorting* may send mail elsewhere (`decide_mail_to_cloud`, set
/// by choosing a model elsewhere for it), a row offers "Keep mail here"; it is never switched on here.
/// The row says "Sorting sends mail to the cloud" and where mail goes only while the model that sorts
/// now computes elsewhere ([`sorting_now`]), else that it may (review of #325: the permission can
/// outlast a model elsewhere, as after the take-over of an old `mailToCloud`) – with "The model that
/// sorts now runs on this computer" only while one here sorts, without a line while nothing sorts.
fn data_rule_group<'a>(data: &Data, models: &BTreeMap<String, Vec<AiModel>>) -> Element<'a, Message> {
    let settings = &data.settings;
    let switch =
        toggler(settings.mail_to_cloud).size(style::CHECK_SIZE).style(style::toggler).on_toggle(Message::MailToCloud);
    let detail = match settings.mail_to_cloud {
        false => tr("Off: the chat reads mail only with AI on your own computers"),
        true => tr("Mail goes to the model's provider and can't be taken back"),
    };
    let mut rows = vec![setting_row(None, tr("Chat may read mail in the cloud"), Some(detail.to_string()), switch)];
    if settings.mail_to_cloud_in(AiSlot::Decide) {
        let keep =
            widgets::text_button(None, tr("Keep mail here"), style::button::secondary, Some(Message::StopSortingMail));
        let may = tr("Sorting may send mail to the cloud");
        let row = match sorting_now(data, models) {
            SortingNow::Elsewhere(provider, seat) => {
                setting_row(None, tr("Sorting sends mail to the cloud"), Some(mail_sentence(provider, seat)), keep)
            }
            SortingNow::Here => {
                let here = tr("The model that sorts now runs on this computer").to_string();
                setting_row(None, may, Some(here), keep)
            }
            SortingNow::Nothing => setting_row(None, may, None, keep),
        };
        rows.push(row);
    }
    let footer = tr("The switch holds for the assistant and all its bots, whatever their rights allow.");
    widgets::group(tr("Data rule"), rows, Some(footer.to_string()))
}

/// What sorts mail now, as the data rule says it (review of #325).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SortingNow<'a> {
    /// No model sorts: the slot Sorting is empty, or the exception is switched off.
    Nothing,
    /// The model that sorts computes on this computer.
    Here,
    /// The model that sorts computes elsewhere: its provider ("OpenRouter"; the account's id for a
    /// provider Torro does not know) and where that runs ("Cloud · US").
    Elsewhere(&'a str, &'static str),
}

/// What sorts now, weighed as the core does (`assignment_in_effect`): the exception of
/// `inbox.categorize` while one is stored (switched off, nothing sorts), else the slot Sorting; where
/// its model computes by [`sorts_elsewhere`].
fn sorting_now<'a>(data: &'a Data, models: &BTreeMap<String, Vec<AiModel>>) -> SortingNow<'a> {
    let settings = &data.settings;
    let (engine, model) = match settings.ai_assignments.get(&AiFunction::InboxCategorize) {
        Some(own) if own.engine.is_some() => match own.engine.as_deref().filter(|_| own.enabled) {
            Some(engine) => (engine, own.model.as_deref()),
            None => return SortingNow::Nothing,
        },
        _ => match settings.ai_slots.get(AiSlot::Decide) {
            Some(slot) => (slot.engine.as_str(), slot.model.as_deref()),
            None => return SortingNow::Nothing,
        },
    };
    if !sorts_elsewhere(data, models, engine, model) {
        return SortingNow::Here;
    }
    let provider = ai_provider(engine);
    let name = provider.map_or(engine, |provider| provider.name());
    let seat = provider.map_or(tr("Cloud"), |provider| seat_of(provider, false));
    SortingNow::Elsewhere(name, seat)
}

/// Where *Sorting* sends mail while it may (`decide_mail_to_cloud`): the provider and the seat of the
/// model that sorts now ([`SortingNow::Elsewhere`]). `None` while mail stays here, nothing sorts (no
/// model, the exception switched off) or that model computes on this computer.
fn sorting_place<'a>(data: &'a Data, models: &BTreeMap<String, Vec<AiModel>>) -> Option<(&'a str, &'static str)> {
    if !data.settings.mail_to_cloud_in(AiSlot::Decide) {
        return None;
    }
    match sorting_now(data, models) {
        SortingNow::Elsewhere(name, seat) => Some((name, seat)),
        SortingNow::Here | SortingNow::Nothing => None,
    }
}

/// Whether mail sorted with `model` of the AI account `engine` leaves this computer, as the core weighs
/// it (`elsewhere_of`): a model the account lists by where it computes ([`place_of`]); one not listed
/// yet at an account on this computer only by a cloud name (`:cloud`), elsewhere as the rule assumes in
/// doubt; naming no model ("Automatic") by the account's seat – at an account here it picks only models
/// here (review of #325). An unknown provider computes elsewhere.
fn sorts_elsewhere(data: &Data, models: &BTreeMap<String, Vec<AiModel>>, engine: &str, model: Option<&str>) -> bool {
    let Some(provider) = ai_provider(engine) else { return true };
    let Some(id) = model else { return provider.seat() != Seat::ThisComputer };
    let server = data.accounts.iter().find(|account| account.id == engine).map_or("", account_server);
    let listed = models.get(engine).and_then(|models| models.iter().find(|known| same_model(&known.id, id)));
    match listed {
        Some(known) => place_of(provider, server, known) != Place::Local,
        None if provider.seat() == Seat::ThisComputer => torro_core::ai::openai::ollama_cloud_name(id),
        None => place_of(provider, server, &AiModel::named(id)) != Place::Local,
    }
}

/// The base address of an AI account as its state names it; empty while it is not connected.
fn account_server(account: &AccountInfo) -> &str {
    match &account.state {
        AccountState::Connected { server, .. } | AccountState::SigningIn { server, .. } => server.as_str(),
        AccountState::NotConnected => "",
    }
}

/// "Mail goes to OpenRouter (Cloud · US) for sorting only.": the one sentence for where *Sorting* sends
/// mail, under "Models" and in the data rule.
fn mail_sentence(provider: &str, seat: &str) -> String {
    trf("Mail goes to {provider} ({seat}) for sorting only.", &[("provider", &provider), ("seat", &seat)])
}

/// The group "Models" of the AI section (issue #325, task MW.8): the computer as measured, then per
/// slot its row with the list of models, why the last step failed, where *Sorting* sends mail, and
/// what Torro suggests. Not shown without an AI account: the section's "Add" comes first. `stacked`
/// (narrow): every row puts its control under its text ([`models_row`]).
fn slots_group<'a>(state: &'a State, data: &'a Data, stacked: bool) -> Option<Element<'a, Message>> {
    ai_accounts(data).next()?;
    let mut rows = vec![computer_row(&state.slots, stacked)];
    for slot in AiSlot::ALL {
        rows.push(slot_row(state, data, slot, stacked));
        if let Some(failure) = state.slots.failed.as_ref().filter(|failure| failure.slot == slot) {
            rows.push(failure_row(failure, stacked));
        }
        if slot == AiSlot::Decide
            && let Some((provider, seat)) = sorting_place(data, &state.models)
        {
            rows.push(mail_note(provider, seat, data.settings.mail_to_cloud));
        }
        if let Some(suggestion) = suggestion_of(state, slot).filter(|suggestion| {
            !holds(data, slot, suggestion.engine.as_deref(), suggestion.model.as_deref()) || suggestion.engine.is_none()
        }) {
            rows.push(suggestion_row(state, suggestion, stacked));
            if slot == AiSlot::Decide && !suggestion.local {
                rows.push(mail_note(suggestion.provider.name(), &suggestion.seat, data.settings.mail_to_cloud));
            }
        }
    }
    let footer = tr("Torro suggests a model for each slot. Nothing changes until you choose one.");
    Some(widgets::group(tr("Models"), rows, Some(footer.to_string())))
}

/// A row of "Models": `mark` in front (or nothing), `label` over `detail`, and `control`. Beside each
/// other with the metrics of [`setting_row`], each line cut with "…" and a tooltip; `stacked` (narrow)
/// the control stands under the text, on its axis, and the detail wraps – a model's name, a download's
/// progress or a reason do not fit beside a button there, and must not be cut. `alert` puts a
/// `CircleAlert` in the attention colour before the detail: something about the row needs a look.
fn models_row<'a>(
    mark: Option<Element<'a, Message>>,
    label: impl IntoFragment<'a>,
    detail: Option<String>,
    alert: bool,
    control: Element<'a, Message>,
    stacked: bool,
) -> Element<'a, Message> {
    if !stacked && !alert {
        return setting_row(mark, label, detail, control);
    }
    let detail = detail.map(|detail| {
        let line: Element<'a, Message> = match stacked {
            true => typo::text(typo::meta(), detail).style(theme::text_2).width(Fill).into(),
            false => widgets::ellipsized(typo::meta(), detail, theme::text_2),
        };
        match alert {
            true => row![attention_mark(icon::SIZE_SM), line].spacing(space::xs()).align_y(Alignment::Center).into(),
            false => line,
        }
    });
    let tall = detail.is_some();
    let lines = column![widgets::ellipsized(typo::body(), label, theme::text)].push(detail);
    if !stacked {
        let text = row![].push(mark).push(lines.width(Fill)).spacing(space::m()).align_y(Alignment::Center);
        return row_frame(row![text.width(Fill), control].spacing(space::l()).align_y(Alignment::Center), tall);
    }
    // The mark stays beside the label's line, not the middle of text and control.
    let first_line = typo::body().line_height.to_absolute(typo::body().size).0;
    let mark = mark.map(|mark| container(mark).center_y(first_line));
    let text = column![lines, control].spacing(space::s()).width(Fill);
    container(row![].push(mark).push(text).spacing(space::m())).width(Fill).padding([space::m(), space::l()]).into()
}

/// `CircleAlert` at `size` in the attention colour.
fn attention_mark<'a>(size: f32) -> Element<'a, Message> {
    icon::icon(Icon::CircleAlert, size)
        .style(|t: &Theme, _| svg::Style { color: Some(palette(t).attention_text) })
        .into()
}

/// Where a model of `provider` runs, for a sentence about mail: the provider's seat ("Cloud · US"),
/// and "Cloud" for a model elsewhere of a provider whose seat is this computer (an Ollama cloud model).
fn seat_of(provider: AiProvider, local: bool) -> &'static str {
    match provider.seat() {
        Seat::ThisComputer if !local => tr("Cloud"),
        seat => seat.label(),
    }
}

/// The first row of "Models": the computer as `ai.recommend` measured it ("GeForce RTX 4070 · 12 GB
/// video memory · 64 GB memory") with the class of models that fits it at the right ("Graphics card
/// class"; not in the narrow class, where the line wraps instead); while the first answer is on its way
/// "Measuring this computer…" (it can take a while), after a failure the reason with "Retry".
fn computer_row<'a>(slots: &'a SlotsPage, stacked: bool) -> Element<'a, Message> {
    let mark =
        icon::icon(Icon::Monitor, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) });
    if let Some(problem) = slots.recommend_error.as_ref().filter(|_| !slots.recommending) {
        let retry = widgets::text_button(None, tr("Retry"), style::button::secondary, Some(Message::Recommend));
        let (label, detail) = (tr("No suggestion right now"), Some(problem.clone()));
        return models_row(Some(mark.into()), label, detail, false, retry, stacked);
    }
    match &slots.recommendation {
        Some(recommendation) => {
            let tier: Element<'a, Message> = match stacked {
                true => Space::new().into(),
                false => typo::text(typo::meta(), trf("{tier} class", &[("tier", &recommendation.tier_label)]))
                    .style(theme::text_3)
                    .into(),
            };
            let line = hardware_line(&recommendation.hardware);
            models_row(Some(mark.into()), tr("This computer"), Some(line), false, tier, stacked)
        }
        None => {
            let detail = tr("Measuring this computer…").to_string();
            models_row(Some(mark.into()), tr("This computer"), Some(detail), false, Space::new().into(), stacked)
        }
    }
}

/// The computer in one line: the graphics card (else the processor), its video memory, the memory –
/// "shared memory" where processor and graphics share it (Apple silicon). What was not measured is
/// left out; with nothing measured "Not measured".
pub fn hardware_line(hardware: &Hardware) -> String {
    let gpu = hardware.gpu.as_ref();
    let name = gpu.and_then(|gpu| gpu.name.clone()).or_else(|| hardware.cpu.clone());
    let vram = gpu
        .and_then(|gpu| gpu.vram_bytes)
        .map(|bytes| trf("{size} video memory", &[("size", &torro_gui::units::format_bytes(bytes))]));
    let memory = hardware.memory_bytes.map(|bytes| {
        let size = torro_gui::units::format_bytes(bytes);
        match hardware.unified_memory {
            true => trf("{size} shared memory", &[("size", &size)]),
            false => trf("{size} memory", &[("size", &size)]),
        }
    });
    let parts: Vec<String> = [name, vram, memory].into_iter().flatten().collect();
    if parts.is_empty() { tr("Not measured").to_string() } else { parts.join(" · ") }
}

/// What a slot is for, under its name.
fn slot_about(slot: AiSlot) -> &'static str {
    match slot {
        AiSlot::Chat => tr("The assistant and its bots"),
        AiSlot::Decide => tr("Sorts mail into categories"),
    }
}

/// What a sorting model through the fallback adds to its line ([`AiModel::decides_by_chat`], task
/// MW.3), in the words of Settings › Mail › Categorization.
fn by_chat_note() -> &'static str {
    tr("Answers in a fixed format, slower")
}

/// The line under a slot's name: "Saving…" while a choice is on its way; "Turned off in Settings › AI"
/// (with the alert, `true`) while the model it holds is not among those turned on ([`held_model`]);
/// else what the slot is for, for *Sorting* with a chat model "· Answers in a fixed format, slower".
fn slot_detail(state: &State, data: &Data, slot: AiSlot) -> (String, bool) {
    let busy = state.slots.using.as_ref().is_some_and(|using| using.slot == slot && !using.step.downloading());
    if busy {
        return (tr("Saving…").to_string(), false);
    }
    match held_model(state, data, slot) {
        Held::Off => (tr("Turned off in Settings › AI").to_string(), true),
        Held::On(model) if slot == AiSlot::Decide && model.decides_by_chat() => {
            (format!("{} · {}", slot_about(slot), by_chat_note()), false)
        }
        _ => (slot_about(slot).to_string(), false),
    }
}

/// What a slot holds, as far as its account's list says.
enum Held<'a> {
    /// Nothing, the account picks, or its models are not in yet.
    Unknown,
    /// A model the account lists turned on.
    On(&'a AiModel),
    /// A model the account lists turned off, or lacks: the slot does nothing with it.
    Off,
}

/// What `slot` holds ([`Held`]), the model's id matched as Ollama lists it ([`same_model`]).
fn held_model<'a>(state: &'a State, data: &Data, slot: AiSlot) -> Held<'a> {
    let Some(stored) = data.settings.ai_slots.get(slot) else { return Held::Unknown };
    let (Some(id), Some(models)) = (&stored.model, state.models.get(&stored.engine)) else { return Held::Unknown };
    match models.iter().find(|model| same_model(&model.id, id)) {
        Some(model) if model.enabled => Held::On(model),
        _ => Held::Off,
    }
}

/// A slot's row: its name, the line under it ([`slot_detail`]) and the list of models
/// ([`slot_choices`]); `stacked` (narrow) the list under the text, as wide as the row, so both slots'
/// lists line up.
fn slot_row<'a>(state: &'a State, data: &'a Data, slot: AiSlot, stacked: bool) -> Element<'a, Message> {
    let (choices, chosen) = slot_choices(state, data, slot);
    let (detail, alert) = slot_detail(state, data, slot);
    let on_pick = move |choice: SlotChoice| Message::PickSlot(slot, choice);
    let pick = match stacked {
        true => picker_fill(choices, chosen, on_pick),
        false => picker(choices, chosen, on_pick),
    };
    models_row(None, slot.label(), Some(detail), alert, pick, stacked)
}

/// The list of a slot and what it has (task MW.8): "None", then the models turned on of every AI
/// account that serve the slot – *Chat* those that can use tools (or do not say), *Sorting* those that
/// decide or can use tools ([`AiModel::can_decide`]; how one sorts stands under the slot's name, so the
/// list stays narrow) –, by account and name, the account's name only with more than one AI account.
/// A chosen model the lists do not hold (turned off, not listed yet) stands right after "None", by its
/// name where its account lists it, so the list shows it.
pub fn slot_choices(state: &State, data: &Data, slot: AiSlot) -> (Vec<SlotChoice>, SlotChoice) {
    let accounts: Vec<&AccountInfo> = ai_accounts(data).collect();
    let several = accounts.len() > 1;
    let label = |account: Option<&AccountInfo>, model: Option<&AiModel>, id: &str| {
        let mut parts = vec![model.map_or_else(|| id.to_string(), |model| model.name.clone())];
        if several && let Some(account) = account {
            parts.push(account.label.clone());
        }
        parts.join(" · ")
    };
    let none = SlotChoice { engine: None, model: None, label: tr("None").to_string() };
    let mut choices = vec![none.clone()];
    for account in &accounts {
        let mut models: Vec<&AiModel> =
            state.models.get(&account.id).into_iter().flatten().filter(|model| serves(model, slot)).collect();
        models.sort_by_cached_key(|model| (model.name.to_lowercase(), model.id.clone()));
        choices.extend(models.into_iter().map(|model| SlotChoice {
            engine: Some(account.id.clone()),
            model: Some(model.id.clone()),
            label: label(Some(account), Some(model), &model.id),
        }));
    }
    let Some(stored) = data.settings.ai_slots.get(slot) else { return (choices, none) };
    let found = choices.iter().find(|choice| {
        choice.engine.as_deref() == Some(stored.engine.as_str())
            && match (&choice.model, &stored.model) {
                (Some(a), Some(b)) => same_model(a, b),
                (a, b) => a == b,
            }
    });
    if let Some(found) = found {
        return (choices.clone(), found.clone());
    }
    let account = accounts.iter().copied().find(|account| account.id == stored.engine);
    let known = stored
        .model
        .as_ref()
        .and_then(|id| state.models.get(&stored.engine)?.iter().find(|model| same_model(&model.id, id)));
    let name = match &stored.model {
        Some(id) => label(account, known, id),
        // The account picks: its name says which.
        None => account.map_or_else(|| stored.engine.clone(), |account| account.label.clone()),
    };
    let chosen = SlotChoice { engine: Some(stored.engine.clone()), model: stored.model.clone(), label: name };
    choices.insert(1, chosen.clone());
    (choices, chosen)
}

/// A model turned on that can take `slot`.
fn serves(model: &AiModel, slot: AiSlot) -> bool {
    model.enabled
        && match slot {
            AiSlot::Chat => model.tools != Some(false),
            AiSlot::Decide => model.can_decide(),
        }
}

/// Why the last step under a slot failed, in the core's words, with "Retry".
fn failure_row<'a>(failure: &'a SlotFailure, stacked: bool) -> Element<'a, Message> {
    let alert = icon::icon(Icon::CircleAlert, icon::SIZE)
        .style(|t: &Theme, _| svg::Style { color: Some(palette(t).error_text) });
    let retry = widgets::text_button(None, tr("Retry"), style::button::secondary, Some(Message::RetrySlot));
    match stacked {
        // The core's sentence is long: it wraps above "Retry" instead of being cut.
        true => models_row(Some(alert.into()), failure.message.as_str(), None, false, retry, true),
        false => setting_row(Some(alert.into()), failure.message.as_str(), None, retry),
    }
}

/// Where *Sorting* sends mail, said where the choice is made: "Mail goes to OpenRouter (Cloud · US) for
/// sorting only." and, while the chat may read none, "The chat still gets no mail." As high as the rows
/// around it ([`row_frame`]); wraps rather than cutting off: it is the one sentence here that must be
/// read whole.
fn mail_note<'a>(provider: &str, seat: &str, chat_reads_mail: bool) -> Element<'a, Message> {
    let mut sentence = mail_sentence(provider, seat);
    if !chat_reads_mail {
        sentence = format!("{sentence} {}", tr("The chat still gets no mail."));
    }
    let mark = icon::icon(Icon::Cloud, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) });
    let line = typo::text(typo::meta(), sentence).style(theme::text_2).width(Fill);
    row_frame(row![mark, line].spacing(space::m()).align_y(Alignment::Center), false)
}

/// What Torro suggests for a slot: the model (or, where the account picks one, the provider), where it
/// runs and why, and "Use suggestion" – "Use suggestion · 5.2 GB" where it downloads first. While its
/// download runs the progress with "Cancel"; while the chain saves "Saving…"; while the chain of the
/// other slot runs the button is off and its tooltip says what it waits for; without an account to run
/// it (no Ollama) the reason stands there instead of the button. No mark: the suggestion comes from
/// Torro's fixed list, not from a model, so it has no Sparkles – only their room, so it lines up with
/// the rows around it. `stacked` (narrow): the control under the text ([`models_row`]).
fn suggestion_row<'a>(state: &'a State, suggestion: &'a SlotSuggestion, stacked: bool) -> Element<'a, Message> {
    let slot = suggestion.slot;
    let name = match (&suggestion.engine, &suggestion.model) {
        (Some(engine), Some(id)) => state
            .models
            .get(engine)
            .and_then(|models| models.iter().find(|model| same_model(&model.id, id)))
            .map_or_else(|| id.clone(), |model| model.name.clone()),
        (None, Some(id)) => id.clone(),
        (_, None) => suggestion.provider.name().to_string(),
    };
    let label = trf("Suggested: {model}", &[("model", &name)]);
    let mut detail = vec![suggestion.seat.clone()];
    if suggestion.engine.is_some() {
        detail.push(suggestion.reason.clone());
    }
    if suggestion.by_chat {
        detail.push(by_chat_note().to_string());
    }
    let room = || Some(Space::new().width(icon::SIZE).into());
    let Some(engine) = &suggestion.engine else {
        let reason = typo::text(typo::meta(), suggestion.reason.as_str()).style(theme::text_2);
        return models_row(room(), label, Some(detail.join(" · ")), false, reason.into(), stacked);
    };
    let model = suggestion.model.as_deref().unwrap_or_default();
    let pull = pull_of(&state.slots.pulls, engine, model).filter(|pull| pull.state == AiPullState::Running);
    let using = state.slots.using.as_ref();
    let control: Element<'a, Message> = match (pull, using.filter(|using| using.slot == slot)) {
        (Some(pull), _) if suggestion.download_bytes.is_some() => {
            detail = vec![download_text(pull)];
            download_control(slot, pull)
        }
        (_, Some(using)) if !using.step.downloading() => widgets::busy_button(tr("Saving…"), style::button::secondary),
        (_, Some(_)) => widgets::busy_button(tr("Starting download…"), style::button::secondary),
        _ => {
            let words = match suggestion.download_bytes {
                Some(bytes) => trf("Use suggestion · {size}", &[("size", &torro_gui::units::format_bytes(bytes))]),
                None => tr("Use suggestion").to_string(),
            };
            let free = using.is_none().then_some(Message::UseSuggestion(slot));
            let button = widgets::text_button(None, words, style::button::secondary, free);
            match using {
                Some(other) => widgets::with_tooltip(button, waits_for(other)),
                None => button,
            }
        }
    };
    models_row(room(), label, Some(detail.join(" · ")), false, control, stacked)
}

/// Why "Use suggestion" is off while the chain of another slot runs: "Wait until the download for
/// Chat has finished", or "Wait until Chat is saved" once the download is done.
fn waits_for(other: &Using) -> String {
    let slot = other.slot.label();
    match other.step.downloading() {
        true => trf("Wait until the download for {slot} has finished", &[("slot", &slot)]),
        false => trf("Wait until {slot} is saved", &[("slot", &slot)]),
    }
}

/// How far a download is: "Downloading 3.9 GB of 9.3 GB", or "Starting download…" before Ollama
/// names a size.
fn download_text(pull: &AiPull) -> String {
    match (pull.completed, pull.total) {
        (Some(done), Some(total)) => trf(
            "Downloading {done} of {total}",
            &[("done", &torro_gui::units::format_bytes(done)), ("total", &torro_gui::units::format_bytes(total))],
        ),
        _ => tr("Starting download…").to_string(),
    }
}

/// The share of a download in per mille and as "41%"; `None` before Ollama names a size. The total
/// can grow while layers are added, so the share never passes 100 %.
pub fn download_share(pull: &AiPull) -> Option<(usize, String)> {
    let (done, total) = (pull.completed?, pull.total.filter(|total| *total > 0)?);
    let per_mille = (done.min(total) * 1000 / total) as usize;
    Some((per_mille, format!("{}%", per_mille / 10)))
}

/// The progress of a running download with "Cancel": the bar and the share, tabular so it does not
/// jump; before Ollama names a size only "Cancel".
fn download_control<'a>(slot: AiSlot, pull: &AiPull) -> Element<'a, Message> {
    let cancel = widgets::text_button(
        None,
        tr("Cancel"),
        style::button::ghost,
        Some(Message::CancelPull(slot, pull.id.clone())),
    );
    let mut parts = row![].spacing(space::s()).align_y(Alignment::Center);
    if let Some((per_mille, share)) = download_share(pull) {
        parts = parts.push(widgets::progress_bar(per_mille, 1000));
        parts = parts.push(typo::text(typo::meta().tabular(), share).style(theme::text_3));
    }
    parts.push(cancel).into()
}

/// A sign-in in progress: attention dot, the server and "Signing in…". Not pressable: there is
/// no account yet.
fn pending_row<'a>(account: &'a AccountInfo) -> Element<'a, Message> {
    let line = status_line(&account.state);
    let dot = widgets::status_dot(line.tone.status());
    setting_row(Some(dot), pending_name(account), Some(line.text), Space::new())
}

/// One account in the list: status dot, label, status and last sync, and a chevron; the whole row
/// opens the detail page. `models_on`: how many models of an AI account are on, if known
/// ([`models_on`]).
fn account_row<'a>(account: &'a AccountInfo, models_on: Option<usize>, now: DateTime<Utc>) -> Element<'a, Message> {
    let dot = widgets::status_dot(status_line(&account.state).tone.status());
    let row = setting_row(Some(dot), account.label.as_str(), Some(row_detail(account, models_on, now)), chevron());
    pressable(row, Some(Message::Open(account.id.clone())))
}

/// How many models of the account `id` are on, as the last list of its models says (task S.31);
/// `None` while the page has never listed them.
fn models_on(state: &State, id: &str) -> Option<usize> {
    state.models.get(id).map(|models| models.iter().filter(|model| model.enabled).count())
}

/// "1 model on", "3 models on".
fn models_on_text(n: usize) -> String {
    match n {
        1 => tr("1 model on").to_string(),
        n => trf("{n} models on", &[("n", &n)]),
    }
}

/// The line under an account's name in the list. An AI account never syncs: "Last synced" would
/// say nothing true about it; it says how many of its models are on once its page listed them
/// (`models_on`, task S.31). An ntfy account listens instead, so its topics are what matters. In
/// the group Mail a CalDAV account says what it is, as its page does (task S.30).
fn row_detail(account: &AccountInfo, models_on: Option<usize>, now: DateTime<Utc>) -> String {
    let line = status_line(&account.state);
    match (&account.ntfy, &account.state) {
        (Some(view), AccountState::Connected { problem: None, .. }) => {
            format!("{} · {}", line.text, topics_text(view.topics.len()))
        }
        _ if account.kind == AI_KIND => match models_on {
            Some(n) => format!("{} · {}", line.text, models_on_text(n)),
            None => line.text,
        },
        _ if account.ntfy.is_some() => line.text,
        _ if account.kind == "caldav" => {
            format!("{} · {}", provider_name("caldav"), account_detail(&account.state, now))
        }
        _ => account_detail(&account.state, now),
    }
}

/// The name of an account and its provider, at the top of its detail page.
fn heading(account: &AccountInfo) -> Element<'_, Message> {
    let name = typo::text(typo::headline(), account.label.as_str()).wrapping(Wrapping::None);
    column![
        container(name).width(Fill).clip(true),
        typo::text(typo::meta(), provider_name(&account.kind)).style(theme::text_2),
    ]
    .spacing(space::xs())
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
        return mail_detail(account, mail, now);
    }
    if let Some(view) = &account.ntfy {
        return ntfy_detail(account, view, data, now);
    }
    // A GitHub account has no calendars or lists (yet); what its token may do matters instead. An
    // AI account has its models (task S.31).
    let middle = match account.kind.as_str() {
        "github" => Some(permissions_group(account.scopes.as_deref())),
        AI_KIND => Some(models_group(state, account)),
        _ => Some(collections_moved()),
    };
    column![heading(account), status_group(account, now)]
        .push(middle)
        .push(sources_moved(account))
        .push(connection_group(account))
        .push(remove_button())
        .spacing(layout::section_gap())
        .into()
}

/// A group that says where a setting of this account lives now and opens the window it lives in
/// (task S.21): the sentence under the rows, one row per window with its "Open" button, which
/// sends `apps.open_app` with the window class – a running window comes to the front.
///
/// The control centre cannot open the settings of a running window from here: only a person opens
/// settings (`docs/settings.md` › Technik), so there is no such target for `apps.open_app`.
fn moved_group<'a>(
    title: &'a str,
    rows: Vec<(&'a str, &'static str, &'static str)>,
    footer: &'a str,
) -> Element<'a, Message> {
    let rows = rows
        .into_iter()
        .map(|(name, button, window_class)| {
            let open =
                widgets::text_button(None, button, style::button::secondary, Some(Message::OpenApp(window_class)));
            setting_row(None, name, None, open)
        })
        .collect();
    widgets::group(title, rows, Some(footer.to_string()))
}

/// In place of "Calendars and lists": which of them the suite shows is chosen in Calendar and in
/// Tasks now, behind the gear.
fn collections_moved<'a>() -> Element<'a, Message> {
    moved_group(
        tr("Calendars and lists"),
        vec![
            (tr("Calendars"), tr("Open Calendar"), AppId::CALENDAR.window_class),
            (tr("Task lists"), tr("Open Tasks"), AppId::TASKS.window_class),
        ],
        tr("Calendars and lists are chosen in Calendar and Tasks, under the gear."),
    )
}

/// In place of the switches of a GitHub or Nextcloud account's notifications: they are switched on
/// in the Notification Hub now. `None` for an account that brings none (task E5.14 listed the
/// sources by kind; here the two kinds that had switches).
fn sources_moved<'a>(account: &AccountInfo) -> Option<Element<'a, Message>> {
    let source = account.id.parse::<SourceRef>().ok()?;
    if !matches!(source.kind, SourceKind::Github | SourceKind::Nextcloud) {
        return None;
    }
    Some(moved_group(
        tr("Notification Hub"),
        vec![(tr("Notifications"), tr("Open Notification Hub"), AppId::INBOX.window_class)],
        tr("What this account sends to the Notification Hub is switched on there, under the gear."),
    ))
}

/// In place of a mail account's sender name, signature, writing format and folder questions: they
/// are set in the mail window now, and whether new mail shows up in the Notification Hub there.
fn mail_moved<'a>() -> Vec<Element<'a, Message>> {
    vec![
        moved_group(
            tr("Mail"),
            vec![(tr("Sender name and signature"), tr("Open Mail"), AppId::INBOX_MAIL.window_class)],
            tr(
                "Sender name, signature, how new mail is written and which folder holds what are set in Mail, under the gear.",
            ),
        ),
        moved_group(
            tr("Notification Hub"),
            vec![(tr("New mail"), tr("Open Notification Hub"), AppId::INBOX.window_class)],
            tr("Whether new mail of this account shows up there is set in the Notification Hub, under the gear."),
        ),
    ]
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

/// A mail account's page: the state of each service with "Sync now", where its mail settings and
/// its place in the Notification Hub are now ([`mail_moved`]), the services and the connection, and
/// the removal at the end.
fn mail_detail<'a>(account: &'a AccountInfo, mail: &'a MailView, now: DateTime<Utc>) -> Element<'a, Message> {
    column![heading(account), mail_status_group(account, mail, now)]
        .extend(mail_moved())
        .push(services_group(mail))
        .push(mail_connection_group(account, mail))
        .push(trusted_certificates_group(mail))
        .push(remove_button())
        .spacing(layout::section_gap())
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
    let lines = column![typo::text(typo::body(), name), typo::text(typo::meta(), detail).style(theme::text_2)];
    container(row![].push(mark).push(lines.width(Fill)).push(control).spacing(space::m()).align_y(Alignment::Center))
        .width(Fill)
        .padding([space::m(), space::l()])
        .into()
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
    let user = typo::text(typo::body(), login_name(account, mail)).style(theme::text_2).wrapping(Wrapping::None);
    let rows = vec![
        wrapped_row(None, tr("Mail servers"), servers, Some(settings)),
        setting_row(None, tr("User"), None, user),
        setting_row(None, tr("Password"), None, password),
    ];
    let footer = tr("The password stays in the system keychain. Agents never see it.");
    widgets::group(tr("Connection"), rows, Some(footer.to_string()))
}

/// The certificates a person trusted for the account (task M11.4), each fingerprint whole in the
/// two lines of the question ([`wizard::fingerprint_row`]); `None` while there are none. They
/// change only by signing in again, which keeps the ones the servers still show, so the footer
/// says where.
fn trusted_certificates_group(mail: &MailView) -> Option<Element<'_, Message>> {
    if mail.trusted_certificates.is_empty() {
        return None;
    }
    let rows = mail.trusted_certificates.iter().map(|sha256| wizard::fingerprint_row(sha256)).collect();
    let footer = tr("Signing in again under Server settings keeps only the certificates the servers still show.");
    Some(widgets::group(tr("Trusted certificates"), rows, Some(footer.to_string())))
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
    if let Some(asked) = form.certificates.shown() {
        return certificate_dialog(asked, account, form.sending, Message::SubmitServers, Message::CloseServers);
    }
    let submit = form.can_send().then_some(Message::SubmitServers);
    let head = column![
        typo::text(typo::title(), tr("Server settings")),
        typo::text(typo::body(), servers_purpose(form, account)).style(theme::text_2),
    ]
    .spacing(space::xs());
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
    let mut body = column![].spacing(layout::section_gap());
    body = match form.adding {
        Some(_) => body.push(caldav).extend(servers),
        None => body.extend(servers).push(caldav),
    };
    body = body.push(widgets::group(tr("Password"), vec![row_frame(password, false)], None));
    let error: Element<'_, Message> = match &form.error {
        Some(error) => typo::text(typo::body(), error.as_str())
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
    .spacing(space::s())
    .align_y(Alignment::Center);
    dialog(dialog_frame(head, body, foot, space::xl()), size::form_max(), Message::CloseServers)
}

/// The question about a certificate (task M11.4) in place of the form of "Server settings" or
/// "Change password": the title with the name of `account` under it (as "Server settings" has its
/// purpose there), the question of the add-account dialog with the fingerprint above the foot, and
/// "Back", "Cancel" and "Trust certificate" (`trust`, which sends again,
/// [`wizard::trust_button`]). While that request is out (`sending`) the question stays, "Back" is
/// off and the last button says "Signing in…". As wide as a form, also while sending: the warning
/// and three buttons need the room in any language. `close` also goes with a click next to it.
fn certificate_dialog<'a>(
    asked: &wizard::Untrusted,
    account: &'a AccountInfo,
    sending: bool,
    trust: Message,
    close: Message,
) -> Element<'a, Message> {
    let head = column![
        typo::text(typo::title(), wizard::certificate_title()),
        typo::text(typo::body(), account.label.as_str()).style(theme::text_2),
    ]
    .spacing(space::xs());
    let back = (!sending).then_some(Message::CertificateBack);
    let foot = row![
        blank::horizontal(),
        widgets::text_button(None, tr("Back"), style::button::secondary, back),
        widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(close.clone())),
        wizard::trust_button(asked, sending, Some(trust)),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    dialog(dialog_frame(head, wizard::certificate_question(asked), foot, space::xl()), size::form_max(), close)
}

/// A text button moved left by its own padding, so its label stands on the page's content axis
/// (the name, the groups) instead of 10 to the right of it; the hover surface reaches into the margin.
/// The approvals page uses it for "Show details" (task M3.5).
pub(super) fn on_axis<'a, M: 'a>(button: Element<'a, M>) -> Element<'a, M> {
    row![Space::new(), button].spacing(-style::button_padding().left).into()
}

/// The kind of an AI account (task KI.1a), `torro_core::account::AccountKind::Ai` as listed.
const AI_KIND: &str = "ai";

/// The kind of an ntfy account (task E5.12), `torro_core::account::AccountKind::Ntfy` as listed.
const NTFY_KIND: &str = "ntfy";

// ----- An AI account's models (task S.31) ----------------------------------------------------

/// The provider of an AI account, from its id `ai:<provider>@<host>` (`torro_core::Account::ai`):
/// the account list does not carry it otherwise. `None` for any other id or an unknown provider.
pub fn ai_provider(id: &str) -> Option<AiProvider> {
    let (provider, _) = id.strip_prefix("ai:")?.split_once('@')?;
    AiProvider::ALL.into_iter().find(|known| known.id() == provider)
}

/// Where `model` of an AI account of `provider` at the base address `server` computes, in the words
/// of its row (`docs/settings.md` › KI): "On this computer" where the data rule counts it as local
/// ([`crate::assistant::place_of`], so the page says what the rule does), else "Ollama Cloud" for
/// Ollama's cloud models, "Cloud (Anthropic)" for Anthropic and "Cloud" for everything else. An
/// unknown provider computes elsewhere, as the rule assumes in doubt.
pub fn model_place(provider: Option<AiProvider>, server: &str, model: &AiModel) -> &'static str {
    let Some(provider) = provider else { return tr("Cloud") };
    match (place_of(provider, server, model), provider) {
        (Place::Local, _) => tr("On this computer"),
        (Place::Remote, AiProvider::Ollama) => tr("Ollama Cloud"),
        (Place::Remote, AiProvider::Anthropic) => tr("Cloud (Anthropic)"),
        (Place::Remote, _) => tr("Cloud"),
    }
}

/// A model has a switch when a module can use it ([`AiModel::is_usable`], #231): it can use tools
/// (or does not say), or it decides, as sorting mail needs. One that can do neither stands faded
/// with "Can't use tools or decide", because no module of the suite can use it.
pub fn has_switch(model: &AiModel) -> bool {
    model.is_usable()
}

/// "131K context", "1M context": how much fits into one conversation, rounded to thousands or, from
/// a million on, to millions of tokens.
fn context_text(tokens: u64) -> String {
    match tokens {
        1_000_000.. => trf("{n}M context", &[("n", &((tokens + 500_000) / 1_000_000))]),
        _ => trf("{n}K context", &[("n", &((tokens + 500) / 1000).max(1))]),
    }
}

/// The line under a model's name: what it can (tools, deciding, thinking, its context) and where it
/// computes, "Tools · Thinking · 131K context · Cloud"; a decision model says "Decides", so one sees
/// what it is for (#231), and a model that can do neither starts with "Can't use tools or decide".
pub fn model_detail(model: &AiModel, place: &str) -> String {
    let mut parts: Vec<String> = Vec::new();
    if model.tools == Some(true) {
        parts.push(tr("Tools").to_string());
    }
    if model.decision == Some(true) {
        parts.push(tr("Decides").to_string());
    }
    if !model.is_usable() {
        parts.push(tr("Can't use tools or decide").to_string());
    }
    if model.reasoning == Some(true) {
        parts.push(tr("Thinking").to_string());
    }
    parts.extend(model.context.map(context_text));
    parts.push(place.to_string());
    parts.join(" · ")
}

/// The models a page lists: those whose name or id holds `search` (any case, spaces around
/// ignored), by name in any case and by id where names are the same (task S.36) – a server's own
/// order says nothing a person looks for. With `closed_filter` (OpenRouter, Open First) those with
/// closed weights only with `show_closed`. Only the page sorts: `ai.list_models` keeps the server's
/// order.
pub fn shown_models<'a>(
    models: &'a [AiModel],
    closed_filter: bool,
    search: &str,
    show_closed: bool,
) -> Vec<&'a AiModel> {
    let search = search.trim().to_lowercase();
    let mut shown: Vec<&AiModel> = models
        .iter()
        .filter(|model| !closed_filter || show_closed || model.open_weights != Some(false))
        .filter(|model| {
            search.is_empty()
                || model.name.to_lowercase().contains(&search)
                || model.id.to_lowercase().contains(&search)
        })
        .collect();
    shown.sort_by_cached_key(|model| (model.name.to_lowercase(), model.id.clone()));
    shown
}

/// The group "Models" of an AI account's page (`docs/settings.md` › KI): first the search, at
/// OpenRouter with "Show closed models" beside it, then one row per model with its name, what it can and where it
/// computes, and its switch; a model that can't use tools faded and without one. While the list
/// is asked for "Loading models…", a failure as a sentence with "Retry". A refused switch is a
/// sentence under the group.
fn models_group<'a>(state: &'a State, account: &'a AccountInfo) -> Element<'a, Message> {
    let page = &state.models_page;
    let provider = ai_provider(&account.id);
    let closed_filter = provider == Some(AiProvider::OpenRouter);
    let server = account_server(account);
    let models = state.models.get(&account.id);
    let mut rows: Vec<Element<'a, Message>> = Vec::new();
    if models.is_some() {
        rows.push(model_filters(page, closed_filter));
    }
    match (models, &page.error) {
        (_, Some(error)) => rows.push(wrapped_row(
            Some(notice_icon()),
            tr("The models could not be listed"),
            error.clone(),
            Some(widgets::text_button(None, tr("Retry"), style::button::secondary, Some(Message::LoadModels))),
        )),
        (None, None) => rows.push(quiet_row(tr("Loading models…"))),
        (Some(models), None) => {
            let shown = shown_models(models, closed_filter, &page.search, page.show_closed);
            if shown.is_empty() {
                let empty =
                    if models.is_empty() { tr("This server offers no models.") } else { tr("No models match.") };
                rows.push(quiet_row(empty));
            }
            rows.extend(shown.into_iter().map(|model| model_row(model, model_place(provider, server, model))));
        }
    }
    let footer = tr("The assistant, its bots and every other module offer only the models that are on.");
    let group = widgets::group(tr("Models"), rows, Some(footer.to_string()));
    match &page.refused {
        Some(refused) => column![group, notice(refused.clone())].spacing(layout::item_gap()).into(),
        None => group,
    }
}

/// The search field over an AI account's models, at OpenRouter (`closed_filter`) with "Show closed
/// models" beside it in one row. The field is as high as the chip (28, `size::control_sm()`): 4
/// above and below its line of text, as a filter, not a form field.
fn model_filters(page: &ModelsPage, closed_filter: bool) -> Element<'_, Message> {
    let search = wizard::field(tr("Search models"), &page.search)
        .padding([space::xs(), space::s()])
        .on_input(Message::ModelSearch);
    let closed = closed_filter.then(|| {
        widgets::choice_chip(
            tr("Show closed models"),
            None,
            page.show_closed,
            Some(Message::ShowClosed(!page.show_closed)),
        )
    });
    row_frame(row![search].push(closed).spacing(space::s()).align_y(Alignment::Center), false)
}

/// One model: its name, what it can and where it computes, and its switch. A model no module can
/// use ([`has_switch`]) stands faded (`text_3`) without a switch.
fn model_row<'a>(model: &'a AiModel, place: &str) -> Element<'a, Message> {
    let detail = model_detail(model, place);
    if !has_switch(model) {
        let lines = column![
            widgets::ellipsized(typo::body(), model.name.as_str(), theme::text_3),
            widgets::ellipsized(typo::meta(), detail, theme::text_3),
        ];
        return row_frame(lines.width(Fill), true);
    }
    let id = model.id.clone();
    let switch = toggler(model.enabled)
        .size(style::CHECK_SIZE)
        .style(style::toggler)
        .on_toggle(move |on| Message::SetModel(id.clone(), on));
    setting_row(None, model.name.as_str(), Some(detail), switch)
}

/// A row that only says something quiet in `text_3`: loading, nothing there.
fn quiet_row<'a>(text: &'a str) -> Element<'a, Message> {
    row_frame(typo::text(typo::body(), text).style(theme::text_3), false)
}

/// The alert icon in the error colour, in front of a failure's words (the colour is only here).
fn notice_icon<'a>() -> Element<'a, Message> {
    icon::icon(Icon::CircleAlert, icon::SIZE)
        .style(|t: &Theme, _| svg::Style { color: Some(palette(t).error_text) })
        .into()
}

/// The status as a dot and a word, when it last synced, and "Sync now" for a connected account.
/// An AI account gets the dot and the word only: nothing of it syncs.
fn status_group<'a>(account: &'a AccountInfo, now: DateTime<Utc>) -> Element<'a, Message> {
    let line = status_line(&account.state);
    let (detail, sync) = match &account.state {
        AccountState::Connected { .. } if account.kind == AI_KIND || account.kind == NTFY_KIND => (None, None),
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

/// Server and user, and where the app password lives. Nothing to show without a server.
fn connection_group<'a>(account: &'a AccountInfo) -> Option<Element<'a, Message>> {
    let (server, user) = match &account.state {
        AccountState::Connected { server, login_name, .. } => (server, Some(login_name)),
        AccountState::SigningIn { server, .. } => (server, None),
        AccountState::NotConnected => return None,
    };
    let mut rows = vec![value_row(tr("Server"), server)];
    // An AI account signs in as nobody: only a key, if any.
    rows.extend(user.filter(|user| !user.is_empty()).map(|user| value_row(tr("User"), user)));
    if account.kind == "caldav" && user.is_some() {
        let change =
            widgets::text_button(None, tr("Change password…"), style::button::secondary, Some(Message::ChangePassword));
        let stored = tr("Stored in the system keychain").to_string();
        rows.push(setting_row(None, tr("Password"), Some(stored), change));
    }
    let stored = match account.kind.as_str() {
        "github" => Some(tr("Token stored in the system keychain")),
        "caldav" => None,
        AI_KIND => Some(tr("An API key, if the server needs one, stays in the system keychain.")),
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

/// Why `source` brings nothing at the moment, as the core keeps it.
fn source_health(data: &Data, source: &SourceRef) -> Option<SourceHealth> {
    data.sources.iter().find(|info| info.source == *source).and_then(|info| info.health)
}

/// Why a source of `kind` is quiet, in calm words that say what helps.
pub fn health_text(kind: SourceKind, health: SourceHealth) -> &'static str {
    match (kind, health) {
        (SourceKind::Github, SourceHealth::MissingScope) => {
            tr("Notifications need a classic token with notifications or repo, or the GitHub CLI sign-in.")
        }
        (_, SourceHealth::MissingScope) => tr("This sign-in may not read notifications."),
        (SourceKind::Talk, SourceHealth::MissingApp) => tr("This Nextcloud has no Talk."),
        (_, SourceHealth::MissingApp) => tr("This Nextcloud has no notifications app, so nothing comes from it."),
        (_, SourceHealth::SignedOut) => tr("The server refused the sign-in. Sign in again to get notifications."),
        (_, SourceHealth::Offline) => tr("The server cannot be reached. Notifications come once it answers."),
    }
}

// ----- An ntfy account's page (task E5.14) --------------------------------------------------

/// "1 topic", "3 topics".
fn topics_text(n: usize) -> String {
    match n {
        1 => tr("1 topic").to_string(),
        n => trf("{n} topics", &[("n", &n)]),
    }
}

/// An ntfy account's page: the status, its topics, the server and login, and the removal at the
/// end. Nothing of it syncs, so the status has no "Sync now".
fn ntfy_detail<'a>(
    account: &'a AccountInfo,
    view: &'a NtfyView,
    data: &Data,
    now: DateTime<Utc>,
) -> Element<'a, Message> {
    let health = account.id.parse::<SourceRef>().ok().and_then(|source| source_health(data, &source));
    column![
        heading(account),
        status_group(account, now),
        topics_group(view, health),
        ntfy_connection_group(view),
        remove_button(),
    ]
    .spacing(layout::section_gap())
    .into()
}

/// The topics by label with their names below; a row opens the topic dialog. "Add topic…" ends the
/// group while the account has room. The sentence under it says why the source is quiet, or what
/// the label is for.
fn topics_group(view: &NtfyView, health: Option<SourceHealth>) -> Element<'_, Message> {
    let mut rows: Vec<Element<'_, Message>> = view
        .topics
        .iter()
        .enumerate()
        .map(|(at, topic)| {
            let row = setting_row(None, topic.label.as_str(), topic.topic.clone(), chevron());
            pressable(row, Some(Message::EditTopic(at)))
        })
        .collect();
    let room = view.topics.len() < TOPICS_MAX;
    let add = widgets::text_button(
        Some(Icon::Plus),
        tr("Add topic…"),
        style::button::ghost,
        room.then_some(Message::AddTopic),
    );
    rows.push(row_frame(on_axis(add), false));
    let footer = match health {
        Some(health) => health_text(SourceKind::Ntfy, health),
        None => tr(
            "The Notification Hub shows each topic by its label. On a public server a topic name works like a password.",
        ),
    };
    widgets::group(tr("Topics"), rows, Some(footer.to_string()))
}

/// The server and how the account signs in, with "Change login…".
fn ntfy_connection_group(view: &NtfyView) -> Element<'_, Message> {
    let login = match (&view.user, view.login) {
        (Some(user), _) => trf("Signed in as {user}", &[("user", user)]),
        (None, true) => tr("Access token").to_string(),
        (None, false) => tr("No login").to_string(),
    };
    let change = widgets::text_button(None, tr("Change login…"), style::button::secondary, Some(Message::ChangeLogin));
    let rows = vec![value_row(tr("Server"), &view.server), setting_row(None, tr("Login"), Some(login), change)];
    let footer = tr("The password or token stays in the system keychain. Agents never see it.");
    widgets::group(tr("Connection"), rows, Some(footer.to_string()))
}

/// The topic dialog: for a new topic its name and label, for one the account has its name (fixed)
/// and label with "Unsubscribe…" at the left of the foot. What is wrong shows after the first
/// "Subscribe" or "Save"; the button waits while the server checks the topic.
fn topic_dialog<'a>(edit: &'a TopicEdit, account: &'a AccountInfo) -> Element<'a, Message> {
    let new = edit.stored.is_none();
    let (title, purpose) = if new {
        (tr("Add topic"), trf("Subscribe {account} to another topic.", &[("account", &account.label)]))
    } else {
        (tr("Edit topic"), tr("The Notification Hub shows the topic by its label.").to_string())
    };
    let head = column![typo::text(typo::title(), title), typo::text(typo::body(), purpose).style(theme::text_2)]
        .spacing(space::xs());
    let submit = (!edit.sending).then_some(Message::SaveTopic);
    let name_row = if new {
        let field = wizard::field("backups-7f3a", edit.name.as_str())
            .on_input(|name| Message::TopicNameEdited(TopicName::new(name)))
            .on_submit_maybe(submit.clone());
        row_frame(field, false)
    } else {
        // Fixed: a topic of another name is another topic.
        row_frame(typo::text(typo::body(), edit.name.as_str()).style(theme::text_2), false)
    };
    let label =
        wizard::field("Backups", &edit.label).on_input(Message::TopicLabelEdited).on_submit_maybe(submit.clone());
    let mut body = column![
        widgets::group(tr("Topic name"), vec![name_row], None),
        widgets::group(
            tr("Label"),
            vec![row_frame(label, false)],
            Some(tr("Shown in the Notification Hub instead of the name.").to_string())
        ),
    ]
    .spacing(layout::section_gap());
    if let Some(problem) = edit.problem().filter(|_| edit.tried).or_else(|| edit.error.clone()) {
        body = body.push(error_text(problem));
    }
    // On the dialog's content axis, like "Remove account…" on the page.
    let unsubscribe = (!new).then(|| {
        on_axis(widgets::text_button(
            None,
            tr("Unsubscribe…"),
            style::button::danger,
            (!edit.sending).then_some(Message::AskUnsubscribe),
        ))
    });
    let save = if edit.sending {
        widgets::busy_button(tr("Checking…"), style::button::primary)
    } else {
        widgets::text_button(None, if new { tr("Subscribe") } else { tr("Save") }, style::button::primary, submit)
    };
    let foot = row![]
        .push(unsubscribe)
        .push(blank::horizontal())
        .push(widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(Message::CloseTopic)))
        .push(save)
        .spacing(space::s())
        .align_y(Alignment::Center);
    dialog(dialog_frame(head, body, foot, space::xl()), size::form_max(), Message::CloseTopic)
}

/// "Unsubscribe from this topic?", from the topic dialog; its entries stay in the inbox.
fn unsubscribe_confirm<'a>(edit: &'a TopicEdit) -> Element<'a, Message> {
    let label = edit.stored.as_deref().unwrap_or_default();
    confirm(
        tr("Unsubscribe from this topic?"),
        trf("{label} no longer comes to the Notification Hub. Its entries stay there.", &[("label", &label)]),
        tr("Unsubscribe"),
        Message::Unsubscribe,
        Message::CancelUnsubscribe,
    )
}

/// "Change login": the login form of the add-account dialog for the account's server, a refusal
/// in the body, "Cancel" and "Save" right; the button waits while the server checks the login.
fn login_dialog<'a>(change: &'a LoginChange, view: &'a NtfyView) -> Element<'a, Message> {
    let head = column![
        typo::text(typo::title(), tr("Change login")),
        typo::text(typo::body(), trf("Sign in to {server}. The topics stay as they are.", &[("server", &view.server)]))
            .style(theme::text_2),
    ]
    .spacing(space::xs());
    let ready = !change.sending && change.login.is_ready();
    let fields = wizard::ntfy_login_fields(
        &change.login,
        Message::LoginKind,
        Message::LoginUserEdited,
        Message::LoginSecretEdited,
        ready.then_some(Message::SubmitLogin),
    );
    let mut body = column![fields].spacing(layout::section_gap());
    if let Some(error) = &change.error {
        body = body.push(error_text(error.clone()));
    }
    let save = if change.sending {
        widgets::busy_button(tr("Checking…"), style::button::primary)
    } else {
        widgets::text_button(None, tr("Save"), style::button::primary, ready.then_some(Message::SubmitLogin))
    };
    let foot = row![
        blank::horizontal(),
        widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(Message::CloseLogin)),
        save,
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    dialog(dialog_frame(head, body, foot, space::xl()), size::form_max(), Message::CloseLogin)
}

/// A sentence in the error colour.
fn error_text<'a>(text: String) -> Element<'a, Message> {
    typo::text(typo::body(), text).style(|t: &Theme| text::Style { color: Some(palette(t).error_text) }).into()
}

/// A row with a value at the right in `text_2`, on one line.
fn value_row<'a>(label: &'a str, value: &'a str) -> Element<'a, Message> {
    setting_row(None, label, None, typo::text(typo::body(), value).style(theme::text_2).wrapping(Wrapping::None))
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
            ntfy: None,
            services: None,
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

    fn of_kind(id: &str, kind: &str) -> AccountInfo {
        AccountInfo { kind: kind.into(), ..account(id, connected(None, None)) }
    }

    /// Builds the page in every class with a window, and the provider column where it has one.
    fn draw(state: &State, data: &Data) {
        let palette = Palette::dark();
        for class in [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow] {
            drop(view(state, data, class, &palette));
            for content in [600.0, layout::wide_column_from()] {
                drop(provider_column(state, data, class, content, &palette));
            }
        }
    }

    /// Task S.37: without a choice the list shows the first provider with an account or a sign-in
    /// in progress, else Nextcloud; a choice holds while its group is listed, and a kind of its own
    /// that lost its last account falls back.
    #[test]
    fn the_list_shows_the_chosen_provider_or_the_first_with_accounts() {
        let (github, slack) = (of_kind("github:anna", "github"), of_kind("slack:anna", "slack"));
        let signing_in =
            AccountInfo { id: String::new(), kind: "ai".into(), ..account("", AccountState::NotConnected) };
        let shown = |choice: Option<&str>, entries: &[&AccountInfo]| {
            let groups = providers(entries);
            chosen(choice, &groups, entries).key().to_string()
        };
        assert_eq!(shown(None, &[]), "nextcloud", "nothing: the first group");
        assert_eq!(shown(None, &[&slack, &github]), "github", "the first fixed group with an account");
        assert_eq!(shown(None, &[&signing_in]), "ai", "a sign-in in progress counts");
        assert_eq!(shown(Some("ntfy"), &[&github]), "ntfy", "an empty fixed group can be chosen");
        assert_eq!(shown(Some("slack"), &[&slack]), "slack");
        assert_eq!(shown(Some("slack"), &[&github]), "github", "a kind of its own without accounts falls back");
        assert_eq!(shown(Some("nowhere"), &[]), "nextcloud");
        assert_eq!((key_of("caldav"), key_of("mail"), key_of("slack")), ("mail", "mail", "slack"));
    }

    /// Task S.37: the column counts the accounts of a provider, not the sign-ins in progress, and
    /// tints its icon with the worst way one of them stands, in a colour readable on light surfaces.
    #[test]
    fn a_provider_counts_its_accounts_and_takes_the_worst_tone() {
        let ok = of_kind("nextcloud:a", "nextcloud");
        let broken = AccountInfo { id: "nextcloud:b".into(), ..account("", connected(None, Some("Server gone"))) };
        let waiting = AccountInfo { id: String::new(), ..account("", AccountState::NotConnected) };
        let nextcloud = Listed::Fixed(Group::Nextcloud);
        assert_eq!(accounts_in(nextcloud, &[&ok, &waiting]), 1, "the sign-in is no account yet");
        assert_eq!(accounts_in(Listed::Fixed(Group::Mail), &[&ok]), 0);
        assert_eq!(group_tone(nextcloud, &[]), None);
        assert_eq!(group_tone(nextcloud, &[&ok]), Some(Tone::Ok));
        assert_eq!(group_tone(nextcloud, &[&ok, &waiting]), Some(Tone::Attention));
        assert_eq!(group_tone(nextcloud, &[&waiting, &broken, &ok]), Some(Tone::Error));
        let palette = Palette::dark();
        assert_eq!(provider_mark(nextcloud, &[&ok], &palette), Mark::Icon(Icon::Cloud));
        let alert = |colour| Mark::Badged(Icon::Cloud, colour);
        assert_eq!(provider_mark(nextcloud, &[&waiting], &palette), alert(palette.attention_text));
        assert_eq!(provider_mark(nextcloud, &[&broken], &palette), alert(palette.error_text));
        let light = Palette::light();
        assert_eq!(provider_mark(nextcloud, &[&broken], &light), alert(light.error_text), "readable on light");
    }

    /// The width the list gets beside the providers' column in a content `content` wide (as
    /// `Ui::body` lays it out): the content less the panel's two edges, the column and its line and the
    /// page's padding, at most the reading width.
    fn page_width(class: SizeClass, content: f32) -> f32 {
        let column = column_mode(class, content).map_or(0.0, |mode| column_width(mode) + size::HAIRLINE);
        let page = layout::page_padding();
        (content - 2.0 * size::HAIRLINE - column - page.left - page.right).min(size::content_max())
    }

    /// Issue #248: the column gets its names only where the list keeps its reading width beside it,
    /// so growing the content or the window never makes the list narrower – except for the shell's
    /// own sidebar opening at 1024, by at most its difference to the rail.
    #[test]
    fn the_provider_column_never_makes_the_list_narrower() {
        let from = layout::wide_column_from();
        assert_eq!(column_mode(SizeClass::Wide, from), Some(SidebarMode::Expanded));
        assert_eq!(column_mode(SizeClass::Wide, from - 1.0), Some(SidebarMode::Rail));
        assert_eq!(column_mode(SizeClass::Medium, from), Some(SidebarMode::Expanded), "the width decides");
        assert_eq!(column_mode(SizeClass::Narrow, from), None, "chips there");
        assert_eq!(page_width(SizeClass::Wide, from), size::content_max(), "the whole reading width");
        // (a) by the content width.
        let mut before = page_width(SizeClass::Wide, 600.0);
        for content in 601..=2000 {
            let width = page_width(SizeClass::Wide, content as f32);
            assert!(width >= before, "content {content}: {width} < {before}");
            before = width;
        }
        // (b) and (c) by the window width, with the sidebar collapsed and expanded.
        let jump = size::sidebar() - shell::rail();
        for choice in [SidebarMode::Rail, SidebarMode::Expanded] {
            let at = |width: f32| {
                let class = layout::of(iced::Size::new(width, 900.0));
                page_width(class, shell::content_width(class, choice, width))
            };
            let mut before = at(600.0);
            for window in 601..=2000 {
                let width = at(window as f32);
                if choice == SidebarMode::Expanded && window == 1024 {
                    assert!(before - width <= jump, "the sidebar's jump only: {before} → {width}");
                } else {
                    assert!(width >= before, "{choice:?}, window {window}: {width} < {before}");
                }
                before = width;
            }
        }
        let content = shell::content_width(SizeClass::Wide, SidebarMode::Expanded, 1024.0);
        assert_eq!(column_mode(SizeClass::Wide, content), Some(SidebarMode::Rail), "no wide column at 1024");
    }

    /// Task S.37: the overview's "Open accounts" of a row shows that row's account's provider, also
    /// with accounts of several providers in need and whatever was chosen; "Add" and the import show
    /// the provider they add to.
    #[test]
    fn the_way_into_the_list_chooses_the_provider_it_is_about() {
        let github = AccountInfo { kind: "github".into(), ..account("github:anna", connected(None, Some("Gone"))) };
        let ntfy = AccountInfo { kind: "ntfy".into(), ..account("ntfy:anna", AccountState::NotConnected) };
        let caldav = AccountInfo { kind: "caldav".into(), ..account("caldav:anna", AccountState::NotConnected) };
        let data =
            Data { accounts: vec![of_kind("nextcloud:a", "nextcloud"), github, ntfy, caldav], ..Data::default() };
        let mut state = State { provider: Some("ai".into()), selected: Some("nextcloud:a".into()), ..State::default() };
        show_provider_of(&mut state, &data, "ntfy:anna");
        assert_eq!(state.provider.as_deref(), Some("ntfy"), "the row's provider, not the first in need");
        assert_eq!(state.selected, None, "the list, not the page left open");
        show_provider_of(&mut state, &data, "github:anna");
        assert_eq!(state.provider.as_deref(), Some("github"));
        show_provider_of(&mut state, &data, "caldav:anna");
        assert_eq!(state.provider.as_deref(), Some("mail"), "CalDAV stands under Mail");
        show_provider_of(&mut state, &data, "gone:anna");
        assert_eq!(state.provider.as_deref(), Some("mail"), "an account gone by then changes nothing");
        let fine = Data { accounts: vec![of_kind("nextcloud:a", "nextcloud")], ..Data::default() };
        update(&mut state, Message::Add(wizard::Provider::Ntfy), &fine);
        assert_eq!(state.provider.as_deref(), Some("ntfy"));
        update(&mut state, Message::ImportTorroMail, &fine);
        assert_eq!(state.provider.as_deref(), Some("mail"));
    }

    /// Task S.37: choosing a provider shows it; opening an account chooses its provider, so "Back"
    /// lands there – a CalDAV account under Mail.
    #[test]
    fn opening_an_account_chooses_its_provider() {
        let caldav = AccountInfo { kind: "caldav".into(), ..account("caldav:anna", connected(None, None)) };
        let data = Data { accounts: vec![of_kind("github:anna", "github"), caldav], ..Data::default() };
        let mut state = State::default();
        update(&mut state, Message::ShowProvider("ntfy".into()), &data);
        assert_eq!(state.provider.as_deref(), Some("ntfy"));
        draw(&state, &data);
        update(&mut state, Message::Open("caldav:anna".into()), &data);
        update(&mut state, Message::Back, &data);
        assert_eq!(state.provider.as_deref(), Some("mail"));
        assert!(provider_column(&state, &data, SizeClass::Wide, 1200.0, &Palette::dark()).is_some());
        assert!(provider_column(&state, &data, SizeClass::Narrow, 480.0, &Palette::dark()).is_none(), "chips there");
        update(&mut state, Message::Open("github:anna".into()), &data);
        assert!(
            provider_column(&state, &data, SizeClass::Wide, 1200.0, &Palette::dark()).is_none(),
            "not beside a detail page"
        );
    }

    /// Task S.30: the groups stand in a fixed order whatever accounts there are, also with none; a
    /// kind no group holds gets a group of its own at the end, once.
    #[test]
    fn the_groups_are_fixed_and_unknown_kinds_follow_at_the_end() {
        let fixed: Vec<Listed<'_>> = Group::ALL.into_iter().map(Listed::Fixed).collect();
        assert_eq!(providers(&[]), fixed, "every group without an account");
        assert_eq!(
            Group::ALL,
            [Group::Nextcloud, Group::Mail, Group::GitHub, Group::Ai, Group::Ntfy],
            "Nextcloud, Mail, GitHub, AI, ntfy"
        );
        let (github, slack, a, b, more) = (
            of_kind("github:anna", "github"),
            of_kind("slack:anna", "slack"),
            of_kind("nextcloud:a", "nextcloud"),
            of_kind("nextcloud:b", "nextcloud"),
            of_kind("slack:ben", "slack"),
        );
        let groups = providers(&[&slack, &github, &a, &b, &more]);
        assert_eq!(groups[..5], fixed[..], "the order does not follow the accounts");
        assert_eq!(groups[5..], [Listed::Other("slack")], "the unknown kind once, at the end");
        assert!(
            !account_providers(&[&slack, &github, &a, &b, &more]).contains(&Listed::Fixed(Group::Ai)),
            "AI has its own section"
        );
        let titles: Vec<String> = Group::ALL.into_iter().map(Group::title).collect();
        assert_eq!(titles, [tr("Nextcloud"), tr("Mail"), tr("GitHub"), tr("AI"), "ntfy"]);
        // Every page builds: no accounts, accounts, an unknown kind.
        draw(&State::default(), &Data::default());
        draw(&State::default(), &Data { accounts: vec![slack, github, a], ..Data::default() });
        drop(ai_view(&State::default(), &Data::default(), SizeClass::Wide, &Palette::dark()));
    }

    /// Task S.30: a mail provider's CalDAV account stands in the group Mail and says "CalDAV".
    #[test]
    fn a_caldav_account_is_listed_under_mail_as_caldav() {
        let mail = Listed::Fixed(Group::Mail);
        assert!(mail.holds("mail") && mail.holds("caldav") && !mail.holds("nextcloud"));
        let data = caldav_data();
        let caldav = &data.accounts[0];
        let detail = row_detail(caldav, None, Utc::now());
        assert!(detail.starts_with(&format!("{} · ", tr("CalDAV"))), "{detail}");
        assert!(detail.contains(tr("Connected")), "{detail}");
        let nextcloud = account("nextcloud:anna", connected(None, None));
        assert!(!row_detail(&nextcloud, None, Utc::now()).contains(tr("CalDAV")));
    }

    /// Task S.30: each group's "Add" opens the dialog at its own provider (the AI providers ask for
    /// the servers on this computer at once); the header bar has no button of its own.
    #[test]
    fn every_group_adds_its_own_provider_and_the_header_has_no_button() {
        let data = two_accounts();
        for class in [SizeClass::Wide, SizeClass::Medium, SizeClass::Narrow, SizeClass::Mini] {
            assert!(header(&State::default(), &data, class).actions.is_empty(), "{class:?}");
        }
        let providers = Group::ALL.map(Group::provider);
        assert_eq!(
            providers,
            [
                wizard::Provider::Nextcloud,
                wizard::Provider::Mail,
                wizard::Provider::GitHub,
                wizard::Provider::Ai,
                wizard::Provider::Ntfy
            ]
        );
        for provider in providers {
            let mut state = State::default();
            let action = update(&mut state, Message::Add(provider), &data);
            assert!(state.wizard.is_some() && overlay(&state, &data).is_some(), "{provider:?}");
            match provider {
                wizard::Provider::Ai => assert!(matches!(action, Action::Send(Request::FindAiServers)), "{action:?}"),
                _ => assert!(matches!(action, Action::None), "{provider:?}: {action:?}"),
            }
        }
    }

    /// Task S.30: the page asks for TorroMail's accounts as it opens and when the dialog closes,
    /// keeps the answer, and offers the import only while an account can be taken over.
    #[test]
    fn the_import_is_offered_only_with_an_account_to_take_over() {
        let data = Data::default();
        let mut state = State::default();
        assert!(unlisted(&state) && !offers_import(&state));
        assert!(matches!(opened(&mut state), Action::Send(Request::TorroMailAccounts)));
        assert!(!unlisted(&state), "asked");
        let demo = super::super::launch::demo_answer(&Request::TorroMailAccounts, &super::super::launch::demo_data())
            .expect("the demo's TorroMail");
        assert!(answered(&mut state, &Ok(demo)));
        assert!(offers_import(&state), "the demo's Posteo account can be taken over");
        let Some(accounts) = state.torromail.as_mut() else { panic!("listed") };
        for account in accounts.iter_mut() {
            account.connected = true;
        }
        assert!(!offers_import(&state), "everything is here already");
        assert!(matches!(
            update(&mut state, Message::ImportTorroMail, &data),
            Action::Send(Request::TorroMailAccounts)
        ));
        assert!(state.wizard.is_some() && overlay(&state, &data).is_some());
        let closed = update(&mut state, Message::Wizard(wizard::Message::Close), &data);
        assert!(matches!(closed, Action::Send(Request::TorroMailAccounts)), "asks again after the dialog");
        assert!(state.wizard.is_none());
        // A list that cannot be read leaves the button out, quietly, and is asked for no more.
        let mut broken = State::default();
        opened(&mut broken);
        assert!(answered(&mut broken, &Err("TorroMail's state.json is not valid JSON".into())));
        assert!(!offers_import(&broken) && !unlisted(&broken));
        assert!(!answered(&mut broken, &Err("no template".into())), "only the one answer it waited for");
        draw(&state, &data);
    }

    /// Task S.30: the data rule stands under AI and its switch sends the setting, never a right.
    #[test]
    fn the_data_rule_under_ai_sends_the_setting() {
        let data = Data::default();
        for on in [true, false] {
            let action = update(&mut State::default(), Message::MailToCloud(on), &data);
            let Action::Send(Request::SetSetting { key, value }) = action else { panic!("a setting") };
            assert_eq!((key, value), (SettingKey::MailToCloud, serde_json::json!(on)));
            let settings = Settings { mail_to_cloud: on, ..Settings::default() };
            drop(data_rule_group(&Data { settings, ..Data::default() }, &BTreeMap::new()));
        }
        // Issue #325: the permission of Sorting is only stopped here, never switched on.
        let action = update(&mut State::default(), Message::StopSortingMail, &data);
        let Action::Send(Request::SetSetting { key, value }) = action else { panic!("a setting") };
        assert_eq!((key, value), (SettingKey::DecideMailToCloud, serde_json::json!(false)));
        let settings = Settings { decide_mail_to_cloud: Some(true), ..Settings::default() };
        drop(data_rule_group(&Data { settings, ..Data::default() }, &BTreeMap::new()));
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
        update(&mut state, Message::Add(wizard::Provider::Nextcloud), &data);
        assert!(overlay(&state, &data).is_some());
        assert!(matches!(escape(&state), Some(Message::Wizard(wizard::Message::Close))));
    }

    #[test]
    fn pages_build_for_list_and_detail() {
        let now = Utc::now();
        let anna = account("nextcloud:anna@cloud.example.org", connected(Some(now), None));
        let data =
            Data { accounts: vec![anna, account("nextcloud:ben", AccountState::NotConnected)], ..Data::default() };
        let mut state = State::default();
        draw(&state, &data);
        update(&mut state, Message::Open("nextcloud:anna@cloud.example.org".into()), &data);
        draw(&state, &data);
        draw(&State::default(), &Data::default());
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
        draw(&State::default(), &data);
        // Only a sign-in: a row instead of the empty state.
        draw(&State::default(), &Data { accounts: vec![waiting], ..Data::default() });
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
            draw(&state, &data);
        }
    }

    #[test]
    fn the_dialog_takes_only_its_own_answers() {
        let mut state = State::default();
        assert!(!answered(&mut state, &Err("no template".into())), "no dialog: the shell shows it");
        let ask = update(&mut state, Message::ImportTorroMail, &Data::default());
        assert!(matches!(ask, Action::Send(Request::TorroMailAccounts)), "it opens asking for TorroMail's accounts");
        assert!(answered(&mut state, &Ok(Response::TorroMailAccounts { accounts: Vec::new() })));
        assert!(!answered(&mut state, &Err("no template".into())), "the dialog waits for nothing else");
        // A dialog at a provider takes the answers of its own steps only.
        let mut github = State::default();
        update(&mut github, Message::Add(wizard::Provider::GitHub), &Data::default());
        assert!(!answered(&mut github, &Err("no template".into())));
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

    /// Task S.21: in place of "Calendars and lists" the page says where they are chosen now and
    /// opens Calendar or Tasks with `apps.open_app`, by window class.
    #[test]
    fn the_new_place_of_the_collections_opens_its_window() {
        let data = Data::default();
        for class in ["torro-calendar", "torro-tasks"] {
            let action = update(&mut State::default(), Message::OpenApp(class), &data);
            assert!(
                matches!(action, Action::Send(Request::OpenApp { ref app, target: None }) if app == class),
                "{action:?}"
            );
        }
        drop(collections_moved());
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
        draw(&state, &data);
        update(&mut state, Message::Open(CALDAV.into()), &data);
        draw(&state, &data);
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
            draw(&state, &data);
            update(&mut state, Message::ChangePassword, &data);
            assert!(overlay(&state, &data).is_some());
            update(&mut state, Message::ChangeServers, &data);
            assert!(state.password.is_none() && overlay(&state, &data).is_some(), "one dialog at a time");
            assert!(matches!(escape(&state), Some(Message::CloseServers)));
        }
        // Not signed in: one status row, and the password can still be given.
        let (mut state, mut data) = open_mail(MAILBOX);
        data.accounts[0].state = AccountState::NotConnected;
        draw(&state, &data);
        update(&mut state, Message::ChangePassword, &data);
        assert!(state.password.is_some());
    }

    /// Task S.21: a mail account's page keeps only its services; the sender name, the signature,
    /// how mail is written, the folder questions and "Show new mail" are set in the windows, and
    /// its page only leads there.
    #[test]
    fn the_services_are_the_only_setting_left_on_a_mail_page() {
        let (mut state, data) = open_mail(MAILBOX);
        let request = sent(update(&mut state, Message::SetService(MailService::Tasks, false), &data));
        assert!(
            matches!(&request, Request::UpdateMailAccount { services: Some(services), notify: None,
                sender_name: None, signature: None, folder_roles: None, format: None, .. }
                if *services == Services { mail: true, calendars: true, tasks: false }),
            "{request:?}"
        );
        assert!(matches!(update(&mut state, Message::SetService(MailService::Mail, true), &data), Action::None));
        for class in ["torro-inbox-mail", "torro-inbox"] {
            let action = update(&mut state, Message::OpenApp(class), &data);
            assert!(
                matches!(action, Action::Send(Request::OpenApp { ref app, target: None }) if app == class),
                "{action:?}"
            );
        }
        assert_eq!(mail_moved().len(), 2, "Mail and the Notification Hub");
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
                trusted_certificates,
            } => {
                assert!(trusted_certificates.is_empty());
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

    // ----- A certificate the system does not trust (task M11.4) ---------------------------------

    fn untrusted(service: &str, sha256: &str) -> Response {
        let host = "mail.home.lan".to_string();
        Response::UntrustedCertificate { service: service.into(), host, sha256: sha256.into(), public: false }
    }

    /// The password and trusted certificates of a `ConnectMail`.
    fn connect_mail(action: Action) -> (String, Vec<String>) {
        match sent(action) {
            Request::ConnectMail { password, trusted_certificates, .. } => {
                (password.expose().to_string(), trusted_certificates)
            }
            other => panic!("expected ConnectMail, got {other:?}"),
        }
    }

    #[test]
    fn server_settings_ask_about_a_certificate_and_send_every_one_trusted() {
        let (mut state, data) = open_mail(MAILBOX);
        update(&mut state, Message::ChangeServers, &data);
        update(&mut state, Message::ServersPasswordEdited(Secret::new("pw")), &data);
        let (_, trusted) = connect_mail(update(&mut state, Message::SubmitServers, &data));
        assert!(trusted.is_empty());
        let (imap, caldav) = (vec!["AB"; 32].join(":"), vec!["CD"; 32].join(":"));
        assert!(answered(&mut state, &Ok(untrusted("IMAP", &imap))));
        let dialog = state.servers.as_ref().expect("the dialog asks");
        assert_eq!(dialog.certificates.asking().map(|asked| asked.sha256.as_str()), Some(imap.as_str()));
        assert!(!dialog.sending && !dialog.password.is_blank(), "the password stays for the question");
        assert_eq!(dialog.error, None);
        drop(overlay(&state, &data));

        // "Trust certificate" sends again with the password; the question stays while it is out
        // (no jump back to the form), and a second question takes its place.
        let (password, trusted) = connect_mail(update(&mut state, Message::SubmitServers, &data));
        assert_eq!((password.as_str(), trusted), ("pw", vec![imap.clone()]));
        let dialog = state.servers.as_ref().expect("the dialog");
        assert!(dialog.sending);
        assert_eq!(dialog.certificates.shown().map(|asked| asked.sha256.as_str()), Some(imap.as_str()));
        drop(overlay(&state, &data));
        assert!(matches!(update(&mut state, Message::CertificateBack, &data), Action::None));
        assert!(state.servers.as_ref().is_some_and(|dialog| dialog.certificates.shown().is_some()), "Back is off");
        answered(&mut state, &Ok(untrusted("CalDAV", &caldav)));
        let dialog = state.servers.as_ref().expect("the dialog");
        assert_eq!(dialog.certificates.shown().map(|asked| asked.sha256.as_str()), Some(caldav.as_str()));
        let (_, trusted) = connect_mail(update(&mut state, Message::SubmitServers, &data));
        assert_eq!(trusted, [imap, caldav]);
        // Any other answer clears the password.
        assert!(answered(&mut state, &Err("The server cannot be reached.".into())));
        let dialog = state.servers.as_ref().expect("the dialog stays");
        assert!(dialog.password.is_blank() && dialog.certificates.shown().is_none(), "the form again");
        assert_eq!(dialog.error.as_deref(), Some("The server cannot be reached."));
    }

    #[test]
    fn back_from_the_question_clears_the_password_of_either_dialog() {
        let (mut state, data) = open_mail(MAILBOX);
        update(&mut state, Message::ChangeServers, &data);
        update(&mut state, Message::ServersPasswordEdited(Secret::new("pw")), &data);
        update(&mut state, Message::SubmitServers, &data);
        answered(&mut state, &Ok(untrusted("IMAP", &vec!["AB"; 32].join(":"))));
        update(&mut state, Message::CertificateBack, &data);
        let dialog = state.servers.as_ref().expect("the form again");
        assert!(dialog.password.is_blank() && dialog.certificates.asking().is_none());
        assert!(matches!(update(&mut state, Message::SubmitServers, &data), Action::None), "the password is gone");
        drop(overlay(&state, &data));

        update(&mut state, Message::ChangePassword, &data);
        update(&mut state, Message::PasswordEdited(Secret::new("new-secret")), &data);
        update(&mut state, Message::SubmitPassword, &data);
        answered(&mut state, &Ok(untrusted("SMTP", &vec!["EF"; 32].join(":"))));
        assert!(state.password.as_ref().is_some_and(|change| !change.password.is_blank()));
        update(&mut state, Message::CertificateBack, &data);
        let change = state.password.as_ref().expect("the form again");
        assert!(change.password.is_blank() && change.certificates.asking().is_none());
    }

    #[test]
    fn change_password_asks_about_a_certificate_like_server_settings() {
        let (mut state, data) = open_mail(MAILBOX);
        update(&mut state, Message::ChangePassword, &data);
        update(&mut state, Message::PasswordEdited(Secret::new("new-secret")), &data);
        update(&mut state, Message::SubmitPassword, &data);
        let sha = vec!["AB"; 32].join(":");
        assert!(answered(&mut state, &Ok(untrusted("IMAP", &sha))));
        drop(overlay(&state, &data));
        let (password, trusted) = connect_mail(update(&mut state, Message::SubmitPassword, &data));
        assert_eq!((password.as_str(), trusted), ("new-secret", vec![sha.clone()]));
        // The question stays while signing in, so the dialog keeps its width.
        let change = state.password.as_ref().expect("the dialog");
        assert!(change.sending);
        assert_eq!(change.certificates.shown().map(|asked| asked.sha256.as_str()), Some(sha.as_str()));
        drop(overlay(&state, &data));
        let done = Response::MailConnected {
            account: data.accounts[0].clone(),
            calendars: true,
            tasks: true,
            open_roles: Vec::new(),
        };
        assert!(answered(&mut state, &Ok(done)));
        assert!(state.password.is_none(), "connected: the dialog closes");
        // A refusal clears the password too.
        update(&mut state, Message::ChangePassword, &data);
        update(&mut state, Message::PasswordEdited(Secret::new("wrong")), &data);
        update(&mut state, Message::SubmitPassword, &data);
        answered(&mut state, &Err(tr("Password was refused").into()));
        assert!(state.password.as_ref().is_some_and(|change| change.password.is_blank()));
    }

    #[test]
    fn the_page_lists_the_trusted_certificates() {
        let (state, data) = open_mail("mail:anna@work.example.net");
        let mail =
            data.accounts.iter().find_map(|account| account.mail.as_ref().filter(|m| m.address.contains("work")));
        assert_eq!(mail.map(|mail| mail.trusted_certificates.len()), Some(1), "the demo trusts one");
        assert!(mail.and_then(trusted_certificates_group).is_some(), "their own group");
        let none = MailView { trusted_certificates: Vec::new(), ..mail.expect("the work account").clone() };
        assert!(trusted_certificates_group(&none).is_none(), "no group without one");
        draw(&state, &data);
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
    fn a_mail_account_builds_with_and_without_a_calendar_address() {
        let (state, mut data) = open_mail("mail:anna@work.example.net");
        assert!(data.accounts[1].mail.as_ref().is_some_and(|mail| mail.caldav.is_none()));
        draw(&state, &data);
        data.accounts[1].mail.as_mut().expect("mail").caldav = Some("https://dav.example.net/".into());
        draw(&state, &data);
    }

    // ----- An AI account's models (task S.31) ----------------------------------------------------

    const OPENROUTER: &str = "ai:openrouter@openrouter.ai";

    /// The demo's OpenRouter page with its models as the demo answers them.
    fn open_ai() -> (State, Data) {
        let data = super::super::launch::demo_data();
        let mut state = State::default();
        let ask = update(&mut state, Message::Open(OPENROUTER.into()), &data);
        let request = sent(ask);
        assert!(
            matches!(&request, Request::AiModels { account, all: true } if account == OPENROUTER),
            "opening the page asks for all its models: {request:?}"
        );
        assert!(state.models_page.loading);
        draw(&state, &data);
        let answer = super::super::launch::demo_answer(&request, &data).expect("the demo's models");
        assert!(answered(&mut state, &Ok(answer)));
        assert!(!state.models_page.loading);
        (state, data)
    }

    fn listed_model<'a>(state: &'a State, id: &str) -> &'a AiModel {
        state.models[OPENROUTER].iter().find(|model| model.id == id).expect("a demo model")
    }

    #[test]
    fn the_provider_comes_from_the_account_id() {
        assert_eq!(ai_provider("ai:ollama@localhost:11434"), Some(AiProvider::Ollama));
        assert_eq!(ai_provider(OPENROUTER), Some(AiProvider::OpenRouter));
        assert_eq!(ai_provider("ai:lm_studio@localhost:1234"), Some(AiProvider::LmStudio));
        assert_eq!(ai_provider("ai:openai_compatible@gpu.local:8000"), Some(AiProvider::OpenAiCompatible));
        for other in ["ai:unknown@host", "ai:ollama", "nextcloud:anna@cloud.example.org", ""] {
            assert_eq!(ai_provider(other), None, "{other}");
        }
    }

    /// The group Models lists every model with what it can and where it computes; only the page of
    /// an AI account asks for them, and a reload asks for nothing.
    #[test]
    fn an_ai_page_lists_its_models() {
        let (state, data) = open_ai();
        assert_eq!(state.models[OPENROUTER].len(), super::super::launch::demo_models().len());
        let qwen = listed_model(&state, "qwen/qwen3-235b-a22b");
        let place = model_place(Some(AiProvider::OpenRouter), "https://openrouter.ai/api/v1", qwen);
        assert_eq!(place, tr("Cloud"));
        assert_eq!(
            model_detail(qwen, place),
            [tr("Tools"), tr("Thinking"), &trf("{n}K context", &[("n", &131)]), tr("Cloud")].join(" · ")
        );
        draw(&state, &data);
        let mut nextcloud = State::default();
        let action = update(&mut nextcloud, Message::Open("nextcloud:anna@cloud.example.org".into()), &data);
        assert!(matches!(action, Action::None) && !nextcloud.models_page.loading, "no models elsewhere");
        assert!(matches!(update(&mut nextcloud, Message::LoadModels, &data), Action::None));
    }

    #[test]
    fn the_context_is_counted_in_thousands_or_millions() {
        assert_eq!(context_text(131_072), trf("{n}K context", &[("n", &131)]));
        assert_eq!(context_text(200_000), trf("{n}K context", &[("n", &200)]));
        assert_eq!(context_text(300), trf("{n}K context", &[("n", &1)]));
        assert_eq!(context_text(1_048_576), trf("{n}M context", &[("n", &1)]));
    }

    /// A switch sends `ai.set_model_enabled` for the open account's model and shows the new state
    /// at once; the list's row counts what is on.
    #[test]
    fn a_switch_sends_set_model_enabled() {
        let (mut state, data) = open_ai();
        assert_eq!(models_on(&state, OPENROUTER), Some(1), "the demo has one on");
        let request = sent(update(&mut state, Message::SetModel("deepseek/deepseek-chat-v3.1".into(), true), &data));
        assert!(
            matches!(&request, Request::SetModelEnabled { account, model, enabled: true }
                if account == OPENROUTER && model == "deepseek/deepseek-chat-v3.1"),
            "{request:?}"
        );
        assert!(listed_model(&state, "deepseek/deepseek-chat-v3.1").enabled, "on at once");
        assert_eq!(models_on(&state, OPENROUTER), Some(2));
        let account = data.accounts.iter().find(|account| account.id == OPENROUTER).expect("demo AI account");
        assert_eq!(
            row_detail(account, models_on(&state, OPENROUTER), Utc::now()),
            format!("{} · {}", tr("Connected"), trf("{n} models on", &[("n", &2)]))
        );
        assert_eq!(row_detail(account, None, Utc::now()), tr("Connected"), "not listed yet: the status alone");
        assert_eq!(models_on_text(1), tr("1 model on"));
        assert!(answered(&mut state, &Ok(Response::Ok)));
        assert!(listed_model(&state, "deepseek/deepseek-chat-v3.1").enabled, "taken: it stays on");
        // Switching to what it is, or a model the page does not list, sends nothing.
        let again = update(&mut state, Message::SetModel("deepseek/deepseek-chat-v3.1".into(), true), &data);
        assert!(matches!(again, Action::None));
        assert!(matches!(update(&mut state, Message::SetModel("nobody/none".into(), true), &data), Action::None));
    }

    /// A model that can neither use tools nor decide stands without a switch, and pressing one anyway
    /// sends nothing; a decision model has one and says it decides (#231).
    #[test]
    fn a_model_without_tools_has_no_switch() {
        let (mut state, data) = open_ai();
        let gemma = listed_model(&state, "google/gemma-3-27b-it");
        assert!(!has_switch(gemma));
        assert!(model_detail(gemma, tr("Cloud")).starts_with(tr("Can't use tools or decide")));
        assert!(has_switch(&AiModel::named("unknown")), "a server that does not tell keeps the switch");
        let nimble = AiModel { tools: Some(false), decision: Some(true), ..AiModel::named("nimble") };
        assert!(has_switch(&nimble), "sorting mail uses it");
        let place = tr("On this computer");
        assert_eq!(model_detail(&nimble, place), format!("{} · {place}", tr("Decides")));
        let deciding = listed_model(&state, "nimble");
        assert!(has_switch(deciding) && !deciding.enabled, "the demo's decision model has a switch");
        let action = update(&mut state, Message::SetModel("google/gemma-3-27b-it".into(), true), &data);
        assert!(matches!(action, Action::None));
        assert!(!listed_model(&state, "google/gemma-3-27b-it").enabled);
    }

    /// Where each model computes, in the words of the data rule.
    #[test]
    fn every_model_says_where_it_computes() {
        let local = AiModel { remote: Some(false), ..AiModel::named("qwen3:8b") };
        let cloud = AiModel { remote: Some(true), ..AiModel::named("gpt-oss:120b-cloud") };
        let ollama = Some(AiProvider::Ollama);
        assert_eq!(model_place(ollama, "http://localhost:11434/v1", &local), tr("On this computer"));
        assert_eq!(model_place(ollama, "http://localhost:11434/v1", &cloud), tr("Ollama Cloud"));
        let lm_studio = Some(AiProvider::LmStudio);
        assert_eq!(model_place(lm_studio, "http://localhost:1234/v1", &AiModel::named("x")), tr("On this computer"));
        let claude = AiModel::named("claude-sonnet-4-5");
        assert_eq!(
            model_place(Some(AiProvider::Anthropic), "https://api.anthropic.com/v1", &claude),
            tr("Cloud (Anthropic)")
        );
        let any = AiModel::named("m");
        assert_eq!(model_place(Some(AiProvider::OpenRouter), "https://openrouter.ai/api/v1", &any), tr("Cloud"));
        let compatible = Some(AiProvider::OpenAiCompatible);
        assert_eq!(model_place(compatible, "http://localhost:8000/v1", &any), tr("On this computer"));
        assert_eq!(model_place(compatible, "https://llm.example.org/v1", &any), tr("Cloud"));
        assert_eq!(model_place(None, "http://localhost:11434/v1", &local), tr("Cloud"), "unknown: elsewhere");
    }

    /// Every AI account's page searches name and id; only OpenRouter's hides closed models until
    /// asked.
    #[test]
    fn openrouter_searches_and_hides_closed_models() {
        let models = super::super::launch::demo_models();
        let ids = |shown: Vec<&AiModel>| shown.into_iter().map(|model| model.id.clone()).collect::<Vec<_>>();
        let open = ids(shown_models(&models, true, "", false));
        assert_eq!(open.len(), models.len() - 2, "the two closed ones hidden");
        assert!(!open.iter().any(|id| id == "openai/gpt-5"));
        assert_eq!(ids(shown_models(&models, true, "", true)).len(), models.len());
        assert_eq!(ids(shown_models(&models, true, " LLAMA ", false)), ["meta-llama/llama-3.3-70b-instruct"]);
        assert_eq!(ids(shown_models(&models, true, "gpt", false)), Vec::<String>::new(), "closed: not shown");
        assert_eq!(ids(shown_models(&models, true, "gpt", true)), ["openai/gpt-5"]);
        assert_eq!(ids(shown_models(&models, true, "Mistral Small", false)).len(), 1, "by name");
        assert_eq!(ids(shown_models(&models, false, "llama", false)), ["meta-llama/llama-3.3-70b-instruct"]);
        assert_eq!(ids(shown_models(&models, false, "", false)).len(), models.len(), "closed ones elsewhere too");
        // The page keeps both, and a new page starts without them.
        let (mut state, data) = open_ai();
        update(&mut state, Message::ModelSearch("qwen".into()), &data);
        update(&mut state, Message::ShowClosed(true), &data);
        assert_eq!((state.models_page.search.as_str(), state.models_page.show_closed), ("qwen", true));
        draw(&state, &data);
        update(&mut state, Message::ModelSearch("nothing like it".into()), &data);
        draw(&state, &data);
        update(&mut state, Message::Back, &data);
        assert!(state.models_page.search.is_empty() && !state.models_page.show_closed);
        assert!(state.models.contains_key(OPENROUTER), "the list's row still counts them");
    }

    /// The models stand by name in any case, those with the same name by id, whatever order the
    /// server sent them in; searching keeps that order.
    #[test]
    fn models_stand_in_alphabetical_order() {
        let named = |id: &str, name: &str| AiModel { name: name.into(), ..AiModel::named(id) };
        let models = vec![
            named("z/qwen", "Qwen3 Coder"),
            named("b/beta", "beta"),
            named("a/alpha", "Alpha"),
            named("y/same", "Same"),
            named("x/same", "same"),
        ];
        let ids = |shown: Vec<&AiModel>| shown.into_iter().map(|model| model.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(shown_models(&models, false, "", false)), ["a/alpha", "b/beta", "z/qwen", "x/same", "y/same"]);
        assert_eq!(ids(shown_models(&models, true, "", true)), ["a/alpha", "b/beta", "z/qwen", "x/same", "y/same"]);
        assert_eq!(ids(shown_models(&models, false, "SAME", false)), ["x/same", "y/same"]);
        assert_eq!(ids(shown_models(&models, false, "z/q", false)), ["z/qwen"], "by id");
    }

    /// A refused switch turns back and says why under the group; the next switch clears it.
    #[test]
    fn a_refused_switch_turns_back() {
        let (mut state, data) = open_ai();
        update(&mut state, Message::SetModel("qwen/qwen3-235b-a22b".into(), false), &data);
        update(&mut state, Message::SetModel("deepseek/deepseek-chat-v3.1".into(), true), &data);
        assert!(!listed_model(&state, "qwen/qwen3-235b-a22b").enabled);
        let refused = Response::Error { code: "forbidden".into(), message: "Only a person turns this on.".into() };
        assert!(answered(&mut state, &Err("The server cannot be reached.".into())), "the oldest switch's answer");
        assert!(listed_model(&state, "qwen/qwen3-235b-a22b").enabled, "turned back on");
        assert!(listed_model(&state, "deepseek/deepseek-chat-v3.1").enabled, "the other one waits");
        assert_eq!(state.models_page.refused.as_deref(), Some("The server cannot be reached."));
        draw(&state, &data);
        assert!(answered(&mut state, &Ok(refused)));
        assert!(!listed_model(&state, "deepseek/deepseek-chat-v3.1").enabled);
        assert_eq!(state.models_page.refused.as_deref(), Some("Only a person turns this on."));
        assert!(!answered(&mut state, &Ok(Response::Ok)), "nothing waits any more");
        update(&mut state, Message::SetModel("llama".into(), true), &data);
        assert!(state.models_page.refused.is_some(), "a model it does not list changes nothing");
        update(&mut state, Message::SetModel("meta-llama/llama-3.3-70b-instruct".into(), true), &data);
        assert!(state.models_page.refused.is_none(), "a new switch, a new answer");
    }

    /// The server's refusal to list is a sentence in the group with "Retry", which asks again.
    #[test]
    fn a_list_that_fails_says_why_and_can_be_retried() {
        let data = super::super::launch::demo_data();
        let mut state = State::default();
        update(&mut state, Message::Open(OPENROUTER.into()), &data);
        let refused = Response::Error { code: "auth".into(), message: "The server refused the key.".into() };
        assert!(answered(&mut state, &Ok(refused)));
        assert_eq!(state.models_page.error.as_deref(), Some("The server refused the key."));
        assert!(!state.models_page.loading);
        draw(&state, &data);
        assert!(!answered(&mut state, &Err("later".into())), "it waits for nothing any more");
        let again = sent(update(&mut state, Message::LoadModels, &data));
        assert!(matches!(again, Request::AiModels { all: true, .. }) && state.models_page.error.is_none());
        let answer = super::super::launch::demo_answer(&again, &data).expect("the demo's models");
        assert!(answered(&mut state, &Ok(answer)));
        assert!(state.models.contains_key(OPENROUTER) && state.models_page.error.is_none());
    }

    // ----- The inbox and ntfy (task E5.14) -------------------------------------------------------

    const NTFY: &str = "ntfy:push.example.org";

    /// The demo's data with the page of `id` open.
    fn open_demo(id: &str) -> (State, Data) {
        let data = super::super::launch::demo_data();
        let mut state = State::default();
        update(&mut state, Message::Open(id.into()), &data);
        assert!(selected(&state, &data).is_some(), "{id} is a demo account");
        (state, data)
    }

    /// Task S.21: a GitHub or Nextcloud account's page no longer switches its notifications; it
    /// says that the Notification Hub does and opens that window. Every other kind of account has
    /// no such group at all.
    #[test]
    fn github_and_nextcloud_point_at_the_notification_hub() {
        let data = super::super::launch::demo_data();
        let of = |id: &str| {
            let account = data.accounts.iter().find(|account| account.id == id).expect("demo account");
            sources_moved(account).is_some()
        };
        assert!(of("github:anna-dev"));
        assert!(of("nextcloud:anna@cloud.example.org"));
        for other in [NTFY, MAILBOX, "caldav:anna@posteo.de@posteo.de", "ai:openrouter@openrouter.ai"] {
            assert!(!of(other), "{other}");
        }
        let (mut state, data) = open_demo("nextcloud:anna@cloud.example.org");
        let request = sent(update(&mut state, Message::OpenApp("torro-inbox"), &data));
        assert!(matches!(request, Request::OpenApp { ref app, target: None } if app == "torro-inbox"), "{request:?}");
    }

    #[test]
    fn a_quiet_source_says_why_in_calm_words() {
        assert_eq!(
            health_text(SourceKind::Github, SourceHealth::MissingScope),
            tr("Notifications need a classic token with notifications or repo, or the GitHub CLI sign-in.")
        );
        assert_eq!(health_text(SourceKind::Talk, SourceHealth::MissingApp), tr("This Nextcloud has no Talk."));
        assert_ne!(
            health_text(SourceKind::Nextcloud, SourceHealth::MissingApp),
            health_text(SourceKind::Talk, SourceHealth::MissingApp)
        );
        // Every page builds with every reason.
        let reasons =
            [SourceHealth::MissingApp, SourceHealth::MissingScope, SourceHealth::SignedOut, SourceHealth::Offline];
        for health in reasons {
            for id in ["github:anna-dev", "nextcloud:anna@cloud.example.org", NTFY] {
                let (state, mut data) = open_demo(id);
                for info in &mut data.sources {
                    info.health = Some(health);
                }
                draw(&state, &data);
            }
        }
    }

    #[test]
    fn the_ntfy_row_counts_its_topics_and_the_page_has_no_sync() {
        let data = super::super::launch::demo_data();
        let ntfy = data.accounts.iter().find(|account| account.id == NTFY).expect("demo ntfy");
        assert_eq!(provider_name("ntfy"), "ntfy");
        assert_eq!(topics_text(3), trf("{n} topics", &[("n", &3)]));
        assert_eq!(topics_text(1), tr("1 topic"));
        let (state, data) = open_demo(NTFY);
        draw(&state, &data);
        drop(account_row(ntfy, None, Utc::now()));
    }

    #[test]
    fn a_new_topic_is_checked_after_the_first_press_and_sent_with_its_label() {
        let (mut state, data) = open_demo(NTFY);
        update(&mut state, Message::AddTopic, &data);
        assert!(state.topic.as_ref().is_some_and(|edit| !edit.tried && edit.problem().is_some()));
        assert!(matches!(update(&mut state, Message::SaveTopic, &data), Action::None), "nothing typed");
        assert!(state.topic.as_ref().is_some_and(|edit| edit.tried));
        update(&mut state, Message::TopicNameEdited(TopicName::new("alerts-77ab")), &data);
        update(&mut state, Message::TopicLabelEdited(" Alerts ".into()), &data);
        let request = sent(update(&mut state, Message::SaveTopic, &data));
        let Request::SetNtfyTopic { account, topic, label, subscribed } = request else { panic!("SetNtfyTopic") };
        assert_eq!(
            (account.as_str(), topic.as_str(), label.as_deref(), subscribed),
            (NTFY, "alerts-77ab", Some("Alerts"), true)
        );
        // The dialog waits for the answer: a refusal stays in it, `ok` closes it.
        let refused = tr("The ntfy server refused: the topics need a login, or this login may not read them.");
        assert!(answered(&mut state, &Err(refused.to_string())));
        assert_eq!(state.topic.as_ref().and_then(|edit| edit.error.as_deref()), Some(refused));
        update(&mut state, Message::SaveTopic, &data);
        assert!(answered(&mut state, &Ok(Response::Ok)));
        assert!(state.topic.is_none());
        // Its name never shows in `Debug`.
        update(&mut state, Message::AddTopic, &data);
        update(&mut state, Message::TopicNameEdited(TopicName::new("alerts-77ab")), &data);
        assert!(!format!("{state:?}").contains("alerts-77ab"));
    }

    #[test]
    fn a_topic_is_renamed_or_unsubscribed_after_asking() {
        let (mut state, data) = open_demo(NTFY);
        update(&mut state, Message::EditTopic(0), &data);
        update(&mut state, Message::TopicNameEdited(TopicName::new("other")), &data);
        assert_eq!(state.topic.as_ref().map(|edit| edit.name.as_str()), Some("backups-7f3a"), "the name is fixed");
        // The label as it was: nothing to send, the dialog closes.
        assert!(matches!(update(&mut state, Message::SaveTopic, &data), Action::None));
        assert!(state.topic.is_none());
        update(&mut state, Message::EditTopic(0), &data);
        update(&mut state, Message::TopicLabelEdited("Nightly backups".into()), &data);
        let request = sent(update(&mut state, Message::SaveTopic, &data));
        assert!(matches!(
            request,
            Request::SetNtfyTopic { ref label, subscribed: true, .. } if label.as_deref() == Some("Nightly backups")
        ));
        answered(&mut state, &Ok(Response::Ok));

        update(&mut state, Message::EditTopic(1), &data);
        assert!(matches!(update(&mut state, Message::Unsubscribe, &data), Action::None), "only after asking");
        update(&mut state, Message::AskUnsubscribe, &data);
        assert!(matches!(escape(&state), Some(Message::CancelUnsubscribe)));
        drop(overlay(&state, &data));
        let request = sent(update(&mut state, Message::Unsubscribe, &data));
        let Request::SetNtfyTopic { topic, label, subscribed, .. } = request else { panic!("SetNtfyTopic") };
        assert_eq!((topic.as_str(), label, subscribed), ("ci-anna-91c2", None, false));
        assert!(state.topic.is_none());
    }

    #[test]
    fn change_login_sends_the_secret_once_for_the_same_server() {
        let (mut state, data) = open_demo(NTFY);
        update(&mut state, Message::ChangeLogin, &data);
        let form = &state.login.as_ref().expect("the dialog").login;
        assert_eq!((form.token, form.user.as_str()), (false, "anna"), "filled with how it signs in");
        assert!(matches!(update(&mut state, Message::SubmitLogin, &data), Action::None), "no secret yet");
        update(&mut state, Message::LoginSecretEdited(Secret::new("n3w-pass")), &data);
        assert!(!format!("{state:?}").contains("n3w-pass"), "Debug shows no secret");
        drop(overlay(&state, &data));
        let request = sent(update(&mut state, Message::SubmitLogin, &data));
        let Request::SetNtfyLogin { server, topics, user, secret } = request else { panic!("SetNtfyLogin") };
        assert_eq!((server.as_str(), topics.len(), user.as_deref()), ("https://push.example.org", 0, Some("anna")));
        assert_eq!(secret.expose(), "n3w-pass");
        assert!(state.login.as_ref().is_some_and(|dialog| dialog.login.secret.is_blank()), "no copy stays");
        assert!(answered(&mut state, &Err("refused".into())));
        assert_eq!(state.login.as_ref().and_then(|dialog| dialog.error.as_deref()), Some("refused"));
        update(&mut state, Message::LoginKind(true), &data);
        update(&mut state, Message::LoginSecretEdited(Secret::new("tk_x")), &data);
        let Request::SetNtfyLogin { user, .. } = sent(update(&mut state, Message::SubmitLogin, &data)) else {
            panic!("SetNtfyLogin")
        };
        assert_eq!(user, None, "a token goes without a user name");
        let account = data.accounts.iter().find(|account| account.id == NTFY).cloned().expect("demo ntfy");
        assert!(answered(&mut state, &Ok(Response::Account { account })));
        assert!(state.login.is_none());
    }

    // ----- The slots (issue #325, task MW.8) -----------------------------------------------------

    use super::super::launch::{DEMO_OLLAMA, DEMO_OPENROUTER, demo_answer, demo_data, demo_ollama, demo_ollama_models};

    /// The requests of an [`Action::AskAi`], with what each is for.
    fn asks(action: Action) -> Vec<(Asked, Request)> {
        match action {
            Action::AskAi(asks) => asks,
            Action::None => Vec::new(),
            other => panic!("expected requests of the group Models, got {other:?}"),
        }
    }

    /// The one request of an [`Action::AskAi`].
    fn ask(action: Action) -> (Asked, Request) {
        let mut all = asks(action);
        assert_eq!(all.len(), 1, "one request: {all:?}");
        all.remove(0)
    }

    fn pull(id: &str, state: AiPullState) -> AiPull {
        AiPull {
            id: id.into(),
            account: DEMO_OLLAMA.into(),
            model: "qwen3:14b".into(),
            state,
            completed: Some(400),
            total: Some(1000),
            error: None,
        }
    }

    /// The demo with its Ollama, the section shown and every answer of the demo taken: the suggestion,
    /// the downloads (with `pulls` the demo's running one) and the models of both accounts.
    fn shown(pulls: bool) -> (State, Data) {
        let mut data = demo_data();
        demo_ollama(&mut data);
        let mut state = State::default();
        for (asked, request) in asks(ai_shown(&mut state, &data)) {
            let answer = match (&request, pulls) {
                (Request::AiPulls, false) => Response::AiPulls { pulls: Vec::new() },
                _ => demo_answer(&request, &data).expect("the demo answers"),
            };
            assert!(matches!(ai_answered(&mut state, asked, &Ok(answer)), Action::None));
        }
        (state, data)
    }

    /// Opening the section asks for the suggestion, the downloads and the models of every AI account
    /// once; a reload asks again only for an account that came with it, and leaving forgets it.
    #[test]
    fn the_section_asks_once_for_what_the_slots_need() {
        let mut data = demo_data();
        let mut state = State::default();
        let first: Vec<Asked> = asks(ai_shown(&mut state, &data)).into_iter().map(|(asked, _)| asked).collect();
        assert_eq!(first, [Asked::Recommend, Asked::Pulls, Asked::Models(OPENROUTER.into())]);
        assert!(state.slots.recommending);
        assert!(asks(ai_shown(&mut state, &data)).is_empty(), "a reload asks nothing again");
        demo_ollama(&mut data);
        let new = ask(ai_shown(&mut state, &data));
        assert_eq!(
            new,
            (Asked::Models(DEMO_OLLAMA.into()), Request::AiModels { account: DEMO_OLLAMA.into(), all: true })
        );
        leave(&mut state);
        // Opened again while the first answers are out: only the models, whose lists never came.
        let again: Vec<Asked> = asks(ai_shown(&mut state, &data)).into_iter().map(|(asked, _)| asked).collect();
        assert_eq!(again, [Asked::Models(OPENROUTER.into()), Asked::Models(DEMO_OLLAMA.into())]);
        let _ = ai_answered(&mut state, Asked::Recommend, &Err("No answer".into()));
        let _ = ai_answered(&mut state, Asked::Pulls, &Err("No answer".into()));
        leave(&mut state);
        let third: Vec<Asked> = asks(ai_shown(&mut state, &data)).into_iter().map(|(asked, _)| asked).collect();
        assert!(third.starts_with(&[Asked::Recommend, Asked::Pulls]), "{third:?}");
        let none = Data { accounts: Vec::new(), ..demo_data() };
        assert!(asks(ai_shown(&mut State::default(), &none)).is_empty(), "no AI account, no group");
    }

    /// "Use suggestion" with a download: `ai.pull_model`, `ai.list_pulls` every second while it runs,
    /// then the account's models, the model turned on and put in the slot – in this order – and a fresh
    /// suggestion after it.
    #[test]
    fn use_suggestion_downloads_then_turns_on_and_sets() {
        let (mut state, data) = shown(false);
        let (asked, request) = ask(update(&mut state, Message::UseSuggestion(AiSlot::Chat), &data));
        assert_eq!(asked, Asked::Slot(AiSlot::Chat));
        assert_eq!(request, Request::PullAiModel { account: DEMO_OLLAMA.into(), model: "qwen3:14b".into() });
        assert!(asks(update(&mut state, Message::UseSuggestion(AiSlot::Decide), &data)).is_empty(), "one at a time");
        // The start answer lists every download of the run: the newest of this model is the one.
        let older = pull("p0", AiPullState::Failed);
        let started = Response::AiPulls { pulls: vec![older, pull("p1", AiPullState::Running)] };
        assert!(matches!(ai_answered(&mut state, asked, &Ok(started)), Action::None));
        assert_eq!(state.slots.using.as_ref().map(|using| using.step.clone()), Some(UseStep::Pull(Some("p1".into()))));
        assert!(running(&state.slots.pulls), "the page ticks");
        assert_eq!(ask(update(&mut state, Message::PollPulls, &data)), (Asked::Pulls, Request::AiPulls));
        assert!(asks(update(&mut state, Message::PollPulls, &data)).is_empty(), "the last poll is still out");
        let still = Response::AiPulls { pulls: vec![pull("p1", AiPullState::Running)] };
        assert!(matches!(ai_answered(&mut state, Asked::Pulls, &Ok(still)), Action::None));
        draw_ai(&state, &data);

        let done = Response::AiPulls { pulls: vec![pull("p1", AiPullState::Done)] };
        let mut order = vec![ask(ai_answered(&mut state, Asked::Pulls, &Ok(done)))];
        let mut models = demo_ollama_models();
        models.push(AiModel { tools: Some(true), remote: Some(false), ..AiModel::named("qwen3:14b") });
        let listed = Response::AiModels { account: DEMO_OLLAMA.into(), models };
        order.push(ask(ai_answered(&mut state, Asked::Slot(AiSlot::Chat), &Ok(listed))));
        draw_ai(&state, &data);
        order.push(ask(ai_answered(&mut state, Asked::Slot(AiSlot::Chat), &Ok(Response::Ok))));
        let set = Response::AiAssignments { assignments: Vec::new(), slots: Vec::new() };
        order.push(ask(ai_answered(&mut state, Asked::Slot(AiSlot::Chat), &Ok(set))));
        let chat = Asked::Slot(AiSlot::Chat);
        assert_eq!(
            order,
            [
                (chat.clone(), Request::AiModels { account: DEMO_OLLAMA.into(), all: true }),
                (
                    chat.clone(),
                    Request::SetModelEnabled { account: DEMO_OLLAMA.into(), model: "qwen3:14b".into(), enabled: true }
                ),
                (
                    chat,
                    Request::SetAiSlot {
                        slot: AiSlot::Chat,
                        engine: Some(DEMO_OLLAMA.into()),
                        model: Some("qwen3:14b".into()),
                        effort: Some(Effort::High),
                    }
                ),
                (Asked::Recommend, Request::AiRecommend),
            ]
        );
        assert!(state.slots.using.is_none() && state.slots.failed.is_none());
        assert!(state.models[DEMO_OLLAMA].iter().any(|model| model.id == "qwen3:14b" && model.enabled));
    }

    /// A model the account lists already and turned on goes straight to `ai.set_slot`, by the id the
    /// account lists (`nimble:latest` for `nimble`).
    #[test]
    fn a_listed_model_is_set_by_its_listed_id() {
        let (mut state, data) = shown(false);
        let (asked, request) = ask(update(&mut state, Message::UseSuggestion(AiSlot::Decide), &data));
        assert_eq!(request, Request::AiModels { account: DEMO_OLLAMA.into(), all: true });
        let mut models = demo_ollama_models();
        models.iter_mut().for_each(|model| model.enabled = true);
        let listed = Response::AiModels { account: DEMO_OLLAMA.into(), models };
        let (_, request) = ask(ai_answered(&mut state, asked, &Ok(listed)));
        assert_eq!(
            request,
            Request::SetAiSlot {
                slot: AiSlot::Decide,
                engine: Some(DEMO_OLLAMA.into()),
                model: Some("nimble:latest".into()),
                effort: None
            }
        );
        assert!(same_model("nimble", "nimble:latest") && !same_model("qwen3", "qwen3:14b"));
        assert!(!same_model("host:5000/x", "host:5000/x:5000"), "a port is no tag");
    }

    /// A step that fails stops the chain: nothing more is sent, the reason stands under its slot, a late
    /// answer is dropped, and "Retry" starts the chain again from its first step.
    #[test]
    fn a_failed_step_stops_the_chain() {
        let (mut state, data) = shown(false);
        let (asked, _) = ask(update(&mut state, Message::UseSuggestion(AiSlot::Decide), &data));
        let listed = Response::AiModels { account: DEMO_OLLAMA.into(), models: demo_ollama_models() };
        let (asked, request) = ask(ai_answered(&mut state, asked, &Ok(listed)));
        assert!(matches!(request, Request::SetModelEnabled { ref model, .. } if model == "nimble:latest"));
        let refused = Response::Error { code: "forbidden".into(), message: "Only you can do that.".into() };
        assert!(matches!(ai_answered(&mut state, asked.clone(), &Ok(refused)), Action::None));
        assert!(state.slots.using.is_none());
        let failure = state.slots.failed.as_ref().expect("the reason shows");
        assert_eq!((failure.slot, failure.message.as_str()), (AiSlot::Decide, "Only you can do that."));
        draw_ai(&state, &data);
        assert!(matches!(ai_answered(&mut state, asked, &Ok(Response::Ok)), Action::None), "a late answer is dropped");
        let (_, again) = ask(update(&mut state, Message::RetrySlot, &data));
        assert_eq!(again, Request::AiModels { account: DEMO_OLLAMA.into(), all: true });
        assert!(state.slots.failed.is_none());

        // A download the core refuses at once comes back `failed`, with the core's own sentence.
        let (mut state, data) = shown(false);
        let (asked, _) = ask(update(&mut state, Message::UseSuggestion(AiSlot::Chat), &data));
        let failed = AiPull { error: Some("Ollama does not answer.".into()), ..pull("p1", AiPullState::Failed) };
        assert!(matches!(ai_answered(&mut state, asked, &Ok(Response::AiPulls { pulls: vec![failed] })), Action::None));
        assert_eq!(state.slots.failed.as_ref().map(|f| f.message.as_str()), Some("Ollama does not answer."));
        // No answer at all stops it too.
        let (mut state, data) = shown(false);
        let (asked, _) = ask(update(&mut state, Message::UseSuggestion(AiSlot::Chat), &data));
        assert!(matches!(ai_answered(&mut state, asked, &Err("No home directory".into())), Action::None));
        assert!(state.slots.using.is_none() && state.slots.failed.is_some());
    }

    /// "Use suggestion" while the model downloads already waits for that download; "Cancel" ends it
    /// quietly and the page stops ticking.
    #[test]
    fn cancel_ends_the_download_quietly() {
        let (mut state, data) = shown(true);
        assert!(running(&state.slots.pulls), "the demo's download runs");
        draw_ai(&state, &data);
        assert!(asks(update(&mut state, Message::UseSuggestion(AiSlot::Chat), &data)).is_empty(), "it waits");
        assert_eq!(state.slots.using.as_ref().map(|using| using.step.clone()), Some(UseStep::Pull(Some("p1".into()))));
        let (asked, request) = ask(update(&mut state, Message::CancelPull(AiSlot::Chat, "p1".into()), &data));
        let cancel = Asked::Cancel(AiSlot::Chat, "p1".into());
        assert_eq!((asked.clone(), request), (cancel, Request::CancelAiPull { id: "p1".into() }));
        let cancelled = Response::AiPulls { pulls: vec![pull("p1", AiPullState::Cancelled)] };
        assert!(matches!(ai_answered(&mut state, asked, &Ok(cancelled)), Action::None));
        assert!(state.slots.using.is_none() && state.slots.failed.is_none() && !running(&state.slots.pulls));
        assert!(asks(update(&mut state, Message::PollPulls, &data)).is_empty(), "nothing runs: no poll");
        // A cancel the core refuses keeps the chain and says why.
        let (mut state, data) = shown(true);
        let _ = update(&mut state, Message::UseSuggestion(AiSlot::Chat), &data);
        let refused = Response::Error { code: "not-found".into(), message: "No such download.".into() };
        let _ = ai_answered(&mut state, Asked::Cancel(AiSlot::Chat, "p1".into()), &Ok(refused));
        assert!(state.slots.using.is_some());
        assert_eq!(state.slots.failed.as_ref().map(|f| f.message.as_str()), Some("No such download."));
    }

    /// A choice in a slot's list sends `ai.set_slot` at once; "None" empties it; what the slot has
    /// already sends nothing.
    #[test]
    fn a_choice_in_the_list_sets_the_slot() {
        let (mut state, data) = shown(false);
        let (choices, chosen) = slot_choices(&state, &data, AiSlot::Decide);
        assert_eq!(chosen.model.as_deref(), Some("qwen/qwen3-235b-a22b"));
        assert!(asks(update(&mut state, Message::PickSlot(AiSlot::Decide, chosen), &data)).is_empty());
        let none = choices[0].clone();
        assert_eq!(none.label, tr("None"));
        let (asked, request) = ask(update(&mut state, Message::PickSlot(AiSlot::Decide, none), &data));
        assert_eq!(asked, Asked::Slot(AiSlot::Decide));
        assert_eq!(request, Request::SetAiSlot { slot: AiSlot::Decide, engine: None, model: None, effort: None });
        assert!(asked.changes() && !Asked::Pulls.changes());
    }

    /// Chat offers models turned on that can use tools; Sorting those that decide or can use tools, each
    /// with how it sorts, and the account's name with more than one AI account. A chosen model the lists
    /// do not hold stands after "None".
    #[test]
    fn the_lists_offer_what_serves_the_slot() {
        let (state, mut data) = shown(false);
        let (chat, chosen) = slot_choices(&state, &data, AiSlot::Chat);
        assert_eq!(chosen.model.as_deref(), Some("qwen/qwen3-32b"), "not listed, still shown");
        assert_eq!(chat[1], chosen);
        let models: Vec<&str> = chat.iter().filter_map(|choice| choice.model.as_deref()).collect();
        assert!(models.contains(&"qwen3:8b") && models.contains(&"qwen/qwen3-235b-a22b"));
        assert!(!models.contains(&"nimble:latest") && !models.contains(&"deepseek/deepseek-chat-v3.1"), "{models:?}");
        assert!(chat.iter().any(|choice| choice.label == format!("qwen3:8b · {}", "Ollama")));
        let (decide, _) = slot_choices(&state, &data, AiSlot::Decide);
        let qwen =
            decide.iter().find(|choice| choice.model.as_deref() == Some("qwen/qwen3-235b-a22b")).expect("listed");
        assert_eq!(qwen.label, "Qwen3 235B A22B · OpenRouter", "how it sorts stands under the slot's name");
        assert!(!decide.iter().any(|choice| choice.model.as_deref() == Some("nimble:latest")), "nimble is off");
        data.accounts.retain(|account| account.id != DEMO_OLLAMA);
        let (one, _) = slot_choices(&state, &data, AiSlot::Chat);
        assert!(one.iter().any(|choice| choice.label == "Qwen3 235B A22B"), "one account: no account name");
    }

    /// The line under a slot's name: a model the account lists off (or lacks) says so with the alert,
    /// Sorting through a chat model says how it sorts, and a choice on its way "Saving…".
    #[test]
    fn the_slot_says_what_its_model_does() {
        let (mut state, mut data) = shown(false);
        let off = (tr("Turned off in Settings › AI").to_string(), true);
        assert_eq!(slot_detail(&state, &data, AiSlot::Chat), off, "qwen3-32b is not on");
        let sorting = format!("{} · {}", tr("Sorts mail into categories"), tr("Answers in a fixed format, slower"));
        assert_eq!(slot_detail(&state, &data, AiSlot::Decide), (sorting, false));
        // A model turned off but listed shows by its name.
        if let Some(chat) = data.settings.ai_slots.chat.as_mut() {
            chat.model = Some("deepseek/deepseek-chat-v3.1".into());
        }
        assert_eq!(slot_detail(&state, &data, AiSlot::Chat), off);
        assert_eq!(slot_choices(&state, &data, AiSlot::Chat).1.label, "DeepSeek V3.1 · OpenRouter");
        // A model turned on says what the slot is for; before the account's models are in, too.
        if let Some(chat) = data.settings.ai_slots.chat.as_mut() {
            chat.model = Some("qwen/qwen3-235b-a22b".into());
        }
        let chat = (tr("The assistant and its bots").to_string(), false);
        assert_eq!(slot_detail(&state, &data, AiSlot::Chat), chat);
        state.models.remove(OPENROUTER);
        assert_eq!(slot_detail(&state, &data, AiSlot::Chat), chat, "not known yet: no alert");
        let none = slot_choices(&state, &data, AiSlot::Chat).0[0].clone();
        let _ = update(&mut state, Message::PickSlot(AiSlot::Chat, none), &data);
        assert_eq!(slot_detail(&state, &data, AiSlot::Chat), (tr("Saving…").to_string(), false));
        draw_ai(&state, &data);
    }

    /// Sorting's place in the data rule and under "Models", and why "Use suggestion" waits.
    #[test]
    fn the_rows_name_where_mail_goes_and_what_waits() {
        let (mut state, mut data) = shown(true);
        assert_eq!(sorting_place(&data, &state.models), Some(("OpenRouter", tr("Cloud · US"))));
        let _ = update(&mut state, Message::UseSuggestion(AiSlot::Chat), &data);
        draw_ai(&state, &data);
        let using = state.slots.using.as_ref().expect("the chat's download runs");
        assert_eq!(waits_for(using), trf("Wait until the download for {slot} has finished", &[("slot", &tr("Chat"))]));
        let saving = Using { step: UseStep::Set, ..using.clone() };
        assert_eq!(waits_for(&saving), trf("Wait until {slot} is saved", &[("slot", &tr("Chat"))]));
        data.settings.decide_mail_to_cloud = Some(false);
        assert_eq!(sorting_place(&data, &state.models), None, "mail stays here");
    }

    /// The computer in one line, the share of a download, and the group in every state.
    #[test]
    fn the_group_says_the_computer_and_the_download() {
        const GIB: u64 = 1024 * 1024 * 1024;
        let gpu = Hardware {
            cpu: Some("AMD Ryzen 9 7950X".into()),
            memory_bytes: Some(64 * GIB),
            gpu: Some(torro_core::ai::hardware::Gpu {
                name: Some("GeForce RTX 4070".into()),
                vram_bytes: Some(12 * GIB),
            }),
            unified_memory: false,
        };
        let line = hardware_line(&gpu);
        let (vram, memory) = (
            trf("{size} video memory", &[("size", &torro_gui::units::format_bytes(12 * GIB))]),
            trf("{size} memory", &[("size", &torro_gui::units::format_bytes(64 * GIB))]),
        );
        assert_eq!(line, format!("GeForce RTX 4070 · {vram} · {memory}"));
        let mac = Hardware {
            cpu: Some("Apple M3 Max".into()),
            memory_bytes: Some(36 * GIB),
            gpu: None,
            unified_memory: true,
        };
        assert!(hardware_line(&mac).starts_with("Apple M3 Max · "));
        assert_eq!(hardware_line(&Hardware::default()), tr("Not measured"));

        assert_eq!(download_share(&pull("p1", AiPullState::Running)), Some((400, "40%".into())));
        let grown = AiPull { completed: Some(1200), ..pull("p1", AiPullState::Running) };
        assert_eq!(download_share(&grown).map(|(share, _)| share), Some(1000), "never past the whole");
        assert_eq!(download_share(&AiPull { total: None, ..grown }), None);

        let (mut state, data) = shown(true);
        draw_ai(&state, &data);
        state.slots.recommendation = None;
        state.slots.recommending = true;
        draw_ai(&state, &data);
        state.slots.recommending = false;
        state.slots.recommend_error = Some("No answer.".into());
        draw_ai(&state, &data);
        assert_eq!(ask(update(&mut state, Message::Recommend, &data)), (Asked::Recommend, Request::AiRecommend));
        let (mut base_state, base) = {
            let data = demo_data();
            let mut state = State::default();
            for (asked, request) in asks(ai_shown(&mut state, &data)) {
                let answer = demo_answer(&request, &data).expect("answered");
                let _ = ai_answered(&mut state, asked, &Ok(answer));
            }
            (state, data)
        };
        let decide = suggestion_of(&base_state, AiSlot::Decide).expect("Sorting waits for Ollama");
        assert!(decide.engine.is_none(), "no Ollama: the reason instead of the button");
        assert!(asks(update(&mut base_state, Message::UseSuggestion(AiSlot::Decide), &base)).is_empty());
        draw_ai(&base_state, &base);
    }

    /// An exception of sorting mail, `enabled` or not, at `engine` with `model`.
    fn with_exception(data: &mut Data, engine: &str, model: &str, enabled: bool) {
        let exception = torro_core::ai::AiAssignment {
            function: AiFunction::InboxCategorize,
            engine: Some(engine.into()),
            model: Some(model.into()),
            effort: None,
            enabled,
        };
        data.settings.ai_assignments.insert(AiFunction::InboxCategorize, exception);
    }

    /// Review of #325 (F2, R6): while an exception of sorting is stored, putting a model into Sorting – from
    /// the list or by "Use suggestion" – takes it back first, then `ai.set_slot`, as Settings ›
    /// Categorization does, so the slot sorts and meanwhile no exception the person stopped sorts again;
    /// also for the model the slot has already. A take-back that fails stops with its reason and sets no
    /// slot. The chat's slot never touches it.
    #[test]
    fn a_model_for_sorting_takes_a_stored_exception_back() {
        let (mut state, mut data) = shown(false);
        with_exception(&mut data, DEMO_OLLAMA, "nimble:latest", false);
        let (_, chosen) = slot_choices(&state, &data, AiSlot::Decide);
        let (asked, request) = ask(update(&mut state, Message::PickSlot(AiSlot::Decide, chosen.clone()), &data));
        assert_eq!(request, no_exception(), "held, but the exception sorts: it goes first");
        assert_eq!(state.slots.using.as_ref().map(|using| using.step.clone()), Some(UseStep::TakeBack));
        assert!(asked.changes(), "the shell reloads the settings after it");
        let set = || Ok(Response::AiAssignments { assignments: Vec::new(), slots: Vec::new() });
        let (asked, request) = ask(ai_answered(&mut state, asked, &set()));
        assert!(matches!(request, Request::SetAiSlot { slot: AiSlot::Decide, .. }), "{request:?}");
        assert_eq!(ask(ai_answered(&mut state, asked, &set())), (Asked::Recommend, Request::AiRecommend));
        assert!(state.slots.using.is_none() && state.slots.failed.is_none());

        // The take-back refused: the chain stops there, the slot is not set.
        let (mut state, _) = shown(false);
        let (asked, _) = ask(update(&mut state, Message::PickSlot(AiSlot::Decide, chosen), &data));
        let refused = Response::Error { code: "error".into(), message: "Settings cannot be written.".into() };
        assert!(matches!(ai_answered(&mut state, asked, &Ok(refused)), Action::None), "no ai.set_slot");
        assert_eq!(state.slots.failed.as_ref().map(|f| f.message.as_str()), Some("Settings cannot be written."));
        assert!(state.slots.using.is_none());

        // "Use suggestion" for Sorting: listed, on – then asked whether the exception still stands, taken
        // back, and only then the slot.
        let (mut state, _) = shown(false);
        with_exception(&mut data, DEMO_OPENROUTER, "qwen/qwen3-235b-a22b", true);
        let exception = data.settings.ai_assignments[&AiFunction::InboxCategorize].clone();
        let (asked, _) = ask(update(&mut state, Message::UseSuggestion(AiSlot::Decide), &data));
        let mut models = demo_ollama_models();
        models.iter_mut().for_each(|model| model.enabled = true);
        let listed = Response::AiModels { account: DEMO_OLLAMA.into(), models };
        let (asked, request) = ask(ai_answered(&mut state, asked, &Ok(listed.clone())));
        assert_eq!(request, Request::AiAssignments, "what stands now, not at the start");
        let (asked, request) = ask(ai_answered(&mut state, asked, &Ok(stored(Some(exception.clone())))));
        assert_eq!(request, no_exception());
        let (asked, request) = ask(ai_answered(&mut state, asked, &set()));
        assert!(matches!(request, Request::SetAiSlot { slot: AiSlot::Decide, .. }), "{request:?}");
        assert_eq!(ask(ai_answered(&mut state, asked, &set())), (Asked::Recommend, Request::AiRecommend));

        // Taken back already, or changed by the person while the chain ran (a long download): theirs, it
        // stays – straight to the slot.
        for now in [None, Some(torro_core::ai::AiAssignment { enabled: false, ..exception.clone() })] {
            let (mut state, _) = shown(false);
            let (asked, _) = ask(update(&mut state, Message::UseSuggestion(AiSlot::Decide), &data));
            let (asked, _) = ask(ai_answered(&mut state, asked, &Ok(listed.clone())));
            let (_, request) = ask(ai_answered(&mut state, asked, &Ok(stored(now))));
            assert!(matches!(request, Request::SetAiSlot { slot: AiSlot::Decide, .. }), "{request:?}");
        }

        // The chat's slot leaves the exception as it is.
        let (mut state, _) = shown(false);
        let none = slot_choices(&state, &data, AiSlot::Chat).0[0].clone();
        let (asked, request) = ask(update(&mut state, Message::PickSlot(AiSlot::Chat, none), &data));
        assert!(matches!(request, Request::SetAiSlot { slot: AiSlot::Chat, .. }), "{request:?}");
        assert_eq!(ask(ai_answered(&mut state, asked, &set())), (Asked::Recommend, Request::AiRecommend));
    }

    /// The answer of `ai.list_assignments` with `exception` stored for sorting, or none.
    fn stored(exception: Option<torro_core::ai::AiAssignment>) -> Response {
        let assignment = exception.unwrap_or_else(|| torro_core::ai::AiAssignment::none(AiFunction::InboxCategorize));
        let info = torro_ipc::AiAssignmentInfo { assignment, label: "Sorting mail".into(), needs_decision: true };
        Response::AiAssignments { assignments: vec![info], slots: Vec::new() }
    }

    /// Review of #325 (R5): "Use suggestion" whose start the core answers `busy` (another window or an
    /// agent downloads the model) joins that download by its id; with none running it stops with the
    /// core's sentence.
    #[test]
    fn use_suggestion_joins_a_download_that_runs_already() {
        let (mut state, data) = shown(false);
        let (asked, request) = ask(update(&mut state, Message::UseSuggestion(AiSlot::Chat), &data));
        assert!(matches!(request, Request::PullAiModel { .. }), "{request:?}");
        let busy = Response::Error { code: "busy".into(), message: "It downloads already.".into() };
        let (asked, request) = ask(ai_answered(&mut state, asked, &Ok(busy.clone())));
        assert_eq!((asked.clone(), request), (Asked::Slot(AiSlot::Chat), Request::AiPulls));
        assert!(state.slots.using.as_ref().is_some_and(|using| using.step.downloading()), "still the download");
        let other = AiPull { model: "gemma3:12b".into(), ..pull("p8", AiPullState::Running) };
        let runs = vec![pull("p5", AiPullState::Failed), pull("p7", AiPullState::Running), other];
        assert!(matches!(ai_answered(&mut state, asked, &Ok(Response::AiPulls { pulls: runs })), Action::None));
        assert_eq!(state.slots.using.as_ref().map(|using| using.step.clone()), Some(UseStep::Pull(Some("p7".into()))));
        assert!(state.slots.failed.is_none());
        draw_ai(&state, &data);
        let done = Response::AiPulls { pulls: vec![pull("p7", AiPullState::Done)] };
        let (_, request) = ask(ai_answered(&mut state, Asked::Pulls, &Ok(done)));
        assert_eq!(request, Request::AiModels { account: DEMO_OLLAMA.into(), all: true }, "followed to its end");

        // None of the model runs (one cancelled while its request is on the way holds it): the sentence.
        let (mut state, data) = shown(false);
        let (asked, _) = ask(update(&mut state, Message::UseSuggestion(AiSlot::Chat), &data));
        let (asked, _) = ask(ai_answered(&mut state, asked, &Ok(busy)));
        let cancelled = Response::AiPulls { pulls: vec![pull("p7", AiPullState::Cancelled)] };
        assert!(matches!(ai_answered(&mut state, asked, &Ok(cancelled)), Action::None));
        assert!(state.slots.using.is_none());
        assert_eq!(state.slots.failed.as_ref().map(|f| f.message.as_str()), Some("It downloads already."));
    }

    /// Review of #325 (F3): the data rule and "Models" say that sorting sends mail to the cloud only while
    /// the model that sorts now computes elsewhere – the exception before the slot, by where the model
    /// computes; with the permission on and a model here, "Keep mail here" stays without the sentence.
    #[test]
    fn mail_goes_to_the_cloud_only_with_a_model_elsewhere_in_effect() {
        let (state, mut data) = shown(false);
        let cloud = Some(("OpenRouter", tr("Cloud · US")));
        assert_eq!(sorting_place(&data, &state.models), cloud, "the demo sorts at OpenRouter");
        // The take-over of an old `mailToCloud` with nimble here: the permission is on, mail stays here.
        let nimble =
            torro_core::ai::SlotModel { engine: DEMO_OLLAMA.into(), model: Some("nimble:latest".into()), effort: None };
        data.settings.ai_slots.decide = Some(nimble.clone());
        assert_eq!(sorting_place(&data, &state.models), None, "nimble computes here");
        assert_eq!(sorting_place(&data, &BTreeMap::new()), None, "also before the models are in");
        drop(data_rule_group(&data, &state.models));
        // "Automatic" at the Ollama here stays here; a `:cloud` model named goes out.
        data.settings.ai_slots.decide = Some(torro_core::ai::SlotModel { model: None, ..nimble.clone() });
        assert_eq!(sorting_place(&data, &state.models), None, "Automatic here");
        let named_cloud = torro_core::ai::SlotModel { model: Some("glm-5.3:cloud".into()), ..nimble.clone() };
        data.settings.ai_slots.decide = Some(named_cloud);
        assert_eq!(sorting_place(&data, &BTreeMap::new()), Some(("Ollama", tr("Cloud"))));
        // An exception goes before the slot: on elsewhere it is where mail goes, off nothing sorts.
        data.settings.ai_slots.decide = Some(nimble);
        with_exception(&mut data, DEMO_OPENROUTER, "qwen/qwen3-235b-a22b", true);
        assert_eq!(sorting_place(&data, &state.models), cloud, "the exception sorts");
        with_exception(&mut data, DEMO_OPENROUTER, "qwen/qwen3-235b-a22b", false);
        assert_eq!(sorting_place(&data, &state.models), None, "switched off, nothing sorts");
        with_exception(&mut data, DEMO_OLLAMA, "nimble:latest", true);
        assert_eq!(sorting_place(&data, &state.models), None);
        data.settings.decide_mail_to_cloud = Some(false);
        with_exception(&mut data, DEMO_OPENROUTER, "qwen/qwen3-235b-a22b", true);
        assert_eq!(sorting_place(&data, &state.models), None, "the permission is off");

        // The line under "Sorting may send mail to the cloud" (R4): "runs on this computer" only while a
        // model here sorts; nothing while the exception is off or the slot is empty.
        data.settings.decide_mail_to_cloud = Some(true);
        with_exception(&mut data, DEMO_OLLAMA, "nimble:latest", true);
        assert_eq!(sorting_now(&data, &state.models), SortingNow::Here);
        with_exception(&mut data, DEMO_OLLAMA, "nimble:latest", false);
        assert_eq!(sorting_now(&data, &state.models), SortingNow::Nothing, "switched off");
        drop(data_rule_group(&data, &state.models));
        data.settings.ai_assignments.clear();
        data.settings.ai_slots.decide = None;
        assert_eq!(sorting_now(&data, &state.models), SortingNow::Nothing, "the slot is empty");
        drop(data_rule_group(&data, &state.models));
    }

    /// Review of #325 (F4): while the start of a download is out, a list of downloads is dropped – an older
    /// download of the same model never stands in; the poll that was out when the start went is dropped
    /// too, so it cannot stop the chain. From the start answer on, the chain follows that id alone.
    #[test]
    fn use_suggestion_follows_only_the_download_it_started() {
        let (mut state, data) = shown(false);
        state.slots.pulls = vec![AiPull { model: "gemma3:12b".into(), ..pull("px", AiPullState::Running) }];
        assert_eq!(ask(update(&mut state, Message::PollPulls, &data)), (Asked::Pulls, Request::AiPulls));
        let (asked, _) = ask(update(&mut state, Message::UseSuggestion(AiSlot::Chat), &data));
        assert_eq!(state.slots.using.as_ref().map(|using| using.step.clone()), Some(UseStep::Pull(None)));
        // The poll that was out answers before the start: dropped, with its older failed download.
        let old = Response::AiPulls { pulls: vec![pull("p0", AiPullState::Failed)] };
        assert!(matches!(ai_answered(&mut state, Asked::Pulls, &Ok(old.clone())), Action::None));
        assert_eq!(state.slots.using.as_ref().map(|using| using.step.clone()), Some(UseStep::Pull(None)));
        assert!(state.slots.failed.is_none());
        // Another tick while the start is out: dropped as well.
        assert_eq!(ask(update(&mut state, Message::PollPulls, &data)), (Asked::Pulls, Request::AiPulls));
        assert!(matches!(ai_answered(&mut state, Asked::Pulls, &Ok(old)), Action::None));
        assert!(state.slots.using.as_ref().is_some_and(|using| using.step == UseStep::Pull(None)));
        let started =
            Response::AiPulls { pulls: vec![pull("p0", AiPullState::Failed), pull("p1", AiPullState::Running)] };
        assert!(matches!(ai_answered(&mut state, asked, &Ok(started)), Action::None));
        assert_eq!(state.slots.using.as_ref().map(|using| using.step.clone()), Some(UseStep::Pull(Some("p1".into()))));
        // A list that names the older download only still stops: the one started is gone.
        let gone = Response::AiPulls { pulls: vec![pull("p0", AiPullState::Done)] };
        assert!(matches!(ai_answered(&mut state, Asked::Pulls, &Ok(gone)), Action::None));
        assert!(state.slots.using.is_none() && state.slots.failed.is_some());

        // The poll out at the start answers after it, without the new download: dropped, the chain goes on.
        let (mut state, data) = shown(false);
        state.slots.pulls = vec![AiPull { model: "gemma3:12b".into(), ..pull("px", AiPullState::Running) }];
        let _ = update(&mut state, Message::PollPulls, &data);
        let (asked, _) = ask(update(&mut state, Message::UseSuggestion(AiSlot::Chat), &data));
        let started =
            Response::AiPulls { pulls: vec![pull("p0", AiPullState::Running), pull("p1", AiPullState::Running)] };
        let _ = ai_answered(&mut state, asked, &Ok(started));
        let before = Response::AiPulls { pulls: vec![pull("p0", AiPullState::Running)] };
        assert!(matches!(ai_answered(&mut state, Asked::Pulls, &Ok(before)), Action::None));
        assert_eq!(state.slots.using.as_ref().map(|using| using.step.clone()), Some(UseStep::Pull(Some("p1".into()))));
        assert!(state.slots.failed.is_none() && state.slots.pulls.iter().any(|pull| pull.id == "p1"));
    }

    /// Review of #325 (F5): "Retry" after a refused cancel aims at the download the chain wanted to
    /// cancel, never at another one that runs; once that one runs no more, it only asks for the list.
    #[test]
    fn retry_after_a_refused_cancel_aims_at_the_same_download() {
        let (mut state, data) = shown(true);
        let _ = update(&mut state, Message::UseSuggestion(AiSlot::Chat), &data);
        let other = AiPull { model: "gemma3:12b".into(), ..pull("p2", AiPullState::Running) };
        state.slots.pulls.push(other);
        let (asked, _) = ask(update(&mut state, Message::CancelPull(AiSlot::Chat, "p1".into()), &data));
        let refused = Response::Error { code: "busy".into(), message: "Try again.".into() };
        let _ = ai_answered(&mut state, asked.clone(), &Ok(refused.clone()));
        let (_, again) = ask(update(&mut state, Message::RetrySlot, &data));
        assert_eq!(again, Request::CancelAiPull { id: "p1".into() }, "not the newest that runs");
        state.slots.pulls.retain(|pull| pull.id != "p1");
        let _ = ai_answered(&mut state, asked, &Ok(refused));
        let failure = state.slots.failed.as_ref().expect("the reason shows");
        assert!(matches!(*failure.retry, Message::PollPulls), "{:?}", failure.retry);
    }

    fn draw_ai(state: &State, data: &Data) {
        for palette in [Palette::dark(), Palette::light()] {
            for class in [SizeClass::Wide, SizeClass::Narrow] {
                drop(ai_view(state, data, class, &palette));
            }
        }
    }
}
