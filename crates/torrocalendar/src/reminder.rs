//! The reminder over the whole screen (task ER2.1, `docs/erinnerungen.md`, screens in
//! `docs/prototypes/calendar/erinnerung.html`).
//!
//! `torrocalendar --reminders` is started by the core when a reminder in the style "full screen"
//! fires (`crates/torroworkplace/src/reminders.rs`). It reads `calendar.list_reminders
//! { due_only: true }` every two seconds and closes itself once nothing is due any more; the core
//! keeps what the person did (snooze, dismiss). The window is a separate process of the calendar
//! with the window class `torro-reminder`, borderless and over every other window.
//!
//! - **Layout** ([`Screen`]): one column up to `size::content_max()` (normal), two columns from
//!   1600 wide (the facts beside the lead, also on a 32:9 screen: centred, not stretched), and a
//!   small one below 1280 × 760 (the facts as chips under the title, notes behind "Show notes").
//! - **Content**: calendar and kind of event, the title in `typo::hero()`, the countdown in
//!   `typo::countdown()` ("Starts in 4:32", after the start "Started 2 min ago" in the attention
//!   colour), the time, the buttons, the place, links and notes, and the next events. Several
//!   reminders at once are paged with ‹ › (← →) and dismissed together with "Dismiss all".
//! - **Buttons and keys**: Join (Enter, red, only with a call link; joining dismisses the reminder),
//!   else Dismiss is the main button; Snooze (S, then 1 / 5 / 0 / B for 1, 5, 10 minutes and until
//!   it starts), Dismiss (Esc). Keys count only from [`KEY_LOCK`] after the window opened, so
//!   someone typing elsewhere does not dismiss it by accident.
//! - **Every screen** (task ER2.2): one process, one window per screen (with `--all-displays`, the
//!   setting `allDisplays`). The main screen shows all of it, the others a calm version (calendar,
//!   title, countdown, Join / In 5 min / Dismiss); a click in any of them acts on all. On macOS each
//!   window is laid over its screen by `torro-appkit` (`torro_gui::cover_screen`), opened hidden so
//!   nothing flashes; on Hyprland the core registers a rule per monitor first and names them with
//!   `--monitors`, the windows' titles (`Reminder · <monitor>`) match the rules; on Windows the
//!   window is full screen on the main screen.
//! - **Demo** (`--demo --open reminder…`, with `--size WIDTHxHEIGHT`): sample reminders of the demo
//!   calendar at a fixed time, for the visual check; nothing is sent.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Local, TimeDelta, Utc};
use iced::theme::Mode;
use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, responsive, row};
use iced::{Alignment, Color, Element, Fill, Size, Subscription, Task, Theme};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_gui::datetime::{format_long_date, format_time, to_local};
use torro_gui::icon::{self, Icon};
use torro_gui::source::Source;
use torro_gui::theme::{self, Appearance, palette, size, space};
use torro_gui::widgets::{self, Align, Anchor, MenuItem, MenuSection, Placement, Trail, collection_color};
use torro_gui::{links, style, typo};
use torro_ipc::{Join, Reminder, ReminderState, Request, Response, Snooze};
use torro_pim::demo::DemoSource;
use torro_pim::{AlarmStyle, Collection, Event, EventTime, PimSource};

/// How often the window asks the core what is due.
const POLL: Duration = Duration::from_secs(2);

/// Keys do nothing this long after the window opened (`docs/erinnerungen.md`, "Bedienung").
pub const KEY_LOCK: Duration = Duration::from_millis(600);

/// How far ahead the footer looks for the next events.
fn next_ahead() -> TimeDelta {
    TimeDelta::hours(12)
}

/// Failed asks in a row after which the window gives up (the core is gone).
const GIVE_UP_AFTER: u32 = 5;

/// The name of the binary in the agent app "Torro Reminder.app" (macOS, task ER2.2): the calendar's
/// binary under this name starts as the reminder window.
pub const BINARY: &str = "torro-reminder";

/// The window class (Linux app id) of the reminder window.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub const WINDOW_CLASS: &str = "torro-reminder";

/// The sample states of `--demo --open …`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Demo {
    /// A video call in 4:32.
    Reminder,
    /// The same call, started two minutes ago.
    Started,
    /// Two reminders at once.
    Many,
    /// The notes open.
    Notes,
    /// The call in light colours.
    Light,
    /// A preview an agent sent.
    Preview,
    /// An appointment on site, without a call.
    Onsite,
    /// The snooze menu open.
    Snooze,
    /// The calm version of another screen.
    Second,
}

impl Demo {
    /// The names `--open` takes.
    pub const ALL: &[(&'static str, Demo)] = &[
        ("reminder", Demo::Reminder),
        ("reminder-started", Demo::Started),
        ("reminder-many", Demo::Many),
        ("reminder-notes", Demo::Notes),
        ("reminder-light", Demo::Light),
        ("reminder-preview", Demo::Preview),
        ("reminder-onsite", Demo::Onsite),
        ("reminder-snooze", Demo::Snooze),
        ("reminder-second", Demo::Second),
    ];

    /// The state `--open` names, if it is one of the reminder window's.
    pub fn named(name: &str) -> Option<Demo> {
        Demo::ALL.iter().find(|(known, _)| *known == name).map(|(_, demo)| *demo)
    }
}

/// How the reminder window starts.
#[derive(Debug, Clone, PartialEq)]
pub struct Launch {
    /// A sample state instead of the core.
    pub demo: Option<Demo>,
    /// The window's size in a demo (`--size 1920x1080`); full screen otherwise.
    pub size: Option<Size>,
    pub appearance: Option<Appearance>,
    pub design: Option<String>,
    /// `--font` and `--color-scheme` (task L.22), like `--design` passed on unchecked: the core starts
    /// this window, and a part it cannot read is the standard's.
    pub font: Option<String>,
    pub color_scheme: Option<String>,
    /// A window on every screen, not only the main one (`--all-displays`, from the setting).
    pub all_displays: bool,
    /// The monitors by name, main first, as the window manager names them (`--monitors DP-1,HDMI-A-1`;
    /// Hyprland, whose rules the core registered for them).
    pub monitors: Vec<String>,
    /// In a demo: cover the screen as the real window does (`--cover`), to check the platform's way.
    pub cover: bool,
}

impl Launch {
    /// `--design`, `--font` and `--color-scheme` for `torro_gui::design::start_for` (task L.22).
    pub fn forced(&self) -> torro_gui::design::Forced {
        let (design, font, color_scheme) = (self.design.clone(), self.font.clone(), self.color_scheme.clone());
        torro_gui::design::Forced { design, font, color_scheme }
    }

    /// The window starts in reminder mode: `--reminders`, or a reminder state of `--open`.
    pub fn wanted(args: &[String]) -> bool {
        args.iter().any(|arg| arg == "--reminders")
            || args.windows(2).any(|pair| pair[0] == "--open" && Demo::named(&pair[1]).is_some())
            || args.iter().any(|arg| arg.strip_prefix("--open=").is_some_and(|name| Demo::named(name).is_some()))
    }

    /// The reminder window's parameters, or why they are refused.
    pub fn parse(args: &[String]) -> Result<Launch, String> {
        let value = |flag: &str| -> Option<String> {
            let prefix = format!("{flag}=");
            args.iter()
                .position(|arg| arg == flag)
                .and_then(|i| args.get(i + 1).cloned())
                .or_else(|| args.iter().find_map(|arg| arg.strip_prefix(&prefix).map(str::to_string)))
        };
        let demo_flag = args.iter().any(|arg| arg == "--demo");
        let demo = match value("--open") {
            None if demo_flag => Some(Demo::Reminder),
            None => None,
            Some(name) => match Demo::named(&name) {
                Some(_) if !demo_flag => return Err("--open only works together with --demo".into()),
                Some(demo) => Some(demo),
                None => {
                    let valid: Vec<&str> = Demo::ALL.iter().map(|(name, _)| *name).collect();
                    return Err(format!("unknown value {name:?} for --open; valid values: {}", valid.join(", ")));
                }
            },
        };
        let size = match value("--size") {
            None => None,
            Some(_) if demo.is_none() => return Err("--size only works together with --demo".into()),
            Some(text) => Some(parse_size(&text).ok_or_else(|| format!("--size expects WIDTHxHEIGHT, got {text:?}"))?),
        };
        let appearance = match value("--appearance").as_deref() {
            None => None,
            Some("light") => Some(Appearance::Light),
            Some("dark") => Some(Appearance::Dark),
            Some("system") => Some(Appearance::System),
            Some(other) => {
                return Err(format!("unknown value {other:?} for --appearance; valid values: light, dark, system"));
            }
        };
        let appearance = appearance.or((demo == Some(Demo::Light)).then_some(Appearance::Light));
        let all_displays = args.iter().any(|arg| arg == "--all-displays");
        let monitors = value("--monitors")
            .map(|names| names.split(',').map(str::trim).filter(|name| !name.is_empty()).map(str::to_string).collect())
            .unwrap_or_default();
        let cover = demo.is_some() && args.iter().any(|arg| arg == "--cover");
        Ok(Launch {
            demo,
            size,
            appearance,
            design: value("--design"),
            font: value("--font"),
            color_scheme: value("--color-scheme"),
            all_displays,
            monitors,
            cover,
        })
    }
}

/// `1920x1080` as a size.
fn parse_size(text: &str) -> Option<Size> {
    let (width, height) = text.split_once('x')?;
    let (width, height) = (width.trim().parse::<f32>().ok()?, height.trim().parse::<f32>().ok()?);
    (width >= 200.0 && height >= 120.0).then(|| Size::new(width, height))
}

/// One screen the window shows on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Display {
    /// 0 for the main screen, else its place in the system's order.
    pub index: usize,
    /// The monitor's name, when the window manager gave one (Hyprland).
    pub monitor: Option<String>,
}

impl Display {
    /// The main screen shows everything, the others the calm version.
    pub fn is_main(&self) -> bool {
        self.index == 0
    }
}

/// The screens to show on: the demo's one, the named monitors, every screen with `all_displays`
/// (`screens` of them), else the main one.
pub fn displays(launch: &Launch, screens: usize) -> Vec<Display> {
    if launch.demo.is_some() && !launch.cover {
        return vec![Display { index: usize::from(launch.demo == Some(Demo::Second)), monitor: None }];
    }
    if !launch.monitors.is_empty() {
        let shown = if launch.all_displays { launch.monitors.len() } else { 1 };
        return launch
            .monitors
            .iter()
            .take(shown)
            .enumerate()
            .map(|(index, name)| Display { index, monitor: Some(name.clone()) })
            .collect();
    }
    let shown = if launch.all_displays { screens.max(1) } else { 1 };
    (0..shown).map(|index| Display { index, monitor: None }).collect()
}

/// The title of the window on `display`: the rules of a window manager find it by it, so it is
/// the same in every language (`Reminder · DP-1`).
pub fn window_title(display: &Display) -> String {
    match &display.monitor {
        Some(monitor) => format!("Reminder · {monitor}"),
        None => "Reminder".to_string(),
    }
}

/// The settings of the window on `display`: borderless over every window, or the demo's size.
pub fn window(launch: &Launch, display: &Display) -> iced::window::Settings {
    let mut settings = torro_gui::window(AppId::CALENDAR);
    #[cfg(target_os = "linux")]
    {
        settings.platform_specific.application_id = WINDOW_CLASS.to_string();
    }
    settings.decorations = false;
    if let Some(size) = launch.size.filter(|_| !launch.cover) {
        settings.size = size;
        return settings;
    }
    let _ = display;
    settings.level = iced::window::Level::AlwaysOnTop;
    if cfg!(target_os = "macos") {
        // macOS "full screen" is a space of its own and moves the person there (task ER0.1): the
        // window opens hidden and `torro-appkit` lays it over its screen.
        settings.visible = false;
    } else {
        // Hyprland's rule puts it on its monitor; Windows covers the main screen.
        settings.fullscreen = true;
    }
    #[cfg(target_os = "windows")]
    {
        settings.platform_specific.skip_taskbar = true;
    }
    settings
}

/// What the window shows at a time.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Snapshot {
    /// The reminders that are due, soonest first.
    pub reminders: Vec<Reminder>,
    /// The events of the next hours, for "Next".
    pub next: Vec<Event>,
    /// The calendars, for names and colours.
    pub calendars: Vec<Collection>,
}

/// Where the reminders come from.
#[derive(Clone)]
enum Feed {
    /// The core at this socket.
    Core(PathBuf),
    /// The demo's sample state.
    Demo(Arc<DemoSource>, Demo),
}

/// The layout of the screen ([`Screen::of`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    /// Below 1280 × 760: the facts as chips.
    Small,
    /// One column.
    Normal,
    /// From 1600 wide: lead and facts side by side.
    Wide,
}

impl Screen {
    /// The layout for a window of `size` (logical pixels, after the zoom).
    pub fn of(size: Size) -> Screen {
        if size.width < 1280.0 || size.height < 760.0 {
            Screen::Small
        } else if size.width >= 1600.0 {
            Screen::Wide
        } else {
            Screen::Normal
        }
    }
}

/// The countdown of a reminder at `now` ([`countdown`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Countdown {
    /// The event starts in this long, as "4:32" or "1:05:00".
    Starts(String),
    /// The event started this many whole minutes ago (0 in its first minute).
    Started(i64),
}

/// How long until an event starting at `start` begins, seen at `now`.
pub fn countdown(start: DateTime<Utc>, now: DateTime<Utc>) -> Countdown {
    if now >= start {
        return Countdown::Started((now - start).num_minutes());
    }
    let seconds = (start - now).num_seconds().max(0);
    let (hours, minutes, seconds) = (seconds / 3600, seconds % 3600 / 60, seconds % 60);
    Countdown::Starts(match hours {
        0 => format!("{minutes}:{seconds:02}"),
        _ => format!("{hours}:{minutes:02}:{seconds:02}"),
    })
}

/// A key the window answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pressed {
    Enter,
    Escape,
    Left,
    Right,
    Char(String),
}

#[derive(Debug, Clone)]
pub enum Message {
    /// The clock, every second.
    Tick(DateTime<Utc>),
    /// Time to ask the core again.
    Poll,
    Loaded(Result<Snapshot, String>),
    Resized(Size),
    /// A window opened; on macOS it is now laid over its screen.
    Opened(iced::window::Id),
    SystemMode(Mode),
    Appearance(Result<Appearance, String>),
    Key(Pressed),
    Join,
    ToggleSnooze,
    Snooze(Snooze),
    Dismiss,
    DismissAll,
    Previous,
    Next,
    OpenCalendar,
    OpenMaps,
    OpenLink(String),
    ToggleNotes,
    PauseHour,
    /// The core answered a command.
    Sent(Result<(), String>),
}

/// The reminder window.
pub struct App {
    feed: Feed,
    launch: Launch,
    snapshot: Snapshot,
    /// The reminder shown, an index into `snapshot.reminders`.
    selected: usize,
    snooze_open: bool,
    notes_open: bool,
    opened: Instant,
    now: DateTime<Utc>,
    size: Size,
    system_mode: Mode,
    appearance: Appearance,
    /// Something was due at least once: an empty answer after that closes the window.
    loaded: bool,
    failures: u32,
    snooze_anchor: Anchor,
    /// The open windows and the screen each shows.
    windows: Vec<(iced::window::Id, Display)>,
}

impl App {
    pub fn new(source: Source, launch: Launch) -> (App, Task<Message>) {
        let feed = match (&source, launch.demo) {
            (Source::Demo(demo), Some(case)) => Feed::Demo(Arc::clone(demo), case),
            (Source::Demo(demo), None) => Feed::Demo(Arc::clone(demo), Demo::Reminder),
            (Source::Core(client), _) => Feed::Core(client.socket.clone()),
        };
        let now = Utc::now();
        let snooze_open = launch.demo == Some(Demo::Snooze);
        let app = App {
            notes_open: launch.demo == Some(Demo::Notes),
            size: launch.size.unwrap_or(Size::new(1920.0, 1080.0)),
            feed,
            launch,
            snapshot: Snapshot::default(),
            selected: 0,
            snooze_open,
            opened: Instant::now(),
            now,
            system_mode: Mode::None,
            appearance: Appearance::System,
            loaded: false,
            failures: 0,
            snooze_anchor: Anchor::new(),
            windows: Vec::new(),
        };
        let mut app = app;
        let mut opening = Vec::new();
        for display in displays(&app.launch, torro_gui::screen_count()) {
            let (id, opened) = iced::window::open(window(&app.launch, &display));
            app.windows.push((id, display));
            opening.push(opened.map(Message::Opened));
        }
        let appearance = {
            let source = source.clone();
            Task::perform(async move { theme::stored_appearance(&source).await }, Message::Appearance)
        };
        let tasks = [app.load(), iced::system::theme().map(Message::SystemMode), appearance];
        (app, Task::batch(opening.into_iter().chain(tasks)))
    }

    pub fn title(&self, id: iced::window::Id) -> String {
        match self.display(id) {
            Some(Display { monitor: Some(_), .. }) => self.display(id).map(window_title).unwrap_or_default(),
            _ => match self.current() {
                Some(reminder) => trf("Reminder · {title}", &[("title", &reminder.event.summary)]),
                None => tr("Reminder").to_string(),
            },
        }
    }

    /// The screen the window `id` shows on.
    fn display(&self, id: iced::window::Id) -> Option<&Display> {
        self.windows.iter().find(|(window, _)| *window == id).map(|(_, display)| display)
    }

    pub fn theme(&self, _id: iced::window::Id) -> Theme {
        theme::for_appearance(self.launch.appearance.unwrap_or(self.appearance), self.system_mode)
    }

    pub fn subscription(&self) -> Subscription<Message> {
        let mut all = vec![
            iced::window::resize_events().map(|(_, size)| Message::Resized(size)),
            iced::system::theme_changes().map(Message::SystemMode),
            iced::event::listen_with(key),
        ];
        // The demo stands still, so its pictures can be compared.
        if let Feed::Core(_) = self.feed {
            all.push(iced::time::every(Duration::from_secs(1)).map(|_| Message::Tick(Utc::now())));
            all.push(iced::time::every(POLL).map(|_| Message::Poll));
        }
        Subscription::batch(all)
    }

    /// The reminder shown.
    fn current(&self) -> Option<&Reminder> {
        self.snapshot.reminders.get(self.selected).or(self.snapshot.reminders.first())
    }

    fn load(&self) -> Task<Message> {
        match &self.feed {
            Feed::Core(socket) => {
                let socket = socket.clone();
                Task::perform(async move { load(socket, Utc::now()).await }, Message::Loaded)
            }
            Feed::Demo(source, case) => {
                let (source, case) = (Arc::clone(source), *case);
                Task::perform(async move { demo(source, case).await }, Message::Loaded)
            }
        }
    }

    /// Sends `requests` to the core in order (nothing in the demo), then asks again.
    fn send(&self, requests: Vec<Request>) -> Task<Message> {
        let Feed::Core(socket) = &self.feed else { return Task::none() };
        let socket = socket.clone();
        Task::perform(
            async move {
                for request in requests {
                    match torro_ipc::client::call(&socket, &request).await {
                        Ok(Response::Error { message, .. }) => return Err(message),
                        Ok(_) => {}
                        Err(err) => return Err(err.to_string()),
                    }
                }
                Ok(())
            },
            Message::Sent,
        )
    }

    /// Takes the shown reminder off the screen at once (the core's answer follows), and sends
    /// `request` for it.
    fn act_on_current(&mut self, request: impl FnOnce(String) -> Request) -> Task<Message> {
        let Some(id) = self.current().map(|reminder| reminder.id.clone()) else { return Task::none() };
        self.snooze_open = false;
        self.snapshot.reminders.retain(|reminder| reminder.id != id);
        self.selected = self.selected.min(self.snapshot.reminders.len().saturating_sub(1));
        let task = self.send(vec![request(id)]);
        Task::batch([task, self.close_when_empty()])
    }

    /// Closes the window when nothing is left to show.
    fn close_when_empty(&self) -> Task<Message> {
        match (&self.feed, self.snapshot.reminders.is_empty()) {
            (Feed::Core(_), true) => iced::exit(),
            _ => Task::none(),
        }
    }

    pub fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::Tick(now) => self.now = now,
            Message::Poll => return self.load(),
            Message::Loaded(Ok(snapshot)) => {
                self.failures = 0;
                let shown = self.current().map(|reminder| reminder.id.clone());
                self.snapshot = snapshot;
                self.selected = shown
                    .and_then(|id| self.snapshot.reminders.iter().position(|reminder| reminder.id == id))
                    .unwrap_or(0);
                if let Feed::Demo(..) = self.feed {
                    // The demo's clock stands at its sample moment.
                    self.now = demo_now(&self.snapshot, self.launch.demo.unwrap_or(Demo::Reminder));
                }
                if self.snapshot.reminders.is_empty() && self.loaded {
                    return self.close_when_empty();
                }
                self.loaded |= !self.snapshot.reminders.is_empty();
                if !self.loaded && matches!(self.feed, Feed::Core(_)) {
                    // Started while the reminder went already: nothing to show.
                    return iced::exit();
                }
            }
            Message::Loaded(Err(err)) => {
                self.failures += 1;
                eprintln!("torrocalendar: cannot read the reminders: {err}");
                if self.failures >= GIVE_UP_AFTER {
                    return iced::exit();
                }
            }
            Message::Resized(size) => self.size = size,
            Message::Opened(id) => {
                let real = matches!(self.feed, Feed::Core(_)) || self.launch.cover;
                if let (true, Some(display)) = (real, self.display(id)) {
                    return torro_gui::cover_screen(id, display.index, display.is_main());
                }
            }
            Message::SystemMode(mode) => self.system_mode = mode,
            Message::Appearance(Ok(appearance)) => self.appearance = appearance,
            Message::Appearance(Err(_)) => {}
            Message::Key(pressed) => {
                if self.opened.elapsed() < KEY_LOCK {
                    return Task::none();
                }
                if let Some(message) = self.key_message(&pressed) {
                    return self.update(message);
                }
            }
            Message::Join => {
                if let Some(join) = self.current().and_then(|reminder| reminder.join.clone()) {
                    open(&join.url);
                    return self.act_on_current(|id| Request::DismissReminder { id });
                }
            }
            Message::ToggleSnooze => self.snooze_open = !self.snooze_open,
            Message::Snooze(until) => return self.act_on_current(|id| Request::SnoozeReminder { id, until }),
            Message::Dismiss => return self.act_on_current(|id| Request::DismissReminder { id }),
            Message::DismissAll => {
                let all: Vec<Request> = self
                    .snapshot
                    .reminders
                    .drain(..)
                    .map(|reminder| Request::DismissReminder { id: reminder.id })
                    .collect();
                let task = self.send(all);
                return Task::batch([task, self.close_when_empty()]);
            }
            Message::Previous => self.step(-1),
            Message::Next => self.step(1),
            Message::OpenCalendar => {
                let calendar = Request::OpenApp { app: AppId::CALENDAR.binary.to_string(), target: None };
                return self.send(vec![calendar]);
            }
            Message::OpenMaps => {
                if let Some(place) = self.current().and_then(|reminder| reminder.event.location.clone()) {
                    open(&maps_url(&place));
                }
            }
            Message::OpenLink(url) => open(&url),
            Message::ToggleNotes => self.notes_open = !self.notes_open,
            Message::PauseHour => {
                let until = Some(self.now + TimeDelta::hours(1));
                let task = self.send(vec![Request::PauseReminders { until }]);
                return match self.feed {
                    Feed::Core(_) => Task::batch([task, iced::exit()]),
                    Feed::Demo(..) => task,
                };
            }
            Message::Sent(Ok(())) => return self.load(),
            Message::Sent(Err(err)) => {
                eprintln!("torrocalendar: a reminder command failed: {err}");
                return self.load();
            }
        }
        Task::none()
    }

    /// Shows the reminder `by` places before or after the shown one, around the end.
    fn step(&mut self, by: isize) {
        let count = self.snapshot.reminders.len();
        if count > 1 {
            let next = (self.selected as isize + by).rem_euclid(count as isize);
            self.selected = usize::try_from(next).unwrap_or(0);
        }
    }

    /// What `pressed` does now (see the module); `None` for a key that does nothing.
    pub fn key_message(&self, pressed: &Pressed) -> Option<Message> {
        let reminder = self.current()?;
        let started = starts(&reminder.event) <= self.now;
        if self.snooze_open {
            return match pressed {
                Pressed::Char(c) if c == "1" => Some(Message::Snooze(Snooze::Minutes(1))),
                Pressed::Char(c) if c == "5" => Some(Message::Snooze(Snooze::Minutes(5))),
                Pressed::Char(c) if c == "0" => Some(Message::Snooze(Snooze::Minutes(10))),
                Pressed::Char(c) if c == "b" && !started => Some(Message::Snooze(Snooze::UntilStart)),
                Pressed::Escape => Some(Message::ToggleSnooze),
                Pressed::Char(c) if c == "s" => Some(Message::ToggleSnooze),
                _ => None,
            };
        }
        match pressed {
            Pressed::Enter if reminder.join.is_some() => Some(Message::Join),
            Pressed::Enter => Some(Message::OpenCalendar),
            Pressed::Escape => Some(Message::Dismiss),
            Pressed::Left => Some(Message::Previous),
            Pressed::Right => Some(Message::Next),
            Pressed::Char(c) if c == "s" => Some(Message::ToggleSnooze),
            Pressed::Char(_) => None,
        }
    }

    pub fn view(&self, id: iced::window::Id) -> Element<'_, Message> {
        let color = self.current().map_or(Color::TRANSPARENT, |reminder| self.calendar_color(&reminder.event));
        if self.display(id).is_some_and(|display| !display.is_main()) {
            let calm = container(self.calm()).center(Fill).padding([0.0, space::xxl()]);
            return container(calm).width(Fill).height(Fill).style(style::container::reminder_screen(color)).into();
        }
        let content = responsive(move |size| self.screen(Screen::of(size)));
        let screen = container(content).width(Fill).height(Fill).style(style::container::reminder_screen(color));
        match self.snooze_menu() {
            Some(menu) => iced::widget::stack![screen, menu].into(),
            None => screen.into(),
        }
    }

    fn screen(&self, screen: Screen) -> Element<'_, Message> {
        let Some(reminder) = self.current() else {
            return container(typo::text(typo::body(), tr("Nothing is due.")).style(theme::text_3)).center(Fill).into();
        };
        let lead = self.lead(reminder, screen);
        let body: Element<'_, Message> = match screen {
            // Side by side, centred: on a 32:9 screen the extra room stays around them.
            Screen::Wide => container(
                row![container(lead).width(Fill), container(self.facts(reminder, screen)).width(size::panel()),]
                    .spacing(space::xxl())
                    .align_y(Alignment::Center),
            )
            .width(Fill)
            .max_width(size::content_max() + size::panel() + space::xxl())
            .into(),
            Screen::Normal => column![lead, self.facts(reminder, screen)]
                .spacing(space::xl())
                .width(Fill)
                .max_width(size::content_max())
                .into(),
            Screen::Small => container(lead).width(Fill).max_width(size::content_max()).into(),
        };
        let middle = container(body).center(Fill).padding([0.0, space::xxl()]);
        column![self.head(reminder), middle, self.foot()].width(Fill).height(Fill).into()
    }

    /// The calm version of another screen: what and when, and the three buttons; the details stay
    /// on the main screen.
    fn calm(&self) -> Element<'_, Message> {
        let Some(reminder) = self.current() else { return Space::new().into() };
        let title = match reminder.event.summary.trim() {
            "" => tr("(no title)").to_string(),
            summary => summary.to_string(),
        };
        let mut buttons = row![].spacing(space::m()).align_y(Alignment::Center);
        if let Some(join) = &reminder.join {
            let label = trf("Join {service}", &[("service", &join.service)]);
            buttons = buttons.push(widgets::big_button(
                Some(Icon::Video),
                label,
                None,
                style::button::primary,
                Some(Message::Join),
            ));
        }
        let style = if reminder.join.is_some() { style::button::secondary } else { style::button::primary };
        buttons = buttons
            .push(widgets::big_button(
                Some(Icon::Clock),
                tr("In 5 min"),
                None,
                style,
                Some(Message::Snooze(Snooze::Minutes(5))),
            ))
            .push(widgets::big_button(
                Some(Icon::X),
                tr("Dismiss"),
                None,
                style::button::ghost,
                Some(Message::Dismiss),
            ));
        column![
            self.kind(reminder),
            typo::text(typo::hero(), title).style(theme::text),
            self.countdown_line(&reminder.event),
            container(buttons).padding([space::l(), 0.0]),
            typo::text(typo::meta(), tr("Details on your main display")).style(theme::text_3),
        ]
        .spacing(space::m())
        .align_x(Alignment::Center)
        .max_width(size::content_max())
        .into()
    }

    /// Top: what this is, a preview's sender, the clock.
    fn head(&self, reminder: &Reminder) -> Element<'_, Message> {
        let mut left = row![
            mark(Icon::Bell, icon::SIZE_SM, Ink::Text3),
            typo::text(typo::caption(), tr("Reminder")).style(theme::text_3),
        ]
        .spacing(space::s())
        .align_y(Alignment::Center);
        if let Some(by) = &reminder.preview_by {
            let preview = trf("Preview · sent by {client}", &[("client", by)]);
            left = left.push(widgets::state_pill(Icon::Eye, preview));
        }
        let clock = format_time(to_local(self.now).time());
        let clock = typo::text(typo::countdown(), clock).style(theme::text);
        row![left, Space::new().width(Fill), clock]
            .align_y(Alignment::Center)
            .height(size::header())
            .padding([0.0, space::xxl()])
            .into()
    }

    /// The calendar, the kind of event, the title, the countdown, the time, the buttons.
    fn lead<'a>(&'a self, reminder: &'a Reminder, screen: Screen) -> Element<'a, Message> {
        let event = &reminder.event;
        let mut lead = column![].spacing(space::m());
        if self.snapshot.reminders.len() > 1 {
            lead = lead.push(self.pager());
        }
        lead = lead.push(self.kind(reminder));
        let title = match event.summary.trim() {
            "" => tr("(no title)").to_string(),
            summary => summary.to_string(),
        };
        lead = lead.push(typo::text(typo::hero(), title).style(theme::text));
        lead = lead.push(self.countdown_line(event));
        lead = lead.push(when_line(event));
        if screen == Screen::Small {
            lead = lead.push(self.chips(reminder));
        }
        lead.push(container(self.buttons(reminder)).padding([space::l(), 0.0])).into()
    }

    fn pager(&self) -> Element<'_, Message> {
        let place = trf("{i} of {n}", &[("i", &(self.selected + 1)), ("n", &self.snapshot.reminders.len())]);
        row![
            widgets::icon_button(Icon::ChevronLeft, tr("Previous"), Some(Message::Previous)),
            widgets::icon_button(Icon::ChevronRight, tr("Next"), Some(Message::Next)),
            typo::text(typo::caption(), place).style(theme::text_3),
            widgets::kbd("←"),
            widgets::kbd("→"),
        ]
        .spacing(space::s())
        .align_y(Alignment::Center)
        .into()
    }

    fn kind<'a>(&'a self, reminder: &'a Reminder) -> Element<'a, Message> {
        let event = &reminder.event;
        let calendar = self.snapshot.calendars.iter().find(|calendar| calendar.id == event.collection);
        let name = calendar.map(|calendar| calendar.name.clone()).unwrap_or_default();
        let mut line = row![
            widgets::color_dot(self.calendar_color(event), space::s()),
            typo::text(typo::body(), name).style(theme::text_2),
        ]
        .spacing(space::s())
        .align_y(Alignment::Center);
        let kind = match (&reminder.join, &event.location) {
            (Some(join), _) => Some((Icon::Video, trf("Video call · {service}", &[("service", &join.service)]))),
            (None, Some(place)) if !place.trim().is_empty() => Some((Icon::MapPin, tr("In person").to_string())),
            _ => None,
        };
        if let Some((which, label)) = kind {
            line = line
                .push(typo::text(typo::body(), "·").style(theme::text_3))
                .push(mark(which, icon::SIZE_SM, Ink::Text2))
                .push(typo::text(typo::body(), label).style(theme::text_2));
        }
        line.into()
    }

    fn countdown_line(&self, event: &Event) -> Element<'_, Message> {
        match countdown(starts(event), self.now) {
            Countdown::Starts(left) => row![
                typo::text(typo::countdown(), tr("Starts in")).style(theme::text_2),
                typo::text(typo::countdown(), left).style(theme::text),
            ]
            .spacing(space::m())
            .align_y(Alignment::Center)
            .into(),
            Countdown::Started(minutes) => row![
                mark(Icon::CircleAlert, icon::SIZE, Ink::Attention),
                typo::text(typo::countdown(), trf("Started {n} min ago", &[("n", &minutes)]))
                    .style(|t: &Theme| iced::widget::text::Style { color: Some(palette(t).attention_text) }),
            ]
            .spacing(space::s())
            .align_y(Alignment::Center)
            .into(),
        }
    }

    /// The facts of the small layout as chips under the title.
    fn chips(&self, reminder: &Reminder) -> Element<'_, Message> {
        let event = &reminder.event;
        let mut chips = row![].spacing(space::s());
        if let Some(place) = event.location.as_deref().filter(|place| !place.trim().is_empty()) {
            chips = chips.push(widgets::icon_pill(Icon::MapPin, typo::ellipsize(place, typo::caption(), size::list())));
        }
        if event.description.as_deref().is_some_and(|notes| !notes.trim().is_empty()) {
            let label = if self.notes_open { tr("Hide notes") } else { tr("Show notes") };
            chips = chips.push(widgets::text_button(
                Some(Icon::AlignLeft),
                label,
                style::button::secondary,
                Some(Message::ToggleNotes),
            ));
        }
        let mut all = column![chips].spacing(space::m());
        if self.notes_open
            && let Some(notes) = &event.description
        {
            all = all.push(typo::text(typo::body(), notes.clone()).style(theme::text_2));
        }
        all.into()
    }

    fn buttons<'a>(&'a self, reminder: &'a Reminder) -> Element<'a, Message> {
        let started = starts(&reminder.event) <= self.now;
        let snooze = widgets::big_button(
            Some(Icon::Clock),
            tr("Snooze"),
            Some("S"),
            style::button::secondary,
            Some(Message::ToggleSnooze),
        );
        let snooze = widgets::anchored(&self.snooze_anchor, snooze);
        let mut buttons = row![].spacing(space::m()).align_y(Alignment::Center);
        match &reminder.join {
            Some(join) => {
                let label = match started {
                    true => tr("Join now").to_string(),
                    false => trf("Join {service}", &[("service", &join.service)]),
                };
                buttons = buttons
                    .push(widgets::big_button(
                        Some(Icon::Video),
                        label,
                        Some("Enter"),
                        style::button::primary,
                        Some(Message::Join),
                    ))
                    .push(snooze)
                    .push(widgets::big_button(
                        Some(Icon::X),
                        tr("Dismiss"),
                        Some("Esc"),
                        style::button::ghost,
                        Some(Message::Dismiss),
                    ));
            }
            None => {
                buttons = buttons
                    .push(widgets::big_button(
                        Some(Icon::Check),
                        tr("Dismiss"),
                        Some("Esc"),
                        style::button::primary,
                        Some(Message::Dismiss),
                    ))
                    .push(snooze)
                    .push(widgets::big_button(
                        Some(Icon::Calendar),
                        tr("Open in calendar"),
                        Some("Enter"),
                        style::button::ghost,
                        Some(Message::OpenCalendar),
                    ));
            }
        }
        buttons.into()
    }

    /// The snooze menu at its button, over the whole window, while it is open.
    fn snooze_menu(&self) -> Option<Element<'_, Message>> {
        if !self.snooze_open {
            return None;
        }
        let started = self.current().is_some_and(|reminder| starts(&reminder.event) <= self.now);
        let item = |label: &'static str, key: &'static str, until: Snooze| {
            MenuItem::new(Icon::Clock, tr(label), Message::Snooze(until)).trail(Trail::Kbd(key))
        };
        let mut items = vec![
            item("1 min", "1", Snooze::Minutes(1)),
            item("5 min", "5", Snooze::Minutes(5)),
            item("10 min", "0", Snooze::Minutes(10)),
        ];
        if !started {
            items.push(item("Until it starts", "B", Snooze::UntilStart));
        }
        let menu = widgets::menu(vec![MenuSection::titled(tr("Snooze"), items)]);
        Some(widgets::floating(&self.snooze_anchor, Placement::Below(Align::Start), menu, Message::ToggleSnooze))
    }

    /// Place, links and notes; nothing for the small layout (its chips show them).
    fn facts<'a>(&'a self, reminder: &'a Reminder, screen: Screen) -> Element<'a, Message> {
        if screen == Screen::Small {
            return Space::new().into();
        }
        let event = &reminder.event;
        let mut facts = column![].spacing(space::s());
        if let Some(place) = event.location.as_deref().filter(|place| !place.trim().is_empty()) {
            let value = column![
                typo::text(typo::body(), place).style(theme::text),
                widgets::text_button(
                    Some(Icon::MapPin),
                    tr("Open in Maps"),
                    style::button::ghost,
                    Some(Message::OpenMaps)
                ),
            ]
            .spacing(space::xs());
            facts = facts.push(fact(Icon::MapPin, tr("Location"), value));
        }
        let found = event_links(event);
        if !found.is_empty() {
            let list = found.into_iter().fold(column![].spacing(space::xs()), |list, url| {
                let mark = if links::meeting(&url).is_some() { Icon::Video } else { Icon::Link };
                let label = typo::ellipsize(&url, typo::body(), size::panel());
                list.push(widgets::text_button(Some(mark), label, style::button::ghost, Some(Message::OpenLink(url))))
            });
            facts = facts.push(fact(Icon::Link, tr("Links"), list));
        }
        if let Some(notes) = event.description.as_deref().filter(|notes| !notes.trim().is_empty()) {
            let all = self.notes_open || screen == Screen::Wide;
            let (shown, more) = notes_shown(notes, all);
            let mut value = column![typo::text(typo::body(), shown).style(theme::text_2)].spacing(space::xs());
            if more || self.notes_open {
                let label = if self.notes_open { tr("Show less") } else { tr("Show all") };
                value = value.push(widgets::text_button(None, label, style::button::ghost, Some(Message::ToggleNotes)));
            }
            facts = facts.push(fact(Icon::AlignLeft, tr("Notes"), value));
        }
        facts.into()
    }

    /// The next events and the window's quiet actions.
    fn foot(&self) -> Element<'_, Message> {
        let mut next = row![typo::text(typo::caption(), tr("Next")).style(theme::text_3)]
            .spacing(space::m())
            .align_y(Alignment::Center);
        let due: Vec<&str> = self.snapshot.reminders.iter().map(|reminder| reminder.event.uid.as_str()).collect();
        let coming = self
            .snapshot
            .next
            .iter()
            .filter(|event| !event.all_day() && starts(event) > self.now && !due.contains(&event.uid.as_str()))
            .take(2);
        let today = to_local(self.now).date();
        for event in coming {
            let start = to_local(starts(event));
            let when = match start.date() == today {
                true => format_time(start.time()),
                false => {
                    format!("{} {}", torro_gui::datetime::weekday_name(start.date(), true), format_time(start.time()))
                }
            };
            next = next
                .push(typo::text(typo::meta(), when).style(theme::text_3))
                .push(widgets::color_dot(self.calendar_color(event), space::s()))
                .push(
                    typo::text(typo::body(), typo::ellipsize(&event.summary, typo::body(), size::props()))
                        .style(theme::text),
                );
        }
        let mut right = row![].spacing(space::s()).align_y(Alignment::Center);
        if self.snapshot.reminders.len() > 1 {
            right = right.push(widgets::text_button(
                Some(Icon::Check),
                tr("Dismiss all"),
                style::button::ghost,
                Some(Message::DismissAll),
            ));
        }
        right = right.push(widgets::text_button(
            Some(Icon::BellOff),
            tr("Pause for 1 hour"),
            style::button::ghost,
            Some(Message::PauseHour),
        ));
        row![next, Space::new().width(Fill), right]
            .align_y(Alignment::Center)
            .height(size::row_notice())
            .padding([0.0, space::xxl()])
            .into()
    }

    fn calendar_color(&self, event: &Event) -> Color {
        let calendar = self.snapshot.calendars.iter().find(|calendar| calendar.id == event.collection);
        collection_color(calendar.and_then(|calendar| calendar.color.as_deref()))
    }
}

/// The colour an icon of the window takes.
#[derive(Debug, Clone, Copy)]
enum Ink {
    Text2,
    Text3,
    Attention,
}

/// `which` at `edge` in `ink`.
fn mark<'a>(which: Icon, edge: f32, ink: Ink) -> Element<'a, Message> {
    icon::icon(which, edge)
        .style(move |t: &Theme, _| {
            let p = palette(t);
            let color = match ink {
                Ink::Text2 => p.text_2,
                Ink::Text3 => p.text_3,
                Ink::Attention => p.attention,
            };
            iced::widget::svg::Style { color: Some(color) }
        })
        .into()
}

/// One row of facts: an icon and a label in `text_3`, the value beside.
fn fact<'a>(which: Icon, label: &'a str, value: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
    let label = row![
        mark(which, icon::SIZE_SM, Ink::Text3),
        typo::text(typo::caption(), label).style(theme::text_3).wrapping(Wrapping::None),
    ]
    .spacing(space::s())
    .align_y(Alignment::Center)
    .width(widgets::prop_label_width() + space::xl());
    let line = row![label, container(value.into()).width(Fill)].spacing(space::m());
    column![
        iced::widget::rule::horizontal(size::HAIRLINE).style(style::rule),
        container(line).padding([space::s(), 0.0])
    ]
    .into()
}

/// "10:00 – 10:45 · 45 min · Thu, 15 Oct", with the series' sign for a recurring event.
fn when_line(event: &Event) -> Element<'_, Message> {
    let start = starts(event);
    let date = format_long_date(to_local(start).date());
    let text = match (event.start, event.end) {
        (EventTime::Date(_), _) => format!("{} · {date}", tr("All day")),
        (EventTime::DateTime(_), Some(EventTime::DateTime(end))) => {
            let minutes = (end - start).num_minutes();
            let length = trf("{n} min", &[("n", &minutes)]);
            format!(
                "{} – {} · {length} · {date}",
                format_time(to_local(start).time()),
                format_time(to_local(end).time())
            )
        }
        (EventTime::DateTime(_), _) => format!("{} · {date}", format_time(to_local(start).time())),
    };
    let mut line =
        row![mark(Icon::Clock, icon::SIZE_SM, Ink::Text3), typo::text(typo::read(), text).style(theme::text_2),]
            .spacing(space::s())
            .align_y(Alignment::Center);
    if event.recurring {
        line = line.push(mark(Icon::Repeat, icon::SIZE_SM, Ink::Text3));
    }
    line.into()
}

/// The notes as shown: three lines (or all), and whether more is left.
pub fn notes_shown(notes: &str, all: bool) -> (String, bool) {
    let lines: Vec<&str> = notes.trim().lines().collect();
    if all || lines.len() <= 3 {
        return (lines.join("\n"), false);
    }
    (format!("{}…", lines[..3].join("\n")), true)
}

/// The links of the event's place and notes, a call first, each once.
fn event_links(event: &Event) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    for text in [event.location.as_deref(), event.description.as_deref()].into_iter().flatten() {
        for url in links::links(text) {
            if links::openable(url) && !found.iter().any(|known| known == url) {
                found.push(url.to_string());
            }
        }
    }
    found.sort_by_key(|url| links::meeting(url).is_none());
    found
}

/// When the event starts; a day from local midnight.
fn starts(event: &Event) -> DateTime<Utc> {
    match event.start {
        EventTime::DateTime(at) => at,
        EventTime::Date(date) => torro_gui::datetime::to_utc(date.and_time(chrono::NaiveTime::MIN)),
    }
}

/// OpenStreetMap's search for `place`; the place goes there only when the person clicks.
pub fn maps_url(place: &str) -> String {
    let mut query = String::new();
    for byte in place.trim().bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => query.push(char::from(byte)),
            b' ' => query.push('+'),
            other => query.push_str(&format!("%{other:02X}")),
        }
    }
    format!("https://www.openstreetmap.org/search?query={query}")
}

fn open(url: &str) {
    if !links::openable(url) {
        return;
    }
    if let Err(err) = webbrowser::open(url) {
        eprintln!("torrocalendar: cannot open {url}: {err}");
    }
}

/// The window's keys, without modifiers.
fn key(event: iced::Event, status: iced::event::Status, _window: iced::window::Id) -> Option<Message> {
    use iced::keyboard::{Event as Keyboard, Key, key::Named};
    if status == iced::event::Status::Captured {
        return None;
    }
    let iced::Event::Keyboard(Keyboard::KeyPressed { key, modifiers, .. }) = event else { return None };
    if modifiers.control() || modifiers.alt() || modifiers.logo() {
        return None;
    }
    let pressed = match key.as_ref() {
        Key::Named(Named::Enter) => Pressed::Enter,
        Key::Named(Named::Escape) => Pressed::Escape,
        Key::Named(Named::ArrowLeft) => Pressed::Left,
        Key::Named(Named::ArrowRight) => Pressed::Right,
        Key::Character(c) => Pressed::Char(c.to_lowercase()),
        _ => return None,
    };
    Some(Message::Key(pressed))
}

/// What is due, the next events and the calendars, from the core at `socket`.
async fn load(socket: PathBuf, now: DateTime<Utc>) -> Result<Snapshot, String> {
    let ask = |request: Request| {
        let socket = socket.clone();
        async move { torro_ipc::client::call(&socket, &request).await.map_err(|err| err.to_string()) }
    };
    let reminders = match ask(Request::ListReminders { due_only: true }).await? {
        Response::Reminders { reminders, .. } => reminders,
        Response::Error { message, .. } => return Err(message),
        other => return Err(format!("unexpected answer: {other:?}")),
    };
    let next = match ask(Request::Events { start: now, end: now + next_ahead() }).await? {
        Response::Events { mut events } => {
            events.sort_by_key(starts);
            events
        }
        _ => Vec::new(),
    };
    let calendars = match ask(Request::CalendarCollections).await? {
        Response::Collections { collections } => collections,
        _ => Vec::new(),
    };
    Ok(Snapshot { reminders, next, calendars })
}

/// The sample state `case` from the demo calendar: the sprint planning with its Zoom link (two
/// reminders with the client presentation for [`Demo::Many`]), the dentist for [`Demo::Onsite`].
async fn demo(source: Arc<DemoSource>, case: Demo) -> Result<Snapshot, String> {
    let from = Local::now().with_timezone(&Utc) - TimeDelta::days(31);
    let events = source.events(from, from + TimeDelta::days(62)).await.map_err(|err| err.to_string())?;
    let calendars = source.collections().await.map_err(|err| err.to_string())?;
    let find = |uid: &str| events.iter().find(|event| event.uid == uid).cloned();
    let lead = match case {
        Demo::Onsite => find("dentist@torro-demo"),
        _ => find("sprint-planning@torro-demo"),
    }
    .ok_or("the demo calendar has no such event")?;
    let mut reminders = vec![sample(&lead, case)];
    if case == Demo::Many {
        let mut other = find("client-presentation@torro-demo").ok_or("the demo calendar has no such event")?;
        // At the same time as the planning, for the sample.
        other.start = lead.start;
        other.end = lead.end;
        reminders.push(sample(&other, case));
    }
    let lead_start = starts(&lead);
    let mut next: Vec<Event> =
        events.into_iter().filter(|event| starts(event) > lead_start && !event.all_day()).collect();
    next.sort_by_key(starts);
    Ok(Snapshot { reminders, next, calendars })
}

fn sample(event: &Event, case: Demo) -> Reminder {
    let join = event_links(event)
        .into_iter()
        .find_map(|url| links::meeting(&url).map(|service| Join { service: service.to_string(), url }));
    Reminder {
        id: format!("demo-{}", event.uid),
        event: event.clone(),
        fires_at: starts(event) - TimeDelta::minutes(5),
        style: AlarmStyle::FullScreen,
        state: ReminderState::Due,
        snoozed_until: None,
        join,
        preview_by: (case == Demo::Preview).then(|| "Claude Code".to_string()),
    }
}

/// The demo's moment: 4:32 before the start, two minutes after it for [`Demo::Started`].
fn demo_now(snapshot: &Snapshot, case: Demo) -> DateTime<Utc> {
    let start = snapshot.reminders.first().map(|reminder| starts(&reminder.event)).unwrap_or_else(Utc::now);
    match case {
        Demo::Started => start + TimeDelta::minutes(2),
        Demo::Onsite => start - TimeDelta::minutes(29) - TimeDelta::seconds(40),
        _ => start - TimeDelta::minutes(4) - TimeDelta::seconds(32),
    }
}

/// Runs the reminder window with `args` (see [`Launch`]).
pub fn run(args: &[String]) -> Result<(), String> {
    let launch = Launch::parse(args)?;
    let source = match launch.demo {
        Some(_) => Source::Demo(Arc::new(DemoSource::new(Local::now().date_naive()))),
        None => Source::from_args(args.iter().cloned()),
    };
    torro_gui::design::start_for(&source, &launch.forced());
    iced::daemon(move || App::new(source.clone(), launch.clone()), App::update, App::view)
        .settings(torro_gui::settings())
        .title(App::title)
        .theme(App::theme)
        .subscription(App::subscription)
        .run()
        .map_err(|err| err.to_string())
}

#[cfg(test)]
mod tests;
