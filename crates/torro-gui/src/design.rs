//! The design this process draws with (task L.2, concept `docs/design-pakete.md`).
//!
//! Every token of `torro_gui` – the colours of [`crate::theme::palette`], the levels of
//! [`crate::typo`], [`crate::theme::space`], [`crate::theme::radius`], [`crate::theme::size`], the
//! layout gaps of [`crate::layout`] and the fonts – is read from the **active design**, a
//! [`Design`] from `torro_core::design`. There is one per process: [`active`] reads it, [`set`]
//! replaces it. [`Design::standard`] gives exactly the values the suite had as constants (the test
//! "same picture" below holds it to that).
//!
//! Views never cache a token across frames: a switch takes effect on the next `view`/`draw`.
//!
//! # Following the setting (task L.3)
//!
//! The suite settings name one design (`Settings::design`); every window draws with it and follows
//! a change at once, on the way the appearance and the zoom take. A pack that is gone or breaks a
//! rule gives the standard, never a crash. An app wires it up in four places:
//!
//! ```ignore
//! use torro_gui::design::{self, Follow};
//!
//! // main, before `torro_gui::settings()`: the first frame already has the right design and fonts.
//! design::start_for(&source, launch.design.as_deref()); // `--design <id>` of `--demo` wins
//! // new: no socket with `--demo`, so the design stays as it started.
//! let design = Follow::for_source(&source);
//! let task = Task::batch([design.fetch().map(Message::Design), …]);
//! // subscription: always, also in glance views. Asks the core every 2 s and reports only a change,
//! // so a choice in the control centre arrives at once and an idle window is not woken up.
//! all.push(self.design.subscription().map(Message::Design));
//! // update: a switch loads the new fonts; canvas caches (`canvas::Cache`) would be cleared here.
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
//! not name its font would keep the old design's text font after a switch, so every text of the
//! suite names its font from the active design: `typo::text`, and `.font(typo::body().font)` (or the
//! level it uses) on text inputs, text editors, pick lists (their menus take the same font) and
//! rich text. Only Iced's own fallback for missing glyphs stays the system's.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, PoisonError, RwLock};
use std::time::Duration;

use iced::futures::Stream;
use iced::{Font, Subscription, Task, Theme};
use torro_core::design::{Face, FontRole, Mode, STANDARD_ID, Weight};
use torro_ipc::{Request, Response};

pub use torro_core::design::Design;

use crate::source::Source;
use crate::theme::{self, Palette};

#[cfg(test)]
mod legacy;

/// The active design and what `torro_gui` derives from it once per [`set`], so the token functions
/// only copy values out.
pub(crate) struct Tokens {
    /// The design itself.
    pub design: Arc<Design>,
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
    fn new(design: Design) -> Tokens {
        let dark = Palette::of(&design, Mode::Dark);
        let light = Palette::of(&design, Mode::Light);
        // The standard keeps the names the apps' tests know ("Torro Dark"); a pack is named after itself.
        let name = |mode: &str| -> Cow<'static, str> {
            if design.is_standard() {
                Cow::Owned(format!("Torro {mode}"))
            } else {
                Cow::Owned(format!("{} {mode}", design.about.name))
            }
        };
        let dark_theme = theme::iced_theme(name("Dark"), &dark);
        let light_theme = theme::iced_theme(name("Light"), &light);
        let sidebar_dark = Palette::of_sidebar(&design, Mode::Dark);
        let sidebar_light = Palette::of_sidebar(&design, Mode::Light);
        let sidebar_dark_theme = theme::iced_theme(name("Dark sidebar"), &sidebar_dark);
        let sidebar_light_theme = theme::iced_theme(name("Light sidebar"), &sidebar_light);
        Tokens {
            design: Arc::new(design),
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
}

static ACTIVE: LazyLock<RwLock<Arc<Tokens>>> = LazyLock::new(|| RwLock::new(Arc::new(Tokens::new(Design::standard()))));

/// Runs `f` on the active tokens. `f` must not call another token function: it runs under the
/// read lock, and a nested read could wait behind a writer.
pub(crate) fn with<R>(f: impl FnOnce(&Tokens) -> R) -> R {
    let tokens = ACTIVE.read().unwrap_or_else(PoisonError::into_inner);
    f(&tokens)
}

/// The design this process draws with. Cheap (an `Arc`); read it again after a switch instead of
/// keeping it.
pub fn active() -> Arc<Design> {
    with(|t| Arc::clone(&t.design))
}

/// Makes `design` the active one for the whole process: every token function returns its values
/// from the next call on. Takes a design that `torro_core::design::load` accepted (merged and
/// checked); nothing is checked here.
///
/// Fonts are not loaded here: at start [`crate::settings`] loads the active design's fonts; on a
/// switch [`Follow::apply`] loads them with `iced::font::load`. Apps rarely call this themselves:
/// [`start`] and [`Follow`] do. Canvas caches keep what they drew; apps clear them after a switch.
pub fn set(design: Design) {
    let tokens = Arc::new(Tokens::new(design));
    *ACTIVE.write().unwrap_or_else(PoisonError::into_inner) = tokens;
}

/// How long [`start`] waits for the core's answer before the window opens with the standard. A
/// core that is just starting answers with "not running" at once; this only bounds a core that
/// hangs. The first [`Follow::fetch`] brings the stored design later.
const START_WAIT: Duration = Duration::from_secs(1);

/// [`start`] for an app on `source`: the core's socket, none for demo data.
pub fn start_for(source: &Source, forced: Option<&str>) {
    let socket = match source {
        Source::Core(client) => Some(client.socket.as_path()),
        Source::Demo(_) => None,
    };
    start(socket, forced);
}

/// Sets the design a process starts with. Call in `main` before [`crate::settings`], which loads
/// the fonts of the active design, so the first frame is already drawn right.
///
/// `forced` is `--design <id>` (only with `--demo`, checked by [`is_choice`] when the parameters
/// are read) and wins. Otherwise the design the suite settings name, asked from the core on
/// `socket` (waiting at most a second); without a socket (`--demo`), without an answer, or when
/// the pack is gone or broken, the standard stays. Never fails: a window always opens.
pub fn start(socket: Option<&Path>, forced: Option<&str>) {
    let id = match (forced, socket) {
        (Some(id), _) => id.to_string(),
        (None, Some(socket)) => match stored_now(socket) {
            Some(id) => id,
            None => return,
        },
        (None, None) => return,
    };
    let design = load_or_standard(torro_core::design::dir().as_deref(), &id);
    if !design.is_standard() {
        set(design);
    }
}

/// The design the settings name, asked from the core on `socket` and waited for at most
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
        Ok(Ok(Response::Settings { settings })) => Some(settings.design),
        _ => None,
    }
}

/// The design `id` from the designs folder `dir`, or the standard when it is gone or breaks a rule
/// (said once on stderr, with every reason: the control centre shows them too).
fn load_or_standard(dir: Option<&Path>, id: &str) -> Design {
    // The standard needs no folder; `load` answers it without touching the disk.
    let loaded = match dir {
        Some(dir) => torro_core::design::load(dir, id),
        None => torro_core::design::load(Path::new(""), id),
    };
    loaded.unwrap_or_else(|problems| {
        let reasons: Vec<String> = problems.iter().map(ToString::to_string).collect();
        eprintln!("the design {id:?} can't be used, drawing with the standard: {}", reasons.join(" "));
        Design::standard()
    })
}

/// Whether `--design <id>` names a design this process can draw with: the standard, or a pack in
/// the designs folder that passes every rule. [`choices`] lists the valid ones for the error.
pub fn is_choice(id: &str) -> bool {
    is_choice_in(torro_core::design::dir().as_deref(), id)
}

/// [`is_choice`] for the designs folder `dir`.
pub fn is_choice_in(dir: Option<&Path>, id: &str) -> bool {
    id == STANDARD_ID || dir.is_some_and(|dir| torro_core::design::load(dir, id).is_ok())
}

/// The ids `--design` takes, the standard first: what the error for an unknown one lists.
pub fn choices() -> Vec<String> {
    choices_in(torro_core::design::dir().as_deref())
}

/// [`choices`] for the designs folder `dir`.
pub fn choices_in(dir: Option<&Path>) -> Vec<String> {
    match dir {
        Some(dir) => {
            torro_core::design::list(dir).into_iter().filter(|pack| pack.valid()).map(|pack| pack.id).collect()
        }
        None => vec![STANDARD_ID.to_string()],
    }
}

/// What reaches a window about the design. The app wraps it in one variant of its own message
/// (`Message::Design(design::Message)`) and hands it to [`Follow::update`].
#[derive(Debug, Clone)]
pub enum Message {
    /// The design the suite settings name, as [`Follow::fetch`] asked; `Err` when the core did not
    /// answer, and the window keeps the design it has.
    Stored(Result<String, String>),
    /// A font of the new design is loaded. Nothing to do: the message alone makes Iced build the
    /// view again, so text laid out before the font was there is laid out anew.
    FontLoaded,
}

/// How one window follows the design the suite settings name, like [`crate::zoom::Zoom`] follows
/// the zoom. The design itself is the process's ([`active`]); this keeps where to ask and what was
/// asked for last.
#[derive(Debug, Clone)]
pub struct Follow {
    /// The core's socket; `None` with `--demo`, where the design stays as it started.
    socket: Option<PathBuf>,
    /// The designs folder (`torro_core::design::dir`); `None` without a home directory, then only
    /// the standard can be drawn.
    dir: Option<PathBuf>,
    /// The id asked for last, so an unchanged setting costs nothing and a broken pack is read
    /// once, not on every tick.
    chosen: String,
}

impl Follow {
    /// Follows the setting through the core on `socket` (`torro_ipc::socket_path()`); `None` keeps
    /// the design the process started with (`--demo`). Starts from the active design, so the
    /// first answer changes nothing when [`start`] already set it.
    pub fn new(socket: Option<PathBuf>) -> Follow {
        Follow { socket, dir: torro_core::design::dir(), chosen: active().id.clone() }
    }

    /// [`Follow::new`] for an app on `source`: the core's socket, or none for demo data.
    pub fn for_source(source: &Source) -> Follow {
        match source {
            Source::Core(client) => Follow::new(Some(client.socket.clone())),
            Source::Demo(_) => Follow::new(None),
        }
    }

    /// The same, with the packs read from `dir` instead of the user's designs folder: for tests,
    /// which never touch the user's data.
    pub fn in_dir(self, dir: PathBuf) -> Follow {
        Follow { dir: Some(dir), ..self }
    }

    /// Asks the core for the design the settings name (`Request::Settings`). Call at start, so the
    /// window has it before the first poll of [`Follow::subscription`]. Without a socket (`--demo`)
    /// it does nothing.
    pub fn fetch(&self) -> Task<Message> {
        let Some(socket) = self.socket.clone() else { return Task::none() };
        Task::perform(async move { stored(&socket).await }, Message::Stored)
    }

    /// Asks the core every [`POLL`] for the design the settings name and reports it as
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

    /// Handles `message`. `Some` when the active design changed: the window draws with it from the
    /// next frame; the app clears its canvas caches, if it keeps any, and returns the task, which
    /// loads the new fonts. `None` when nothing changed.
    pub fn update(&mut self, message: Message) -> Option<Task<Message>> {
        match message {
            Message::Stored(Ok(id)) => self.apply(&id),
            // The core is not answering: keep what we have.
            Message::Stored(Err(_)) | Message::FontLoaded => None,
        }
    }

    /// Switches the process to the design `id` (the setting as the window learnt it, from
    /// [`Follow::fetch`] or with other settings, as in the control centre). `id` asked for last
    /// time does nothing. A pack that is gone or breaks a rule gives the standard. Returns what
    /// [`Follow::update`] returns.
    pub fn apply(&mut self, id: &str) -> Option<Task<Message>> {
        if id == self.chosen {
            return None;
        }
        self.chosen = id.to_string();
        let design = load_or_standard(self.dir.as_deref(), id);
        if design == *active() {
            return None;
        }
        let fonts = load_fonts(&design);
        set(design);
        Some(fonts)
    }
}

/// How often [`Follow::subscription`] asks the core for the chosen design: a choice in the control
/// centre reaches every open window within this time.
pub const POLL: Duration = Duration::from_secs(2);

/// The design the settings name, asked from the core on `socket`.
async fn stored(socket: &Path) -> Result<String, String> {
    match torro_ipc::client::call(socket, &Request::Settings).await {
        Ok(Response::Settings { settings }) => Ok(settings.design),
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

/// Loads the font files of `design` into Iced, the first time only: Iced keeps every font it
/// loaded, so switching back and forth, or showing a pack's sample in the control centre, loads
/// each design's fonts once. The task ends in [`Message::FontLoaded`] per file, which makes Iced lay
/// out text again; nothing when they are loaded already.
pub fn load_fonts(design: &Design) -> Task<Message> {
    if !mark_fonts_loaded(&design.id) {
        return Task::none();
    }
    Task::batch(font_files(design).into_iter().map(|font| iced::font::load(font).map(|_| Message::FontLoaded)))
}

/// Notes that the fonts of the design `id` are loaded; `false` when they already were.
pub(crate) fn mark_fonts_loaded(id: &str) -> bool {
    static LOADED: Mutex<Vec<String>> = Mutex::new(Vec::new());
    let mut loaded = LOADED.lock().unwrap_or_else(PoisonError::into_inner);
    if loaded.iter().any(|known| known == id) {
        return false;
    }
    loaded.push(id.to_string());
    true
}

/// The font files of `design` in the form `iced::Settings::fonts` and `iced::font::load` take:
/// each family once, every weight it has. Built-in faces are the Inter files embedded in
/// `torro_gui` ([`crate::fonts`]); a pack's files are read from its folder (a file that cannot be
/// read is left out, Iced then falls back to another font – the pack was checked when it was
/// loaded, so this only happens when the folder changed since).
pub fn font_files(design: &Design) -> Vec<Cow<'static, [u8]>> {
    let mut seen: Vec<&str> = Vec::new();
    let mut files = Vec::new();
    for role in [FontRole::Text, FontRole::Heading, FontRole::Numbers] {
        let font = design.fonts.get(role);
        if seen.contains(&font.family.as_str()) {
            continue;
        }
        seen.push(&font.family);
        for weight in WEIGHTS {
            match font.face(weight) {
                Some(Face::Builtin) => files.extend(builtin(&font.family, weight).map(Cow::Borrowed)),
                Some(Face::File(path)) => files.extend(std::fs::read(path).ok().map(Cow::Owned)),
                None => {}
            }
        }
    }
    files
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

/// The Iced font of `role` at `weight` in the active design.
pub fn font(role: FontRole, weight: Weight) -> Font {
    let family = with(|t| family(&t.design.fonts.get(role).family));
    Font { weight: iced_weight(weight), ..Font::with_name(family) }
}

/// The Iced font of `role` at `weight` in `design`, which need not be the active one: for a
/// sample of a pack ("Aa" in the control centre). Draws in that font once [`load_fonts`] loaded it;
/// until then Iced falls back to another.
pub fn font_of(design: &Design, role: FontRole, weight: Weight) -> Font {
    Font { weight: iced_weight(weight), ..Font::with_name(family(&design.fonts.get(role).family)) }
}

/// The figures font (`numbers`) of the active design at `weight`, or at the nearest lighter weight
/// it has: the standard ships "Inter Tabular" only in Regular and Medium, so a SemiBold level
/// becomes tabular Medium. Regular when it has nothing lighter.
pub fn tabular_font(weight: iced::font::Weight) -> Font {
    let (name, weight) = with(|t| {
        let numbers = &t.design.fonts.numbers;
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

    #[test]
    fn tokens_follow_the_design_they_are_built_from() {
        let mut design = Design::standard();
        design.colors.dark.accent = torro_core::design::Rgba::hex(0x0EA5A4);
        design.colors.light.text = torro_core::design::Rgba::hex(0x101010);
        let tokens = Tokens::new(design);
        assert_eq!(tokens.dark.accent.into_rgba8(), [0x0E, 0xA5, 0xA4, 0xFF]);
        assert_eq!(tokens.dark_theme.palette().primary, tokens.dark.accent, "the Iced palette follows");
        // Derived colours are computed from the design, not taken from the old constants.
        assert_ne!(tokens.light.attention_text, legacy::LIGHT.attention_text);
        assert_eq!(tokens.dark_theme.to_string(), "Torro Dark");
    }

    #[test]
    fn a_sidebar_of_its_own_gets_its_own_theme() {
        use torro_core::design::Rgba;
        let mut design = Design::standard();
        let light = &mut design.colors.light;
        (light.sidebar, light.sidebar_hover, light.sidebar_selected) =
            (Rgba::hex(0x24292F), Rgba::hex(0x2F353C), Rgba::hex(0x3A4149));
        (light.sidebar_text, light.sidebar_text_2, light.sidebar_text_3) =
            (Rgba::hex(0xF6F8FA), Rgba::hex(0xD0D7DE), Rgba::hex(0xAEB7C1));
        let tokens = Tokens::new(design);
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

    /// Held by the tests that switch the process's design, so they do not see each other's.
    static SWITCHING: Mutex<()> = Mutex::new(());

    /// A designs folder in a temp dir with the packs `twin` (the standard's values under its own
    /// id and the standard's name, so tests running beside see the same tokens) and `pale`
    /// (dark text on dark surfaces: refused).
    fn designs() -> std::path::PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("torro-gui-designs-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (id, toml) in [
            ("twin", "format = 1\n[about]\nname = \"Torro\"\nversion = \"1.0.0\"\n"),
            ("pale", "format = 1\n[about]\nname = \"Pale\"\nversion = \"1.0.0\"\n[colors.dark]\ntext = \"#202020\"\n"),
        ] {
            std::fs::create_dir_all(dir.join(id)).expect("pack folder");
            std::fs::write(dir.join(id).join("design.toml"), toml).expect("design.toml");
        }
        dir
    }

    #[test]
    fn follow_switches_to_the_chosen_design_and_falls_back_to_the_standard() {
        let _switching = SWITCHING.lock().unwrap_or_else(PoisonError::into_inner);
        let dir = designs();
        // An id of its own: `crate::settings()` in a test running beside marks the fonts of
        // whatever design is active as loaded, `twin` included.
        let fresh = format!("fresh-{}", std::process::id());
        std::fs::create_dir_all(dir.join(&fresh)).expect("pack folder");
        std::fs::copy(dir.join("twin/design.toml"), dir.join(&fresh).join("design.toml")).expect("design.toml");
        let mut follow = Follow::new(None).in_dir(dir.clone());
        assert!(follow.fetch().units() == 0, "without a socket nothing is asked");

        let switched = follow.update(Message::Stored(Ok(fresh.clone()))).expect("the design changed");
        assert_eq!(active().id, fresh);
        assert!(switched.units() > 0, "the first switch loads the design's fonts");
        assert!(follow.update(Message::Stored(Ok(fresh.clone()))).is_none(), "the same setting again");
        assert!(follow.update(Message::Stored(Err("core is not running".into()))).is_none());
        assert_eq!(active().id, fresh, "a core that does not answer changes nothing");

        assert!(follow.update(Message::Stored(Ok("pale".into()))).is_some(), "back to the standard");
        assert!(active().is_standard(), "a broken pack is never drawn");
        assert!(follow.update(Message::Stored(Ok("gone".into()))).is_none(), "already the standard");
        assert!(active().is_standard(), "a missing pack neither");

        assert!(follow.apply("twin").is_some());
        assert!(follow.apply("torro").is_some());
        assert!(active().is_standard());
        let _ = std::fs::remove_dir_all(dir);
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
    fn fonts_of_a_design_are_loaded_once() {
        let mut once = Design::standard();
        once.id = format!("fonts-once-{}", std::process::id());
        assert!(load_fonts(&once).units() > 0, "the first time: the files");
        assert_eq!(load_fonts(&once).units(), 0, "afterwards nothing");
    }

    #[test]
    fn a_sample_font_names_the_pack_family() {
        let mut pack = Design::standard();
        pack.fonts.text.family = "Sample Sans".to_string();
        let font = font_of(&pack, FontRole::Text, Weight::Medium);
        assert_eq!(font.family, iced::font::Family::Name("Sample Sans"));
        assert_eq!(font.weight, iced::font::Weight::Medium);
    }

    #[test]
    fn only_valid_designs_are_choices() {
        let dir = designs();
        assert_eq!(choices_in(Some(&dir)), ["torro", "twin"], "the standard first, never a broken pack");
        assert_eq!(choices_in(None), ["torro"]);
        assert!(is_choice_in(Some(&dir), "twin"));
        assert!(is_choice_in(None, "torro"), "the standard needs no folder");
        for id in ["pale", "gone", "../twin"] {
            assert!(!is_choice_in(Some(&dir), id), "{id}");
        }
        let _ = std::fs::remove_dir_all(dir);
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
