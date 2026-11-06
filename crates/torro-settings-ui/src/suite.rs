//! The pages of the whole suite's settings (issue #408, `docs/einzelne-apps.md`): **Appearance** and
//! **General** (Language, Startup, Background service), in one place for the control centre and the
//! apps.
//!
//! - The control centre shows Appearance as its page, and Language and Startup in its page General,
//!   beside what only it has (Apps, Maps, Setup); "Background service" not, its tray has "Quit".
//! - An app shows both pages as sections of its own settings while the main application is not
//!   installed (decision 2 in `docs/einzelne-apps.md`): the [`crate::Host`] adds them and carries
//!   out what they ask. With the main application the app keeps the links "Appearance ↗" and
//!   "All settings ↗" instead, so no setting stands in two places.
//!
//! The pages never talk to the daemon themselves: [`update`] returns an [`Action`], which the
//! control centre maps onto its own and the host onto its own. Every change is one
//! `Request::SetSetting` with the key that changed; "Quit" sends `Request::Quit` after a question.
//!
//! # Appearance (tasks S.14, L.4, L.18, L.23 and L.37, issue #484, concept `docs/design-pakete.md`)
//!
//! Four groups, each about one thing (issue #484, draft `docs/prototypes/abstaende/`):
//!
//! - **Spacing** – the three spacing levels Comfort, Standard and Dense as a segment, under it a small
//!   mail window drawn from the chosen level's values in the window's colours ([`MailPreview`]). A
//!   level sets only spacing and the heights of rows and controls; text, font, colours and the mail
//!   view stay.
//! - **Font** – a dropdown with a search field (issue #299, `widgets::search_menu`), each entry named
//!   by its family and written in it, the one drawn with a check mark. "pack · version · author ·
//!   licence" as tooltip. The section "Torro fonts" (`FontInfo::origin` bundled or folder) holds
//!   the groups Sans, Serif, Mono and Other (`FontInfo::kind`), a line parts it from "System fonts",
//!   the computer's: "System" first, then the families ([`font_sections`]). Typing filters both,
//!   Enter takes the first usable match.
//! - **Colors** – the colour scheme as a dropdown: four stripes (`bg`, `panel`, `text`, `accent` in
//!   the window's light or dark mode) and the name, in the sections "Torro", "Light and dark" and
//!   "Dark only" (`ColorSchemeInfo::dark_only`); an empty section is left out ([`scheme_groups`]).
//!   Under it "Color mode" as a segment of System, Light, Dark with symbol and word.
//! - **Display** – the zoom of the suite with −, the value, + and "Reset" (task Z.1).
//!
//! Wide, Spacing stands on the left and the other three on the right; narrow, all four stand one
//! under the other in that order.
//!
//! The three lists come from the core (`settings.list_designs`, `settings.list_fonts`,
//! `settings.list_color_schemes`, [`list`]); the parts for the previews are read here, only for the
//! ids the core listed as usable, the way every window reads the part it draws with
//! (`torro_core::design::load`, `load_font`, `load_color_scheme`): [`Entry`], [`Lists::read`].
//!
//! Choosing is [`Action::Choose`]: the control centre draws its own window with it at once and saves
//! it; an app saves it and draws with it as soon as `torro_gui::design::Follow` sees it. Choosing a
//! level sets nothing else.
//!
//! A font or colour scheme that breaks a rule is greyed out with an alert mark. A click on it shows,
//! under its group, its name, "Can't be used:" and every reason in full, wrapping – a list of reasons
//! is too long for a tooltip and must be readable without a mouse over it. A second click, or a click
//! on another one, closes or swaps that ([`Message::Problems`]); one entry's reasons are shown at a
//! time on the whole page.
//!
//! "Reload" (at the heading "Font": it lists all three again) and "Open … folder" (created if it is
//! missing, its path as tooltip) are quiet actions at the right of the headings. Opening a folder is a
//! local action without a request: it only shows the file manager on this desktop (`docs/api.md`,
//! footnote 9). Designs have no folder since issue #484.

use std::path::PathBuf;
use std::sync::Arc;

use iced::widget::text::{LineHeight, Wrapping};
use iced::widget::{button, canvas, column, container, responsive, row, rule, scrollable, space as blank, svg, text};
use iced::{
    Alignment, Background, Border, Color, Element, Fill, Length, Padding, Pixels, Point, Rectangle, Renderer, Size,
    Theme, mouse,
};
use torro_core::design::{
    ColorScheme, Design, FontKind, FontRole, FontSet, Level, ListSelection, ListStyle, Mode, Part, STANDARD_ID,
    SYSTEM_ID, Weight,
};
use torro_core::i18n::tr;
use torro_core::settings::{Language, Settings, Zoom};
use torro_core::theme::Appearance;
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{Palette, palette, radius, size, space};
use torro_gui::widgets::{Align, Anchor, Placement};
use torro_gui::zoom::{self, Step};
use torro_gui::{layout, style, typo, widgets};
use torro_ipc::{ColorSchemeInfo, DesignInfo, FontInfo, FontOrigin, Request, Response, SettingKey};

use crate::Backend;
use crate::parts::{choice, confirm, row_frame, setting_row, switch};

/// The section words of the two pages, as `--settings` and `--section` take them.
pub const APPEARANCE: &str = "appearance";
/// See [`APPEARANCE`].
pub const GENERAL: &str = "general";

/// The pages' own state: the lists, the open dropdown, the reasons shown, the question open.
#[derive(Debug, Default)]
pub struct State {
    /// The spacing levels as the core listed them last, the standard first; `Err` when it did not
    /// answer, `None` until the first answer. The fonts and colour schemes alike.
    designs: Option<Result<Vec<DesignEntry>, String>>,
    fonts: Option<Result<Vec<FontEntry>, String>>,
    schemes: Option<Result<Vec<SchemeEntry>, String>>,
    /// The entry that can't be used whose reasons are shown under its group (its part and id), after
    /// a click on it; `None` while no reasons are shown. The part, because one id can name a font
    /// and a colour scheme at once (`plex`).
    problems: Option<(Part, String)>,
    /// Whether the colour scheme's dropdown is open ([`overlay`]). At most one of the two dropdowns is
    /// open.
    schemes_open: bool,
    /// Where the dropdown's button is, so its menu hangs under it.
    anchor: Anchor,
    /// Whether the font's dropdown is open ([`overlay`], issue #299).
    fonts_open: bool,
    /// The words in the font dropdown's search field; emptied each time it opens.
    font_query: String,
    /// Where the font dropdown's button is.
    font_anchor: Anchor,
    /// The question "Quit background service?" is open ([`overlay`]).
    quitting: bool,
}

/// One entry of a list of the core – a design, a font or a colour scheme (`info`) – with the part
/// itself as read here for its sample.
#[derive(Debug, Clone)]
pub struct Entry<I, T> {
    pub info: I,
    /// The part as read: what the sample shows and what the demo draws with when it is chosen.
    /// `None` for one that can't be used (or whose folder changed since the core listed it).
    pub part: Option<Arc<T>>,
}

/// A spacing level of the list (`settings.list_designs`).
pub type DesignEntry = Entry<DesignInfo, Design>;
/// A font of the list (`settings.list_fonts`).
pub type FontEntry = Entry<FontInfo, FontSet>;
/// A colour scheme of the list (`settings.list_color_schemes`).
pub type SchemeEntry = Entry<ColorSchemeInfo, ColorScheme>;

/// Two entries are the same when the core listed the same: the part is read from it.
impl<I: PartialEq, T> PartialEq for Entry<I, T> {
    fn eq(&self, other: &Self) -> bool {
        self.info == other.info
    }
}

impl<I, T> Entry<I, T> {
    /// The row `info` with `part` as read (`None` when it can't be used).
    pub fn new(info: I, part: Option<T>) -> Entry<I, T> {
        Entry { info, part: part.map(Arc::new) }
    }
}

/// The three lists of one listing ("Reload" lists all three at once), each `Err` with what went wrong
/// when the core did not list it.
#[derive(Debug, Clone, PartialEq)]
pub struct Lists {
    pub designs: Result<Vec<DesignEntry>, String>,
    pub fonts: Result<Vec<FontEntry>, String>,
    pub schemes: Result<Vec<SchemeEntry>, String>,
}

impl Lists {
    /// Three lists that all failed for `problem` (no home directory, a worker that died).
    pub fn failed(problem: String) -> Lists {
        Lists { designs: Err(problem.clone()), fonts: Err(problem.clone()), schemes: Err(problem) }
    }
}

/// What the page needs of every list's rows, whichever part they list.
trait Listed {
    fn id(&self) -> &str;
    /// The name its tile shows.
    fn name(&self) -> &str;
    fn valid(&self) -> bool;
    fn problems(&self) -> &[String];
}

impl Listed for DesignInfo {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn valid(&self) -> bool {
        self.valid
    }
    fn problems(&self) -> &[String] {
        &self.problems
    }
}

impl Listed for FontInfo {
    fn id(&self) -> &str {
        &self.id
    }
    /// A font is named by its family (`IBM Plex Sans`), its pack's name goes in the tooltip; one
    /// that can't be used has no family, then the pack's name.
    fn name(&self) -> &str {
        self.family.as_deref().filter(|family| !family.is_empty()).unwrap_or(&self.name)
    }
    fn valid(&self) -> bool {
        self.valid
    }
    fn problems(&self) -> &[String] {
        &self.problems
    }
}

impl Listed for ColorSchemeInfo {
    fn id(&self) -> &str {
        &self.id
    }
    fn name(&self) -> &str {
        &self.name
    }
    fn valid(&self) -> bool {
        self.valid
    }
    fn problems(&self) -> &[String] {
        &self.problems
    }
}

/// Stores the lists of one listing ([`Lists`]).
pub fn listed(state: &mut State, lists: Lists) {
    state.designs = Some(lists.designs);
    state.fonts = Some(lists.fonts);
    state.schemes = Some(lists.schemes);
}

/// Whether nothing has been listed yet, so the shell asks the core when the page opens.
pub fn unlisted(state: &State) -> bool {
    state.designs.is_none()
}

/// The design `id` as read for the page, for a window that draws with what the page holds (`--demo`,
/// whose entries live in memory). `None` when it is not listed or can't be used.
pub fn design_of(state: &State, id: &str) -> Option<Arc<Design>> {
    part_of(&state.designs, id)
}

/// [`design_of`] for a font.
pub fn font_of(state: &State, id: &str) -> Option<Arc<FontSet>> {
    part_of(&state.fonts, id)
}

/// [`design_of`] for a colour scheme.
pub fn scheme_of(state: &State, id: &str) -> Option<Arc<ColorScheme>> {
    part_of(&state.schemes, id)
}

fn part_of<I: Listed, T>(list: &Option<Result<Vec<Entry<I, T>>, String>>, id: &str) -> Option<Arc<T>> {
    let entries = list.as_ref()?.as_ref().ok()?;
    entries.iter().find(|entry| entry.info.id() == id)?.part.clone()
}

/// Leaving the page closes the dropdowns and the question.
pub fn leave(state: &mut State) {
    state.schemes_open = false;
    state.fonts_open = false;
    state.quitting = false;
}

/// What Escape does on the page: close the question or the dropdown that is open, if one is.
pub fn escape(state: &State) -> Option<Message> {
    if state.quitting {
        return Some(Message::QuitCancelled);
    }
    if state.schemes_open { Some(Message::CloseSchemes) } else { state.fonts_open.then_some(Message::CloseFonts) }
}

/// The id of the search field in the font's dropdown, focused when it opens ([`Action::Focus`]).
pub const FONT_SEARCH: &str = "appearance-font-search";

/// A list as the page keeps it, when the core listed it.
fn ok<I, T>(list: &Option<Result<Vec<Entry<I, T>>, String>>) -> Option<&[Entry<I, T>]> {
    list.as_ref()?.as_ref().ok().map(Vec::as_slice)
}

/// What the pages' widgets report.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    Language(Language),
    Appearance(Appearance),
    StartAtLogin(bool),
    /// −, + or "Reset" of the zoom.
    Zoom(Step),
    /// A segment of the spacing levels, a font or a colour scheme of a dropdown (`part`, its id): draw
    /// every window with it. Closes the reasons of an entry that can't be used and the dropdown.
    Choose(Part, String),
    /// An entry that can't be used (`part`, its id): show its reasons under its group, or hide them
    /// if they are shown already. Closes the dropdown. Asks nothing of the window.
    Problems(Part, String),
    /// The colour scheme's button: open or close its dropdown.
    ToggleSchemes,
    /// A click next to the open dropdown, or Escape: close it.
    CloseSchemes,
    /// The font's button: open its dropdown, with an empty search field that has the focus, or
    /// close it (issue #299).
    ToggleFonts,
    /// A click next to the font's dropdown, or Escape: close it.
    CloseFonts,
    /// The words in the font dropdown's search field changed.
    FontQuery(String),
    /// Enter in the search field: choose the first match that can be used.
    FirstFont,
    /// "Reload": list designs, fonts and colour schemes again, after one was added to or changed
    /// in a folder.
    Reload,
    /// "Open fonts folder" or "Open color schemes folder".
    OpenFolder(Part),
    /// "Quit" under "Background service": ask first.
    Quit,
    /// "Quit service" in the question.
    QuitConfirmed,
    /// "Cancel", a click next to the question, or Escape.
    QuitCancelled,
}

/// What a page asks of the window that shows it.
#[derive(Debug, Clone, PartialEq)]
#[expect(
    clippy::large_enum_variant,
    reason = "a request is handled once per click; boxing would only add an allocation"
)]
pub enum Action {
    None,
    /// Send this request to the daemon, then load again: one `settings.set_setting` with the key
    /// that changed, or `system.quit`.
    Send(Request),
    /// Zoom every window of the suite one step.
    Zoom(Step),
    /// Give the keyboard focus to the text field with this id ([`FONT_SEARCH`]).
    Focus(&'static str),
    /// List the designs, fonts and colour schemes again ([`list`]) and hand them to [`listed`].
    ListLooks,
    /// Draw with the design, font or colour scheme of this id and save it ([`choose_request`]).
    Choose(Part, String),
    /// Open the folder of a part in the system's file manager ([`reveal_folder`]).
    OpenFolder(Part),
}

/// Handles `message` on the pages, with the settings as loaded last.
#[expect(
    clippy::only_used_in_recursion,
    reason = "the settings stay in the pages' interface; since issue #484 no message compares with them"
)]
pub fn update(state: &mut State, message: Message, settings: &Settings) -> Action {
    // The core takes the same names the settings file uses (`"german"`, `"dark"`, `true`), so the
    // values come from the enums' own serialisation. A value that cannot serialise (none here)
    // would become `null`, which the core refuses.
    let (key, value) = match message {
        Message::Language(language) => (SettingKey::Language, serde_json::to_value(language).unwrap_or_default()),
        Message::Appearance(appearance) => {
            (SettingKey::Appearance, serde_json::to_value(appearance).unwrap_or_default())
        }
        Message::StartAtLogin(start) => (SettingKey::StartAtLogin, serde_json::Value::Bool(start)),
        Message::Zoom(step) => return Action::Zoom(step),
        Message::Choose(part, id) => {
            state.problems = None;
            state.schemes_open = false;
            state.fonts_open = false;
            return Action::Choose(part, id);
        }
        Message::Problems(part, id) => {
            let shown = state.problems.as_ref().is_some_and(|(p, i)| *p == part && *i == id);
            state.problems = if shown { None } else { Some((part, id)) };
            state.schemes_open = false;
            state.fonts_open = false;
            return Action::None;
        }
        Message::ToggleSchemes => {
            state.schemes_open = !state.schemes_open;
            state.fonts_open = false;
            return Action::None;
        }
        Message::CloseSchemes => {
            state.schemes_open = false;
            return Action::None;
        }
        Message::ToggleFonts => {
            state.fonts_open = !state.fonts_open;
            state.schemes_open = false;
            if !state.fonts_open {
                return Action::None;
            }
            state.font_query.clear();
            return Action::Focus(FONT_SEARCH);
        }
        Message::CloseFonts => {
            state.fonts_open = false;
            return Action::None;
        }
        Message::FontQuery(words) => {
            state.font_query = words;
            return Action::None;
        }
        Message::FirstFont => {
            let first = ok(&state.fonts).and_then(|fonts| first_font(fonts, &state.font_query));
            return match first.map(|entry| entry.info.id.clone()) {
                Some(id) => update(state, Message::Choose(Part::Font, id), settings),
                None => Action::None,
            };
        }
        Message::Reload => return Action::ListLooks,
        Message::OpenFolder(part) => return Action::OpenFolder(part),
        Message::Quit => {
            state.quitting = true;
            return Action::None;
        }
        Message::QuitCancelled => {
            state.quitting = false;
            return Action::None;
        }
        Message::QuitConfirmed => {
            state.quitting = false;
            return Action::Send(Request::Quit);
        }
    };
    Action::Send(Request::SetSetting { key, value })
}

/// The request that saves the choice of `id` for `part` (Appearance): `settings.set_setting` with the
/// part's key and the id. A design sets nothing else (issue #484).
pub fn choose_request(part: Part, id: &str) -> Request {
    let key = match part {
        Part::Design => SettingKey::Design,
        Part::Font => SettingKey::Font,
        Part::ColorScheme => SettingKey::ColorScheme,
    };
    Request::SetSetting { key, value: serde_json::json!(id) }
}

/// Where a page is shown: its texts say what happens there.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    /// The control centre, with the tray icon.
    ControlCentre,
    /// An app without the main application: the service runs without a tray icon.
    App,
}

/// The group "Language", a list of choices – one row each, the whole row is the button, a check mark
/// at the chosen one (Deutsch is shown in its own name, like the language picker of any app).
pub fn language<'a>(settings: &Settings, place: Place) -> Element<'a, Message> {
    let chosen = settings.language;
    let language =
        |label: &'static str, language: Language| choice(label, chosen == language, Message::Language(language));
    widgets::group(
        tr("Language"),
        vec![
            language(tr("System"), Language::System),
            language("English", Language::English),
            // Shown in its own name on purpose: someone who cannot read the current language
            // must still find their way back.
            language("Deutsch", Language::German),
        ],
        Some(
            match place {
                Place::ControlCentre => {
                    tr("Applies to apps you open afterwards. “System” follows your system language.")
                }
                Place::App => {
                    tr("Applies when you open an app again, this one included. “System” follows your system language.")
                }
            }
            .to_string(),
        ),
    )
}

/// The group "Startup" with the "Start at login" switch; its sentence says what starts.
pub fn startup<'a>(settings: &Settings, place: Place) -> Element<'a, Message> {
    let footer = match place {
        Place::ControlCentre => tr(
            "Starts Torro Workplace in the background when you log in, so the tray icon and your workspaces are ready.",
        ),
        Place::App => {
            tr("Starts the background service when you log in, so reminders and sync run before you open an app.")
        }
    };
    widgets::group(
        tr("Startup"),
        vec![setting_row(None, tr("Start at login"), None, switch(settings.start_at_login, Message::StartAtLogin))],
        Some(footer.to_string()),
    )
}

/// The group "Background service": what it does, and "Quit", which asks first ([`overlay`]). Only in
/// an app without the main application; the control centre quits it from the tray.
pub fn service<'a>() -> Element<'a, Message> {
    let quit = widgets::text_button(None, tr("Quit…"), style::button::secondary, Some(Message::Quit));
    widgets::group(
        tr("Background service"),
        vec![setting_row(None, tr("Quit background service"), None, quit)],
        Some(tr("Quitting stops reminders, sync and notifications until a Torro app starts it again.").to_string()),
    )
}

/// The page General of an app without the main application: Language, Startup and Background
/// service.
pub fn general<'a>(settings: &Settings) -> Element<'a, Message> {
    column![language(settings, Place::App), startup(settings, Place::App), service()]
        .spacing(layout::section_gap())
        .into()
}

/// The three lists of the core with the parts read for their samples ([`Lists::read`]), or the
/// demo's ([`demo_looks`]) when `backend` is the demo, which touches nothing.
#[expect(clippy::result_large_err, reason = "an unexpected answer is handed back once, to be named in the error")]
pub async fn list(backend: Arc<dyn Backend>) -> Lists {
    if backend.demo() {
        return demo_looks();
    }
    let designs = listing(&backend, Request::ListDesigns, |answer| match answer {
        Response::Designs { designs } => Ok(designs),
        other => Err(other),
    })
    .await;
    let fonts = listing(&backend, Request::ListFonts, |answer| match answer {
        Response::Fonts { fonts } => Ok(fonts),
        other => Err(other),
    })
    .await;
    let schemes = listing(&backend, Request::ListColorSchemes, |answer| match answer {
        Response::ColorSchemes { color_schemes } => Ok(color_schemes),
        other => Err(other),
    })
    .await;
    // Fonts are opened to check them: off the window's thread.
    let (sender, receiver) = iced::futures::channel::oneshot::channel();
    std::thread::spawn(move || {
        let _ = sender.send(Lists::read(designs, fonts, schemes));
    });
    receiver.await.unwrap_or_else(|_| Lists::failed(tr("The list could not be read.").to_string()))
}

/// Asks `request` through `backend` and takes the list out of its answer with `pick`, which hands
/// back any other answer; an error answer, another answer or no answer becomes the text to show.
async fn listing<T>(
    backend: &Arc<dyn Backend>,
    request: Request,
    pick: impl FnOnce(Response) -> Result<T, Response>,
) -> Result<T, String> {
    let answer = backend.call(request).await?;
    pick(answer).map_err(|other| format!("unexpected answer: {other:?}"))
}

impl Lists {
    /// The lists the core gave, each entry the core listed as usable read from its folder for its
    /// sample (task L.23: each part apart). Blocking: fonts are opened to check them, and the
    /// computer's fonts are scanned once if one of them is listed.
    pub fn read(
        designs: Result<Vec<DesignInfo>, String>,
        fonts: Result<Vec<FontInfo>, String>,
        schemes: Result<Vec<ColorSchemeInfo>, String>,
    ) -> Lists {
        let (fonts_dir, schemes_dir) = (folder(Part::Font), folder(Part::ColorScheme));
        Lists {
            // The spacing levels are built in (issue #484): read without a folder.
            designs: designs.map(|list| {
                list.into_iter()
                    .map(|info| {
                        let design = info.valid.then(|| torro_core::design::load(&info.id).ok()).flatten();
                        Entry::new(info, design)
                    })
                    .collect()
            }),
            fonts: fonts.map(|list| {
                // The computer's fonts from one scan, not one per entry (issue #299), and only to draw
                // their samples: which there are and whether they can be used is the core's to say. A
                // family this window can't find draws its sample in the standard.
                let usable = |info: &FontInfo| info.origin == FontOrigin::System && info.valid;
                let system = list.iter().any(usable).then(torro_core::design::SystemFonts::scan);
                list.into_iter()
                    .map(|info| {
                        let font = match (&system, info.origin) {
                            (Some(scan), FontOrigin::System) if info.valid => scan.load(&info.id).ok(),
                            (_, FontOrigin::System) => None,
                            _ => read_part(fonts_dir.as_deref(), &info.id, info.valid, torro_core::design::load_font),
                        };
                        Entry::new(info, font)
                    })
                    .collect()
            }),
            schemes: schemes.map(|list| {
                list.into_iter()
                    .map(|info| {
                        let load = torro_core::design::load_color_scheme;
                        let scheme = read_part(schemes_dir.as_deref(), &info.id, info.valid, load);
                        Entry::new(info, scheme)
                    })
                    .collect()
            }),
        }
    }
}

/// The part `id` read from `dir` with `load`, only when the core listed it as `valid`: a part that
/// can't be read any more (its folder changed since the listing) has no sample either. The standard
/// needs no folder; `load` answers it without touching the disk.
fn read_part<T>(
    dir: Option<&std::path::Path>,
    id: &str,
    valid: bool,
    load: fn(&std::path::Path, &str) -> Result<T, Vec<torro_core::design::DesignError>>,
) -> Option<T> {
    valid.then(|| load(dir.unwrap_or(std::path::Path::new("")), id).ok()).flatten()
}

/// Creates the folder of `part` if it is missing and opens it with the system's file manager: `open`
/// on macOS, the Explorer on Windows, `xdg-open` elsewhere. Returns what went wrong, for the notice
/// above the page.
pub fn reveal_folder(part: Part) -> Result<(), String> {
    let dir = folder(part).ok_or_else(|| tr("No home directory").to_string())?;
    std::fs::create_dir_all(&dir).map_err(|err| err.to_string())?;
    #[cfg(target_os = "macos")]
    let program = "open";
    #[cfg(target_os = "windows")]
    let program = "explorer";
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let program = "xdg-open";
    let mut child = std::process::Command::new(program).arg(&dir).spawn().map_err(|err| err.to_string())?;
    // Waited for on a thread of its own, so it leaves no zombie behind.
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// The page Appearance (issue #484): the groups "Spacing", "Font", "Colors" and "Display", each laid
/// out for the width the page has.
pub fn appearance<'a>(state: &'a State, settings: &'a Settings) -> Element<'a, Message> {
    responsive(move |room: Size| page(state, settings, room.width)).height(Length::Shrink).into()
}

/// The groups of [`appearance`] for a page `width` wide: where it is wide (issue #464) "Spacing" on
/// the left and "Font", "Colors" and "Display" one under the other on the right; narrow, all four in
/// that order. Each is laid out for the width of its column.
fn page<'a>(state: &'a State, settings: &'a Settings, width: f32) -> Element<'a, Message> {
    let width = widgets::page_column_width(width, 2);
    widgets::page_columns(move || {
        let rest = column![font_group(state, settings), colors_group(state, settings, width), display(settings)]
            .spacing(layout::section_gap());
        vec![spacing(state, settings, width), rest.into()]
    })
}

/// The three colour modes in the order of the segment, each with its symbol and word.
fn modes() -> [(Icon, &'static str, Appearance); 3] {
    [
        (Icon::Monitor, tr("System"), Appearance::System),
        (Icon::Sun, tr("Light"), Appearance::Light),
        (Icon::Moon, tr("Dark"), Appearance::Dark),
    ]
}

/// The group "Display": the zoom, with the sentence that explains it under the card.
fn display<'a>(settings: &'a Settings) -> Element<'a, Message> {
    let zoom = setting_row(None, tr("Zoom"), None, zoom_control(settings.zoom));
    let footer = tr(
        "Makes all Torro apps larger or smaller, like zooming in a browser: Ctrl + Plus and Ctrl + Minus, Ctrl + 0 resets.",
    );
    widgets::group(tr("Display"), vec![zoom], Some(footer.to_string()))
}

/// How wide the colour mode's row must be for the segment's words: 16 at both sides and between
/// label and segment (a settings row), the label, the segment with its words.
fn mode_words_need() -> f32 {
    let labels = modes().map(|(_, label, _)| label);
    3.0 * space::l() + typo::text_width(tr("Color mode"), typo::body()) + widgets::icon_segmented_width(&labels, true)
}

/// The group "Spacing" (issue #484): the spacing levels as a segment in the order the core lists them
/// (Comfort, Standard, Dense, named in its words), under it the [`MailPreview`] of the level drawn,
/// and the sentence that explains it. The control centre follows a choice at once, so the whole page
/// shows the level as well.
fn spacing<'a>(state: &'a State, settings: &'a Settings, width: f32) -> Element<'a, Message> {
    // As high as the headings with buttons beside it, so the groups of both columns start on one line.
    let heading =
        container(widgets::group_heading(tr("Spacing"), Vec::new())).height(size::control_sm()).center_y(Fill);
    let mut block = column![heading].spacing(layout::item_gap());
    match listed_or(&state.designs, tr("The spacing levels can't be listed.")) {
        Ok(entries) => {
            let drawn = drawn(entries, &settings.design);
            let levels: Vec<&DesignEntry> = level_order(entries);
            let active = levels.iter().position(|entry| entry.info.id == drawn).unwrap_or_default();
            let options = levels
                .iter()
                .map(|entry| (entry.info.name.as_str(), Message::Choose(Part::Design, entry.info.id.clone())));
            let segment = container(widgets::segmented(options, active)).width(Fill).padding(layout::panel_padding());
            let mut rows: Vec<Element<'a, Message>> = vec![segment.into()];
            if let Some(design) = entries.iter().find(|entry| entry.info.id == drawn).and_then(|e| e.part.clone()) {
                rows.push(preview(design, width));
            }
            block = block.push(card(rows));
        }
        Err(placeholder) => block = block.push(card(vec![placeholder])),
    }
    let footer = tr(
        "How much room all Torro apps leave between their elements. Font and colors stay as they are; for larger text use the zoom.",
    );
    block.push(widgets::reading_cell(typo::text(typo::meta(), footer).style(torro_gui::theme::text_2))).into()
}

/// The levels of `entries` in the order of the segment: from the most room to the least (Comfort,
/// Standard, Dense), whatever order the core lists them in; a level it does not know goes last.
fn level_order(entries: &[DesignEntry]) -> Vec<&DesignEntry> {
    const ORDER: [&str; 3] = ["comfort", STANDARD_ID, "dense"];
    let rank = |entry: &&DesignEntry| ORDER.iter().position(|id| *id == entry.info.id).unwrap_or(ORDER.len());
    let mut levels: Vec<&DesignEntry> = entries.iter().filter(|entry| entry.info.valid).collect();
    levels.sort_by_key(rank);
    levels
}

/// The small mail window of `design` under the segment, in the proportions of [`MAIL_WINDOW`]: as
/// wide as the card in a column `width` wide, but at most [`size::panel()`] high, then centred in the
/// card – a whole window, never a cut-out of one that is drawn too large.
fn preview<'a>(design: Arc<Design>, width: f32) -> Element<'a, Message> {
    let wide = width.min(size::panel() * MAIL_WINDOW.width / MAIL_WINDOW.height);
    let high = wide * MAIL_WINDOW.height / MAIL_WINDOW.width;
    let window = container(canvas(MailPreview { design }).width(Fill).height(Fill)).width(wide).height(high).clip(true);
    container(window).center_x(Fill).into()
}

/// `rows` in the framed card of a settings group with a line between them, as `widgets::group` draws
/// them, for a group whose heading carries actions ([`widgets::group_heading`]).
fn card<'a>(rows: Vec<Element<'a, Message>>) -> Element<'a, Message> {
    let mut card = column![];
    for (index, row) in rows.into_iter().enumerate() {
        if index > 0 {
            card = card.push(rule::horizontal(size::HAIRLINE).style(style::rule));
        }
        card = card.push(row);
    }
    container(card).width(Fill).style(style::container::group).into()
}

/// The entries of `list` once the core listed them; until then the row that stands in their place:
/// quiet "Loading …" (the core answers in a moment at most), or `failed` with the core's reason and
/// an alert mark.
fn listed_or<'a, I, T>(
    list: &'a Option<Result<Vec<Entry<I, T>>, String>>,
    failed: &'a str,
) -> Result<&'a [Entry<I, T>], Element<'a, Message>> {
    match list {
        None => Err(row_frame(typo::text(typo::body(), tr("Loading …")).style(torro_gui::theme::text_3), false)),
        Some(Err(problem)) => {
            let alert = icon::icon(Icon::CircleAlert, icon::SIZE)
                .style(|t: &Theme, _| svg::Style { color: Some(palette(t).error_text) });
            Err(setting_row(Some(alert.into()), failed, Some(problem.clone()), None::<Element<'a, Message>>))
        }
        Some(Ok(entries)) => Ok(entries),
    }
}

/// The check mark at the end of the row or entry that is drawn, in `text`; the same room left empty
/// at the others, so their texts end in one column.
fn check_mark<'a>(on: bool) -> Element<'a, Message> {
    match on {
        true => {
            icon::icon(Icon::Check, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text) }).into()
        }
        false => blank().width(icon::SIZE).into(),
    }
}

/// The group "Font": the heading with "Reload" (for all three lists) and "Open fonts folder", the
/// font's row ([`font_row`]), the reasons of the font opened, and the sentence that explains it.
fn font_group<'a>(state: &'a State, settings: &'a Settings) -> Element<'a, Message> {
    let reload = widgets::icon_button(Icon::RefreshCw, tr("Reload"), Some(Message::Reload));
    let heading = widgets::group_heading(tr("Font"), vec![reload, open_folder(Part::Font)]);
    let font = match listed_or(&state.fonts, tr("The fonts can't be listed.")) {
        Ok(entries) => font_row(state, settings, entries),
        Err(placeholder) => placeholder,
    };
    let reasons = ok(&state.fonts)
        .and_then(|fonts| opened(fonts, Part::Font, state.problems.as_ref()))
        .map(|e| problems(&e.info));
    let footer = tr(
        "The typeface of all Torro apps, sized so that text reads alike in every font. More fonts go into the fonts folder, and the fonts installed on this computer can be used too.",
    );
    column![heading, card(vec![font])]
        .push(reasons)
        .push(widgets::reading_cell(typo::text(typo::meta(), footer).style(torro_gui::theme::text_2)))
        .spacing(layout::item_gap())
        .into()
}

/// The group "Colors": the heading with "Open color schemes folder", the colour scheme's row
/// ([`scheme_row`]) and "Color mode" as a segment of System, Light and Dark, the reasons of the scheme
/// opened, and the sentence that explains both. The control centre follows the colour mode at once:
/// it reloads the settings after every change. Where the segment's words do not fit beside its label
/// (a narrow column `width`, a long translation) it shows its symbols alone, the words as tooltips.
fn colors_group<'a>(state: &'a State, settings: &'a Settings, width: f32) -> Element<'a, Message> {
    let heading = widgets::group_heading(tr("Colors"), vec![open_folder(Part::ColorScheme)]);
    let scheme = match listed_or(&state.schemes, tr("The color schemes can't be listed.")) {
        Ok(entries) => scheme_row(state, settings, entries),
        Err(placeholder) => placeholder,
    };
    let chosen = settings.appearance;
    let active = modes().iter().position(|(_, _, mode)| *mode == chosen).unwrap_or_default();
    let parts = modes().map(|(which, label, mode)| (which, label, Message::Appearance(mode)));
    let segment = widgets::icon_segmented(parts, active, width >= mode_words_need());
    let mode = setting_row(None, tr("Color mode"), None, segment);
    let reasons = ok(&state.schemes)
        .and_then(|schemes| opened(schemes, Part::ColorScheme, state.problems.as_ref()))
        .map(|entry| problems(&entry.info));
    let footer = tr(
        "The colors of all Torro apps, light and dark. “System” follows your desktop. More color schemes go into the color schemes folder.",
    );
    column![heading, card(vec![scheme, mode])]
        .push(reasons)
        .push(widgets::reading_cell(typo::text(typo::meta(), footer).style(torro_gui::theme::text_2)))
        .spacing(layout::item_gap())
        .into()
}

/// The font's row: its name and the dropdown's button with the font drawn ([`font_button`]).
fn font_row<'a>(state: &'a State, settings: &'a Settings, entries: &'a [FontEntry]) -> Element<'a, Message> {
    let drawn = drawn(entries, &settings.font);
    let current = entries.iter().find(|entry| entry.info.id == drawn);
    setting_row(None, tr("Font"), None, font_button(&state.font_anchor, current))
}

/// The button of the font's dropdown, in the look of a secondary button [`size::sidebar()`] wide:
/// the name of the font drawn, written in it, and a chevron; it opens the dropdown under it
/// ([`overlay`]).
fn font_button<'a>(anchor: &Anchor, current: Option<&'a FontEntry>) -> Element<'a, Message> {
    let name = current.map_or("", |entry| font_name(&entry.info));
    let label = in_font(typo::text(typo::body(), name).wrapping(Wrapping::None), current);
    let content = row![container(label).width(Fill).clip(true), small_icon(Icon::ChevronDown, |p| p.text_3)]
        .spacing(space::s())
        .align_y(Alignment::Center);
    let open = button(container(content).center_y(Fill))
        .width(size::sidebar())
        .height(size::control_sm())
        .padding(style::button_padding())
        .style(style::button::secondary)
        .on_press(Message::ToggleFonts);
    widgets::anchored(anchor, open)
}

/// `label` written in the text font of `entry` at the size it draws body text; unchanged when the
/// entry has no font to draw with (one that can't be used, or a font of the computer this window
/// can't find).
fn in_font<'a>(label: text::Text<'a>, entry: Option<&FontEntry>) -> text::Text<'a> {
    let Some(font) = entry.and_then(|entry| entry.part.as_deref()) else { return label };
    let level = font.apply(torro_gui::design::active().typo.body);
    label
        .font(torro_gui::design::font_of(font, FontRole::Text, Weight::Regular))
        .size(Pixels(level.size))
        .line_height(LineHeight::Absolute(Pixels(level.line)))
}

/// What the font is called on the page: its family; the entry that follows the system's interface
/// font is "System".
fn font_name(info: &FontInfo) -> &str {
    if info.id == SYSTEM_ID { tr("System") } else { info.name() }
}

/// The sections of the font's dropdown (issue #299), in this order, a line between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FontSection {
    /// The suite's own: shipped with it or laid into the fonts folder.
    Torro,
    /// The fonts installed on this computer.
    System,
}

impl FontSection {
    fn title(self) -> &'static str {
        match self {
            FontSection::Torro => tr("Torro fonts"),
            FontSection::System => tr("System fonts"),
        }
    }
}

/// The groups of "Torro fonts", by their kind (`FontInfo::kind`), in this order; "Other" for one
/// without a kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FontGroup {
    Sans,
    Serif,
    Mono,
    Other,
}

impl FontGroup {
    const ALL: [FontGroup; 4] = [FontGroup::Sans, FontGroup::Serif, FontGroup::Mono, FontGroup::Other];

    fn of(info: &FontInfo) -> FontGroup {
        match info.kind {
            Some(FontKind::Sans) => FontGroup::Sans,
            Some(FontKind::Serif) => FontGroup::Serif,
            Some(FontKind::Mono) => FontGroup::Mono,
            None => FontGroup::Other,
        }
    }

    fn title(self) -> &'static str {
        match self {
            FontGroup::Sans => tr("Sans"),
            FontGroup::Serif => tr("Serif"),
            FontGroup::Mono => tr("Mono"),
            FontGroup::Other => tr("Other"),
        }
    }
}

/// One section of the font's dropdown: its groups (`None`: no group title, the computer's fonts),
/// each with its fonts.
type FontList<'a> = (FontSection, Vec<(Option<FontGroup>, Vec<&'a FontEntry>)>);

/// The fonts that match `query` (name or family, ignoring case; all for an empty one) by section and
/// group: in "Torro fonts" the standard first in its group, then by name; in "System fonts"
/// "System" first, then by name. A group or section without a match is left out.
fn font_sections<'a>(entries: &'a [FontEntry], query: &str) -> Vec<FontList<'a>> {
    let words = query.trim().to_lowercase();
    let matching: Vec<&FontEntry> = entries.iter().filter(|entry| matches(&entry.info, &words)).collect();
    let sorted = |mut fonts: Vec<&'a FontEntry>, first: fn(&FontInfo) -> bool| {
        fonts.sort_by_key(|entry| (!first(&entry.info), font_name(&entry.info).to_lowercase()));
        fonts
    };
    let (system, own): (Vec<&FontEntry>, Vec<&FontEntry>) =
        matching.into_iter().partition(|entry| entry.info.origin == FontOrigin::System);
    let groups: Vec<(Option<FontGroup>, Vec<&FontEntry>)> = FontGroup::ALL
        .into_iter()
        .map(|group| {
            let members = own.iter().copied().filter(|entry| FontGroup::of(&entry.info) == group).collect();
            (Some(group), sorted(members, |info| info.standard))
        })
        .filter(|(_, members)| !members.is_empty())
        .collect();
    let mut sections = Vec::new();
    if !groups.is_empty() {
        sections.push((FontSection::Torro, groups));
    }
    if !system.is_empty() {
        sections.push((FontSection::System, vec![(None, sorted(system, |info| info.id == SYSTEM_ID))]));
    }
    sections
}

/// Whether the font `info` matches the lowercase `words`: its name on the page, its pack's name or
/// its family contains them.
fn matches(info: &FontInfo, words: &str) -> bool {
    words.is_empty()
        || [Some(font_name(info)), Some(info.name.as_str()), info.family.as_deref()]
            .into_iter()
            .flatten()
            .any(|name| name.to_lowercase().contains(words))
}

/// The font Enter takes: the first match of `query` in the dropdown's order that can be used.
fn first_font<'a>(entries: &'a [FontEntry], query: &str) -> Option<&'a FontEntry> {
    font_sections(entries, query)
        .into_iter()
        .flat_map(|(_, groups)| groups)
        .flat_map(|(_, members)| members)
        .find(|entry| entry.info.valid)
}

/// The font's dropdown: a menu with search ([`widgets::search_menu`], [`size::props()`] wide) with the
/// [`font_sections`] of the words typed – section titles in `text_2`, group titles in `text_3`, a line
/// between the sections, an entry per font ([`font_item`]).
fn font_menu<'a>(state: &'a State, settings: &'a Settings, entries: &'a [FontEntry]) -> Element<'a, Message> {
    let drawn = drawn(entries, &settings.font);
    let open = state.problems.as_ref().filter(|(part, _)| *part == Part::Font).map(|(_, id)| id.as_str());
    let title = |words: &'static str, tone: fn(&Theme) -> text::Style| -> Element<'a, Message> {
        container(typo::text(typo::caption(), words).style(tone))
            .padding(Padding::new(space::s()).bottom(space::xs()))
            .into()
    };
    let mut items: Vec<Element<'a, Message>> = Vec::new();
    for (section, groups) in font_sections(entries, &state.font_query) {
        if !items.is_empty() {
            items.push(
                container(rule::horizontal(size::HAIRLINE).style(style::rule)).padding([space::xs(), 0.0]).into(),
            );
        }
        items.push(title(section.title(), torro_gui::theme::text_2));
        for (group, members) in groups {
            if let Some(group) = group {
                items.push(title(group.title(), torro_gui::theme::text_3));
            }
            items.extend(members.into_iter().map(|entry| {
                let id = entry.info.id.as_str();
                font_item(entry, id == drawn, open == Some(id))
            }));
        }
    }
    widgets::search_menu(FONT_SEARCH, &state.font_query, Message::FontQuery, Message::FirstFont, items, size::props())
}

/// A font of the dropdown, the whole entry a menu item: its name written in it ("pack · version ·
/// author · licence" as tooltip; for "System" the family it stands for), the check mark at the one
/// `drawn`. One that can't be used has its name in `text_3` and an alert mark; its click shows its
/// reasons under the group (`open`: they are shown).
fn font_item<'a>(entry: &'a FontEntry, drawn: bool, open: bool) -> Element<'a, Message> {
    let info = &entry.info;
    let tip = match info.id == SYSTEM_ID {
        true => info.family.clone(),
        false => byline([
            Some(info.name.as_str()),
            Some(info.version.as_str()),
            info.author.as_deref(),
            info.license.as_deref(),
        ]),
    };
    let name = in_font(typo::text(typo::body(), font_name(info)).wrapping(Wrapping::None), Some(entry));
    let (name, press) = match info.valid {
        true => (name, Message::Choose(Part::Font, info.id.clone())),
        false => (name.style(torro_gui::theme::text_3), Message::Problems(Part::Font, info.id.clone())),
    };
    let mut content = row![container(name).width(Fill).clip(true)].spacing(space::s()).align_y(Alignment::Center);
    if !info.valid {
        content = content.push(small_icon(Icon::CircleAlert, |p| p.text_3));
    }
    let item = button(container(content.push(check_mark(drawn))).center_y(Fill))
        .width(Fill)
        .height(style::MENU_ITEM_HEIGHT)
        .padding(style::ITEM_PADDING)
        .style(style::button::menu_item(open))
        .on_press(press);
    match tip {
        Some(tip) => widgets::with_tooltip(item, tip),
        None => item.into(),
    }
}

/// `which` at the size of icons in buttons, in the colour `pick` takes from the window's palette.
fn small_icon<'a>(which: Icon, pick: fn(&Palette) -> Color) -> Element<'a, Message> {
    icon::icon(which, icon::SIZE_SM).style(move |t: &Theme, _| svg::Style { color: Some(pick(&palette(t))) }).into()
}

/// The colour scheme's row: its name and the dropdown's button with the scheme drawn
/// ([`scheme_button`]).
fn scheme_row<'a>(state: &'a State, settings: &'a Settings, entries: &'a [SchemeEntry]) -> Element<'a, Message> {
    let drawn = drawn(entries, &settings.color_scheme);
    let current = entries.iter().find(|entry| entry.info.id == drawn);
    setting_row(None, tr("Color scheme"), None, scheme_button(&state.anchor, current))
}

/// The button of the colour scheme's dropdown, in the look of a secondary button
/// [`size::sidebar()`] wide: the stripes and the name of the scheme drawn, a chevron; it opens the
/// dropdown under it ([`overlay`]).
fn scheme_button<'a>(anchor: &Anchor, current: Option<&'a SchemeEntry>) -> Element<'a, Message> {
    let name = current.map_or("", |entry| entry.info.name.as_str());
    let content = row![
        swatch(current.and_then(|entry| entry.part.as_ref())),
        container(typo::text(typo::body(), name).wrapping(Wrapping::None)).width(Fill).clip(true),
        small_icon(Icon::ChevronDown, |p| p.text_3),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center);
    let open = button(container(content).center_y(Fill))
        .width(size::sidebar())
        .height(size::control_sm())
        .padding(style::button_padding())
        .style(style::button::secondary)
        .on_press(Message::ToggleSchemes);
    widgets::anchored(anchor, open)
}

/// The pages' layer over the window: the question "Quit background service?", or the dropdown
/// that is open, under its button, its right edge on the button's – the colour scheme's
/// ([`scheme_menu`]) or the font's ([`font_menu`]). A click next to it closes it.
pub fn overlay<'a>(state: &'a State, settings: &'a Settings) -> Option<Element<'a, Message>> {
    if state.quitting {
        return Some(confirm(
            tr("Quit background service?"),
            tr("Reminders, sync and notifications stop until a Torro app starts it again.").to_string(),
            tr("Quit service"),
            Message::QuitConfirmed,
            Message::QuitCancelled,
        ));
    }
    if state.schemes_open {
        let entries = ok(&state.schemes)?;
        let menu = scheme_menu(entries, drawn(entries, &settings.color_scheme));
        return Some(widgets::floating(&state.anchor, Placement::Below(Align::End), menu, Message::CloseSchemes));
    }
    if state.fonts_open {
        let menu = font_menu(state, settings, ok(&state.fonts)?);
        return Some(widgets::floating(&state.font_anchor, Placement::Below(Align::End), menu, Message::CloseFonts));
    }
    None
}

/// The sections of the colour scheme's dropdown, in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SchemeGroup {
    /// The standard colour scheme the suite ships.
    Torro,
    /// Every other one with a light and a dark mode.
    LightAndDark,
    /// The ones that are only dark (`ColorSchemeInfo::dark_only`).
    DarkOnly,
}

impl SchemeGroup {
    const ALL: [SchemeGroup; 3] = [SchemeGroup::Torro, SchemeGroup::LightAndDark, SchemeGroup::DarkOnly];

    /// The section of the colour scheme `info`.
    fn of(info: &ColorSchemeInfo) -> SchemeGroup {
        match (info.standard, info.dark_only) {
            (true, _) => SchemeGroup::Torro,
            (false, true) => SchemeGroup::DarkOnly,
            (false, false) => SchemeGroup::LightAndDark,
        }
    }

    /// The section's title: "Torro" is a name and stays as it is.
    fn title(self) -> &'static str {
        match self {
            SchemeGroup::Torro => "Torro",
            SchemeGroup::LightAndDark => tr("Light and dark"),
            SchemeGroup::DarkOnly => tr("Dark only"),
        }
    }
}

/// The colour schemes by [`SchemeGroup`], in the order of the sections and of the list within each;
/// a section without one is left out.
fn scheme_groups(entries: &[SchemeEntry]) -> Vec<(SchemeGroup, Vec<&SchemeEntry>)> {
    SchemeGroup::ALL
        .into_iter()
        .map(|group| (group, entries.iter().filter(|entry| SchemeGroup::of(&entry.info) == group).collect::<Vec<_>>()))
        .filter(|(_, members)| !members.is_empty())
        .collect()
}

/// The dropdown's card in the look of a menu ([`size::props()`] wide, scrolling past
/// [`size::panel()`]): each [`SchemeGroup`] with its title, a line between them, an entry per colour
/// scheme ([`scheme_item`]), the one `drawn` with the check mark.
fn scheme_menu<'a>(entries: &'a [SchemeEntry], drawn: &str) -> Element<'a, Message> {
    let mut list = column![];
    for (index, (group, members)) in scheme_groups(entries).into_iter().enumerate() {
        if index > 0 {
            list =
                list.push(container(rule::horizontal(size::HAIRLINE).style(style::rule)).padding([space::xs(), 0.0]));
        }
        let title = typo::text(typo::caption(), group.title()).style(torro_gui::theme::text_3);
        list = list.push(container(title).padding(Padding::new(space::s()).bottom(space::xs())));
        list = list.extend(members.into_iter().map(|entry| scheme_item(entry, entry.info.id == drawn)));
    }
    let body = scrollable(list)
        .direction(scrollable::Direction::Vertical(style::scrollbar()))
        .style(style::scrollable)
        .height(Length::Shrink)
        .width(Fill);
    container(container(body).max_height(size::panel()))
        .width(size::props())
        .padding(style::MENU_INSET)
        .style(style::container::menu)
        .into()
}

/// A colour scheme of the dropdown: its stripes, its name and the check mark at the one `drawn`, the
/// whole entry a menu item. One that can't be used has an alert mark for the stripes and its name in
/// `text_3`; its click shows its reasons under the group.
fn scheme_item<'a>(entry: &'a SchemeEntry, drawn: bool) -> Element<'a, Message> {
    let info = &entry.info;
    let name = typo::text(typo::body(), info.name.as_str()).wrapping(Wrapping::None);
    let (lead, name, press) = match (info.valid, entry.part.as_ref()) {
        (true, part) => (swatch(part), name, Message::Choose(Part::ColorScheme, info.id.clone())),
        (false, _) => (
            small_icon(Icon::CircleAlert, |p| p.text_3),
            name.style(torro_gui::theme::text_3),
            Message::Problems(Part::ColorScheme, info.id.clone()),
        ),
    };
    let content = row![lead, container(name).width(Fill).clip(true), check_mark(drawn)]
        .spacing(space::s())
        .align_y(Alignment::Center);
    button(container(content).center_y(Fill))
        .width(Fill)
        .height(style::MENU_ITEM_HEIGHT)
        .padding(style::ITEM_PADDING)
        .style(style::button::menu_item(false))
        .on_press(press)
        .into()
}

/// The four colours of a colour scheme's stripes, left to right: ground, surface, text, accent.
fn stripes() -> [fn(&Palette) -> Color; 4] {
    [|p| p.bg, |p| p.panel, |p| p.text, |p| p.accent]
}

/// A colour scheme's four [`stripes`] side by side in the window's light or dark mode (a scheme that
/// is only dark, dark in both), [`size::tile()`] wide and [`icon::SIZE`] high, in a `line_strong`
/// frame so a stripe of the window's own colour still shows its edge. Empty for a scheme that could
/// not be read.
fn swatch<'a>(scheme: Option<&Arc<ColorScheme>>) -> Element<'a, Message> {
    let parts = stripes().into_iter().map(|pick| -> Element<'a, Message> {
        let scheme = scheme.cloned();
        container(blank().width(Fill).height(Fill))
            .width(Fill)
            .height(Fill)
            .style(move |t: &Theme| container::Style {
                background: scheme.as_deref().map(|scheme| Background::Color(pick(&colors(scheme, t)))),
                ..container::Style::default()
            })
            .into()
    });
    container(row(parts))
        .width(size::tile())
        .height(icon::SIZE)
        .padding(size::HAIRLINE)
        .clip(true)
        .style(|t: &Theme| container::Style {
            border: Border { color: palette(t).line_strong, width: size::HAIRLINE, radius: radius::xs().into() },
            ..container::Style::default()
        })
        .into()
}

/// The colours of `scheme` in the mode of `theme` (the window's light or dark).
fn colors(scheme: &ColorScheme, theme: &Theme) -> Palette {
    let mode = if theme.extended_palette().is_dark { Mode::Dark } else { Mode::Light };
    Palette::of(scheme, mode)
}

/// The id of the entry the windows draw with: `chosen` (`Settings::design`, `font` or
/// `color_scheme`) if it is listed and can be used, else the standard, which the suite falls back to
/// (concept: "Wählen und wechseln"). An empty `chosen` (a core from before task L.22) is the
/// standard too.
fn drawn<'a, I: Listed, T>(entries: &'a [Entry<I, T>], chosen: &'a str) -> &'a str {
    if entries.iter().any(|entry| entry.info.id() == chosen && entry.info.valid()) { chosen } else { STANDARD_ID }
}

/// The entry whose reasons are shown: the one `open` names, while its list is `part`'s and it is
/// listed and can't be used. After "Reload" an entry that was fixed is drawn again, and nothing is
/// left open for it.
fn opened<'a, I: Listed, T>(
    entries: &'a [Entry<I, T>],
    part: Part,
    open: Option<&(Part, String)>,
) -> Option<&'a Entry<I, T>> {
    let (which, id) = open?;
    if *which != part {
        return None;
    }
    entries.iter().find(|entry| entry.info.id() == id && !entry.info.valid())
}

/// Under the group, for the entry that can't be used that was clicked: its name, "Can't be used:"
/// and every reason, at full strength and wrapping – they say what to fix, one per line, so an
/// entry with many can be worked through like a list.
fn problems<'a, I: Listed>(info: &'a I) -> Element<'a, Message> {
    let mark =
        icon::icon(Icon::CircleAlert, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) });
    let name = row![mark, typo::text(typo::body_strong(), info.name())].spacing(space::s()).align_y(Alignment::Center);
    let mut lines = column![name, typo::text(typo::meta(), tr("Can't be used:")).style(torro_gui::theme::text_2)]
        .spacing(space::xs());
    for reason in info.problems() {
        lines = lines.push(typo::text(typo::meta(), reason.as_str()).style(torro_gui::theme::text_2));
    }
    container(lines).width(Fill).padding(layout::panel_padding()).style(style::container::group).into()
}

/// The parts of a tooltip that are there and not empty, joined with " · " ("1.2.0 · Anna Berg");
/// `None` when none is.
fn byline<'s>(parts: impl IntoIterator<Item = Option<&'s str>>) -> Option<String> {
    let parts: Vec<&str> = parts.into_iter().flatten().filter(|part| !part.is_empty()).collect();
    (!parts.is_empty()).then(|| parts.join(" · "))
}

// ----- The small mail window of a spacing level (task L.37, issue #484) ---------------------------

/// The window a [`MailPreview`] shows in small, in logical pixels: a mail window of a laptop, small
/// enough that the gaps of a level can be told apart in the preview.
const MAIL_WINDOW: Size = Size::new(1024.0, 640.0);

/// The share of a text size a line of letters covers seen from afar (about the x-height): how high
/// the bars are that stand for text in a [`MailPreview`].
const INK: f32 = 0.5;

/// The widths of lines of text in a [`MailPreview`] as shares of their room, repeated, so a
/// paragraph reads as text and not as stripes.
const LINES: [f32; 4] = [1.0, 0.92, 0.97, 0.58];

/// How many rows the sidebar of a [`MailPreview`] has, and how many icons its action rail.
const NAV_ROWS: usize = 7;
const RAIL_ICONS: usize = 5;

/// The mail window of the suite in small, drawn from the values of `design` – sidebar, header, the
/// list of mails as the design draws lists (`[list]`: rows with avatar and preview or one line,
/// hairlines, the chosen row with the accent bar or in accent), the reader in its reading column
/// and the action rail at the right – in the colours of the window's palette, so only what the
/// design changes differs from row to row. Text is drawn as bars at the height of its level.
#[derive(Debug, Clone)]
struct MailPreview {
    design: Arc<Design>,
}

impl<M> canvas::Program<M> for MailPreview {
    type State = ();

    fn draw(
        &self,
        _state: &Self::State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        _cursor: mouse::Cursor,
    ) -> Vec<canvas::Geometry> {
        let mut frame = canvas::Frame::new(renderer, bounds.size());
        paint(&mut frame, &self.design, &palette(theme), bounds.size());
        vec![frame.into_geometry()]
    }
}

/// The regions of a [`MailPreview`] `width` wide, in its pixels: the design's sizes scaled from
/// [`MAIL_WINDOW`].
#[derive(Debug, Clone, Copy, PartialEq)]
struct Regions {
    /// Pixels of the preview per pixel of the window.
    scale: f32,
    /// The width of the sidebar (`size.sidebar`).
    sidebar: f32,
    /// The height of the header (`size.header`).
    header: f32,
    /// The width of the list (`size.list`).
    list: f32,
    /// The width of the collapsed action rail (`size.rail`).
    rail: f32,
    /// The height of a mail row (`size.row_message`).
    row: f32,
}

impl Regions {
    fn of(design: &Design, width: f32) -> Regions {
        let scale = width / MAIL_WINDOW.width;
        let size = &design.size;
        Regions {
            scale,
            sidebar: size.sidebar * scale,
            header: size.header * scale,
            list: size.list * scale,
            rail: size.rail * scale,
            row: size.row_message * scale,
        }
    }
}

/// Paints the mail window of `design` onto `frame` of `area` in the colours `p`.
fn paint(frame: &mut canvas::Frame, design: &Design, p: &Palette, area: Size) {
    let r = Regions::of(design, area.width);
    frame.fill_rectangle(Point::ORIGIN, area, p.panel);
    sidebar(frame, design, p, &r, area.height);
    let content = r.sidebar;
    fill(frame, content, 0.0, size::HAIRLINE, area.height, p.line);
    // The header: the title, a line under it across the content.
    let title_x = content + design.layout.row_padding.horizontal * r.scale;
    ink(frame, title_x, r.header / 2.0, r.list * 0.3, &design.typo.body_strong, r.scale, p.text);
    fill(frame, content, r.header - size::HAIRLINE, area.width - content, size::HAIRLINE, p.line);
    let rail_x = area.width - r.rail;
    mail_list(frame, design, p, &r, content, area.height);
    reader(frame, design, p, &r, content + r.list, rail_x, area.height);
    rail(frame, design, p, &r, rail_x, area.height);
}

/// A rectangle at least a hairline high.
fn fill(frame: &mut canvas::Frame, x: f32, y: f32, width: f32, height: f32, colour: Color) {
    frame.fill_rectangle(Point::new(x, y), Size::new(width.max(0.0), height.max(size::HAIRLINE)), colour);
}

/// A bar that stands for a line of text of `level` (scaled by `scale`), its middle at `middle`.
fn ink(frame: &mut canvas::Frame, x: f32, middle: f32, width: f32, level: &Level, scale: f32, colour: Color) {
    let height = (level.size * INK * scale).max(size::HAIRLINE);
    fill(frame, x, middle - height / 2.0, width, height, colour);
}

/// The sidebar on `bg`: the app switcher (logo in accent, the app's name) in the header's height,
/// then its rows, the first one chosen.
fn sidebar(frame: &mut canvas::Frame, design: &Design, p: &Palette, r: &Regions, height: f32) {
    let k = r.scale;
    fill(frame, 0.0, 0.0, r.sidebar, height, p.bg);
    let (pad, mark, gap) = (design.space.m * k, icon::SIZE * k, design.space.s * k);
    let middle = r.header / 2.0;
    fill(frame, pad, middle - mark / 2.0, mark, mark, p.accent);
    ink(frame, pad + mark + gap, middle, r.sidebar * 0.3, &design.typo.body_strong, k, p.text);
    let step = (design.size.row_sidebar * k).max(size::HAIRLINE);
    for index in 0..NAV_ROWS {
        let top = r.header + index as f32 * step;
        if top + step > height {
            break;
        }
        let middle = top + step / 2.0;
        let tone = if index == 0 { p.text } else { p.text_2 };
        if index == 0 {
            fill(frame, pad / 2.0, top, r.sidebar - pad, step, p.selected);
        }
        fill(frame, pad, middle - mark / 2.0, mark, mark, p.text_3);
        let x = pad + mark + gap;
        let width = (r.sidebar - x - pad) * LINES[index % LINES.len()] * 0.6;
        ink(frame, x, middle, width, &design.typo.body, k, tone);
    }
}

/// The list of mails from `x`: a row of `size.row_message` after the other down to `height`, the
/// second one chosen; a line at its right edge.
fn mail_list(frame: &mut canvas::Frame, design: &Design, p: &Palette, r: &Regions, x: f32, height: f32) {
    let (k, look) = (r.scale, design.list);
    let pad = design.layout.row_padding;
    let (pad_x, pad_y) = (pad.horizontal * k, pad.vertical * k);
    let step = r.row.max(size::HAIRLINE);
    let mut top = r.header;
    let mut index = 0;
    while top < height {
        let chosen = index == 1;
        let filled = chosen && look.selection == ListSelection::Fill;
        if chosen {
            fill(frame, x, top, r.list, step, if filled { p.accent } else { p.selected });
            if !filled {
                fill(frame, x, top, 2.0 * size::HAIRLINE, step, p.accent);
            }
        }
        // Text, secondary text, quiet text, avatar.
        let tones = match filled {
            true => [p.on_accent; 4],
            false => [p.text, p.text_2, p.text_3, p.line_strong],
        };
        let cell = Rectangle { x: x + pad_x, y: top + pad_y, width: r.list - 2.0 * pad_x, height: step - 2.0 * pad_y };
        match look.style {
            ListStyle::Line => mail_line(frame, design, k, cell, tones),
            ListStyle::Rows | ListStyle::Cards => mail_rows(frame, design, k, cell, tones),
        }
        if look.separators {
            fill(frame, x, top + step - size::HAIRLINE, r.list, size::HAIRLINE, p.line);
        }
        top += step;
        index += 1;
    }
    fill(frame, x + r.list, r.header, size::HAIRLINE, height - r.header, p.line);
}

/// A mail as one line in fixed columns (`[list] style = "line"`): date, sender, subject.
fn mail_line(frame: &mut canvas::Frame, design: &Design, k: f32, cell: Rectangle, tones: [Color; 4]) {
    let (body, gap, middle) = (&design.typo.body, design.space.m * k, cell.y + cell.height / 2.0);
    let (date, sender) = (cell.width * 0.1, cell.width * 0.22);
    ink(frame, cell.x, middle, date * 0.8, body, k, tones[2]);
    ink(frame, cell.x + date + gap, middle, sender * 0.8, body, k, tones[0]);
    let subject = cell.x + date + sender + 2.0 * gap;
    ink(frame, subject, middle, (cell.x + cell.width - subject) * 0.7, body, k, tones[1]);
}

/// A mail as a row with its lines (`[list] style = "rows"`): the avatar (`[list] avatar`, none at 0),
/// sender and time, subject and `[list] preview_lines` of preview, each line of `body` height.
fn mail_rows(frame: &mut canvas::Frame, design: &Design, k: f32, cell: Rectangle, tones: [Color; 4]) {
    let look = design.list;
    let mut left = cell.x;
    if look.avatar > 0.0 {
        let edge = look.avatar * k;
        frame.fill(&canvas::Path::circle(Point::new(left + edge / 2.0, cell.y + edge / 2.0), edge / 2.0), tones[3]);
        left += edge + design.space.m * k;
    }
    let width = (cell.x + cell.width - left).max(0.0);
    let line = design.typo.body.line * k;
    let typo = &design.typo;
    let lines = [
        (0.4, &typo.body_strong, tones[0]),
        (0.75, &typo.body, tones[0]),
        (0.9, &typo.body, tones[1]),
        (0.7, &typo.body, tones[1]),
    ];
    let count = 2 + usize::from(look.preview_lines);
    for (index, (share, level, colour)) in lines.into_iter().take(count).enumerate() {
        let middle = cell.y + line * (index as f32 + 0.5);
        if middle > cell.y + cell.height {
            break;
        }
        ink(frame, left, middle, width * share, level, k, colour);
    }
    let time = width * 0.12;
    ink(frame, cell.x + cell.width - time, cell.y + line / 2.0, time, &typo.meta, k, tones[2]);
}

/// The reader between `left` and `right`: the subject and paragraphs in the reading column –
/// `size.reading_max` wide at most, `layout.reading_padding` inside.
fn reader(frame: &mut canvas::Frame, design: &Design, p: &Palette, r: &Regions, left: f32, right: f32, height: f32) {
    let k = r.scale;
    let pad = design.layout.reading_padding;
    let x = left + pad.horizontal * k;
    let column = (design.size.reading_max * k).min(right - left - 2.0 * pad.horizontal * k).max(0.0);
    let (title, read) = (&design.typo.title, &design.typo.read);
    let mut y = r.header + pad.vertical * k;
    ink(frame, x, y + title.line * k / 2.0, column * 0.7, title, k, p.text);
    let line = (read.line * k).max(size::HAIRLINE);
    y += title.line * k + line;
    let mut index = 0;
    while y < height {
        ink(frame, x, y + line / 2.0, column * LINES[index % LINES.len()], read, k, p.text_3);
        y += line;
        index += 1;
        // A blank line after each paragraph.
        if index % LINES.len() == 0 {
            y += line;
        }
    }
}

/// The collapsed action rail from `x` to the right edge, a line at its left: an icon per group, the
/// first (Reply) in accent.
fn rail(frame: &mut canvas::Frame, design: &Design, p: &Palette, r: &Regions, x: f32, height: f32) {
    let k = r.scale;
    fill(frame, x, r.header, size::HAIRLINE, height - r.header, p.line);
    let (mark, step) = (icon::SIZE * k, design.size.control_lg * k);
    for index in 0..RAIL_ICONS {
        let middle = r.header + design.space.s * k + step * (index as f32 + 0.5);
        let colour = if index == 0 { p.accent } else { p.text_3 };
        fill(frame, x + (r.rail - mark) / 2.0, middle - mark / 2.0, mark, mark, colour);
    }
}

/// The folder of `part` in the data directory (`fonts/`, `color-schemes/`); `None` without a home
/// directory, and always for designs, which have no folder since issue #484.
pub fn folder(part: Part) -> Option<PathBuf> {
    match part {
        Part::Design => None,
        Part::Font => torro_core::design::font_dir(),
        Part::ColorScheme => torro_core::design::color_scheme_dir(),
    }
}

/// "Open fonts folder" or "Open color schemes folder", a quiet action of a heading that names the
/// folder (from home, as `~/…`) in its tooltip. Designs have no folder (issue #484): nothing.
fn open_folder<'a>(part: Part) -> Element<'a, Message> {
    let label = match part {
        Part::Design => return blank().into(),
        Part::Font => tr("Open fonts folder"),
        Part::ColorScheme => tr("Open color schemes folder"),
    };
    let press = Some(Message::OpenFolder(part));
    let open = widgets::text_button(Some(Icon::Folder), label, style::button::ghost, press);
    match folder(part) {
        Some(dir) => widgets::with_tooltip(open, home_relative(&dir)),
        None => open,
    }
}

/// `path` with the home directory written as `~`, so the part that matters fits the row.
fn home_relative(path: &std::path::Path) -> String {
    match std::env::home_dir().and_then(|home| path.strip_prefix(home).ok().map(std::path::Path::to_path_buf)) {
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => path.display().to_string(),
    }
}

/// −, the level, + and "Reset". The level keeps the width of the widest one ("200%"), so the
/// buttons stay where they are; a button that would change nothing is off.
fn zoom_control<'a>(level: Zoom) -> Element<'a, Message> {
    let press = |step: Step| (step.apply(level) != level).then_some(Message::Zoom(step));
    let width = zoom_value_width();
    let value = typo::text(typo::body().tabular(), zoom::label(level)).wrapping(Wrapping::None);
    let value = container(value).center_x(width);
    row![
        widgets::icon_button(Icon::Minus, tr("Zoom out"), press(Step::Out)),
        value,
        widgets::icon_button(Icon::Plus, tr("Zoom in"), press(Step::In)),
        widgets::text_button(None, tr("Reset"), style::button::secondary, press(Step::Reset)),
    ]
    .spacing(space::xs())
    .align_y(Alignment::Center)
    .into()
}

/// The width of the zoom's value: the estimate of the widest level ("200%", German "200 %") rounded
/// up by [`space::xs()`] – an estimate a hair short broke "100 %" at its space (issue #291).
fn zoom_value_width() -> f32 {
    typo::text_width(&zoom::label(Zoom::MAX), typo::body().tabular()).ceil() + space::xs()
}

/// What the demo's samples say about themselves: name, version, author "Torro samples" and a
/// description.
fn sample_about(name: &str, version: &str, description: &str) -> torro_core::design::About {
    torro_core::design::About {
        name: name.into(),
        version: version.into(),
        author: Some("Torro samples".into()),
        description: Some(description.into()),
    }
}

/// The designs, fonts and colour schemes of `--demo` (Appearance, tasks L.4, L.23 and L.37), in memory
/// like all its data, each list with the standard first, samples that can be chosen and are drawn
/// at once, and one that breaks rules and says so:
///
/// - designs: the three built-in spacing levels, Standard, Comfort and Dense (issue #484);
/// - fonts: "Tabular" (Inter's tabular cut as text font: the only other family the demo has without
///   a folder), "Brush" (broken);
/// - colour schemes: "Harbor" (teal accent, cool surfaces), "Paper" (warm light surfaces, blue
///   accent), "Midnight" (dark only), "Neon" (broken).
///
/// A font or colour scheme `--font` or `--color-scheme` names (read from its folder at start, so it is
/// the active one) joins its list after the samples, so it carries the check mark.
pub fn demo_looks() -> Lists {
    Lists { designs: Ok(demo_designs()), fonts: Ok(demo_fonts()), schemes: Ok(demo_schemes()) }
}

/// The designs of [`demo_looks`]: the three built-in spacing levels, as the core lists them.
fn demo_designs() -> Vec<DesignEntry> {
    torro_core::design::BUILT_IN_IDS
        .iter()
        .filter_map(|id| torro_core::design::load(id).ok())
        .map(|design| {
            let info = DesignInfo {
                id: design.id.clone(),
                name: design.about.name.clone(),
                version: design.about.version.clone(),
                author: design.about.author.clone(),
                description: design.about.description.clone(),
                standard: design.is_standard(),
                valid: true,
                problems: Vec::new(),
                active: false,
                density: design.density_level(),
            };
            Entry::new(info, Some(design))
        })
        .collect()
}

/// The fonts of [`demo_looks`]: the standard (bundled, Sans), Tabular from a folder without a kind
/// ("Other"), Brush broken, and three fonts of the computer (issue #299) – "System", Avenir Next and
/// Monaco – listed without a font to draw with, so their samples are the standard's and the pictures
/// look alike on every computer.
fn demo_fonts() -> Vec<FontEntry> {
    use torro_core::design::{DesignError, FontSet};
    let info = |font: &FontSet| FontInfo {
        id: font.id.clone(),
        family: Some(font.fonts.text.family.clone()),
        name: font.about.name.clone(),
        version: font.about.version.clone(),
        author: font.about.author.clone(),
        description: font.about.description.clone(),
        license: Some(font.license.clone()),
        origin: torro_ipc::FontOrigin::Bundled,
        kind: font.kind,
        standard: font.is_standard(),
        valid: true,
        problems: Vec::new(),
        active: false,
    };
    let standard = FontSet::standard();
    let figures = standard.fonts.numbers.clone();
    let mut tabular = FontSet {
        id: "tabular".into(),
        about: sample_about("Tabular", "1.0.0", "Inter with tabular figures"),
        ..FontSet::standard()
    };
    (tabular.fonts.text, tabular.fonts.heading) = (figures.clone(), figures);
    tabular.kind = None;
    let installed = |id: &str, family: &str, name: &str| FontInfo {
        id: id.into(),
        family: Some(family.into()),
        name: name.into(),
        version: String::new(),
        author: None,
        description: None,
        license: None,
        origin: torro_ipc::FontOrigin::System,
        kind: None,
        standard: false,
        valid: true,
        problems: Vec::new(),
        active: false,
    };
    let system = [
        installed("system", "Helvetica Neue", "System"),
        installed("system:Avenir Next", "Avenir Next", "Avenir Next"),
        installed("system:Monaco", "Monaco", "Monaco"),
    ];
    let brush = FontInfo {
        id: "brush".into(),
        family: None,
        name: "Brush".into(),
        version: "0.2.0".into(),
        author: Some("Torro samples".into()),
        description: None,
        license: None,
        origin: torro_ipc::FontOrigin::Folder,
        kind: None,
        standard: false,
        valid: false,
        problems: [
            DesignError::MissingFile { key: "fonts.text.regular".into(), path: "Brush-Regular.ttf".into() },
            DesignError::FontLicense,
        ]
        .iter()
        .map(ToString::to_string)
        .collect(),
        active: false,
    };
    let mut fonts = vec![standard, tabular];
    let forced = torro_gui::font::active();
    if !fonts.iter().any(|font| font.id == forced.id) {
        fonts.push((*forced).clone());
    }
    let folder = |mut info: FontInfo| {
        if !info.standard && info.id == "tabular" {
            info.origin = torro_ipc::FontOrigin::Folder;
        }
        info
    };
    let mut entries: Vec<FontEntry> =
        fonts.into_iter().map(|font| Entry::new(folder(info(&font)), Some(font))).collect();
    entries.push(Entry::new(brush, None));
    entries.extend(system.into_iter().map(|info| Entry::new(info, None)));
    entries
}

/// The colour schemes of [`demo_looks`].
fn demo_schemes() -> Vec<SchemeEntry> {
    use torro_core::design::{ColorScheme, DesignError, Mode, Rgba};
    let info = |scheme: &ColorScheme| ColorSchemeInfo {
        id: scheme.id.clone(),
        name: scheme.about.name.clone(),
        version: scheme.about.version.clone(),
        author: scheme.about.author.clone(),
        description: scheme.about.description.clone(),
        standard: scheme.is_standard(),
        valid: true,
        problems: Vec::new(),
        active: false,
        dark_only: scheme.dark_only,
    };
    let sample = |id: &str, about| ColorScheme { id: id.into(), about, ..ColorScheme::standard() };
    let mut harbor = sample("harbor", sample_about("Harbor", "1.2.0", "Teal accent, cool surfaces"));
    let dark = &mut harbor.colors.dark;
    (dark.bg, dark.panel, dark.raised) = (Rgba::hex(0x0A0E10), Rgba::hex(0x101518), Rgba::hex(0x151B1F));
    (dark.accent, dark.on_accent) = (Rgba::hex(0x14B8A6), Rgba::hex(0x042F2C));
    let light = &mut harbor.colors.light;
    (light.bg, light.panel) = (Rgba::hex(0xEEF3F4), Rgba::hex(0xFFFFFF));
    (light.accent, light.on_accent) = (Rgba::hex(0x0B7A72), Rgba::hex(0xFFFFFF));
    let mut paper = sample("paper", sample_about("Paper", "0.9.1", "Warm paper, ink blue"));
    let dark = &mut paper.colors.dark;
    (dark.bg, dark.panel, dark.raised) = (Rgba::hex(0x12100D), Rgba::hex(0x1A1714), Rgba::hex(0x201C18));
    (dark.accent, dark.on_accent) = (Rgba::hex(0x7C9CF5), Rgba::hex(0x0B1530));
    let light = &mut paper.colors.light;
    (light.bg, light.panel, light.raised) = (Rgba::hex(0xF3EEE4), Rgba::hex(0xFFFCF6), Rgba::hex(0xF8F4EC));
    (light.accent, light.on_accent) = (Rgba::hex(0x2A4FC4), Rgba::hex(0xFFFFFF));
    let neon = ColorSchemeInfo {
        id: "neon".into(),
        name: "Neon".into(),
        version: "0.3.0".into(),
        author: Some("Torro samples".into()),
        description: None,
        standard: false,
        valid: false,
        problems: [
            DesignError::TextContrast { fg: "colors.dark.text_3".into(), on: "panel".into(), ratio: 3.12 },
            DesignError::UnknownKey { key: "colors.dark.acent".into() },
            DesignError::AccentContrast { mode: Mode::Light, ratio: 2.4 },
        ]
        .iter()
        .map(ToString::to_string)
        .collect(),
        active: false,
        dark_only: false,
    };
    // Dark only: the light mode is a copy of the dark one, as the core reads such a scheme.
    let mut midnight = sample("midnight", sample_about("Midnight", "1.0.0", "Deep blue, dark only"));
    let dark = &mut midnight.colors.dark;
    (dark.bg, dark.panel, dark.raised) = (Rgba::hex(0x070B14), Rgba::hex(0x0C1220), Rgba::hex(0x111A2C));
    (dark.accent, dark.on_accent) = (Rgba::hex(0x6E8BFF), Rgba::hex(0x0A1030));
    midnight.colors.light = midnight.colors.dark;
    midnight.dark_only = true;
    let mut schemes = vec![ColorScheme::standard(), harbor, paper, midnight];
    let forced = torro_gui::scheme::active();
    if !schemes.iter().any(|scheme| scheme.id == forced.id) {
        schemes.push((*forced).clone());
    }
    let mut entries: Vec<SchemeEntry> =
        schemes.into_iter().map(|scheme| Entry::new(info(&scheme), Some(scheme))).collect();
    entries.push(Entry::new(neon, None));
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What the control centre's pages had as their data: here only the settings.
    #[derive(Default)]
    struct Data {
        settings: Settings,
    }

    /// The key and value the page asked the shell to set.
    fn sent(action: Action) -> (SettingKey, serde_json::Value) {
        let Action::Send(Request::SetSetting { key, value }) = action else {
            panic!("expected SetSetting, got {action:?}");
        };
        (key, value)
    }

    #[test]
    fn each_language_choice_sets_only_the_language() {
        let cases = [(Language::System, "system"), (Language::English, "english"), (Language::German, "german")];
        for (language, name) in cases {
            let action = update(&mut State::default(), Message::Language(language), &Settings::default());
            assert_eq!(sent(action), (SettingKey::Language, serde_json::json!(name)), "choice {language:?}");
        }
    }

    #[test]
    fn each_appearance_choice_sets_only_the_appearance() {
        let cases = [(Appearance::System, "system"), (Appearance::Light, "light"), (Appearance::Dark, "dark")];
        for (appearance, name) in cases {
            let action = update(&mut State::default(), Message::Appearance(appearance), &Settings::default());
            assert_eq!(sent(action), (SettingKey::Appearance, serde_json::json!(name)), "choice {appearance:?}");
        }
    }

    #[test]
    fn start_at_login_sets_only_the_switch() {
        let on = update(&mut State::default(), Message::StartAtLogin(true), &Settings::default());
        assert_eq!(sent(on), (SettingKey::StartAtLogin, serde_json::json!(true)));
        let off = update(&mut State::default(), Message::StartAtLogin(false), &Settings::default());
        assert_eq!(sent(off), (SettingKey::StartAtLogin, serde_json::json!(false)));
    }

    #[test]
    fn the_zoom_buttons_zoom_the_window() {
        for step in [Step::In, Step::Out, Step::Reset] {
            let action = update(&mut State::default(), Message::Zoom(step), &Settings::default());
            assert!(matches!(action, Action::Zoom(sent) if sent == step), "{step:?}: {action:?}");
        }
    }

    /// Tasks L.18 and L.23: a choice of each part draws with it, "Reload" lists all three again and
    /// each heading opens its own folder – each without a question first. Designs have no folder
    /// (issue #484).
    #[test]
    fn each_choice_draws_with_its_part_and_the_headings_list_and_open() {
        for part in [Part::Design, Part::Font, Part::ColorScheme] {
            let choose = update(&mut State::default(), Message::Choose(part, "harbor".into()), &Settings::default());
            assert!(matches!(choose, Action::Choose(p, ref id) if p == part && id == "harbor"), "{choose:?}");
        }
        for part in [Part::Font, Part::ColorScheme] {
            let open = update(&mut State::default(), Message::OpenFolder(part), &Settings::default());
            assert!(matches!(open, Action::OpenFolder(p) if p == part), "{open:?}");
        }
        let reload = update(&mut State::default(), Message::Reload, &Settings::default());
        assert!(matches!(reload, Action::ListLooks), "{reload:?}");
        assert_eq!(folder(Part::Font), torro_core::design::font_dir());
        assert_eq!(folder(Part::ColorScheme), torro_core::design::color_scheme_dir());
        assert_eq!(folder(Part::Design), None, "no designs folder");
        let choose = choose_request(Part::Design, "dense");
        assert_eq!(choose, Request::SetSetting { key: SettingKey::Design, value: serde_json::json!("dense") });
    }

    /// Task L.18: the colour mode's segment sets the appearance, one segment per mode in the order
    /// System, Light, Dark, with the monitor, the sun and the moon.
    #[test]
    fn each_segment_of_the_colour_mode_sets_its_mode() {
        let symbols = modes().map(|(which, _, mode)| (which, mode));
        assert_eq!(
            symbols,
            [(Icon::Monitor, Appearance::System), (Icon::Sun, Appearance::Light), (Icon::Moon, Appearance::Dark)]
        );
        for (_, _, mode) in modes() {
            let action = update(&mut State::default(), Message::Appearance(mode), &Settings::default());
            assert_eq!(sent(action).0, SettingKey::Appearance, "{mode:?}");
        }
        assert!(mode_words_need() < size::content_max(), "a wide page shows the words");
        let state = demo();
        for appearance in [Appearance::System, Appearance::Light, Appearance::Dark] {
            let settings = Settings { appearance, ..Settings::default() };
            drop(colors_group(&state, &settings, size::content_max()));
            drop(colors_group(&state, &settings, size::props()));
            drop(display(&settings));
        }
    }

    /// The demo's lists, as the page keeps them.
    fn demo() -> State {
        let mut state = State::default();
        listed(&mut state, demo_looks());
        state
    }

    /// Tasks L.18, L.23 and L.37: a click on an entry that can't be used shows its reasons, a second
    /// click hides them, a click on another one swaps them – also to another part's, the same id in
    /// another list being another entry; choosing anything closes them. None of it asks the shell.
    #[test]
    fn an_unusable_entry_opens_and_closes_its_reasons() {
        let data = Data::default();
        let mut state = demo();
        let open = |state: &State, part: Part| match part {
            Part::Design => {
                ok(&state.designs).and_then(|e| opened(e, part, state.problems.as_ref())).map(|e| e.info.id.clone())
            }
            Part::Font => {
                ok(&state.fonts).and_then(|e| opened(e, part, state.problems.as_ref())).map(|e| e.info.id.clone())
            }
            Part::ColorScheme => {
                ok(&state.schemes).and_then(|e| opened(e, part, state.problems.as_ref())).map(|e| e.info.id.clone())
            }
        };
        assert_eq!(open(&state, Part::ColorScheme), None);

        let action = update(&mut state, Message::Problems(Part::ColorScheme, "neon".into()), &data.settings);
        assert!(matches!(action, Action::None), "{action:?}");
        assert_eq!(open(&state, Part::ColorScheme).as_deref(), Some("neon"));
        assert_eq!(open(&state, Part::Font), None, "only the colour scheme");
        let _: Element<'_, Message> = appearance(&state, &data.settings);

        let _ = update(&mut state, Message::Problems(Part::ColorScheme, "neon".into()), &data.settings);
        assert_eq!(state.problems, None, "a second click hides them");

        let _ = update(&mut state, Message::Problems(Part::ColorScheme, "neon".into()), &data.settings);
        assert_eq!(open(&state, Part::ColorScheme).as_deref(), Some("neon"));
        assert_eq!(open(&state, Part::Design), None, "every spacing level can be used");
        let _ = update(&mut state, Message::Problems(Part::Font, "brush".into()), &data.settings);
        assert_eq!(open(&state, Part::Font).as_deref(), Some("brush"), "another tile swaps them");
        let _: Element<'_, Message> = appearance(&state, &data.settings);
        let _ = update(&mut state, Message::Problems(Part::Font, "tabular".into()), &data.settings);
        assert_eq!(open(&state, Part::Font), None, "nor does an entry that can be used");

        let _ = update(&mut state, Message::Problems(Part::Font, "brush".into()), &data.settings);
        let _ = update(&mut state, Message::Choose(Part::ColorScheme, "harbor".into()), &data.settings);
        assert_eq!(state.problems, None, "choosing closes them");
    }

    /// Task L.23: designs and colour schemes say version and author, fonts the pack, its version,
    /// author and licence; a font is named by its family, one that can't be used by its pack.
    #[test]
    fn the_tooltip_of_an_entry_says_who_made_it() {
        assert_eq!(byline([Some("1.2.0"), Some("Torro samples")]).as_deref(), Some("1.2.0 · Torro samples"));
        assert_eq!(byline([Some(""), None]), None);
        let state = demo();
        let fonts = ok(&state.fonts).expect("fonts");
        let tabular = &fonts.iter().find(|e| e.info.id == "tabular").expect("tabular").info;
        assert_eq!(tabular.name(), "Inter Tabular", "named by its family");
        let tip = byline([
            Some(tabular.name.as_str()),
            Some(tabular.version.as_str()),
            tabular.author.as_deref(),
            tabular.license.as_deref(),
        ]);
        assert_eq!(tip.as_deref(), Some("Tabular · 1.0.0 · Torro samples · OFL-1.1"));
        let brush = &fonts.iter().find(|e| e.info.id == "brush").expect("brush").info;
        assert_eq!(brush.name(), "Brush", "no family: its pack's name");
    }

    /// The check mark of each part is at the one the windows draw with.
    #[test]
    fn the_check_mark_is_at_the_part_the_windows_draw_with() {
        let state = demo();
        let designs = ok(&state.designs).expect("designs");
        assert_eq!(drawn(designs, "dense"), "dense");
        assert_eq!(drawn(designs, "torro"), "torro");
        assert_eq!(drawn(designs, "console"), "torro", "an old design is no level: the standard is drawn");
        let fonts = ok(&state.fonts).expect("fonts");
        assert_eq!(drawn(fonts, "tabular"), "tabular");
        assert_eq!(drawn(fonts, ""), "torro", "a core from before task L.22 names none");
        assert_eq!(drawn(ok(&state.schemes).expect("schemes"), "neon"), "torro");
    }

    /// Issue #291 (review of the three designs): the zoom's value has room for every level with
    /// `xs` to spare – the estimate of the widest is rounded up –, so "100 %" (German, with a space)
    /// does not break into two lines; the value never wraps anyway.
    #[test]
    fn the_zoom_value_has_room_for_every_level() {
        for step in Zoom::STEPS {
            let level = Zoom::nearest(f64::from(step) / 100.0);
            let label = zoom::label(level);
            assert!(typo::text_width(&label, typo::body().tabular()) + space::xs() <= zoom_value_width(), "{label}");
        }
        drop(zoom_control(Zoom::DEFAULT));
    }

    /// Issue #484: choosing a spacing level asks for the level alone and leaves no hint behind; the
    /// segment shows Comfort, Standard and Dense in that order, named as the core names them.
    #[test]
    fn choosing_a_spacing_level_sets_only_the_level() {
        let data = Data::default();
        let mut state = demo();
        let action = update(&mut state, Message::Choose(Part::Design, "dense".into()), &data.settings);
        assert!(matches!(action, Action::Choose(Part::Design, ref id) if id == "dense"), "{action:?}");
        let entries = ok(&state.designs).expect("designs");
        let order: Vec<&str> = level_order(entries).iter().map(|entry| entry.info.id.as_str()).collect();
        assert_eq!(order, ["comfort", "torro", "dense"], "from the most room to the least");
        let names: Vec<&str> = level_order(entries).iter().map(|entry| entry.info.name.as_str()).collect();
        assert_eq!(names, [tr("Comfort"), tr("Standard"), tr("Dense")]);
        for design in ["torro", "comfort", "dense"] {
            let settings = Settings { design: design.into(), ..Settings::default() };
            for width in [size::content_max(), size::props()] {
                drop(spacing(&state, &settings, width));
            }
        }
    }

    /// Task L.37: the colour schemes' dropdown has the sections "Torro", "Light and dark" and "Dark
    /// only" in this order, each scheme in its own, and leaves out a section without one.
    #[test]
    fn the_scheme_dropdown_groups_its_entries() {
        let state = demo();
        let schemes = ok(&state.schemes).expect("schemes");
        let ids = |groups: Vec<(SchemeGroup, Vec<&SchemeEntry>)>| -> Vec<(SchemeGroup, Vec<String>)> {
            groups
                .into_iter()
                .map(|(group, members)| (group, members.iter().map(|e| e.info.id.clone()).collect()))
                .collect()
        };
        let strings = |list: &[&str]| list.iter().map(|id| id.to_string()).collect::<Vec<_>>();
        assert_eq!(
            ids(scheme_groups(schemes)),
            vec![
                (SchemeGroup::Torro, strings(&["torro"])),
                (SchemeGroup::LightAndDark, strings(&["harbor", "paper", "neon"])),
                (SchemeGroup::DarkOnly, strings(&["midnight"])),
            ]
        );
        assert_eq!(SchemeGroup::Torro.title(), "Torro", "a name");
        assert_eq!(SchemeGroup::DarkOnly.title(), tr("Dark only"));
        let without: Vec<SchemeEntry> = schemes.iter().filter(|e| !e.info.dark_only).cloned().collect();
        let groups: Vec<SchemeGroup> = scheme_groups(&without).into_iter().map(|(group, _)| group).collect();
        assert_eq!(groups, [SchemeGroup::Torro, SchemeGroup::LightAndDark], "no empty section");
    }

    /// [`font_sections`] as ids.
    type FontIds = Vec<(FontSection, Vec<(Option<FontGroup>, Vec<String>)>)>;

    /// The ids of [`font_sections`], by section and group; the demo's fonts only (a test elsewhere may
    /// add the font the process draws with).
    fn font_ids(entries: &[FontEntry], query: &str) -> FontIds {
        let known = ["torro", "tabular", "brush", "system", "system:Avenir Next", "system:Monaco"];
        font_sections(entries, query)
            .into_iter()
            .map(|(section, groups)| {
                let groups = groups
                    .into_iter()
                    .map(|(group, members)| {
                        let ids = members.iter().map(|e| e.info.id.clone()).filter(|id| known.contains(&id.as_str()));
                        (group, ids.collect())
                    })
                    .collect();
                (section, groups)
            })
            .collect()
    }

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|id| id.to_string()).collect()
    }

    /// Issue #299: "Torro fonts" first with the groups by kind (the standard first in its group, then
    /// by name), then "System fonts" with "System" first; a group or a section without a font is left
    /// out.
    #[test]
    fn the_font_dropdown_groups_the_suites_fonts_and_the_computers() {
        let state = demo();
        let fonts = ok(&state.fonts).expect("fonts");
        assert_eq!(
            font_ids(fonts, ""),
            vec![
                (
                    FontSection::Torro,
                    vec![
                        (Some(FontGroup::Sans), strings(&["torro"])),
                        (Some(FontGroup::Other), strings(&["brush", "tabular"]))
                    ]
                ),
                (FontSection::System, vec![(None, strings(&["system", "system:Avenir Next", "system:Monaco"]))]),
            ]
        );
        let system = fonts.iter().find(|entry| entry.info.id == SYSTEM_ID).expect("System");
        assert_eq!(font_name(&system.info), tr("System"), "not the family it stands for");
        assert_eq!(FontSection::System.title(), tr("System fonts"));
        assert_eq!(FontGroup::Other.title(), tr("Other"));
    }

    /// Issue #299: the search filters by name and family without regard to case, and titles without a
    /// match go; Enter takes the first match that can be used.
    #[test]
    fn the_font_search_filters_and_enter_takes_the_first_match() {
        let data = Data::default();
        let mut state = demo();
        let fonts = ok(&state.fonts).expect("fonts");
        assert_eq!(font_ids(fonts, "mo"), vec![(FontSection::System, vec![(None, strings(&["system:Monaco"]))])]);
        assert_eq!(
            font_ids(fonts, " INTER "),
            vec![(
                FontSection::Torro,
                vec![(Some(FontGroup::Sans), strings(&["torro"])), (Some(FontGroup::Other), strings(&["tabular"]))]
            )],
            "the family counts, Inter Tabular too"
        );
        assert_eq!(font_ids(fonts, "helvetica"), vec![(FontSection::System, vec![(None, strings(&["system"]))])]);
        assert!(font_ids(fonts, "zzz").is_empty(), "nothing: the menu says so");

        let _ = update(&mut state, Message::ToggleFonts, &data.settings);
        let _ = update(&mut state, Message::FontQuery("ave".into()), &data.settings);
        let chosen = update(&mut state, Message::FirstFont, &data.settings);
        assert!(matches!(chosen, Action::Choose(Part::Font, ref id) if id == "system:Avenir Next"), "{chosen:?}");
        assert!(!state.fonts_open, "choosing closes it");
        let _ = update(&mut state, Message::ToggleFonts, &data.settings);
        assert_eq!(state.font_query, "", "it opens empty");
        let _ = update(&mut state, Message::FontQuery("brush".into()), &data.settings);
        let nothing = update(&mut state, Message::FirstFont, &data.settings);
        assert!(matches!(nothing, Action::None), "a broken font is never taken: {nothing:?}");
        assert!(state.fonts_open);
    }

    /// Issue #299: the font's button opens its dropdown with the focus in the search field; Escape, a
    /// click next to it and leaving the page close it; only one of the two dropdowns is open; a broken
    /// font's click shows its reasons under the group.
    #[test]
    fn the_font_dropdown_opens_closes_and_shows_reasons() {
        let data = Data::default();
        let mut state = demo();
        let opened_it = update(&mut state, Message::ToggleFonts, &data.settings);
        assert!(matches!(&opened_it, Action::Focus(id) if *id == FONT_SEARCH), "{opened_it:?}");
        assert!(overlay(&state, &data.settings).is_some());
        assert!(matches!(escape(&state), Some(Message::CloseFonts)));
        let _ = update(&mut state, Message::CloseFonts, &data.settings);
        assert!(overlay(&state, &data.settings).is_none() && escape(&state).is_none());

        let _ = update(&mut state, Message::ToggleFonts, &data.settings);
        let _ = update(&mut state, Message::ToggleSchemes, &data.settings);
        assert!(state.schemes_open && !state.fonts_open, "one dropdown at a time");
        let _ = update(&mut state, Message::ToggleFonts, &data.settings);
        assert!(state.fonts_open && !state.schemes_open);
        let closed = update(&mut state, Message::ToggleFonts, &data.settings);
        assert!(matches!(closed, Action::None) && !state.fonts_open, "a second click closes it");

        let _ = update(&mut state, Message::ToggleFonts, &data.settings);
        let _ = update(&mut state, Message::Problems(Part::Font, "brush".into()), &data.settings);
        assert!(!state.fonts_open, "the reasons show under the group");
        let fonts = ok(&state.fonts).expect("fonts");
        let shown = opened(fonts, Part::Font, state.problems.as_ref()).map(|entry| entry.info.id.as_str());
        assert_eq!(shown, Some("brush"));
        let _ = update(&mut state, Message::ToggleFonts, &data.settings);
        leave(&mut state);
        assert!(overlay(&state, &data.settings).is_none(), "leaving the page closes it");
    }

    /// Task L.37: the colour scheme's button opens and closes its dropdown, a layer over the window;
    /// a click next to it, Escape, choosing a scheme, opening the reasons of a broken one and leaving
    /// the page close it. Choosing a scheme or a font goes to the shell, which saves it with
    /// `set_setting` (`Ui::choose`).
    #[test]
    fn the_scheme_dropdown_opens_and_closes() {
        let data = Data::default();
        let mut state = demo();
        assert!(overlay(&state, &data.settings).is_none() && escape(&state).is_none());
        let action = update(&mut state, Message::ToggleSchemes, &data.settings);
        assert!(matches!(action, Action::None), "{action:?}");
        assert!(overlay(&state, &data.settings).is_some());
        assert!(matches!(escape(&state), Some(Message::CloseSchemes)));
        let _ = update(&mut state, Message::CloseSchemes, &data.settings);
        assert!(overlay(&state, &data.settings).is_none());

        let _ = update(&mut state, Message::ToggleSchemes, &data.settings);
        let choose = update(&mut state, Message::Choose(Part::ColorScheme, "midnight".into()), &data.settings);
        assert!(matches!(choose, Action::Choose(Part::ColorScheme, ref id) if id == "midnight"), "{choose:?}");
        assert!(!state.schemes_open, "choosing closes it");

        let _ = update(&mut state, Message::ToggleSchemes, &data.settings);
        let _ = update(&mut state, Message::Problems(Part::ColorScheme, "neon".into()), &data.settings);
        assert!(!state.schemes_open, "the reasons show under the group");
        let _ = update(&mut state, Message::ToggleSchemes, &data.settings);
        leave(&mut state);
        assert!(overlay(&state, &data.settings).is_none(), "leaving the page closes it");
        assert!(overlay(&State::default(), &Settings::default()).is_none(), "nothing listed: nothing to open");
    }

    /// Task L.37: a design's density comes from the core (`DesignInfo::density`) as dots and a word,
    /// one more dot per step.
    /// Issue #484: the demo lists the three levels with their density, the core's order.
    #[test]
    fn the_demo_lists_the_three_levels() {
        use torro_core::design::DensityLevel;
        let state = demo();
        let designs = ok(&state.designs).expect("designs");
        let listed: Vec<(&str, DensityLevel)> = designs.iter().map(|e| (e.info.id.as_str(), e.info.density)).collect();
        assert_eq!(
            listed,
            [("torro", DensityLevel::Balanced), ("comfort", DensityLevel::Airy), ("dense", DensityLevel::Dense)]
        );
    }

    /// Task L.37, issue #484: the small mail window is drawn from the level's own values, scaled from
    /// the window to the preview: the denser the level, the lower its rows and header and the narrower
    /// its rail; the widths of sidebar and list are the same in every level.
    #[test]
    fn the_mail_preview_is_drawn_from_the_design() {
        let state = demo();
        let of = |id: &str| design_of(&state, id).expect(id);
        let width = MAIL_WINDOW.width / 10.0;
        let [comfort, standard, dense] = [of("comfort"), of("torro"), of("dense")].map(|d| Regions::of(&d, width));
        assert_eq!(standard.scale, 0.1);
        assert_eq!((standard.sidebar, standard.header, standard.row), (24.0, 6.4, 10.0));
        assert!(comfort.row > standard.row && standard.row > dense.row);
        assert!(comfort.rail > standard.rail && standard.rail > dense.rail);
        assert!(standard.header > dense.header);
        assert_eq!((comfort.sidebar, comfort.list), (dense.sidebar, dense.list), "widths stay");
        for design in ["comfort", "torro", "dense"] {
            drop(preview(of(design), size::content_max()));
        }
    }

    #[test]
    fn listed_parts_are_kept_with_what_they_draw_with() {
        let mut state = State::default();
        assert!(unlisted(&state));
        listed(&mut state, demo_looks());
        assert!(!unlisted(&state));
        assert!(design_of(&state, "dense").is_some(), "an entry that can be used carries its part");
        assert!(font_of(&state, "tabular").is_some());
        assert!(scheme_of(&state, "harbor").is_some());
        assert!(design_of(&state, "console").is_none(), "an old design is not listed");
        assert!(font_of(&state, "brush").is_none());
        assert!(scheme_of(&state, "gone").is_none());
        listed(&mut state, Lists::failed("core is not running".into()));
        assert!(design_of(&state, "dense").is_none());
        assert!(font_of(&state, "tabular").is_none());
    }

    #[test]
    fn page_builds_while_loading_listed_and_failed() {
        let (loading, listing, mut failed) = (State::default(), demo(), State::default());
        listed(&mut failed, Lists::failed("core is not running".into()));
        // One list failed, the others listed.
        let mut partly = demo();
        partly.fonts = Some(Err("no fonts".into()));
        for state in [&loading, &listing, &failed, &partly] {
            let plain = Settings::default();
            let _page: Element<'_, Message> = appearance(state, &plain);
        }
        let marked = Data { settings: Settings { design: "dense".into(), ..Settings::default() } };
        let _page: Element<'_, Message> = appearance(&listing, &marked.settings);
        // The page itself is laid out for its width; build it wide and narrow, in every state.
        for state in [&loading, &listing, &failed, &partly] {
            for width in [size::content_max(), size::props()] {
                drop(page(state, &marked.settings, width));
                drop(spacing(state, &marked.settings, width));
                drop(font_group(state, &marked.settings));
            }
        }
    }

    #[test]
    fn the_folder_is_shown_from_home() {
        if let Some(home) = std::env::home_dir() {
            let inside = home.join("fonts");
            assert_eq!(home_relative(&inside), format!("~{}fonts", std::path::MAIN_SEPARATOR));
        }
        let outside = std::path::Path::new("/opt/torro/fonts");
        assert_eq!(home_relative(outside), outside.display().to_string());
    }
}
