//! The look this process draws with: the active design, font and colour scheme (tasks L.2 and L.21,
//! concept `docs/design-pakete.md`).
//!
//! Every token of `torro_gui` is read from **three active parts**, one per process each:
//!
//! - the [`Design`] ([`active`], [`set`]): the type scale, [`crate::theme::space`],
//!   [`crate::theme::radius`], [`crate::theme::size`] and the layout gaps of [`crate::layout`];
//! - the [`FontSet`] ([`crate::font::active`], [`crate::font::set`]): the families of the levels and of
//!   [`crate::theme::font`];
//! - the [`ColorScheme`] ([`crate::scheme::active`], [`crate::scheme::set`]): the colours of
//!   [`crate::theme::palette`] and the logo of [`crate::shell`].
//!
//! A level of [`crate::typo`] is the design's level as drawn in the font, as [`Selection::level`] gives
//! it: the size (meant for Inter) times the font's size adjustment, the nearest weight the font has.
//! Torro, Torro, Torro gives exactly the values the suite had as constants (the test "same picture"
//! below holds it to that).
//!
//! Views never cache a token across frames: a switch takes effect on the next `view`/`draw`.
//!
//! # Following the setting (tasks L.3 and L.21)
//!
//! Until task L.22 the suite settings name one id (`Settings::design`), and every window draws with
//! the three parts of that id, each from its own folder ([`Folders::look`]): a part whose folder lacks
//! the id, or that breaks a rule, is the standard's – never a crash. A window follows a change at once,
//! the way the appearance and the zoom do. An app wires it up in four places:
//!
//! ```ignore
//! use torro_gui::design::{self, Follow};
//!
//! // main, before `torro_gui::settings()`: the first frame already has the right look and fonts.
//! design::start_for(&source, launch.design.as_deref()); // `--design <id>` of `--demo` wins
//! // new: no socket with `--demo`, so the look stays as it started.
//! let design = Follow::for_source(&source);
//! let task = Task::batch([design.fetch().map(Message::Design), …]);
//! // subscription: always, also in glance views. Asks the core every 2 s and reports only a change,
//! // so a choice in the control centre arrives at once and an idle window is not woken up.
//! all.push(self.design.subscription().map(Message::Design));
//! // update: a switch of the font loads its files; canvas caches (`canvas::Cache`) would be cleared here.
//! Message::Design(message) => {
//!     if let Some(task) = self.design.update(message) {
//!         return task.map(Message::Design);
//!     }
//! }
//! ```
//!
//! # Fonts after a switch
//!
//! Iced fixes its default font (`Settings::default_font`) when the window opens. A text that does
//! not name its font would keep the old font after a switch, so every text of the suite names its
//! font from the active font: `typo::text`, and `.font(typo::body().font)` (or the level it uses) on
//! text inputs, text editors, pick lists (their menus take the same font) and rich text. Only Iced's
//! own fallback for missing glyphs stays the system's. Only a switch of the font loads font files
//! ([`load_fonts`]); a switch of design or colour scheme loads nothing.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, PoisonError, RwLock};
use std::time::Duration;

use iced::futures::Stream;
use iced::{Font, Subscription, Task, Theme};
use torro_core::design::{
    DesignError, Face, FontRole, Level, Listed, Mode, STANDARD_ID, TypeScale, Weight, list, list_color_schemes,
    list_fonts, load, load_color_scheme, load_font,
};
use torro_ipc::{Request, Response};

pub use torro_core::design::{ColorScheme, Design, FontSet, Selection};

use crate::source::Source;
use crate::theme::{self, Palette};

#[cfg(test)]
mod legacy;

/// The three active parts and what `torro_gui` derives from them once per switch, so the token
/// functions only copy values out.
pub(crate) struct Tokens {
    /// The active design.
    pub design: Arc<Design>,
    /// The active font.
    pub font: Arc<FontSet>,
    /// The active colour scheme.
    pub scheme: Arc<ColorScheme>,
    /// The design's levels as drawn in the font: [`Selection::level`] for every level.
    pub drawn: TypeScale,
    /// The Iced colours of both modes, with the derived ones (`attention_text`, `error_text`).
    pub dark: Palette,
    pub light: Palette,
    /// The Iced themes built from [`Tokens::dark`] and [`Tokens::light`].
    pub dark_theme: Theme,
    pub light_theme: Theme,
    /// The colours the sidebar draws with in each mode (`Palette::of_sidebar`).
    pub sidebar_dark: Palette,
    pub sidebar_light: Palette,
    /// The Iced themes the sidebar's content is drawn in (`theme::sidebar_theme`), told apart from
    /// the window's by identity: [`Tokens::sidebar_mode`].
    pub sidebar_dark_theme: Theme,
    pub sidebar_light_theme: Theme,
}

impl Tokens {
    fn new(design: Arc<Design>, font: Arc<FontSet>, scheme: Arc<ColorScheme>) -> Tokens {
        let dark = Palette::of(&scheme, Mode::Dark);
        let light = Palette::of(&scheme, Mode::Light);
        // The standard keeps the names the apps' tests know ("Torro Dark"); a scheme is named after itself.
        let name = |mode: &str| -> Cow<'static, str> {
            if scheme.is_standard() {
                Cow::Owned(format!("Torro {mode}"))
            } else {
                Cow::Owned(format!("{} {mode}", scheme.about.name))
            }
        };
        let dark_theme = theme::iced_theme(name("Dark"), &dark);
        let light_theme = theme::iced_theme(name("Light"), &light);
        let sidebar_dark = Palette::of_sidebar(&scheme, Mode::Dark);
        let sidebar_light = Palette::of_sidebar(&scheme, Mode::Light);
        let sidebar_dark_theme = theme::iced_theme(name("Dark sidebar"), &sidebar_dark);
        let sidebar_light_theme = theme::iced_theme(name("Light sidebar"), &sidebar_light);
        Tokens {
            drawn: drawn(&design, &font),
            design,
            font,
            scheme,
            dark,
            light,
            dark_theme,
            light_theme,
            sidebar_dark,
            sidebar_light,
            sidebar_dark_theme,
            sidebar_light_theme,
        }
    }

    /// The mode of `theme` if it is one of the sidebar themes of these tokens, else `None`. By
    /// identity of the theme's palette (an `Arc` the clones share), not by name: cheap enough for
    /// every style call, and a sidebar theme's own brightness says nothing about the window's mode.
    pub fn sidebar_mode(&self, theme: &Theme) -> Option<Mode> {
        let same = |sidebar: &Theme| match (theme, sidebar) {
            (Theme::Custom(a), Theme::Custom(b)) => Arc::ptr_eq(a, b),
            _ => false,
        };
        if same(&self.sidebar_dark_theme) {
            Some(Mode::Dark)
        } else if same(&self.sidebar_light_theme) {
            Some(Mode::Light)
        } else {
            None
        }
    }

    /// The Iced font of `role` at `weight` in the active font.
    pub fn font(&self, role: FontRole, weight: Weight) -> Font {
        font_of(&self.font, role, weight)
    }
}

/// Every level of `design` as drawn in `font` ([`FontSet::apply`], which is what [`Selection::level`]
/// does for one level), so `typo` copies the level out instead of computing it on every text.
fn drawn(design: &Design, font: &FontSet) -> TypeScale {
    let t = &design.typo;
    let a = |level: Level| font.apply(level);
    TypeScale {
        micro: a(t.micro),
        caption: a(t.caption),
        meta: a(t.meta),
        body: a(t.body),
        body_strong: a(t.body_strong),
        read: a(t.read),
        title: a(t.title),
        headline: a(t.headline),
        display: a(t.display),
        hero: a(t.hero),
        countdown: a(t.countdown),
    }
}

static ACTIVE: LazyLock<RwLock<Arc<Tokens>>> = LazyLock::new(|| {
    RwLock::new(Arc::new(Tokens::new(
        Arc::new(Design::standard()),
        Arc::new(FontSet::standard()),
        Arc::new(ColorScheme::standard()),
    )))
});

/// Runs `f` on the active tokens. `f` must not call another token function: it runs under the
/// read lock, and a nested read could wait behind a writer.
pub(crate) fn with<R>(f: impl FnOnce(&Tokens) -> R) -> R {
    let tokens = ACTIVE.read().unwrap_or_else(PoisonError::into_inner);
    f(&tokens)
}

/// Replaces the parts that are `Some` and keeps the others: one new set of tokens, built under the
/// write lock so two switches at once never lose one another's part.
pub(crate) fn replace(design: Option<Design>, font: Option<FontSet>, scheme: Option<ColorScheme>) {
    let mut active = ACTIVE.write().unwrap_or_else(PoisonError::into_inner);
    let tokens = Tokens::new(
        design.map_or_else(|| Arc::clone(&active.design), Arc::new),
        font.map_or_else(|| Arc::clone(&active.font), Arc::new),
        scheme.map_or_else(|| Arc::clone(&active.scheme), Arc::new),
    );
    *active = Arc::new(tokens);
}

/// The design this process draws with. Cheap (an `Arc`); read it again after a switch instead of
/// keeping it.
pub fn active() -> Arc<Design> {
    with(|t| Arc::clone(&t.design))
}

/// Makes `design` the active one for the whole process: every token function returns its values
/// from the next call on; font and colour scheme stay. Takes a design that `torro_core::design::load`
/// accepted (merged and checked); nothing is checked here.
///
/// Apps rarely call this themselves: [`start`] and [`Follow`] do. Canvas caches keep what they drew;
/// apps clear them after a switch.
pub fn set(design: Design) {
    replace(Some(design), None, None);
}

/// Makes the three parts of `look` the active ones, as [`Follow`] does on a switch: the window draws
/// with them from the next frame. `Some` when a part changed, with the task that loads the font's
/// files when the font changed ([`load_fonts`], else a task that does nothing); `None` when all
/// three were active already.
pub fn show(look: Selection) -> Option<Task<Message>> {
    let Selection { design, font, scheme } = look;
    let (design, font, scheme) = with(|t| {
        (
            (design != *t.design).then_some(design),
            (font != *t.font).then_some(font),
            (scheme != *t.scheme).then_some(scheme),
        )
    });
    if design.is_none() && font.is_none() && scheme.is_none() {
        return None;
    }
    let fonts = font.as_ref().map_or_else(Task::none, load_fonts);
    replace(design, font, scheme);
    Some(fonts)
}

/// How long [`start`] waits for the core's answer before the window opens with the standard. A
/// core that is just starting answers with "not running" at once; this only bounds a core that
/// hangs. The first [`Follow::fetch`] brings the stored look later.
const START_WAIT: Duration = Duration::from_secs(1);

/// [`start`] for an app on `source`: the core's socket, none for demo data.
pub fn start_for(source: &Source, forced: Option<&str>) {
    let socket = match source {
        Source::Core(client) => Some(client.socket.as_path()),
        Source::Demo(_) => None,
    };
    start(socket, forced);
}

/// Sets the look a process starts with. Call in `main` before [`crate::settings`], which loads
/// the files of the active font, so the first frame is already drawn right.
///
/// `forced` is `--design <id>` (only with `--demo`, checked by [`is_choice`] when the parameters
/// are read) and wins. Otherwise the id the suite settings name (`Settings::design`), asked from the
/// core on `socket` (waiting at most a second). The design, font and colour scheme of that id are
/// drawn, each from its own folder ([`Folders::look`]). Without a socket (`--demo`) or without an
/// answer the standard stays. Never fails: a window always opens.
pub fn start(socket: Option<&Path>, forced: Option<&str>) {
    let id = match (forced, socket) {
        (Some(id), _) => id.to_string(),
        (None, Some(socket)) => match stored_now(socket) {
            Some(id) => id,
            None => return,
        },
        (None, None) => return,
    };
    let Selection { design, font, scheme } = Folders::user().look(&id);
    replace(Some(design), Some(font), Some(scheme));
}

/// The id the settings name, asked from the core on `socket` and waited for at most
/// [`START_WAIT`]; `None` when the core does not answer in time.
fn stored_now(socket: &Path) -> Option<String> {
    let socket = socket.to_path_buf();
    let (answer, answered) = std::sync::mpsc::channel();
    // On a thread of its own, so a hanging core is left behind after START_WAIT instead of
    // keeping the window from opening.
    std::thread::spawn(move || {
        let Ok(runtime) = iced::executor::Default::new() else { return };
        let response = runtime.block_on(torro_ipc::client::call(&socket, &Request::Settings));
        let _ = answer.send(response);
    });
    match answered.recv_timeout(START_WAIT) {
        Ok(Ok(Response::Settings { settings })) => {
            // The same answer tells which components are on (task W.2).
            crate::components::set_off(&settings.components_off);
            Some(settings.design)
        }
        _ => None,
    }
}

/// The three folders the parts are read from: the user's ([`Folders::user`]) or, in tests, others.
/// A folder that is `None` (no home directory, or a test that leaves it out) holds only the standard.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Folders {
    /// The designs folder (`torro_core::design::dir`).
    pub designs: Option<PathBuf>,
    /// The fonts folder (`torro_core::design::font_dir`).
    pub fonts: Option<PathBuf>,
    /// The colour schemes folder (`torro_core::design::color_scheme_dir`).
    pub color_schemes: Option<PathBuf>,
}

impl Folders {
    /// The user's folders in the core's data directory.
    pub fn user() -> Folders {
        Folders {
            designs: torro_core::design::dir(),
            fonts: torro_core::design::font_dir(),
            color_schemes: torro_core::design::color_scheme_dir(),
        }
    }

    /// The design, font and colour scheme `id`, each from its own folder: until task L.22 one id names
    /// all three. A part whose folder lacks `id` is the standard's (the colour scheme "nord" has no
    /// design and no font); one that breaks a rule too, said once on stderr with every reason (the
    /// control centre shows them as well). [`STANDARD_ID`] is all three standards.
    pub fn look(&self, id: &str) -> Selection {
        Selection {
            design: part(self.designs.as_deref(), id, "design", load, Design::standard),
            font: part(self.fonts.as_deref(), id, "font", load_font, FontSet::standard),
            scheme: part(self.color_schemes.as_deref(), id, "colour scheme", load_color_scheme, ColorScheme::standard),
        }
    }

    /// The ids `--design` takes, the standard first, then every id one of the three folders holds a
    /// valid part for, by name, each once.
    pub fn choices(&self) -> Vec<String> {
        fn valid<T>(dir: Option<&Path>, list: fn(&Path) -> Vec<Listed<T>>) -> Vec<String> {
            let listed = dir.map(list).unwrap_or_default();
            listed.into_iter().filter(|entry| entry.valid() && !entry.standard).map(|entry| entry.id).collect()
        }
        let mut ids = valid(self.designs.as_deref(), list);
        ids.extend(valid(self.fonts.as_deref(), list_fonts));
        ids.extend(valid(self.color_schemes.as_deref(), list_color_schemes));
        ids.sort();
        ids.dedup();
        ids.insert(0, STANDARD_ID.to_string());
        ids
    }
}

/// The part `id` from `dir` read by `load`, or `standard()` when the folder lacks it or it breaks a
/// rule (the latter said on stderr as `what`).
fn part<T>(
    dir: Option<&Path>,
    id: &str,
    what: &str,
    load: fn(&Path, &str) -> Result<T, Vec<DesignError>>,
    standard: fn() -> T,
) -> T {
    // The standard needs no folder; `load` answers it without touching the disk.
    match load(dir.unwrap_or(Path::new("")), id) {
        Ok(found) => found,
        Err(problems) if matches!(problems[..], [DesignError::NotFound { .. }]) => standard(),
        Err(problems) => {
            let reasons: Vec<String> = problems.iter().map(ToString::to_string).collect();
            eprintln!("the {what} {id:?} can't be used, drawing with the standard: {}", reasons.join(" "));
            standard()
        }
    }
}

/// Whether `--design <id>` names a look this process can draw with: the standard, or an id one of the
/// user's three folders holds a valid part for. [`choices`] lists them for the error.
pub fn is_choice(id: &str) -> bool {
    is_choice_in(&Folders::user(), id)
}

/// [`is_choice`] for the folders `folders`.
pub fn is_choice_in(folders: &Folders, id: &str) -> bool {
    folders.choices().iter().any(|choice| choice == id)
}

/// The ids `--design` takes, the standard first: what the error for an unknown one lists.
pub fn choices() -> Vec<String> {
    choices_in(&Folders::user())
}

/// [`choices`] for the folders `folders` ([`Folders::choices`]).
pub fn choices_in(folders: &Folders) -> Vec<String> {
    folders.choices()
}

/// What reaches a window about the look. The app wraps it in one variant of its own message
/// (`Message::Design(design::Message)`) and hands it to [`Follow::update`].
#[derive(Debug, Clone)]
pub enum Message {
    /// The id the suite settings name, as [`Follow::fetch`] asked; `Err` when the core did not
    /// answer, and the window keeps the look it has.
    Stored(Result<String, String>),
    /// A file of the new font is loaded. Nothing to do: the message alone makes Iced build the
    /// view again, so text laid out before the font was there is laid out anew.
    FontLoaded,
}

/// How one window follows the look the suite settings name, like [`crate::zoom::Zoom`] follows
/// the zoom. The parts themselves are the process's ([`active`], [`crate::font::active`],
/// [`crate::scheme::active`]); this keeps where to ask, where to read and what was asked for last.
#[derive(Debug, Clone)]
pub struct Follow {
    /// The core's socket; `None` with `--demo`, where the look stays as it started.
    socket: Option<PathBuf>,
    /// Where the parts are read from ([`Folders::user`]).
    folders: Folders,
    /// The id asked for last, so an unchanged setting costs nothing and a broken part is read
    /// once, not on every tick.
    chosen: String,
}

impl Follow {
    /// Follows the setting through the core on `socket` (`torro_ipc::socket_path()`); `None` keeps
    /// the look the process started with (`--demo`). Starts from the active design's id, so the
    /// first answer changes nothing when [`start`] already set it.
    pub fn new(socket: Option<PathBuf>) -> Follow {
        Follow { socket, folders: Folders::user(), chosen: active().id.clone() }
    }

    /// [`Follow::new`] for an app on `source`: the core's socket, or none for demo data.
    pub fn for_source(source: &Source) -> Follow {
        match source {
            Source::Core(client) => Follow::new(Some(client.socket.clone())),
            Source::Demo(_) => Follow::new(None),
        }
    }

    /// The same, with the designs read from `dir` instead of the user's designs folder, and the font
    /// and colour scheme always the standard's: for tests, which never touch the user's data.
    pub fn in_dir(self, dir: PathBuf) -> Follow {
        self.in_folders(Folders { designs: Some(dir), fonts: None, color_schemes: None })
    }

    /// The same, with all three parts read from `folders`: for tests.
    pub fn in_folders(self, folders: Folders) -> Follow {
        Follow { folders, ..self }
    }

    /// Asks the core for the id the settings name (`Request::Settings`). Call at start, so the
    /// window has it before the first poll of [`Follow::subscription`]. Without a socket (`--demo`)
    /// it does nothing.
    pub fn fetch(&self) -> Task<Message> {
        let Some(socket) = self.socket.clone() else { return Task::none() };
        Task::perform(async move { stored(&socket).await }, Message::Stored)
    }

    /// Asks the core every [`POLL`] for the id the settings name and reports it as
    /// [`Message::Stored`] **only when it differs** from the one asked for last: a local
    /// `Request::Settings` is cheap, but every message makes Iced build and draw the window again,
    /// so an unchanged setting must not produce one (design skill: continuous operation). A core
    /// that does not answer produces nothing either. Without a socket (`--demo`) there is nothing
    /// to follow. Subscribe always, also in glance views; the poll starts anew after each switch.
    pub fn subscription(&self) -> Subscription<Message> {
        match &self.socket {
            Some(socket) => Subscription::run_with((socket.clone(), self.chosen.clone()), watch),
            None => Subscription::none(),
        }
    }

    /// Handles `message`. `Some` when a part changed: the window draws with it from the next frame;
    /// the app clears its canvas caches, if it keeps any, and returns the task, which loads the new
    /// font's files when the font changed. `None` when nothing changed.
    pub fn update(&mut self, message: Message) -> Option<Task<Message>> {
        match message {
            Message::Stored(Ok(id)) => self.apply(&id),
            // The core is not answering: keep what we have.
            Message::Stored(Err(_)) | Message::FontLoaded => None,
        }
    }

    /// Switches the process to the design, font and colour scheme `id` (the setting as the window
    /// learnt it, from [`Follow::fetch`] or with other settings, as in the control centre), each from
    /// its own folder ([`Folders::look`]). `id` asked for last time does nothing. Returns what
    /// [`Follow::update`] returns ([`show`]).
    pub fn apply(&mut self, id: &str) -> Option<Task<Message>> {
        if id == self.chosen {
            return None;
        }
        self.chosen = id.to_string();
        show(self.folders.look(id))
    }
}

/// How often [`Follow::subscription`] asks the core for the chosen look: a choice in the control
/// centre reaches every open window within this time.
pub const POLL: Duration = Duration::from_secs(2);

/// The id the settings name, asked from the core on `socket`.
async fn stored(socket: &Path) -> Result<String, String> {
    match torro_ipc::client::call(socket, &Request::Settings).await {
        Ok(Response::Settings { settings }) => {
            crate::components::set_off(&settings.components_off);
            Ok(settings.design)
        }
        Ok(other) => Err(format!("unexpected answer: {other:?}")),
        Err(err) => Err(err.to_string()),
    }
}

/// The stream behind [`Follow::subscription`] for the core's socket and the id asked for last:
/// every [`POLL`] one question, an item only for a different answer.
fn watch((socket, chosen): &(PathBuf, String)) -> impl Stream<Item = Message> + use<> {
    let socket = socket.clone();
    iced::futures::stream::unfold(chosen.clone(), move |last| {
        let socket = socket.clone();
        async move {
            loop {
                tokio::time::sleep(POLL).await;
                if let Ok(id) = stored(&socket).await
                    && id != last
                {
                    return Some((Message::Stored(Ok(id.clone())), id));
                }
            }
        }
    })
}

/// Loads the files of `font` into Iced, the first time only: Iced keeps every font it loaded, so
/// switching back and forth, or showing a font's sample in the control centre, loads each font once
/// (by its id). The task ends in [`Message::FontLoaded`] per file, which makes Iced lay out text
/// again; nothing when they are loaded already. A switch of design or colour scheme never calls it.
pub fn load_fonts(font: &FontSet) -> Task<Message> {
    if !mark_fonts_loaded(&font.id) {
        return Task::none();
    }
    Task::batch(font_files(font).into_iter().map(|file| iced::font::load(file).map(|_| Message::FontLoaded)))
}

/// Notes that the files of the font `id` are loaded; `false` when they already were.
pub(crate) fn mark_fonts_loaded(id: &str) -> bool {
    static LOADED: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let mut loaded = LOADED.lock().unwrap_or_else(PoisonError::into_inner);
    if loaded.iter().any(|known| known == id) {
        return false;
    }
    loaded.push(id.to_string());
    true
}

/// The files of `font` in the form `iced::Settings::fonts` and `iced::font::load` take: each family
/// once, every weight it has. Built-in faces are the Inter files embedded in `torro_gui`
/// ([`crate::fonts`]); a font folder's files are read from it (a file that cannot be read is left
/// out, Iced then falls back to another font – the font was checked when it was loaded, so this only
/// happens when the folder changed since).
pub fn font_files(font: &FontSet) -> Vec<Cow<'static, [u8]>> {
    let mut seen: Vec<&str> = Vec::new();
    let mut files = Vec::new();
    for role in [FontRole::Text, FontRole::Heading, FontRole::Numbers] {
        let family = font.fonts.get(role);
        if seen.contains(&family.family.as_str()) {
            continue;
        }
        seen.push(&family.family);
        for weight in WEIGHTS {
            match family.face(weight) {
                Some(Face::Builtin) => files.extend(builtin(&family.family, weight).map(Cow::Borrowed)),
                Some(Face::File(path)) => files.extend(std::fs::read(path).ok().map(Cow::Owned)),
                None => {}
            }
        }
    }
    files
}

/// The file of one face of `font`: the family of `role` at exactly `weight`, resolved like
/// [`font_files`] (built-in faces from `torro_gui`, a folder's from it). `None` when the family has no
/// file for that weight, or a folder's file cannot be read any more. Picking another weight instead is
/// the caller's choice (the PDF tool takes the next heavier one for bold).
pub fn font_file(font: &FontSet, role: FontRole, weight: Weight) -> Option<Cow<'static, [u8]>> {
    let family = font.fonts.get(role);
    match family.face(weight)? {
        Face::Builtin => builtin(&family.family, weight).map(Cow::Borrowed),
        Face::File(path) => std::fs::read(path).ok().map(Cow::Owned),
    }
}

/// The weights a family can have, lightest first.
const WEIGHTS: [Weight; 4] = [Weight::Regular, Weight::Medium, Weight::Semibold, Weight::Bold];

/// The embedded file of a built-in face.
fn builtin(family: &str, weight: Weight) -> Option<&'static [u8]> {
    let [regular, medium, semibold, tabular, tabular_medium] = crate::fonts();
    match (family, weight) {
        ("Inter", Weight::Regular) => Some(regular),
        ("Inter", Weight::Medium) => Some(medium),
        ("Inter", Weight::Semibold) => Some(semibold),
        ("Inter Tabular", Weight::Regular) => Some(tabular),
        ("Inter Tabular", Weight::Medium) => Some(tabular_medium),
        _ => None,
    }
}

/// The Iced font of `role` at `weight` in the active font.
pub fn font(role: FontRole, weight: Weight) -> Font {
    with(|t| t.font(role, weight))
}

/// The Iced font of `role` at `weight` in `font`, which need not be the active one: for a sample
/// ("Aa" in the control centre). Draws in that font once [`load_fonts`] loaded it; until then Iced
/// falls back to another.
pub fn font_of(font: &FontSet, role: FontRole, weight: Weight) -> Font {
    Font { weight: iced_weight(weight), ..Font::with_name(family(&font.fonts.get(role).family)) }
}

/// The figures font (`numbers`) of the active font at `weight`, or at the nearest lighter weight
/// it has: the standard ships "Inter Tabular" only in Regular and Medium, so a SemiBold level
/// becomes tabular Medium. Regular when it has nothing lighter.
pub fn tabular_font(weight: iced::font::Weight) -> Font {
    let (name, weight) = with(|t| {
        let numbers = &t.font.fonts.numbers;
        let wanted = WEIGHTS.iter().rposition(|w| rank(iced_weight(*w)) <= rank(weight)).unwrap_or(0);
        let found = WEIGHTS[..=wanted].iter().rev().find(|w| numbers.face(**w).is_some()).copied();
        (numbers.family.clone(), found.unwrap_or(Weight::Regular))
    });
    Font { weight: iced_weight(weight), ..Font::with_name(family(&name)) }
}

fn iced_weight(weight: Weight) -> iced::font::Weight {
    match weight {
        Weight::Regular => iced::font::Weight::Normal,
        Weight::Medium => iced::font::Weight::Medium,
        Weight::Semibold => iced::font::Weight::Semibold,
        Weight::Bold => iced::font::Weight::Bold,
    }
}

/// The CSS number of an Iced weight (400 = normal), to compare weights.
fn rank(weight: iced::font::Weight) -> u16 {
    use iced::font::Weight as W;
    match weight {
        W::Thin => 100,
        W::ExtraLight => 200,
        W::Light => 300,
        W::Normal => 400,
        W::Medium => 500,
        W::Semibold => 600,
        W::Bold => 700,
        W::ExtraBold => 800,
        W::Black => 900,
    }
}

/// `name` as the `&'static str` Iced wants in `Font::with_name`. Each family is leaked once and
/// reused afterwards, so memory grows only with the families a process ever draws with.
pub(crate) fn family(name: &str) -> &'static str {
    static FAMILIES: Mutex<Vec<&'static str>> = Mutex::new(Vec::new());
    let mut families = FAMILIES.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(known) = families.iter().find(|known| **known == name) {
        return known;
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    families.push(leaked);
    leaked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{radius, size, space};
    use crate::{layout, shell, style, typo};
    use iced::widget::text::LineHeight;
    use torro_core::design::LevelName;

    /// The test "same picture" (L.2): with the standard design, every token is what the constants
    /// were before design packs ([`legacy`]).
    #[test]
    fn the_standard_design_gives_the_old_palettes() {
        assert_eq!(Palette::dark(), legacy::DARK, "dark palette, derived colours included");
        assert_eq!(Palette::light(), legacy::LIGHT, "light palette, derived colours included");
        assert_eq!(theme::theme().palette(), legacy::iced_palette(&legacy::DARK));
        assert_eq!(theme::light().palette(), legacy::iced_palette(&legacy::LIGHT));
        assert_eq!(theme::attention(), legacy::DARK.attention);
    }

    #[test]
    fn the_standard_design_gives_the_old_type_scale() {
        let levels = typo::all();
        assert_eq!(levels.len(), legacy::LEVELS.len());
        for (level, (name, size, line, font)) in levels.into_iter().zip(legacy::LEVELS) {
            assert_eq!(level.size.0, size, "size of {name}");
            assert_eq!(level.line_height, LineHeight::Absolute(iced::Pixels(line)), "line height of {name}");
            assert_eq!(level.font, font, "font of {name}");
        }
        // Tabular cuts: Regular stays Regular, everything heavier is the tabular Medium.
        assert_eq!(typo::meta().tabular().font, legacy::FONT_TABULAR);
        assert_eq!(typo::micro().tabular().font, legacy::FONT_TABULAR);
        assert_eq!(typo::body_strong().tabular().font, legacy::FONT_TABULAR_MEDIUM);
        assert_eq!(typo::display().tabular().font, legacy::FONT_TABULAR_MEDIUM);
        assert_eq!(
            [
                theme::font(),
                theme::font_medium(),
                theme::font_semibold(),
                theme::font_tabular(),
                theme::font_tabular_medium()
            ],
            [
                legacy::FONT,
                legacy::FONT_MEDIUM,
                legacy::FONT_SEMIBOLD,
                legacy::FONT_TABULAR,
                legacy::FONT_TABULAR_MEDIUM
            ]
        );
    }

    #[test]
    fn the_standard_design_gives_the_old_spacing_radii_and_sizes() {
        assert_eq!([space::xs(), space::s(), space::m(), space::l(), space::xl(), space::xxl()], legacy::SPACE);
        assert_eq!([radius::xs(), radius::sm(), radius::md(), radius::lg()], legacy::RADIUS);
        let now = [
            size::control_sm(),
            size::control(),
            size::control_lg(),
            size::row(),
            size::row_two(),
            size::row_sidebar(),
            size::row_group(),
            size::header(),
            size::sidebar(),
            size::props(),
            size::panel(),
            size::list(),
            size::list_medium(),
            size::hour(),
            size::axis(),
            size::axis_narrow(),
            layout::inset(),
            size::content_max(),
            size::form_max(),
            size::HAIRLINE,
            shell::rail(),
        ];
        for (value, (name, old)) in now.into_iter().zip(legacy::SIZES) {
            assert_eq!(value, old, "{name}");
        }
        assert_eq!(style::button_padding(), legacy::BUTTON_PADDING);
        assert_eq!(style::pick_list_padding(), legacy::PICK_LIST_PADDING);
    }

    #[test]
    fn the_standard_design_loads_the_old_fonts() {
        let settings = crate::settings();
        let embedded: Vec<&[u8]> = crate::fonts().to_vec();
        let loaded: Vec<&[u8]> = settings.fonts.iter().map(|f| f.as_ref()).collect();
        assert_eq!(loaded, embedded, "the five Inter files, in the old order");
        assert_eq!(settings.default_font, legacy::FONT);
        assert_eq!(settings.default_text_size, iced::Pixels(13.0));
    }

    /// Tokens of the standard design and font with `scheme`.
    fn tokens_of(scheme: ColorScheme) -> Tokens {
        Tokens::new(Arc::new(Design::standard()), Arc::new(FontSet::standard()), Arc::new(scheme))
    }

    #[test]
    fn tokens_follow_the_colour_scheme_they_are_built_from() {
        let mut scheme = ColorScheme::standard();
        scheme.colors.dark.accent = torro_core::design::Rgba::hex(0x0EA5A4);
        scheme.colors.light.text = torro_core::design::Rgba::hex(0x101010);
        let tokens = tokens_of(scheme);
        assert_eq!(tokens.dark.accent.into_rgba8(), [0x0E, 0xA5, 0xA4, 0xFF]);
        assert_eq!(tokens.dark_theme.palette().primary, tokens.dark.accent, "the Iced palette follows");
        // Derived colours are computed from the colour scheme, not taken from the old constants.
        assert_ne!(tokens.light.attention_text, legacy::LIGHT.attention_text);
        assert_eq!(tokens.dark_theme.to_string(), "Torro Dark");
    }

    #[test]
    fn a_sidebar_of_its_own_gets_its_own_theme() {
        use torro_core::design::Rgba;
        let mut scheme = ColorScheme::standard();
        let light = &mut scheme.colors.light;
        (light.sidebar, light.sidebar_hover, light.sidebar_selected) =
            (Rgba::hex(0x24292F), Rgba::hex(0x2F353C), Rgba::hex(0x3A4149));
        (light.sidebar_text, light.sidebar_text_2, light.sidebar_text_3) =
            (Rgba::hex(0xF6F8FA), Rgba::hex(0xD0D7DE), Rgba::hex(0xAEB7C1));
        let tokens = tokens_of(scheme);
        let side = tokens.sidebar_light;
        assert_eq!(
            (side.bg.into_rgba8(), side.text.into_rgba8()),
            ([0x24, 0x29, 0x2F, 0xFF], [0xF6, 0xF8, 0xFA, 0xFF])
        );
        assert_eq!(side.panel, side.bg, "surfaces made for the light content take the sidebar's");
        assert_eq!(side.error_text, tokens.light.error, "a dark sidebar derives as dark mode does");
        assert_eq!(tokens.light.bg, legacy::LIGHT.bg, "the content keeps its colours");
        assert_eq!(tokens.sidebar_dark, tokens.dark, "dark mode has no sidebar keys: as before");

        assert_eq!(tokens.sidebar_mode(&tokens.sidebar_light_theme.clone()), Some(Mode::Light));
        assert_eq!(tokens.sidebar_mode(&tokens.sidebar_dark_theme), Some(Mode::Dark));
        assert_eq!(tokens.sidebar_mode(&tokens.light_theme), None);
        assert_eq!(tokens.sidebar_mode(&Theme::Light), None);
        // A theme with the same name and colours is not the sidebar's: identity counts.
        let lookalike = theme::iced_theme(tokens.sidebar_light_theme.to_string().into(), &side);
        assert_eq!(tokens.sidebar_mode(&lookalike), None);
    }

    /// Held by the tests that switch the process's look, so they do not see each other's.
    static SWITCHING: Mutex<()> = Mutex::new(());

    /// Writes `toml` after `format` and `[about]` (named "Torro", so tests running beside see the
    /// standard's theme names) as the file of the part folder `folder`/`id`.
    fn write(folder: &Path, id: &str, file: &str, toml: &str) {
        std::fs::create_dir_all(folder.join(id)).expect("part folder");
        let text = format!("format = 1\n[about]\nname = \"Torro\"\nversion = \"1.0.0\"\n{toml}");
        std::fs::write(folder.join(id).join(file), text).expect("toml");
    }

    /// Three part folders in a temp dir: the designs `twin` (the standard's values under its own id)
    /// and `pale` (a pack from before task L.19 with `[colors.dark]`: an unknown key, refused), the
    /// colour schemes `twin` and `harbor` (only a colour scheme). Every part that can be drawn has the
    /// standard's values, so the tests running beside, which read the tokens, see no change.
    /// The caller removes the returned root (no `tempfile` in this crate).
    fn folders() -> (PathBuf, Folders) {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("torro-gui-look-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let folders = Folders {
            designs: Some(root.join("designs")),
            fonts: Some(root.join("fonts")),
            color_schemes: Some(root.join("color-schemes")),
        };
        let (designs, schemes) = (root.join("designs"), root.join("color-schemes"));
        write(&designs, "twin", "design.toml", "");
        write(&designs, "pale", "design.toml", "[colors.dark]\ntext = \"#202020\"\n");
        write(&schemes, "twin", "color-scheme.toml", "");
        write(&schemes, "harbor", "color-scheme.toml", "");
        (root, folders)
    }

    /// Task L.21: until task L.22 one id names all three parts, each from its own folder; only a
    /// switch of the font loads font files, and a part that is gone or broken is the standard's.
    #[test]
    fn follow_takes_all_three_parts_of_one_id_and_loads_fonts_only_for_a_new_font() {
        let _switching = SWITCHING.lock().unwrap_or_else(PoisonError::into_inner);
        let (root, folders) = folders();
        // A font of its own id with the standard's families: `crate::settings()` in a test running
        // beside marks only the active font as loaded, never this one.
        let fresh = format!("fresh-{}", std::process::id());
        write(folders.fonts.as_deref().expect("fonts"), &fresh, "font.toml", "");
        write(folders.designs.as_deref().expect("designs"), &fresh, "design.toml", "");
        mark_fonts_loaded(STANDARD_ID);
        let mut follow = Follow::new(None).in_folders(folders);
        assert!(follow.fetch().units() == 0, "without a socket nothing is asked");

        let switched = follow.update(Message::Stored(Ok("twin".into()))).expect("the design and the scheme changed");
        assert_eq!((active().id.as_str(), crate::scheme::active().id.as_str()), ("twin", "twin"));
        assert!(crate::font::active().is_standard(), "there is no font `twin`: the standard's");
        assert_eq!(switched.units(), 0, "design and colour scheme load nothing");

        let switched = follow.update(Message::Stored(Ok(fresh.clone()))).expect("the font changed");
        assert_eq!((active().id.as_str(), crate::font::active().id.as_str()), (fresh.as_str(), fresh.as_str()));
        assert!(crate::scheme::active().is_standard(), "there is no colour scheme of that id");
        assert!(switched.units() > 0, "a new font loads its files");
        assert!(follow.update(Message::Stored(Ok(fresh.clone()))).is_none(), "the same setting again");
        assert!(follow.update(Message::Stored(Err("core is not running".into()))).is_none());
        assert_eq!(crate::font::active().id, fresh, "a core that does not answer changes nothing");

        assert!(follow.update(Message::Stored(Ok("pale".into()))).is_some(), "back to the standard");
        assert!(active().is_standard() && crate::font::active().is_standard(), "a broken design is never drawn");
        assert!(follow.update(Message::Stored(Ok("gone".into()))).is_none(), "already the standard");

        assert!(follow.apply("harbor").is_some(), "a colour scheme alone is a look too");
        assert_eq!(crate::scheme::active().id, "harbor");
        assert!(follow.apply(STANDARD_ID).is_some());
        assert!(active().is_standard() && crate::scheme::active().is_standard());
        let _ = std::fs::remove_dir_all(root);
    }

    /// Task L.21: the token functions read the three active parts – typo the design's levels as
    /// drawn in the font, the palette the colour scheme – and a switch of one part keeps the others.
    #[test]
    fn each_part_switches_on_its_own() {
        let _switching = SWITCHING.lock().unwrap_or_else(PoisonError::into_inner);
        let mut twin = ColorScheme::standard();
        twin.id = "twin".into();
        crate::scheme::set(twin);
        assert_eq!(crate::scheme::active().id, "twin");
        assert!(active().is_standard() && crate::font::active().is_standard(), "the others stay");
        assert_eq!(theme::theme().to_string(), "Torro Dark", "named after the scheme, which says Torro");
        let mut font = FontSet::standard();
        font.id = "twin".into();
        crate::font::set(font);
        assert_eq!((crate::font::active().id.as_str(), crate::scheme::active().id.as_str()), ("twin", "twin"));
        assert_eq!(crate::typo::body().size.0, 13.0, "Inter by another id: the same size");
        replace(Some(Design::standard()), Some(FontSet::standard()), Some(ColorScheme::standard()));
        assert!(crate::scheme::active().is_standard() && crate::font::active().is_standard());
    }

    /// Task L.21: every example of the repository, as all three parts of its id, draws its levels as
    /// `Selection::level` gives them – task L.20 holds those within 0.5 px of the look before the
    /// split – in the families of its font and the colours of its colour scheme.
    #[test]
    fn the_examples_draw_as_their_selection() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let folders = Folders {
            designs: Some(repo.join("designs")),
            fonts: Some(repo.join("fonts")),
            color_schemes: Some(repo.join("color-schemes")),
        };
        let ids = folders.choices();
        assert!(ids.len() >= 13, "the twelve examples and the standard: {ids:?}");
        for id in ids {
            let look = folders.look(&id);
            let tokens =
                Tokens::new(Arc::new(look.design.clone()), Arc::new(look.font.clone()), Arc::new(look.scheme.clone()));
            for name in LevelName::ALL {
                assert_eq!(tokens.drawn.get(name), look.level(name), "{id} {}", name.key());
            }
            let body = tokens.drawn.body;
            let family = iced::font::Family::Name(super::family(&look.font.fonts.text.family));
            assert_eq!(tokens.font(body.font, body.weight).family, family, "{id}");
            assert_eq!(tokens.dark.accent, theme::rgba(look.scheme.colors.dark.accent), "{id}");
        }
        let plex = folders.look("plex");
        assert_eq!((plex.level(LevelName::Body).size, plex.level(LevelName::Body).line), (14.0, 22.0), "as before");
        assert_eq!(plex.font.fonts.text.family, "IBM Plex Sans");
        assert!(plex.scheme.logo.mark.is_some(), "the logo comes with the colour scheme");
        let nord = folders.look("nord");
        assert!(nord.design.is_standard() && nord.font.is_standard() && !nord.scheme.is_standard(), "colours only");
    }

    /// Nacharbeit L.3 (task L.4): a window follows the setting through a poll of its own, which
    /// exists only where there is a core to ask.
    #[test]
    fn the_poll_runs_only_with_a_core() {
        assert_eq!(Follow::new(None).subscription().units(), 0, "--demo: nothing to follow");
        let socket = std::env::temp_dir().join(format!("torro-gui-poll-{}", std::process::id()));
        assert_eq!(Follow::new(Some(socket)).subscription().units(), 1);
        assert_eq!(POLL, Duration::from_secs(2), "a choice reaches every window within two seconds");
    }

    /// The poll asks a core that does not answer and reports nothing: no message, so the window is
    /// neither updated nor drawn again.
    #[test]
    fn the_poll_is_silent_while_nothing_answers() {
        use iced::futures::StreamExt;
        let nowhere = std::env::temp_dir().join(format!("torro-gui-silent-{}", std::process::id()));
        let runtime = tokio::runtime::Builder::new_current_thread().enable_time().build().expect("runtime");
        let first = runtime.block_on(async {
            let mut stream = std::pin::pin!(watch(&(nowhere, "torro".to_string())));
            tokio::time::timeout(POLL + Duration::from_millis(500), stream.next()).await
        });
        assert!(first.is_err(), "no item after a poll");
    }

    #[test]
    fn one_font_file_is_the_face_of_that_weight_or_none() {
        let standard = FontSet::standard();
        let [regular, _, semibold, _, _] = crate::fonts();
        assert_eq!(font_file(&standard, FontRole::Text, Weight::Regular).as_deref(), Some(regular));
        assert_eq!(font_file(&standard, FontRole::Text, Weight::Semibold).as_deref(), Some(semibold));
        assert_eq!(font_file(&standard, FontRole::Text, Weight::Bold), None, "Inter ships no bold file");

        let dir = std::env::temp_dir().join(format!("torro-gui-font-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("dir");
        let file = dir.join("Sample-Bold.ttf");
        std::fs::write(&file, b"bold bytes").expect("font file");
        let mut pack = FontSet::standard();
        pack.fonts.heading.bold = Some(Face::File(file.clone()));
        assert_eq!(font_file(&pack, FontRole::Heading, Weight::Bold).as_deref(), Some(&b"bold bytes"[..]));
        std::fs::remove_file(&file).expect("remove");
        assert_eq!(font_file(&pack, FontRole::Heading, Weight::Bold), None, "a file gone since is none");
    }

    #[test]
    fn the_files_of_a_font_are_loaded_once() {
        let mut once = FontSet::standard();
        once.id = format!("fonts-once-{}", std::process::id());
        assert!(load_fonts(&once).units() > 0, "the first time: the files");
        assert_eq!(load_fonts(&once).units(), 0, "afterwards nothing");
    }

    #[test]
    fn a_sample_font_names_the_pack_family() {
        let mut pack = FontSet::standard();
        pack.fonts.text.family = "Sample Sans".to_string();
        let font = font_of(&pack, FontRole::Text, Weight::Medium);
        assert_eq!(font.family, iced::font::Family::Name("Sample Sans"));
        assert_eq!(font.weight, iced::font::Weight::Medium);
    }

    /// `--design` takes every id one of the three folders holds a valid part for (task L.21).
    #[test]
    fn only_valid_parts_are_choices() {
        let (root, folders) = folders();
        assert_eq!(choices_in(&folders), ["torro", "harbor", "twin"], "the standard first, each once, never broken");
        assert_eq!(choices_in(&Folders::default()), ["torro"]);
        assert!(is_choice_in(&folders, "twin") && is_choice_in(&folders, "harbor"));
        assert!(is_choice_in(&Folders::default(), "torro"), "the standard needs no folder");
        for id in ["pale", "gone", "../twin"] {
            assert!(!is_choice_in(&folders, id), "{id}");
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn without_a_core_the_start_keeps_the_standard() {
        let _switching = SWITCHING.lock().unwrap_or_else(PoisonError::into_inner);
        let nowhere = std::env::temp_dir().join(format!("torro-gui-no-core-{}", std::process::id()));
        start(Some(&nowhere), None);
        start(None, None);
        assert!(active().is_standard());
        start(None, Some("torro"));
        assert!(active().is_standard());
    }

    #[test]
    fn set_replaces_the_active_design() {
        let _switching = SWITCHING.lock().unwrap_or_else(PoisonError::into_inner);
        // Same values as the standard under another id: tests running beside this one keep reading
        // the standard tokens.
        let mut twin = Design::standard();
        twin.id = "twin".to_string();
        set(twin);
        assert_eq!(active().id, "twin");
        assert_eq!(space::m(), 12.0);
        set(Design::standard());
        assert!(active().is_standard());
    }

    #[test]
    fn families_are_leaked_once() {
        let a = family("Some Family");
        let b = family(&String::from("Some Family"));
        assert!(std::ptr::eq(a, b));
    }

    #[test]
    fn tabular_falls_back_to_a_lighter_weight() {
        use iced::font::Weight as W;
        assert_eq!(tabular_font(W::Semibold).weight, W::Medium);
        assert_eq!(tabular_font(W::Bold).weight, W::Medium);
        assert_eq!(tabular_font(W::Normal).weight, W::Normal);
        assert_eq!(tabular_font(W::Thin).weight, W::Normal);
    }
}
