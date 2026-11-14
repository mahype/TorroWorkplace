//! The player window of a YouTube video (`torrochat --player=<message>`, class `torro-chat-player`,
//! issue #673, decision 25 of `docs/chat.md`): the video of the message's card in YouTube's embedded
//! player, in a floating window of its own like "Picture large".
//!
//! Iced draws no web page, so the window is a tao window with the system's web view (wry: WKWebView on
//! macOS, WebView2 on Windows; [`window`]). Under Linux there is no player: WebKitGTK is not part of the
//! suite, and the chat window opens the video at YouTube in the browser instead.
//!
//! What may play is what the core says ([`playable`]): the window reads the message with
//! `chat.get_messages` `around` it, and plays only a video on a card the core gave, which it does only
//! where Settings › Privacy lets YouTube's content load (or after "Show once"). The player loads only
//! YouTube's privacy-enhanced embed of that one video ([`allowed_navigation`]); every other address
//! goes to the browser.
//!
//! This file holds what the window decides, on every system, so it is tested everywhere; the window
//! itself is [`window`].

#![cfg_attr(
    not(any(target_os = "macos", target_os = "windows")),
    allow(dead_code, reason = "the player window runs on macOS and Windows; its rules are tested everywhere")
)]

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub mod window;

use torro_chat::{LinkCard, MessagePage, Video};
use torro_core::i18n::tr;

/// The longest window title, in characters: the video's title, cut with "…".
pub const TITLE_MAX: usize = 100;

/// The smallest player: still a picture one can make out.
pub const MIN_SIZE: (f64, f64) = (320.0, 180.0);

/// Why the window plays nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerError {
    /// The message is not on the page the core answered.
    NotFound,
    /// The message has no video card: Settings › Privacy does not let YouTube load, or its link is
    /// no video.
    NotAllowed,
}

/// The card and the video of the message `id` on `page`, as the core gave them.
pub fn playable(page: &MessagePage, id: &str) -> Result<(LinkCard, Video), PlayerError> {
    let message = page.messages.iter().find(|message| message.id == id).ok_or(PlayerError::NotFound)?;
    let card = message.cards.first().ok_or(PlayerError::NotAllowed)?;
    let video = card.video.clone().ok_or(PlayerError::NotAllowed)?;
    Ok((card.clone(), video))
}

/// Whether the player may load `url` for `video`: only the embed of this video at YouTube's
/// privacy-enhanced host (with any parameters) and the empty page the player starts from. Every other
/// address, a click on the YouTube logo or a suggested video, goes to the browser instead.
pub fn allowed_navigation(url: &str, video: &Video) -> bool {
    if url == "about:blank" {
        return true;
    }
    let embed = format!("https://{}/embed/{}", torro_chat::video::EMBED_HOST, video.id);
    match url.strip_prefix(&embed) {
        Some(rest) => rest.is_empty() || rest.starts_with('?') || rest.starts_with('#'),
        None => false,
    }
}

/// The size the window opens at, logical: 640 × 360, a Short upright 360 × 640.
pub fn window_size(video: &Video) -> (f64, f64) {
    if video.short { (360.0, 640.0) } else { (640.0, 360.0) }
}

/// The window's title: the video's title, at most [`TITLE_MAX`] characters, else "YouTube video".
pub fn title(card: &LinkCard) -> String {
    let title = card.title.split_whitespace().collect::<Vec<_>>().join(" ");
    if title.is_empty() {
        return tr("YouTube video").to_owned();
    }
    if title.chars().count() <= TITLE_MAX {
        return title;
    }
    let cut: String = title.chars().take(TITLE_MAX - 1).collect();
    format!("{}…", cut.trim_end())
}

/// What the window says instead of the video for `err`.
pub fn refusal(err: PlayerError) -> &'static str {
    match err {
        PlayerError::NotAllowed => {
            tr("This video may not load. Allow YouTube in Settings › Privacy, or use Show once in the chat.")
        }
        PlayerError::NotFound => tr("This video is no longer in the chat."),
    }
}

/// The page that says `text` in the window instead of the video: our own HTML, nothing loads (no
/// script, no picture, no font), the text escaped.
pub fn note_page(text: &str) -> String {
    let escaped = text
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;");
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'\">\
         <style>html,body{{margin:0;height:100%;background:#000;color:#b4b8bf;\
         font:14px/22px system-ui,-apple-system,'Segoe UI',sans-serif}}\
         body{{display:flex;align-items:center;justify-content:center;text-align:center}}\
         p{{max-width:28em;padding:0 24px}}</style></head><body><p>{escaped}</p></body></html>"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use torro_chat::demo::{DESIGN, DemoSource};
    use torro_chat::{ChatSource, message_id};

    fn video(id: &str, short: bool) -> Video {
        Video { id: id.into(), start: None, short }
    }

    #[test]
    fn only_the_embed_of_this_video_loads() {
        let intro = video("M7lc1UVf-VE", false);
        for url in [
            "about:blank",
            "https://www.youtube-nocookie.com/embed/M7lc1UVf-VE",
            "https://www.youtube-nocookie.com/embed/M7lc1UVf-VE?autoplay=1&playsinline=1&rel=0&start=90",
        ] {
            assert!(allowed_navigation(url, &intro), "{url}");
        }
        for url in [
            "https://www.youtube-nocookie.com/embed/abcDEF_12-3?autoplay=1",
            "https://www.youtube-nocookie.com/embed/M7lc1UVf-VEx",
            "https://www.youtube-nocookie.com/embed/M7lc1UVf-VE/../other",
            "https://www.youtube.com/watch?v=M7lc1UVf-VE",
            "https://www.youtube-nocookie.com.example.org/embed/M7lc1UVf-VE",
            "http://www.youtube-nocookie.com/embed/M7lc1UVf-VE",
            "https://accounts.google.com/",
            "about:srcdoc",
            "",
        ] {
            assert!(!allowed_navigation(url, &intro), "{url}");
        }
    }

    #[test]
    fn a_short_stands_upright() {
        assert_eq!(window_size(&video("M7lc1UVf-VE", false)), (640.0, 360.0));
        assert_eq!(window_size(&video("abcDEF_12-3", true)), (360.0, 640.0));
    }

    #[test]
    fn the_core_decides_what_plays() {
        let demo = DemoSource::default();
        let run = |future| iced::futures::executor::block_on(future);
        let page = run(demo.messages_around(DESIGN.into(), message_id(DESIGN, 17), None)).expect("around");
        let (card, played) = playable(&page, &message_id(DESIGN, 17)).expect("the demo's video");
        assert_eq!(played.id, "M7lc1UVf-VE");
        assert_eq!(title(&card), card.title);
        // A link card without a video, a message without a card, one not on the page.
        let page = run(demo.messages_around(DESIGN.into(), message_id(DESIGN, 10), None)).expect("around");
        assert_eq!(playable(&page, &message_id(DESIGN, 10)), Err(PlayerError::NotAllowed));
        assert_eq!(playable(&page, &message_id(DESIGN, 11)), Err(PlayerError::NotAllowed));
        assert_eq!(playable(&page, &message_id(DESIGN, 99)), Err(PlayerError::NotFound));
    }

    #[test]
    fn the_title_is_the_videos_in_one_line() {
        let card = |title: &str| LinkCard {
            url: "https://youtu.be/M7lc1UVf-VE".into(),
            title: title.into(),
            description: None,
            domain: "youtu.be".into(),
            image: false,
            video: Some(video("M7lc1UVf-VE", false)),
        };
        assert_eq!(title(&card("  An\nintro  ")), "An intro");
        assert_eq!(title(&card(" ")), tr("YouTube video"));
        let long = title(&card(&"x".repeat(300)));
        assert!(long.ends_with('…') && long.chars().count() == TITLE_MAX);
    }

    #[test]
    fn the_note_page_loads_nothing_and_escapes_its_text() {
        let page = note_page("<script>alert('x')</script> & more");
        assert!(page.contains("default-src 'none'"));
        assert!(page.contains("&lt;script&gt;alert(&#39;x&#39;)&lt;/script&gt; &amp; more"));
        assert!(!page.contains("<script>"));
    }
}
