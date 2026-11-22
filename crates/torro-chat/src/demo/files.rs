//! Files and media of the sample (stage 4): the shared files of "Design sync" ([`DESIGN`]) – two
//! pictures, a video with a preview and one without, a PDF, a voice message, a file that may not be
//! downloaded, an SVG without a preview, a link to the account's own server with its card, a
//! foreign link whose card comes only with "Show once" (issue #672) and a YouTube video with its video
//! card (issue #673), and a message of four links (issue #784): a GitHub issue, a pull request and a file of the own
//! server with their cards, the fourth, a page, only as text – and what the demo does with files:
//! prepared files and uploads in memory, sendings that advance a quarter with each look
//! ([`Files::progress`]), pictures drawn on the spot ([`picture`]).
//!
//! Nothing is read from or written to disk: a path names the file, the demo makes up its size.

use std::collections::BTreeMap;

use chrono::{DateTime, Duration};

use super::{ME, text};
use crate::files::{PreparedFile, PreviewSize, SharedItem, SharedKind, UploadProgress, UploadState};
use crate::model::{
    FileRef, ForeignProvider, GithubState, LinkCard, LinkCardKind, LinkFile, LinkGithub, Message, MessageKind,
};
use crate::source::ChatError;

/// The group "Design sync": the conversation of the sample's files and media.
pub const DESIGN: &str = "nextcloud:anna@cloud.example.org/design4sync";

/// The longer side of a large picture of the demo: enough for the viewer, quick to draw.
const LARGE_EDGE: u32 = 800;

/// The longer side of a picture in the history (`chat.get_preview` medium): smaller than the real
/// 1024, so the demo stays quick to draw, yet sharp at the history's largest size.
const MEDIUM_EDGE: u32 = 640;

/// The longer side of a small picture (`chat.get_preview` small).
const SMALL_EDGE: u32 = 256;

/// The sample's file `id` with `name`, `size` and kind, a preview when `preview`.
fn file(id: &str, name: &str, size: u64, content_type: &str, preview: bool, edges: Option<(u32, u32)>) -> FileRef {
    FileRef {
        id: id.into(),
        name: name.into(),
        size,
        content_type: content_type.into(),
        preview,
        hide_download: false,
        width: edges.map(|(width, _)| width),
        height: edges.map(|(_, height)| height),
        blurhash: None,
    }
}

/// The messages 3 to 18 of [`DESIGN`], four hours to a little over three before `now`: older than
/// the sample's other recent messages, so the order of the conversations stays as it was. Gesa's
/// four pictures sent at once (13 to 16) the chat window shows as one grid; her next message is a
/// YouTube video with its video card, as YouTube allowed in Settings › Privacy gives it (17). Elif's
/// last (18, issue #784) names an issue, a pull request, a file of the own server and a page: three cards, the
/// page only as a link in the text.
pub(super) fn design_media(now: DateTime<chrono::Utc>) -> Vec<Message> {
    let ago = |minutes: i64| now - Duration::minutes(minutes);
    let shared = |number: u64, user: &str, name: &str, minutes: i64, caption: &str, file: FileRef| {
        let mut message = text(DESIGN, number, user, name, ago(minutes), caption);
        message.files = vec![file];
        message
    };
    let gesa = "Gesa Lange";
    let elif = "Elif Aydin";
    let mut voice = shared(
        8,
        "elif",
        elif,
        214,
        "Voice message.ogg",
        file("5108", "Voice message.ogg", 48_213, "audio/ogg", false, None),
    );
    voice.kind = MessageKind::Voice;
    let mut brand = shared(
        9,
        "gesa",
        gesa,
        208,
        "Brand book.pdf",
        file("5109", "Brand book.pdf", 8_734_112, "application/pdf", true, None),
    );
    brand.files[0].hide_download = true;
    let mut palette =
        text(DESIGN, 10, ME, "Anna Berg", ago(202), "The palette is here: https://cloud.example.org/s/palette4icons");
    palette.cards = vec![LinkCard {
        url: "https://cloud.example.org/s/palette4icons".into(),
        title: "Palette for the new icons".into(),
        description: Some("Shared folder · 12 files".into()),
        domain: "cloud.example.org".into(),
        image: true,
        video: None,
        ..LinkCard::default()
    }];
    let mut grids =
        text(DESIGN, 11, "gesa", gesa, ago(198), "A good read on icon grids: https://example.com/blog/icon-grids");
    // Settings › Privacy at "Ask" (issue #672): the window asks before the card of another server loads.
    grids.foreign = Some(ForeignProvider { key: "example.com".into(), name: "example.com".into() });
    let intro_link = "https://youtu.be/M7lc1UVf-VE";
    let mut intro = text(DESIGN, 17, "gesa", gesa, ago(190), &format!("Our intro: {intro_link}"));
    // YouTube allowed (issue #673): the card carries the video, its picture as the one of message 10.
    intro.cards = vec![LinkCard {
        url: intro_link.into(),
        title: "Torro Workplace in two minutes".into(),
        description: None,
        domain: "youtu.be".into(),
        image: true,
        video: crate::video::video_of(intro_link),
        ..LinkCard::default()
    }];
    let links = four_links(now - Duration::minutes(186));
    vec![
        shared(
            3,
            "gesa",
            gesa,
            240,
            "First drafts of the app icons.",
            file("5103", "Icons draft.png", 1_284_096, "image/png", true, Some((1600, 1000))),
        ),
        shared(
            4,
            "elif",
            elif,
            236,
            "Sketch.jpg",
            file("5104", "Sketch.jpg", 412_640, "image/jpeg", true, Some((900, 1200))),
        ),
        shared(
            5,
            "gesa",
            gesa,
            230,
            "Short walkthrough",
            file("5105", "Walkthrough.mp4", 18_874_368, "video/mp4", true, Some((1920, 1080))),
        ),
        shared(
            6,
            "elif",
            elif,
            226,
            "Screen recording.mov",
            file("5106", "Screen recording.mov", 90_177_536, "video/quicktime", false, None),
        ),
        shared(
            7,
            "gesa",
            gesa,
            220,
            "Icon guide.pdf",
            file("5107", "Icon guide.pdf", 2_516_582, "application/pdf", false, None),
        ),
        voice,
        brand,
        palette,
        grids,
        shared(12, "elif", elif, 196, "logo.svg", file("5112", "logo.svg", 12_288, "image/svg+xml", false, None)),
        shared(
            13,
            "gesa",
            gesa,
            195,
            "Header A.png",
            file("5113", "Header A.png", 964_812, "image/png", true, Some((1600, 900))),
        ),
        shared(
            14,
            "gesa",
            gesa,
            195,
            "Header B.png",
            file("5114", "Header B.png", 812_302, "image/png", true, Some((1200, 1200))),
        ),
        shared(
            15,
            "gesa",
            gesa,
            195,
            "Header C.jpg",
            file("5115", "Header C.jpg", 402_771, "image/jpeg", true, Some((900, 1200))),
        ),
        shared(
            16,
            "gesa",
            gesa,
            195,
            "Options for the new header",
            file("5116", "Header D.png", 1_102_448, "image/png", true, Some((1600, 1000))),
        ),
        intro,
        links,
    ]
}

/// The links of message 18 of [`DESIGN`] (issue #784), in the order they stand in its text.
pub(super) const FOUR_LINKS: [&str; 4] = [
    "https://github.com/torro/workplace/issues/142",
    "https://github.com/torro/workplace/pull/143",
    "https://cloud.example.org/f/5120",
    "https://example.com/blog/icon-grids",
];

/// Message 18 of [`DESIGN`] at `at` (issue #784): four links, the first three with their cards – the issue and the
/// pull request as the connected GitHub account sees them, the file as the own server names it; at most three cards
/// per message, so the page stays a link.
fn four_links(at: DateTime<chrono::Utc>) -> Message {
    let [issue, pull, file, page] = FOUR_LINKS;
    let words = format!("See {issue} and the fix {pull}, the file is {file}, background: {page}");
    let mut message = text(DESIGN, 18, "elif", "Elif Aydin", at, &words);
    let github = |url: &str, index: u8, number: u64, title: &str, is_pull: bool, state: GithubState| LinkCard {
        url: url.into(),
        index,
        title: title.into(),
        domain: "github.com".into(),
        kind: LinkCardKind::Github,
        github: Some(Box::new(LinkGithub { repo: "torro/workplace".into(), number, is_pull, state })),
        ..LinkCard::default()
    };
    message.cards = vec![
        github(issue, 0, 142, "Calendar shows the wrong week after midnight", false, GithubState::Open),
        github(pull, 1, 143, "Fix the week after midnight", true, GithubState::Merged),
        LinkCard {
            url: file.into(),
            index: 2,
            title: "Icon grid.pdf".into(),
            domain: "cloud.example.org".into(),
            image: true,
            kind: LinkCardKind::NextcloudFile,
            file: Some(Box::new(LinkFile {
                name: "Icon grid.pdf".into(),
                size: 2_202_009,
                content_type: "application/pdf".into(),
                folder: "Projects/Icons".into(),
                id: None,
            })),
            ..LinkCard::default()
        },
    ];
    message
}

/// The card "Show once" brings for a foreign link of the sample (`chat.show_card`).
pub(super) fn foreign_card(url: &str) -> Option<LinkCard> {
    url.starts_with("https://example.com/").then(|| LinkCard {
        url: url.into(),
        title: "Icon grids that hold up".into(),
        description: Some("Why a 24-pixel grid with 2 pixels of padding works for line icons.".into()),
        domain: "example.com".into(),
        image: false,
        video: None,
        ..LinkCard::default()
    })
}

/// The first link of `text`, `http` or `https`, up to the next space.
pub(super) fn first_link(text: &str) -> Option<&str> {
    let start = text.find("https://").or_else(|| text.find("http://"))?;
    let rest = &text[start..];
    Some(rest.split_whitespace().next().unwrap_or(rest))
}

/// What the sample's shared items of `messages` are, the newest first: a file by its message and
/// kind, a link by its card.
pub(super) fn shared_of(messages: &[Message]) -> Vec<SharedItem> {
    let mut items = Vec::new();
    for message in messages.iter().rev() {
        for file in &message.files {
            let kind = if message.kind == MessageKind::Voice {
                SharedKind::Voice
            } else if file.content_type.starts_with("image/") || file.content_type.starts_with("video/") {
                SharedKind::Media
            } else if file.content_type.starts_with("audio/") {
                SharedKind::Audio
            } else {
                SharedKind::File
            };
            items.push(SharedItem {
                message: message.id.clone(),
                kind,
                file: Some(file.id.clone()),
                name: file.name.clone(),
                size: file.size,
                content_type: file.content_type.clone(),
                author: message.author.name.clone(),
                at: message.at,
                preview: file.preview,
            });
        }
        for card in &message.cards {
            items.push(SharedItem {
                message: message.id.clone(),
                kind: SharedKind::Link,
                file: None,
                name: card.url.clone(),
                size: 0,
                content_type: String::new(),
                author: message.author.name.clone(),
                at: message.at,
                preview: card.image,
            });
        }
    }
    items
}

/// A sending of the demo: what goes, and how far it is.
#[derive(Debug)]
pub(super) struct Upload {
    pub conversation: String,
    pub text: String,
    pub reply_to: Option<String>,
    pub files: Vec<PreparedFile>,
    pub progress: UploadProgress,
    /// It never moves: the state `--open upload` shows.
    pub stalled: bool,
}

/// What the demo holds of files: prepared ones, unfinished uploads, sendings.
#[derive(Debug, Default)]
pub(super) struct Files {
    pub prepared: BTreeMap<String, PreparedFile>,
    /// Unfinished uploads: their name and the bytes so far.
    pub pieces: BTreeMap<String, (Option<String>, Vec<u8>)>,
    pub uploads: BTreeMap<String, Upload>,
    pub next: u64,
}

impl Files {
    /// A new id with `prefix`.
    pub fn id(&mut self, prefix: &str) -> String {
        self.next += 1;
        format!("{prefix}-{}", self.next)
    }

    /// One look at the sending `id`: it moves a quarter on (sharing once all bytes are up, sent at
    /// the next look) – unless it is stalled or over. `true` in the answer's second part when it was
    /// sent just now, so the caller adds the messages.
    pub fn progress(&mut self, id: &str) -> Result<(UploadProgress, bool), ChatError> {
        let upload = self.uploads.get_mut(id).ok_or(ChatError::NotFound)?;
        let progress = &mut upload.progress;
        let mut sent_now = false;
        if !upload.stalled {
            match progress.state {
                UploadState::Uploading => {
                    progress.sent = (progress.sent + (progress.total / 4).max(1)).min(progress.total);
                    if progress.sent >= progress.total {
                        progress.state = UploadState::Sharing;
                    }
                }
                UploadState::Sharing => {
                    progress.state = UploadState::Sent;
                    sent_now = true;
                }
                UploadState::Sent | UploadState::Cancelled | UploadState::Failed => {}
            }
        }
        Ok((progress.clone(), sent_now))
    }
}

/// What a file of the computer is, by its name: the demo reads no file.
pub(super) fn kind_of_name(name: &str) -> (&'static str, u64) {
    let extension = name.rsplit_once('.').map(|(_, ext)| ext.to_ascii_lowercase()).unwrap_or_default();
    match extension.as_str() {
        "png" => ("image/png", 846_212),
        "jpg" | "jpeg" => ("image/jpeg", 1_203_334),
        "gif" => ("image/gif", 402_113),
        "webp" => ("image/webp", 302_448),
        "pdf" => ("application/pdf", 2_516_582),
        "mp4" => ("video/mp4", 24_117_248),
        "mov" => ("video/quicktime", 31_457_280),
        "ogg" | "mp3" | "m4a" => ("audio/mpeg", 1_048_576),
        _ => ("application/octet-stream", 65_536),
    }
}

/// What uploaded bytes are, by their first bytes, and a PNG's pixels from its header.
pub(super) fn kind_of_bytes(bytes: &[u8]) -> (&'static str, Option<(u32, u32)>) {
    const PNG: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    if bytes.starts_with(&PNG) && bytes.len() >= 24 {
        let read = |at: usize| u32::from_be_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]]);
        return ("image/png", Some((read(16), read(20))));
    }
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        return ("image/jpeg", None);
    }
    if bytes.starts_with(b"%PDF") {
        return ("application/pdf", None);
    }
    ("application/octet-stream", None)
}

/// `width` × `height` fitted into a square of `edge`, keeping the shape; at least one pixel each.
pub(super) fn fitted(width: u32, height: u32, edge: u32) -> (u32, u32) {
    let longer = width.max(height).max(1);
    if longer <= edge {
        return (width.max(1), height.max(1));
    }
    let scale = |side: u32| ((u64::from(side) * u64::from(edge) / u64::from(longer)) as u32).max(1);
    (scale(width), scale(height))
}

/// The edge of a picture of `size`.
pub(super) fn edge(size: PreviewSize) -> u32 {
    match size {
        PreviewSize::Small => SMALL_EDGE,
        PreviewSize::Medium => MEDIUM_EDGE,
        PreviewSize::Large => LARGE_EDGE,
    }
}

/// A picture of the sample as PNG, `width` × `height`: a soft diagonal blend of two colours picked by
/// `seed`, with a round mark and a band, so pictures differ and a preview reads as a picture.
pub fn picture(seed: u64, width: u32, height: u32) -> Vec<u8> {
    const COLOURS: [[u8; 3]; 6] =
        [[91, 140, 255], [63, 185, 132], [232, 163, 61], [217, 108, 168], [167, 139, 250], [72, 170, 190]];
    let first = COLOURS[(seed % 6) as usize];
    let second = COLOURS[((seed / 6 + seed + 2) % 6) as usize];
    let (w, h) = (width.max(1), height.max(1));
    let mut data = Vec::with_capacity((w * h * 3) as usize);
    let (cx, cy, r) = (f64::from(w) * 0.32, f64::from(h) * 0.42, f64::from(w.min(h)) * 0.22);
    for y in 0..h {
        for x in 0..w {
            let t = (f64::from(x) / f64::from(w) + f64::from(y) / f64::from(h)) / 2.0;
            let mut pixel = [0u8; 3];
            for (channel, value) in pixel.iter_mut().enumerate() {
                let blend = f64::from(first[channel]) * (1.0 - t) + f64::from(second[channel]) * t;
                *value = blend as u8;
            }
            let (dx, dy) = (f64::from(x) - cx, f64::from(y) - cy);
            if dx * dx + dy * dy < r * r {
                pixel = pixel.map(|value| value.saturating_add(60));
            }
            let band = f64::from(y) > f64::from(h) * 0.72 && f64::from(y) < f64::from(h) * 0.8;
            if band && f64::from(x) > f64::from(w) * 0.55 {
                pixel = pixel.map(|value| value / 2 + 110);
            }
            data.extend_from_slice(&pixel);
        }
    }
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, w, h);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let written = encoder.write_header().and_then(|mut writer| writer.write_image_data(&data));
    if written.is_err() {
        return Vec::new();
    }
    out
}

/// A seed for the pictures of a file: its id's digits, so each file keeps its look.
pub(super) fn seed_of(id: &str) -> u64 {
    id.bytes().fold(7u64, |seed, byte| seed.wrapping_mul(31).wrapping_add(u64::from(byte)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pictures_are_pngs_of_the_size_asked_for() {
        let png = picture(3, 40, 25);
        assert_eq!(kind_of_bytes(&png), ("image/png", Some((40, 25))));
        assert_eq!(fitted(1600, 1000, 256), (256, 160));
        assert_eq!(fitted(100, 50, 256), (100, 50));
        assert!(
            edge(PreviewSize::Small) < edge(PreviewSize::Medium)
                && edge(PreviewSize::Medium) < edge(PreviewSize::Large)
        );
        assert_eq!(DESIGN, crate::model::conversation_id(super::super::ACCOUNT, "design4sync"));
    }

    #[test]
    fn the_first_link_ends_at_a_space() {
        assert_eq!(first_link("see https://a.org/x now"), Some("https://a.org/x"));
        assert_eq!(first_link("no link"), None);
    }
}
