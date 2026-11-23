//! The compose window as a window with the system's webview (issue #969): a tao window with a wry webview that
//! shows the page of `assets/compose/` (the editor Squire, DOMPurify, no network) and the bridge between that page
//! and the [`Session`].
//!
//! - **Window:** tao + wry, WKWebView on macOS, WebKitGTK (`webkit2gtk-4.1`) on Linux, WebView2 on Windows. The
//!   window class is `torro-compose`. Everything is shown by the one page; the Rust side draws nothing.
//! - **The page's files** come from the program itself through a custom protocol (`torro://localhost/…`, on
//!   Windows `http://torro.localhost/…`): the page, Squire, DOMPurify, the font, and what the session puts into
//!   its [`Store`] (previews of pictures prepared by the core, portraits of contacts). Nothing is read from the
//!   disk or the network at run time. The page's Content-Security-Policy allows scripts, styles, fonts and
//!   pictures from its own origin only and no connections at all; navigation away from the page and new windows
//!   are refused (links go to the system's browser through the bridge, only `http`, `https` and `mailto`).
//! - **The bridge:** the page sends JSON lines (`window.ipc.postMessage`), only from the page's own origin,
//!   at most [`MAX_MESSAGE`] bytes, parsed to [`FromPage`]; anything that does not parse is dropped. The session
//!   answers with events that the window evaluates as `window.torroCompose.receive(<json>)`.
//! - **Files dropped on the window** come from the native drag-and-drop of the webview (the page does not see file
//!   drops itself: on Wayland and in WebKit the DOM gets no file, only an address). Only plain local files count;
//!   the window tells the page where they fell and the page decides (on the text a picture is embedded, anywhere
//!   else everything is attached).
//! - **Linux with NVIDIA:** WebKitGTK then crashes under Wayland at every start (`Missing acquire timeline`, see
//!   `docs/research.md` section 9). [`relaunch`] starts the window again with `WEBKIT_DISABLE_DMABUF_RENDERER=1`;
//!   setting a variable in the running program would need `unsafe`.
//! - **The session runs on its own thread** with a small Tokio runtime: the event loop of the window must not wait
//!   for the core.

use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tao::dpi::LogicalSize;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::WindowBuilder;
use torro_core::i18n::tr;
use torro_gui::source::Source;
use torro_gui::theme::Palette;
use wry::http::{Request, Response, header::CONTENT_TYPE};
use wry::{DragDropEvent, WebViewBuilder};

#[cfg(target_os = "linux")]
use tao::platform::unix::WindowBuilderExtUnix;
#[cfg(target_os = "linux")]
use tao::platform::unix::WindowExtUnix;
#[cfg(target_os = "linux")]
use wry::WebViewBuilderExtUnix;

use super::session::{Emit, FromPage, Session, Store};
use super::{ComposeSource, Start};

/// The most the page may send in one message: a picture of 25 MiB as Base64 and some room.
pub const MAX_MESSAGE: usize = 40 * 1024 * 1024;

/// How often the session's clock runs (the autosave looks at it).
const CLOCK: Duration = Duration::from_secs(1);

/// The size a compose window opens at: a letter, not the inbox's three columns.
const WINDOW: (f64, f64) = (760.0, 700.0);

/// The address the page and everything it loads has: `torro://localhost` (Windows: `http://torro.localhost`).
const ORIGIN: &str = if cfg!(windows) { "http://torro.localhost" } else { "torro://localhost" };

/// The Content-Security-Policy of the page: its own origin for scripts, fonts and pictures, inline styles (the
/// editor sets some), no connections, no frames, no forms, no base.
const POLICY: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self'; \
                      font-src 'self'; connect-src 'none'; frame-src 'none'; object-src 'none'; form-action 'none'; \
                      base-uri 'none'; frame-ancestors 'none'";

/// Why the window did not open.
#[derive(Debug)]
pub enum WebError {
    /// The system has no webview the program can use (on Windows: WebView2 is missing or too old).
    Webview(String),
    /// The window could not be created.
    Window(String),
}

impl std::fmt::Display for WebError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WebError::Webview(why) => write!(f, "{} {why}", tr("The compose window needs the system's web view.")),
            WebError::Window(why) => write!(f, "{} {why}", tr("The compose window could not be opened.")),
        }
    }
}

impl std::error::Error for WebError {}

/// What the event loop is told from other threads.
enum UserEvent {
    /// The session tells the page something.
    Page(Value),
    /// Files fell on the window, or are over it.
    Drag(Value),
}

/// What the session thread is told.
enum Message {
    Page(Box<FromPage>),
    /// The window's close button.
    CloseRequested,
}

/// A file of the compose page, and what it is.
fn asset(path: &str) -> Option<(&'static str, &'static [u8])> {
    Some(match path {
        "/" | "/index.html" => ("text/html; charset=utf-8", include_bytes!("../../assets/compose/compose.html")),
        "/compose.css" => ("text/css; charset=utf-8", include_bytes!("../../assets/compose/compose.css")),
        "/compose.js" => ("text/javascript; charset=utf-8", include_bytes!("../../assets/compose/compose.js")),
        "/squire.js" => ("text/javascript; charset=utf-8", include_bytes!("../../assets/compose/squire.js")),
        "/purify.min.js" => ("text/javascript; charset=utf-8", include_bytes!("../../assets/compose/purify.min.js")),
        "/fonts/Inter-Regular.ttf" => ("font/ttf", include_bytes!("../../../torro-gui/assets/fonts/Inter-Regular.ttf")),
        "/fonts/Inter-Medium.ttf" => ("font/ttf", include_bytes!("../../../torro-gui/assets/fonts/Inter-Medium.ttf")),
        "/fonts/Inter-SemiBold.ttf" => {
            ("font/ttf", include_bytes!("../../../torro-gui/assets/fonts/Inter-SemiBold.ttf"))
        }
        _ => return None,
    })
}

/// The answer to a request of the page: its own files, and the store of the session; anything else is a 404.
fn serve(path: &str, store: &Store) -> (u16, &'static str, Vec<u8>) {
    if let Some((kind, bytes)) = asset(path) {
        return (200, kind, bytes.to_vec());
    }
    let png = |found: Option<Vec<u8>>| match found {
        Some(bytes) => (200, "image/png", bytes),
        None => (404, "text/plain", Vec::new()),
    };
    if let Some(cid) = path.strip_prefix("/img/")
        && torro_mail::html::valid_cid(cid)
    {
        return png(store.image(cid));
    }
    if let Some(key) = path.strip_prefix("/photo/")
        && key.len() <= 12
        && key.bytes().all(|c| c.is_ascii_alphanumeric())
    {
        return png(store.photo(key));
    }
    (404, "text/plain", Vec::new())
}

fn respond(request: Request<Vec<u8>>, store: &Store) -> Response<Vec<u8>> {
    let (status, kind, body) = serve(request.uri().path(), store);
    let mut builder = Response::builder()
        .status(status)
        .header(CONTENT_TYPE, kind)
        .header("X-Content-Type-Options", "nosniff")
        .header("Cache-Control", "no-store");
    if kind.starts_with("text/html") {
        builder = builder.header("Content-Security-Policy", POLICY);
    }
    builder.body(body).unwrap_or_default()
}

/// A colour as CSS.
fn css(color: iced::Color) -> String {
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("rgba({},{},{},{:.3})", byte(color.r), byte(color.g), byte(color.b), color.a)
}

/// The design's colours for the page, as CSS variables for light and dark.
fn palette() -> Value {
    let vars = |p: &Palette| {
        json!({
            "--w-panel": css(p.panel), "--w-raised": css(p.raised), "--w-hover": css(p.hover),
            "--w-sel": css(p.selected), "--w-menu": css(p.menu), "--w-text": css(p.text),
            "--w-text2": css(p.text_2), "--w-text3": css(p.text_3), "--w-line": css(p.line),
            "--w-line2": css(p.line_strong), "--w-line3": css(p.line_selected), "--w-focus": css(p.focus_ring),
            "--w-acc": css(p.accent), "--w-on-acc": css(p.on_accent), "--w-err": css(p.error_text),
            "--w-ok": css(p.ok), "--w-attn": css(p.attention_text), "--w-blue": css(p.unread),
            "--w-scrim": css(p.scrim),
        })
    };
    json!({ "light": vars(&Palette::light()), "dark": vars(&Palette::dark()) })
}

/// The English texts of the page, each in the program's language. The page asks for them by their English words.
fn strings() -> Value {
    macro_rules! texts {
        ($($text:literal),* $(,)?) => {{
            let mut map = serde_json::Map::new();
            $(map.insert($text.to_owned(), Value::String(tr($text).to_owned()));)*
            Value::Object(map)
        }};
    }
    texts![
        "As attachment",
        "Attach files",
        "Bcc",
        "Bold",
        "Bulleted list",
        "Cancel",
        "Cc",
        "Clear formatting",
        "Close",
        "Contact",
        "Contacts",
        "Discard changes",
        "Discard draft",
        "Discard this draft?",
        "Dismiss",
        "Draft saved",
        "From",
        "From mail, not in contacts",
        "Hide formatting",
        "Insert image",
        "It moves to the trash.",
        "Italic",
        "Keep writing",
        "Large",
        "Link",
        "Medium",
        "Numbered list",
        "Original",
        "Quote",
        "Redo",
        "Remove",
        "Retry",
        "Save draft",
        "Save this draft?",
        "Saving…",
        "Send",
        "Sending…",
        "Show formatting",
        "Show history",
        "Hide history",
        "Small",
        "Subject",
        "Switch to dark paper",
        "Switch to light paper",
        "To",
        "Underline",
        "Undo",
        "Unsaved changes",
        "Write your message…",
        "Your latest changes aren't saved yet.",
        "Loading…",
        "Signature",
        "Remove link",
        "Apply",
        "https://",
        "Drop files to attach them",
        "Drop to embed the picture here",
        "This isn't a valid address.",
        "Image",
        "Remove the attachment",
    ]
}

// ---------------------------------------------------------------------------------------------
// Linux with NVIDIA
// ---------------------------------------------------------------------------------------------

/// Starts the program again with the variable that keeps WebKitGTK from crashing on NVIDIA under Wayland, when
/// that is the case and the variable is not set yet; waits for it and returns its exit code. `None` everywhere
/// else: go on in this process.
#[cfg(target_os = "linux")]
pub fn relaunch() -> Option<ExitCode> {
    const VARIABLE: &str = "WEBKIT_DISABLE_DMABUF_RENDERER";
    if std::env::var_os(VARIABLE).is_some() || !std::path::Path::new("/proc/driver/nvidia/version").exists() {
        return None;
    }
    let exe = std::env::current_exe().ok()?;
    let status = std::process::Command::new(exe).args(std::env::args_os().skip(1)).env(VARIABLE, "1").status().ok()?;
    Some(status.code().map_or(ExitCode::FAILURE, |code| ExitCode::from(code as u8)))
}

#[cfg(not(target_os = "linux"))]
pub fn relaunch() -> Option<ExitCode> {
    None
}

// ---------------------------------------------------------------------------------------------
// The window
// ---------------------------------------------------------------------------------------------

/// Whether `path` is where a request of the page may come from: our own origin.
fn own_origin(uri: &str) -> bool {
    uri == ORIGIN || uri.strip_prefix(ORIGIN).is_some_and(|rest| rest.starts_with('/'))
}

/// Opens the compose window on `source` and runs until it closes.
pub fn run<S: ComposeSource + 'static>(source: Arc<S>, suite: Source, start: Start) -> Result<(), WebError> {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let title = start.kind.title();
    let builder = WindowBuilder::new()
        .with_title(title)
        .with_inner_size(LogicalSize::new(WINDOW.0, WINDOW.1))
        .with_min_inner_size(LogicalSize::new(320.0, 320.0));
    #[cfg(target_os = "linux")]
    let builder = builder.with_name(super::COMPOSE.window_class, super::COMPOSE.window_class);
    let window = builder.build(&event_loop).map_err(|err| WebError::Window(err.to_string()))?;

    let store = Arc::new(Store::default());
    let (to_session, from_page) = tokio::sync::mpsc::unbounded_channel::<Message>();

    // Files dropped on the window: the page learns where, the page decides.
    let dragging = Arc::new(AtomicBool::new(false));
    let drag_proxy = proxy.clone();
    let drag_store = store.clone();
    let drag = move |event: DragDropEvent| -> bool {
        let local = |paths: &[PathBuf]| !paths.is_empty() && paths.iter().all(|path| super::images::local_file(path));
        match event {
            DragDropEvent::Enter { paths, position } if local(&paths) => {
                dragging.store(true, Ordering::Relaxed);
                let _ = drag_proxy
                    .send_event(UserEvent::Drag(json!({ "e": "drag", "on": true, "x": position.0, "y": position.1 })));
                true
            }
            DragDropEvent::Over { position } if dragging.load(Ordering::Relaxed) => {
                let _ = drag_proxy
                    .send_event(UserEvent::Drag(json!({ "e": "drag", "on": true, "x": position.0, "y": position.1 })));
                true
            }
            DragDropEvent::Drop { paths, position } if dragging.swap(false, Ordering::Relaxed) && local(&paths) => {
                let paths: Vec<String> = paths.iter().map(|path| path.to_string_lossy().into_owned()).collect();
                // The page may name these files, once, and no others.
                drag_store.allow_drop(&paths);
                let _ = drag_proxy.send_event(UserEvent::Drag(
                    json!({ "e": "drop", "paths": paths, "x": position.0, "y": position.1 }),
                ));
                true
            }
            DragDropEvent::Leave if dragging.swap(false, Ordering::Relaxed) => {
                let _ = drag_proxy.send_event(UserEvent::Drag(json!({ "e": "drag", "on": false })));
                true
            }
            // A picture dragged inside the text, or anything that is no local file: the page deals with it.
            _ => false,
        }
    };

    let ipc_sender = to_session.clone();
    let protocol_store = store.clone();
    let webview = WebViewBuilder::new()
        .with_url(format!("{ORIGIN}/index.html"))
        .with_custom_protocol("torro".into(), move |_id, request| respond(request, &protocol_store).map(Into::into))
        .with_ipc_handler(move |request| {
            if !own_origin(&request.uri().to_string()) || request.body().len() > MAX_MESSAGE {
                return;
            }
            if debug() {
                eprintln!("torro-compose page> {}", request.body().chars().take(300).collect::<String>());
            }
            match serde_json::from_str::<FromPage>(request.body()) {
                Ok(message) => {
                    let _ = ipc_sender.send(Message::Page(Box::new(message)));
                }
                Err(err) => eprintln!("torro-compose: dropped a message of the page: {err}"),
            }
        })
        .with_navigation_handler(|url| own_origin(&url))
        .with_drag_drop_handler(drag)
        .with_new_window_req_handler(|_url, _features| wry::NewWindowResponse::Deny);

    #[cfg(target_os = "linux")]
    let built = match window.default_vbox() {
        Some(vbox) => webview.build_gtk(vbox),
        None => return Err(WebError::Window("the window has no GTK box".into())),
    };
    #[cfg(not(target_os = "linux"))]
    let built = webview.build(&window);
    let webview = built.map_err(|err| WebError::Webview(err.to_string()))?;

    // The session, on a thread of its own.
    let emit_proxy = proxy.clone();
    let emit: Emit = Arc::new(move |event| {
        let _ = emit_proxy.send_event(UserEvent::Page(event));
    });
    let thread_store = store.clone();
    let session_thread = std::thread::Builder::new()
        .name("torro-compose-session".into())
        .spawn(move || {
            let Ok(runtime) = tokio::runtime::Builder::new_multi_thread().worker_threads(2).enable_all().build() else {
                eprintln!("torro-compose: no runtime for the session");
                return;
            };
            runtime.block_on(run_session(source, suite, start, thread_store, emit, from_page));
        })
        .map_err(|err| WebError::Window(err.to_string()))?;

    let close_sender = to_session;
    let mut closing = false;
    // Kept alive as long as the window: a dropped menu is gone from the menu bar. Without it the keys stay dead.
    #[cfg(target_os = "macos")]
    let menu = menu();
    event_loop.run(move |event, _, flow| {
        #[cfg(target_os = "macos")]
        let _ = &menu;
        *flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {
                // The session decides: at once, or after a question. A window the session never answered (the
                // page did not load) closes on the second try.
                if closing {
                    *flow = ControlFlow::Exit;
                }
                closing = true;
                let _ = close_sender.send(Message::CloseRequested);
            }
            Event::UserEvent(UserEvent::Page(event)) => {
                match event["e"].as_str() {
                    Some("close") => *flow = ControlFlow::Exit,
                    Some("status") => {
                        closing = false;
                        if let Some(title) = event["title"].as_str() {
                            window.set_title(title);
                        }
                    }
                    _ => {}
                }
                deliver(&webview, &event);
            }
            Event::UserEvent(UserEvent::Drag(event)) => deliver(&webview, &event),
            Event::LoopDestroyed => {
                // The session ends with its channel; nothing waits for the core any longer.
                let _ = &session_thread;
            }
            _ => {}
        }
    })
}

/// The menu bar of macOS: WKWebView gets ⌘C, ⌘V, ⌘X, ⌘A and ⌘Z only as menu items (the same as the video window of
/// torrochat), and ⌘W and ⌘Q work as menu items too. Closing asks the window to close (`CloseRequested`).
#[cfg(target_os = "macos")]
fn menu() -> Result<muda::Menu, muda::Error> {
    use muda::{Menu, PredefinedMenuItem, Submenu};
    let app = Submenu::with_items(
        "Torro",
        true,
        &[&PredefinedMenuItem::hide(Some(tr("Hide"))), &PredefinedMenuItem::quit(Some(tr("Quit")))],
    )?;
    let edit = Submenu::with_items(
        tr("Edit"),
        true,
        &[
            &PredefinedMenuItem::undo(Some(tr("Undo"))),
            &PredefinedMenuItem::redo(Some(tr("Redo"))),
            &PredefinedMenuItem::separator(),
            &PredefinedMenuItem::cut(Some(tr("Cut"))),
            &PredefinedMenuItem::copy(Some(tr("Copy"))),
            &PredefinedMenuItem::paste(Some(tr("Paste"))),
            &PredefinedMenuItem::select_all(Some(tr("Select all"))),
        ],
    )?;
    let window =
        Submenu::with_items(tr("Window"), true, &[&PredefinedMenuItem::close_window(Some(tr("Close window")))])?;
    let menu = Menu::with_items(&[&app, &edit, &window])?;
    menu.init_for_nsapp();
    Ok(menu)
}

/// `TORRO_COMPOSE_DEBUG=1` writes every message of the bridge to stderr (cut to 300 characters), for finding out what
/// the page and the session say to each other.
fn debug() -> bool {
    std::env::var_os("TORRO_COMPOSE_DEBUG").is_some()
}

/// Hands `event` to the page.
fn deliver(webview: &wry::WebView, event: &Value) {
    if debug() {
        eprintln!("torro-compose page< {}", event.to_string().chars().take(300).collect::<String>());
    }
    // `serde_json` escapes everything a script literal needs; U+2028 and U+2029 are valid in script strings since
    // ES2019, which every webview of the three systems has.
    let script = format!("window.torroCompose&&window.torroCompose.receive({event});");
    let _ = webview.evaluate_script(&script);
}

/// The session: waits for the messages of the page, ticks, and answers.
async fn run_session<S: ComposeSource + 'static>(
    source: Arc<S>,
    suite: Source,
    start: Start,
    store: Arc<Store>,
    emit: Emit,
    mut from_page: tokio::sync::mpsc::UnboundedReceiver<Message>,
) {
    let appearance = torro_gui::theme::stored_appearance(&suite).await.unwrap_or_default();
    let mut session = Session::new(source, start, store, emit.clone());
    let mut clock = tokio::time::interval(CLOCK);
    loop {
        tokio::select! {
            message = from_page.recv() => {
                let Some(message) = message else { return };
                match message {
                    Message::Page(message) => {
                        if matches!(*message, FromPage::Ready) {
                            emit(json!({
                                "e": "theme",
                                "appearance": match appearance {
                                    torro_core::theme::Appearance::System => "system",
                                    torro_core::theme::Appearance::Light => "light",
                                    torro_core::theme::Appearance::Dark => "dark",
                                },
                                "palette": palette(),
                                "strings": strings(),
                                "mac": cfg!(target_os = "macos"),
                            }));
                        }
                        session.handle(*message).await;
                    }
                    Message::CloseRequested => session.handle(FromPage::Close).await,
                }
            }
            _ = clock.tick() => session.tick(Instant::now()).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_is_served_with_its_policy_and_unknown_paths_are_404() {
        let store = Store::default();
        let (status, kind, body) = serve("/index.html", &store);
        assert_eq!((status, kind), (200, "text/html; charset=utf-8"));
        assert!(String::from_utf8_lossy(&body).contains("compose.js"));
        for path in ["/compose.css", "/compose.js", "/squire.js", "/purify.min.js", "/fonts/Inter-Regular.ttf"] {
            assert_eq!(serve(path, &store).0, 200, "{path}");
        }
        for path in ["/", "/../Cargo.toml", "/img/", "/img/../x", "/photo/", "/etc/passwd", "/fonts/../x"] {
            let answer = serve(path, &store).0;
            assert!(answer == 404 || path == "/", "{path}: {answer}");
        }
        assert_eq!(serve("/img/unknown@torro", &store).0, 404, "a picture the session does not know");
        assert!(POLICY.contains("connect-src 'none'") && POLICY.contains("script-src 'self'"));
        assert!(!POLICY.contains("unsafe-eval") && !POLICY.contains("script-src 'self' 'unsafe-inline'"));
    }

    #[test]
    fn the_store_serves_what_the_session_put_in_and_only_valid_names() {
        let store = Store::default();
        store.put_image("a@torro", vec![1, 2, 3]);
        store.put_photo("p0", vec![4]);
        assert_eq!(serve("/img/a@torro", &store), (200, "image/png", vec![1, 2, 3]));
        assert_eq!(serve("/photo/p0", &store), (200, "image/png", vec![4]));
        assert_eq!(serve("/img/a b", &store).0, 404, "no white space in a Content-ID");
        assert_eq!(serve("/photo/p0%2e", &store).0, 404);
    }

    #[test]
    fn only_the_own_origin_may_talk_to_the_bridge_and_be_navigated_to() {
        assert!(own_origin(&format!("{ORIGIN}/index.html")));
        assert!(own_origin(ORIGIN));
        assert!(!own_origin("https://example.org/"));
        assert!(!own_origin(&format!("{ORIGIN}.evil.example/")));
        assert!(!own_origin("file:///etc/passwd"));
        assert!(!own_origin("javascript:alert(1)"));
    }

    #[test]
    fn every_text_the_page_asks_for_is_in_the_list_and_nothing_else_is() {
        let known = strings();
        let known = known.as_object().expect("an object");
        let page = include_str!("../../assets/compose/compose.js");
        let html = include_str!("../../assets/compose/compose.html");
        let mut asked: Vec<String> = Vec::new();
        for (source, open, close) in [
            (page, "T(\"", "\")"),
            (html, "data-t=\"", "\""),
            (html, "data-title=\"", "\""),
            (html, "data-ph=\"", "\""),
        ] {
            let mut rest = source;
            while let Some(at) = rest.find(open) {
                rest = &rest[at + open.len()..];
                if let Some(end) = rest.find(close) {
                    asked.push(rest[..end].to_owned());
                }
            }
        }
        assert!(asked.len() > 20, "the scan finds the page's texts ({} found)", asked.len());
        for text in known.keys() {
            assert!(asked.contains(text), "strings() holds {text:?}, which the page never asks for");
        }
        for text in &asked {
            assert!(known.contains_key(text), "the page asks for {text:?}, which strings() does not hold");
        }
    }

    #[test]
    fn the_palette_has_the_variables_the_stylesheet_uses() {
        let palette = palette();
        let css = include_str!("../../assets/compose/compose.css");
        for mode in ["light", "dark"] {
            let vars = palette[mode].as_object().expect("variables");
            for name in vars.keys() {
                assert!(css.contains(name.as_str()), "{name} is set for {mode} but the stylesheet never reads it");
            }
            assert!(vars["--w-text"].as_str().is_some_and(|c| c.starts_with("rgba(")));
        }
    }
}
