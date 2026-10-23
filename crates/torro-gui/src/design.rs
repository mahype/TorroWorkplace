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
//! # Following the settings (tasks L.3, L.21 and L.22)
//!
//! The suite settings name the three parts apart (`Settings::design`, `font`, `color_scheme`:
//! [`Chosen`]), and every window draws with each from its own folder ([`Folders::select`]): a part
//! whose folder lacks the id, or that breaks a rule, is the standard's – never a crash. A window
//! follows a change at once, the way the appearance and the zoom do, and reads only the part that
//! changed. An app wires it up in four places:
//!
//! ```ignore
//! use torro_gui::design::{self, Follow, Forced};
//!
//! // main, before `torro_gui::settings()`: the first frame already has the right look and fonts.
//! // `--design`, `--font` and `--color-scheme` of `--demo` win.
//! design::start_for(&source, &Forced { design: launch.design.clone(), font: launch.font.clone(), color_scheme: launch.color_scheme.clone() });
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
use torro_core::settings::Settings;
use torro_ipc::{Request, Response};

pub use torro_core::design::{ColorScheme, Design, FontSet, Part, Selection};

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
    /// The colours of the content of a chosen row drawn in the accent colour (issue #291, `[list]
    /// selection = "fill"`): the window's, with every text colour `on_accent` (`Palette::on_accent_row`).
    pub on_accent_dark: Palette,
    pub on_accent_light: Palette,
    /// The Iced themes that content is drawn in (`theme::on_accent_theme`), told apart by identity
    /// like the sidebar's: [`Tokens::on_accent_mode`].
    pub on_accent_dark_theme: Theme,
    pub on_accent_light_theme: Theme,
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
        let on_accent_dark = dark.on_accent_row();
        let on_accent_light = light.on_accent_row();
        let on_accent_dark_theme = theme::iced_theme(name("Dark on accent"), &on_accent_dark);
        let on_accent_light_theme = theme::iced_theme(name("Light on accent"), &on_accent_light);
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
            on_accent_dark,
            on_accent_light,
            on_accent_dark_theme,
            on_accent_light_theme,
        }
    }

    /// The mode of `theme` if it is one of the sidebar themes of these tokens, else `None`. By
    /// identity of the theme's palette (an `Arc` the clones share), not by name: cheap enough for
    /// every style call, and a sidebar theme's own brightness says nothing about the window's mode.
    pub fn sidebar_mode(&self, theme: &Theme) -> Option<Mode> {
        mode_of(theme, &self.sidebar_dark_theme, &self.sidebar_light_theme)
    }

    /// The mode of `theme` if it is one of the on-accent themes of these tokens (issue #291), else
    /// `None`; by identity, as [`Tokens::sidebar_mode`].
    pub fn on_accent_mode(&self, theme: &Theme) -> Option<Mode> {
        mode_of(theme, &self.on_accent_dark_theme, &self.on_accent_light_theme)
    }

    /// The Iced font of `role` at `weight` in the active font.
    pub fn font(&self, role: FontRole, weight: Weight) -> Font {
        font_of(&self.font, role, weight)
    }
}

/// `Some(Mode::Dark)` when `theme` is `dark` itself (a clone sharing its palette), `Some(Mode::Light)`
/// when it is `light`, else `None`.
fn mode_of(theme: &Theme, dark: &Theme, light: &Theme) -> Option<Mode> {
    let same = |other: &Theme| match (theme, other) {
        (Theme::Custom(a), Theme::Custom(b)) => Arc::ptr_eq(a, b),
        _ => false,
    };
    if same(dark) {
        Some(Mode::Dark)
    } else if same(light) {
        Some(Mode::Light)
    } else {
        None
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
    show_parts(Some(design), Some(font), Some(scheme))
}

/// [`show`] for the parts that are `Some`; the others stay as they are, so a switch of one part never
/// touches the other two.
fn show_parts(design: Option<Design>, font: Option<FontSet>, scheme: Option<ColorScheme>) -> Option<Task<Message>> {
    let (design, font, scheme) = with(|t| {
        (
            design.filter(|design| *design != *t.design),
            font.filter(|font| *font != *t.font),
            scheme.filter(|scheme| *scheme != *t.scheme),
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

/// The ids of the three parts the suite settings name (task L.22): [`Settings::design`],
/// [`Settings::font`] and [`Settings::color_scheme`]. [`Folders::select`] reads them, [`Follow`]
/// follows them.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Chosen {
    /// The design's id, `torro` for the standard.
    pub design: String,
    /// The font's id, `torro` for the standard.
    pub font: String,
    /// The colour scheme's id, `torro` for the standard.
    pub color_scheme: String,
}

impl Chosen {
    /// Torro, Torro, Torro.
    pub fn standard() -> Chosen {
        Chosen::of_ids(STANDARD_ID, STANDARD_ID, STANDARD_ID)
    }

    /// The three ids `settings` name; an empty one (a core from before task L.22 sends no font or
    /// colour scheme) is the standard.
    pub fn of(settings: &Settings) -> Chosen {
        let named = |id: &str| if id.is_empty() { STANDARD_ID.to_string() } else { id.to_string() };
        Chosen {
            design: named(&settings.design),
            font: named(&settings.font),
            color_scheme: named(&settings.color_scheme),
        }
    }

    /// The ids of the parts this process draws with now.
    pub fn active() -> Chosen {
        with(|t| Chosen::of_ids(&t.design.id, &t.font.id, &t.scheme.id))
    }

    fn of_ids(design: &str, font: &str, color_scheme: &str) -> Chosen {
        Chosen { design: design.to_string(), font: font.to_string(), color_scheme: color_scheme.to_string() }
    }
}

/// `--design`, `--font` and `--color-scheme` of `--demo` (task L.22): the parts a process draws with
/// whatever the settings name, each checked by [`is_choice`] when the parameters are read. A part
/// left `None` is the stored one (the standard without a core).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Forced {
    pub design: Option<String>,
    pub font: Option<String>,
    pub color_scheme: Option<String>,
}

impl Forced {
    /// Whether no part is forced.
    pub fn is_empty(&self) -> bool {
        self.design.is_none() && self.font.is_none() && self.color_scheme.is_none()
    }

    /// `chosen` with each part this names in its place.
    pub fn over(&self, chosen: Chosen) -> Chosen {
        Chosen {
            design: self.design.clone().unwrap_or(chosen.design),
            font: self.font.clone().unwrap_or(chosen.font),
            color_scheme: self.color_scheme.clone().unwrap_or(chosen.color_scheme),
        }
    }
}

/// [`start`] for an app on `source`: the core's socket, none for demo data.
pub fn start_for(source: &Source, forced: &Forced) {
    let socket = match source {
        Source::Core(client) => Some(client.socket.as_path()),
        Source::Demo(_) => None,
    };
    start(socket, forced);
}

/// Sets the look a process starts with. Call in `main` before [`crate::settings`], which loads
/// the files of the active font, so the first frame is already drawn right.
///
/// The parts `forced` names (`--design`, `--font`, `--color-scheme`, only with `--demo`) win. The
/// others are the ones the suite settings name ([`Chosen::of`]), asked from the core on `socket`
/// (waiting at most a second), each read from its own folder ([`Folders::select`]). Without a socket
/// (`--demo`) or without an answer the standard stays for every part not forced. Never fails: a window
/// always opens.
pub fn start(socket: Option<&Path>, forced: &Forced) {
    let stored = socket.and_then(stored_now);
    if stored.is_none() && forced.is_empty() {
        return;
    }
    let chosen = forced.over(stored.unwrap_or_else(Chosen::standard));
    let Selection { design, font, scheme } = Folders::user().select(&chosen);
    replace(Some(design), Some(font), Some(scheme));
}

/// The ids the settings name, asked from the core on `socket` and waited for at most
/// [`START_WAIT`]; `None` when the core does not answer in time.
fn stored_now(socket: &Path) -> Option<Chosen> {
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
            Some(Chosen::of(&settings))
        }
        _ => None,
    }
}

/// The three folders the parts are read from: the user's ([`Folders::user`]) or, in tests, others.
/// A folder that is `None` (no home directory, or a test that leaves it out) lists only what is built
/// in: the three designs (`torro_core::design::BUILT_IN_IDS`), the standard font and colour scheme.
/// [`part`] still looks any other id up under the working directory then.
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

    /// The design, font and colour scheme `chosen` names, each from its own folder (task L.22). A part
    /// whose folder lacks its id is the standard's; one that breaks a rule too, said once on stderr
    /// with every reason (the control centre shows them as well).
    pub fn select(&self, chosen: &Chosen) -> Selection {
        Selection {
            design: self.design(&chosen.design),
            font: self.font(&chosen.font),
            scheme: self.scheme(&chosen.color_scheme),
        }
    }

    /// The design, font and colour scheme of one `id` together ([`Folders::select`] with the same id
    /// three times): how the control centre previews a design with what shares its id, until task
    /// L.23 previews it apart.
    pub fn look(&self, id: &str) -> Selection {
        self.select(&Chosen::of_ids(id, id, id))
    }

    /// The design `id`, or the standard's.
    fn design(&self, id: &str) -> Design {
        part(self.designs.as_deref(), id, "design", load, Design::standard)
    }

    /// The font `id`, or the standard's.
    fn font(&self, id: &str) -> FontSet {
        part(self.fonts.as_deref(), id, "font", load_font, FontSet::standard)
    }

    /// The colour scheme `id`, or the standard's.
    fn scheme(&self, id: &str) -> ColorScheme {
        part(self.color_schemes.as_deref(), id, "colour scheme", load_color_scheme, ColorScheme::standard)
    }

    /// The ids `--design`, `--font` or `--color-scheme` take for `part` (task L.22): the standard
    /// first, then the other built-in ones (the designs Compact and Console, issue #291, also without
    /// a folder), then every id the folder of `part` holds a valid entry for, by name.
    pub fn choices(&self, part: Part) -> Vec<String> {
        fn valid<T>(dir: Option<&Path>, list: fn(&Path) -> Vec<Listed<T>>) -> Vec<String> {
            // A missing folder lists the built-in entries alone, as `part` reads them without one.
            let listed = list(dir.unwrap_or(Path::new("")));
            listed.into_iter().filter(|entry| entry.valid() && !entry.standard).map(|entry| entry.id).collect()
        }
        let mut ids = match part {
            Part::Design => valid(self.designs.as_deref(), list),
            Part::Font => valid(self.fonts.as_deref(), list_fonts),
            Part::ColorScheme => valid(self.color_schemes.as_deref(), list_color_schemes),
        };
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

/// Whether `--design <id>` (`part` [`Part::Design`]), `--font <id>` ([`Part::Font`]) or
/// `--color-scheme <id>` ([`Part::ColorScheme`]) names a part this process can draw with: the
/// standard, or an id the user's folder of `part` holds a valid entry for (task L.22: `--design nord`
/// is refused, `nord` is a colour scheme). [`choices`] lists them for the error.
pub fn is_choice(part: Part, id: &str) -> bool {
    is_choice_in(&Folders::user(), part, id)
}

/// [`is_choice`] for the folders `folders`.
pub fn is_choice_in(folders: &Folders, part: Part, id: &str) -> bool {
    folders.choices(part).iter().any(|choice| choice == id)
}

/// The ids the flag of `part` takes, the standard first: what the error for an unknown one lists.
pub fn choices(part: Part) -> Vec<String> {
    choices_in(&Folders::user(), part)
}

/// [`choices`] for the folders `folders` ([`Folders::choices`]).
pub fn choices_in(folders: &Folders, part: Part) -> Vec<String> {
    folders.choices(part)
}

/// What reaches a window about the look. The app wraps it in one variant of its own message
/// (`Message::Design(design::Message)`) and hands it to [`Follow::update`].
#[derive(Debug, Clone)]
pub enum Message {
    /// The ids the suite settings name, as [`Follow::fetch`] asked; `Err` when the core did not
    /// answer, and the window keeps the look it has.
    Stored(Result<Chosen, String>),
    /// A file of the new font is loaded. Nothing to do: the message alone makes Iced build the
    /// view again, so text laid out before the font was there is laid out anew.
    FontLoaded,
}

/// How one window follows the design, font and colour scheme the suite settings name, like
/// [`crate::zoom::Zoom`] follows the zoom. The parts themselves are the process's ([`active`],
/// [`crate::font::active`], [`crate::scheme::active`]); this keeps where to ask, where to read and
/// what was asked for last.
#[derive(Debug, Clone)]
pub struct Follow {
    /// The core's socket; `None` with `--demo`, where the look stays as it started.
    socket: Option<PathBuf>,
    /// Where the parts are read from ([`Folders::user`]).
    folders: Folders,
    /// The ids asked for last, so an unchanged setting costs nothing, a broken part is read once,
    /// not on every tick, and a change reads only the part that changed.
    chosen: Chosen,
}

impl Follow {
    /// Follows the setting through the core on `socket` (`torro_ipc::socket_path()`); `None` keeps
    /// the look the process started with (`--demo`). Starts from the ids of the active parts, so
    /// the first answer changes nothing when [`start`] already set them.
    pub fn new(socket: Option<PathBuf>) -> Follow {
        Follow { socket, folders: Folders::user(), chosen: Chosen::active() }
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

    /// Asks the core for the ids the settings name (`Request::Settings`). Call at start, so the
    /// window has it before the first poll of [`Follow::subscription`]. Without a socket (`--demo`)
    /// it does nothing.
    pub fn fetch(&self) -> Task<Message> {
        let Some(socket) = self.socket.clone() else { return Task::none() };
        Task::perform(async move { stored(&socket).await }, Message::Stored)
    }

    /// Asks the core every [`POLL`] for the ids the settings name and reports them as
    /// [`Message::Stored`] **only when one differs** from the ones asked for last: a local
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
            Message::Stored(Ok(chosen)) => self.follow(chosen),
            // The core is not answering: keep what we have.
            Message::Stored(Err(_)) | Message::FontLoaded => None,
        }
    }

    /// Switches the process to the parts `chosen` names (the settings as the window learnt them,
    /// from [`Follow::fetch`] or with other settings), reading only the parts whose id differs from
    /// the one asked for last: a new font alone touches neither design nor colour scheme, and only
    /// it loads font files. The same ids again do nothing. Returns what [`Follow::update`] returns.
    pub fn follow(&mut self, chosen: Chosen) -> Option<Task<Message>> {
        if chosen == self.chosen {
            return None;
        }
        let design = (chosen.design != self.chosen.design).then(|| self.folders.design(&chosen.design));
        let font = (chosen.font != self.chosen.font).then(|| self.folders.font(&chosen.font));
        let scheme =
            (chosen.color_scheme != self.chosen.color_scheme).then(|| self.folders.scheme(&chosen.color_scheme));
        self.chosen = chosen;
        show_parts(design, font, scheme)
    }

    /// Switches only the design to `id` ([`Follow::follow`] with the font and colour scheme asked
    /// for last), as the control centre does when a person picks a design: choosing a design never
    /// changes font or colour scheme (task L.22).
    pub fn apply(&mut self, id: &str) -> Option<Task<Message>> {
        let chosen = Chosen { design: id.to_string(), ..self.chosen.clone() };
        self.follow(chosen)
    }
}

/// How often [`Follow::subscription`] asks the core for the chosen look: a choice in the control
/// centre reaches every open window within this time.
pub const POLL: Duration = Duration::from_secs(2);

/// The ids the settings name, asked from the core on `socket`.
async fn stored(socket: &Path) -> Result<Chosen, String> {
    match torro_ipc::client::call(socket, &Request::Settings).await {
        Ok(Response::Settings { settings }) => {
            crate::components::set_off(&settings.components_off);
            Ok(Chosen::of(&settings))
        }
        Ok(other) => Err(format!("unexpected answer: {other:?}")),
        Err(err) => Err(err.to_string()),
    }
}

/// The stream behind [`Follow::subscription`] for the core's socket and the ids asked for last:
/// every [`POLL`] one question, an item only for a different answer.
fn watch((socket, chosen): &(PathBuf, Chosen)) -> impl Stream<Item = Message> + use<> {
    let socket = socket.clone();
    iced::futures::stream::unfold(chosen.clone(), move |last| {
        let socket = socket.clone();
        async move {
            loop {
                tokio::time::sleep(POLL).await;
                if let Ok(chosen) = stored(&socket).await
                    && chosen != last
                {
                    return Some((Message::Stored(Ok(chosen.clone())), chosen));
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

    /// Issue #291 (task L.34): the standard has new measures, read from its file; every level is the
    /// file's level as Inter draws it (unchanged), in the fonts the suite always had.
    #[test]
    fn the_standard_design_draws_the_type_scale_of_its_file() {
        let t = Design::standard().typo;
        let levels = [
            (typo::micro(), t.micro, legacy::FONT_TABULAR),
            (typo::caption(), t.caption, legacy::FONT_MEDIUM),
            (typo::meta(), t.meta, legacy::FONT),
            (typo::body(), t.body, legacy::FONT),
            (typo::body_strong(), t.body_strong, legacy::FONT_MEDIUM),
            (typo::read(), t.read, legacy::FONT),
            (typo::title(), t.title, legacy::FONT_SEMIBOLD),
            (typo::headline(), t.headline, legacy::FONT_SEMIBOLD),
            (typo::display(), t.display, legacy::FONT_SEMIBOLD),
            (typo::hero(), t.hero, legacy::FONT_SEMIBOLD),
            (typo::countdown(), t.countdown, legacy::FONT_TABULAR_MEDIUM),
        ];
        for (drawn, level, font) in levels {
            assert_eq!(drawn.size.0, level.size);
            assert_eq!(drawn.line_height, LineHeight::Absolute(iced::Pixels(level.line)), "{}", level.size);
            assert_eq!(drawn.font, font, "font of the {} level", level.size);
        }
        // The new standard's text (the table in `docs/tasks/design-pakete.md`).
        assert_eq!((typo::body().size.0, typo::read().size.0, typo::headline().size.0), (14.0, 16.0, 28.0));
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

    /// Issue #291 (task L.34): every measure token is the standard file's value, unchanged.
    #[test]
    fn the_standard_design_gives_the_spacing_radii_and_sizes_of_its_file() {
        let d = Design::standard();
        let (sp, r, l, z) = (d.space, d.radius, d.layout, d.size);
        assert_eq!(
            [space::xs(), space::s(), space::m(), space::l(), space::xl(), space::xxl()],
            [sp.xs, sp.s, sp.m, sp.l, sp.xl, sp.xxl]
        );
        assert_eq!([radius::xs(), radius::sm(), radius::md(), radius::lg()], [r.xs, r.sm, r.md, r.lg]);
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
            size::content_max(),
            size::form_max(),
            size::rail(),
            size::rail_wide(),
            size::reading_max(),
            shell::rail(),
            layout::inset(),
        ];
        let file = [
            z.control_sm,
            z.control,
            z.control_lg,
            z.row,
            z.row_two,
            z.row_sidebar,
            z.row_group,
            z.header,
            z.sidebar,
            z.props,
            z.panel,
            z.list,
            z.list_medium,
            z.hour,
            z.axis,
            z.axis_narrow,
            z.content_max,
            z.form_max,
            z.rail,
            z.rail_wide,
            z.reading_max,
            z.rail,
            l.inset,
        ];
        assert_eq!(now, file);
        assert_eq!(size::HAIRLINE, 1.0, "a line stays one logical pixel");
        let sides = l.control_padding.horizontal;
        assert_eq!(style::button_padding(), iced::Padding::new(0.0).left(sides).right(sides));
        assert_eq!(style::pick_list_padding(), iced::Padding::new(6.0).left(sides).right(sides));
    }

    #[test]
    fn the_standard_design_loads_the_old_fonts() {
        let settings = crate::settings();
        let embedded: Vec<&[u8]> = crate::fonts().to_vec();
        let loaded: Vec<&[u8]> = settings.fonts.iter().map(|f| f.as_ref()).collect();
        assert_eq!(loaded, embedded, "the five Inter files, in the old order");
        assert_eq!(settings.default_font, legacy::FONT);
        assert_eq!(settings.default_text_size, iced::Pixels(14.0), "the new standard's body (issue #291)");
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

    /// Issue #291: a colour scheme that is `dark_only` draws dark in light mode too – the light
    /// palette, its derived text colours and the light theme are the dark ones, so a window in light
    /// mode looks the same as in dark mode.
    #[test]
    fn a_dark_only_scheme_draws_dark_in_light_mode() {
        let mut scheme = ColorScheme::standard();
        scheme.colors.light = scheme.colors.dark;
        scheme.dark_only = true;
        let tokens = tokens_of(scheme);
        assert_eq!(tokens.light, tokens.dark, "derived attention and error text included");
        assert_eq!(tokens.light.attention_text, legacy::DARK.attention_text);
        assert_eq!(tokens.sidebar_light, tokens.sidebar_dark);
        assert!(tokens.light_theme.extended_palette().is_dark, "Iced's own widgets draw dark too");
        assert_eq!(tokens.light_theme.palette(), tokens.dark_theme.palette());
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

    /// The ids `id` for all three parts, as the tests of task L.21 sent one id.
    fn all(id: &str) -> Chosen {
        Chosen::of_ids(id, id, id)
    }

    /// Task L.21: each part from its own folder; only a switch of the font loads font files, and a
    /// part that is gone or broken is the standard's.
    #[test]
    fn follow_takes_each_part_from_its_folder_and_loads_fonts_only_for_a_new_font() {
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

        let switched = follow.update(Message::Stored(Ok(all("twin")))).expect("the design and the scheme changed");
        assert_eq!((active().id.as_str(), crate::scheme::active().id.as_str()), ("twin", "twin"));
        assert!(crate::font::active().is_standard(), "there is no font `twin`: the standard's");
        assert_eq!(switched.units(), 0, "design and colour scheme load nothing");

        let switched = follow.update(Message::Stored(Ok(all(&fresh)))).expect("the font changed");
        assert_eq!((active().id.as_str(), crate::font::active().id.as_str()), (fresh.as_str(), fresh.as_str()));
        assert!(crate::scheme::active().is_standard(), "there is no colour scheme of that id");
        assert!(switched.units() > 0, "a new font loads its files");
        assert!(follow.update(Message::Stored(Ok(all(&fresh)))).is_none(), "the same setting again");
        assert!(follow.update(Message::Stored(Err("core is not running".into()))).is_none());
        assert_eq!(crate::font::active().id, fresh, "a core that does not answer changes nothing");

        assert!(follow.update(Message::Stored(Ok(all("pale")))).is_some(), "back to the standard");
        assert!(active().is_standard() && crate::font::active().is_standard(), "a broken design is never drawn");
        assert!(follow.update(Message::Stored(Ok(all("gone")))).is_none(), "already the standard");
        assert!(follow.update(Message::Stored(Ok(Chosen::standard()))).is_none(), "already the standard");
        let _ = std::fs::remove_dir_all(root);
    }

    /// Task L.22: a window follows the three settings apart – a message that changes one id reads and
    /// replaces only that part, the others stay the very same; only a new font loads files; choosing a
    /// design (`apply`) keeps font and colour scheme.
    #[test]
    fn a_message_sets_only_its_part() {
        let _switching = SWITCHING.lock().unwrap_or_else(PoisonError::into_inner);
        let (root, folders) = folders();
        let fresh = format!("only-{}", std::process::id());
        write(folders.fonts.as_deref().expect("fonts"), &fresh, "font.toml", "");
        mark_fonts_loaded(STANDARD_ID);
        let mut follow = Follow::new(None).in_folders(folders);
        let chosen = |design: &str, font: &str, scheme: &str| Chosen::of_ids(design, font, scheme);

        let switched = follow.update(Message::Stored(Ok(chosen("torro", "torro", "harbor")))).expect("a scheme");
        assert_eq!(switched.units(), 0, "a colour scheme loads nothing");
        assert_eq!(crate::scheme::active().id, "harbor");
        assert!(active().is_standard() && crate::font::active().is_standard(), "the others stay");
        let (design, scheme) = (active(), crate::scheme::active());

        let switched = follow.update(Message::Stored(Ok(chosen("torro", &fresh, "harbor")))).expect("a font");
        assert!(switched.units() > 0, "a new font loads its files");
        assert_eq!(crate::font::active().id, fresh);
        assert!(Arc::ptr_eq(&design, &active()) && Arc::ptr_eq(&scheme, &crate::scheme::active()), "untouched");

        assert!(follow.apply("twin").is_some(), "the design alone");
        assert_eq!(Chosen::active(), chosen("twin", &fresh, "harbor"), "font and colour scheme stay");
        assert!(follow.apply("twin").is_none(), "the same design again");
        assert!(follow.update(Message::Stored(Ok(chosen("twin", &fresh, "nord")))).is_some());
        assert!(crate::scheme::active().is_standard(), "no colour scheme `nord` there: the standard's");
        assert_eq!(Chosen::active(), chosen("twin", &fresh, "torro"));

        assert!(follow.follow(Chosen::standard()).is_some());
        assert_eq!(Chosen::active(), Chosen::standard());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn forced_parts_win_over_the_stored_ones() {
        let stored = Chosen::of_ids("plex", "channelr", "nord");
        assert_eq!(Forced::default().over(stored.clone()), stored);
        assert!(Forced::default().is_empty());
        let forced = Forced { font: Some("torro".into()), ..Forced::default() };
        assert!(!forced.is_empty());
        assert_eq!(forced.over(stored), Chosen::of_ids("plex", "torro", "nord"));
        let settings = Settings { font: "channelr".into(), ..Settings::default() };
        assert_eq!(Chosen::of(&settings), Chosen::of_ids("torro", "channelr", "torro"));
        let older = Settings { font: String::new(), color_scheme: String::new(), ..Settings::default() };
        assert_eq!(Chosen::of(&older), Chosen::standard(), "a core that sends neither: the standards");
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
        assert_eq!(crate::typo::body().size.0, 14.0, "Inter by another id: the same size");
        replace(Some(Design::standard()), Some(FontSet::standard()), Some(ColorScheme::standard()));
        assert!(crate::scheme::active().is_standard() && crate::font::active().is_standard());
    }

    /// Task L.21: every example of the repository, as all three parts of its id, draws its levels as
    /// `Selection::level` gives them, in the families of its font and the colours of its colour
    /// scheme. Since issue #291 the examples are fonts and colour schemes only: the designs under
    /// `designs/` are the built-in Standard, Compact and Console, so an example like Plex draws in the
    /// standard's form.
    #[test]
    fn the_examples_draw_as_their_selection() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let folders = Folders {
            designs: Some(repo.join("designs")),
            fonts: Some(repo.join("fonts")),
            color_schemes: Some(repo.join("color-schemes")),
        };
        let mut ids = folders.choices(Part::Design);
        ids.extend(folders.choices(Part::Font));
        ids.extend(folders.choices(Part::ColorScheme));
        ids.sort();
        ids.dedup();
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
        let body = plex.level(LevelName::Body);
        assert_eq!((body.size, body.line), (15.0, 22.0), "the standard's 14/22 in IBM Plex Sans (× 1.058)");
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
            let mut stream = std::pin::pin!(watch(&(nowhere, Chosen::standard())));
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

    /// `--design`, `--font` and `--color-scheme` each take the valid ids of their own folder (task
    /// L.22): `--design harbor` is refused, `harbor` is only a colour scheme.
    #[test]
    fn only_valid_parts_of_the_flags_folder_are_choices() {
        let (root, folders) = folders();
        assert_eq!(
            choices_in(&folders, Part::Design),
            ["torro", "compact", "console", "twin"],
            "the built-in designs first, never broken"
        );
        assert_eq!(choices_in(&folders, Part::ColorScheme), ["torro", "harbor", "twin"]);
        assert_eq!(choices_in(&folders, Part::Font), ["torro"]);
        assert_eq!(
            choices_in(&Folders::default(), Part::Design),
            ["torro", "compact", "console"],
            "the built-in designs need no folder (`--design console` with a fresh data folder)"
        );
        assert!(is_choice_in(&Folders::default(), Part::Design, "console"));
        assert!(is_choice_in(&folders, Part::Design, "twin") && is_choice_in(&folders, Part::ColorScheme, "harbor"));
        assert!(!is_choice_in(&folders, Part::Design, "harbor"), "a colour scheme is no design");
        assert!(is_choice_in(&Folders::default(), Part::Font, "torro"), "the standard needs no folder");
        for id in ["pale", "gone", "../twin"] {
            assert!(!is_choice_in(&folders, Part::Design, id), "{id}");
        }
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn without_a_core_the_start_keeps_the_standard() {
        let _switching = SWITCHING.lock().unwrap_or_else(PoisonError::into_inner);
        let nowhere = std::env::temp_dir().join(format!("torro-gui-no-core-{}", std::process::id()));
        start(Some(&nowhere), &Forced::default());
        start(None, &Forced::default());
        assert!(active().is_standard());
        start(None, &Forced { design: Some("torro".into()), font: Some("torro".into()), color_scheme: None });
        assert_eq!(Chosen::active(), Chosen::standard());
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
