//! Torro's own check of the paths in a call of a server that works in a folder (issue #303, ArtCraft): what a
//! call may name, before anyone is asked to approve it and again when it runs ([`refusal`]).
//!
//! ArtCraft's own bounds (`--automation-read-root`, `--automation-write-root`) are one guard; this is the other, and
//! for GridCraft, which has none, the only one. In this order:
//!
//! - A tool the catalog blocks is not there (`not-found`, as a tool not allowed); for GridCraft also every tool the
//!   catalog does not name ([`Entry::blocks`]).
//! - A blocked tool or command named under `name`, `id`, `command` or `method` anywhere in the arguments is
//!   `forbidden`: the all-purpose tools (`execute`, `tool_call`, `command_run` …) reach every command by name.
//! - Without a usable folder (none chosen, no longer allowed, or no longer one that may be chosen) every call is
//!   `forbidden`.
//! - The arguments the catalog names as read must exist in the chosen folder and not be locked.
//! - The target must be given where saving without one writes over the original (`error`), lie in the chosen
//!   folder, which must allow writing, and not exist, not even as a symbolic link (`conflict`): Torro saves changes
//!   only as a new file.
//! - Every text under a path key ([`PATH_KEYS`]) anywhere in the arguments is a full path in the chosen folder –
//!   not merely in another allowed folder – and not locked.
//!
//! Messages name the app, never a path or a name from the request.

use std::path::{Path, PathBuf};

use serde_json::{Map, Value};
use torro_core::i18n::{tr, trf};
use torro_core::settings::FolderAccess;
use torro_ipc::Response;

use super::catalog::Entry;
use crate::tools::folders::{Allowed, FolderError};

/// The keys whose texts are paths, wherever they are in the arguments: those ArtCraft's tools take files and folders
/// by. Numbers and other values under them (a bookmark's `path`, an image's number) are no paths.
pub const PATH_KEYS: &[&str] = &["path", "paths", "out", "out_dir", "folder", "file", "image", "directories"];

/// The keys whose texts name a tool or a command for the all-purpose tools; more than the ones ArtCraft uses today,
/// since a name blocked in vain costs only a refused call.
const NAME_KEYS: &[&str] = &["name", "id", "command", "commands", "method", "tool", "tools", "op", "action", "cmd"];

/// Why `entry` must not run `tool` with `arguments` now, as the answer; `None` when it may. `folder` is the chosen
/// folder as usable now (`super::Extensions::usable_folder`), `allowed` the allowed folders with their locks.
pub fn refusal(
    entry: &Entry,
    folder: Option<(PathBuf, FolderAccess)>,
    allowed: &Allowed,
    tool: &str,
    arguments: &Map<String, Value>,
) -> Option<Response> {
    if entry.blocks(tool) {
        return Some(super::no_tool(tool));
    }
    if arguments.iter().any(|(key, value)| names_a_blocked(entry, key, value)) {
        let message = tr("Torro blocks this command: it works the app's window, runs scripts or reaches keys.");
        return Some(error("forbidden", message.into()));
    }
    let app = entry.info.name.as_str();
    let Some((root, access)) = folder else {
        let message = trf(
            "Choose a folder for {app} in Torro Workplace under MCP servers first. It works only in that folder.",
            &[("app", &app)],
        );
        return Some(error("forbidden", message));
    };
    let place = Place { allowed, root: &root, app };
    let files = entry.files.get(tool);
    for name in files.map(|files| files.reads.as_slice()).unwrap_or_default() {
        for path in texts(arguments.get(name)) {
            if let Err(refused) = place.reads(path) {
                return Some(refused);
            }
        }
    }
    if let Some(files) = files
        && let Some(target) = &files.target
    {
        match arguments.get(target) {
            None | Some(Value::Null) if files.target_required => {
                let message = trf(
                    "Give the new file's path in “{argument}”: without it {app} saves over the original, and Torro \
                     saves changes only as a new file.",
                    &[("argument", target), ("app", &app)],
                );
                return Some(error("error", message));
            }
            None | Some(Value::Null) => {}
            Some(Value::String(path)) => {
                if let Err(refused) = place.target(path, access) {
                    return Some(refused);
                }
            }
            Some(_) => return Some(place.refused(&FolderError::Invalid(""))),
        }
    }
    let mut paths = Vec::new();
    arguments.iter().for_each(|(key, value)| path_texts(value, PATH_KEYS.contains(&key.as_str()), &mut paths));
    paths.into_iter().find_map(|path| place.inside(path).err())
}

/// Whether `value`, under `key`, names a blocked tool or command of `entry` anywhere: a text under one of
/// [`NAME_KEYS`], without regard to case – a blocked tool by its name, a command by its id, one ending in `.*` by
/// its beginning.
fn names_a_blocked(entry: &Entry, key: &str, value: &Value) -> bool {
    if NAME_KEYS.contains(&key) && texts(Some(value)).any(|text| is_blocked(entry, text)) {
        return true;
    }
    match value {
        Value::Object(map) => map.iter().any(|(key, value)| names_a_blocked(entry, key, value)),
        // A list keeps the key it stands under: `"commands": ["ui.click"]`.
        Value::Array(items) => items.iter().any(|item| names_a_blocked(entry, key, item)),
        _ => false,
    }
}

fn is_blocked(entry: &Entry, text: &str) -> bool {
    let text = text.trim().to_lowercase();
    entry.blocked.iter().any(|tool| tool.to_lowercase() == text)
        || entry.blocked_commands.iter().any(|command| {
            let command = command.to_lowercase();
            match command.strip_suffix(".*") {
                Some(stem) => text.strip_prefix(stem).is_some_and(|rest| rest.starts_with('.')),
                None => text == command,
            }
        })
}

/// The texts of `value`: itself when it is one, those in it when it is a list (at any depth); nothing else.
fn texts(value: Option<&Value>) -> impl Iterator<Item = &str> {
    let mut found = Vec::new();
    if let Some(value) = value {
        collect_texts(value, &mut found);
    }
    found.into_iter()
}

fn collect_texts<'a>(value: &'a Value, found: &mut Vec<&'a str>) {
    match value {
        Value::String(text) => found.push(text),
        Value::Array(items) => items.iter().for_each(|item| collect_texts(item, found)),
        _ => {}
    }
}

/// Every text under a path key anywhere in `value` (`under`: `value` is one, or is inside one): under a key, a text,
/// the texts of a list and every text of an object below it; objects are looked into at any depth
/// (`commands[].params.path`).
fn path_texts<'a>(value: &'a Value, under: bool, found: &mut Vec<&'a str>) {
    match value {
        Value::String(text) if under => found.push(text),
        Value::Array(items) => items.iter().for_each(|item| path_texts(item, under, found)),
        Value::Object(map) => {
            map.iter().for_each(|(key, value)| path_texts(value, under || PATH_KEYS.contains(&key.as_str()), found))
        }
        _ => {}
    }
}

/// The chosen folder of an app, resolved, among the allowed folders.
struct Place<'a> {
    allowed: &'a Allowed,
    root: &'a Path,
    app: &'a str,
}

impl Place<'_> {
    /// A path the tool reads: there, in the chosen folder, not locked.
    fn reads(&self, path: &str) -> Result<(), Response> {
        let resolved = self.allowed.existing(path, FolderAccess::Read).map_err(|err| self.refused(&err))?;
        self.holds(&resolved)
    }

    /// The file the tool writes: in the chosen folder, which allows writing, and not there yet, not even as a link.
    fn target(&self, path: &str, access: FolderAccess) -> Result<(), Response> {
        if access == FolderAccess::Read {
            return Err(self.refused(&FolderError::Forbidden("")));
        }
        let target = self.allowed.new_target(path, FolderAccess::ReadWrite).map_err(|err| self.refused(&err))?;
        self.holds(&target)?;
        match std::fs::symlink_metadata(&target) {
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(self.refused(&FolderError::of_io(&err))),
            Ok(_) => {
                let message = tr(
                    "There is a file of that name already. Torro saves changes only as a new file: choose a name that \
                     isn't taken.",
                );
                Err(error("conflict", message.into()))
            }
        }
    }

    /// Any other path: a full path in the chosen folder, not locked, there or not.
    fn inside(&self, path: &str) -> Result<(), Response> {
        let resolved = match self.allowed.existing(path, FolderAccess::Read) {
            Ok(resolved) => resolved,
            Err(FolderError::NotFound) => {
                self.allowed.new_target(path, FolderAccess::Read).map_err(|err| self.refused(&err))?
            }
            Err(err) => return Err(self.refused(&err)),
        };
        self.holds(&resolved)
    }

    /// `not-found` unless the resolved `path` is in the chosen folder: another allowed folder is not enough.
    fn holds(&self, path: &Path) -> Result<(), Response> {
        match path.starts_with(self.root) {
            true => Ok(()),
            false => Err(self.refused(&FolderError::NotFound)),
        }
    }

    /// The answer for `err`, naming the app and no path.
    fn refused(&self, err: &FolderError) -> Response {
        let app = [("app", &self.app as &dyn std::fmt::Display)];
        match err {
            FolderError::NotFound => error(
                "not-found",
                trf("A path in this call isn't in the folder chosen for {app}, or Torro protects it.", &app),
            ),
            FolderError::Invalid(_) => {
                error("error", trf("Give every path in full, inside the folder chosen for {app}.", &app))
            }
            FolderError::Forbidden(_) => error(
                "forbidden",
                trf("The folder chosen for {app} may only be read. To save, allow reading and writing there.", &app),
            ),
            other => other.response(),
        }
    }
}

fn error(code: &str, message: String) -> Response {
    Response::Error { code: code.into(), message }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use torro_core::settings::AllowedFolder;

    use super::*;
    use crate::extensions::catalog;

    /// A temporary home with `Docs` (the chosen folder, with a letter and a locked `.ssh`), `Other` (another allowed
    /// folder) and `Outside`.
    struct Home {
        _dir: tempfile::TempDir,
        root: PathBuf,
    }

    impl Home {
        fn new() -> Home {
            let dir = tempfile::tempdir().expect("temp dir");
            let root = std::fs::canonicalize(dir.path()).expect("resolved");
            for folder in ["Docs/.ssh", "Docs/Sub", "Other", "Outside"] {
                std::fs::create_dir_all(root.join(folder)).expect("made");
            }
            for file in ["Docs/letter.docx", "Docs/.ssh/id_ed25519", "Other/b.docx", "Outside/c.docx"] {
                std::fs::write(root.join(file), b"x").expect("written");
            }
            Home { _dir: dir, root }
        }

        fn path(&self, relative: &str) -> String {
            self.root.join(relative).to_string_lossy().into_owned()
        }

        fn allowed(&self) -> Allowed {
            let folders = [
                AllowedFolder { path: self.path("Docs"), access: FolderAccess::ReadWrite },
                AllowedFolder { path: self.path("Other"), access: FolderAccess::ReadWrite },
            ];
            Allowed::of(&folders, &[self.root.join("Docs/.ssh")])
        }

        fn docs(&self, access: FolderAccess) -> Option<(PathBuf, FolderAccess)> {
            Some((self.root.join("Docs"), access))
        }
    }

    fn entry(id: &str) -> &'static Entry {
        catalog::builtin().expect("the catalog").iter().find(|entry| entry.info.id == id).expect("in the catalog")
    }

    fn args(value: Value) -> Map<String, Value> {
        value.as_object().cloned().expect("an object")
    }

    /// The code of the refusal of `tool` of `app` with `arguments` in `Docs` with "Read and write"; `None` when it
    /// may run.
    fn refused(home: &Home, app: &str, tool: &str, arguments: Value) -> Option<String> {
        refused_in(home, app, home.docs(FolderAccess::ReadWrite), tool, arguments)
    }

    fn refused_in(
        home: &Home,
        app: &str,
        folder: Option<(PathBuf, FolderAccess)>,
        tool: &str,
        arguments: Value,
    ) -> Option<String> {
        let refusal = refusal(entry(app), folder, &home.allowed(), tool, &args(arguments));
        refusal.map(|response| match response {
            Response::Error { code, message } => {
                assert!(!message.contains(&*home.root.to_string_lossy()), "no path in {message}");
                code
            }
            other => panic!("expected an error, got {other:?}"),
        })
    }

    #[test]
    fn blocked_tools_are_not_there_and_blocked_names_are_forbidden() {
        let home = Home::new();
        let code = |app: &str, tool: &str, arguments: Value| refused(&home, app, tool, arguments);
        assert_eq!(code("wordcraft", "screenshot", json!({})).as_deref(), Some("not-found"));
        assert_eq!(code("pdfcraft", "js_run", json!({})).as_deref(), Some("not-found"));
        for (app, tool, arguments) in [
            ("wordcraft", "execute", json!({"command": "ui.click"})),
            ("wordcraft", "execute", json!({"command": "UI.Click"})),
            ("wordcraft", "batch", json!({"commands": [{"command": "file.save"}, {"command": "ui.screenshot"}]})),
            ("wordcraft", "execute", json!({"command": "Screenshot"})),
            ("deckcraft", "run_command", json!({"command": "ui.pointer"})),
            ("pdfcraft", "tool_call", json!({"name": "JS_RUN", "arguments": {}})),
            ("pdfcraft", "tool_call", json!({"name": "sign_document"})),
            ("pdfcraft", "command_run", json!({"id": "tools.js_console"})),
            ("pdfcraft", "command_run", json!({"id": "sign.digital"})),
            ("pdfcraft", "command_batch", json!({"commands": [{"method": "Sign.Digital"}]})),
            ("wordcraft", "batch", json!({"commands": ["file.save", "ui.click"]})),
            ("pdfcraft", "doc_batch", json!({"steps": [{"tool": "sign_document"}]})),
        ] {
            assert_eq!(code(app, tool, arguments.clone()).as_deref(), Some("forbidden"), "{app} {tool} {arguments}");
        }
        for (app, tool, arguments) in [
            ("wordcraft", "execute", json!({"command": "file.save", "params": {"path": home.path("Docs/new.docx")}})),
            ("wordcraft", "execute", json!({"command": "uiconfig.set"})),
            ("pdfcraft", "command_run", json!({"id": "tools.other"})),
            ("pdfcraft", "bookmark_rename", json!({"doc": 1, "path": [2, 1], "title": "x"})),
        ] {
            assert_eq!(code(app, tool, arguments.clone()), None, "{app} {tool} {arguments}");
        }
    }

    /// API review of #303: GridCraft has no bound of its own, so a tool the catalog does not name is not there.
    #[test]
    fn gridcraft_runs_only_the_tools_the_catalog_names() {
        let home = Home::new();
        let code = |tool: &str| refused(&home, "gridcraft", tool, json!({}));
        assert_eq!(code("export_pdf").as_deref(), Some("not-found"));
        assert_eq!(code("read_range"), None);
        assert!(entry("gridcraft").blocks("export_pdf") && !entry("wordcraft").blocks("export_pdf"));
    }

    #[test]
    fn without_a_usable_folder_every_call_is_forbidden() {
        let home = Home::new();
        for app in ["wordcraft", "gridcraft"] {
            let code = refused_in(&home, app, None, "list_commands", json!({}));
            assert_eq!(code.as_deref(), Some("forbidden"), "{app}");
        }
    }

    #[test]
    fn what_a_tool_reads_must_be_in_the_chosen_folder() {
        let home = Home::new();
        let open = |path: String| refused(&home, "wordcraft", "open_document", json!({"path": path}));
        assert_eq!(open(home.path("Docs/letter.docx")), None);
        let sep = std::path::MAIN_SEPARATOR;
        for (path, expected) in [
            (home.path("Docs/missing.docx"), "not-found"),
            (home.path("Other/b.docx"), "not-found"),
            (home.path("Outside/c.docx"), "not-found"),
            (home.path("Docs/.ssh/id_ed25519"), "not-found"),
            (format!("{}{sep}..{sep}Outside{sep}c.docx", home.path("Docs")), "error"),
            ("letter.docx".to_string(), "error"),
        ] {
            assert_eq!(open(path.clone()).as_deref(), Some(expected), "{path}");
        }
        let combine = json!({"paths": [home.path("Docs/letter.docx"), home.path("Other/b.docx")]});
        assert_eq!(refused(&home, "pdfcraft", "doc_combine", combine).as_deref(), Some("not-found"));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_out_of_the_folder_is_outside() {
        let home = Home::new();
        std::os::unix::fs::symlink(home.root.join("Outside/c.docx"), home.root.join("Docs/link.docx")).expect("link");
        let open = json!({"path": home.path("Docs/link.docx")});
        assert_eq!(refused(&home, "wordcraft", "open_document", open).as_deref(), Some("not-found"));
        // A link as the target is there already, also one that leads nowhere.
        std::os::unix::fs::symlink(home.root.join("Docs/nothing"), home.root.join("Docs/dangling.docx")).expect("link");
        let save = json!({"path": home.path("Docs/dangling.docx")});
        assert_eq!(refused(&home, "wordcraft", "save_document", save).as_deref(), Some("conflict"));
    }

    #[test]
    fn saving_makes_a_new_file_in_the_chosen_folder() {
        let home = Home::new();
        let save = |arguments: Value| refused(&home, "wordcraft", "save_document", arguments);
        assert_eq!(save(json!({})).as_deref(), Some("error"), "over the original");
        assert_eq!(save(json!({"path": null})).as_deref(), Some("error"));
        assert_eq!(save(json!({"path": 3})).as_deref(), Some("error"));
        assert_eq!(save(json!({"path": home.path("Docs/letter.docx")})).as_deref(), Some("conflict"));
        assert_eq!(save(json!({"path": home.path("Docs/LETTER.docx")})).as_deref(), refused_case(&home));
        assert_eq!(save(json!({"path": home.path("Other/new.docx")})).as_deref(), Some("not-found"));
        assert_eq!(save(json!({"path": home.path("Docs/.ssh/new")})).as_deref(), Some("not-found"));
        assert_eq!(save(json!({"path": home.path("Docs/new.docx")})), None);
        assert_eq!(save(json!({"path": home.path("Docs/Sub/new.docx")})), None);
        let read_only = home.docs(FolderAccess::Read);
        let saved =
            refused_in(&home, "wordcraft", read_only, "save_document", json!({"path": home.path("Docs/n.docx")}));
        assert_eq!(saved.as_deref(), Some("forbidden"));
        // An optional target may be left out (PdfCraft opens the result instead).
        let combine = json!({"paths": [home.path("Docs/letter.docx"), home.path("Docs/letter.docx")]});
        assert_eq!(refused(&home, "pdfcraft", "doc_combine", combine), None);
        let gridcraft = json!({"path": home.path("Docs/letter.docx")});
        assert_eq!(refused(&home, "gridcraft", "save_workbook", gridcraft).as_deref(), Some("conflict"));
    }

    /// On a drive that does not tell cases apart, `LETTER.docx` is the letter: there already.
    fn refused_case(home: &Home) -> Option<&'static str> {
        let same = std::fs::metadata(home.root.join("Docs/LETTER.docx")).is_ok();
        same.then_some("conflict")
    }

    #[test]
    fn every_path_anywhere_in_the_call_is_in_the_chosen_folder() {
        let home = Home::new();
        let batch = |path: String| json!({"commands": [{"command": "file.open", "params": {"path": path}}, {"command": "edit.undo"}]});
        assert_eq!(refused(&home, "wordcraft", "batch", batch(home.path("Docs/letter.docx"))), None);
        assert_eq!(refused(&home, "wordcraft", "batch", batch(home.path("Docs/Sub/new.docx"))), None);
        assert_eq!(
            refused(&home, "wordcraft", "batch", batch(home.path("Other/b.docx"))).as_deref(),
            Some("not-found")
        );
        assert_eq!(refused(&home, "wordcraft", "batch", batch("b.docx".into())).as_deref(), Some("error"));
        let split = json!({"doc": 1, "out_dir": home.path("Outside")});
        assert_eq!(refused(&home, "pdfcraft", "doc_split", split).as_deref(), Some("not-found"));
        let exported = json!({"doc": 1, "folder": home.path("Docs/.ssh")});
        assert_eq!(refused(&home, "pdfcraft", "doc_export_images", exported).as_deref(), Some("not-found"));
        let fonts = json!({"directories": [home.path("Docs"), home.path("Outside")]});
        assert_eq!(refused(&home, "pdfcraft", "text_font_files", fonts).as_deref(), Some("not-found"));
        let deep = json!({"a": {"b": [{"c": {"file": home.path("Outside/c.docx")}}]}});
        assert_eq!(refused(&home, "pdfcraft", "doc_batch", deep).as_deref(), Some("not-found"));
        // Below a path key every text counts, also inside an object.
        let wrapped = json!({"doc": 1, "out_dir": {"to": home.path("Outside")}});
        assert_eq!(refused(&home, "pdfcraft", "doc_split", wrapped).as_deref(), Some("not-found"));
        // Numbers under a path key are no paths.
        let image = json!({"doc": 1, "page": 1, "image": 2, "path": home.path("Docs/picture.png")});
        assert_eq!(refused(&home, "pdfcraft", "image_save", image), None);
    }
}
