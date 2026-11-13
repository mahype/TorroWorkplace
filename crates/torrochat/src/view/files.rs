//! How files look in a chat window (stage 4 of `docs/chat.md`, draft "Dateien, Bilder und Videos" in
//! `docs/prototypes/chat/`): a picture as its preview, a video as its preview with a play mark, a
//! voice message with its play button, every other file – and every fallback without a preview –
//! as a line with its kind, name, size and the buttons "Open", "Download" and "Open in Nextcloud";
//! link cards under a message; the files chosen for sending and the progress above the composer;
//! the drop zone over the window.
//!
//! A preview stands in the size of the picture from the first frame on (a calm placeholder until it
//! came), so nothing jumps; it is at most [`size::sidebar()`] on its longer side.

use iced::widget::text::Wrapping;
use iced::widget::{Space, column, container, image, mouse_area, row, stack};
use iced::{Alignment, Background, Color, ContentFit, Element, Fill, Theme, border, mouse};
use torro_chat::{FileRef, ForeignProvider, LinkCard, Message as ChatMessage, MessageKind};
use torro_core::i18n::{tr, trf};
use torro_core::privacy::ProviderChoice;
use torro_gui::icon::{self, Icon};
use torro_gui::theme::{self, palette, radius, size, space};
use torro_gui::widgets::{self, TileFace, TileSize};
use torro_gui::{style, typo, units};

use super::{HAIRLINE, tinted};
use crate::app::files::{Attached, Busy, CardAsk, FilesMsg, PictureKey, Shown, Upload, wrap};
use crate::app::{App, ChatState, Message};
use crate::system;
use crate::timeline;

/// How a file of a message shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Look {
    /// Its preview, a click shows it large.
    Picture,
    /// Its preview with a play mark, a click opens it in the system's player.
    Video,
    /// A voice message: play button, name, size.
    Voice,
    /// A line: kind, name, size, buttons.
    Line,
}

/// A file the server draws a preview of: a picture (not SVG) or a video, with `preview`.
pub(crate) fn has_preview(file: &FileRef) -> bool {
    file.preview
        && ((file.content_type.starts_with("image/") && file.content_type != "image/svg+xml")
            || file.content_type.starts_with("video/"))
}

/// How `file` of `message` shows: as its preview while the server has one (and drew it), else as a
/// line.
pub fn look(message: &ChatMessage, file: &FileRef, shown: Option<&Shown>) -> Look {
    if message.kind == MessageKind::Voice {
        return Look::Voice;
    }
    if !has_preview(file) || matches!(shown, Some(Shown::Missing)) {
        return Look::Line;
    }
    if file.content_type.starts_with("video/") { Look::Video } else { Look::Picture }
}

/// The word of a file's kind ("Picture", "PDF"), from the media type the server names.
pub fn kind_word(content_type: &str) -> &'static str {
    match content_type {
        kind if kind.starts_with("image/") => tr("Picture"),
        kind if kind.starts_with("video/") => tr("Video"),
        kind if kind.starts_with("audio/") => tr("Audio"),
        "application/pdf" => tr("PDF"),
        _ => tr("File"),
    }
}

/// The icon of a file's kind.
pub fn kind_icon(content_type: &str) -> Icon {
    match content_type {
        kind if kind.starts_with("image/") => Icon::Image,
        kind if kind.starts_with("video/") => Icon::Video,
        kind if kind.starts_with("audio/") => Icon::Play,
        _ => Icon::FileText,
    }
}

/// `width` × `height` fitted into a square of `edge`; without pixels a 4 : 3 frame.
pub fn fitted(width: Option<u32>, height: Option<u32>, edge: f32) -> (f32, f32) {
    match (width, height) {
        (Some(w), Some(h)) if w > 0 && h > 0 => {
            let (w, h) = (w as f32, h as f32);
            let scale = (edge / w.max(h)).min(1.0);
            // Whole pixels, so the frame does not blur its edge.
            ((w * scale).round(), (h * scale).round())
        }
        _ => (edge, edge * 3.0 / 4.0),
    }
}

/// The calm stand-in of a picture until it came: `selected`, radius `sm`, its kind's icon.
fn placeholder<'a>(which: Icon, width: f32, height: f32) -> Element<'a, Message> {
    container(tinted(which, icon::SIZE, |p| p.text_3))
        .width(width)
        .height(height)
        .center(Fill)
        .style(|t: &Theme| container::Style {
            background: Some(Background::Color(palette(t).selected)),
            border: border::rounded(radius::sm()),
            ..container::Style::default()
        })
        .into()
}

/// `shown` in `width` × `height`, filling the frame, or the placeholder with `which`.
fn picture<'a>(shown: Option<&Shown>, which: Icon, width: f32, height: f32) -> Element<'a, Message> {
    match shown {
        Some(Shown::Ready(handle)) => image(handle.clone())
            .width(width)
            .height(height)
            .content_fit(ContentFit::Cover)
            .border_radius(radius::sm())
            .into(),
        _ => placeholder(which, width, height),
    }
}

/// The play mark over a video's preview: a round `menu` disc with a line, the play icon in `text`.
pub(super) fn play_mark<'a>() -> Element<'a, Message> {
    container(tinted(Icon::Play, icon::SIZE, |p| p.text))
        .center(size::control_sm())
        .style(|t: &Theme| {
            let p = palette(t);
            container::Style {
                background: Some(Background::Color(p.menu)),
                border: border::rounded(size::control_sm() / 2.0).color(p.line_strong).width(HAIRLINE),
                ..container::Style::default()
            }
        })
        .into()
}

/// `content` that sends `on_press` on a click, with the pointer of a link and `tip` as its tooltip.
pub(super) fn clickable<'a>(content: Element<'a, Message>, on_press: Message, tip: String) -> Element<'a, Message> {
    widgets::with_tooltip(mouse_area(content).on_press(on_press).interaction(mouse::Interaction::Pointer), tip)
}

/// "1.2 MB · PDF", with "no preview" and "view only" where they hold.
fn file_meta(file: &FileRef, no_preview: bool) -> String {
    let mut parts = vec![units::format_bytes(file.size), kind_word(&file.content_type).to_owned()];
    if no_preview {
        parts.push(tr("no preview").to_owned());
    }
    if file.hide_download {
        parts.push(tr("view only").to_owned());
    }
    parts.join(" · ")
}

impl App {
    /// One file of a message in the history, at most `room` wide.
    pub(super) fn file_part<'a>(
        &'a self,
        chat: &'a ChatState,
        message: &'a ChatMessage,
        file: &'a FileRef,
        room: f32,
    ) -> Element<'a, Message> {
        let key: PictureKey = (message.id.clone(), Some(file.id.clone()));
        let shown = chat.files.picture(&key);
        let (id, file_id) = (message.id.clone(), file.id.clone());
        match look(message, file, shown) {
            Look::Picture => {
                let (w, h) = fitted(file.width, file.height, size::sidebar().min(room));
                let view = wrap(FilesMsg::View(id, file_id));
                clickable(picture(shown, Icon::Image, w, h), view, trf("Show {name} large", &[("name", &file.name)]))
            }
            Look::Video => {
                let (w, h) = fitted(file.width, file.height, size::sidebar().min(room));
                let frame =
                    stack![picture(shown, Icon::Video, w, h), container(play_mark()).center(Fill)].width(w).height(h);
                let (press, tip) = if file.hide_download {
                    (wrap(FilesMsg::Nextcloud(file_id)), tr("Opens in Nextcloud: the sender turned downloads off"))
                } else {
                    (wrap(FilesMsg::Open(id, file_id)), tr("Opens in your video player"))
                };
                clickable(frame.into(), press, trf("{name} · {what}", &[("name", &file.name), ("what", &tip)]))
            }
            // A line takes the bubble's room (at most a panel's width): its name gets what the buttons
            // leave, instead of a bubble that shrinks around it.
            Look::Voice => container(self.voice_line(chat, message, file)).width(room.min(size::panel())).into(),
            Look::Line => {
                let visual = file.content_type.starts_with("image/") || file.content_type.starts_with("video/");
                container(self.file_line(chat, message, file, visual)).width(room.min(size::panel())).into()
            }
        }
    }

    /// A voice message: play (in the system's player), "Voice message" and its size, Download.
    fn voice_line<'a>(
        &'a self,
        chat: &'a ChatState,
        message: &'a ChatMessage,
        file: &'a FileRef,
    ) -> Element<'a, Message> {
        let busy = chat.files.busy.get(&(message.id.clone(), file.id.clone())).copied();
        let open =
            (!file.hide_download && busy.is_none()).then(|| wrap(FilesMsg::Open(message.id.clone(), file.id.clone())));
        let tip = if busy == Some(Busy::Opening) { tr("Opening…") } else { tr("Play in your audio player") };
        let words = column![
            typo::text(typo::body(), tr("Voice message")).wrapping(Wrapping::None),
            typo::text(typo::meta().tabular(), units::format_bytes(file.size))
                .style(theme::text_3)
                .wrapping(Wrapping::None),
        ];
        let mut line = row![widgets::icon_button(Icon::Play, tip, open), container(words).width(Fill).clip(true)]
            .spacing(space::s())
            .align_y(Alignment::Center);
        line = line.extend(self.file_buttons(chat, message, file, false));
        line.into()
    }

    /// A file as a line: its kind on a tile, name over size and kind, the buttons.
    fn file_line<'a>(
        &'a self,
        chat: &'a ChatState,
        message: &'a ChatMessage,
        file: &'a FileRef,
        no_preview: bool,
    ) -> Element<'a, Message> {
        let busy = chat.files.busy.get(&(message.id.clone(), file.id.clone())).copied();
        let meta = match busy {
            Some(Busy::Opening) => tr("Opening…").to_owned(),
            Some(Busy::Saving) => tr("Saving…").to_owned(),
            None => file_meta(file, no_preview),
        };
        let words = column![
            widgets::ellipsized(typo::body(), file.name.as_str(), theme::text),
            widgets::ellipsized(typo::meta().tabular(), meta, theme::text_3),
        ];
        let tile = widgets::source_tile(TileFace::Icon(kind_icon(&file.content_type)), TileSize::Medium, None);
        row![tile, container(words).width(Fill).clip(true)]
            .extend(self.file_buttons(chat, message, file, true))
            .spacing(space::s())
            .align_y(Alignment::Center)
            .into()
    }

    /// "Open" (with `open`), "Download" and "Open in Nextcloud" for a file; a file shared without
    /// download has only the last. A button whose action is under way takes no click.
    fn file_buttons<'a>(
        &'a self,
        chat: &'a ChatState,
        message: &'a ChatMessage,
        file: &'a FileRef,
        open: bool,
    ) -> Vec<Element<'a, Message>> {
        let busy = chat.files.busy.get(&(message.id.clone(), file.id.clone())).copied();
        let (id, file_id) = (message.id.clone(), file.id.clone());
        let mut buttons = Vec::new();
        if !file.hide_download {
            if open {
                let tip = if busy == Some(Busy::Opening) { tr("Opening…") } else { tr("Open") };
                let press = busy.is_none().then(|| wrap(FilesMsg::Open(id.clone(), file_id.clone())));
                buttons.push(widgets::icon_button(Icon::ExternalLink, tip, press));
            }
            let tip = if busy == Some(Busy::Saving) { tr("Saving…") } else { tr("Download") };
            let press = busy.is_none().then(|| wrap(FilesMsg::Save(id, file_id.clone())));
            buttons.push(widgets::icon_button(Icon::Download, tip, press));
        }
        buttons.push(widgets::icon_button(
            Icon::Cloud,
            tr("Open in Nextcloud"),
            Some(wrap(FilesMsg::Nextcloud(file_id))),
        ));
        buttons
    }

    /// Under a message with a link: its card; for a link to another provider not asked about yet
    /// (Settings › Privacy at "Ask", issue #672) the question "Show once", "Always allow …", "Never for
    /// …"; else nothing – a blocked provider, "Never show" and a link without a card keep only the link
    /// in the text.
    pub(super) fn link_part<'a>(
        &'a self,
        chat: &'a ChatState,
        message: &'a ChatMessage,
        room: f32,
    ) -> Option<Element<'a, Message>> {
        if message.kind == MessageKind::Deleted {
            return None;
        }
        if let Some(card) = message.cards.first() {
            return Some(self.card(chat, message, card, room));
        }
        let link = timeline::first_link(&message.text)?;
        let account = torro_chat::split_conversation(&chat.id).map(|(account, _)| account)?;
        // Links to the own server get their card by themselves (decision 23).
        if system::is_own_link(account, link) {
            return None;
        }
        Some(match chat.files.cards.get(&message.id) {
            Some(CardAsk::Asking) => typo::text(typo::meta(), tr("Loading preview…")).style(theme::text_3).into(),
            Some(CardAsk::None) => typo::text(typo::meta(), tr("No preview for this link")).style(theme::text_3).into(),
            None => foreign_question(&message.id, message.foreign.as_ref()?),
        })
    }

    /// A link card: its picture from the own server, title, description and domain; a click opens
    /// the link in the browser.
    fn card<'a>(
        &'a self,
        chat: &'a ChatState,
        message: &'a ChatMessage,
        card: &'a LinkCard,
        room: f32,
    ) -> Element<'a, Message> {
        let width = room.min(size::props());
        let mut parts: Vec<Element<'a, Message>> = Vec::new();
        if card.image {
            let key: PictureKey = (message.id.clone(), None);
            parts.push(picture(chat.files.picture(&key), Icon::Image, width, width / 2.0));
        }
        let mut words = column![widgets::ellipsized(typo::body_strong(), card.title.as_str(), theme::text)];
        if let Some(description) = card.description.as_deref().filter(|text| !text.trim().is_empty()) {
            words = words.push(widgets::ellipsized(typo::meta(), description, theme::text_2));
        }
        words = words.push(
            row![
                tinted(Icon::Link, icon::SIZE_SM, |p| p.text_3),
                widgets::ellipsized(typo::meta(), card.domain.as_str(), theme::text_3)
            ]
            .spacing(space::xs())
            .align_y(Alignment::Center),
        );
        parts.push(container(words).padding([space::s(), space::m()]).width(Fill).into());
        let content = container(column(parts)).width(width).clip(true).style(|t: &Theme| {
            let p = palette(t);
            container::Style {
                background: Some(Background::Color(p.panel)),
                border: border::rounded(radius::sm()).color(p.line_strong).width(HAIRLINE),
                ..container::Style::default()
            }
        });
        clickable(content.into(), wrap(FilesMsg::OpenLink(card.url.clone())), card.url.clone())
    }

    /// Above the composer: the files chosen for sending, or the sending under way. `None` when there
    /// is neither.
    pub(super) fn composer_files<'a>(&'a self, chat: &'a ChatState) -> Option<Element<'a, Message>> {
        if let Some(upload) = &chat.files.upload {
            return Some(upload_row(upload));
        }
        if chat.files.attached.is_empty() {
            return None;
        }
        let rows: Vec<Element<'a, Message>> = chat.files.attached.iter().map(attached_row).collect();
        Some(column(rows).spacing(space::xs()).into())
    }

    /// Over the whole window while a file is dragged over it: where it goes.
    pub(super) fn drop_layer<'a>(&'a self, chat: &'a ChatState) -> Element<'a, Message> {
        if !chat.files.dropping {
            return Space::new().into();
        }
        let content = column![
            widgets::source_tile(TileFace::Icon(Icon::Upload), TileSize::Large, None),
            typo::text(typo::body_strong(), trf("Drop to send to {name}", &[("name", &self.chat_name(chat))]))
                .align_x(Alignment::Center),
            typo::text(typo::meta(), tr("Files go to your Talk folder in Nextcloud.")).style(theme::text_3),
        ]
        .spacing(space::s())
        .align_x(Alignment::Center);
        let zone = container(content).width(Fill).height(Fill).center(Fill).style(|t: &Theme| {
            let p = palette(t);
            container::Style {
                background: Some(Background::Color(p.panel)),
                border: border::rounded(radius::lg()).color(p.line_selected).width(HAIRLINE),
                ..container::Style::default()
            }
        });
        container(zone)
            .width(Fill)
            .height(Fill)
            .padding(space::l())
            .style(|t: &Theme| container::Style {
                background: Some(Background::Color(Color { a: 0.92, ..palette(t).panel })),
                ..container::Style::default()
            })
            .into()
    }
}

/// The small picture of a file above the composer: its preview, else its kind on a tile.
fn thumb<'a>(handle: Option<&iced::widget::image::Handle>, which: Icon) -> Element<'a, Message> {
    let edge = TileSize::Medium.edge();
    match handle {
        Some(handle) => image(handle.clone())
            .width(edge)
            .height(edge)
            .content_fit(ContentFit::Cover)
            .border_radius(radius::sm())
            .into(),
        None => widgets::source_tile(TileFace::Icon(which), TileSize::Medium, None),
    }
}

/// A file chosen for sending: picture or kind, name over what it is (or why it cannot go), ✕.
fn attached_row(attached: &Attached) -> Element<'_, Message> {
    let (lead, name, meta, failed): (Element<'_, Message>, &str, String, bool) = match attached {
        Attached::Preparing { name, .. } => {
            (thumb(None, Icon::Clock), name.as_str(), tr("Preparing…").to_owned(), false)
        }
        Attached::Ready { file, thumb: handle, .. } => {
            let mut parts = Vec::new();
            if let (Some(w), Some(h)) = (file.width, file.height) {
                parts.push(trf("{width} × {height}", &[("width", &w), ("height", &h)]));
            } else {
                parts.push(kind_word(&file.content_type).to_owned());
            }
            parts.push(units::format_bytes(file.size));
            (thumb(handle.as_ref(), kind_icon(&file.content_type)), file.name.as_str(), parts.join(" · "), false)
        }
        Attached::Failed { name, why, .. } => {
            let mark =
                container(tinted(Icon::CircleAlert, icon::SIZE, |p| p.error_text)).center(TileSize::Medium.edge());
            (mark.into(), name.as_str(), why.clone(), true)
        }
    };
    let meta_line: Element<'_, Message> = if failed {
        typo::text(typo::meta(), meta).style(theme::text_2).wrapping(Wrapping::WordOrGlyph).into()
    } else {
        widgets::ellipsized(typo::meta().tabular(), meta, theme::text_3)
    };
    let words = column![widgets::ellipsized(typo::body(), name, theme::text), meta_line];
    let remove = widgets::icon_button(Icon::X, tr("Remove"), Some(wrap(FilesMsg::Remove(attached.key()))));
    row![lead, container(words).width(Fill).clip(true), remove].spacing(space::s()).align_y(Alignment::Center).into()
}

/// The sending under way: the first file's picture, its name over the progress, the share in
/// figures, ✕ "Cancel upload" (off once the files are being shared); failed, why and ✕ "Dismiss".
fn upload_row(upload: &Upload) -> Element<'_, Message> {
    let name = if upload.others == 0 {
        upload.name.clone()
    } else {
        trf("{name} and {count} more", &[("name", &upload.name), ("count", &upload.others)])
    };
    if let Some(error) = &upload.error {
        let mark = container(tinted(Icon::CircleAlert, icon::SIZE, |p| p.error_text)).center(TileSize::Medium.edge());
        let words = column![
            widgets::ellipsized(typo::body(), name, theme::text),
            typo::text(typo::meta(), error.as_str()).style(theme::text_2).wrapping(Wrapping::WordOrGlyph),
        ];
        let dismiss = widgets::icon_button(Icon::X, tr("Dismiss"), Some(wrap(FilesMsg::DismissUpload)));
        return row![mark, container(words).width(Fill), dismiss].spacing(space::s()).align_y(Alignment::Center).into();
    }
    let (sent, total, sharing) = match &upload.progress {
        Some(progress) => (progress.sent, progress.total, progress.state == torro_chat::UploadState::Sharing),
        None => (0, 0, false),
    };
    let figure = match (sent.min(total) * 100).checked_div(total) {
        _ if sharing => tr("Sharing…").to_owned(),
        Some(share) => format!("{share}%"),
        None => tr("Starting…").to_owned(),
    };
    let words = column![widgets::ellipsized(typo::body(), name, theme::text), widgets::progress_line(sent, total)]
        .spacing(space::xs());
    let cancel = if sharing || upload.cancelling {
        let tip = if upload.cancelling {
            tr("Cancelling…")
        } else {
            tr("The files are being shared and can no longer be cancelled")
        };
        widgets::icon_button(Icon::X, tip, None)
    } else {
        widgets::icon_button(Icon::X, tr("Cancel upload"), Some(wrap(FilesMsg::Cancel)))
    };
    row![
        thumb(upload.thumb.as_ref(), Icon::Upload),
        container(words).width(Fill),
        typo::text(typo::meta().tabular(), figure).style(theme::text_3).wrapping(Wrapping::None),
        cancel,
    ]
    .spacing(space::s())
    .align_y(Alignment::Center)
    .into()
}

/// The question in place of the card of another provider's link (issue #672): what the message would
/// load, and the three answers. "Show once" fetches only this card and keeps nothing; the other two put
/// the provider on the list of Settings › Privacy. Quiet: words in `text_2`, ghost buttons that wrap in
/// a narrow window.
fn foreign_question<'a>(message: &str, provider: &ForeignProvider) -> Element<'a, Message> {
    let name = provider.name.as_str();
    let what = trf("This message shows content from {provider}.", &[("provider", &name)]);
    let once = widgets::with_tooltip(
        widgets::text_button(
            Some(Icon::Eye),
            tr("Show once"),
            style::button::ghost,
            Some(wrap(FilesMsg::ShowCard(message.to_owned()))),
        ),
        tr("Your Nextcloud fetches the page, so the site may see when it is read."),
    );
    let decide = |label: String, choice: ProviderChoice| {
        widgets::text_button(
            None,
            label,
            style::button::ghost,
            Some(wrap(FilesMsg::Decide(provider.key.clone(), choice))),
        )
    };
    let allow = decide(trf("Always allow {provider}", &[("provider", &name)]), ProviderChoice::Allowed);
    let block = decide(trf("Never for {provider}", &[("provider", &name)]), ProviderChoice::Blocked);
    column![
        typo::text(typo::meta(), what).style(theme::text_2).wrapping(Wrapping::WordOrGlyph),
        row![once, allow, block].spacing(space::xs()).wrap(),
    ]
    .spacing(space::xs())
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(content_type: &str, preview: bool) -> FileRef {
        FileRef {
            id: "1".into(),
            name: "x".into(),
            size: 1,
            content_type: content_type.into(),
            preview,
            hide_download: false,
            width: Some(1600),
            height: Some(1000),
            blurhash: None,
        }
    }

    #[test]
    fn files_show_as_preview_only_while_the_server_draws_one() {
        let mut message = torro_chat::Message {
            id: "a/b/1".into(),
            author: torro_chat::Author::default(),
            at: chrono::Utc::now(),
            text: String::new(),
            kind: MessageKind::Message,
            mentions: Vec::new(),
            files: Vec::new(),
            reply_to: None,
            reactions: Default::default(),
            own_reactions: Vec::new(),
            edited: false,
            cards: Vec::new(),
            foreign: None,
            untrusted: true,
        };
        assert_eq!(look(&message, &file("image/png", true), None), Look::Picture);
        assert_eq!(look(&message, &file("image/png", true), Some(&Shown::Missing)), Look::Line);
        assert_eq!(look(&message, &file("image/svg+xml", true), None), Look::Line, "the server draws no SVG");
        assert_eq!(look(&message, &file("video/mp4", true), None), Look::Video);
        assert_eq!(look(&message, &file("video/mp4", false), None), Look::Line);
        assert_eq!(look(&message, &file("application/pdf", false), None), Look::Line);
        message.kind = MessageKind::Voice;
        assert_eq!(look(&message, &file("audio/ogg", false), None), Look::Voice);
    }

    #[test]
    fn previews_keep_their_shape_inside_the_edge() {
        assert_eq!(fitted(Some(1600), Some(1000), 240.0), (240.0, 150.0));
        assert_eq!(fitted(Some(900), Some(1200), 240.0), (180.0, 240.0));
        assert_eq!(fitted(Some(100), Some(50), 240.0), (100.0, 50.0), "small pictures stay small");
        assert_eq!(fitted(None, None, 240.0), (240.0, 180.0));
    }

    #[test]
    fn kinds_have_words() {
        assert_eq!(kind_word("application/pdf"), tr("PDF"));
        assert_eq!(kind_word("video/quicktime"), tr("Video"));
        assert_eq!(kind_word("application/zip"), tr("File"));
        let mut hidden = file("application/pdf", false);
        hidden.hide_download = true;
        assert!(file_meta(&hidden, false).ends_with(tr("view only")));
    }
}
