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

/// The sections, in the order of the list. [`Section::id`] is what `--settings <id>` takes.
pub const SECTIONS: [Section; 2] = [
    // Accounts is first: the gear lands on who the mail comes from.
    Section { id: "accounts", icon: Icon::Mail, label: "Accounts" },
    Section { id: "reading", icon: Icon::MailOpen, label: "Reading" },
];

/// What one load of these sections needs: the suite-wide settings (Reading) and every account with
/// its mail settings (Accounts).
pub fn wanted() -> Vec<torro_ipc::Request> {
    vec![torro_ipc::Request::Settings, torro_ipc::Request::Accounts]
}

/// The state of every section that keeps some; the app owns one of these next to its host.
#[derive(Debug, Default)]
pub struct State {
    /// Accounts: which account's page is open and what is typed into it. Reading keeps nothing.
    pub accounts: accounts::State,
}

/// What the sections report; the app wraps it in its own message.
#[derive(Debug, Clone)]
pub enum Message {
    Accounts(accounts::Message),
    Reading(reading::Message),
}

/// What a section's widget reported, and what the host is to do about it.
pub fn update<A>(state: &mut State, message: Message, data: &Data) -> Action<A> {
    match message {
        Message::Accounts(message) => accounts::update(&mut state.accounts, message, data),
        Message::Reading(message) => reading::update(message),
    }
}

/// The page of the section with this id, as the host takes it. An id no section has shows
/// Accounts, the section the gear lands on; the host only ever asks for one of [`SECTIONS`].
pub fn page<'a>(state: &'a State, data: &'a Data, section: &str) -> Page<'a, Message> {
    match section {
        "reading" => wrap(Page::new(reading::view(data)), Message::Reading),
        _ => wrap(accounts::page(&state.accounts, data), Message::Accounts),
    }
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
        assert_eq!(ids, ["accounts", "reading"]);
        assert_eq!(SECTIONS[0].name(), tr("Accounts"));
        let (mut host, _) = Host::new(Arc::new(Demo::new()), SECTIONS.to_vec(), wanted());
        for id in ids {
            assert!(host.open(id), "{id}");
        }
        assert!(!host.open("sources"), "a section of the Notification Hub is not one of these");
    }

    /// One load asks for what the two sections show, and for nothing else.
    #[test]
    fn the_load_asks_for_what_the_sections_show() {
        assert_eq!(wanted(), vec![torro_ipc::Request::Settings, torro_ipc::Request::Accounts]);
    }
}
