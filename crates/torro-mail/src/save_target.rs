//! Save targets for attachments (task M12.1, issue #290): a folder with a scheme for subfolders and
//! file names, where a click puts an attachment ("Invoices" → `…/Rechnungen/2026/03/`).
//!
//! Types only, the API's contract: the core keeps the targets in `<data>/mail/save-targets.json`
//! (`mail.list_save_targets`, `mail.create_save_target` …), the settings page edits them, and the
//! reader offers them in the menu of an attachment. Resolving a scheme to a path and the rule that
//! suggests a target are pure functions here as well (task M12.2), so the core and the live preview
//! of the settings page compute the same.
//!
//! Targets are the same for every mail account; `{account}` in a scheme tells them apart where a
//! person wants that. Only a person sets a target's folder (point 9 of "Was nie nach außen geht",
//! `docs/api.md`): it is a place on the computer the core writes to.
//!
//! On the wire like the settings: fields in camelCase (`fileName`, `dateFrom`), values in
//! snake_case (`file_name`, `keep_both`).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::Category;

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
}
