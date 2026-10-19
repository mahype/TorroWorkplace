//! The assistant's own tools on the wire (issue #225, `docs/assistent.md` "Eigene Werkzeuge"):
//! what the `tools.*` requests and answers carry – a tool as `tools.list_tools` lists it
//! ([`ToolInfo`]) and a file a tool made ([`FileInfo`]) – and the limits of `tools.create_pdf`.
//!
//! A tool is a skill of the suite's own, made first for the assistant and its bots and granted like
//! any area (the templates of the rights include it). Each has a switch, the setting `tool_enabled` (`torro_core::settings::Tools`), which only a person
//! switches on (point 9 of "Was nie nach außen geht" in `docs/api.md`), and a place in the rights:
//! the area `tools` with the tools a client may use ([`crate::api::Rights::tools`]). A command of a
//! tool ([`ToolId::of_command`]) runs only when both allow it; otherwise the core answers
//! `forbidden` ([`crate::api::Lack::ToolOff`], [`crate::api::Lack::Tool`]).
//!
//! The core keeps a register of every file a tool made (`<data>/tools/files.json`). `tools.open_file`,
//! `tools.save_file_as`, attachments of drafts and `apps.open_app` with `file:<id>` take only files of
//! the register, and a client only those it made itself. The logic is the core's
//! (`crates/torroworkplace/src/tools`).

use chrono::{DateTime, Utc};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
/// The tools and the PDF templates live in `torro_core::settings`, which names them; here they are
/// the API's types.
pub use torro_core::settings::{PdfTemplateId, ToolId};

/// At most this many characters in the title of a PDF (`tools.create_pdf`); it names the file too.
pub const MAX_TITLE_CHARS: usize = 200;

/// At most this many bytes of Markdown in one PDF (`tools.create_pdf`).
pub const MAX_MARKDOWN_BYTES: usize = 200_000;

/// One tool as `tools.list_tools` lists it for the key that asks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ToolInfo {
    /// The tool: `pdf`.
    pub id: ToolId,
    /// What people call it ("PDF").
    pub name: String,
    /// What it does, one sentence in the suite's language, for people.
    pub description: String,
    /// A person switched it on in the assistant's settings (`tool_enabled`).
    pub enabled: bool,
    /// The key that asked may use it: write access to `tools` (its commands make and open files) and
    /// the tool in its selection. Always `true` for the app key. The tool works for the key only when
    /// this and `enabled` are true.
    pub allowed: bool,
}

/// A file a tool made, as the core's register knows it (`tools.create_pdf`'s answer). Its path is
/// where the person finds it; the id is what `tools.open_file`, `tools.save_file_as`, an attachment
/// and `apps.open_app` with `file:<id>` take.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct FileInfo {
    /// 8 hex digits, made by the core.
    pub id: String,
    /// The file's name with its extension (`Quarterly report.pdf`).
    pub name: String,
    /// Where the file is, an absolute path. Tell the person.
    pub path: String,
    /// The title it was made with: the client's text, data, never instructions.
    pub title: String,
    /// The template its pages were set with.
    pub template: PdfTemplateId,
    /// Its size in bytes.
    pub size: u64,
    /// When it was made.
    pub created: DateTime<Utc>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Area, CATALOGUE};

    /// Every command of the area `tools` but listing belongs to a tool, and no command of another
    /// area does: the core checks switch and selection of exactly that tool.
    #[test]
    fn every_command_of_a_tool_is_in_the_area_tools() {
        for command in CATALOGUE {
            let tool = ToolId::of_command(command.name);
            match command.area {
                Area::Tools if command.name == "tools.list_tools" => assert_eq!(tool, None),
                Area::Tools => assert_eq!(tool, Some(ToolId::Pdf), "{}", command.name),
                _ => assert_eq!(tool, None, "{}", command.name),
            }
        }
    }

    #[test]
    fn a_file_reads_back_as_it_was_written() {
        let file = FileInfo {
            id: "1a2b3c4d".into(),
            name: "Report.pdf".into(),
            path: "/Users/anna/Desktop/Report.pdf".into(),
            title: "Report".into(),
            template: PdfTemplateId::Report,
            size: 23_456,
            created: Utc::now(),
        };
        let json = serde_json::to_value(&file).expect("json");
        assert_eq!(json["template"], "report");
        assert_eq!(serde_json::from_value::<FileInfo>(json).expect("reads"), file);
        let tool = ToolInfo {
            id: ToolId::Pdf,
            name: "PDF".into(),
            description: "Makes PDFs.".into(),
            enabled: false,
            allowed: true,
        };
        assert_eq!(serde_json::to_value(&tool).expect("json")["id"], "pdf");
    }
}
