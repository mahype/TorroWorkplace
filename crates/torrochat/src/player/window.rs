//! The player window itself (macOS and Windows, issue #673): a tao window with the system's web view
//! (wry) on YouTube's embedded player of one video, black, without developer tools.
//!
//! - **One window per video:** a second start on the same message brings this window to the front
//!   and ends ([`torro_gui::instance::claim_named_with`], `player:<message>`).
//! - **What plays** comes from the core ([`super::playable`]); the demo plays nothing and says so.
//! - **Only the embed of this video loads** ([`super::allowed_navigation`]); a link out of the player
//!   to YouTube itself (the YouTube logo, a suggested video) goes to the browser, every other address
//!   is refused. Nothing stays between starts (incognito).
//! - **YouTube wants to know who embeds its player** (else "Error 153"): the page is asked for with the
//!   suite's address as `Referer` ([`referer`]).
//! - Without a web view (WebView2 missing) the video opens at YouTube in the browser, exit code 1.
//!
//! Keys: on macOS ⌘W closes and ⌘Q quits through the menu bar ([`menu`]), where WKWebView keeps every
//! other key; on Windows the window's own close button and Alt+F4.

use std::process::ExitCode;
use std::time::Duration;

use tao::dpi::{LogicalPosition, LogicalSize};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::{Theme, WindowBuilder};
use torro_chat::{ChatError, Video};
use torro_core::i18n::tr;
use torro_gui::instance::Claim;
use wry::http::{HeaderMap, HeaderValue, header::REFERER};
use wry::{NewWindowResponse, Rect, WebView, WebViewBuilder};

use super::{
    MIN_SIZE, PlayerError, allowed_navigation, leads_to_youtube, note_page, playable, refusal, title, window_size,
};
use crate::app::CONTACTS;
use crate::backend::Backend;
use crate::system;

/// The program's name in the log.
const BINARY: &str = "torrochat";

/// How long the window waits for the core's answer before it says the video cannot be shown.
const LOAD_WAIT: Duration = Duration::from_secs(15);

/// Events from outside the window's own.
#[derive(Debug, Clone, Copy)]
enum UserEvent {
    /// A second start on the same video: to the front.
    Show,
}

/// What the window shows.
enum Shown {
    /// The video, with the link of its card for the browser.
    Video { title: String, link: String, video: Video },
    /// A sentence instead of the video.
    Note(&'static str),
}

/// The address the player is asked for from: the suite's bundle id as a web origin, which YouTube
/// takes to know who embeds its player.
fn referer() -> String {
    format!("https://{}/", torro_core::app::WORKPLACE_BUNDLE_ID)
}

/// The player window of the video on the card of the message `message`, on the core or the demo,
/// until it is closed.
pub fn run(message: String, demo: bool) -> ExitCode {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();
    if !demo {
        let proxy = event_loop.create_proxy();
        let name = format!("player:{message}");
        let claim = torro_gui::instance::claim_named_with(CONTACTS, &name, move || {
            let _ = proxy.send_event(UserEvent::Show);
        });
        if claim == Claim::Shown {
            return ExitCode::SUCCESS;
        }
    }
    let shown = if demo { Shown::Note(tr("The demo plays no videos.")) } else { load(&message) };
    let (window_title, size, link) = match &shown {
        Shown::Video { title, link, video } => (title.clone(), window_size(video), Some(link.clone())),
        Shown::Note(_) => {
            (tr("YouTube video").to_owned(), window_size(&Video { id: String::new(), start: None, short: false }), None)
        }
    };
    let window = WindowBuilder::new()
        .with_title(window_title)
        .with_inner_size(LogicalSize::new(size.0, size.1))
        .with_min_inner_size(LogicalSize::new(MIN_SIZE.0, MIN_SIZE.1))
        .with_theme(Some(Theme::Dark))
        .build(&event_loop);
    let window = match window {
        Ok(window) => window,
        Err(err) => return without_player(&link, &err.to_string()),
    };
    let webview = match build(webview_builder(&shown), &window) {
        Ok(webview) => webview,
        Err(err) => return without_player(&link, &err.to_string()),
    };
    #[cfg(target_os = "macos")]
    let _menu = match menu() {
        Ok(menu) => Some(menu),
        Err(err) => {
            eprintln!("{BINARY}: the player has no menu: {err}");
            None
        }
    };
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => *control_flow = ControlFlow::Exit,
            Event::WindowEvent { event: WindowEvent::Resized(size), .. } => {
                let bounds = Rect { position: LogicalPosition::new(0.0, 0.0).into(), size: size.into() };
                if webview.set_bounds(bounds).is_err() {
                    eprintln!("{BINARY}: the player could not follow the window's size");
                }
            }
            Event::UserEvent(UserEvent::Show) => {
                window.set_minimized(false);
                window.set_focus();
            }
            _ => {}
        }
    })
}

/// What the core says about the message `message`: its video, or why there is none.
fn load(message: &str) -> Shown {
    let Some((conversation, _)) = torro_chat::split_message(message) else {
        return Shown::Note(refusal(PlayerError::NotFound));
    };
    let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(runtime) => runtime,
        Err(_) => return Shown::Note(unavailable()),
    };
    let around = Backend::core().messages_around(conversation.to_owned(), message.to_owned());
    // The timer is made inside the runtime: outside it there is no reactor for it.
    let page = match runtime.block_on(async { tokio::time::timeout(LOAD_WAIT, around).await }) {
        Ok(Ok(page)) => page,
        Ok(Err(ChatError::NotFound)) => return Shown::Note(refusal(PlayerError::NotFound)),
        Ok(Err(err)) => {
            // The kind only: never a text of the conversation in the log.
            eprintln!("{BINARY}: the player could not read the message ({})", error_code(&err));
            return Shown::Note(unavailable());
        }
        Err(_) => return Shown::Note(unavailable()),
    };
    match playable(&page, message) {
        Ok((card, video)) => Shown::Video { title: title(&card), link: card.url, video },
        Err(err) => Shown::Note(refusal(err)),
    }
}

/// The sentence for every problem but a missing or refused video: no core, offline, no server.
fn unavailable() -> &'static str {
    tr("This video cannot be shown right now.")
}

/// `err` as a fixed word for the log.
fn error_code(err: &ChatError) -> &'static str {
    match err {
        ChatError::NotFound => "not-found",
        ChatError::Unsupported(_) => "unsupported",
        ChatError::Offline => "offline",
        ChatError::NotConnected => "not-connected",
        _ => "error",
    }
}

/// The web view of `shown` with every rule of the window: autoplay, no developer tools, no swiping
/// back, nothing kept between starts (incognito: what YouTube stores goes with the window), only the
/// embed of the video, no downloads. An address of YouTube itself the player leads to (its logo, a
/// suggested video) goes to the browser; any other is only refused, so a frame of the page that loads
/// elsewhere never opens a browser tab by itself.
fn webview_builder<'a>(shown: &Shown) -> WebViewBuilder<'a> {
    let builder = WebViewBuilder::new()
        .with_incognito(true)
        .with_autoplay(true)
        .with_devtools(false)
        .with_back_forward_navigation_gestures(false)
        .with_background_color((0, 0, 0, 255))
        .with_download_started_handler(|_, _| false);
    match shown {
        Shown::Video { video, .. } => {
            let allowed = video.clone();
            let mut headers = HeaderMap::new();
            if let Ok(value) = HeaderValue::from_str(&referer()) {
                headers.insert(REFERER, value);
            }
            builder
                .with_navigation_handler(move |url| {
                    if allowed_navigation(&url, &allowed) {
                        return true;
                    }
                    to_browser(&url);
                    false
                })
                .with_new_window_req_handler(|url, _| {
                    to_browser(&url);
                    NewWindowResponse::Deny
                })
                .with_url_and_headers(torro_chat::embed_url(video), headers)
        }
        Shown::Note(text) => builder
            .with_navigation_handler(|url| url == "about:blank")
            .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
            .with_html(note_page(text)),
    }
}

/// Hands `url` to the browser when it is an address of YouTube itself ([`super::leads_to_youtube`]);
/// never logs it.
fn to_browser(url: &str) {
    if leads_to_youtube(url) && system::open_link(url).is_err() {
        eprintln!("{BINARY}: a link of the player could not be opened");
    }
}

/// The web view in `window`.
fn build(builder: WebViewBuilder<'_>, window: &tao::window::Window) -> Result<WebView, wry::Error> {
    builder.build(window)
}

/// There is no player (no window, no web view, `why`): the video opens at YouTube in the browser
/// when there is one, and the program ends with exit code 1.
fn without_player(link: &Option<String>, why: &str) -> ExitCode {
    eprintln!("{BINARY}: the player cannot open: {why}");
    if let Some(link) = link
        && system::open_link(link).is_err()
    {
        eprintln!("{BINARY}: the video could not be opened in the browser");
    }
    ExitCode::FAILURE
}

/// The menu bar of macOS: WKWebView keeps every key, so ⌘W ("Close window") and ⌘Q ("Quit") work only
/// as menu items. Both act on their own: closing asks the window to close (`CloseRequested`).
#[cfg(target_os = "macos")]
fn menu() -> Result<muda::Menu, muda::Error> {
    use muda::{Menu, PredefinedMenuItem, Submenu};
    let app = Submenu::with_items(
        "Torro",
        true,
        &[&PredefinedMenuItem::hide(Some(tr("Hide"))), &PredefinedMenuItem::quit(Some(tr("Quit")))],
    )?;
    let window =
        Submenu::with_items(tr("Window"), true, &[&PredefinedMenuItem::close_window(Some(tr("Close window")))])?;
    let menu = Menu::with_items(&[&app, &window])?;
    menu.init_for_nsapp();
    Ok(menu)
}
