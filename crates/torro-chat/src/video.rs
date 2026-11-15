//! YouTube videos in the chat (issue #673, decision 25 in `docs/chat.md`).
//!
//! A link to a YouTube video ([`video_of`]: `youtube.com/watch?v=…`, `youtu.be/…`, Shorts) gives its
//! link card a [`Video`]: the video's id, where it starts and whether it is a Short. The chat window
//! shows such a card as a video card and plays it in a window of its own ([`embed_url`]); the card,
//! and with it the video, only comes where Settings › Privacy lets YouTube's content load.
//!
//! Only the parts of a link that name the video are read; everything else (a playlist, tracking
//! words) is dropped. A link that is no video of YouTube gives `None`, also one whose host only looks
//! like YouTube's (`youtube.com.example.org`, a login in front of the host).
//!
//! No input or output here.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The host the player loads videos from: YouTube's privacy-enhanced mode, which sets no cookies
/// before a person plays the video.
pub const EMBED_HOST: &str = "www.youtube-nocookie.com";

/// The length of a YouTube video id.
const ID_LEN: usize = 11;

/// The latest start a link may ask for, in seconds (a day): beyond it the start is ignored.
const START_MAX: u32 = 24 * 60 * 60;

/// A YouTube video a link names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Video {
    /// The video's id: 11 letters, digits, `-` or `_`.
    pub id: String,
    /// Where the link starts the video, in seconds (`t=90`, `t=1m30s`, `start=90`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<u32>,
    /// A Short (`/shorts/…`): an upright video.
    #[serde(default, skip_serializing_if = "is_false")]
    pub short: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// The video `link` names, when it is one of YouTube's: `http(s)` to `youtube.com`, `www.youtube.com`
/// or `m.youtube.com` with `/watch?v=<id>` or `/shorts/<id>`, or to `youtu.be/<id>`. `None` for any
/// other link and for an id that is not exactly 11 of `[A-Za-z0-9_-]`.
pub fn video_of(link: &str) -> Option<Video> {
    let rest = strip_prefix_ignore_case(link, "https://").or_else(|| strip_prefix_ignore_case(link, "http://"))?;
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (host, rest) = rest.split_at(end);
    // A login or a port in front of the path: no link people send to a video.
    if host.contains(['@', ':']) {
        return None;
    }
    let host = host.to_ascii_lowercase();
    let rest = rest.split('#').next().unwrap_or_default();
    let (path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let start = start_of(query);
    let (id, short) = match host.as_str() {
        "youtu.be" => (path.strip_prefix('/')?.trim_end_matches('/'), false),
        "youtube.com" | "www.youtube.com" | "m.youtube.com" => {
            if let Some(id) = path.strip_prefix("/shorts/") {
                (id.trim_end_matches('/'), true)
            } else if path == "/watch" || path == "/watch/" {
                (param(query, "v")?, false)
            } else {
                return None;
            }
        }
        _ => return None,
    };
    is_id(id).then(|| Video { id: id.to_string(), start, short })
}

/// The address the player loads `video` from: the embedded player of [`EMBED_HOST`], playing at once
/// and inline, suggesting only videos of the same channel at the end, from its start.
pub fn embed_url(video: &Video) -> String {
    let mut url = format!("https://{EMBED_HOST}/embed/{}?autoplay=1&playsinline=1&rel=0", video.id);
    if let Some(start) = video.start {
        url.push_str(&format!("&start={start}"));
    }
    url
}

/// Whether `id` is a video id of YouTube.
fn is_id(id: &str) -> bool {
    id.len() == ID_LEN && id.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// `text` without `prefix`, ignoring the case of ASCII letters.
fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix).then(|| &text[prefix.len()..])
}

/// The value of the first `name=` in `query` (without its `?`); `None` without one.
fn param<'a>(query: &'a str, name: &str) -> Option<&'a str> {
    query.split('&').find_map(|pair| pair.split_once('=').filter(|(key, _)| *key == name).map(|(_, value)| value))
}

/// The start `query` asks for with `t` or `start`; `None` without one, for one that is no time and for
/// one beyond [`START_MAX`].
fn start_of(query: &str) -> Option<u32> {
    let value = param(query, "t").or_else(|| param(query, "start"))?;
    seconds(value).filter(|&start| start <= START_MAX)
}

/// `90`, `90s`, `1m30s` or `1h2m3s` in seconds; `None` for anything else.
fn seconds(value: &str) -> Option<u32> {
    if !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit()) {
        return value.parse().ok();
    }
    let mut total: u32 = 0;
    let mut number = String::new();
    let mut last = 0;
    for c in value.chars() {
        if c.is_ascii_digit() {
            number.push(c);
            continue;
        }
        // Hours, minutes and seconds, each once and in this order.
        let (rank, unit) = match c {
            'h' => (1, 3600),
            'm' => (2, 60),
            's' => (3, 1),
            _ => return None,
        };
        if number.is_empty() || rank <= last {
            return None;
        }
        let amount: u32 = number.parse().ok()?;
        total = total.checked_add(amount.checked_mul(unit)?)?;
        number.clear();
        last = rank;
    }
    (number.is_empty() && last > 0).then_some(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn video(id: &str, start: Option<u32>, short: bool) -> Option<Video> {
        Some(Video { id: id.into(), start, short })
    }

    #[test]
    fn every_form_of_a_youtube_link_names_its_video() {
        let id = "M7lc1UVf-VE";
        for link in [
            "https://www.youtube.com/watch?v=M7lc1UVf-VE",
            "https://youtube.com/watch?v=M7lc1UVf-VE&list=PL123&index=2",
            "https://m.youtube.com/watch?feature=share&v=M7lc1UVf-VE",
            "http://www.youtube.com/watch/?v=M7lc1UVf-VE#comments",
            "HTTPS://WWW.YouTube.com/watch?v=M7lc1UVf-VE",
            "https://youtu.be/M7lc1UVf-VE",
            "https://youtu.be/M7lc1UVf-VE?si=abcdef",
        ] {
            assert_eq!(video_of(link), video(id, None, false), "{link}");
        }
        assert_eq!(video_of("https://www.youtube.com/shorts/abcDEF_12-3"), video("abcDEF_12-3", None, true));
        assert_eq!(video_of("https://youtube.com/shorts/abcDEF_12-3/?feature=share"), video("abcDEF_12-3", None, true));
    }

    #[test]
    fn a_start_comes_from_t_or_start() {
        let id = "M7lc1UVf-VE";
        for (link, start) in [
            ("https://youtu.be/M7lc1UVf-VE?t=90", 90),
            ("https://youtu.be/M7lc1UVf-VE?t=90s", 90),
            ("https://www.youtube.com/watch?v=M7lc1UVf-VE&t=1m30s", 90),
            ("https://www.youtube.com/watch?v=M7lc1UVf-VE&t=1h2m3s", 3723),
            ("https://www.youtube.com/watch?v=M7lc1UVf-VE&start=42", 42),
            ("https://youtu.be/M7lc1UVf-VE?t=2m", 120),
        ] {
            assert_eq!(video_of(link), video(id, Some(start), false), "{link}");
        }
        for bad in ["abc", "1m30", "30s1m", "1x", "", "s", "-5", "99999999999", "1h1h"] {
            let link = format!("https://youtu.be/M7lc1UVf-VE?t={bad}");
            assert_eq!(video_of(&link), video(id, None, false), "{bad:?}: the video without a start");
        }
        assert_eq!(video_of("https://youtu.be/M7lc1UVf-VE?t=90000"), video(id, None, false), "beyond a day");
    }

    #[test]
    fn anything_else_is_no_video() {
        for link in [
            "https://www.youtube.com/watch?v=M7lc1UVf-V",
            "https://www.youtube.com/watch?v=M7lc1UVf-VEx",
            "https://www.youtube.com/watch?v=M7lc1UVf+VE",
            "https://www.youtube.com/watch?list=PL123",
            "https://www.youtube.com/@channel",
            "https://www.youtube.com/embed/M7lc1UVf-VE",
            "https://youtu.be/",
            "https://youtu.be/M7lc1UVf-VE/more",
            "https://youtube.com.example.org/watch?v=M7lc1UVf-VE",
            "https://example.org/youtube.com/watch?v=M7lc1UVf-VE",
            "https://www.youtube.com@example.org/watch?v=M7lc1UVf-VE",
            "https://www.youtube.com:8443/watch?v=M7lc1UVf-VE",
            "https://music.youtube.com/watch?v=M7lc1UVf-VE",
            "ftp://youtu.be/M7lc1UVf-VE",
            "javascript:alert(1)//youtu.be/M7lc1UVf-VE",
            "youtu.be/M7lc1UVf-VE",
        ] {
            assert_eq!(video_of(link), None, "{link}");
        }
    }

    #[test]
    fn the_player_loads_the_privacy_enhanced_embed() {
        let plain = Video { id: "M7lc1UVf-VE".into(), start: None, short: false };
        assert_eq!(
            embed_url(&plain),
            "https://www.youtube-nocookie.com/embed/M7lc1UVf-VE?autoplay=1&playsinline=1&rel=0"
        );
        let later = Video { start: Some(90), ..plain.clone() };
        assert!(embed_url(&later).ends_with("&rel=0&start=90"));
    }

    #[test]
    fn empty_fields_are_left_out_on_the_wire() {
        let plain = Video { id: "M7lc1UVf-VE".into(), start: None, short: false };
        assert_eq!(serde_json::to_string(&plain).expect("json"), r#"{"id":"M7lc1UVf-VE"}"#);
        let back: Video = serde_json::from_str(r#"{"id":"M7lc1UVf-VE"}"#).expect("read");
        assert_eq!(back, plain);
    }
}
