//! The settings pages General and Appearance (tasks S.9 and S.14 in `docs/tasks/steuerzentrale.md`):
//! settings groups, each with a footer sentence that explains the consequence.
//!
//! - **General** ([`view`]): "Language", a list of choices – one row each, the whole row is the
//!   button, a check mark at the chosen one (Deutsch is shown in its own name, like the language
//!   picker of any app); "Apps" ([`apps`], task S.21) has every part of the suite with its switch,
//!   its window class under the name (for window rules) and "Open", the App Feed among them;
//!   "Startup" has the "Start at login" switch; "Setup" runs the first-run setup again.
//! - **Appearance** ([`appearance`], its own page in the sidebar since S.14, so it is found at a
//!   glance): "Appearance" (System, Light, Dark) as a list of choices like the language; "Design"
//!   lists the design packs (task L.4, below); "Interface" has the zoom of the suite with −, the
//!   value, + and "Reset" (task Z.1).
//!
//! What a single app or window shows and how it behaves is set in that window, behind its gear
//! (task S.21): the calendar's night and reminders, how mail is read, the sources of the
//! Notification Hub. Only what holds for the whole suite is left here.
//!
//! Both pages share one state and one message type. There is no Save button: every change sends
//! `Request::SetSetting` with the one key that changed, and the shell reloads `Data` afterwards.
//! The zoom goes through the window's own zoom instead (`Action::Zoom`), which saves it and
//! applies it at once, also in the demo. The page shows `data.settings`, so it follows the daemon.
//!
//! # Design (task L.4, concept `docs/design-pakete.md`)
//!
//! One row per pack as the core lists them (`settings.list_designs`, asked by the shell when the
//! page opens and on "Reload"): a sample – the pack's `preview.png`, or a swatch of its `bg`,
//! `panel`, `accent` and `text` with "Aa" in its text font, in the window's light or dark mode –
//! then name, version and author, and a check mark at the one the windows draw with. A pack that
//! breaks a rule is greyed out and says "Can't be used:" with every reason. Choosing one is
//! [`Action::Design`]: the shell draws its own window with it at once and saves it
//! (`settings.set_setting design`); no question first. The last row opens the designs folder
//! (created if it is missing) and lists the packs again.
//!
//! The list comes from the core; the sample's colours, font and picture come from the pack's
//! folder, read here the way every window reads the design it draws with
//! (`torro_core::design::load`): [`Pack::new`].

use std::sync::Arc;

use iced::widget::{column, container, row, space as blank, svg, text};
use iced::{Alignment, Background, Border, ContentFit, Element, Fill, Padding, Theme};
use torro_core::AppId;
use torro_core::design::{Design, FontRole, Mode, STANDARD_ID, Weight};
use torro_core::i18n::{tr, trf};
use torro_core::settings::{Component, ComponentSwitch, Language, Zoom};
use torro_core::theme::Appearance;
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{Palette, palette, radius, size, space};
use torro_gui::zoom::{self, Step};
use torro_gui::{layout, style, typo, widgets};
use torro_ipc::{DesignInfo, Request, SettingKey};
use torro_settings_ui::parts::{choice, switch};

use super::setup::component_name;
use super::{Action, Data, Section, one_line, pressable, row_frame, setting_row};

#[derive(Debug, Default)]
pub struct State {
    /// The design packs as the core listed them last, the standard first; `Err` when it did not
    /// answer, `None` until the first answer.
    designs: Option<Result<Vec<Pack>, String>>,
}

/// One design pack of the list: what the core says about it and what its row shows of its look.
#[derive(Debug, Clone)]
pub struct Pack {
    pub info: DesignInfo,
    /// The pack merged with the standard, as read from its folder here: the colours and the font
    /// of the swatch, and what the window draws with when it is chosen. `None` for a pack that
    /// can't be used (or whose folder changed since the core listed it).
    pub design: Option<Arc<Design>>,
    /// `preview.png` as an SVG image: Iced here draws SVG but not PNG (no `image` feature), and
    /// its SVG renderer takes a PNG embedded as a `data:` address. Built once per listing, so Iced
    /// rasterises it once.
    preview: Option<svg::Handle>,
}

impl Pack {
    /// The row of `info`, with `design` read from the pack's folder (`None` when it can't be
    /// used). Reads the pack's `preview.png` if it has one (checked by `design::load`: a PNG of at
    /// most 2 MB and 1200 × 800); a picture that can't be read gives the swatch.
    pub fn new(info: DesignInfo, design: Option<Design>) -> Pack {
        let preview = design.as_ref().and_then(|d| d.preview.as_ref()).and_then(|path| std::fs::read(path).ok());
        Pack { info, preview: preview.as_deref().and_then(png_as_svg), design: design.map(Arc::new) }
    }
}

/// Stores the packs the core listed (`settings.list_designs`), or why it could not list them.
pub fn listed(state: &mut State, designs: Result<Vec<Pack>, String>) {
    state.designs = Some(designs);
}

/// Whether the packs have not been listed yet, so the shell asks the core when the page opens.
pub fn unlisted(state: &State) -> bool {
    state.designs.is_none()
}

/// The pack `id` as read for its row, for a window that cannot read it from the designs folder
/// (`--demo`, whose packs live in memory).
pub fn design_of(state: &State, id: &str) -> Option<Arc<Design>> {
    let packs = state.designs.as_ref()?.as_ref().ok()?;
    packs.iter().find(|pack| pack.info.id == id)?.design.clone()
}

#[derive(Debug, Clone)]
pub enum Message {
    Language(Language),
    Appearance(Appearance),
    StartAtLogin(bool),
    /// −, + or "Reset" of the zoom.
    Zoom(Step),
    /// A part of the suite was switched on or off under Apps (task W.2).
    Component(Component, bool),
    /// "Open" of a part of the suite: its window, by window class and nothing else (task S.21).
    /// The window class alone, deliberately – a target such as `settings:<section>` has no place
    /// here, because a window's settings are opened by a person with its gear ([`apps`]).
    OpenApp(String),
    /// "Open" of Workspaces, which has no window of its own: the Workspaces page.
    OpenWorkspaces,
    /// A design pack's row: draw every window with this pack (its id).
    Design(String),
    /// "Reload": list the packs again, after one was added to or changed in the folder.
    ReloadDesigns,
    /// "Open designs folder".
    OpenDesignsFolder,
    /// "Run setup again" (task W.9).
    RunSetup,
}

pub fn update(_state: &mut State, message: Message, _data: &Data) -> Action {
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
        Message::Component(component, enabled) => {
            (SettingKey::Component, serde_json::to_value(ComponentSwitch { component, enabled }).unwrap_or_default())
        }
        Message::OpenApp(app) => return Action::Send(Request::OpenApp { app, target: None }),
        Message::OpenWorkspaces => return Action::Select(Section::Workspaces),
        // The shell draws its own window with it at once, then saves it.
        Message::Design(id) => return Action::Design(id),
        Message::ReloadDesigns => return Action::ListDesigns,
        Message::OpenDesignsFolder => return Action::OpenDesignsFolder,
        Message::RunSetup => return Action::OpenSetup,
    };
    Action::Send(Request::SetSetting { key, value })
}

/// The page General: language, calendar, startup.
pub fn view<'a>(_state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let settings = &data.settings;
    let language = |label: &'static str, language: Language| {
        choice(label, settings.language == language, Message::Language(language))
    };
    let language = widgets::group(
        tr("Language"),
        vec![
            language(tr("System"), Language::System),
            language("English", Language::English),
            // Shown in its own name on purpose: someone who cannot read the current language
            // must still find their way back.
            language("Deutsch", Language::German),
        ],
        Some(tr("Applies to apps you open afterwards. “System” follows your system language.").to_string()),
    );

    let startup = widgets::group(
        tr("Startup"),
        vec![setting_row(None, tr("Start at login"), None, switch(settings.start_at_login, Message::StartAtLogin))],
        Some(
            tr("Starts Torro Workplace in the background when you log in, so the tray icon and your workspaces are ready.")
                .to_string(),
        ),
    );

    let run = widgets::text_button(None, tr("Run setup again"), style::button::secondary, Some(Message::RunSetup));
    let setup = widgets::group(
        tr("Setup"),
        vec![setting_row(
            None,
            tr("First-run setup"),
            Some(tr("Nextcloud, components, accounts and AI, step by step").to_string()),
            run,
        )],
        Some(tr("What is connected stays; the steps show it.").to_string()),
    );

    column![language, apps(data), startup, setup].spacing(layout::section_gap()).into()
}

/// The group "Apps" (task S.21): one row per part of the suite with its switch – a part turned off
/// leaves the app switcher and the tray – and "Open", which brings its window to the front or
/// starts it (`apps.open_app` with the window class, so the inbox's three windows are told apart).
/// "Open" is off while the core has not reported the app as installed. Workspaces has no window of
/// its own: its "Open" shows the Workspaces page, where the templates are.
///
/// Under the name stands the window's class, quietly ([`app_detail`]): that is what a window rule
/// in Hyprland needs, and the control centre is the one place it is written down.
///
/// The App Feed ([`AppId::INBOX_NOTIFICATIONS`]) gets a row of its own after the Notification Hub,
/// with its class and "Open" but no switch: it is a second window of the same part of the suite,
/// not a part of its own. Nothing else in the control centre opens it.
///
/// What a window shows and how it behaves is set in that window, behind its gear; this page only
/// says so and leads there. No "Open" can name a section of the settings (`apps.open_app` with a
/// target `settings:<section>`), because [`Message::OpenApp`] carries nothing but the window class
/// and [`update`] fills in `target: None`: there is no place to put one. That is on purpose – the
/// settings of a window are opened by a person with its gear, not by a button elsewhere. Giving
/// this page such a way means widening the message first, which is where the decision belongs.
fn apps(data: &Data) -> Element<'_, Message> {
    let mut rows: Vec<Element<'_, Message>> = Vec::new();
    for component in Component::ALL {
        let app = window_of(component);
        let open = match app {
            Some(app) => installed(data, app).then(|| Message::OpenApp(app.window_class.to_string())),
            None => Some(Message::OpenWorkspaces),
        };
        // A button without a message is drawn disabled, so a missing app cannot be opened.
        let button = widgets::text_button(None, tr("Open"), style::button::secondary, open);
        let controls = row![
            button,
            switch(data.settings.component_on(component), move |on| { Message::Component(component, on) })
        ]
        .spacing(space::l())
        .align_y(Alignment::Center);
        rows.push(setting_row(None, component_name(component), app_detail(data, app), controls));
        if component == Component::Inbox {
            rows.push(second_window(data, AppId::INBOX_NOTIFICATIONS, tr("App Feed")));
        }
    }
    let footer = format!(
        "{} {}",
        tr("Its settings are behind the gear in its window."),
        tr(
            "Turned off, a part of the suite leaves the app switcher and the tray, and nothing runs for it in the background. Accounts and data stay."
        ),
    );
    widgets::group(tr("Apps"), rows, Some(footer))
}

/// A second window of a part of the suite (the App Feed of the Notification Hub): its name, its
/// class and "Open", without a switch – what switches it is the part's own row above.
fn second_window<'a>(data: &'a Data, app: AppId, label: &'a str) -> Element<'a, Message> {
    let open = installed(data, app).then(|| Message::OpenApp(app.window_class.to_string()));
    let button = widgets::text_button(None, tr("Open"), style::button::secondary, open);
    // The switch's place stays empty, so every "Open" of the group stands in the same column.
    let controls = row![button, blank().width(switch_width())].spacing(space::l()).align_y(Alignment::Center);
    setting_row(None, label, app_detail(data, Some(app)), controls)
}

/// How wide Iced draws a switch of the suite: twice its height, which is what a row leaves for it.
fn switch_width() -> f32 {
    style::CHECK_SIZE + style::CHECK_SIZE
}

/// The line under an app's name: its window class, for window rules, and "Not installed" while the
/// core does not report it. `None` for a part of the suite without a window of its own
/// (Workspaces), which has neither.
fn app_detail(data: &Data, app: Option<AppId>) -> Option<String> {
    let app = app?;
    let class = trf("Window class: {class}", &[("class", &app.window_class)]);
    match installed(data, app) {
        true => Some(class),
        false => Some(format!("{class} · {}", tr("Not installed"))),
    }
}

/// The window a part of the suite opens: mail is the inbox's mail window, the Notification Hub its
/// hub (task E8.1, each with its own class). Workspaces has none – it is the service itself.
fn window_of(component: Component) -> Option<AppId> {
    match component {
        Component::Calendar => Some(AppId::CALENDAR),
        Component::Tasks => Some(AppId::TASKS),
        Component::Mail => Some(AppId::INBOX_MAIL),
        Component::Inbox => Some(AppId::INBOX),
        Component::Assistant => Some(AppId::ASSISTANT),
        Component::Workspaces => None,
    }
}

/// Whether the core reported the window `app` as installed, so "Open" can start it; `false` until
/// the first answer (`apps.list_apps`).
fn installed(data: &Data, app: AppId) -> bool {
    data.apps
        .as_deref()
        .unwrap_or_default()
        .iter()
        .any(|listed| listed.window_class == app.window_class && listed.installed)
}

/// The page Appearance: light or dark, the design pack, the zoom, how mail shows.
pub fn appearance<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let settings = &data.settings;
    // The control centre itself follows at once: it reloads the settings after every change.
    let appearance = |label: &'static str, appearance: Appearance| {
        choice(label, settings.appearance == appearance, Message::Appearance(appearance))
    };
    let appearance = widgets::group(
        tr("Color mode"),
        vec![
            appearance(tr("System"), Appearance::System),
            appearance(tr("Light"), Appearance::Light),
            appearance(tr("Dark"), Appearance::Dark),
        ],
        Some(tr("Light or dark colors for all Torro apps. “System” follows your desktop.").to_string()),
    );

    let interface = widgets::group(
        tr("Interface"),
        vec![setting_row(None, tr("Zoom"), None, zoom_control(settings.zoom))],
        Some(
            tr("Makes all Torro apps larger or smaller, like zooming in a browser: Ctrl + Plus and Ctrl + Minus, Ctrl + 0 resets.")
                .to_string(),
        ),
    );

    let design = designs(state, data);
    column![appearance, design, interface].spacing(layout::section_gap()).into()
}

/// The group "Design": a row per pack, then the row of the designs folder.
fn designs<'a>(state: &'a State, data: &'a Data) -> Element<'a, Message> {
    let mut rows: Vec<Element<'a, Message>> = match &state.designs {
        // Quiet until the core answers, which takes a moment at most.
        None => vec![row_frame(typo::text(typo::body(), tr("Loading …")).style(torro_gui::theme::text_3), false)],
        Some(Err(problem)) => {
            let alert = icon::icon(Icon::CircleAlert, icon::SIZE)
                .style(|t: &Theme, _| svg::Style { color: Some(palette(t).error_text) });
            vec![setting_row(
                Some(alert.into()),
                tr("The designs can't be listed."),
                Some(problem.clone()),
                None::<Element<'a, Message>>,
            )]
        }
        Some(Ok(packs)) => {
            let drawn = drawn(packs, &data.settings.design);
            packs.iter().map(|pack| pack_row(pack, pack.info.id == drawn)).collect()
        }
    };
    rows.push(folder_row());
    widgets::group(
        tr("Design"),
        rows,
        Some(
            tr("Colors, fonts and spacing of all Torro apps. More designs go into the designs folder; one that breaks a rule can't be used.")
                .to_string(),
        ),
    )
}

/// The id of the pack the windows draw with: the chosen one (`Settings::design`) if it can be
/// used, else the standard, which the suite falls back to (concept: "Wählen und wechseln").
fn drawn<'a>(packs: &'a [Pack], chosen: &'a str) -> &'a str {
    if packs.iter().any(|pack| pack.info.id == chosen && pack.info.valid) { chosen } else { STANDARD_ID }
}

/// One pack: the sample, its name, "version · author", and the check mark at the one drawn with.
/// A pack that can't be used is a disabled row – its name greyed out – that says why, in full.
fn pack_row<'a>(pack: &'a Pack, drawn: bool) -> Element<'a, Message> {
    let info = &pack.info;
    let byline = [Some(info.version.as_str()), info.author.as_deref()]
        .into_iter()
        .flatten()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ");
    if !info.valid {
        let mut lines = column![one_line(typo::text(typo::body(), info.name.as_str()))];
        if !byline.is_empty() {
            lines = lines.push(one_line(typo::text(typo::meta(), byline)));
        }
        // The reasons stay readable at full strength and wrap: they say what to fix, one per line,
        // so a pack with many can be worked through like a list.
        lines = lines.push(typo::text(typo::meta(), tr("Can't be used:")).style(torro_gui::theme::text_2));
        for reason in &info.problems {
            lines = lines.push(typo::text(typo::meta(), reason.as_str()).style(torro_gui::theme::text_2));
        }
        let content = row![unusable(), lines.width(Fill)].spacing(space::m()).align_y(Alignment::Center);
        let frame = container(content).width(Fill).padding([space::m(), space::l()]);
        return pressable(frame.into(), None);
    }
    let check: Option<Element<'a, Message>> = drawn.then(|| {
        icon::icon(Icon::Check, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text) }).into()
    });
    let detail = (!byline.is_empty()).then_some(byline);
    let row = setting_row(Some(sample(pack)), info.name.as_str(), detail, check);
    pressable(row, Some(Message::Design(info.id.clone())))
}

/// The width of a pack's sample: 3 : 2 like the largest `preview.png` (1200 × 800), at the height
/// of a large control.
fn sample_width() -> f32 {
    size::control_lg() * 3.0 / 2.0
}

/// A pack's sample: its `preview.png`, or a swatch – the pack's `bg` as the window with its
/// `panel` set in (like the suite's frame), on it "Aa" in the pack's text font and `text` colour
/// and a dot in its `accent` – in the mode the window is in.
fn sample<'a>(pack: &Pack) -> Element<'a, Message> {
    let frame = |content: Element<'a, Message>, ground: Option<Arc<Design>>| {
        container(content).width(sample_width()).height(size::control_lg()).clip(true).style(move |t: &Theme| {
            container::Style {
                background: ground.as_ref().map(|d| Background::Color(colors(d, t).bg)),
                border: Border { color: palette(t).line_strong, width: size::HAIRLINE, radius: radius::xs().into() },
                ..container::Style::default()
            }
        })
    };
    let Some(design) = pack.design.clone() else {
        return frame(blank().into(), None).into();
    };
    if let Some(preview) = &pack.preview {
        return frame(svg(preview.clone()).content_fit(ContentFit::Cover).width(Fill).height(Fill).into(), None).into();
    }
    let font = torro_gui::design::font_of(&design, FontRole::Text, Weight::Regular);
    let ink = Arc::clone(&design);
    let aa = typo::text(typo::body(), "Aa")
        .font(font)
        .style(move |t: &Theme| text::Style { color: Some(colors(&ink, t).text) });
    let tint = Arc::clone(&design);
    let dot = container(blank().width(space::s()).height(space::s())).style(move |t: &Theme| container::Style {
        background: Some(Background::Color(colors(&tint, t).accent)),
        border: Border { radius: (space::s() / 2.0).into(), ..Border::default() },
        ..container::Style::default()
    });
    let ground = Arc::clone(&design);
    let panel = container(row![aa, blank::horizontal(), dot].align_y(Alignment::Center))
        .padding([0.0, space::xs()])
        .width(Fill)
        .height(Fill)
        .center_y(Fill)
        .style(move |t: &Theme| container::Style {
            background: Some(Background::Color(colors(&ground, t).panel)),
            border: Border { radius: radius::xs().into(), ..Border::default() },
            ..container::Style::default()
        });
    // The sidebar's strip at the left, the panel set in by the rest: the suite's frame in small.
    let inset = Padding { top: space::xs(), right: space::xs(), bottom: space::xs(), left: space::s() };
    frame(container(panel).padding(inset).into(), Some(design)).into()
}

/// The sample place of a pack that can't be used: an empty frame with an alert mark, quiet.
fn unusable<'a>() -> Element<'a, Message> {
    let mark =
        icon::icon(Icon::CircleAlert, icon::SIZE).style(|t: &Theme, _| svg::Style { color: Some(palette(t).text_3) });
    container(mark)
        .center_x(sample_width())
        .center_y(size::control_lg())
        .style(|t: &Theme| container::Style {
            border: Border { color: palette(t).line_strong, width: size::HAIRLINE, radius: radius::xs().into() },
            ..container::Style::default()
        })
        .into()
}

/// The colours of `design` in the mode of `theme` (the window's light or dark).
fn colors(design: &Design, theme: &Theme) -> Palette {
    let mode = if theme.extended_palette().is_dark { Mode::Dark } else { Mode::Light };
    Palette::of(design, mode)
}

/// The last row: where packs go, "Reload" and "Open designs folder".
fn folder_row<'a>() -> Element<'a, Message> {
    let path = torro_core::design::dir().map(|dir| home_relative(&dir));
    let controls = row![
        widgets::icon_button(Icon::RefreshCw, tr("Reload"), Some(Message::ReloadDesigns)),
        widgets::text_button(
            Some(Icon::Folder),
            tr("Open designs folder"),
            style::button::secondary,
            Some(Message::OpenDesignsFolder)
        ),
    ]
    .spacing(space::xs())
    .align_y(Alignment::Center);
    setting_row(None, tr("Designs folder"), path, controls)
}

/// `path` with the home directory written as `~`, so the part that matters fits the row.
fn home_relative(path: &std::path::Path) -> String {
    match std::env::home_dir().and_then(|home| path.strip_prefix(home).ok().map(std::path::Path::to_path_buf)) {
        Some(rest) => format!("~{}{}", std::path::MAIN_SEPARATOR, rest.display()),
        None => path.display().to_string(),
    }
}

/// A PNG as an SVG image for Iced, or `None` when it is not a PNG ([`png_svg`]).
fn png_as_svg(png: &[u8]) -> Option<svg::Handle> {
    png_svg(png).map(|svg| svg::Handle::from_memory(svg.into_bytes()))
}

/// The text of an SVG that shows `png` at its own size, embedded as a `data:` address (the one
/// kind of reference the SVG renderer resolves without touching the disk). `None` when it is not a
/// PNG: no signature, or no `IHDR` chunk with the size at its place.
fn png_svg(png: &[u8]) -> Option<String> {
    const SIGNATURE: &[u8] = b"\x89PNG\r\n\x1a\n";
    if !png.starts_with(SIGNATURE) || png.get(12..16)? != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(png.get(16..20)?.try_into().ok()?);
    let height = u32::from_be_bytes(png.get(20..24)?.try_into().ok()?);
    Some(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\"><image width=\"{width}\" height=\"{height}\" href=\"data:image/png;base64,{}\"/></svg>",
        base64(png)
    ))
}

/// Standard base64 with padding (RFC 4648), for the `data:` address of a preview.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |n, (i, byte)| n | (u32::from(*byte) << (16 - 8 * i)));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// −, the level, + and "Reset". The level keeps the width of the widest one ("200%"), so the
/// buttons stay where they are; a button that would change nothing is off.
fn zoom_control<'a>(level: Zoom) -> Element<'a, Message> {
    let press = |step: Step| (step.apply(level) != level).then_some(Message::Zoom(step));
    let width = typo::text_width(&zoom::label(Zoom::MAX), typo::body().tabular());
    let value = container(typo::text(typo::body().tabular(), zoom::label(level))).center_x(width);
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

#[cfg(test)]
mod tests {
    use super::*;
    use torro_core::settings::Settings;

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
            let action = update(&mut State::default(), Message::Language(language), &Data::default());
            assert_eq!(sent(action), (SettingKey::Language, serde_json::json!(name)), "choice {language:?}");
        }
    }

    #[test]
    fn each_appearance_choice_sets_only_the_appearance() {
        let cases = [(Appearance::System, "system"), (Appearance::Light, "light"), (Appearance::Dark, "dark")];
        for (appearance, name) in cases {
            let action = update(&mut State::default(), Message::Appearance(appearance), &Data::default());
            assert_eq!(sent(action), (SettingKey::Appearance, serde_json::json!(name)), "choice {appearance:?}");
        }
    }

    #[test]
    fn start_at_login_sets_only_the_switch() {
        let on = update(&mut State::default(), Message::StartAtLogin(true), &Data::default());
        assert_eq!(sent(on), (SettingKey::StartAtLogin, serde_json::json!(true)));
        let off = update(&mut State::default(), Message::StartAtLogin(false), &Data::default());
        assert_eq!(sent(off), (SettingKey::StartAtLogin, serde_json::json!(false)));
    }

    #[test]
    fn the_zoom_buttons_zoom_the_window() {
        for step in [Step::In, Step::Out, Step::Reset] {
            let action = update(&mut State::default(), Message::Zoom(step), &Data::default());
            assert!(matches!(action, Action::Zoom(sent) if sent == step), "{step:?}: {action:?}");
        }
    }

    #[test]
    fn a_design_row_draws_with_it_and_the_folder_row_lists_and_opens() {
        let choose = update(&mut State::default(), Message::Design("harbor".into()), &Data::default());
        assert!(matches!(choose, Action::Design(ref id) if id == "harbor"), "{choose:?}: no question first");
        let reload = update(&mut State::default(), Message::ReloadDesigns, &Data::default());
        assert!(matches!(reload, Action::ListDesigns), "{reload:?}");
        let open = update(&mut State::default(), Message::OpenDesignsFolder, &Data::default());
        assert!(matches!(open, Action::OpenDesignsFolder), "{open:?}");
    }

    #[test]
    fn the_check_mark_is_at_the_pack_the_windows_draw_with() {
        let packs = super::super::launch::demo_designs();
        assert_eq!(drawn(&packs, "harbor"), "harbor");
        assert_eq!(drawn(&packs, "torro"), "torro");
        assert_eq!(drawn(&packs, "neon"), "torro", "a broken pack is not drawn: the standard is");
        assert_eq!(drawn(&packs, "gone"), "torro", "nor a missing one");
    }

    #[test]
    fn listed_packs_are_kept_with_their_looks() {
        let mut state = State::default();
        assert!(unlisted(&state));
        listed(&mut state, Ok(super::super::launch::demo_designs()));
        assert!(!unlisted(&state));
        assert!(design_of(&state, "paper").is_some(), "a pack that can be used carries its look");
        assert!(design_of(&state, "neon").is_none(), "one that can't, none");
        assert!(design_of(&state, "gone").is_none());
        listed(&mut state, Err("core is not running".into()));
        assert!(design_of(&state, "paper").is_none());
    }

    #[test]
    fn page_builds_while_loading_listed_and_failed() {
        let data = Data::default();
        let (loading, mut packs, mut failed) = (State::default(), State::default(), State::default());
        listed(&mut packs, Ok(super::super::launch::demo_designs()));
        listed(&mut failed, Err("core is not running".into()));
        for state in [&loading, &packs, &failed] {
            let _page: Element<'_, Message> = view(state, &data);
        }
    }

    #[test]
    fn base64_matches_the_rfc_4648_vectors() {
        let cases = [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg==")];
        for (plain, coded) in cases.into_iter().chain([("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")]) {
            assert_eq!(base64(plain.as_bytes()), coded, "{plain:?}");
        }
    }

    /// A pack's `preview.png` is drawn through the SVG renderer: the SVG built around it must show
    /// the picture's pixels at its size.
    #[test]
    fn a_preview_png_is_drawn_as_an_svg_image() {
        use resvg::tiny_skia::{Color, Pixmap};
        let mut picture = Pixmap::new(6, 4).expect("pixmap");
        picture.fill(Color::from_rgba8(0x14, 0xB8, 0xA6, 0xFF));
        let png = picture.encode_png().expect("png");
        let svg = png_svg(&png).expect("a PNG");
        assert!(svg.contains("width=\"6\" height=\"4\""), "{svg}");

        let tree = resvg::usvg::Tree::from_str(&svg, &resvg::usvg::Options::default()).expect("svg");
        let mut drawn = Pixmap::new(6, 4).expect("pixmap");
        resvg::render(&tree, resvg::tiny_skia::Transform::default(), &mut drawn.as_mut());
        let pixel = drawn.pixel(3, 2).expect("pixel");
        assert_eq!((pixel.red(), pixel.green(), pixel.blue(), pixel.alpha()), (0x14, 0xB8, 0xA6, 0xFF));

        assert!(png_svg(b"GIF89a").is_none(), "not a PNG");
        assert!(png_svg(&png[..20]).is_none(), "cut short");
        assert!(png_as_svg(&png).is_some());
    }

    #[test]
    fn the_folder_is_shown_from_home() {
        if let Some(home) = std::env::home_dir() {
            let inside = home.join("designs");
            assert_eq!(home_relative(&inside), format!("~{}designs", std::path::MAIN_SEPARATOR));
        }
        let outside = std::path::Path::new("/opt/torro/designs");
        assert_eq!(home_relative(outside), outside.display().to_string());
    }

    /// Task S.21: General › Apps switches a part of the suite and opens its window by window
    /// class; Workspaces has no window and leads to its page instead. "Open" stays off while the
    /// core has not reported an app as installed.
    #[test]
    fn the_apps_group_switches_a_part_and_opens_its_window() {
        let data = Data::default();
        let set = |component, on| sent(update(&mut State::default(), Message::Component(component, on), &data));
        assert_eq!(
            set(Component::Calendar, false),
            (SettingKey::Component, serde_json::json!({"component": "calendar", "enabled": false}))
        );
        assert_eq!(
            set(Component::Mail, true),
            (SettingKey::Component, serde_json::json!({"component": "mail", "enabled": true}))
        );
        // The inbox's windows share a binary: mail is opened by its own class (task E8.1).
        assert_eq!(window_of(Component::Mail).map(|app| app.window_class), Some("torro-inbox-mail"));
        assert_eq!(window_of(Component::Inbox).map(|app| app.window_class), Some("torro-inbox"));
        assert_eq!(window_of(Component::Workspaces), None, "no window of its own");
        let open = update(&mut State::default(), Message::OpenApp("torro-inbox-mail".into()), &data);
        assert!(matches!(open, Action::Send(Request::OpenApp { ref app, target: None }) if app == "torro-inbox-mail"));
        let workspaces = update(&mut State::default(), Message::OpenWorkspaces, &data);
        assert!(matches!(workspaces, Action::Select(Section::Workspaces)), "{workspaces:?}");

        assert!(!installed(&data, AppId::CALENDAR), "no answer yet");
        let listed = |class: &str, installed: bool| torro_ipc::AppInfo {
            binary: "torroinbox".into(),
            name: "Torro".into(),
            window_class: class.into(),
            installed,
        };
        let apps = Data { apps: Some(vec![listed("torro-inbox-mail", true), listed("torro-inbox", false)]), ..data };
        assert!(installed(&apps, AppId::INBOX_MAIL));
        assert!(!installed(&apps, AppId::INBOX));
        drop(view(&State::default(), &apps));
    }

    /// Found in the review of task S.22: the window class belongs on the row (window rules in
    /// Hyprland need it and the control centre is the one place it is written down), and the App
    /// Feed must be reachable – it is a window of the Notification Hub, not a part of the suite,
    /// so it gets no switch of its own.
    #[test]
    fn every_app_row_names_its_window_class_and_the_app_feed_can_be_opened() {
        let listed = |class: &str, installed: bool| torro_ipc::AppInfo {
            binary: "torroinbox".into(),
            name: "Torro".into(),
            window_class: class.into(),
            installed,
        };
        let class_of = |app: AppId| trf("Window class: {class}", &[("class", &app.window_class)]);

        // Every window of the suite is in the list; only Workspaces has none.
        let all: Vec<torro_ipc::AppInfo> =
            AppId::ALL.iter().map(|app| listed(app.window_class, true)).collect::<Vec<_>>();
        let data = Data { apps: Some(all), ..Data::default() };
        for component in Component::ALL {
            let app = window_of(component);
            assert_eq!(app_detail(&data, app), app.map(class_of), "{component:?}");
        }
        assert_eq!(window_of(Component::Workspaces), None);

        // Not installed: the class stays, the state follows it.
        let none = Data::default();
        assert_eq!(
            app_detail(&none, Some(AppId::CALENDAR)),
            Some(format!("{} · {}", class_of(AppId::CALENDAR), tr("Not installed")))
        );

        // The App Feed opens by its own class, and only while the core reports it.
        assert_eq!(AppId::INBOX_NOTIFICATIONS.window_class, "torro-inbox-notifications");
        let feed =
            update(&mut State::default(), Message::OpenApp(AppId::INBOX_NOTIFICATIONS.window_class.into()), &data);
        assert!(
            matches!(feed, Action::Send(Request::OpenApp { ref app, target: None })
                if app == "torro-inbox-notifications"),
            "{feed:?}"
        );
        drop(view(&State::default(), &data));
        drop(second_window(&none, AppId::INBOX_NOTIFICATIONS, tr("App Feed")));
    }

    #[test]
    fn page_builds_with_default_and_stored_settings() {
        let mut settings = Settings {
            language: Language::German,
            appearance: Appearance::Dark,
            start_at_login: false,
            design: "plex".into(),
            ..Settings::default()
        };
        settings.zoom = torro_core::settings::Zoom::MAX;
        settings.components_off = [Component::Mail].into_iter().collect();
        let stored = Data { settings, ..Data::default() };
        let (state, default) = (State::default(), Data::default());
        let _default: Element<'_, Message> = view(&state, &default);
        let _stored: Element<'_, Message> = view(&state, &stored);
        let _default: Element<'_, Message> = appearance(&state, &default);
        let _stored: Element<'_, Message> = appearance(&state, &stored);
    }
}
