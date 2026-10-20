//! The assistant's own tools (concept "Eigene Werkzeuge" in `docs/assistent.md`, issue #225): abilities only the
//! assistant and the bots have, each with a switch and settings of its own. The first one makes PDFs.
//!
//! - `pdf`: sets a PDF from Markdown and one of the three page templates (task WZ.3).
//! - `place`: where a new file goes and what it is called (task WZ.4).
//! - `files`: the register of every file a tool made (task WZ.4).
//!
//! [`Tools`] answers the `tools.*` requests (task WZ.4). The core checks before: the tool's switch for every key
//! (`Lack::ToolOff`), for a client its rights and selection (`Lack::Tool`) and that a file is its own; the handlers
//! here take the maker of a file ([`Maker`]) and never reach another client's files. Nothing here logs a title, a
//! path or content: only the kind of an error.

pub mod files;
pub mod pdf;
pub mod place;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Local;
use torro_core::i18n::{tr, trf};
use torro_core::settings::{Settings, ToolId};
use torro_ipc::Response;
use torro_ipc::api::{Access, Area, Rights};
use torro_ipc::tools::{MAX_MARKDOWN_BYTES, MAX_TITLE_CHARS, PdfTemplateId, ToolInfo};

pub use files::{FileEntry, Maker, Register};

use crate::pim::Reach;

/// Opens a file with the system's program for it, without waiting ([`system_open`]); tests record instead.
pub type Opener = Arc<dyn Fn(&Path) -> Result<(), String> + Send + Sync>;

/// The folders the tools work with. Each is a parameter, so tests never touch the person's.
#[derive(Debug, Clone, Default)]
pub struct Roots {
    /// The tools' data folder (`<data>/tools`): the register lives here. `None` keeps no register.
    pub data: Option<PathBuf>,
    /// The system's desktop (`dirs::desktop_dir`), where a PDF goes when no folder is set.
    pub desktop: Option<PathBuf>,
    /// The home folder, where a PDF goes when there is no desktop either. `None`: no PDF can be saved.
    pub home: Option<PathBuf>,
    /// The folder of the design packs (`torro_core::design::dir`), for the fonts; `None` sets in the standard's.
    pub designs: Option<PathBuf>,
}

/// The tools' handlers and their register.
pub struct Tools {
    roots: Roots,
    register: Register,
    open: Opener,
}

impl Tools {
    /// The person's: the register in the core's data folder, the system's desktop and home folder, the design
    /// packs' folder, files opened with the system's program. In tests none of them: nothing of the person's.
    pub fn system() -> Tools {
        if cfg!(test) {
            return Tools::new(Roots::default());
        }
        let data =
            torro_core::paths::AppPaths::for_app(torro_core::AppId::WORKPLACE).map(|paths| paths.data.join("tools"));
        let roots =
            Roots { data, desktop: dirs::desktop_dir(), home: dirs::home_dir(), designs: torro_core::design::dir() };
        Tools { register: Register::in_dir(roots.data.clone()), roots, open: Arc::new(system_open) }
    }

    /// The tools with `roots`; files are opened by nobody (tests set an [`Opener`] with [`Tools::with_opener`]).
    pub fn new(roots: Roots) -> Tools {
        let open: Opener = Arc::new(|_: &Path| Err("nothing is opened here".to_string()));
        Tools { register: Register::in_dir(roots.data.clone()), roots, open }
    }

    /// `self` opening files with `open`, for tests.
    #[cfg(test)]
    pub fn with_opener(mut self, open: Opener) -> Tools {
        self.open = open;
        self
    }

    /// The register, for the attachments of drafts (`crate::mail::write`).
    pub fn register(&self) -> &Register {
        &self.register
    }

    /// The file `id` of the register as `maker` reaches it and while it is intact ([`Register::get`]); read on a
    /// blocking thread. For the core's checks before an approval and for the approval's summary.
    pub async fn file(&self, id: &str, maker: Maker<'_>) -> Option<FileEntry> {
        let (register, id, client) = (self.register.clone(), id.to_string(), maker.client());
        let found = tokio::task::spawn_blocking(move || {
            let maker = client.as_deref().map_or(Maker::App, Maker::Client);
            register.get(&id, maker)
        });
        found.await.ok().flatten()
    }

    /// `tools.create_pdf`: sets `markdown` with the template `template` (`report` when `None`) of `settings`, in
    /// the fonts of the active design pack, saves it in the folder [`place::folder`] picks under a name
    /// [`place::file_name`] makes, never replacing a file, and keeps it in the register as made by `maker`.
    /// Answers [`Response::File`]; `error` for a title or Markdown beyond the limits ([`limit_refusal`]), no folder
    /// to save in, a PDF that cannot be set or a file that cannot be written. The switch and the rights are the
    /// core's to check before.
    pub async fn create_pdf(
        &self,
        title: String,
        markdown: String,
        template: Option<PdfTemplateId>,
        settings: &Settings,
        maker: Maker<'_>,
    ) -> Response {
        if let Some(refused) = limit_refusal(&title, &markdown) {
            return refused;
        }
        let title = title.trim().to_string();
        let id = template.unwrap_or_default();
        let page = settings.tools.pdf.template(id).clone();
        let set_folder = settings.tools.pdf.folder.clone().map(PathBuf::from);
        let design = settings.design.clone();
        let (roots, register, client) = (self.roots.clone(), self.register.clone(), maker.client());
        let made = tokio::task::spawn_blocking(move || -> Result<FileEntry, ToolError> {
            let home = roots.home.ok_or(ToolError::NoFolder)?;
            let fonts = pdf::PdfFonts::of_design(roots.designs.as_deref(), &design);
            let logo = page.logo.as_deref().and_then(|logo| pdf::read_logo(Path::new(logo)));
            let date = Local::now().date_naive();
            let bytes = pdf::render(&title, &markdown, &page, &fonts, logo.as_deref(), date).map_err(ToolError::Pdf)?;
            let folder = place::folder(set_folder.as_deref(), roots.desktop, &home);
            let path = place::write_new(&folder, &place::file_name(&title), &bytes)
                .map_err(|err| ToolError::Write(err.kind().to_string()))?;
            let maker = client.as_deref().map_or(Maker::App, Maker::Client);
            let kept = register.add_new(|file| FileEntry::of(file, path.clone(), &title, id, maker, &bytes));
            kept.map_err(|err| {
                // A file nobody can open, save or attach again is not left behind.
                eprintln!("torroworkplace: a PDF was made but cannot be kept: {err}");
                let _ = fs::remove_file(&path);
                ToolError::Register
            })
        });
        match made.await {
            Ok(Ok(entry)) => Response::File { file: entry.info() },
            Ok(Err(err)) => err.response(),
            Err(err) => ToolError::Write(err.to_string()).response(),
        }
    }

    /// `tools.open_file`: opens the file `id` of the register that `maker` reaches with the system's program for
    /// it, without waiting ([`Response::Ok`]). `not-found` for an unknown id, another client's file and a file that
    /// was changed, moved or removed; `error` when the program cannot be started.
    pub async fn open_file(&self, id: &str, maker: Maker<'_>) -> Response {
        let Some(entry) = self.file(id, maker).await else { return ToolError::NotFound.response() };
        match (self.open)(&entry.path) {
            Ok(()) => Response::Ok,
            Err(_) => {
                eprintln!("torroworkplace: the system's program for a file of the tools did not start");
                ToolError::Open.response()
            }
        }
    }

    /// `tools.save_file_as` (app key only, point 9 of "Was nie nach außen geht"): a copy of the file `id` at
    /// `path`, which a person chose in the system's save dialog; `.pdf` is added when the name lacks it, and a file
    /// there is replaced (the dialog asked). `not-found` as [`Tools::open_file`] and for a file that changed while
    /// it was copied; `error` for a path that is not absolute, a folder that does not exist, a folder or a symbolic
    /// link as the target, or a copy that cannot be written.
    pub async fn save_file_as(&self, id: &str, path: &str) -> Response {
        let Some((target, added)) = target_of(path) else { return ToolError::Invalid(NOT_ABSOLUTE).response() };
        let Some(entry) = self.file(id, Maker::App).await else { return ToolError::NotFound.response() };
        let copied = tokio::task::spawn_blocking(move || -> Result<(), ToolError> {
            if !target.parent().is_some_and(Path::is_dir) {
                return Err(ToolError::Invalid(NO_FOLDER));
            }
            if fs::symlink_metadata(&target).is_ok_and(|meta| meta.is_dir() || meta.file_type().is_symlink()) {
                return Err(ToolError::Invalid(NOT_REPLACEABLE));
            }
            // The system's dialog asked about the name the person chose, not about the one with `.pdf`
            // added: that file is not replaced unasked.
            if added && fs::symlink_metadata(&target).is_ok() {
                return Err(ToolError::Invalid(TAKEN));
            }
            let bytes = fs::read(&entry.path).map_err(|_| ToolError::NotFound)?;
            if !entry.holds(&bytes) {
                return Err(ToolError::NotFound);
            }
            fs::write(&target, &bytes).map_err(|err| ToolError::Write(err.kind().to_string()))
        });
        match copied.await {
            Ok(Ok(())) => Response::Ok,
            Ok(Err(err)) => err.response(),
            Err(err) => ToolError::Write(err.to_string()).response(),
        }
    }
}

/// `tools.list_tools` for `reach`: every tool with whether a person switched it on (`settings`) and whether the
/// key may use it: the app key always; a client with write access to `tools` and the tool in its selection.
pub fn list(settings: &torro_core::settings::Tools, reach: Reach<'_>) -> Response {
    let tools = ToolId::ALL
        .into_iter()
        .map(|id| ToolInfo {
            id,
            name: id.title().to_string(),
            description: description(id).to_string(),
            enabled: settings.is_on(id),
            allowed: match reach {
                Reach::Suite => true,
                Reach::Client(rights) => rights.area(Area::Tools).allows(Access::Write) && rights.reaches_tool(id),
            },
        })
        .collect();
    Response::Tools { tools }
}

/// What the tool `id` does, for people, in the suite's language.
fn description(id: ToolId) -> &'static str {
    match id {
        ToolId::Pdf => tr("Makes PDFs from the assistant's text, set after the templates in its settings."),
    }
}

/// Whether a client with `rights` reaches the files the tools made (its own, the register's rule): the PDF tool
/// switched on in `settings`, read access to `tools` and `pdf` in its selection. For drafts' attachments; the
/// commands of the tool check their switch and rights themselves.
pub fn reaches_files(settings: &torro_core::settings::Tools, rights: &Rights) -> bool {
    settings.is_on(ToolId::Pdf) && rights.area(Area::Tools).allows(Access::Read) && rights.reaches_tool(ToolId::Pdf)
}

/// The `error` for a `tools.create_pdf` beyond its limits, which the core gives before anyone is asked to approve
/// it: a blank title, one over [`MAX_TITLE_CHARS`] characters or with a line break or another control character,
/// Markdown over [`MAX_MARKDOWN_BYTES`] bytes. `None` when it fits.
pub fn limit_refusal(title: &str, markdown: &str) -> Option<Response> {
    let title = title.trim();
    let message = if title.is_empty() {
        BLANK_TITLE
    } else if title.chars().count() > MAX_TITLE_CHARS || title.chars().any(char::is_control) {
        LONG_TITLE
    } else if markdown.len() > MAX_MARKDOWN_BYTES {
        LONG_MARKDOWN
    } else {
        return None;
    };
    Some(ToolError::Invalid(message).response())
}

/// `path` as the target of a copy: absolute, with a name, `.pdf` added when the name does not end in it; and
/// whether it was added.
fn target_of(path: &str) -> Option<(PathBuf, bool)> {
    let path = PathBuf::from(path.trim());
    if !path.is_absolute() || path.file_name().is_none() {
        return None;
    }
    let pdf = path.extension().is_some_and(|extension| extension.eq_ignore_ascii_case("pdf"));
    Some(if pdf { (path, false) } else { (PathBuf::from(format!("{}.pdf", path.display())), true) })
}

const BLANK_TITLE: &str = "The PDF needs a title.";
const LONG_TITLE: &str = "The title of a PDF is one line of at most 200 characters.";
const LONG_MARKDOWN: &str = "The text of a PDF may be at most 200,000 bytes.";
const NOT_ABSOLUTE: &str = "Give the full path of the copy.";
const NO_FOLDER: &str = "The folder for the copy does not exist.";
const NOT_REPLACEABLE: &str = "A folder or a link cannot be replaced by the copy.";
const TAKEN: &str = "A file of that name with .pdf exists already; choose the name with .pdf to replace it.";

/// Why a tool's command failed. [`ToolError::response`] is the answer, for people, without titles or paths.
#[derive(Debug)]
enum ToolError {
    /// `error`: the request cannot work as given.
    Invalid(&'static str),
    /// `not-found`: no such file for this key, or it changed.
    NotFound,
    /// `error`: there is no folder to save in (no home directory).
    NoFolder,
    /// `error`: the PDF could not be set.
    Pdf(pdf::PdfError),
    /// `error`: a file could not be written; the kind of the error.
    Write(String),
    /// `error`: the register could not keep the file, which was removed again.
    Register,
    /// `error`: the system's program did not start.
    Open,
}

impl ToolError {
    fn response(&self) -> Response {
        let (code, message) = match self {
            ToolError::Invalid(message) => ("error", tr(message).to_string()),
            ToolError::NotFound => ("not-found", tr("No such file.").to_string()),
            ToolError::NoFolder => ("error", tr("There is no folder to save the PDF in.").to_string()),
            ToolError::Pdf(err) => ("error", trf("The PDF cannot be made: {reason}", &[("reason", err)])),
            ToolError::Write(reason) => ("error", trf("The file cannot be written: {reason}", &[("reason", reason)])),
            ToolError::Register => (
                "error",
                tr("The PDF cannot be kept: Torro Workplace cannot read or write its list of files.").to_string(),
            ),
            ToolError::Open => ("error", tr("The file cannot be opened: no program for it started.").to_string()),
        };
        Response::Error { code: code.into(), message }
    }
}

/// Opens `path` with the system's program for it: `open` on macOS, the Explorer on Windows, `xdg-open` elsewhere
/// (as the control centre opens the designs folder). Does not wait for it.
fn system_open(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let program = "xdg-open";
    let mut child = std::process::Command::new(program).arg(path).spawn().map_err(|err| err.kind().to_string())?;
    // Waited for on a thread of its own, so it leaves no zombie behind.
    std::thread::spawn(move || child.wait());
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use std::sync::Mutex;

    use torro_ipc::api::{AreaRights, Level, RightsTemplate, Selection};

    use super::*;

    /// Temporary roots: data, desktop and home folders under one directory, the desktop and home made.
    pub(crate) struct Room {
        pub(crate) dir: tempfile::TempDir,
        pub(crate) opened: Arc<Mutex<Vec<PathBuf>>>,
    }

    impl Room {
        pub(crate) fn new() -> Room {
            let dir = tempfile::tempdir().expect("temp dir");
            for folder in ["Desktop", "home"] {
                fs::create_dir(dir.path().join(folder)).expect("made");
            }
            Room { dir, opened: Arc::new(Mutex::new(Vec::new())) }
        }

        pub(crate) fn desktop(&self) -> PathBuf {
            self.dir.path().join("Desktop")
        }

        pub(crate) fn home(&self) -> PathBuf {
            self.dir.path().join("home")
        }

        /// The tools over this room, recording what they open.
        pub(crate) fn tools(&self) -> Tools {
            let roots = Roots {
                data: Some(self.dir.path().join("data").join("tools")),
                desktop: Some(self.desktop()),
                home: Some(self.home()),
                designs: None,
            };
            let opened = Arc::clone(&self.opened);
            Tools::new(roots).with_opener(Arc::new(move |path: &Path| {
                opened.lock().expect("lock").push(path.to_path_buf());
                Ok(())
            }))
        }
    }

    fn file(response: Response) -> torro_ipc::tools::FileInfo {
        match response {
            Response::File { file } => file,
            other => panic!("expected a file, got {other:?}"),
        }
    }

    fn code(response: &Response) -> &str {
        match response {
            Response::Error { code, .. } => code,
            other => panic!("expected an error, got {other:?}"),
        }
    }

    async fn make(tools: &Tools, title: &str, settings: &Settings, maker: Maker<'_>) -> Response {
        tools.create_pdf(title.into(), "# Hello\n\nÄpfel und Birnen.".into(), None, settings, maker).await
    }

    #[tokio::test]
    async fn a_pdf_goes_to_the_set_folder_the_desktop_or_home_and_is_never_replaced() {
        let room = Room::new();
        let tools = room.tools();
        let mut settings = Settings::default();

        let first = file(make(&tools, "Quarterly report", &settings, Maker::App).await);
        assert_eq!(PathBuf::from(&first.path), room.desktop().join("Quarterly report.pdf"));
        assert_eq!((first.name.as_str(), first.template), ("Quarterly report.pdf", PdfTemplateId::Report));
        let bytes = fs::read(&first.path).expect("written");
        assert!(bytes.starts_with(b"%PDF"), "a PDF");
        assert_eq!(first.size, bytes.len() as u64);
        let second = file(make(&tools, "Quarterly report", &settings, Maker::App).await);
        assert_eq!(PathBuf::from(&second.path), room.desktop().join("Quarterly report-2.pdf"));
        assert_eq!(fs::read(&first.path).expect("kept"), bytes, "the first one stays as it was");
        let cleaned = file(make(&tools, "Offer: A/B?", &settings, Maker::App).await);
        assert_eq!(cleaned.name, "Offer A B.pdf");
        assert_eq!(cleaned.title, "Offer: A/B?", "the title as given");

        let set = room.dir.path().join("PDFs");
        fs::create_dir(&set).expect("made");
        settings.tools.pdf.folder = Some(set.to_string_lossy().into_owned());
        let in_set = file(make(&tools, "Letter", &settings, Maker::App).await);
        assert_eq!(PathBuf::from(&in_set.path), set.join("Letter.pdf"));

        fs::remove_dir_all(room.desktop()).expect("removed");
        settings.tools.pdf.folder = Some(room.dir.path().join("gone").to_string_lossy().into_owned());
        let at_home = file(make(&tools, "Note", &settings, Maker::App).await);
        assert_eq!(PathBuf::from(&at_home.path), room.home().join("Note.pdf"));
        assert!(!room.dir.path().join("gone").exists(), "a missing folder is not made");

        let none = Tools::new(Roots::default());
        assert_eq!(code(&make(&none, "Nowhere", &settings, Maker::App).await), "error");
    }

    #[tokio::test]
    async fn the_limits_hold_before_anything_is_written() {
        let room = Room::new();
        let tools = room.tools();
        let settings = Settings::default();
        for title in ["", "  ", "Two\nlines", &"x".repeat(MAX_TITLE_CHARS + 1)] {
            assert_eq!(code(&make(&tools, title, &settings, Maker::App).await), "error", "{title:?}");
        }
        let long = "x".repeat(MAX_MARKDOWN_BYTES + 1);
        let refused = tools.create_pdf("Long".into(), long, None, &settings, Maker::App).await;
        assert_eq!(code(&refused), "error");
        assert_eq!(fs::read_dir(room.desktop()).expect("listed").count(), 0, "nothing written");
        assert!(limit_refusal(&"x".repeat(MAX_TITLE_CHARS), "fits").is_none());
    }

    #[tokio::test]
    async fn a_file_is_opened_and_saved_only_from_the_register_and_by_its_maker() {
        let room = Room::new();
        let tools = room.tools();
        let settings = Settings::default();
        let mine = file(make(&tools, "Mine", &settings, Maker::Client("c1")).await);
        let theirs = file(make(&tools, "Theirs", &settings, Maker::Client("c2")).await);

        assert_eq!(tools.open_file(&mine.id, Maker::Client("c1")).await, Response::Ok);
        assert_eq!(*room.opened.lock().expect("lock"), vec![PathBuf::from(&mine.path)]);
        assert_eq!(code(&tools.open_file(&theirs.id, Maker::Client("c1")).await), "not-found");
        assert_eq!(code(&tools.open_file("00000000", Maker::App).await), "not-found");
        assert_eq!(tools.open_file(&theirs.id, Maker::App).await, Response::Ok, "the person opens every file");

        let copy = room.dir.path().join("Copy");
        assert_eq!(tools.save_file_as(&mine.id, &copy.to_string_lossy()).await, Response::Ok);
        let copied = room.dir.path().join("Copy.pdf");
        assert_eq!(fs::read(&copied).expect("copied"), fs::read(&mine.path).expect("read"), ".pdf added");
        let asked_without = tools.save_file_as(&mine.id, &copy.to_string_lossy()).await;
        assert_eq!(code(&asked_without), "error", "the name with .pdf added is not replaced unasked");
        assert_eq!(tools.save_file_as(&mine.id, &copied.to_string_lossy()).await, Response::Ok, "replaced");
        for refused in ["Copy.pdf", "", "/"] {
            assert_eq!(code(&tools.save_file_as(&mine.id, refused).await), "error", "{refused:?}");
        }
        let missing = room.dir.path().join("missing").join("Copy.pdf");
        assert_eq!(code(&tools.save_file_as(&mine.id, &missing.to_string_lossy()).await), "error");
        let folder = room.dir.path().join("Folder.pdf");
        fs::create_dir(&folder).expect("made");
        assert_eq!(code(&tools.save_file_as(&mine.id, &folder.to_string_lossy()).await), "error");
        #[cfg(unix)]
        {
            let link = room.dir.path().join("Link.pdf");
            std::os::unix::fs::symlink(&copied, &link).expect("linked");
            assert_eq!(code(&tools.save_file_as(&mine.id, &link.to_string_lossy()).await), "error");
        }
        let unknown = tools.save_file_as("00000000", &copied.to_string_lossy()).await;
        assert_eq!(code(&unknown), "not-found");

        // Exchanged behind the core's back: not the register's file any more.
        fs::write(&mine.path, b"%PDF something else").expect("exchanged");
        assert_eq!(code(&tools.open_file(&mine.id, Maker::Client("c1")).await), "not-found");
        assert_eq!(code(&tools.save_file_as(&mine.id, &copied.to_string_lossy()).await), "not-found");
    }

    #[test]
    fn the_list_says_switch_and_access() {
        let mut settings = torro_core::settings::Tools::default();
        let listed = |settings: &torro_core::settings::Tools, reach: Reach<'_>| match list(settings, reach) {
            Response::Tools { tools } => tools,
            other => panic!("expected tools, got {other:?}"),
        };
        let suite = listed(&settings, Reach::Suite);
        assert_eq!(suite.len(), 1);
        assert_eq!((suite[0].id, suite[0].enabled, suite[0].allowed), (ToolId::Pdf, false, true));
        assert_eq!(suite[0].description, tr(description(ToolId::Pdf)));
        settings.set_on(ToolId::Pdf, true);
        let mut rights = RightsTemplate::ReadOnly.rights();
        assert!(!listed(&settings, Reach::Client(&rights))[0].allowed, "reading only");
        rights.areas.insert(Area::Tools, AreaRights { level: Level::Write, admin: false });
        rights.tools = Selection::Only(Vec::new());
        assert!(!listed(&settings, Reach::Client(&rights))[0].allowed, "not in the selection");
        rights.tools = Selection::Only(vec!["pdf".into()]);
        let client = listed(&settings, Reach::Client(&rights));
        assert!(client[0].allowed && client[0].enabled);
    }

    #[test]
    fn files_are_reached_with_switch_reading_and_selection() {
        let mut settings = torro_core::settings::Tools::default();
        let mut rights = RightsTemplate::ReadOnly.rights();
        rights.tools = Selection::Only(vec!["pdf".into()]);
        assert!(!reaches_files(&settings, &rights), "switched off");
        settings.set_on(ToolId::Pdf, true);
        assert!(reaches_files(&settings, &rights));
        rights.tools = Selection::Only(Vec::new());
        assert!(!reaches_files(&settings, &rights), "not selected");
        rights.tools = Selection::All;
        rights.areas.remove(&Area::Tools);
        assert!(!reaches_files(&settings, &rights), "no access to tools");
    }
}
