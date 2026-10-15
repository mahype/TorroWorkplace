//! `torromailview` — one mail as it was written, in a window of its own (task M9.2, decision 8 in
//! `docs/mail.md`).
//!
//! `torromailview <m:id>` shows the mail with the core's id `m:…` (as `torroinbox --compose` gets its
//! message): it asks the core for it with `mail.get_message` (`include_body`, `include_html`, through
//! [`InboxClient::get_mail_html`] with the app key) and shows it in the system's web view (wry:
//! WebKitGTK on Linux, WKWebView on macOS, WebView2 on Windows) in a tao window. Iced draws no HTML,
//! and under Wayland no web view can be put into a winit window, hence this own program. Window class
//! and Wayland app id `torro-mailview`, so a tiling manager places it beside the list.
//!
//! The program reads and writes nothing itself; everything comes from the core over `torro_ipc`. The
//! one exception, as when the inbox opens an attachment: an inline picture (`cid:`) is fetched by the
//! core with `mail.get_attachment` into its managed folder, and the window reads that file to hand
//! it to the web view ([`Pictures`]).
//!
//! Safety in the window, the second layer after the cleaning in the core ([`page`]): the CSP forbids
//! every script, form and `base`; JavaScript is switched off in the web view as well; the page never
//! navigates – links go to the system's browser or mail program, everything else is refused; no
//! downloads, no new windows, no developer tools. Pictures from the web load directly (decision 8).
//!
//! Keys: `Esc` and `Ctrl+W` close, `Ctrl` + `+`/`-`/`0` zoom (⌘ on macOS); copying is the web view's
//! own (`Ctrl+C`; on macOS ⌘C through the Edit menu, which the window brings for it). The zoom starts
//! at the suite's (`Settings::zoom`) and changes only this window.
//!
//! `--demo` shows a sample HTML mail without the core; `--open newsletter|text|offline|gone` picks
//! the sample (a newsletter with tables and pictures, a plain text mail, the two problem pages).
//! `--appearance light|dark|system` overrides the stored appearance for this process; `--design <id>`
//! (only with `--demo`) draws with that design pack. Wrong parameters: a message and exit code 64.

mod page;

use std::borrow::Cow;
use std::collections::HashMap;
use std::error::Error;
use std::process::ExitCode;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use chrono::{DateTime, Utc};
use tao::dpi::LogicalSize;
use tao::event::{ElementState, Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::keyboard::{Key, ModifiersState};
use tao::window::{Theme, WindowBuilder};
use torro_core::design::{Design, Mode};
use torro_core::i18n::tr;
use torro_core::settings::Zoom;
use torro_core::theme::Appearance;
use torro_inbox::InboxError;
use torro_ipc::inbox::InboxClient;
use torro_ipc::{Request, Response};
use torro_mail::{AttachmentInfo, MailAddress, Message, MessageSummary};
use wry::http::header::{CONTENT_SECURITY_POLICY, CONTENT_TYPE, HeaderValue};
use wry::http::{Response as HttpResponse, StatusCode};
use wry::{NewWindowResponse, WebView, WebViewBuilder};

use crate::page::{Look, Navigation};

/// The program's name, in messages and `--version`.
const BINARY: &str = "torromailview";

/// Window class (X11) and app id (Wayland): rules of a tiling manager match on it.
const WINDOW_CLASS: &str = "torro-mailview";

/// The longest mail id the program takes, as the core's mail commands.
const MESSAGE_MAX: usize = 300;

/// How long the window waits for the core's answers before it says the core does not respond.
const LOAD_WAIT: Duration = Duration::from_secs(15);

/// The largest inline picture handed to the web view; larger files are refused (404).
const PICTURE_MAX: u64 = 20 * 1024 * 1024;

/// The size the window opens at: a letter, beside the list.
const WINDOW: LogicalSize<f64> = LogicalSize::new(820.0, 900.0);

/// The smallest window, as every window of the suite (design rule: mini and strip are possible).
const WINDOW_MIN: LogicalSize<f64> = LogicalSize::new(200.0, 120.0);

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--version" || arg == "-V") {
        println!("{BINARY} {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    let launch = match Launch::parse(&args, torro_gui::design::is_choice) {
        Ok(launch) => launch,
        Err(err) => {
            eprintln!("{BINARY}: {err}");
            return ExitCode::from(64);
        }
    };
    match run(launch) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{BINARY}: {err}");
            ExitCode::FAILURE
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Start parameters
// ---------------------------------------------------------------------------------------------

/// What the program was started with.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Launch {
    /// The mail to show (`m:…`); `None` only with `--demo`.
    id: Option<String>,
    /// `--demo`: a sample instead of the core.
    demo: bool,
    /// Which sample (`--open`).
    sample: Sample,
    /// `--appearance`, else the stored one.
    appearance: Option<Appearance>,
    /// `--design <id>` (only with `--demo`).
    design: Option<String>,
}

/// The samples of `--demo` (`--open`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Sample {
    /// A newsletter with tables and inline pictures, and an attachment.
    Newsletter,
    /// A plain text mail.
    Text,
    /// The page when the core does not answer.
    Offline,
    /// The page when the mail is gone.
    Gone,
}

impl Sample {
    const ALL: [(&'static str, Sample); 4] = [
        ("newsletter", Sample::Newsletter),
        ("text", Sample::Text),
        ("offline", Sample::Offline),
        ("gone", Sample::Gone),
    ];
}

impl Launch {
    /// Reads `args` (without the program's name). `is_design` tells whether `--design` names a design
    /// this process can draw with. `Err` with the message for the person: an unknown parameter, a
    /// missing value, an id that is no mail id of the core (it starts with `m:`, at most
    /// [`MESSAGE_MAX`] bytes, no spaces or control characters), no id without `--demo`, `--open` or
    /// `--design` without `--demo`.
    fn parse(args: &[String], is_design: impl Fn(&str) -> bool) -> Result<Launch, String> {
        let mut launch = Launch { id: None, demo: false, sample: Sample::Newsletter, appearance: None, design: None };
        let mut open = None;
        let mut rest = args.iter();
        while let Some(arg) = rest.next() {
            let (flag, inline) = match arg.split_once('=') {
                Some((flag, value)) if flag.starts_with("--") => (flag, Some(value.to_string())),
                _ => (arg.as_str(), None),
            };
            let mut value =
                |name: &str| inline.clone().or_else(|| rest.next().cloned()).ok_or(format!("{name} needs a value"));
            match flag {
                "--demo" => launch.demo = true,
                "--open" => open = Some(value("--open")?),
                "--appearance" => {
                    let word = value("--appearance")?;
                    launch.appearance = Some(match word.as_str() {
                        "light" => Appearance::Light,
                        "dark" => Appearance::Dark,
                        "system" => Appearance::System,
                        _ => return Err(format!("--appearance takes light, dark or system, not {word:?}")),
                    });
                }
                "--design" => {
                    let id = value("--design")?;
                    if !is_design(&id) {
                        return Err(format!(
                            "--design takes one of {}, not {id:?}",
                            torro_gui::design::choices().join(", ")
                        ));
                    }
                    launch.design = Some(id);
                }
                id if !id.starts_with('-') && launch.id.is_none() => {
                    let valid = id.starts_with("m:")
                        && id.len() > 2
                        && id.len() <= MESSAGE_MAX
                        && !id.chars().any(|c| c.is_whitespace() || c.is_control());
                    if !valid {
                        return Err(format!("takes a mail id (m:…), not {id:?}"));
                    }
                    launch.id = Some(id.to_string());
                }
                other => return Err(format!("unknown parameter {other:?}; usage: {BINARY} <m:id> | --demo")),
            }
        }
        if let Some(word) = open {
            if !launch.demo {
                return Err("--open only goes with --demo".into());
            }
            launch.sample =
                Sample::ALL.iter().find(|(name, _)| *name == word).map(|(_, sample)| *sample).ok_or_else(|| {
                    let names: Vec<&str> = Sample::ALL.iter().map(|(name, _)| *name).collect();
                    format!("--open takes {}, not {word:?}", names.join(", "))
                })?;
        }
        if launch.design.is_some() && !launch.demo {
            return Err("--design only goes with --demo".into());
        }
        if launch.id.is_none() && !launch.demo {
            return Err(format!("usage: {BINARY} <m:id> | --demo"));
        }
        Ok(launch)
    }
}

// ---------------------------------------------------------------------------------------------
// What the window shows
// ---------------------------------------------------------------------------------------------

/// Everything the window needs, read before it opens: the page, the inline parts it may ask for,
/// how it looks and how large it is drawn.
struct Shown {
    title: String,
    page: String,
    pictures: Pictures,
    appearance: Appearance,
    design: Design,
    zoom: Zoom,
}

/// Where the inline pictures of the shown mail come from.
enum Source {
    /// The core: the mail with this id.
    Core { client: InboxClient, id: String },
    /// The samples of `--demo` ([`demo_picture`]).
    Demo,
}

/// The `torro-cid` protocol of one mail: its parts, where to fetch them, and what was fetched already
/// (a newsletter names the same picture many times; the core then writes it once).
struct Pictures {
    parts: Vec<AttachmentInfo>,
    source: Source,
    fetched: Mutex<HashMap<String, Option<Arc<Vec<u8>>>>>,
}

impl Pictures {
    /// The answer to a request for `path` ([`page::cid_part`]): the picture with the type its bytes
    /// show ([`page::picture_type`]), or 404 for a part that is no picture, cannot be fetched, is
    /// larger than [`PICTURE_MAX`] or whose bytes are no PNG, JPEG, GIF or WebP.
    async fn answer(&self, path: &str) -> HttpResponse<Cow<'static, [u8]>> {
        let Some(part) = page::cid_part(path, &self.parts) else { return not_found() };
        let Some(bytes) = self.bytes(part).await else { return not_found() };
        let Some(kind) = page::picture_type(&bytes) else { return not_found() };
        let mut response = HttpResponse::new(Cow::Owned(bytes.as_ref().clone()));
        response.headers_mut().insert(CONTENT_TYPE, HeaderValue::from_static(kind));
        response.headers_mut().insert("x-content-type-options", HeaderValue::from_static("nosniff"));
        response
    }

    /// The bytes of `part`, fetched once.
    async fn bytes(&self, part: &AttachmentInfo) -> Option<Arc<Vec<u8>>> {
        let known = self.fetched.lock().unwrap_or_else(PoisonError::into_inner).get(&part.part).cloned();
        if let Some(known) = known {
            return known;
        }
        let bytes = match &self.source {
            Source::Core { client, id } => match client.mail_attachment(id.clone(), part.part.clone()).await {
                Ok(path) => read_picture(&path),
                Err(err) => {
                    // The code only: no id, part, content id or file name in the log.
                    eprintln!("{BINARY}: an inline picture could not be fetched ({})", error_code(&err));
                    None
                }
            },
            Source::Demo => demo_picture(&part.part),
        }
        .map(Arc::new);
        self.fetched.lock().unwrap_or_else(PoisonError::into_inner).insert(part.part.clone(), bytes.clone());
        bytes
    }
}

/// The file the core put into its managed attachment folder, when it is not larger than
/// [`PICTURE_MAX`].
fn read_picture(path: &std::path::Path) -> Option<Vec<u8>> {
    let size = std::fs::metadata(path).ok()?.len();
    if size > PICTURE_MAX {
        return None;
    }
    std::fs::read(path).ok()
}

/// An empty answer with status 404.
fn not_found() -> HttpResponse<Cow<'static, [u8]>> {
    let mut response = HttpResponse::new(Cow::Borrowed(&[][..]));
    *response.status_mut() = StatusCode::NOT_FOUND;
    response
}

/// The core's error as a fixed code for the log: never its message, which could quote the mail.
fn error_code(err: &InboxError) -> &'static str {
    match err {
        InboxError::NotFound => "not-found",
        InboxError::Forbidden => "forbidden",
        InboxError::Busy => "busy",
        InboxError::Unsupported => "unsupported",
        InboxError::Settled => "conflict",
        InboxError::Offline => "offline",
        InboxError::Other(_) => "error",
    }
}

/// What the problem page says for `err`: a title and what to do.
fn problem_text(err: &InboxError) -> (&'static str, &'static str) {
    let text = match err {
        InboxError::NotFound | InboxError::Forbidden => {
            tr("The mail is no longer there. It may have been moved or deleted.")
        }
        InboxError::Offline => tr("Torro Workplace isn't responding. It may still be starting."),
        _ => tr("Try again from the inbox."),
    };
    (tr("Couldn't open the mail."), text)
}

/// Reads what the window shows: from the core, or the sample of `--demo`.
fn load(launch: &Launch, runtime: &tokio::runtime::Runtime) -> Shown {
    if launch.demo {
        let design = launch
            .design
            .as_deref()
            .and_then(|id| torro_core::design::load(torro_core::design::dir().as_deref()?, id).ok())
            .unwrap_or_else(Design::standard);
        let appearance = launch.appearance.unwrap_or_default();
        let look = Look::new(&design, appearance);
        let (title, page, parts) = match launch.sample {
            Sample::Newsletter => mail_parts(&demo_newsletter(), &look),
            Sample::Text => mail_parts(&demo_text(), &look),
            Sample::Offline => problem_parts(&InboxError::Offline, &look),
            Sample::Gone => problem_parts(&InboxError::NotFound, &look),
        };
        let pictures = Pictures { parts, source: Source::Demo, fetched: Mutex::default() };
        return Shown { title, page, pictures, appearance, design, zoom: Zoom::DEFAULT };
    }
    let client = InboxClient::default();
    let id = launch.id.clone().unwrap_or_default();
    let (settings, mail) = runtime.block_on(async {
        let settings = tokio::time::timeout(LOAD_WAIT, torro_ipc::client::call(&client.socket, &Request::Settings));
        let settings = match settings.await {
            Ok(Ok(Response::Settings { settings })) => Some(settings),
            _ => None,
        };
        let mail = match tokio::time::timeout(LOAD_WAIT, client.get_mail_html(id.clone())).await {
            Ok(mail) => mail,
            Err(_) => Err(InboxError::Offline),
        };
        (settings, mail)
    });
    let design = settings
        .as_ref()
        .and_then(|settings| torro_core::design::load(torro_core::design::dir().as_deref()?, &settings.design).ok())
        .unwrap_or_else(Design::standard);
    let appearance = launch.appearance.or(settings.as_ref().map(|settings| settings.appearance)).unwrap_or_default();
    let zoom = settings.as_ref().map_or(Zoom::DEFAULT, |settings| settings.zoom);
    let look = Look::new(&design, appearance);
    let (title, page, parts) = match mail {
        Ok(mail) => mail_parts(&mail, &look),
        Err(err) => {
            eprintln!("{BINARY}: couldn't load the mail ({})", error_code(&err));
            problem_parts(&err, &look)
        }
    };
    let pictures = Pictures { parts, source: Source::Core { client, id }, fetched: Mutex::default() };
    Shown { title, page, pictures, appearance, design, zoom }
}

/// Title, page and parts of `mail`.
fn mail_parts(mail: &Message, look: &Look) -> (String, String, Vec<AttachmentInfo>) {
    (page::subject(mail), page::mail_page(mail, look), mail.attachments.clone())
}

/// Title, page and (no) parts of the problem `err`.
fn problem_parts(err: &InboxError, look: &Look) -> (String, String, Vec<AttachmentInfo>) {
    let (title, text) = problem_text(err);
    (tr("Mail").to_string(), page::problem_page(title, text, look), Vec::new())
}

// ---------------------------------------------------------------------------------------------
// The window
// ---------------------------------------------------------------------------------------------

/// What a key or a menu item asks of the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Close,
    ZoomIn,
    ZoomOut,
    ZoomReset,
}

impl Command {
    /// The menu item ids of the macOS menu.
    #[cfg(target_os = "macos")]
    const MENU: [(&'static str, Command); 4] = [
        ("close", Command::Close),
        ("zoom-in", Command::ZoomIn),
        ("zoom-out", Command::ZoomOut),
        ("zoom-reset", Command::ZoomReset),
    ];
}

/// The command of a pressed key: `Esc`, or with the platform's command key (`Ctrl`, ⌘ on macOS)
/// `W`, `+` (also `=`), `-` and `0`. `None` for every other key, which the web view keeps.
fn command(key: &Key<'_>, modifiers: ModifiersState) -> Option<Command> {
    let primary = if cfg!(target_os = "macos") { modifiers.super_key() } else { modifiers.control_key() };
    match key {
        Key::Escape if !primary => Some(Command::Close),
        Key::Character(text) if primary => match text.to_lowercase().as_str() {
            "w" => Some(Command::Close),
            "+" | "=" => Some(Command::ZoomIn),
            "-" => Some(Command::ZoomOut),
            "0" => Some(Command::ZoomReset),
            _ => None,
        },
        _ => None,
    }
}

/// Events from outside the window's own: an item of the macOS menu.
#[derive(Debug)]
enum UserEvent {
    #[cfg_attr(not(target_os = "macos"), allow(dead_code, reason = "only macOS has the menu"))]
    Menu(Command),
}

/// Opens the window on what [`load`] read and runs it until it is closed.
fn run(launch: Launch) -> Result<(), Box<dyn Error>> {
    let runtime = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build()?;
    let shown = load(&launch, &runtime);

    // GTK takes the program name as the window's Wayland app id and X11 class; it must be set before
    // the event loop starts GTK.
    #[cfg(target_os = "linux")]
    gtk::glib::set_prgname(Some(WINDOW_CLASS));
    #[cfg(not(target_os = "linux"))]
    let _ = WINDOW_CLASS;

    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let theme = match shown.appearance {
        Appearance::Light => Some(Theme::Light),
        Appearance::Dark => Some(Theme::Dark),
        Appearance::System => None,
    };
    let window = WindowBuilder::new()
        .with_title(shown.title.clone())
        .with_inner_size(WINDOW)
        .with_min_inner_size(WINDOW_MIN)
        .with_theme(theme)
        .build(&event_loop)?;
    // The panel colour of the mode in effect behind the page, so a dark window does not flash white
    // while the page loads.
    let mode = match window.theme() {
        Theme::Dark => Mode::Dark,
        _ => Mode::Light,
    };
    let panel = shown.design.colors.get(mode).panel;
    let byte = |channel: f32| (channel.clamp(0.0, 1.0) * 255.0).round() as u8;

    let webview = webview_builder(shown.page, Arc::new(shown.pictures), runtime.handle().clone())
        .with_background_color((byte(panel.r), byte(panel.g), byte(panel.b), 255));
    let webview = build(webview, &window)?;
    let mut zoom = shown.zoom;
    webview.zoom(f64::from(zoom.factor()))?;

    #[cfg(target_os = "macos")]
    let _menu = menu(event_loop.create_proxy())?;

    let mut modifiers = ModifiersState::empty();
    event_loop.run(move |event, _, control_flow| {
        // The runtime serves the pictures for as long as the window lives.
        let _ = &runtime;
        *control_flow = ControlFlow::Wait;
        let command = match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => Some(Command::Close),
            Event::WindowEvent { event: WindowEvent::ModifiersChanged(now), .. } => {
                modifiers = now;
                None
            }
            Event::WindowEvent { event: WindowEvent::KeyboardInput { event, .. }, .. }
                if event.state == ElementState::Pressed =>
            {
                command(&event.logical_key, modifiers)
            }
            Event::UserEvent(UserEvent::Menu(command)) => Some(command),
            _ => None,
        };
        let step = match command {
            Some(Command::Close) => {
                *control_flow = ControlFlow::Exit;
                return;
            }
            Some(Command::ZoomIn) => zoom.larger(),
            Some(Command::ZoomOut) => zoom.smaller(),
            Some(Command::ZoomReset) => Zoom::DEFAULT,
            None => return,
        };
        if step != zoom {
            zoom = step;
            if webview.zoom(f64::from(zoom.factor())).is_err() {
                eprintln!("{BINARY}: the zoom could not be changed");
            }
        }
    })
}

/// The web view on `page` with every rule of the window: the page and its pictures through our two
/// protocols, JavaScript off, no navigation away (links to the browser), no new windows, no
/// downloads, no developer tools.
fn webview_builder<'a>(page: String, pictures: Arc<Pictures>, runtime: tokio::runtime::Handle) -> WebViewBuilder<'a> {
    let page = Arc::new(page);
    WebViewBuilder::new()
        .with_javascript_disabled()
        .with_devtools(false)
        .with_clipboard(true)
        // Only Windows reads it: WebView2 zooms with Ctrl + Plus/Minus itself.
        .with_hotkeys_zoom(true)
        .with_back_forward_navigation_gestures(false)
        .with_custom_protocol(page::PAGE_SCHEME.into(), move |_, request| {
            if request.uri().path() != "/" {
                return not_found();
            }
            let mut response = HttpResponse::new(Cow::Owned(page.as_bytes().to_vec()));
            let headers = response.headers_mut();
            headers.insert(CONTENT_TYPE, HeaderValue::from_static("text/html; charset=utf-8"));
            headers.insert(CONTENT_SECURITY_POLICY, HeaderValue::from_static(page::CSP));
            response
        })
        .with_asynchronous_custom_protocol(page::CID_SCHEME.into(), move |_, request, responder| {
            let path = request.uri().path().to_string();
            let pictures = Arc::clone(&pictures);
            runtime.spawn(async move { responder.respond(pictures.answer(&path).await) });
        })
        .with_navigation_handler(|url| match page::navigation(&url) {
            Navigation::Load => true,
            Navigation::Browser => {
                open_outside(&url);
                false
            }
            Navigation::Refuse => false,
        })
        .with_new_window_req_handler(|url, _| {
            if page::navigation(&url) == Navigation::Browser {
                open_outside(&url);
            }
            NewWindowResponse::Deny
        })
        .with_download_started_handler(|_, _| false)
        .with_url(page::PAGE_URL)
}

/// Hands a link to the system's browser or mail program. Never logs the address.
fn open_outside(url: &str) {
    if webbrowser::open(url).is_err() {
        eprintln!("{BINARY}: a link could not be opened");
    }
}

/// The web view in `window`. On Linux inside GTK's box, so it works under Wayland as under X11.
fn build(builder: WebViewBuilder<'_>, window: &tao::window::Window) -> Result<WebView, Box<dyn Error>> {
    #[cfg(target_os = "linux")]
    {
        use tao::platform::unix::WindowExtUnix;
        use wry::WebViewBuilderExtUnix;
        let vbox = window.default_vbox().ok_or("the window has no GTK box for the web view")?;
        Ok(builder.build_gtk(vbox)?)
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(builder.build(window)?)
    }
}

/// The menu bar of macOS. There WKWebView keeps every key it gets, none reaches the window's events
/// ([`command`]): ⌘C and ⌘A work only through the Edit menu's items, and `Esc`, ⌘W and the zoom keys
/// come as menu items too.
#[cfg(target_os = "macos")]
fn menu(proxy: tao::event_loop::EventLoopProxy<UserEvent>) -> Result<muda::Menu, Box<dyn Error>> {
    use muda::accelerator::{Accelerator, Code, Key, KeyAccelerator, Modifiers};
    use muda::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};

    // By the character, not the key's place: ⌘+ is the key with "+" on any layout (on a German keyboard
    // it is not where "=" is on an American one).
    let item = |id: &str, text: &str, character: &str| -> Result<MenuItem, muda::Error> {
        let item = MenuItem::with_id(id, text, true, None);
        item.set_key_accelerator(Some(KeyAccelerator::new(Modifiers::META, Key::Character(character.into()))))?;
        Ok(item)
    };
    let escape =
        MenuItem::with_id("close", tr("Close"), true, Some(Accelerator::new(Modifiers::empty(), Code::Escape)));
    let app = Submenu::with_items(
        "Torro",
        true,
        &[&PredefinedMenuItem::hide(Some(tr("Hide"))), &PredefinedMenuItem::quit(Some(tr("Quit")))],
    )?;
    let edit = Submenu::with_items(
        tr("Edit"),
        true,
        &[&PredefinedMenuItem::copy(Some(tr("Copy"))), &PredefinedMenuItem::select_all(Some(tr("Select all")))],
    )?;
    let view = Submenu::with_items(
        tr("View"),
        true,
        &[
            &item("zoom-in", tr("Zoom in"), "+")?,
            &item("zoom-out", tr("Zoom out"), "-")?,
            &item("zoom-reset", tr("Actual size"), "0")?,
        ],
    )?;
    // A menu item has one key: `Esc` on "Close", ⌘W on "Close window" (both close, as in the other systems).
    let window = Submenu::with_items(
        tr("Window"),
        true,
        &[&escape, &PredefinedMenuItem::close_window(Some(tr("Close window")))],
    )?;
    let menu = Menu::with_items(&[&app, &edit, &view, &window])?;
    menu.init_for_nsapp();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        if let Some((_, command)) = Command::MENU.iter().find(|(id, _)| *id == event.id().0) {
            let _ = proxy.send_event(UserEvent::Menu(*command));
        }
    }));
    Ok(menu)
}

// ---------------------------------------------------------------------------------------------
// The samples of `--demo`
// ---------------------------------------------------------------------------------------------

/// When the samples were sent.
fn demo_date() -> DateTime<Utc> {
    DateTime::from_timestamp(1_791_966_600, 0).unwrap_or_default()
}

/// A sample mail around `html` or `text`.
fn demo_mail(subject: &str, from: (&str, &str), html: Option<&str>, text: &str) -> Message {
    let address = |name: &str, address: &str| MailAddress {
        name: (!name.is_empty()).then(|| name.to_string()),
        address: address.to_string(),
    };
    Message {
        summary: MessageSummary {
            id: "m:demo".into(),
            account: "mail:anna@example.org".into(),
            folder: "f:demo".into(),
            thread_id: "t:demo".into(),
            from: Some(address(from.0, from.1)),
            to: vec![address("Anna Berg", "anna@example.org")],
            subject: subject.into(),
            date: demo_date(),
            snippet: String::new(),
            flags: Vec::new(),
            has_attachments: false,
            invite: None,
            untrusted: true,
        },
        cc: Vec::new(),
        reply_to: Vec::new(),
        text: Some(text.into()),
        from_html: false,
        html: html.map(str::to_string),
        attachments: Vec::new(),
        traits: Default::default(),
        invitation: false,
    }
}

/// An inline picture of the samples.
fn demo_part(part: &str, content_id: &str) -> AttachmentInfo {
    AttachmentInfo {
        part: part.into(),
        filename: None,
        content_type: "image/gif".into(),
        size: 35,
        inline: true,
        content_id: Some(content_id.into()),
    }
}

/// A newsletter as the core hands it out (cleaned): a `<style>` element, nested tables, inline
/// pictures (`cid:`), links, and a PDF attached.
fn demo_newsletter() -> Message {
    let mut mail = demo_mail(
        "Autumn programme: workshops, talks and a new reading room",
        ("Stadtbibliothek Lindenau", "news@bibliothek-lindenau.example"),
        Some(DEMO_NEWSLETTER),
        "Autumn programme at the Stadtbibliothek Lindenau.",
    );
    mail.cc = vec![MailAddress { name: Some("Jonas Weber".into()), address: "jonas@example.org".into() }];
    mail.attachments = vec![
        demo_part("2", "banner@lindenau"),
        demo_part("3", "workshop@lindenau"),
        demo_part("4", "talk@lindenau"),
        AttachmentInfo {
            part: "5".into(),
            filename: Some("Autumn programme 2026.pdf".into()),
            content_type: "application/pdf".into(),
            size: 482_133,
            inline: false,
            content_id: None,
        },
    ];
    mail.summary.has_attachments = true;
    mail
}

/// A plain text mail: no HTML, a link, a quote.
fn demo_text() -> Message {
    demo_mail(
        "Re: Room for Thursday",
        ("Jonas Weber", "jonas@example.org"),
        None,
        "Hi Anna,\n\nthe small meeting room is free on Thursday from 10 to 12. I booked it for us:\n\
         https://rooms.example.org/booking/4711\n\nCould you bring the printouts of the draft? I'll take care of \
         coffee.\n\nBest,\nJonas\n\n> On Tuesday, 13 October 2026 17:02, Anna Berg wrote:\n> Do we have a room \
         for Thursday morning?",
    )
}

/// The colour of each sample picture (a 1×1 GIF, stretched by the page): banner, workshop, talk.
fn demo_picture(part: &str) -> Option<Vec<u8>> {
    let (r, g, b) = match part {
        "2" => (0x2F, 0x5D, 0x50),
        "3" => (0xE8, 0xA3, 0x3D),
        "4" => (0x5B, 0x8C, 0xFF),
        _ => return None,
    };
    // GIF89a, 1 × 1, a palette of two colours, one pixel of colour 0.
    let mut gif = b"GIF89a\x01\x00\x01\x00\x80\x00\x00".to_vec();
    gif.extend([r, g, b, 0, 0, 0]);
    gif.extend(b",\x00\x00\x00\x00\x01\x00\x01\x00\x00\x02\x02D\x01\x00;");
    Some(gif)
}

/// The sample newsletter's HTML, written as the core's cleaning writes it.
const DEMO_NEWSLETTER: &str = r##"<style>
.wrap { background: #f3efe6; padding: 24px 0; }
.card { background: #ffffff; border-radius: 6px; }
h1 { font-family: Georgia, serif; font-size: 28px; line-height: 34px; color: #1d2b26; margin: 0 0 12px; }
h2 { font-family: Georgia, serif; font-size: 19px; color: #1d2b26; margin: 0 0 6px; }
p { font-family: Helvetica, Arial, sans-serif; font-size: 15px; line-height: 23px; color: #3b4440; margin: 0 0 14px; }
.button { display: inline-block; background: #2f5d50; color: #ffffff; padding: 10px 18px; border-radius: 4px;
  text-decoration: none; font-family: Helvetica, Arial, sans-serif; font-weight: bold; font-size: 14px; }
.small { font-size: 12px; line-height: 18px; color: #6b736f; }
</style>
<div class="wrap"><table width="100%" cellpadding="0" cellspacing="0" border="0"><tbody><tr><td align="center">
<table class="card" width="600" cellpadding="0" cellspacing="0" border="0"><tbody>
<tr><td><img src="cid:banner@lindenau" width="600" height="140" alt="Stadtbibliothek Lindenau"
style="display:block;border-radius:6px 6px 0 0"></td></tr>
<tr><td style="padding:28px 32px 8px">
<h1>Autumn at the library</h1>
<p>Dear readers, the days are getting shorter and our programme longer. From November the new reading room on the
second floor is open until 22:00, with forty seats, quiet corners and good light.</p>
<p><a class="button" href="https://bibliothek-lindenau.example/programme">See the whole programme</a></p>
</td></tr>
<tr><td style="padding:8px 32px 24px"><table width="100%" cellpadding="0" cellspacing="0" border="0"><tbody><tr>
<td width="50%" valign="top" style="padding-right:12px">
<img src="cid:workshop@lindenau" width="256" height="120" alt="" style="display:block;border-radius:4px">
<h2 style="margin-top:14px">Bookbinding workshop</h2>
<p>Saturday, 7 November, 10:00–14:00. Bind your own notebook; material is provided.</p>
</td>
<td width="50%" valign="top" style="padding-left:12px">
<img src="cid:talk@lindenau" width="256" height="120" alt="" style="display:block;border-radius:4px">
<h2 style="margin-top:14px">Talk: Cities that read</h2>
<p>Thursday, 19 November, 19:30. How public libraries changed over a century.</p>
</td></tr></tbody></table></td></tr>
<tr><td style="padding:16px 32px 28px;border-top:1px solid #e6e1d6">
<p class="small">You receive this mail because you subscribed to the library's news.
<a href="https://bibliothek-lindenau.example/unsubscribe" style="color:#6b736f">Unsubscribe</a> ·
<a href="mailto:news@bibliothek-lindenau.example" style="color:#6b736f">Write to us</a></p>
</td></tr>
</tbody></table>
</td></tr></tbody></table></div>"##;

#[cfg(test)]
mod tests {
    use super::*;

    fn args(line: &[&str]) -> Vec<String> {
        line.iter().map(|arg| (*arg).to_string()).collect()
    }

    fn parse(line: &[&str]) -> Result<Launch, String> {
        Launch::parse(&args(line), |id| id == "torro" || id == "plex")
    }

    #[test]
    fn the_mail_id_is_a_mail_id_of_the_core() {
        let launch = parse(&["m:0a1b"]).expect("an id");
        assert_eq!((launch.id.as_deref(), launch.demo), (Some("m:0a1b"), false));
        for bad in [&[][..], &["x"], &["m:"], &["m:a b"], &["m:a", "m:b"], &["--unknown"], &["m:a", "--open", "text"]] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
        assert!(parse(&[&format!("m:{}", "a".repeat(MESSAGE_MAX))]).is_err(), "too long");
    }

    #[test]
    fn demo_takes_a_sample_appearance_and_design() {
        let launch = parse(&["--demo", "--open", "text", "--appearance=dark", "--design", "plex"]).expect("demo");
        assert_eq!(
            launch,
            Launch {
                id: None,
                demo: true,
                sample: Sample::Text,
                appearance: Some(Appearance::Dark),
                design: Some("plex".into())
            }
        );
        assert_eq!(parse(&["--demo"]).expect("demo").sample, Sample::Newsletter);
        assert_eq!(parse(&["--demo", "m:x"]).expect("demo with an id").id.as_deref(), Some("m:x"));
        for bad in [
            &["--demo", "--open", "editor"][..],
            &["--demo", "--appearance", "blue"],
            &["--demo", "--design", "gone"],
            &["m:a", "--design", "plex"],
            &["--demo", "--open"],
        ] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn keys_close_and_zoom() {
        let primary = if cfg!(target_os = "macos") { ModifiersState::SUPER } else { ModifiersState::CONTROL };
        let none = ModifiersState::empty();
        assert_eq!(command(&Key::Escape, none), Some(Command::Close));
        assert_eq!(command(&Key::Character("w"), primary), Some(Command::Close));
        assert_eq!(command(&Key::Character("W"), primary), Some(Command::Close));
        assert_eq!(command(&Key::Character("+"), primary), Some(Command::ZoomIn));
        assert_eq!(command(&Key::Character("="), primary), Some(Command::ZoomIn));
        assert_eq!(command(&Key::Character("-"), primary), Some(Command::ZoomOut));
        assert_eq!(command(&Key::Character("0"), primary), Some(Command::ZoomReset));
        // Copying and typing stay the web view's.
        assert_eq!(command(&Key::Character("c"), primary), None);
        assert_eq!(command(&Key::Character("w"), none), None);
    }

    /// Acceptance (log without content): the log names the core's error by its code, never by its
    /// message.
    #[test]
    fn the_log_names_errors_by_code_only() {
        let secret = InboxError::Other("Subject: Salary 2027 from boss@example.org cid:x https://t.example/p".into());
        assert_eq!(error_code(&secret), "error");
        assert_eq!(error_code(&InboxError::NotFound), "not-found");
        assert_eq!(error_code(&InboxError::Offline), "offline");
        let (title, text) = problem_text(&secret);
        assert!(!title.contains("Salary") && !text.contains("Salary"));
    }

    #[test]
    fn problems_say_what_happened() {
        assert_eq!(
            problem_text(&InboxError::Offline).1,
            tr("Torro Workplace isn't responding. It may still be starting.")
        );
        assert_eq!(
            problem_text(&InboxError::NotFound).1,
            tr("The mail is no longer there. It may have been moved or deleted.")
        );
        let look = Look::new(&Design::standard(), Appearance::Light);
        let (title, page, parts) = problem_parts(&InboxError::NotFound, &look);
        assert_eq!(title, tr("Mail"));
        assert!(page.contains(&page::escape(tr("Couldn't open the mail."))));
        assert!(parts.is_empty());
    }

    /// The samples serve their pictures through the same rules as the core's mail.
    #[test]
    fn the_sample_pictures_come_through_the_protocol() {
        let mail = demo_newsletter();
        let runtime = tokio::runtime::Builder::new_current_thread().build().expect("runtime");
        let pictures = Pictures { parts: mail.attachments.clone(), source: Source::Demo, fetched: Mutex::default() };
        for (path, status) in [("/0", 200), ("/1", 200), ("/2", 200), ("/3", 404), ("/9", 404)] {
            let response = runtime.block_on(pictures.answer(path));
            assert_eq!(response.status().as_u16(), status, "{path}");
            if status == 200 {
                assert_eq!(response.headers().get(CONTENT_TYPE).and_then(|v| v.to_str().ok()), Some("image/gif"));
            }
        }
        let page = page::mail_page(&mail, &Look::new(&Design::standard(), Appearance::System));
        assert!(!page.contains("src=\"cid:"), "every sample picture points at the protocol");
    }
}
