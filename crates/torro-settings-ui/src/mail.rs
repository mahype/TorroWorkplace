//! The sections behind the gear of the inbox's mail window (task S.20 in
//! `docs/tasks/steuerzentrale.md`, concept `docs/settings.md`): what belongs to writing and
//! reading mail.
//!
//! ```text
//! Accounts  one row per mail account; its page holds the sender name, the signature,
//!           "Write new mail as" and the folder questions the server left open
//! Reading   "Show mail as" and "Mail colors" for the reader
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

mod editor;

/// The sections, in the order of the list. [`Section::id`] is what `--settings <id>` takes.
pub const SECTIONS: [Section; 3] = [
    // Accounts is first: the gear lands on who the mail comes from.
    Section { id: "accounts", icon: Icon::Mail, label: "Accounts" },
    Section { id: "reading", icon: Icon::MailOpen, label: "Reading" },
    // What incoming mail is sorted into (task E12.10, `docs/kategorien.md`).
    Section { id: "categorization", icon: Icon::Tag, label: "Categorization" },
];

/// What one load of these sections needs: the suite-wide settings (Reading) and every account with
/// its mail settings (Accounts).
pub fn wanted() -> Vec<torro_ipc::Request> {
    vec![
        torro_ipc::Request::Settings,
        torro_ipc::Request::Accounts,
        // "Categorization" (task E12.10): what is assigned and how far the backlog has come.
        torro_ipc::Request::AiAssignments,
        torro_ipc::Request::GetCategorization,
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
}

/// What the sections report; the app wraps it in its own message.
#[derive(Debug, Clone)]
pub enum Message {
    Accounts(accounts::Message),
    Reading(reading::Message),
    Categorization(categorization::Message),
}

/// What a section's widget reported, and what the host is to do about it.
pub fn update<A>(state: &mut State, message: Message, data: &Data) -> Action<A> {
    match message {
        Message::Accounts(message) => accounts::update(&mut state.accounts, message, data),
        Message::Reading(message) => reading::update(message),
        Message::Categorization(message) => categorization::update(&mut state.categorization, message, data),
    }
}

/// The page of the section with this id, as the host takes it. An id no section has shows
/// Accounts, the section the gear lands on; the host only ever asks for one of [`SECTIONS`].
pub fn page<'a>(state: &'a State, data: &'a Data, section: &str) -> Page<'a, Message> {
    match section {
        "reading" => wrap(Page::new(reading::view(data)), Message::Reading),
        "categorization" => wrap(categorization::page(&state.categorization, data), Message::Categorization),
        _ => wrap(accounts::page(&state.accounts, data), Message::Accounts),
    }
}

/// What Escape means in these sections: a page of the categories' editor goes back to its list
/// (task E15.5), asking first when something typed would go. `None` leaves it to the window,
/// which closes the settings.
pub fn escape(state: &State) -> Option<Message> {
    editor::escape(&state.categorization)
        .map(|message| Message::Categorization(categorization::Message::Editor(message)))
}

/// One section's page with its messages wrapped in [`Message`].
fn wrap<'a, M: 'a>(page: Page<'a, M>, into: fn(M) -> Message) -> Page<'a, Message> {
    Page {
        body: page.body.map(into),
        sub_page: page.sub_page.map(|(name, back)| (name, into(back))),
        actions: page.actions.into_iter().map(|action| action.map(into)).collect(),
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

    use iced::widget::text::Wrapping;
    use iced::widget::{column, container, row, space as blank, text_editor};
    use iced::{Alignment, Fill};
    use torro_gui::theme::space;
    use torro_gui::{layout, style, typo, widgets};
    use torro_ipc::{AccountInfo, MailView, OpenRoleView, Request};
    use torro_mail::{MailFormat, Role};

    use super::*;
    use crate::parts::{chevron, field, picker, pressable, row_frame, setting_row};

    /// Section state: the open account's draft, which is also what says that its page is open –
    /// one holds the other, so the page never stands without something to type into.
    #[derive(Debug, Default)]
    pub struct State {
        /// What is typed into the open account's sender name and signature; `None` on the list of
        /// accounts.
        draft: Option<Draft>,
    }

    /// What a row of this section reports.
    #[derive(Debug, Clone)]
    pub enum Message {
        /// A mail account's row: open its page.
        Open(String),
        /// "‹ Back" from an account's page to the list.
        Back,
        /// Typing in the sender name.
        SenderName(String),
        /// Typing in the signature.
        Signature(text_editor::Action),
        /// "Save": the sender name and the signature, as far as they differ from the stored ones.
        Save,
        /// "Write new mail as" was set (issue #125).
        Format(MailFormat),
        /// "Use this folder" for a role the server left open.
        PickFolder { role: Role, path: String },
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

        /// What changed against the stored account, as `accounts.update_mail_account` with only
        /// those fields; `None` when nothing did (then "Save" is off).
        fn changes(&self, mail: &MailView) -> Option<Request> {
            let sender_name = self.sender_name.trim();
            let signature = self.signature_text();
            let stored_signature = mail.settings.signature.as_deref().unwrap_or_default().trim_end();
            let changed_name = (sender_name != mail.sender_name.trim()).then(|| sender_name.to_string());
            let changed_signature = (signature != stored_signature).then_some(signature);
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
                    state.draft = Some(Draft::new(&id, mail));
                }
                Action::None
            }
            Message::Back => {
                state.draft = None;
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
            Message::Save => {
                let Some((draft, mail)) = open_draft(state, data) else { return Action::None };
                draft.changes(mail).map_or(Action::None, Action::send)
            }
            Message::Format(format) => change(state, data, |mail| {
                (format != mail.settings.format).then(|| MailChange { format: Some(format), ..MailChange::default() })
            }),
            Message::PickFolder { role, path } => change(state, data, |_| {
                Some(MailChange { folder_roles: Some(BTreeMap::from([(role, path)])), ..MailChange::default() })
            }),
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
                Page::new(account_page(draft, mail)).sub_page(label, Message::Back)
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

    /// One account's page: sender name and signature with "Save", how new mail is written, and the
    /// folder questions the server left open.
    fn account_page<'a>(draft: &'a Draft, mail: &'a MailView) -> Element<'a, Message> {
        let mut page = column![sender_groups(draft, mail), writing_group(mail)].spacing(layout::section_gap());
        if !mail.open_roles.is_empty() {
            page = page.push(folders_group(&mail.open_roles));
        }
        page.into()
    }

    /// Sender name and signature, and "Save" under them: it is on only while they differ from what
    /// is stored, and Enter in the name works like it.
    fn sender_groups<'a>(draft: &'a Draft, mail: &MailView) -> Element<'a, Message> {
        let dirty = draft.changes(mail).is_some();
        let name = field("Anna Example", &draft.sender_name)
            .on_input(Message::SenderName)
            .on_submit_maybe(dirty.then_some(Message::Save));
        let signature = text_editor(&draft.signature)
            .on_action(Message::Signature)
            .font(typo::body().font)
            .size(typo::body().size)
            .line_height(typo::body().line_height)
            .padding(style::INPUT_PADDING)
            .wrapping(Wrapping::WordOrGlyph)
            .height(signature_height())
            .style(style::text_editor);
        let save_style: widgets::ButtonStyle = if dirty { style::button::primary } else { style::button::secondary };
        let save = widgets::text_button(None, tr("Save"), save_style, dirty.then_some(Message::Save));
        let groups = column![
            widgets::group(
                tr("Sender name"),
                vec![row_frame(name, false)],
                Some(tr("Shown next to your address in the mail you send. Optional.").to_string()),
            ),
            widgets::group(
                tr("Signature"),
                vec![container(signature).width(Fill).padding([space::s(), space::l()]).into()],
                Some(tr("Added under new mail, replies and forwards.").to_string()),
            ),
        ]
        .spacing(layout::section_gap());
        // "Save" belongs to both groups, so it sits close under them.
        column![groups, row![blank::horizontal(), save]].spacing(space::m()).into()
    }

    /// The room of the signature field: four lines of text and the padding of a field.
    const SIGNATURE_LINES: f32 = 4.0;

    fn signature_height() -> f32 {
        SIGNATURE_LINES * typo::line_height(typo::body()) + style::INPUT_PADDING.top + style::INPUT_PADDING.bottom
    }

    /// "Write new mail as": formatted (HTML with a plain text copy) or plain text (issue #125).
    fn writing_group(mail: &MailView) -> Element<'_, Message> {
        let choice = picker(FORMAT_CHOICES, FormatChoice(mail.settings.format), |choice: FormatChoice| {
            Message::Format(choice.0)
        });
        let row = setting_row(None, tr("Write new mail as"), None, choice);
        let footer = tr("Formatted mail can hold bold text, lists, links and quotes; a plain text copy goes along.");
        widgets::group(tr("Writing"), vec![row], Some(footer.to_string()))
    }

    /// The question for each role the server left open, with the folders it could be: one press
    /// picks (task M5.2).
    fn folders_group(roles: &[OpenRoleView]) -> Element<'_, Message> {
        let mut rows = Vec::new();
        for open in roles {
            let question = typo::text(typo::body_strong(), role_question(open.role));
            rows.push(container(question).width(Fill).padding([space::m(), space::l()]).into());
            for folder in &open.candidates {
                let pick = widgets::text_button(
                    None,
                    tr("Use this folder"),
                    style::button::secondary,
                    Some(Message::PickFolder { role: open.role, path: folder.path.clone() }),
                );
                let lines = column![
                    widgets::ellipsized(typo::body(), folder.name.as_str(), torro_gui::theme::text),
                    widgets::ellipsized(typo::meta(), folder.path.as_str(), torro_gui::theme::text_2),
                ];
                rows.push(row_frame(
                    row![lines.width(Fill), pick].spacing(space::l()).align_y(Alignment::Center),
                    true,
                ));
            }
        }
        let footer = tr("The server has more than one folder that could hold it. Your choice is remembered.");
        widgets::group(tr("Folders"), rows, Some(footer.to_string()))
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
        let _: Action<()> =
            update(&mut state, Message::Accounts(accounts::Message::Open("mail:anna@example.org".into())), &data);
        assert!(page(&state, &data, "accounts").sub_page.is_some());
    }

    /// The sections' ids and names, as the list and `--settings <section>` use them. "Keyboard
    /// shortcuts" of task E10.3 is not among them yet.
    #[test]
    fn the_sections_are_reachable_by_their_words() {
        let ids: Vec<&str> = SECTIONS.iter().map(|section| section.id).collect();
        assert_eq!(ids, ["accounts", "reading", "categorization"]);
        assert_eq!(SECTIONS[0].name(), tr("Accounts"));
        assert_eq!(SECTIONS[2].name(), tr("Categorization"), "task E12.10");
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
                torro_ipc::Request::AiAssignments,
                torro_ipc::Request::GetCategorization,
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
        let _ = categorization::view(&data);

        let none = Data { decider_models: vec![decider("off", false)], ..Data::default() };
        let nothing =
            categorization::update::<()>(&mut Default::default(), categorization::Message::Enabled(true), &none);
        assert!(matches!(nothing, Action::None), "nothing on, nothing to switch on");
        let _ = categorization::view(&none);
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
        let _ = categorization::view(&off);
        let Action::ControlCentre(section) =
            categorization::update::<()>(&mut Default::default(), categorization::Message::ManageModels, &off)
        else {
            panic!("the control centre")
        };
        assert_eq!(section, crate::assistant::AI_SECTION);

        let gone = Data { decider_models: vec![decider("other", true)], ..off.clone() };
        assert_eq!(categorization::assigned_off(&gone), Some((engine.to_string(), "nimble".to_string())), "gone");
        let _ = categorization::view(&gone);
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
        let _ = update::<()>(&mut state, open, &data);
        assert!(page(&state, &data, "categorization").sub_page.is_some());
        let back = Message::Categorization(categorization::Message::Editor(super::editor::Message::Back));
        let _ = update::<()>(&mut state, back, &data);
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
    use iced::widget::{column, svg};
    use torro_core::ai::{AiAssignment, AiFunction, AiModel};
    use torro_core::i18n::trf;
    use torro_gui::icon;
    use torro_gui::theme::palette;
    use torro_gui::{layout, style, theme, typo, widgets};
    use torro_ipc::Request;
    use torro_mail::Category;

    use super::*;
    use crate::parts::{choice, pressable, setting_row, switch};

    /// The section's state: the editor's open page (task E15.5).
    pub type State = super::editor::State;

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
            Message::Editor(message) => super::editor::update(state, message, data),
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
        match super::editor::page(state, data) {
            Some((name, body, save)) => {
                let back = Message::Editor(super::editor::Message::Back);
                Page::new(body.map(Message::Editor)).sub_page(name, back).actions(vec![save.map(Message::Editor)])
            }
            None => Page::new(view(data)),
        }
    }

    pub fn view(data: &Data) -> Element<'_, Message> {
        let held = assignment(data);
        let on = held.is_ready();
        let switch_row = setting_row(None, tr("Sort mail into categories"), None, switch(on, Message::Enabled));
        let sorting = widgets::group(
            tr("Categorization"),
            vec![switch_row, catch_up_row(data), sort_again_row(data)],
            Some(
                tr("A model on your computer says what each mail is. No mail leaves this computer, nothing is hidden, moved or deleted, and you can change any category by hand.")
                    .to_string(),
            ),
        );
        let models = widgets::group(tr("Model"), model_rows(data, &held), Some(model_note(data)));
        let sorting_rules = super::editor::rules_group(data).map(Message::Editor);
        // The rules before the long list of categories: they change more often. "Always for this
        // sender?" keeps its senders among them (task E15.3), so no link to the Notification Hub.
        // Whether the sidebar shows a category is on its page (task E15.5), not a second list.
        let mut page = column![sorting, models, sorting_rules].spacing(layout::section_gap());
        for group in super::editor::categories_groups(data) {
            page = page.push(group.map(Message::Editor));
        }
        page.into()
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

    /// "Sort again" (task E15.5): the window anew after new rules or descriptions, but what was
    /// filed by hand. It works with the rules alone, so it is there without a model too; while a
    /// run goes, "Catch up" shows how far and stops it.
    fn sort_again_row<'a>(data: &'a Data) -> Element<'a, Message> {
        let running =
            data.categorization.accounts.iter().any(|held| held.state == torro_ipc::inbox::CatchUpState::Running);
        let button = widgets::text_button(
            None,
            tr("Sort again"),
            style::button::secondary,
            (!running).then_some(Message::SortAgain),
        );
        setting_row(
            None,
            tr("Sort everything again"),
            Some(tr("After new rules or descriptions. What you filed by hand stays.").to_owned()),
            button,
        )
    }

    /// "Catch up": how far the backlog has come, and the button to start or stop it.
    fn catch_up_row<'a>(data: &'a Data) -> Element<'a, Message> {
        let (done, total) = data
            .categorization
            .accounts
            .iter()
            .fold((0u64, 0u64), |(done, total), held| (done + held.done, total + held.messages));
        let running =
            data.categorization.accounts.iter().any(|held| held.state == torro_ipc::inbox::CatchUpState::Running);
        // Sorting again (task E15.5) counts what it went through, the backlog what has a category.
        let again = data.categorization.again;
        let words = if total == 0 {
            tr("Not started").to_owned()
        } else if done >= total && again {
            tr("Sorted again").to_owned()
        } else if done >= total {
            tr("Done").to_owned()
        } else {
            trf("{done} of {total}", &[("done", &done), ("total", &done.max(total))])
        };
        let mut value = iced::widget::row![typo::text(typo::body(), words)]
            .spacing(theme::space::m())
            .align_y(iced::Alignment::Center);
        if running {
            value = value.push(widgets::text_button(
                None,
                tr("Cancel"),
                style::button::ghost,
                Some(Message::CatchUp(true)),
            ));
        } else {
            value = value.push(widgets::text_button(
                None,
                tr("Catch up now"),
                style::button::secondary,
                Some(Message::CatchUp(false)),
            ));
        }
        setting_row(
            None,
            tr("Catch up"),
            Some(tr("The inbox of the last 30 days, at most 500 messages per account.").to_owned()),
            value,
        )
    }
}
