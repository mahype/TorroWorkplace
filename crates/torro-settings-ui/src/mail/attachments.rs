//! Settings › Mail › Attachments (task M12.7, issue #290, views C and D of
//! `docs/prototypes/inbox/mail-anhaenge.html`): what a click on an attachment's name does, the
//! Downloads folder, and the save targets with their editor.
//!
//! ```text
//! Clicking an attachment   Click on an attachment ▾ · Downloads folder  Change…
//! Save targets             one row per target (name, folder and scheme)  Edit
//!                          + Add save target
//! ```
//!
//! The editor is a dialog over the window ([`overlay`]) in three parts – where (name, folder), how
//! it is ordered (subfolders, file name, date) and when it is suggested (variant V1: conditions with
//! ✕ and "Add condition") – with a live preview at the fixed example of the design (the invoice
//! `Rechnung_2026-03-28_4711.pdf` from Stadtwerke Bonn, mail of 2 Apr 2026), computed with the same
//! [`torro_mail::save_target::resolve`] the core uses.
//!
//! Everything is a request: `settings.set_setting` (`mail_attachment_click`, `mail_downloads_folder`),
//! `mail.create_save_target`, `mail.update_save_target`, `mail.remove_save_target`. The page checks
//! only what a person must fix before saving – the core checks all of it again – and shows it after
//! the first "Save". A folder is chosen in the system's chooser, which only the window opens
//! ([`super::AppAction::PickFolder`]); the window hands the path back with [`Message::FolderChosen`].
//!
//! The schemes are typed into one-line text editors, not text inputs: Iced 0.14's `text_input` does
//! not give out its cursor, and "Insert ▾" puts a placeholder where the cursor is
//! (`text_editor::Content::perform` with `Edit::Paste`). Enter saves, line breaks never get in.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::NaiveDate;
use iced::keyboard::{Key, key::Named};
use iced::widget::text::Wrapping;
use iced::widget::text_editor::{self, Binding, Edit, KeyPress, Motion};
use iced::widget::{column, container, row, rule, scrollable, space as blank, stack};
use iced::{Alignment, Element, Fill, Length, Padding, Theme};
use torro_core::i18n::{tr, trf};
use torro_core::settings::AttachmentClick;
use torro_gui::icon::Icon;
use torro_gui::layout::SizeClass;
use torro_gui::theme::{self, palette, size, space};
use torro_gui::widgets::{self, Align, Anchor, Mark, MenuItem, Placement};
use torro_gui::{layout, style, typo};
use torro_ipc::{Request, SettingKey};
use torro_mail::save_target::{DEFAULT_FILE_NAME, DEFAULT_SUBFOLDERS, date_in_name, resolve, suggestion};
use torro_mail::{
    Category, CategoryGroup, CategorySource, DateSource, FileFacts, FileKind, MailCategory, OnConflict, SaveTarget,
    SaveTargetSpec, SchemaError, SuggestWhen,
};

use super::{AppAction, FolderFor};
use crate::parts::{confirm, dialog, field, pressable, row_frame, setting_row};
use crate::{Action, Data};

/// The id of the editor's name field: "Save" with a mistake in the name goes there.
const NAME_FIELD: &str = "mail-save-target-name";

/// The ids of the two scheme fields: "Insert" puts the cursor back into its field.
const SUBFOLDERS_FIELD: &str = "mail-save-target-subfolders";
const FILE_NAME_FIELD: &str = "mail-save-target-file-name";

/// The most a name may have, in characters (the core's rule, `mail/save.rs`).
const NAME_MAX: usize = 60;

/// The most a scheme may have, in characters (the core's rule).
const SCHEME_MAX: usize = 200;

/// The most senders one target may name (the core's rule).
const SENDERS_MAX: usize = 20;

/// The fixed example of the preview (design, view D): the invoice of view A.
const EXAMPLE_FILE: &str = "Rechnung_2026-03-28_4711.pdf";
const EXAMPLE_SENDER: &str = "Stadtwerke Bonn";
const EXAMPLE_ADDRESS: &str = "rechnung@stadtwerke-bonn.de";
/// `{account}` is the account's display name; the example takes the one of the design.
const EXAMPLE_ACCOUNT: &str = "Privat";
const EXAMPLE_SUBJECT: &str = "Ihre Rechnung März 2026";

/// The months for `{month_name}`, January first, as `tr` knows them.
const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// What the section keeps between two frames: the editor while it is open.
#[derive(Debug, Default)]
pub struct State {
    editor: Option<Editor>,
    /// The menu of "Click on an attachment ▾" is open.
    click_menu: bool,
    /// Where that menu hangs: at its button on the page, laid out in the same frame.
    click_anchor: Anchor,
}

impl State {
    /// The editor's dialog is open.
    pub fn is_editing(&self) -> bool {
        self.editor.is_some()
    }
}

/// One of the two schemes of a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scheme {
    /// "Subfolders": `{year}/{month}`.
    Subfolders,
    /// "File name": `{name}.{ext}`.
    FileName,
}

/// A condition of "Suggest when", as a chip with ✕ shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Condition {
    Category(Category),
    Kind(FileKind),
    Sender(String),
}

/// What the page and the editor report.
#[derive(Debug, Clone)]
pub enum Message {
    /// "Click on an attachment" was set.
    Click(AttachmentClick),
    /// "Click on an attachment ▾": open its menu, or close it again.
    OpenClick,
    /// "Change…" at the Downloads folder: the window opens the system's chooser.
    ChangeDownloads,
    /// The window's chooser closed with a folder for `FolderFor` ([`super::folder_chosen`]).
    FolderChosen(FolderFor, PathBuf),
    /// "Add save target": the editor with a new target.
    Add,
    /// "Edit" at a target's row.
    Edit(String),
    Name(String),
    Folder(String),
    /// "Choose…" next to the folder.
    ChooseFolder,
    /// Typing in one of the scheme fields.
    Type(Scheme, text_editor::Action),
    /// "Insert ▾" at a scheme field: open its menu, or close it again.
    OpenInsert(Scheme),
    /// A placeholder of the "Insert" menu: put it where the cursor is.
    Insert(Scheme, &'static str),
    /// A pattern of the "Insert" menu: it replaces the whole scheme.
    Pattern(Scheme, &'static str),
    DateFrom(DateSource),
    OnConflict(OnConflict),
    /// "Add condition": open its menu, or close it again.
    OpenConditions,
    /// "More categories" in that menu: the rest of the catalogue.
    MoreCategories,
    ToggleCategory(Category),
    ToggleKind(FileKind),
    /// Typing a sender (address or domain) in the menu.
    SenderTyped(String),
    /// "Add" next to the sender, or Enter in it.
    AddSender,
    /// ✕ at a condition, or a ticked entry of the menu.
    RemoveCondition(Condition),
    /// A click next to an open menu.
    CloseMenu,
    Save,
    /// "Cancel", ✕ in the dialog's head, a click next to the dialog.
    Cancel,
    /// "Delete target": ask first.
    Delete,
    ConfirmDelete,
    CancelDelete,
}

/// The menu of the editor that is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Menu {
    Insert(Scheme),
    /// "Add condition"; `more` shows the categories beyond the paper trail.
    Conditions {
        more: bool,
    },
}

/// A save target as it is being typed: a new one (`id: None`) or one that is stored.
#[derive(Debug)]
struct Editor {
    id: Option<String>,
    name: String,
    folder: String,
    subfolders: text_editor::Content,
    file_name: text_editor::Content,
    date_from: DateSource,
    suggest: SuggestWhen,
    on_conflict: OnConflict,
    menu: Option<Menu>,
    /// The sender typed in the conditions' menu, not added yet.
    sender: String,
    /// "Delete target" was pressed: the dialog asks.
    deleting: bool,
    /// "Save" was pressed once: mistakes show from now on.
    tried: bool,
}

impl Editor {
    /// The editor for `spec`, stored under `id` or new.
    fn of(id: Option<String>, spec: &SaveTargetSpec) -> Editor {
        Editor {
            id,
            name: spec.name.clone(),
            folder: spec.folder.clone(),
            subfolders: content(&spec.subfolders),
            file_name: content(&spec.file_name),
            date_from: spec.date_from,
            suggest: spec.suggest.clone(),
            on_conflict: spec.on_conflict,
            menu: None,
            sender: String::new(),
            deleting: false,
            tried: false,
        }
    }

    fn scheme(&mut self, scheme: Scheme) -> &mut text_editor::Content {
        match scheme {
            Scheme::Subfolders => &mut self.subfolders,
            Scheme::FileName => &mut self.file_name,
        }
    }

    /// The target as it would be sent: name and folder without white space around them, the
    /// schemes on one line.
    fn spec(&self) -> SaveTargetSpec {
        SaveTargetSpec {
            name: self.name.trim().to_string(),
            folder: self.folder.trim().to_string(),
            subfolders: one_line(&self.subfolders),
            file_name: one_line(&self.file_name),
            date_from: self.date_from,
            suggest: self.suggest.clone(),
            on_conflict: self.on_conflict,
        }
    }

    /// What the person must fix before saving, field by field; empty when nothing.
    fn mistakes(&self, data: &Data) -> Mistakes {
        let spec = self.spec();
        Mistakes {
            name: name_mistake(&spec.name, self.id.as_deref(), &data.save_targets),
            folder: folder_mistake(&spec.folder),
            subfolders: scheme_mistake(Scheme::Subfolders, &spec.subfolders),
            file_name: scheme_mistake(Scheme::FileName, &spec.file_name),
        }
    }

    /// The conditions of "Suggest when" in the order the chips show them.
    fn conditions(&self) -> Vec<Condition> {
        let when = &self.suggest;
        when.categories
            .iter()
            .copied()
            .map(Condition::Category)
            .chain(when.kinds.iter().copied().map(Condition::Kind))
            .chain(when.senders.iter().cloned().map(Condition::Sender))
            .collect()
    }
}

/// The mistakes of the editor's fields.
#[derive(Debug, Default, PartialEq, Eq)]
struct Mistakes {
    name: Option<String>,
    folder: Option<String>,
    subfolders: Option<String>,
    file_name: Option<String>,
}

impl Mistakes {
    fn is_empty(&self) -> bool {
        *self == Mistakes::default()
    }
}

/// What a row of the page or the editor reported: a setting or a change of a target to send, a
/// chooser for the window to open, or nothing but a change of the editor.
pub fn update(state: &mut State, message: Message, data: &Data) -> Action<AppAction> {
    match message {
        Message::Click(click) => {
            state.click_menu = false;
            let value = serde_json::to_value(&click).unwrap_or_default();
            Action::send(Request::SetSetting { key: SettingKey::MailAttachmentClick, value })
        }
        Message::OpenClick => {
            state.click_menu = !state.click_menu;
            Action::None
        }
        Message::CloseMenu if state.click_menu => {
            state.click_menu = false;
            Action::None
        }
        Message::ChangeDownloads => Action::App(AppAction::PickFolder(FolderFor::Downloads)),
        Message::FolderChosen(FolderFor::Downloads, path) => {
            downloads_request(&path).map_or(Action::None, Action::send)
        }
        Message::Add => {
            let spec = SaveTargetSpec::new("", "");
            state.editor = Some(Editor::of(None, &spec));
            Action::Focus(NAME_FIELD)
        }
        Message::Edit(id) => {
            // A target the fresh data no longer holds opens nothing; the list shows what is there.
            if let Some(target) = data.save_targets.iter().find(|target| target.id == id) {
                state.editor = Some(Editor::of(Some(id), &target.spec));
            }
            Action::None
        }
        Message::Cancel => {
            state.editor = None;
            Action::None
        }
        message => match state.editor.as_mut() {
            Some(editor) => {
                let (action, closes) = editor_update(editor, message, data);
                if closes {
                    state.editor = None;
                }
                action
            }
            None => Action::None,
        },
    }
}

/// A message of the open editor: the action, and whether the dialog closes.
fn editor_update(editor: &mut Editor, message: Message, data: &Data) -> (Action<AppAction>, bool) {
    match message {
        Message::Name(name) => editor.name = name,
        Message::Folder(folder) => editor.folder = folder,
        Message::ChooseFolder => return (Action::App(AppAction::PickFolder(FolderFor::Target)), false),
        // A path that is not plain text cannot be stored as the person sees it.
        Message::FolderChosen(FolderFor::Target, path) => {
            if let Some(path) = path.to_str() {
                editor.folder = path.to_string();
            }
        }
        Message::Type(scheme, action) => {
            if let Some(action) = one_line_action(action) {
                editor.scheme(scheme).perform(action);
            }
        }
        Message::OpenInsert(scheme) => editor.menu = toggled(editor.menu, Menu::Insert(scheme)),
        Message::Insert(scheme, placeholder) => {
            editor.scheme(scheme).perform(text_editor::Action::Edit(Edit::Paste(Arc::new(placeholder.to_string()))));
            editor.menu = None;
            return (Action::Focus(field_id(scheme)), false);
        }
        Message::Pattern(scheme, pattern) => {
            let field = editor.scheme(scheme);
            *field = content(pattern);
            editor.menu = None;
            return (Action::Focus(field_id(scheme)), false);
        }
        Message::DateFrom(from) => editor.date_from = from,
        Message::OnConflict(conflict) => editor.on_conflict = conflict,
        Message::OpenConditions => editor.menu = toggled(editor.menu, Menu::Conditions { more: false }),
        Message::MoreCategories => editor.menu = Some(Menu::Conditions { more: true }),
        Message::ToggleCategory(category) => toggle(&mut editor.suggest.categories, category),
        Message::ToggleKind(kind) => toggle(&mut editor.suggest.kinds, kind),
        Message::SenderTyped(sender) => editor.sender = sender,
        Message::AddSender => {
            let sender = editor.sender.trim().to_lowercase();
            let known = editor.suggest.senders.iter().any(|held| held.eq_ignore_ascii_case(&sender));
            if !sender.is_empty() && !known && editor.suggest.senders.len() < SENDERS_MAX {
                editor.suggest.senders.push(sender);
            }
            editor.sender.clear();
        }
        Message::RemoveCondition(condition) => match condition {
            Condition::Category(category) => editor.suggest.categories.retain(|held| *held != category),
            Condition::Kind(kind) => editor.suggest.kinds.retain(|held| *held != kind),
            Condition::Sender(sender) => editor.suggest.senders.retain(|held| *held != sender),
        },
        Message::CloseMenu => editor.menu = None,
        Message::Save => {
            editor.tried = true;
            editor.menu = None;
            let mistakes = editor.mistakes(data);
            if !mistakes.is_empty() {
                // The cursor into the first field with a mistake; the others stand at their field.
                let into = if mistakes.name.is_some() {
                    Some(NAME_FIELD)
                } else if mistakes.subfolders.is_some() {
                    Some(SUBFOLDERS_FIELD)
                } else if mistakes.file_name.is_some() {
                    Some(FILE_NAME_FIELD)
                } else {
                    None
                };
                return (into.map_or(Action::None, Action::Focus), false);
            }
            let target = editor.spec();
            let request = match editor.id.clone() {
                Some(id) => Request::UpdateSaveTarget { id, target },
                None => Request::CreateSaveTarget { target },
            };
            return (Action::send(request), true);
        }
        Message::Delete => editor.deleting = editor.id.is_some(),
        Message::CancelDelete => editor.deleting = false,
        Message::ConfirmDelete => {
            let Some(id) = editor.id.clone() else { return (Action::None, false) };
            return (Action::send(Request::RemoveSaveTarget { id }), true);
        }
        // Handled by `update`, which owns the page.
        Message::Click(_)
        | Message::OpenClick
        | Message::ChangeDownloads
        | Message::FolderChosen(FolderFor::Downloads, _)
        | Message::Add
        | Message::Edit(_)
        | Message::Cancel => {}
    }
    (Action::None, false)
}

/// What Escape means here: close the open menu, then the question, then the dialog. `None` while
/// no dialog is open – then the settings close as before.
pub fn escape(state: &State) -> Option<Message> {
    if state.click_menu {
        return Some(Message::CloseMenu);
    }
    let editor = state.editor.as_ref()?;
    Some(if editor.menu.is_some() {
        Message::CloseMenu
    } else if editor.deleting {
        Message::CancelDelete
    } else {
        Message::Cancel
    })
}

/// `settings.set_setting mail_downloads_folder` with the folder the person chose. `None` for a path
/// that is not plain text.
pub fn downloads_request(folder: &Path) -> Option<Request> {
    let path = folder.to_str()?;
    Some(Request::SetSetting { key: SettingKey::MailDownloadsFolder, value: serde_json::Value::String(path.into()) })
}

// ----- Helpers ------------------------------------------------------------------------------

fn content(text: &str) -> text_editor::Content {
    let mut content = text_editor::Content::with_text(text);
    // The cursor at the end, where typing goes on.
    content.perform(text_editor::Action::Move(Motion::DocumentEnd));
    content
}

/// The scheme of a field: one line, without the line ending an editor adds.
fn one_line(content: &text_editor::Content) -> String {
    content.text().replace(['\r', '\n'], "")
}

/// `action` as it may reach a one-line field: no line breaks, no indenting; pasted text on one line.
fn one_line_action(action: text_editor::Action) -> Option<text_editor::Action> {
    match action {
        text_editor::Action::Edit(Edit::Enter | Edit::Indent | Edit::Unindent) => None,
        text_editor::Action::Edit(Edit::Paste(text)) => {
            let flat = text.replace(['\r', '\n'], "");
            Some(text_editor::Action::Edit(Edit::Paste(Arc::new(flat))))
        }
        other => Some(other),
    }
}

fn field_id(scheme: Scheme) -> &'static str {
    match scheme {
        Scheme::Subfolders => SUBFOLDERS_FIELD,
        Scheme::FileName => FILE_NAME_FIELD,
    }
}

/// `menu` when another (or none) is open, none when it is open already.
fn toggled(open: Option<Menu>, menu: Menu) -> Option<Menu> {
    if open.is_some_and(|held| held_same(held, menu)) { None } else { Some(menu) }
}

/// Two menus that one button opens: the conditions' menu is one, whatever it shows.
fn held_same(held: Menu, menu: Menu) -> bool {
    match (held, menu) {
        (Menu::Insert(a), Menu::Insert(b)) => a == b,
        (Menu::Conditions { .. }, Menu::Conditions { .. }) => true,
        _ => false,
    }
}

fn toggle<T: PartialEq>(list: &mut Vec<T>, value: T) {
    match list.iter().position(|held| *held == value) {
        Some(at) => {
            list.remove(at);
        }
        None => list.push(value),
    }
}

fn name_mistake(name: &str, id: Option<&str>, targets: &[SaveTarget]) -> Option<String> {
    let chars = name.chars().count();
    if chars == 0 {
        return Some(tr("Enter a name.").to_string());
    }
    if chars > NAME_MAX {
        return Some(trf("A name can have at most {n} characters.", &[("n", &NAME_MAX)]));
    }
    let taken = targets
        .iter()
        .any(|target| Some(target.id.as_str()) != id && target.spec.name.to_lowercase() == name.to_lowercase());
    taken.then(|| tr("Another save target has this name.").to_string())
}

fn folder_mistake(folder: &str) -> Option<String> {
    if folder.is_empty() {
        return Some(tr("Choose a folder.").to_string());
    }
    (!Path::new(folder).is_absolute()).then(|| tr("Choose a folder, or enter its full path.").to_string())
}

/// The mistake in one scheme: too long, or a placeholder that does not exist. Checked on its own,
/// with the other scheme as a new target has it, so the mistake stands at the field it is in.
fn scheme_mistake(scheme: Scheme, text: &str) -> Option<String> {
    if text.chars().count() > SCHEME_MAX {
        return Some(trf("A scheme can have at most {n} characters.", &[("n", &SCHEME_MAX)]));
    }
    let mut spec = SaveTargetSpec::new(EXAMPLE_ACCOUNT, "/");
    match scheme {
        Scheme::Subfolders => {
            spec.subfolders = text.to_string();
            spec.file_name = DEFAULT_FILE_NAME.to_string();
        }
        Scheme::FileName => {
            spec.subfolders = DEFAULT_SUBFOLDERS.to_string();
            spec.file_name = text.to_string();
        }
    }
    match resolve(&spec, &example()) {
        Ok(_) => None,
        Err(SchemaError::Unknown(name)) => Some(trf(
            "“{placeholder}” is not a placeholder. Insert lists them all.",
            &[("placeholder", &format!("{{{name}}}"))],
        )),
    }
}

/// The attachment of the preview: the invoice of the design.
fn example() -> FileFacts {
    FileFacts {
        mail_date: mail_date(),
        file_name: Some(EXAMPLE_FILE.to_string()),
        content_type: "application/pdf".to_string(),
        sender_name: Some(EXAMPLE_SENDER.to_string()),
        sender_address: EXAMPLE_ADDRESS.to_string(),
        account: EXAMPLE_ACCOUNT.to_string(),
        subject: EXAMPLE_SUBJECT.to_string(),
        month_names: MONTHS.map(|month| tr(month).to_string()),
    }
}

/// The day the example mail came: 2 April 2026.
fn mail_date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 4, 2).unwrap_or_default()
}

/// A day as a person reads it: "2 April 2026".
fn day_words(date: NaiveDate) -> String {
    use chrono::Datelike;
    let month = MONTHS.get(date.month0() as usize).map_or("", |month| tr(month));
    trf("{day} {month} {year}", &[("day", &date.day()), ("month", &month), ("year", &date.year())])
}

/// What the preview shows for `spec`: the whole path, and why in one sentence. `None` while a
/// scheme has a placeholder that does not exist.
fn preview(spec: &SaveTargetSpec) -> Option<(String, String)> {
    let facts = example();
    let resolved = resolve(spec, &facts).ok()?;
    let mut path = PathBuf::from(&spec.folder);
    path.extend(&resolved.folders);
    path.push(&resolved.file);
    let from_name = (spec.date_from == DateSource::FileName).then(|| date_in_name(EXAMPLE_FILE)).flatten();
    let date = match from_name {
        Some(date) => trf(
            "Date {date} from the file name (the mail came on {mail}).",
            &[("date", &day_words(date)), ("mail", &day_words(facts.mail_date))],
        ),
        None => trf("Date {date} from the mail.", &[("date", &day_words(facts.mail_date))]),
    };
    // The example's category comes from a header, as an invoice usually does: a sure one.
    let category = MailCategory { category: Category::Invoice, source: CategorySource::Rule, confidence: None };
    let kind = FileKind::of(&facts.content_type, facts.file_name.as_deref());
    let suggested = match suggestion(&spec.suggest, Some(category), kind, &facts.sender_address) {
        Some(_) => tr("Suggested for this file."),
        None => tr("Not suggested for this file."),
    };
    let conflict = match spec.on_conflict {
        OnConflict::KeepBoth => tr("If a file of that name exists, the new one gets a number."),
        OnConflict::SkipIdentical => tr("If the same file is already there, nothing is saved again."),
    };
    Some((path.display().to_string(), format!("{date} {suggested} {conflict}")))
}

/// The choices of "Click on an attachment", with one per target.
fn click_choices(data: &Data) -> Vec<ClickChoice> {
    let mut choices = vec![
        ClickChoice { click: AttachmentClick::Open, label: tr("Open").to_string() },
        ClickChoice { click: AttachmentClick::Downloads, label: tr("Save to Downloads").to_string() },
        ClickChoice { click: AttachmentClick::SaveAs, label: tr("Save as…").to_string() },
    ];
    choices.extend(data.save_targets.iter().map(|target| ClickChoice {
        click: AttachmentClick::Target(target.id.clone()),
        label: trf("Save to {target}", &[("target", &target.spec.name)]),
    }));
    choices
        .push(ClickChoice { click: AttachmentClick::Suggested, label: tr("Suggested target, else open").to_string() });
    choices
}

/// A choice of "Click on an attachment": what it sets and the words of its menu entry.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ClickChoice {
    click: AttachmentClick,
    label: String,
}

impl std::fmt::Display for ClickChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.label)
    }
}

fn kind_label(kind: FileKind) -> &'static str {
    match kind {
        FileKind::Pdf => tr("PDF"),
        FileKind::Image => tr("Images"),
        FileKind::Office => tr("Office"),
    }
}

fn condition_label(data: &Data, condition: &Condition) -> String {
    match condition {
        Condition::Category(category) => trf("Category: {name}", &[("name", &data.category_name(*category))]),
        Condition::Kind(kind) => trf("Type: {name}", &[("name", &kind_label(*kind))]),
        Condition::Sender(sender) => trf("From: {sender}", &[("sender", sender)]),
    }
}

/// The categories of the paper trail first (invoices, receipts …), then, with `more`, the rest of
/// the catalogue – as the catalogue groups them now, or the suite's groups while it is not loaded.
fn categories(data: &Data) -> (Vec<Category>, Vec<Category>) {
    if data.categories.is_empty() {
        let paper = CategoryGroup::PaperTrail.default_categories();
        let rest = Category::BUILT_IN.into_iter().filter(|category| !paper.contains(category)).collect();
        return (paper, rest);
    }
    let (paper, rest): (Vec<_>, Vec<_>) =
        data.categories.iter().partition(|info| info.group == CategoryGroup::PaperTrail);
    (paper.into_iter().map(|info| info.category).collect(), rest.into_iter().map(|info| info.category).collect())
}

/// The placeholders of "Insert ▾", in their groups, each with its label.
const PLACEHOLDERS: [(&str, &[(&str, &str)]); 3] = [
    (
        "Date",
        &[
            ("Year", "{year}"),
            ("Month", "{month}"),
            ("Month name", "{month_name}"),
            ("Day", "{day}"),
            ("Quarter", "{quarter}"),
            ("Full date", "{date}"),
        ],
    ),
    (
        "Mail",
        &[
            ("Sender", "{sender}"),
            ("Sender domain", "{sender_domain}"),
            ("Account", "{account}"),
            ("Subject", "{subject}"),
        ],
    ),
    ("File", &[("Original name", "{name}"), ("Extension", "{ext}")]),
];

/// The patterns of "Insert ▾" for each scheme: what replaces the whole field.
fn patterns(scheme: Scheme) -> &'static [(&'static str, &'static str)] {
    match scheme {
        Scheme::Subfolders => &[
            ("None", ""),
            ("Year", "{year}"),
            ("Year / Month", "{year}/{month}"),
            ("Year / Quarter", "{year}/{quarter}"),
            ("Sender / Year", "{sender}/{year}"),
            ("Year / Month name", "{year}/{month} {month_name}"),
        ],
        Scheme::FileName => &[
            ("Keep original", "{name}.{ext}"),
            ("Date first", "{date}_{name}.{ext}"),
            ("Date and sender", "{date}_{sender}_{name}.{ext}"),
        ],
    }
}

/// What `placeholder` stands for in the example: `2026`, `Stadtwerke Bonn`.
fn example_value(placeholder: &str, from: DateSource) -> String {
    let mut spec = SaveTargetSpec::new(EXAMPLE_ACCOUNT, "/");
    spec.subfolders = placeholder.to_string();
    spec.date_from = from;
    resolve(&spec, &example()).map(|resolved| resolved.folders.join("/")).unwrap_or_default()
}

// ----- Page (C) -----------------------------------------------------------------------------

/// The section's page: "Clicking an attachment" and "Save targets". The editor is the [`overlay`].
pub fn view<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    column![clicking_group(state, data), targets_group(data)].spacing(layout::section_gap()).into()
}

/// The choice "Click on an attachment" holds now. A target that is gone opens the file (the core sets
/// it back), so the button says "Open".
fn chosen_click(choices: &[ClickChoice], data: &Data) -> Option<usize> {
    let held = &data.settings.mail_attachments.click;
    choices.iter().position(|choice| &choice.click == held).or((!choices.is_empty()).then_some(0))
}

/// "Clicking an attachment": the click and the Downloads folder. The click is a menu button
/// (`widgets::select_button`) like "Insert ▾" in the editor, its menu the [`overlay`] of the page.
fn clicking_group<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let choices = click_choices(data);
    let click = match chosen_click(&choices, data).and_then(|at| choices.get(at)) {
        Some(chosen) => widgets::select_button(&state.click_anchor, chosen.label.clone(), Message::OpenClick),
        None => blank().into(),
    };
    let downloads = Some(data.downloads.clone()).filter(|path| !path.is_empty());
    let change = widgets::text_button(None, tr("Change…"), style::button::secondary, Some(Message::ChangeDownloads));
    widgets::group(
        tr("Clicking an attachment"),
        vec![
            setting_row(
                None,
                tr("Click on an attachment"),
                Some(tr("The triangle beside it always offers every way.").to_string()),
                click,
            ),
            setting_row(None, tr("Downloads folder"), downloads, change),
        ],
        None,
    )
}

/// The menu of "Click on an attachment ▾" at its button: every choice, the held one marked.
fn click_menu<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let choices = click_choices(data);
    let held = chosen_click(&choices, data);
    let items = choices
        .into_iter()
        .enumerate()
        .map(|(at, choice)| {
            let item = MenuItem::new(Icon::Plus, choice.label, Message::Click(choice.click));
            MenuItem { mark: None, ..item }.current(held == Some(at))
        })
        .collect();
    let card = widgets::menu(vec![widgets::MenuSection::new(items)]);
    widgets::floating(&state.click_anchor, Placement::Below(Align::End), card, Message::CloseMenu)
}

/// "Save targets": one row per target and "Add save target".
fn targets_group(data: &Data) -> Element<'_, Message> {
    let mut rows: Vec<Element<'_, Message>> = data.save_targets.iter().map(target_row).collect();
    rows.push(add_row(tr("Add save target"), Message::Add));
    widgets::group(
        tr("Save targets"),
        rows,
        Some(
            tr("A save target is a folder with a scheme for subfolders and file names. It appears in the triangle's menu and can be the click action.")
                .to_string(),
        ),
    )
}

/// A row that adds something, with "+" in front, as the categories' editor has them.
fn add_row<'a>(label: &'a str, message: Message) -> Element<'a, Message> {
    let plus = torro_gui::icon::icon(Icon::Plus, torro_gui::icon::SIZE)
        .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).text_2) });
    pressable(setting_row(Some(plus.into()), label, None, blank()), Some(message))
}

/// A target's row: folder icon, name, and under it where files go with the schemes, in `text_3`.
fn target_row(target: &SaveTarget) -> Element<'_, Message> {
    let spec = &target.spec;
    let place = Path::new(&spec.folder).join(&spec.subfolders).join(&spec.file_name).display().to_string();
    let mark = torro_gui::icon::icon(Icon::Folder, torro_gui::icon::SIZE)
        .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).text_2) });
    let lines = column![
        widgets::ellipsized(typo::body(), spec.name.as_str(), theme::text),
        widgets::ellipsized(typo::meta(), place, theme::text_3),
    ]
    .width(Fill);
    let edit = widgets::text_button(
        Some(Icon::SquarePen),
        tr("Edit"),
        style::button::ghost,
        Some(Message::Edit(target.id.clone())),
    );
    let content = row![mark, lines, edit].spacing(space::m()).align_y(Alignment::Center);
    row_frame(content, true)
}

// ----- Editor (D) ---------------------------------------------------------------------------

/// Where the menus of the editor hang: at their buttons, in the same frame.
#[derive(Default)]
struct Anchors {
    subfolders: Anchor,
    file_name: Anchor,
    conditions: Anchor,
}

/// The editor as a dialog over the window, with its open menu above it; the question "Delete this
/// save target?" instead while it is asked. `None` while no editor is open. `class` is the window's
/// size class: narrow windows put the labels above the fields.
pub fn overlay<'a>(state: &'a State, data: &'a Data, class: SizeClass) -> Option<Element<'a, Message>> {
    if state.click_menu && state.editor.is_none() {
        return Some(click_menu(state, data));
    }
    let editor = state.editor.as_ref()?;
    if editor.deleting {
        return Some(confirm(
            tr("Delete this save target?"),
            trf(
                "“{name}” leaves the menus. Files saved with it stay where they are.",
                &[("name", &editor.name.trim())],
            ),
            tr("Delete target"),
            Message::ConfirmDelete,
            Message::CancelDelete,
        ));
    }
    let anchors = Anchors::default();
    let stacked = matches!(class, SizeClass::Narrow | SizeClass::Mini | SizeClass::Strip);
    let card = dialog(editor_body(editor, data, &anchors, stacked), size::form_max(), Message::Cancel);
    let menu = editor.menu.map(|menu| {
        let (anchor, card) = match menu {
            Menu::Insert(scheme) => {
                let anchor = match scheme {
                    Scheme::Subfolders => &anchors.subfolders,
                    Scheme::FileName => &anchors.file_name,
                };
                (anchor, insert_menu(scheme, editor.date_from))
            }
            Menu::Conditions { more } => (&anchors.conditions, conditions_menu(editor, data, more)),
        };
        widgets::floating(anchor, Placement::Below(Align::End), card, Message::CloseMenu)
    });
    // Always the same two layers, so opening a menu does not build the dialog's widgets afresh
    // (a field would lose its focus).
    let menu = menu.unwrap_or_else(|| blank().into());
    Some(stack![card, menu].into())
}

/// The dialog's content: head, the three parts and the preview (scrolling when the window is low),
/// and the foot.
fn editor_body<'a>(editor: &'a Editor, data: &'a Data, anchors: &Anchors, stacked: bool) -> Element<'a, Message> {
    let title = match editor.id {
        Some(_) => tr("Edit save target"),
        None => tr("New save target"),
    };
    let close = widgets::icon_button(Icon::X, tr("Close"), Some(Message::Cancel));
    let head = row![typo::text(typo::title(), title).width(Fill), close].align_y(Alignment::Center);
    let mistakes = if editor.tried { editor.mistakes(data) } else { Mistakes::default() };
    let parts = column![
        where_part(editor, &mistakes, stacked),
        line(),
        order_part(editor, &mistakes, anchors, stacked),
        line(),
        suggest_part(editor, data, anchors, stacked),
        preview_box(editor),
    ]
    .spacing(space::l());
    // The body keeps room for its scroll bar at the right; head and foot keep the same, so the
    // fields, ✕ and "Save" end on one line.
    let inset = || Padding::ZERO.right(space::m());
    let body = scrollable(container(parts).padding(inset())).height(Length::Shrink).style(style::scrollable);
    column![container(head).padding(inset()), body, container(foot(editor)).padding(inset())].spacing(space::l()).into()
}

fn line<'a>() -> Element<'a, Message> {
    rule::horizontal(size::HAIRLINE).style(style::rule).into()
}

/// Where: the name and the folder.
fn where_part<'a>(editor: &'a Editor, mistakes: &Mistakes, stacked: bool) -> Element<'a, Message> {
    let name = field(tr("Invoices"), &editor.name).id(NAME_FIELD).on_input(Message::Name).on_submit(Message::Save);
    let folder =
        field(tr("A folder on this computer"), &editor.folder).on_input(Message::Folder).on_submit(Message::Save);
    let choose = widgets::text_button(None, tr("Choose…"), style::button::secondary, Some(Message::ChooseFolder));
    let folder = row![folder, choose].spacing(space::s()).align_y(Alignment::Center);
    column![
        form_row(tr("Name"), name, note(mistakes.name.clone(), None), stacked),
        form_row(tr("Folder"), folder, note(mistakes.folder.clone(), None), stacked),
    ]
    .spacing(space::m())
    .into()
}

/// How it is ordered: the subfolders, the file name and where the date comes from.
fn order_part<'a>(editor: &'a Editor, mistakes: &Mistakes, anchors: &Anchors, stacked: bool) -> Element<'a, Message> {
    let subfolders = scheme_field(editor, Scheme::Subfolders, &anchors.subfolders);
    let file_name = scheme_field(editor, Scheme::FileName, &anchors.file_name);
    let date_choices = [DateSource::Mail, DateSource::FileName];
    let date_at = date_choices.iter().position(|from| *from == editor.date_from).unwrap_or_default();
    let date = widgets::segmented(
        [
            (tr("Mail date"), Message::DateFrom(DateSource::Mail)),
            (tr("File name, else mail"), Message::DateFrom(DateSource::FileName)),
        ],
        date_at,
    );
    column![
        form_row(
            tr("Subfolders"),
            subfolders,
            note(mistakes.subfolders.clone(), Some(tr("A “/” starts a new folder."))),
            stacked
        ),
        form_row(tr("File name"), file_name, note(mistakes.file_name.clone(), None), stacked),
        form_row(
            tr("Date from"),
            date,
            note(
                None,
                Some(tr(
                    "For invoices, the date in the file name is often the invoice date. The mail can come weeks later."
                ))
            ),
            stacked
        ),
    ]
    .spacing(space::m())
    .into()
}

/// When it is suggested: the conditions and what happens to a file of the same name.
fn suggest_part<'a>(editor: &'a Editor, data: &'a Data, anchors: &Anchors, stacked: bool) -> Element<'a, Message> {
    let mut chips: Vec<Element<'a, Message>> = editor
        .conditions()
        .into_iter()
        .map(|condition| {
            let label = condition_label(data, &condition);
            let tip = trf("Remove {condition}", &[("condition", &label)]);
            widgets::with_tooltip(condition_chip(label, Message::RemoveCondition(condition)), tip)
        })
        .collect();
    // A chip like the conditions, so it lines up in their grid instead of a taller button that
    // wraps with its own padding.
    let add = widgets::chip(Some(Mark::Icon(Icon::Plus)), tr("Add condition"), Some(Message::OpenConditions));
    chips.push(widgets::anchored(&anchors.conditions, add));
    let explained = match editor.conditions().is_empty() {
        true => tr("Never suggested."),
        false => tr("One of each kind that is set must fit."),
    };
    let conflict_choices = [OnConflict::KeepBoth, OnConflict::SkipIdentical];
    let conflict_at = conflict_choices.iter().position(|held| *held == editor.on_conflict).unwrap_or_default();
    let conflict = widgets::segmented(
        [
            (tr("Keep both"), Message::OnConflict(OnConflict::KeepBoth)),
            (tr("Skip if identical"), Message::OnConflict(OnConflict::SkipIdentical)),
        ],
        conflict_at,
    );
    column![
        form_row(
            tr("Suggest when"),
            row(chips).spacing(space::s()).align_y(Alignment::Center).wrap().vertical_spacing(space::s()),
            note(None, Some(explained)),
            stacked
        ),
        form_row(tr("If file exists"), conflict, None, stacked),
    ]
    .spacing(space::m())
    .into()
}

/// A condition of "Suggest when" as a chip (`style::button::chip`, as high as `widgets::chip`): its
/// words, then ✕ at the right, where a chip that removes something has it – `widgets::chip` leads with
/// its mark, which would read as the condition's icon. A click sends `on_press`.
fn condition_chip<'a>(label: String, on_press: Message) -> Element<'a, Message> {
    let x = torro_gui::icon::icon(Icon::X, torro_gui::icon::SIZE_SM)
        .style(|t: &Theme, _| iced::widget::svg::Style { color: Some(palette(t).text_3) });
    let content = row![typo::text(typo::meta(), label).wrapping(Wrapping::None), x]
        .spacing(space::xs())
        .align_y(Alignment::Center);
    iced::widget::button(container(content).center_y(Fill))
        .height(style::CHIP_HEIGHT)
        .padding(style::CHIP_PADDING)
        .style(style::button::chip)
        .on_press(on_press)
        .into()
}

/// The live preview at the fixed example: where the invoice would go, and why.
fn preview_box(editor: &Editor) -> Element<'_, Message> {
    let caption = trf(
        "Preview · {file} from {sender}, {date}",
        &[("file", &EXAMPLE_FILE), ("sender", &EXAMPLE_SENDER), ("date", &day_words(mail_date()))],
    );
    let mut content = column![typo::text(typo::caption(), caption).style(theme::text_3)].spacing(space::xs());
    content = match preview(&editor.spec()) {
        Some((path, why)) => content
            .push(typo::text(typo::body_strong(), path).wrapping(Wrapping::WordOrGlyph))
            .push(typo::text(typo::meta(), why).style(theme::text_2)),
        None => content.push(
            typo::text(typo::meta(), tr("The preview appears once every placeholder is known.")).style(theme::text_2),
        ),
    };
    container(content).width(Fill).padding(layout::panel_padding()).style(style::container::group).into()
}

/// The foot: "Delete target" for a stored one, "Cancel" and "Save".
fn foot(editor: &Editor) -> Element<'_, Message> {
    let mut foot = row![].spacing(space::s()).align_y(Alignment::Center);
    if editor.id.is_some() {
        foot = foot.push(widgets::text_button(None, tr("Delete target"), style::button::danger, Some(Message::Delete)));
    }
    foot.push(blank::horizontal())
        .push(widgets::text_button(None, tr("Cancel"), style::button::secondary, Some(Message::Cancel)))
        .push(widgets::text_button(None, tr("Save"), style::button::primary, Some(Message::Save)))
        .into()
}

/// One field of the editor: its label to the left (above it in narrow windows), the field, and
/// under it a note or the mistake.
fn form_row<'a>(
    label: &'a str,
    control: impl Into<Element<'a, Message>>,
    note: Option<Element<'a, Message>>,
    stacked: bool,
) -> Element<'a, Message> {
    let field = column![control.into()].push(note).spacing(space::xs()).width(Fill);
    let label = typo::text(typo::body(), label).style(theme::text_2).wrapping(Wrapping::None);
    if stacked {
        return column![label, field].spacing(space::xs()).into();
    }
    // The label on the line of the field: as high as a field, its text in the middle.
    let label = container(label).width(label_width()).center_y(size::control());
    row![label, field].spacing(space::m()).into()
}

/// The room of the labels left of the fields: as wide as those of the properties, with room for
/// "If file exists".
fn label_width() -> f32 {
    widgets::prop_label_width() + space::xl()
}

/// A field's note: the mistake in the colour of mistakes once "Save" was pressed, else `note`.
fn note<'a>(mistake: Option<String>, note: Option<&'a str>) -> Option<Element<'a, Message>> {
    match mistake {
        Some(mistake) => Some(
            typo::text(typo::meta(), mistake)
                .style(|t: &Theme| iced::widget::text::Style { color: Some(palette(t).error_text) })
                .wrapping(Wrapping::WordOrGlyph)
                .into(),
        ),
        None => note.map(|note| typo::text(typo::meta(), note).style(theme::text_2).into()),
    }
}

/// A scheme's one-line editor with "Insert ▾" beside it. Enter saves.
fn scheme_field<'a>(editor: &'a Editor, scheme: Scheme, anchor: &Anchor) -> Element<'a, Message> {
    let content = match scheme {
        Scheme::Subfolders => &editor.subfolders,
        Scheme::FileName => &editor.file_name,
    };
    let input = iced::widget::text_editor(content)
        .id(field_id(scheme))
        .on_action(move |action| Message::Type(scheme, action))
        .key_binding(|press: KeyPress| match press.key.as_ref() {
            Key::Named(Named::Enter) if matches!(press.status, text_editor::Status::Focused { .. }) => {
                Some(Binding::Custom(Message::Save))
            }
            _ => Binding::from_key_press(press),
        })
        .font(typo::body().font)
        .size(typo::body().size)
        .line_height(typo::body().line_height)
        .padding(style::INPUT_PADDING)
        // One line that scrolls sideways with the cursor, like a text input.
        .wrapping(Wrapping::None)
        .height(field_height())
        .style(style::text_editor);
    let insert = widgets::select_button(anchor, tr("Insert"), Message::OpenInsert(scheme));
    row![input, insert].spacing(space::s()).align_y(Alignment::Center).into()
}

/// The height of a one-line field: one line of text and the padding of a field, as a text input.
fn field_height() -> f32 {
    typo::line_height(typo::body()) + style::INPUT_PADDING.top + style::INPUT_PADDING.bottom
}

/// The menu of "Insert ▾": the patterns of the field, then the placeholders with what they stand
/// for in the example. Scrolls in a low window.
fn insert_menu<'a>(scheme: Scheme, from: DateSource) -> Element<'a, Message> {
    let patterns = patterns(scheme)
        .iter()
        .map(|(label, pattern)| {
            let shown = if pattern.is_empty() { tr("Directly in the folder") } else { pattern };
            text_item(tr(label), Message::Pattern(scheme, pattern)).tag(shown, false)
        })
        .collect();
    let mut sections = vec![(tr("Patterns"), patterns)];
    for (group, placeholders) in PLACEHOLDERS {
        let items = placeholders
            .iter()
            .map(|(label, placeholder)| {
                text_item(tr(label), Message::Insert(scheme, placeholder)).tag(example_value(placeholder, from), false)
            })
            .collect();
        sections.push((tr(group), items));
    }
    let mut list = column![];
    for (at, (title, items)) in sections.into_iter().enumerate() {
        if at > 0 {
            list = list.push(separator());
        }
        let items: Vec<MenuItem<'a, Message>> = items;
        list = list.push(caption(title)).extend(items.into_iter().map(widgets::menu_item));
    }
    // Wider than a menu: each entry shows its label and, at the right, the pattern or the value.
    menu_card(list, size::panel())
}

/// An entry of the "Insert" menu: words only, as the design has them; what it inserts stands at the
/// right ([`MenuItem::tag`]).
fn text_item<'a>(label: &'a str, message: Message) -> MenuItem<'a, Message> {
    MenuItem { mark: None, ..MenuItem::new(Icon::Plus, label, message) }
}

/// The most entries a menu of the editor shows before it scrolls, so it opens next to its button.
const MENU_ROWS: f32 = 12.0;

/// A menu card of the editor: `list` on the menu surface, `width` wide, scrolling inside once it is
/// higher than [`MENU_ROWS`] entries.
fn menu_card<'a>(list: iced::widget::Column<'a, Message>, width: f32) -> Element<'a, Message> {
    let list = scrollable(container(list).padding(Padding::ZERO.right(space::s())))
        .height(Length::Shrink)
        .style(style::scrollable);
    container(container(list).max_height(style::MENU_ITEM_HEIGHT * MENU_ROWS))
        .width(width)
        .padding(style::MENU_INSET)
        .style(style::container::menu)
        .into()
}

/// The menu of "Add condition": the categories (the paper trail first, the rest on "More
/// categories"), the file types, the senders and a field for one more. A ticked entry removes its
/// condition again; the menu stays open, so several can be ticked.
fn conditions_menu<'a>(editor: &'a Editor, data: &'a Data, more: bool) -> Element<'a, Message> {
    let when = &editor.suggest;
    let tick = |on: bool| if on { Icon::SquareCheck } else { Icon::Square };
    let category_item = |category: Category| {
        let on = when.categories.contains(&category);
        let message = if on {
            Message::RemoveCondition(Condition::Category(category))
        } else {
            Message::ToggleCategory(category)
        };
        widgets::menu_item(MenuItem::new(tick(on), data.category_name(category), message))
    };
    let (paper, rest) = categories(data);
    let mut list = column![caption(tr("Mail category"))];
    list = list.extend(paper.into_iter().map(category_item));
    if more {
        list = list.extend(rest.into_iter().map(category_item));
    } else {
        list = list.push(widgets::menu_item(MenuItem::new(
            Icon::Ellipsis,
            tr("More categories"),
            Message::MoreCategories,
        )));
    }
    list = list.push(separator()).push(caption(tr("File type")));
    list = list.extend([FileKind::Pdf, FileKind::Image, FileKind::Office].into_iter().map(|kind| {
        let on = when.kinds.contains(&kind);
        widgets::menu_item(MenuItem::new(tick(on), kind_label(kind), Message::ToggleKind(kind)))
    }));
    list = list.push(separator()).push(caption(tr("Sender")));
    list = list.extend(when.senders.iter().map(|sender| {
        let remove = Message::RemoveCondition(Condition::Sender(sender.clone()));
        widgets::menu_item(MenuItem::new(Icon::SquareCheck, sender.as_str(), remove))
    }));
    let typed =
        field(tr("Address or domain"), &editor.sender).on_input(Message::SenderTyped).on_submit(Message::AddSender);
    let can_add = !editor.sender.trim().is_empty() && when.senders.len() < SENDERS_MAX;
    let add = widgets::text_button(None, tr("Add"), style::button::secondary, can_add.then_some(Message::AddSender));
    list = list.push(
        container(row![typed, add].spacing(space::s()).align_y(Alignment::Center)).padding(Padding::new(space::xs())),
    );
    menu_card(list, widgets::menu_width())
}

/// A section title of a menu card, as [`widgets::menu`] draws it.
fn caption<'a>(title: &'a str) -> Element<'a, Message> {
    container(typo::text(typo::caption(), title).style(theme::text_3))
        .padding(Padding::new(space::s()).bottom(space::xs()))
        .into()
}

/// The line between two sections of a menu card, as [`widgets::menu`] draws it.
fn separator<'a>() -> Element<'a, Message> {
    container(rule::horizontal(size::HAIRLINE).style(style::rule)).padding([space::xs(), 0.0]).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An absolute folder on every system: the checks of the page ask for one.
    fn folder(name: &str) -> String {
        std::env::temp_dir().join(name).display().to_string()
    }

    fn invoices() -> SaveTarget {
        let mut spec = SaveTargetSpec::new("Invoices", folder("Invoices"));
        spec.suggest.categories = vec![Category::Invoice];
        SaveTarget { id: "inv".into(), spec }
    }

    fn data() -> Data {
        Data { save_targets: vec![invoices()], downloads: "/home/anna/Downloads".into(), ..Data::default() }
    }

    fn editor(state: &State) -> &Editor {
        state.editor.as_ref().expect("the editor is open")
    }

    fn sent(action: Action<AppAction>) -> Request {
        match action {
            Action::Send(mut requests) if requests.len() == 1 => requests.remove(0),
            other => panic!("expected one request, got {other:?}"),
        }
    }

    /// Types `text` into the field as the keyboard would, character by character.
    fn type_in(state: &mut State, data: &Data, scheme: Scheme, text: &str) {
        for c in text.chars() {
            let _ = update(state, Message::Type(scheme, text_editor::Action::Edit(Edit::Insert(c))), data);
        }
    }

    /// Empties a scheme field: select everything, then delete.
    fn clear(state: &mut State, data: &Data, scheme: Scheme) {
        let _ = update(state, Message::Type(scheme, text_editor::Action::SelectAll), data);
        let _ = update(state, Message::Type(scheme, text_editor::Action::Edit(Edit::Backspace)), data);
    }

    /// Design review of #290: "Click on an attachment ▾" is a menu button like "Insert ▾". Its menu
    /// is the page's overlay while open; a choice, Escape and a click beside it close it again.
    #[test]
    fn the_click_menu_opens_and_closes() {
        let mut state = State::default();
        let data = data();
        assert!(overlay(&state, &data, SizeClass::Wide).is_none() && escape(&state).is_none());
        let _ = update(&mut state, Message::OpenClick, &data);
        assert!(state.click_menu && overlay(&state, &data, SizeClass::Wide).is_some());
        assert!(matches!(escape(&state), Some(Message::CloseMenu)), "Escape closes the menu first");
        let _ = update(&mut state, Message::CloseMenu, &data);
        assert!(!state.click_menu && escape(&state).is_none(), "then Escape is the window's again");
        let _ = update(&mut state, Message::OpenClick, &data);
        let _ = update(&mut state, Message::OpenClick, &data);
        assert!(!state.click_menu, "the button closes its menu, too");
        let _ = update(&mut state, Message::OpenClick, &data);
        let _ = update(&mut state, Message::Click(AttachmentClick::SaveAs), &data);
        assert!(!state.click_menu, "a choice closes it");
        let choices = click_choices(&data);
        assert_eq!(chosen_click(&choices, &data), Some(0), "the demo's setting opens the file");
        let _: Element<'_, Message> = view(&state, &data);
    }

    /// The choice of "Click on an attachment" is one `set_setting` with its key; a target is sent
    /// by its id, the others by their word.
    #[test]
    fn the_click_choice_is_sent_as_its_setting() {
        let mut state = State::default();
        let data = data();
        let words: Vec<String> = click_choices(&data).iter().map(ToString::to_string).collect();
        assert_eq!(
            words,
            [
                tr("Open").to_string(),
                tr("Save to Downloads").to_string(),
                tr("Save as…").to_string(),
                trf("Save to {target}", &[("target", &"Invoices")]),
                tr("Suggested target, else open").to_string(),
            ]
        );
        let target = update(&mut state, Message::Click(AttachmentClick::Target("inv".into())), &data);
        assert_eq!(
            sent(target),
            Request::SetSetting { key: SettingKey::MailAttachmentClick, value: serde_json::json!({"target": "inv"}) }
        );
        let suggested = update(&mut state, Message::Click(AttachmentClick::Suggested), &data);
        assert_eq!(
            sent(suggested),
            Request::SetSetting { key: SettingKey::MailAttachmentClick, value: serde_json::json!("suggested") }
        );
    }

    /// "Change…" asks the window for its chooser; the folder chosen there goes out as
    /// `mail_downloads_folder`.
    #[test]
    fn change_sends_the_chosen_downloads_folder() {
        let mut state = State::default();
        let data = data();
        assert_eq!(
            update(&mut state, Message::ChangeDownloads, &data),
            Action::App(AppAction::PickFolder(FolderFor::Downloads))
        );
        let chosen = update(&mut state, Message::FolderChosen(FolderFor::Downloads, "/home/anna/Saved".into()), &data);
        assert_eq!(
            sent(chosen),
            Request::SetSetting { key: SettingKey::MailDownloadsFolder, value: serde_json::json!("/home/anna/Saved") }
        );
    }

    /// A new target is sent whole: every field as it was set in the editor, the folder from the
    /// chooser.
    #[test]
    fn a_new_target_is_created_with_every_field() {
        let mut state = State::default();
        let data = data();
        assert_eq!(update(&mut state, Message::Add, &data), Action::Focus(NAME_FIELD));
        let _ = update(&mut state, Message::Name("Receipts ".into()), &data);
        assert_eq!(
            update(&mut state, Message::ChooseFolder, &data),
            Action::App(AppAction::PickFolder(FolderFor::Target))
        );
        let _ = update(&mut state, Message::FolderChosen(FolderFor::Target, folder("Receipts").into()), &data);
        let _ = update(&mut state, Message::Pattern(Scheme::Subfolders, "{year}/{quarter}"), &data);
        clear(&mut state, &data, Scheme::FileName);
        type_in(&mut state, &data, Scheme::FileName, "{date}_{name}.{ext}");
        let _ = update(&mut state, Message::DateFrom(DateSource::FileName), &data);
        let _ = update(&mut state, Message::ToggleCategory(Category::Receipt), &data);
        let _ = update(&mut state, Message::ToggleKind(FileKind::Pdf), &data);
        let _ = update(&mut state, Message::SenderTyped(" Shop.Example ".into()), &data);
        let _ = update(&mut state, Message::AddSender, &data);
        let _ = update(&mut state, Message::OnConflict(OnConflict::SkipIdentical), &data);
        let saved = update(&mut state, Message::Save, &data);
        let target = SaveTargetSpec {
            name: "Receipts".into(),
            folder: folder("Receipts"),
            subfolders: "{year}/{quarter}".into(),
            file_name: "{date}_{name}.{ext}".into(),
            date_from: DateSource::FileName,
            suggest: SuggestWhen {
                categories: vec![Category::Receipt],
                kinds: vec![FileKind::Pdf],
                senders: vec!["shop.example".into()],
            },
            on_conflict: OnConflict::SkipIdentical,
        };
        assert_eq!(sent(saved), Request::CreateSaveTarget { target });
        assert!(!state.is_editing(), "the dialog closes, so a second Save makes no twin");
    }

    /// A stored target opens with its fields and is sent back under its id.
    #[test]
    fn a_stored_target_is_updated_under_its_id() {
        let mut state = State::default();
        let data = data();
        let _ = update(&mut state, Message::Edit("inv".into()), &data);
        assert_eq!(editor(&state).spec(), invoices().spec, "the editor holds the target as stored");
        let _ = update(&mut state, Message::RemoveCondition(Condition::Category(Category::Invoice)), &data);
        let mut target = invoices().spec;
        target.suggest.categories.clear();
        assert_eq!(
            sent(update(&mut state, Message::Save, &data)),
            Request::UpdateSaveTarget { id: "inv".into(), target }
        );
        // A target the data no longer holds opens nothing.
        let _ = update(&mut state, Message::Edit("gone".into()), &data);
        assert!(!state.is_editing());
    }

    /// "Insert" puts the placeholder where the cursor is, not at the end, and the cursor goes back
    /// into the field.
    #[test]
    fn insert_puts_the_placeholder_at_the_cursor() {
        let mut state = State::default();
        let data = data();
        let _ = update(&mut state, Message::Add, &data);
        let start = text_editor::Action::Move(Motion::DocumentStart);
        let _ = update(&mut state, Message::Type(Scheme::Subfolders, start), &data);
        let _ = update(&mut state, Message::OpenInsert(Scheme::Subfolders), &data);
        assert_eq!(editor(&state).menu, Some(Menu::Insert(Scheme::Subfolders)));
        let inserted = update(&mut state, Message::Insert(Scheme::Subfolders, "{sender}/"), &data);
        assert_eq!(inserted, Action::Focus(SUBFOLDERS_FIELD));
        assert_eq!(editor(&state).spec().subfolders, "{sender}/{year}/{month}");
        assert_eq!(editor(&state).menu, None, "the menu closes");
        // A pattern replaces the whole field.
        let _ = update(&mut state, Message::Pattern(Scheme::Subfolders, ""), &data);
        assert_eq!(editor(&state).spec().subfolders, "");
    }

    /// The fields hold one line: Enter and line breaks in pasted text never get in.
    #[test]
    fn a_scheme_stays_on_one_line() {
        let mut state = State::default();
        let data = data();
        let _ = update(&mut state, Message::Add, &data);
        let _ = update(&mut state, Message::Type(Scheme::FileName, text_editor::Action::Edit(Edit::Enter)), &data);
        let paste = text_editor::Action::Edit(Edit::Paste(Arc::new("_\n{day}".into())));
        let _ = update(&mut state, Message::Type(Scheme::FileName, paste), &data);
        assert_eq!(editor(&state).spec().file_name, "{name}.{ext}_{day}");
    }

    /// The preview follows the fields: the mail's date by default, the date in the name with
    /// "File name, else mail", and the suggestion of the conditions.
    #[test]
    fn the_preview_follows_the_fields() {
        let mut spec = SaveTargetSpec::new("Invoices", "/home/anna/Invoices");
        let (path, why) = preview(&spec).expect("a preview");
        let expected = |folders: &[&str]| {
            let mut path = PathBuf::from("/home/anna/Invoices");
            path.extend(folders);
            path.push(EXAMPLE_FILE);
            path.display().to_string()
        };
        assert_eq!(path, expected(&["2026", "04"]));
        assert!(why.contains(tr("Not suggested for this file.")), "{why}");
        spec.date_from = DateSource::FileName;
        spec.suggest.categories = vec![Category::Invoice];
        let (path, why) = preview(&spec).expect("a preview");
        assert_eq!(path, expected(&["2026", "03"]));
        assert!(why.contains(tr("Suggested for this file.")), "{why}");
        spec.subfolders = "{sender}/{yaer}".into();
        assert_eq!(preview(&spec), None, "no path while a placeholder is unknown");
    }

    /// Mistakes show only after the first "Save", which then sends nothing and puts the cursor into
    /// the first field with one.
    #[test]
    fn mistakes_show_only_after_saving() {
        let mut state = State::default();
        let data = data();
        let _ = update(&mut state, Message::Add, &data);
        clear(&mut state, &data, Scheme::Subfolders);
        type_in(&mut state, &data, Scheme::Subfolders, "{yaer}");
        assert!(!editor(&state).tried, "nothing shown before Save");
        assert_eq!(update(&mut state, Message::Save, &data), Action::Focus(NAME_FIELD));
        let mistakes = editor(&state).mistakes(&data);
        assert_eq!(mistakes.name.as_deref(), Some(tr("Enter a name.")));
        assert_eq!(mistakes.folder.as_deref(), Some(tr("Choose a folder.")));
        let unknown =
            trf("“{placeholder}” is not a placeholder. Insert lists them all.", &[("placeholder", &"{yaer}")]);
        assert_eq!(mistakes.subfolders, Some(unknown));
        assert!(editor(&state).tried && state.is_editing(), "the dialog stays open");

        let _ = update(&mut state, Message::Name("invoices".into()), &data);
        let _ = update(&mut state, Message::Folder("Documents".into()), &data);
        let mistakes = editor(&state).mistakes(&data);
        assert_eq!(mistakes.name.as_deref(), Some(tr("Another save target has this name.")));
        assert_eq!(mistakes.folder.as_deref(), Some(tr("Choose a folder, or enter its full path.")));
        assert_eq!(update(&mut state, Message::Save, &data), Action::Focus(NAME_FIELD));
    }

    /// "Delete target" asks first; only the answer sends the removal.
    #[test]
    fn delete_asks_before_it_removes() {
        let mut state = State::default();
        let data = data();
        let _ = update(&mut state, Message::Edit("inv".into()), &data);
        assert_eq!(update(&mut state, Message::Delete, &data), Action::None);
        assert!(matches!(escape(&state), Some(Message::CancelDelete)));
        let _ = update(&mut state, Message::CancelDelete, &data);
        let _ = update(&mut state, Message::Delete, &data);
        assert_eq!(
            sent(update(&mut state, Message::ConfirmDelete, &data)),
            Request::RemoveSaveTarget { id: "inv".into() }
        );
        assert!(!state.is_editing());
        // A new target has nothing to delete.
        let _ = update(&mut state, Message::Add, &data);
        let _ = update(&mut state, Message::Delete, &data);
        assert!(!editor(&state).deleting);
    }

    /// The conditions' menu ticks and unticks; a sender is added once, in lower case.
    #[test]
    fn conditions_are_added_and_removed() {
        let mut state = State::default();
        let data = data();
        let _ = update(&mut state, Message::Add, &data);
        let _ = update(&mut state, Message::OpenConditions, &data);
        let _ = update(&mut state, Message::MoreCategories, &data);
        assert_eq!(editor(&state).menu, Some(Menu::Conditions { more: true }));
        let _ = update(&mut state, Message::ToggleCategory(Category::Invoice), &data);
        let _ = update(&mut state, Message::ToggleKind(FileKind::Office), &data);
        for typed in ["Bonn.DE", "bonn.de", "  "] {
            let _ = update(&mut state, Message::SenderTyped(typed.into()), &data);
            let _ = update(&mut state, Message::AddSender, &data);
        }
        assert_eq!(
            editor(&state).conditions(),
            [
                Condition::Category(Category::Invoice),
                Condition::Kind(FileKind::Office),
                Condition::Sender("bonn.de".into())
            ]
        );
        let _ = update(&mut state, Message::ToggleKind(FileKind::Office), &data);
        assert_eq!(editor(&state).suggest.kinds, Vec::new());
        // The same button closes its menu again; Escape closes the menu before the dialog.
        assert!(matches!(escape(&state), Some(Message::CloseMenu)));
        let _ = update(&mut state, Message::OpenConditions, &data);
        assert_eq!(editor(&state).menu, None);
        assert!(matches!(escape(&state), Some(Message::Cancel)));
        let _ = update(&mut state, Message::Cancel, &data);
        assert!(escape(&state).is_none(), "without a dialog the settings close as before");
    }

    /// The page, the editor with each menu and the question build, narrow and wide.
    #[test]
    fn the_page_and_the_dialog_build() {
        let mut state = State::default();
        let data = data();
        let _: Element<'_, Message> = view(&state, &Data::default());
        let _: Element<'_, Message> = view(&state, &data);
        assert!(overlay(&state, &data, SizeClass::Wide).is_none());
        let _ = update(&mut state, Message::Edit("inv".into()), &data);
        for menu in [None, Some(Menu::Insert(Scheme::FileName)), Some(Menu::Conditions { more: true })] {
            if let Some(editor) = state.editor.as_mut() {
                editor.menu = menu;
            }
            for class in [SizeClass::Narrow, SizeClass::Medium, SizeClass::Wide] {
                assert!(overlay(&state, &data, class).is_some());
            }
        }
        let _ = update(&mut state, Message::Delete, &data);
        assert!(overlay(&state, &data, SizeClass::Wide).is_some());
        // Every placeholder of "Insert" is one the scheme knows.
        for (_, placeholders) in PLACEHOLDERS {
            for (_, placeholder) in placeholders {
                assert!(scheme_mistake(Scheme::FileName, placeholder).is_none(), "{placeholder}");
                assert!(!example_value(placeholder, DateSource::Mail).is_empty(), "{placeholder}");
            }
        }
        for scheme in [Scheme::Subfolders, Scheme::FileName] {
            for (_, pattern) in patterns(scheme) {
                assert!(scheme_mistake(scheme, pattern).is_none(), "{pattern}");
            }
        }
    }
}
