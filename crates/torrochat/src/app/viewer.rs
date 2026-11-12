//! The window "Picture large" (`torrochat --viewer=<message>/<file>`, class `torro-chat-viewer`,
//! decision 24 of `docs/chat.md`): one picture of a conversation, fitted into the window, with
//! `←`/`→` through the conversation's pictures, "Download", "Open in Nextcloud" and `Esc` to close.
//!
//! - **The pictures** come from `chat.list_shared` (kind media, pictures only, the oldest first);
//!   a server that cannot list them gives the pictures of the messages around this one
//!   (`chat.get_messages` with `around`), and failing that the one picture alone.
//! - **The picture** is the large preview of the server (`chat.get_preview` large, 1600 pixels), not
//!   the original: the window shows, "Download" fetches the file (`chat.save_attachment`).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use iced::{Subscription, Task, keyboard};
use torro_chat::{ChatError, MessagePage, Picture, PreviewSize, SaveTo, SharedKind, SharedPage};
use torro_core::i18n::{tr, trf};

use super::files::{Shown, file_failed};
use super::{App, Message};
use crate::system;

/// One picture the window can show.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewItem {
    /// The message (`<conversation>/<number>`).
    pub message: String,
    pub file: String,
    /// The file's name; empty while unknown.
    pub name: String,
    pub author: String,
    pub at: Option<DateTime<Utc>>,
}

/// The state of the window "Picture large".
#[derive(Debug)]
pub struct ViewerState {
    /// The conversation of the picture.
    pub conversation: String,
    /// The pictures, the oldest first; the one asked for at least.
    pub items: Vec<ViewItem>,
    /// The picture shown.
    pub at: usize,
    /// The list of pictures came (or the window gave up on it).
    pub listed: bool,
    pub pictures: HashMap<(String, String), Shown>,
    /// "Download" is on its way.
    pub saving: bool,
}

/// What happens in the window "Picture large".
#[derive(Debug, Clone)]
pub enum ViewerMsg {
    Listed(Result<SharedPage, ChatError>),
    Around(Result<MessagePage, ChatError>),
    Picture((String, String), Result<Picture, ChatError>),
    /// `←` (−1) or `→` (+1).
    Step(i32),
    Save,
    Saved(Result<String, ChatError>),
    Nextcloud,
}

/// `msg` as a message of the window.
pub fn wrap(msg: ViewerMsg) -> Message {
    Message::Viewer(msg)
}

/// A file the window shows large: a picture with a preview, no SVG (the server draws none).
fn is_picture(content_type: &str, preview: bool) -> bool {
    preview && content_type.starts_with("image/") && content_type != "image/svg+xml"
}

impl ViewerState {
    /// The window for the file `file` of the message `message`; `None` for a message id that names
    /// no conversation.
    pub fn new(message: &str, file: &str) -> Option<ViewerState> {
        let (conversation, _) = torro_chat::split_message(message)?;
        let only = ViewItem {
            message: message.to_owned(),
            file: file.to_owned(),
            name: String::new(),
            author: String::new(),
            at: None,
        };
        Some(ViewerState {
            conversation: conversation.to_owned(),
            items: vec![only],
            at: 0,
            listed: false,
            pictures: HashMap::new(),
            saving: false,
        })
    }

    /// The picture shown.
    pub fn current(&self) -> Option<&ViewItem> {
        self.items.get(self.at)
    }

    /// Takes `items` (the oldest first) as the pictures, keeping the one shown; it stays alone when
    /// they do not hold it.
    fn take(&mut self, items: Vec<ViewItem>) {
        let Some(shown) = self.current().cloned() else { return };
        if let Some(at) = items.iter().position(|item| item.message == shown.message && item.file == shown.file) {
            self.items = items;
            self.at = at;
        }
    }

    /// `←`/`→` while the window is open: the keys of the window, not of a field (there is none).
    pub fn subscription(&self) -> Subscription<ViewerMsg> {
        iced::event::listen_with(arrow)
    }
}

/// `←` and `→`.
fn arrow(event: iced::Event, _status: iced::event::Status, _window: iced::window::Id) -> Option<ViewerMsg> {
    let iced::Event::Keyboard(keyboard::Event::KeyPressed { key, .. }) = event else { return None };
    match key.as_ref() {
        keyboard::Key::Named(keyboard::key::Named::ArrowLeft) => Some(ViewerMsg::Step(-1)),
        keyboard::Key::Named(keyboard::key::Named::ArrowRight) => Some(ViewerMsg::Step(1)),
        _ => None,
    }
}

impl App {
    /// The first asks of the window: the conversation's pictures and the large picture.
    pub(crate) fn viewer_start(&mut self) -> Task<Message> {
        let Some(viewer) = self.viewer.as_ref() else { return Task::none() };
        let list = Task::perform(
            self.backend.clone().shared(viewer.conversation.clone(), SharedKind::Media, None),
            |result| wrap(ViewerMsg::Listed(result)),
        );
        Task::batch([list, self.want_large()])
    }

    /// What happens in the window "Picture large".
    pub(crate) fn viewer_update(&mut self, msg: ViewerMsg) -> Task<Message> {
        let backend = self.backend.clone();
        let Some(viewer) = self.viewer.as_mut() else { return Task::none() };
        match msg {
            ViewerMsg::Listed(Ok(page)) => {
                let mut items: Vec<ViewItem> = page
                    .items
                    .into_iter()
                    .filter(|item| is_picture(&item.content_type, item.preview))
                    .filter_map(|item| {
                        Some(ViewItem {
                            message: item.message,
                            file: item.file?,
                            name: item.name,
                            author: item.author,
                            at: Some(item.at),
                        })
                    })
                    .collect();
                items.reverse();
                let shown = viewer.current().cloned();
                if shown.is_some_and(|shown| items.iter().any(|i| i.message == shown.message && i.file == shown.file)) {
                    viewer.take(items);
                    viewer.listed = true;
                    return Task::none();
                }
                // Not among the newest listed: the messages around it know it.
                return self.viewer_around();
            }
            ViewerMsg::Listed(Err(_)) => return self.viewer_around(),
            ViewerMsg::Around(result) => {
                viewer.listed = true;
                if let Ok(page) = result {
                    let items = page
                        .messages
                        .iter()
                        .flat_map(|message| {
                            message.files.iter().filter(|file| is_picture(&file.content_type, file.preview)).map(
                                |file| ViewItem {
                                    message: message.id.clone(),
                                    file: file.id.clone(),
                                    name: file.name.clone(),
                                    author: message.author.name.clone(),
                                    at: Some(message.at),
                                },
                            )
                        })
                        .collect();
                    viewer.take(items);
                }
            }
            ViewerMsg::Picture(key, result) => {
                let shown = match result {
                    Ok(picture) => system::picture_handle(&picture.bytes).map_or(Shown::Missing, Shown::Ready),
                    Err(_) => Shown::Missing,
                };
                viewer.pictures.insert(key, shown);
            }
            ViewerMsg::Step(step) => {
                let last = viewer.items.len().saturating_sub(1) as i64;
                viewer.at = (viewer.at as i64 + i64::from(step)).clamp(0, last) as usize;
                return self.want_large();
            }
            ViewerMsg::Save => {
                let Some(item) = viewer.current().cloned().filter(|_| !viewer.saving) else { return Task::none() };
                viewer.saving = true;
                let task = backend.save_attachment(item.message, item.file, SaveTo::Downloads);
                return Task::perform(task, |result| wrap(ViewerMsg::Saved(result)));
            }
            ViewerMsg::Saved(result) => {
                viewer.saving = false;
                match result {
                    Ok(path) => self.notify(trf("Saved to {path}", &[("path", &path)])),
                    Err(err) => self.notify(file_failed(&err)),
                }
            }
            ViewerMsg::Nextcloud => {
                let account = torro_chat::split_conversation(&viewer.conversation).map(|(account, _)| account);
                let link = viewer
                    .current()
                    .zip(account)
                    .and_then(|(item, account)| system::nextcloud_file(account, &item.file));
                match link.map(|link| system::open_link(&link)) {
                    Some(Ok(())) => {}
                    Some(Err(why)) => self.notify(why),
                    None => self.notify(tr("This file has no address in Nextcloud.").to_owned()),
                }
            }
        }
        Task::none()
    }

    /// `chat.get_messages` around the picture asked for, for its neighbours.
    fn viewer_around(&mut self) -> Task<Message> {
        let Some(viewer) = self.viewer.as_ref() else { return Task::none() };
        let Some(item) = viewer.current() else { return Task::none() };
        let task = self.backend.clone().messages_around(viewer.conversation.clone(), item.message.clone());
        Task::perform(task, |result| wrap(ViewerMsg::Around(result)))
    }

    /// The large picture shown, unless it came or is on its way.
    fn want_large(&mut self) -> Task<Message> {
        let backend = self.backend.clone();
        let Some(viewer) = self.viewer.as_mut() else { return Task::none() };
        let Some(item) = viewer.current().cloned() else { return Task::none() };
        let key = (item.message.clone(), item.file.clone());
        if viewer.pictures.contains_key(&key) {
            return Task::none();
        }
        viewer.pictures.insert(key.clone(), Shown::Loading);
        Task::perform(backend.preview(item.message, Some(item.file), PreviewSize::Large), move |result| {
            wrap(ViewerMsg::Picture(key.clone(), result))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_viewer_keeps_its_picture_among_the_listed_ones() {
        let message = "nextcloud:a@b/tok/3";
        let mut viewer = ViewerState::new(message, "7").expect("a message id");
        assert_eq!(viewer.conversation, "nextcloud:a@b/tok");
        let item = |n: u64, file: &str| ViewItem {
            message: format!("nextcloud:a@b/tok/{n}"),
            file: file.into(),
            name: format!("{file}.png"),
            author: "Ben".into(),
            at: None,
        };
        viewer.take(vec![item(1, "5"), item(2, "6")]);
        assert_eq!(viewer.items.len(), 1, "a list without the picture leaves it alone");
        viewer.take(vec![item(1, "5"), item(3, "7"), item(4, "8")]);
        assert_eq!((viewer.at, viewer.items.len()), (1, 3));
        assert!(ViewerState::new("token", "7").is_none());
        assert!(is_picture("image/png", true) && !is_picture("image/svg+xml", true) && !is_picture("video/mp4", true));
    }
}
