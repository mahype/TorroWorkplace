//! Save targets for attachments (task M12.1, issue #290): a folder with a scheme for subfolders and
//! file names, where a click puts an attachment ("Invoices" → `…/Rechnungen/2026/03/`).
//!
//! The types are the API's contract: the core keeps the targets in `<data>/mail/save-targets.json`
//! (`mail.list_save_targets`, `mail.create_save_target` …), the settings page edits them, and the
//! reader offers them in the menu of an attachment. Resolving a scheme to a path ([`resolve`],
//! [`clean_segment`], [`date_in_name`]) and the rule that suggests a target ([`suggestion`],
//! [`FileKind::of`]) are pure functions without file access here as well (task M12.2), so the core
//! and the live preview of the settings page compute the same.
//!
//! Targets are the same for every mail account; `{account}` in a scheme tells them apart where a
//! person wants that. Only a person sets a target's folder (point 9 of "Was nie nach außen geht",
//! `docs/api.md`): it is a place on the computer the core writes to.
//!
//! On the wire like the settings: fields in camelCase (`fileName`, `dateFrom`), values in
//! snake_case (`file_name`, `keep_both`).

use std::fmt;

use chrono::{Datelike, NaiveDate};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::{Category, MailCategory};

/// The subfolder scheme of a new target: one folder per year, one per month in it.
pub const DEFAULT_SUBFOLDERS: &str = "{year}/{month}";

/// The file name scheme of a new target: the attachment's own name.
pub const DEFAULT_FILE_NAME: &str = "{name}.{ext}";

/// What a person sets for a save target, without its id ([`SaveTarget`]): what
/// `mail.create_save_target` and `mail.update_save_target` take whole.
///
/// The schemes hold placeholders in braces (`{year}`, `{month}`, `{day}`, `{month_name}`,
/// `{quarter}`, `{date}`, `{sender}`, `{sender_domain}`, `{account}`, `{subject}`, `{name}`,
/// `{ext}`); a `/` in `subfolders` starts a new folder, and the result always stays below
/// `folder`. Which values are allowed (lengths, an absolute folder, known placeholders) is the
/// core's check when it stores a target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SaveTargetSpec {
    /// What the target is called in menus: `Invoices`. One line, unique among the targets
    /// regardless of case.
    pub name: String,
    /// The folder the files go below, an absolute path. It must exist when a file is saved; the
    /// subfolders below it are made as needed. Only a person sets it.
    pub folder: String,
    /// The scheme of the subfolders below `folder`, `/` between them: `{year}/{month}` (the
    /// default). Empty puts every file into `folder` itself.
    #[serde(default = "default_subfolders")]
    pub subfolders: String,
    /// The scheme of the file name: `{name}.{ext}` (the default), `{date} {sender}.{ext}`.
    #[serde(default = "default_file_name")]
    pub file_name: String,
    /// Where the date of `{year}`, `{month}` … comes from.
    #[serde(default)]
    pub date_from: DateSource,
    /// When the target is suggested for an attachment; with no condition, never.
    #[serde(default)]
    pub suggest: SuggestWhen,
    /// What happens when a file of the same name is there already. A file is never overwritten.
    #[serde(default)]
    pub on_conflict: OnConflict,
}

impl SaveTargetSpec {
    /// A new target called `name` with its files below `folder`, and every other field as a new
    /// target starts: subfolders [`DEFAULT_SUBFOLDERS`], file name [`DEFAULT_FILE_NAME`], the date
    /// of the mail, never suggested, keep both files on a conflict.
    pub fn new(name: impl Into<String>, folder: impl Into<String>) -> SaveTargetSpec {
        SaveTargetSpec {
            name: name.into(),
            folder: folder.into(),
            subfolders: default_subfolders(),
            file_name: default_file_name(),
            date_from: DateSource::default(),
            suggest: SuggestWhen::default(),
            on_conflict: OnConflict::default(),
        }
    }
}

fn default_subfolders() -> String {
    DEFAULT_SUBFOLDERS.to_string()
}

fn default_file_name() -> String {
    DEFAULT_FILE_NAME.to_string()
}

/// A stored save target: the core's id and what a person set ([`SaveTargetSpec`], its fields on
/// the same level: `{"id": "…", "name": "Invoices", "folder": "/Users/anna/Rechnungen", …}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SaveTarget {
    /// The id the core gave the target (random hex); stays the same when it is changed.
    pub id: String,
    #[serde(flatten)]
    pub spec: SaveTargetSpec,
}

/// Where the date of a file comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum DateSource {
    /// `mail`: the day the mail was sent, on this computer's clock (the default).
    #[default]
    Mail,
    /// `file_name`: a date in the attachment's name (`2026-03-28`, `28.03.2026`, `20260328`),
    /// else the day of the mail ("File name, else mail").
    FileName,
}

/// What happens when a file of the same name is in the folder already. Nothing is ever
/// overwritten.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum OnConflict {
    /// `keep_both`: the new file gets a number, `Invoice (2).pdf` (the default).
    #[default]
    KeepBoth,
    /// `skip_identical`: when the file there has the same content, nothing is saved (the answer
    /// lists it as skipped); otherwise both are kept as with `keep_both`.
    SkipIdentical,
}

/// When a target is suggested for an attachment ("Suggest when", variant V1 of the design): each
/// kind of condition that is set must fit, within one kind one hit is enough. With no condition at
/// all the target is never suggested.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct SuggestWhen {
    /// The mail is one of these categories (`invoice`, `receipt` …). A category the model only
    /// guessed suggests the target in the menu, but a click never saves by it alone.
    #[serde(default)]
    pub categories: Vec<Category>,
    /// The attachment is of one of these kinds.
    #[serde(default)]
    pub kinds: Vec<FileKind>,
    /// The mail comes from one of these: a whole address (`billing@stadtwerke-bonn.de`) or a domain
    /// (`stadtwerke-bonn.de`, which takes the domains below it too), regardless of case.
    #[serde(default)]
    pub senders: Vec<String>,
}

/// What kind of file an attachment is, for [`SuggestWhen::kinds`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    /// `pdf`: a PDF document.
    Pdf,
    /// `image`: a picture (PNG, JPEG, GIF, HEIC, WebP …).
    Image,
    /// `office`: a text, spreadsheet or presentation of an office suite (Word, Excel, PowerPoint,
    /// OpenDocument).
    Office,
}

impl FileKind {
    /// The kind of an attachment by its MIME type, else by the extension of its name (many mail
    /// programs send `application/octet-stream`): `application/pdf` or `.pdf` is [`FileKind::Pdf`];
    /// `image/*` or png, jpg, jpeg, gif, heic, webp is [`FileKind::Image`]; Word, Excel,
    /// PowerPoint and OpenDocument types or doc(x), xls(x), ppt(x), odt, ods, odp are
    /// [`FileKind::Office`]. `None` for anything else.
    ///
    /// Parameters of the MIME type (`; name=…`) and case do not matter.
    pub fn of(content_type: &str, name: Option<&str>) -> Option<FileKind> {
        let mime = content_type.split(';').next().unwrap_or_default().trim().to_ascii_lowercase();
        FileKind::by_mime(&mime).or_else(|| {
            let ext = split_name(name?).1.to_ascii_lowercase();
            FileKind::by_extension(&ext)
        })
    }

    fn by_mime(mime: &str) -> Option<FileKind> {
        // Prefixes, so the macro and template variants (`…ms-excel.sheet.macroEnabled.12`,
        // `…opendocument.text-template`) count as well.
        const OFFICE: [&str; 7] = [
            "application/msword",
            "application/vnd.ms-excel",
            "application/vnd.ms-powerpoint",
            "application/vnd.openxmlformats-officedocument.",
            "application/vnd.oasis.opendocument.text",
            "application/vnd.oasis.opendocument.spreadsheet",
            "application/vnd.oasis.opendocument.presentation",
        ];
        if mime == "application/pdf" {
            Some(FileKind::Pdf)
        } else if mime.starts_with("image/") {
            Some(FileKind::Image)
        } else if OFFICE.iter().any(|office| mime.starts_with(office)) {
            Some(FileKind::Office)
        } else {
            None
        }
    }

    fn by_extension(ext: &str) -> Option<FileKind> {
        match ext {
            "pdf" => Some(FileKind::Pdf),
            "png" | "jpg" | "jpeg" | "gif" | "heic" | "webp" => Some(FileKind::Image),
            "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "odp" => Some(FileKind::Office),
            _ => None,
        }
    }
}

/// What a scheme is filled with for one attachment: the facts of its mail and its own name.
/// The core builds them from the stored message (task M12.3), the settings page from its fixed
/// example (task M12.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFacts {
    /// The day the mail was sent, on this computer's clock (not UTC): the date of `{year}` … with
    /// [`DateSource::Mail`], and the fallback of [`DateSource::FileName`].
    pub mail_date: NaiveDate,
    /// The attachment's file name as the message gives it (`Rechnung_2026-03-28_4711.pdf`);
    /// `None` when it gives none. Data from outside: [`resolve`] cleans it.
    pub file_name: Option<String>,
    /// The attachment's MIME type (`application/pdf`), for [`FileKind::of`].
    pub content_type: String,
    /// The display name of the sender (`Stadtwerke Bonn`), when the header has one.
    pub sender_name: Option<String>,
    /// The address of the sender (`billing@stadtwerke-bonn.de`); empty when the mail has none.
    pub sender_address: String,
    /// The mail account's name for `{account}`: its display name, else its address.
    pub account: String,
    /// The subject of the mail; `{subject}` takes at most 60 characters of it.
    pub subject: String,
    /// The names of the twelve months, January first, in the language of the app (`torro-mail`
    /// knows no `tr`): `{month_name}` takes the one of the file's date, which with
    /// [`DateSource::FileName`] may be another month than the mail's.
    pub month_names: [String; 12],
}

/// Where [`resolve`] puts an attachment, below the target's folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    /// The subfolders below the target's folder, outermost first (`["2026", "03"]`); empty when
    /// the file goes into the folder itself. Each is one cleaned name ([`clean_segment`]): never
    /// empty, never `.` or `..`, never with a `/` or `\`.
    pub folders: Vec<String>,
    /// The file name, cleaned like a folder, at most 120 bytes with its extension kept; never
    /// empty (`attachment` when nothing else is left).
    pub file: String,
}

/// A scheme [`resolve`] cannot fill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SchemaError {
    /// A placeholder that does not exist (`{yaer}`, the name without braces), or a `{` without a
    /// `}` after it (the text after the `{`).
    Unknown(String),
}

impl fmt::Display for SchemaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SchemaError::Unknown(name) => write!(f, "unknown placeholder {{{name}}}"),
        }
    }
}

impl std::error::Error for SchemaError {}

/// The most a folder or file name may take, in bytes: below the 255 of every common file
/// system, with room for ` (99)` of "Keep both".
const MAX_SEGMENT_BYTES: usize = 120;

/// The most `{subject}` takes, in characters.
const MAX_SUBJECT_CHARS: usize = 60;

/// The name a file gets when neither the scheme nor the attachment leaves one.
const FALLBACK_FILE_NAME: &str = "attachment";

/// Where `spec` puts the attachment of `facts`: its subfolders and its file name.
///
/// Fills the placeholders of `spec.subfolders` and `spec.file_name` (see [`SaveTargetSpec`]) with
/// the values of the issue's table: `{year}` `2026`, `{month}` `03`, `{day}` `28`,
/// `{month_name}` from [`FileFacts::month_names`], `{quarter}` `Q1`, `{date}` `2026-03-28` – all
/// of the date [`SaveTargetSpec::date_from`] picks; `{sender}` the sender's name, else the
/// address; `{sender_domain}` the domain of the address, lower case; `{account}`; `{subject}` at
/// most 60 characters; `{name}` the attachment's name without, `{ext}` its extension without the
/// dot, lower case.
///
/// Folders come only from a `/` in the scheme, never from a value: the scheme is split first and
/// each piece is filled and cleaned on its own ([`clean_segment`]), so `../etc` in a subject
/// stays one name below the folder; empty pieces are left out. The file name keeps its extension
/// when it is shortened; a dot at its end (`{name}.{ext}` without an extension) goes. Is it empty,
/// the cleaned name of the attachment is taken, else `attachment`.
///
/// # Errors
///
/// [`SchemaError::Unknown`] for a placeholder that does not exist or a `{` without `}`, in
/// either scheme.
pub fn resolve(spec: &SaveTargetSpec, facts: &FileFacts) -> Result<Resolved, SchemaError> {
    let values = Values::of(spec.date_from, facts);
    let mut folders = Vec::new();
    for piece in spec.subfolders.split('/') {
        let folder = clean_segment(&fill(piece, &values)?);
        if !folder.is_empty() {
            folders.push(folder);
        }
    }
    let mut file = clean_name(&fill(&spec.file_name, &values)?, true);
    if file.is_empty() {
        file = facts.file_name.as_deref().map(|name| clean_name(name, true)).unwrap_or_default();
    }
    if file.is_empty() {
        file = FALLBACK_FILE_NAME.to_string();
    }
    Ok(Resolved { folders, file })
}

/// `text` as one folder or file name that every common system takes: `/` and `\` and the
/// characters Windows rejects (`< > : " | ? *`) become `_`; control characters (a tab or line
/// break as a space) and invisible ones (zero width, direction overrides) go; dots and spaces at
/// either end go, so it is never `.` or `..` and never hidden; a name Windows reserves (`CON`,
/// `NUL`, `COM1` …, also with an extension) gets a `_` before it (`_CON`); at most 120 bytes,
/// cut at a character. Empty stays empty (and so does a text of only dots and spaces).
pub fn clean_segment(text: &str) -> String {
    clean_name(text, false)
}

/// [`clean_segment`] for a file name: when it is longer than 120 bytes, the cut falls before its
/// extension (up to ten letters or digits after the last dot), so `Invoice….pdf` stays a PDF – as
/// [`resolve`] shortens the file name of a scheme. Empty stays empty.
pub fn clean_file_name(text: &str) -> String {
    clean_name(text, true)
}

/// The first date in a file name, in one of the forms `2026-03-28`, `28.03.2026` or `20260328`;
/// `None` when there is none.
///
/// Only real days between 1970 and 2099 count (`2026-02-30` or the number `47110312` do not), and
/// a digit right before or after the date stops it (`120260328` is no date).
pub fn date_in_name(name: &str) -> Option<NaiveDate> {
    let bytes = name.as_bytes();
    (0..bytes.len())
        .filter(|&at| bytes[at].is_ascii_digit() && (at == 0 || !bytes[at - 1].is_ascii_digit()))
        .find_map(|at| date_at(&bytes[at..]))
}

/// That a target is suggested for an attachment, and whether a click may save by it alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Suggestion {
    /// The suggestion is certain: the target has no category condition, or the mail's category
    /// is not only a guess of the model ([`MailCategory::is_suggestion`]). Only a sure suggestion
    /// is what the click action "Suggested target" saves to; the menu shows every one.
    pub sure: bool,
}

/// Whether a target with the conditions `when` is suggested for an attachment of `kind` in a mail
/// of `category` from `sender_address` (variant V1 of the design).
///
/// `None` when `when` has no condition at all. Otherwise each kind of condition that is set must
/// fit, within one kind one hit is enough: the mail's category is one of
/// [`SuggestWhen::categories`] (no category fits none); the attachment's kind is one of
/// [`SuggestWhen::kinds`]; the sender is one of [`SuggestWhen::senders`] – an entry with `@`
/// is a whole address, one without (or with a leading `@`) a domain that takes the domains below
/// it too (`stadtwerke-bonn.de` fits `billing@mail.stadtwerke-bonn.de`), regardless of case.
pub fn suggestion(
    when: &SuggestWhen,
    category: Option<MailCategory>,
    kind: Option<FileKind>,
    sender_address: &str,
) -> Option<Suggestion> {
    if when.categories.is_empty() && when.kinds.is_empty() && when.senders.is_empty() {
        return None;
    }
    let category_fits =
        when.categories.is_empty() || category.is_some_and(|found| when.categories.contains(&found.category));
    let kind_fits = when.kinds.is_empty() || kind.is_some_and(|kind| when.kinds.contains(&kind));
    let sender_fits = when.senders.is_empty() || when.senders.iter().any(|sender| sender_fits(sender, sender_address));
    if !(category_fits && kind_fits && sender_fits) {
        return None;
    }
    let guessed = !when.categories.is_empty() && category.is_some_and(MailCategory::is_suggestion);
    Some(Suggestion { sure: !guessed })
}

/// Whether `address` is the address or in the domain `sender` names (see [`suggestion`]).
fn sender_fits(sender: &str, address: &str) -> bool {
    let sender = sender.trim().to_lowercase();
    let address = address.trim().to_lowercase();
    if sender.is_empty() || address.is_empty() {
        return false;
    }
    if let Some(domain) = sender.strip_prefix('@') {
        return in_domain(&address, domain);
    }
    if sender.contains('@') { sender == address } else { in_domain(&address, &sender) }
}

fn in_domain(address: &str, domain: &str) -> bool {
    let Some((_, own)) = address.rsplit_once('@') else { return false };
    !domain.is_empty() && (own == domain || own.strip_suffix(domain).is_some_and(|above| above.ends_with('.')))
}

/// The values of the placeholders for one attachment and one date source.
struct Values<'a> {
    date: NaiveDate,
    facts: &'a FileFacts,
    name: &'a str,
    ext: String,
}

impl<'a> Values<'a> {
    fn of(from: DateSource, facts: &'a FileFacts) -> Values<'a> {
        let date = match from {
            DateSource::Mail => facts.mail_date,
            DateSource::FileName => facts.file_name.as_deref().and_then(date_in_name).unwrap_or(facts.mail_date),
        };
        let (name, ext) = facts.file_name.as_deref().map(split_name).unwrap_or_default();
        Values { date, facts, name, ext: ext.to_lowercase() }
    }

    /// The value of the placeholder `name`; `None` when there is no such placeholder.
    fn get(&self, placeholder: &str) -> Option<String> {
        let date = self.date;
        let facts = self.facts;
        Some(match placeholder {
            "year" => format!("{:04}", date.year()),
            "month" => format!("{:02}", date.month()),
            "day" => format!("{:02}", date.day()),
            "month_name" => facts.month_names.get(date.month0() as usize).cloned().unwrap_or_default(),
            "quarter" => format!("Q{}", date.month0() / 3 + 1),
            "date" => format!("{:04}-{:02}-{:02}", date.year(), date.month(), date.day()),
            "sender" => match facts.sender_name.as_deref().map(str::trim) {
                Some(name) if !name.is_empty() => name.to_string(),
                _ => facts.sender_address.trim().to_string(),
            },
            "sender_domain" => facts
                .sender_address
                .trim()
                .rsplit_once('@')
                .map(|(_, domain)| domain.to_lowercase())
                .unwrap_or_default(),
            "account" => facts.account.clone(),
            "subject" => {
                facts.subject.trim().chars().take(MAX_SUBJECT_CHARS).collect::<String>().trim_end().to_string()
            }
            "name" => self.name.to_string(),
            "ext" => self.ext.clone(),
            _ => return None,
        })
    }
}

/// `scheme` with its placeholders replaced. Values are put in as they are, never read as a scheme
/// again.
fn fill(scheme: &str, values: &Values<'_>) -> Result<String, SchemaError> {
    let mut out = String::new();
    let mut rest = scheme;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else { return Err(SchemaError::Unknown(after.to_string())) };
        let placeholder = &after[..close];
        out.push_str(&values.get(placeholder).ok_or_else(|| SchemaError::Unknown(placeholder.to_string()))?);
        rest = &after[close + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

/// A file name split into what comes before its extension and the extension without the dot:
/// `("Rechnung_2026-03-28_4711", "pdf")`. Only a short run of letters and digits after the last
/// dot counts as an extension, so `Report v1.2 final` has none; neither has `.bashrc`.
fn split_name(name: &str) -> (&str, &str) {
    match name.rsplit_once('.') {
        Some((stem, ext))
            if !stem.is_empty() && (1..=10).contains(&ext.len()) && ext.bytes().all(|b| b.is_ascii_alphanumeric()) =>
        {
            (stem, ext)
        }
        _ => (name, ""),
    }
}

/// [`clean_segment`], keeping the extension when it shortens with `keep_extension` (file names).
fn clean_name(text: &str, keep_extension: bool) -> String {
    let mapped: String = text.chars().filter_map(clean_char).collect();
    let name = shorten(trim_ends(&mapped), keep_extension);
    if is_reserved(&name) { shorten(&format!("_{name}"), keep_extension) } else { name }
}

/// What `c` becomes in a name: `_` for a separator or a character Windows rejects, a space for a
/// tab or line break, nothing for any other control or an invisible character.
fn clean_char(c: char) -> Option<char> {
    match c {
        '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*' => Some('_'),
        '\t' | '\n' | '\r' => Some(' '),
        c if c.is_control() || is_invisible(c) => None,
        c => Some(c),
    }
}

/// Characters that take no room but change what a name looks like: zero width ones and the
/// direction marks and overrides (`invoice\u{202E}fdp.exe` would show as `invoiceexe.pdf`).
fn is_invisible(c: char) -> bool {
    matches!(
        c,
        '\u{00AD}'
            | '\u{061C}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{FEFF}'
            | '\u{FFF9}'..='\u{FFFB}'
    )
}

/// `text` without dots and spaces at either end: Windows drops them at the end, and at the start
/// they make `.`, `..` or a hidden file.
fn trim_ends(text: &str) -> &str {
    text.trim_matches(['.', ' '])
}

/// `name` cut to [`MAX_SEGMENT_BYTES`] at a character, with its extension kept when
/// `keep_extension`, and without dots or spaces the cut leaves at its end.
fn shorten(name: &str, keep_extension: bool) -> String {
    if name.len() <= MAX_SEGMENT_BYTES {
        return name.to_string();
    }
    let ext = if keep_extension { split_name(name).1 } else { "" };
    let tail = if ext.is_empty() { String::new() } else { format!(".{ext}") };
    let stem = &name[..name.len() - tail.len()];
    let mut end = MAX_SEGMENT_BYTES - tail.len();
    while !stem.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{tail}", trim_ends(&stem[..end]))
}

/// Whether Windows reserves `name` for a device: `CON`, `PRN`, `AUX`, `NUL`, `COM0`–`COM9`,
/// `LPT0`–`LPT9`, in any case and with any extension (`nul.txt`).
fn is_reserved(name: &str) -> bool {
    let stem = name.split('.').next().unwrap_or_default().trim_end().to_ascii_uppercase();
    match stem.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" => true,
        _ => {
            let bytes = stem.as_bytes();
            bytes.len() == 4 && (stem.starts_with("COM") || stem.starts_with("LPT")) && bytes[3].is_ascii_digit()
        }
    }
}

/// The date at the start of `bytes`, in one of the forms of [`date_in_name`], with no digit right
/// after it.
fn date_at(bytes: &[u8]) -> Option<NaiveDate> {
    // (pattern, where year, month and day start); `9` stands for a digit.
    const FORMS: [(&[u8], usize, usize, usize); 3] =
        [(b"9999-99-99", 0, 5, 8), (b"99.99.9999", 6, 3, 0), (b"99999999", 0, 4, 6)];
    FORMS.iter().find_map(|&(pattern, year, month, day)| {
        let fits = bytes.len() >= pattern.len()
            && pattern
                .iter()
                .zip(bytes)
                .all(|(&want, &got)| if want == b'9' { got.is_ascii_digit() } else { want == got })
            && bytes.get(pattern.len()).is_none_or(|next| !next.is_ascii_digit());
        if !fits {
            return None;
        }
        let year = number(&bytes[year..year + 4]);
        let date =
            NaiveDate::from_ymd_opt(year as i32, number(&bytes[month..month + 2]), number(&bytes[day..day + 2]))?;
        (1970..=2099).contains(&year).then_some(date)
    })
}

/// The value of ASCII digits.
fn number(digits: &[u8]) -> u32 {
    digits.iter().fold(0, |value, digit| value * 10 + u32::from(*digit - b'0'))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_new_target_starts_with_the_defaults() {
        let spec = SaveTargetSpec::new("Invoices", "/Users/anna/Rechnungen");
        assert_eq!((spec.subfolders.as_str(), spec.file_name.as_str()), ("{year}/{month}", "{name}.{ext}"));
        assert_eq!((spec.date_from, spec.on_conflict), (DateSource::Mail, OnConflict::KeepBoth));
        assert_eq!(spec.suggest, SuggestWhen::default());
        // Left out on the wire, the same defaults.
        let sent: SaveTargetSpec =
            serde_json::from_value(json!({"name": "Invoices", "folder": "/Users/anna/Rechnungen"})).expect("reads");
        assert_eq!(sent, spec);
    }

    #[test]
    fn targets_travel_in_camel_case_with_snake_case_values() {
        let mut spec = SaveTargetSpec::new("Invoices", "/Users/anna/Rechnungen");
        spec.file_name = "{date} {sender}.{ext}".into();
        spec.date_from = DateSource::FileName;
        spec.on_conflict = OnConflict::SkipIdentical;
        spec.suggest = SuggestWhen {
            categories: vec![Category::Invoice],
            kinds: vec![FileKind::Pdf, FileKind::Office],
            senders: vec!["stadtwerke-bonn.de".into()],
        };
        let target = SaveTarget { id: "0a1b2c3d".into(), spec };
        let value = serde_json::to_value(&target).expect("writes");
        assert_eq!(
            value,
            json!({
                "id": "0a1b2c3d",
                "name": "Invoices",
                "folder": "/Users/anna/Rechnungen",
                "subfolders": "{year}/{month}",
                "fileName": "{date} {sender}.{ext}",
                "dateFrom": "file_name",
                "suggest": {"categories": ["invoice"], "kinds": ["pdf", "office"], "senders": ["stadtwerke-bonn.de"]},
                "onConflict": "skip_identical"
            })
        );
        assert_eq!(serde_json::from_value::<SaveTarget>(value).expect("reads"), target);
    }

    #[test]
    fn conditions_may_be_left_out() {
        let read: SuggestWhen = serde_json::from_value(json!({"kinds": ["image"]})).expect("reads");
        assert_eq!(read, SuggestWhen { kinds: vec![FileKind::Image], ..SuggestWhen::default() });
        for kind in [FileKind::Pdf, FileKind::Image, FileKind::Office] {
            let json = serde_json::to_value(kind).expect("writes");
            assert_eq!(serde_json::from_value::<FileKind>(json).expect("reads"), kind);
        }
    }

    // --- M12.2: resolving schemes and suggesting targets ---

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

    fn day(year: i32, month: u32, day: u32) -> NaiveDate {
        NaiveDate::from_ymd_opt(year, month, day).expect("a real day")
    }

    /// The example of the design: an invoice of 28 March in a mail of 2 April.
    fn facts() -> FileFacts {
        FileFacts {
            mail_date: day(2026, 4, 2),
            file_name: Some("Rechnung_2026-03-28_4711.PDF".into()),
            content_type: "application/pdf".into(),
            sender_name: Some("Stadtwerke Bonn".into()),
            sender_address: "Billing@Stadtwerke-Bonn.de".into(),
            account: "Privat".into(),
            subject: "Ihre Rechnung März 2026".into(),
            month_names: MONTHS.map(String::from),
        }
    }

    fn spec(subfolders: &str, file_name: &str) -> SaveTargetSpec {
        let mut spec = SaveTargetSpec::new("Invoices", "/Users/anna/Rechnungen");
        spec.subfolders = subfolders.into();
        spec.file_name = file_name.into();
        spec
    }

    /// The file name `scheme` gives for `facts`.
    fn file(scheme: &str, facts: &FileFacts) -> String {
        resolve(&spec("", scheme), facts).expect("resolves").file
    }

    #[test]
    fn every_placeholder_has_its_value() {
        let facts = facts();
        let cases = [
            ("{year}", "2026"),
            ("{month}", "04"),
            ("{day}", "02"),
            ("{month_name}", "April"),
            ("{quarter}", "Q2"),
            ("{date}", "2026-04-02"),
            ("{sender}", "Stadtwerke Bonn"),
            ("{sender_domain}", "stadtwerke-bonn.de"),
            ("{account}", "Privat"),
            ("{subject}", "Ihre Rechnung März 2026"),
            ("{name}", "Rechnung_2026-03-28_4711"),
            ("{ext}", "pdf"),
        ];
        for (scheme, value) in cases {
            assert_eq!(file(scheme, &facts), value, "{scheme}");
        }
        assert_eq!(file("{date} {sender}.{ext}", &facts), "2026-04-02 Stadtwerke Bonn.pdf");
    }

    #[test]
    fn the_default_schemes_file_by_year_and_month_under_the_own_name() {
        let resolved = resolve(&SaveTargetSpec::new("Invoices", "/x"), &facts()).expect("resolves");
        assert_eq!(
            resolved,
            Resolved { folders: vec!["2026".into(), "04".into()], file: "Rechnung_2026-03-28_4711.pdf".into() }
        );
    }

    #[test]
    fn the_sender_without_a_name_is_the_address_and_the_subject_is_cut() {
        let mut facts = facts();
        facts.sender_name = Some("  ".into());
        facts.subject = "x".repeat(70);
        assert_eq!(file("{sender}", &facts), "Billing@Stadtwerke-Bonn.de");
        assert_eq!(file("{subject}", &facts), "x".repeat(60));
        facts.sender_name = None;
        facts.sender_address = String::new();
        // Nothing left: the attachment's own name.
        assert_eq!(file("{sender}{sender_domain}", &facts), "Rechnung_2026-03-28_4711.PDF");
    }

    #[test]
    fn the_date_comes_from_the_file_name_in_all_three_forms_else_from_the_mail() {
        let mut spec = spec("{year}/{month_name}", "{date}");
        spec.date_from = DateSource::FileName;
        let mut facts = facts();
        for name in ["Rechnung_2026-03-28_4711.pdf", "Rechnung 28.03.2026.pdf", "R20260328.pdf"] {
            facts.file_name = Some(name.into());
            let resolved = resolve(&spec, &facts).expect("resolves");
            assert_eq!(
                resolved,
                Resolved { folders: vec!["2026".into(), "March".into()], file: "2026-03-28".into() },
                "{name}"
            );
        }
        for name in [Some("Rechnung_4711.pdf"), None] {
            facts.file_name = name.map(String::from);
            assert_eq!(resolve(&spec, &facts).expect("resolves").file, "2026-04-02", "{name:?}");
        }
        // With the mail as the source a date in the name does not count.
        facts.file_name = Some("Rechnung_2026-03-28.pdf".into());
        spec.date_from = DateSource::Mail;
        assert_eq!(resolve(&spec, &facts).expect("resolves").file, "2026-04-02");
    }

    #[test]
    fn a_date_in_a_name_is_a_real_day_not_inside_a_number() {
        assert_eq!(date_in_name("2026-03-28"), Some(day(2026, 3, 28)));
        assert_eq!(date_in_name("Beleg 28.03.2026 Bonn"), Some(day(2026, 3, 28)));
        assert_eq!(date_in_name("scan_20260328.jpg"), Some(day(2026, 3, 28)));
        // The first one counts.
        assert_eq!(date_in_name("01.02.2025 bis 2026-03-28"), Some(day(2025, 2, 1)));
        for none in [
            "120260328",
            "202603281",
            "2026-02-30",
            "32.01.2026",
            "47110312",
            "12026-03-28",
            "2026-03-281",
            "Invoice 4711",
        ] {
            assert_eq!(date_in_name(none), None, "{none}");
        }
        // After a number that is no date, a later one still counts.
        assert_eq!(date_in_name("120260328 20260329"), Some(day(2026, 3, 29)));
    }

    #[test]
    fn hostile_values_stay_one_name_below_the_folder() {
        let mut facts = facts();
        for subject in ["../../etc", "/etc/passwd", "..", "a\\..\\b", "C:\\Windows"] {
            facts.subject = subject.into();
            let resolved = resolve(&spec("{subject}", "{subject}.{ext}"), &facts).expect("resolves");
            assert!(resolved.folders.len() <= 1, "{subject}: {resolved:?}");
            for name in resolved.folders.iter().chain([&resolved.file]) {
                assert!(
                    !name.contains(['/', '\\']) && name != "." && name != ".." && !name.starts_with('.'),
                    "{subject}: {name}"
                );
            }
        }
        facts.subject = "../../etc".into();
        assert_eq!(resolve(&spec("{subject}", "x"), &facts).expect("resolves").folders, vec!["_.._etc".to_string()]);
        // Only a `/` in the scheme makes a folder; empty pieces go.
        facts.subject = "..".into();
        let resolved = resolve(&spec("/{year}//{subject}/", "x"), &facts).expect("resolves");
        assert_eq!(resolved.folders, vec!["2026".to_string()]);
    }

    #[test]
    fn names_lose_controls_invisible_and_windows_characters() {
        assert_eq!(clean_segment("Ihre\tRechnung\n\u{7}März"), "Ihre Rechnung März");
        assert_eq!(clean_segment("invoice\u{202E}fdp.exe"), "invoicefdp.exe");
        assert_eq!(clean_segment("a\u{200B}b\u{FEFF}"), "ab");
        assert_eq!(clean_segment(r#"Q1: "draft" <x>|y?*"#), "Q1_ _draft_ _x__y__");
        assert_eq!(clean_segment(" . .hidden. "), "hidden");
        assert_eq!(clean_segment("..."), "");
        assert_eq!(clean_segment(""), "");
        for reserved in ["CON", "nul", "Com1", "LPT9", "aux.txt", "PRN .pdf"] {
            assert_eq!(clean_segment(reserved), format!("_{reserved}"), "{reserved}");
        }
        for free in ["CONSOLE", "COM", "COM10", "NULL.txt"] {
            assert_eq!(clean_segment(free), free, "{free}");
        }
    }

    #[test]
    fn long_names_are_cut_at_a_character_and_files_keep_their_extension() {
        let folder = clean_segment(&"ä".repeat(100));
        assert_eq!((folder.len(), folder.chars().all(|c| c == 'ä')), (120, true));
        assert_eq!(clean_segment(&"b".repeat(300)).len(), 120);
        let mut facts = facts();
        facts.file_name = Some(format!("{}.pdf", "ö".repeat(200)));
        let name = file("{name}.{ext}", &facts);
        assert!(name.len() <= 120 && name.ends_with("ö.pdf"), "{name}");
        // A cut that leaves dots or spaces at the end drops them.
        facts.file_name = Some(format!("{}  .  x.pdf", "a".repeat(115)));
        assert_eq!(file("{name}.{ext}", &facts), format!("{}.pdf", "a".repeat(115)));
        // `CON` hidden behind spaces the cut takes away is still reserved.
        assert_eq!(clean_segment(&format!("CON{}x", " ".repeat(200))), "_CON");
    }

    #[test]
    fn a_file_name_without_extension_or_value_falls_back() {
        let mut facts = facts();
        facts.file_name = Some("Readme".into());
        assert_eq!(file("{name}.{ext}", &facts), "Readme");
        assert_eq!(file("{account}", &facts), "Privat");
        facts.account = "..".into();
        assert_eq!(file("{account}", &facts), "Readme");
        facts.file_name = Some("../CON".into());
        assert_eq!(file("{account}", &facts), "_CON");
        facts.file_name = None;
        assert_eq!(file("{name}.{ext}", &facts), "attachment");
        assert_eq!(file("", &facts), "attachment");
    }

    #[test]
    fn unknown_placeholders_and_open_braces_are_errors() {
        let facts = facts();
        assert_eq!(resolve(&spec("{yaer}", "{name}"), &facts), Err(SchemaError::Unknown("yaer".into())));
        assert_eq!(resolve(&spec("{year}", "{name}.{EXT}"), &facts), Err(SchemaError::Unknown("EXT".into())));
        assert_eq!(resolve(&spec("{year", "x"), &facts), Err(SchemaError::Unknown("year".into())));
        assert_eq!(resolve(&spec("", "{}"), &facts), Err(SchemaError::Unknown(String::new())));
        assert_eq!(SchemaError::Unknown("yaer".into()).to_string(), "unknown placeholder {yaer}");
        // A lone `}` is text; values are never read as a scheme.
        let mut braces = facts.clone();
        braces.subject = "{year}".into();
        assert_eq!(resolve(&spec("", "a}{subject}"), &braces).expect("resolves").file, "a}{year}");
    }

    #[test]
    fn kinds_follow_the_mime_type_else_the_extension() {
        let cases = [
            ("application/pdf", None, Some(FileKind::Pdf)),
            ("Application/PDF; name=x", None, Some(FileKind::Pdf)),
            ("image/heic", None, Some(FileKind::Image)),
            ("application/vnd.openxmlformats-officedocument.wordprocessingml.document", None, Some(FileKind::Office)),
            ("application/vnd.ms-excel", None, Some(FileKind::Office)),
            ("application/vnd.oasis.opendocument.presentation", None, Some(FileKind::Office)),
            ("application/octet-stream", Some("Scan.JPEG"), Some(FileKind::Image)),
            ("application/octet-stream", Some("Vertrag.docx"), Some(FileKind::Office)),
            ("application/octet-stream", Some("Rechnung.pdf"), Some(FileKind::Pdf)),
            ("application/octet-stream", Some("Tabelle.ods"), Some(FileKind::Office)),
            ("application/octet-stream", Some("archive.zip"), None),
            ("text/plain", None, None),
            ("application/vnd.oasis.opendocument.graphics", None, None),
        ];
        for (mime, name, kind) in cases {
            assert_eq!(FileKind::of(mime, name), kind, "{mime} {name:?}");
        }
    }

    fn when(categories: &[Category], kinds: &[FileKind], senders: &[&str]) -> SuggestWhen {
        SuggestWhen {
            categories: categories.to_vec(),
            kinds: kinds.to_vec(),
            senders: senders.iter().map(|s| s.to_string()).collect(),
        }
    }

    const SURE: Option<Suggestion> = Some(Suggestion { sure: true });

    #[test]
    fn a_target_without_conditions_is_never_suggested() {
        let invoice = Some(MailCategory::by_rule(Category::Invoice));
        assert_eq!(suggestion(&SuggestWhen::default(), invoice, Some(FileKind::Pdf), "a@b.de"), None);
    }

    #[test]
    fn targets_are_suggested_by_category_kind_and_sender() {
        let invoice = Some(MailCategory::by_person(Category::Invoice));
        let pdf = Some(FileKind::Pdf);
        let from = "billing@mail.stadtwerke-bonn.de";
        // Category: one of them is enough, none fits none.
        let by_category = when(&[Category::Receipt, Category::Invoice], &[], &[]);
        assert_eq!(suggestion(&by_category, invoice, None, ""), SURE);
        assert_eq!(suggestion(&by_category, Some(MailCategory::by_rule(Category::Personal)), pdf, from), None);
        assert_eq!(suggestion(&by_category, None, pdf, from), None);
        // Kind.
        let by_kind = when(&[], &[FileKind::Image, FileKind::Pdf], &[]);
        assert_eq!(suggestion(&by_kind, None, pdf, ""), SURE);
        assert_eq!(suggestion(&by_kind, None, Some(FileKind::Office), ""), None);
        assert_eq!(suggestion(&by_kind, None, None, ""), None);
        // Sender: a whole address or a domain with the ones below it, regardless of case.
        for sender in
            ["Billing@Mail.Stadtwerke-Bonn.de", "stadtwerke-bonn.de", "MAIL.stadtwerke-bonn.de", "@stadtwerke-bonn.de"]
        {
            assert_eq!(suggestion(&when(&[], &[], &[sender]), None, None, from), SURE, "{sender}");
        }
        for sender in
            ["other@mail.stadtwerke-bonn.de", "werke-bonn.de", "bonn.de.evil", "billing@stadtwerke-bonn.de", ""]
        {
            assert_eq!(suggestion(&when(&[], &[], &[sender]), None, None, from), None, "{sender}");
        }
        assert_eq!(
            suggestion(&when(&[], &[], &["stadtwerke-bonn.de"]), None, None, "billing@evilstadtwerke-bonn.de"),
            None
        );
        // Each kind that is set must fit.
        let all = when(&[Category::Invoice], &[FileKind::Pdf], &["stadtwerke-bonn.de"]);
        assert_eq!(suggestion(&all, invoice, pdf, from), SURE);
        assert_eq!(suggestion(&all, invoice, Some(FileKind::Image), from), None);
        assert_eq!(suggestion(&all, invoice, pdf, "billing@example.org"), None);
    }

    #[test]
    fn a_guessed_category_suggests_but_is_not_sure() {
        let guessed = Some(MailCategory::by_model(Category::Invoice, 0.9));
        let pdf = Some(FileKind::Pdf);
        assert_eq!(
            suggestion(&when(&[Category::Invoice], &[], &[]), guessed, pdf, ""),
            Some(Suggestion { sure: false })
        );
        assert_eq!(
            suggestion(&when(&[Category::Invoice], &[FileKind::Pdf], &[]), guessed, pdf, ""),
            Some(Suggestion { sure: false })
        );
        // Without a category condition the guess plays no part.
        assert_eq!(suggestion(&when(&[], &[FileKind::Pdf], &[]), guessed, pdf, ""), SURE);
        assert_eq!(
            suggestion(&when(&[Category::Invoice], &[], &[]), Some(MailCategory::by_rule(Category::Invoice)), None, ""),
            SURE
        );
    }
}
