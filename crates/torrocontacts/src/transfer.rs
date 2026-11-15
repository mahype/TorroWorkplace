//! vCard files in and out (issue #725): "Import vCard…" and "Export…" in the "…" of the header,
//! "Export vCard…" in the "…" of a contact.
//!
//! Import: the system's dialog picks a `.vcf` file, the dialog "Import vCard" names it, lets the person
//! choose the address book and shows what a trial run of `contacts.import_vcards` (`dry_run`) found;
//! "Import {n}" runs it for real. The window never reads the file: the core does, by its path (the demo's
//! backend reads it for the demo, [`crate::backend`]).
//!
//! Export: what the view shows ([`Scope`]) goes to `contacts.export_vcards`, and the window writes the
//! answer to exactly the file the person chose in the system's save dialog – nothing else.

use std::path::{Path, PathBuf};

use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, row};
use iced::{Alignment, Element, Fill};
use torro_contacts::{AddressBook, ContactsError, ContactsSource, Imported};
use torro_core::i18n::{tr, trf};
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{self, space};
use torro_gui::widgets::{self, Anchor};
use torro_gui::{dialog, layout, style, typo};
use torro_ipc::contacts::MAX_IMPORT_BYTES;

use crate::app::{Message, failed};
use crate::backend::Backend;
use crate::list::tinted;

/// The dialog "Import vCard" after the file was chosen.
#[derive(Debug, Clone, PartialEq)]
pub struct Import {
    /// The file the person chose; the core reads it.
    pub path: PathBuf,
    /// The address book the cards go into (a writable one).
    pub book: String,
    /// The number of the trial run whose answer the dialog waits for ([`Message::ImportCounted`]): an
    /// answer for another book (an older run) is dropped.
    pub asked: u64,
    /// What the last trial run found for `book`; `None` while it runs.
    pub counts: Option<Imported>,
    /// The import runs: the buttons wait.
    pub busy: bool,
    /// Why the trial run or the import failed, for people.
    pub error: Option<String>,
}

impl Import {
    /// The name of the file, as the dialog shows it.
    pub fn file_name(&self) -> String {
        self.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
    }

    /// How many contacts "Import" makes; 0 while counting or after a failure.
    pub fn new_count(&self) -> u32 {
        match (&self.counts, &self.error) {
            (Some(counts), None) => counts.created,
            _ => 0,
        }
    }
}

/// What "Export…" sends to `contacts.export_vcards`: a book, a group of a book, every book not hidden
/// (all empty), or the contacts `ids`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Scope {
    pub book: Option<String>,
    pub group: Option<String>,
    pub ids: Vec<String>,
}

/// The system's dialog "Import vCard": a `.vcf` file, `None` when the person cancels.
pub async fn choose_vcard() -> Option<PathBuf> {
    let file = rfd::AsyncFileDialog::new()
        .set_title(tr("Import vCard"))
        .add_filter(tr("vCard files"), &["vcf", "vcard"])
        .pick_file()
        .await?;
    Some(file.path().to_path_buf())
}

/// A trial run (`dry_run`) or the import of the file at `path` into `book`.
pub async fn import(backend: Backend, book: String, path: PathBuf, dry_run: bool) -> Result<Imported, ContactsError> {
    let path = path.to_string_lossy().into_owned();
    backend.import_vcards(book, None, Some(path), dry_run).await
}

/// "Export…": the system's save dialog starting with `name`, then the cards of `scope`, written to the
/// chosen file. `None` when the person cancels; else how many contacts went out or why nothing did.
pub async fn export(backend: Backend, scope: Scope, name: String) -> Option<Result<u32, String>> {
    let file = rfd::AsyncFileDialog::new()
        .set_title(tr("Export vCard"))
        .set_file_name(name)
        .add_filter(tr("vCard files"), &["vcf"])
        .save_file()
        .await?;
    Some(write_cards(&backend, scope, file.path()).await)
}

/// The cards of `scope` written to `path`: the only file the window writes.
pub async fn write_cards(backend: &Backend, scope: Scope, path: &Path) -> Result<u32, String> {
    let (data, count) = backend.export_vcards(scope.book, scope.group, scope.ids).await.map_err(|err| failed(&err))?;
    std::fs::write(path, data).map_err(|_| tr("The file could not be saved.").to_owned())?;
    Ok(count)
}

/// The name the save dialog suggests for the cards of `what` ("Anna Berg.vcf", "contacts.vcf"): without
/// the signs a file name may not hold.
pub fn file_name(what: Option<&str>) -> String {
    let cleaned: String =
        what.unwrap_or_default()
            .chars()
            .map(|c| {
                if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                    ' '
                } else {
                    c
                }
            })
            .collect();
    let cleaned = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let cleaned = cleaned.trim_matches('.');
    if cleaned.is_empty() { "contacts.vcf".to_owned() } else { format!("{cleaned}.vcf") }
}

/// Why an import failed, for people: a file too large says how large one may be.
pub fn import_failed(err: &ContactsError) -> String {
    match err {
        ContactsError::TooLarge => {
            let most = (MAX_IMPORT_BYTES / (1024 * 1024)).to_string();
            trf("The file is too large to import. It may hold at most {size} MB.", &[("size", &most)])
        }
        other => failed(other),
    }
}

/// The dialog "Import vCard": the file, the address book, what the trial run found, Cancel and
/// "Import {n}". `books` are the writable ones; `anchor` is the one of the book's menu.
pub fn dialog<'a>(import: &'a Import, books: &[&'a AddressBook], anchor: &Anchor) -> Element<'a, Message> {
    let head = typo::text(typo::title(), tr("Import vCard")).wrapping(Wrapping::WordOrGlyph);
    let file = row![
        tinted(Icon::FileText, icon::SIZE, |p| p.text_3),
        typo::text(typo::body_strong(), import.file_name()).wrapping(Wrapping::WordOrGlyph).width(Fill),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    let shown = books.iter().find(|b| b.id == import.book).map(|b| b.display_name.clone()).unwrap_or_default();
    // With one book there is nothing to choose: its name stands alone.
    let choice: Element<'a, Message> = if books.len() >= 2 && !import.busy {
        widgets::anchored(
            anchor,
            widgets::select_button(anchor, shown, Message::ToggleMenu(crate::app::Menu::ImportBook)),
        )
    } else {
        typo::text(typo::body(), shown).wrapping(Wrapping::WordOrGlyph).into()
    };
    let book = row![typo::text(typo::body(), tr("Address book")).style(theme::text_2), choice]
        .spacing(space::m())
        .align_y(Alignment::Center);
    let mut body = column![file, book].spacing(space::l());
    body = body.push(match (&import.error, &import.counts) {
        (Some(error), _) => problem(error.as_str()),
        (None, None) => typo::text(typo::body(), tr("Reading the file…")).style(theme::text_3).into(),
        (None, Some(counts)) => figures(counts),
    });
    let n = import.new_count();
    let go: Element<'a, Message> = if import.busy {
        widgets::busy_button(tr("Importing…"), style::button::secondary)
    } else {
        widgets::text_button(
            Some(Icon::Download),
            trf("Import {n}", &[("n", &n.to_string())]),
            style::button::secondary,
            (n > 0).then_some(Message::ImportConfirm),
        )
    };
    let foot = row![
        Space::new().width(Fill),
        widgets::text_button(None, tr("Cancel"), style::button::ghost, (!import.busy).then_some(Message::CloseDialog)),
        go,
    ]
    .spacing(space::s());
    container(dialog::frame(head, body, foot, space::l()))
        .width(Fill)
        .padding(layout::panel_padding())
        .style(style::container::popover)
        .into()
}

/// "12 new", "3 already in your contacts", "1 skipped": the last two only when there are some.
fn figures<'a>(counts: &Imported) -> Element<'a, Message> {
    let line = |which: Icon, words: String, strong: bool| -> Element<'a, Message> {
        let level = if strong { typo::body_strong() } else { typo::body() };
        row![tinted(which, icon::SIZE, |p| p.text_3), typo::text(level, words).wrapping(Wrapping::WordOrGlyph)]
            .spacing(space::s())
            .align_y(Alignment::Center)
            .into()
    };
    let mut lines =
        column![line(Icon::UserPlus, trf("{n} new", &[("n", &counts.created.to_string())]), true)].spacing(space::s());
    if counts.duplicates > 0 {
        lines = lines.push(line(
            Icon::Users,
            trf("{n} already in your contacts", &[("n", &counts.duplicates.to_string())]),
            false,
        ));
    }
    if counts.skipped > 0 {
        lines = lines.push(line(Icon::CircleDashed, trf("{n} skipped", &[("n", &counts.skipped.to_string())]), false));
    }
    if counts.created == 0 {
        lines = lines.push(
            typo::text(typo::meta(), tr("Nothing new to import into this address book."))
                .style(theme::text_2)
                .wrapping(Wrapping::WordOrGlyph),
        );
    }
    lines.into()
}

/// A failure in a dialog: the alert icon in the error colour and the words.
pub fn problem<'a>(words: &'a str) -> Element<'a, Message> {
    row![
        tinted(Icon::CircleAlert, icon::SIZE_SM, |p| p.error),
        typo::text(typo::meta(), words).style(theme::text_2).wrapping(Wrapping::WordOrGlyph).width(Fill),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_hold_no_path() {
        assert_eq!(file_name(Some("Anna Berg")), "Anna Berg.vcf");
        assert_eq!(file_name(Some("a/b: c")), "a b c.vcf");
        assert_eq!(file_name(Some("..")), "contacts.vcf");
        assert_eq!(file_name(None), "contacts.vcf");
    }

    #[test]
    fn a_failed_run_imports_nothing() {
        let mut import = Import {
            path: PathBuf::from("/tmp/kontakte.vcf"),
            book: "b".into(),
            asked: 1,
            counts: Some(Imported { created: 12, duplicates: 3, skipped: 1 }),
            busy: false,
            error: None,
        };
        assert_eq!(import.file_name(), "kontakte.vcf");
        assert_eq!(import.new_count(), 12);
        import.error = Some("x".into());
        assert_eq!(import.new_count(), 0);
        assert!(import_failed(&ContactsError::TooLarge).contains("20"));
    }
}
