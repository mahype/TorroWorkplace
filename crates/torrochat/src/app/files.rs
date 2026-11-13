//! Files and media of a chat window (stage 4 of `docs/chat.md`): what is chosen for sending, how
//! sending goes, the pictures of the history, opening and saving files, link cards and the shared
//! media at the side ([`FilesState`], [`App::files_update`]).
//!
//! - **Choosing:** the paperclip opens the system's dialog, a file dragged onto the window or
//!   pasted from a file manager is taken as well; each goes to the core with `chat.prepare_file`
//!   (`source: path`). A pasted picture the window holds itself goes up in pieces
//!   (`chat.upload_file`, [`crate::backend::Backend::upload_bytes`]). The files stand above the
//!   composer; ✕ drops one (`chat.release_files`).
//! - **Sending:** Enter sends the ready files with the text as caption (`chat.send_message` with
//!   `files`). The place of the files then shows the progress (`chat.get_upload` every
//!   [`UPLOAD_POLL`]) with ✕ (`chat.cancel_upload`). A sending that failed says why there.
//! - **History:** previews come with `chat.get_preview` (small) as soon as a page arrives; until
//!   then a calm placeholder of the same size stands there. "Open" fetches the file
//!   (`chat.get_attachment`) and opens it with the system's program, "Download" saves it to
//!   Downloads (`chat.save_attachment`), "Save as…" asks the system's dialog first. Under a link to
//!   another provider not asked about yet (issue #672), "Show once" asks for its card
//!   (`chat.show_card`), "Always allow …" and "Never for …" put the provider on the list of Settings ›
//!   Privacy (`settings.set_setting privacy_provider`).
//! - **Shared media:** a column at the side (`chat.list_shared`) with the tabs Media, Files, Voice
//!   and Links; "Show in chat" loads the messages around the item (`chat.get_messages` with
//!   `around`) and marks it.
//!
//! What the core refuses (a file shared without download, a server that cannot) is said in the calm
//! line above the composer ([`super::chat::Note`]), never as a failure of the window.

use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use iced::widget::image::Handle;
use iced::widget::operation::{self, RelativeOffset};
use iced::widget::text_editor::{Action, Content, Edit};
use iced::{Point, Size, Task};
use torro_chat::{
    ChatError, Fetched, LinkCard, MessagePage, Outgoing, Picture, PreparedFile, PreviewSize, SaveTo, Sending,
    SharedItem, SharedKind, SharedPage, UploadProgress, UploadState,
};
use torro_core::AppId;
use torro_core::i18n::{tr, trf};
use torro_core::privacy::ProviderChoice;
use torro_gui::split::{self, Split, SplitEvent};
use torro_gui::theme::size;
use torro_gui::ui_state;

use super::chat::{EDITOR, HISTORY, Msg as ChatMsg, Note};
use super::{App, ChatState, Menu, Message};
use crate::system::{self, Clip};
use crate::timeline;

/// How often the window asks how a sending goes.
pub const UPLOAD_POLL: Duration = Duration::from_millis(600);

/// The key under which `ui.json` keeps the width of the shared media.
pub const MEDIA_SPLIT: &str = "chat.media";

/// The name a pasted picture goes out with: a file name, so not translated.
pub const PASTED_NAME: &str = "Pasted image.png";

/// A picture of the history: the message and its file, or `None` for the picture of its link card.
pub type PictureKey = (String, Option<String>);

/// Where a picture stands.
#[derive(Debug, Clone)]
pub enum Shown {
    /// Asked for: a placeholder of the same size stands there.
    Loading,
    Ready(Handle),
    /// The server has none: the file shows as a line.
    Missing,
}

/// A file chosen for sending, above the composer.
#[derive(Debug, Clone)]
pub enum Attached {
    /// The core reads it.
    Preparing { key: u64, name: String },
    /// Prepared: it goes with the next message.
    Ready { key: u64, file: PreparedFile, thumb: Option<Handle> },
    /// It cannot go; `why` says so, in words for people.
    Failed { key: u64, name: String, why: String },
}

impl Attached {
    pub fn key(&self) -> u64 {
        match self {
            Attached::Preparing { key, .. } | Attached::Ready { key, .. } | Attached::Failed { key, .. } => *key,
        }
    }
}

/// Files on their way to the server.
#[derive(Debug, Clone)]
pub struct Upload {
    /// The id `chat.send_message` answered with.
    pub id: String,
    /// The first file's name, and how many go with it.
    pub name: String,
    pub others: usize,
    /// A picture of the first file, when it is one.
    pub thumb: Option<Handle>,
    /// What the core said last; `None` before the first answer.
    pub progress: Option<UploadProgress>,
    /// An ask for the progress is out.
    pub asking: bool,
    pub cancelling: bool,
    /// Why it failed, in words for people.
    pub error: Option<String>,
}

impl Upload {
    /// Whether the window keeps asking.
    pub fn running(&self) -> bool {
        self.error.is_none() && self.progress.as_ref().is_none_or(|progress| !progress.state.is_over())
    }
}

/// A file action under way, so its button takes no second click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Busy {
    Opening,
    Saving,
}

/// "Show once" of a foreign link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CardAsk {
    Asking,
    /// The server knows no card for it.
    None,
}

/// The tabs of the shared media (`docs/chat.md`, "Medien-Übersicht").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum MediaTab {
    #[default]
    Media,
    Files,
    Voice,
    Links,
}

impl MediaTab {
    pub const ALL: [MediaTab; 4] = [MediaTab::Media, MediaTab::Files, MediaTab::Voice, MediaTab::Links];

    pub fn label(self) -> &'static str {
        match self {
            MediaTab::Media => tr("Media"),
            MediaTab::Files => tr("Files"),
            MediaTab::Voice => tr("Voice messages"),
            MediaTab::Links => tr("Links"),
        }
    }

    /// The kind `chat.list_shared` lists for it.
    pub fn kind(self) -> SharedKind {
        match self {
            MediaTab::Media => SharedKind::Media,
            MediaTab::Files => SharedKind::File,
            MediaTab::Voice => SharedKind::Voice,
            MediaTab::Links => SharedKind::Link,
        }
    }
}

/// The shared media at the side.
#[derive(Debug, Clone, Default)]
pub struct MediaPanel {
    pub tab: MediaTab,
    /// What the tab lists, the newest first.
    pub items: Vec<SharedItem>,
    pub more: bool,
    pub loading: bool,
    /// Why nothing could be listed.
    pub error: Option<ChatError>,
}

/// The files and media of a chat window.
#[derive(Debug, Default)]
pub struct FilesState {
    pub attached: Vec<Attached>,
    next_key: u64,
    pub upload: Option<Upload>,
    pub pictures: HashMap<PictureKey, Shown>,
    /// File actions under way: (message, file).
    pub busy: BTreeMap<(String, String), Busy>,
    /// "Show once" asked or answered without a card, by message.
    pub cards: BTreeMap<String, CardAsk>,
    pub media: Option<MediaPanel>,
    /// The width of the shared media, as the person dragged it.
    pub split: Split,
    /// A file is dragged over the window.
    pub dropping: bool,
    /// The message "Show in chat" went to: it stands marked.
    pub highlight: Option<String>,
    /// How much wider the window was made for the shared media, to give it back on closing.
    grew: Option<f32>,
}

impl FilesState {
    /// The ready files' ids, in the order they were chosen.
    pub fn ready(&self) -> Vec<String> {
        self.attached
            .iter()
            .filter_map(|attached| match attached {
                Attached::Ready { file, .. } => Some(file.id.clone()),
                _ => None,
            })
            .collect()
    }

    /// Whether a file is still being prepared: sending waits for it.
    pub fn preparing(&self) -> bool {
        self.attached.iter().any(|attached| matches!(attached, Attached::Preparing { .. }))
    }

    /// The picture of `key`, as far as it came.
    pub fn picture(&self, key: &PictureKey) -> Option<&Shown> {
        self.pictures.get(key)
    }

    fn key(&mut self) -> u64 {
        self.next_key += 1;
        self.next_key
    }
}

/// What happens to files and media in a chat window.
#[derive(Debug, Clone)]
pub enum FilesMsg {
    /// The paperclip: the system's dialog.
    Attach,
    Chosen(Vec<PathBuf>),
    Prepared(u64, Result<PreparedFile, ChatError>),
    Remove(u64),
    /// Nothing to do: an answer nobody waits for.
    Done,
    /// Ctrl+V (⌘V) in the composer.
    Paste,
    Pasted(Result<Clip, String>),
    /// A file is dragged over the window, or no longer.
    Hovered(bool),
    Dropped(PathBuf),
    Started(Result<Sending, ChatError>),
    Poll,
    Progress(Result<UploadProgress, ChatError>),
    Cancel,
    Cancelled(Result<(), ChatError>),
    DismissUpload,
    Picture(PictureKey, Result<Picture, ChatError>),
    /// "Open" (and "Play"): the file `(message, file)`.
    Open(String, String),
    Fetched(String, String, Result<Fetched, ChatError>),
    /// "Download": to Downloads.
    Save(String, String),
    /// "Save as…": the system's dialog first.
    SaveAs(String, String),
    SaveTo(String, String, Option<PathBuf>),
    Saved(String, String, Result<String, ChatError>),
    /// "Open in Nextcloud": the file's id.
    Nextcloud(String),
    OpenLink(String),
    /// "Show once" of the message's foreign link.
    ShowCard(String),
    Card(String, Result<Option<LinkCard>, ChatError>),
    /// "Always allow …" or "Never for …" (issue #672): the provider's key and the choice for Settings
    /// › Privacy.
    Decide(String, ProviderChoice),
    Decided(String, ProviderChoice, Result<(), ChatError>),
    /// A picture clicked: the window "Picture large".
    View(String, String),
    ToggleMedia,
    CloseMedia,
    Tab(MediaTab),
    Shared(MediaTab, bool, Result<SharedPage, ChatError>),
    MoreShared,
    /// "Show in chat": the message.
    ShowInChat(String),
    Around(String, Result<MessagePage, ChatError>),
    Split(SplitEvent),
    /// The widths `ui.json` keeps.
    Stored(HashMap<String, f32>),
    /// The menu of a shared item, at the pointer.
    ItemMenu(usize, Point),
}

/// `msg` as a message of the window.
pub fn wrap(msg: FilesMsg) -> Message {
    Message::Chat(ChatMsg::Files(msg))
}

/// What the person reads when a file command failed: what happened, without blame.
pub fn file_failed(err: &ChatError) -> String {
    match err {
        ChatError::NotFound => tr("This file is no longer available.").to_owned(),
        ChatError::Busy => tr("Too many files at once. Try again in a moment.").to_owned(),
        other => super::chat_failed(other),
    }
}

/// The limits of the shared media's width beside the history in a window `total` wide.
pub fn media_limits(total: f32) -> split::Limits {
    split::limits_around(size::props(), total, split::min_pane())
}

/// Whether the history and the shared media fit side by side in `width`; else the media take the
/// window.
pub fn media_beside(width: f32) -> bool {
    width >= 2.0 * split::min_pane() + split::divider_width()
}

/// The file `(message, file)` of `chat`, with its message.
fn file_of<'a>(chat: &'a ChatState, message: &str, file: &str) -> Option<&'a torro_chat::FileRef> {
    chat.message(message)?.files.iter().find(|shared| shared.id == file)
}

impl App {
    /// The account of the chat window's conversation.
    fn chat_account(&self) -> Option<String> {
        let chat = self.chat.as_ref()?;
        torro_chat::split_conversation(&chat.id).map(|(account, _)| account.to_owned())
    }

    /// Says `text` in the calm line above the composer.
    fn file_note(&mut self, text: String) {
        if let Some(chat) = self.chat.as_mut() {
            chat.note = Some(Note { text, waiting: false });
        }
    }

    /// What happens to files and media in a chat window.
    pub(crate) fn files_update(&mut self, msg: FilesMsg) -> Task<Message> {
        let backend = self.backend.clone();
        let demo = self.backend.is_demo();
        let account = self.chat_account().unwrap_or_default();
        let Some(chat) = self.chat.as_mut() else { return Task::none() };
        let conversation = chat.id.clone();
        let files = &mut chat.files;
        match msg {
            FilesMsg::Attach => {
                return Task::perform(system::choose_files(), |paths| wrap(FilesMsg::Chosen(paths)));
            }
            FilesMsg::Chosen(paths) => return self.prepare_paths(paths),
            FilesMsg::Prepared(key, result) => {
                let Some(at) = files.attached.iter().position(|attached| attached.key() == key) else {
                    // Taken out while the core read it: let it go there too.
                    if let Ok(file) = result {
                        return release(backend, account, vec![file.id]);
                    }
                    return Task::none();
                };
                let name = match &files.attached[at] {
                    Attached::Preparing { name, .. } | Attached::Failed { name, .. } => name.clone(),
                    Attached::Ready { file, .. } => file.name.clone(),
                };
                files.attached[at] = match result {
                    Ok(file) => {
                        let thumb = file.preview.as_deref().and_then(system::picture_handle);
                        Attached::Ready { key, file, thumb }
                    }
                    Err(err) => Attached::Failed { key, name, why: file_failed(&err) },
                };
            }
            FilesMsg::Remove(key) => {
                let Some(at) = files.attached.iter().position(|attached| attached.key() == key) else {
                    return Task::none();
                };
                if let Attached::Ready { file, .. } = files.attached.remove(at) {
                    return release(backend, account, vec![file.id]);
                }
            }
            FilesMsg::Done => {}
            FilesMsg::Paste => return Task::perform(system::clipboard(), |clip| wrap(FilesMsg::Pasted(clip))),
            FilesMsg::Pasted(result) => match result {
                Ok(Clip::Files(paths)) => return self.prepare_paths(paths),
                Ok(Clip::Picture(png)) => {
                    if !self.room_for(1) {
                        return Task::none();
                    }
                    let Some(chat) = self.chat.as_mut() else { return Task::none() };
                    let key = chat.files.key();
                    chat.files.attached.push(Attached::Preparing { key, name: PASTED_NAME.to_owned() });
                    let task = backend.upload_bytes(account, PASTED_NAME.to_owned(), png);
                    return Task::perform(task, move |result| wrap(FilesMsg::Prepared(key, result)));
                }
                Ok(Clip::Text(text)) => {
                    chat.editor.perform(Action::Edit(Edit::Paste(Arc::new(text))));
                    chat.send_error = None;
                }
                Ok(Clip::Empty) => {}
                Err(why) => self.file_note(why),
            },
            FilesMsg::Hovered(over) => files.dropping = over && !chat.gone,
            FilesMsg::Dropped(path) => {
                files.dropping = false;
                return self.prepare_paths(vec![path]);
            }
            FilesMsg::Started(result) => {
                chat.sending = false;
                match result {
                    Ok(Sending::Uploading(id)) => {
                        let thumb = files.attached.iter().find_map(|attached| match attached {
                            Attached::Ready { thumb, .. } => thumb.clone(),
                            _ => None,
                        });
                        let names: Vec<String> = files
                            .attached
                            .iter()
                            .filter_map(|attached| match attached {
                                Attached::Ready { file, .. } => Some(file.name.clone()),
                                _ => None,
                            })
                            .collect();
                        files.upload = Some(Upload {
                            id,
                            name: names.first().cloned().unwrap_or_default(),
                            others: names.len().saturating_sub(1),
                            thumb,
                            progress: None,
                            asking: false,
                            cancelling: false,
                            error: None,
                        });
                        // The prepared files are used up now; what failed goes with them.
                        files.attached.clear();
                        chat.editor = Content::new();
                        chat.reply = None;
                        chat.picked.clear();
                        chat.suggestions.clear();
                        chat.asked = None;
                        chat.at_bottom = true;
                        return Task::batch([
                            operation::snap_to_end(HISTORY),
                            operation::focus(EDITOR),
                            self.files_update(FilesMsg::Poll),
                        ]);
                    }
                    Ok(Sending::Sent(id)) => {
                        files.attached.clear();
                        return self.chat_update(ChatMsg::Sent(Ok(id)));
                    }
                    // Text and files stay; nothing is sent later (decision 8).
                    Err(err) => chat.send_error = Some(file_failed(&err)),
                }
            }
            FilesMsg::Poll => {
                let Some(upload) = files.upload.as_mut().filter(|upload| upload.running() && !upload.asking) else {
                    return Task::none();
                };
                upload.asking = true;
                let id = upload.id.clone();
                return Task::perform(backend.upload(id), |result| wrap(FilesMsg::Progress(result)));
            }
            FilesMsg::Progress(result) => {
                let Some(upload) = files.upload.as_mut() else { return Task::none() };
                upload.asking = false;
                match result {
                    Ok(progress) => match progress.state {
                        UploadState::Sent => {
                            files.upload = None;
                            return Task::batch([self.chat_update(ChatMsg::Poll), operation::snap_to_end(HISTORY)]);
                        }
                        UploadState::Cancelled => {
                            files.upload = None;
                            self.notify(tr("Upload cancelled").to_owned());
                        }
                        UploadState::Failed => {
                            let why = progress.message.clone().unwrap_or_else(|| {
                                tr("The files could not be sent. Choose them again to retry.").to_owned()
                            });
                            upload.error = Some(why);
                            upload.progress = Some(progress);
                        }
                        UploadState::Uploading | UploadState::Sharing => upload.progress = Some(progress),
                    },
                    Err(ChatError::NotFound) => files.upload = None,
                    // Another look at the next tick.
                    Err(_) => {}
                }
            }
            FilesMsg::Cancel => {
                let Some(upload) = files.upload.as_mut().filter(|upload| upload.running() && !upload.cancelling) else {
                    return Task::none();
                };
                upload.cancelling = true;
                let id = upload.id.clone();
                return Task::perform(backend.cancel_upload(id), |result| wrap(FilesMsg::Cancelled(result)));
            }
            FilesMsg::Cancelled(result) => match result {
                Ok(()) => {
                    files.upload = None;
                    self.notify(tr("Upload cancelled").to_owned());
                }
                Err(err) => {
                    if let Some(upload) = files.upload.as_mut() {
                        upload.cancelling = false;
                    }
                    self.file_note(file_failed(&err));
                }
            },
            FilesMsg::DismissUpload => files.upload = None,
            FilesMsg::Picture(key, result) => {
                let shown = match result {
                    Ok(picture) => system::picture_handle(&picture.bytes).map_or(Shown::Missing, Shown::Ready),
                    Err(_) => Shown::Missing,
                };
                files.pictures.insert(key, shown);
            }
            FilesMsg::Open(message, file) => {
                if files.busy.contains_key(&(message.clone(), file.clone())) {
                    return Task::none();
                }
                files.busy.insert((message.clone(), file.clone()), Busy::Opening);
                return Task::perform(backend.attachment(message.clone(), file.clone()), move |result| {
                    wrap(FilesMsg::Fetched(message.clone(), file.clone(), result))
                });
            }
            FilesMsg::Fetched(message, file, result) => {
                files.busy.remove(&(message, file));
                match result {
                    // The demo's files are nowhere on disk.
                    Ok(_) if demo => self.notify(tr("The demo opens no files.").to_owned()),
                    Ok(fetched) => {
                        if let Err(why) = system::open_file(&fetched.path) {
                            self.file_note(why);
                        }
                    }
                    Err(err) => self.file_note(file_failed(&err)),
                }
            }
            FilesMsg::Save(message, file) => return self.save(message, file, SaveTo::Downloads),
            FilesMsg::SaveAs(message, file) => {
                let name = file_of(chat, &message, &file).map(|shared| shared.name.clone()).unwrap_or_default();
                return Task::perform(system::choose_save_file(name), move |path| {
                    wrap(FilesMsg::SaveTo(message.clone(), file.clone(), path))
                });
            }
            FilesMsg::SaveTo(message, file, path) => match path {
                Some(path) => return self.save(message, file, SaveTo::File(path.to_string_lossy().into_owned())),
                None => self.notify(tr("Nothing was saved").to_owned()),
            },
            FilesMsg::Saved(message, file, result) => {
                files.busy.remove(&(message, file));
                match result {
                    Ok(path) => self.notify(trf("Saved to {path}", &[("path", &path)])),
                    Err(err) => self.file_note(file_failed(&err)),
                }
            }
            FilesMsg::Nextcloud(file) => match system::nextcloud_file(&account, &file) {
                Some(link) => {
                    if let Err(why) = system::open_link(&link) {
                        self.file_note(why);
                    }
                }
                None => self.file_note(tr("This file has no address in Nextcloud.").to_owned()),
            },
            FilesMsg::OpenLink(link) => {
                if let Err(why) = system::open_link(&link) {
                    self.file_note(why);
                }
            }
            FilesMsg::ShowCard(message) => {
                if files.cards.get(&message) == Some(&CardAsk::Asking) {
                    return Task::none();
                }
                files.cards.insert(message.clone(), CardAsk::Asking);
                return Task::perform(backend.show_card(message.clone()), move |result| {
                    wrap(FilesMsg::Card(message.clone(), result))
                });
            }
            FilesMsg::Card(message, result) => match result {
                Ok(Some(card)) => {
                    files.cards.remove(&message);
                    let image = card.image;
                    if let Some(held) = chat.messages.iter_mut().find(|held| held.id == message) {
                        held.cards = vec![card];
                        held.foreign = None;
                    }
                    if image {
                        return self.want_pictures();
                    }
                }
                Ok(None) => {
                    files.cards.insert(message, CardAsk::None);
                }
                Err(err) => {
                    files.cards.remove(&message);
                    self.file_note(file_failed(&err));
                }
            },
            FilesMsg::Decide(key, choice) => {
                return Task::perform(backend.set_provider(key.clone(), choice), move |result| {
                    wrap(FilesMsg::Decided(key.clone(), choice, result))
                });
            }
            // Every message of the window that asked about the provider asks no more: an allowed one
            // fetches its card (as the core will by itself from now on), a blocked one keeps its link.
            FilesMsg::Decided(key, choice, result) => match result {
                Ok(()) => {
                    let mut asked = Vec::new();
                    for held in chat.messages.iter_mut() {
                        if held.foreign.as_ref().is_some_and(|provider| provider.key == key) {
                            held.foreign = None;
                            asked.push(held.id.clone());
                        }
                    }
                    if choice == ProviderChoice::Allowed {
                        return Task::batch(asked.into_iter().map(|id| self.files_update(FilesMsg::ShowCard(id))));
                    }
                }
                Err(err) => self.file_note(super::chat_failed(&err)),
            },
            FilesMsg::View(message, file) => {
                self.menu = None;
                let appearance = self.launch.appearance.map(|appearance| match appearance {
                    torro_gui::theme::Appearance::Light => "light",
                    torro_gui::theme::Appearance::Dark => "dark",
                    torro_gui::theme::Appearance::System => "system",
                });
                if self.backend.open_viewer(&message, &file, appearance).is_err() {
                    self.notify(tr("The picture could not be opened.").to_owned());
                }
            }
            FilesMsg::ToggleMedia if files.media.is_some() => return self.files_update(FilesMsg::CloseMedia),
            FilesMsg::ToggleMedia => {
                files.media = Some(MediaPanel { loading: true, ..MediaPanel::default() });
                let load = load_shared(backend, conversation, MediaTab::Media, None, false);
                // A window too narrow for both grows by the column, and gives it back on closing.
                if media_beside(self.size.width) {
                    return load;
                }
                let wider = self.size.width + size::props() + split::divider_width();
                let grow = wider.max(2.0 * split::min_pane() + split::divider_width()) - self.size.width;
                if let Some(chat) = self.chat.as_mut() {
                    chat.files.grew = Some(grow);
                }
                return Task::batch([load, resize_by(self.size, grow)]);
            }
            FilesMsg::CloseMedia => {
                files.media = None;
                if let Some(grew) = files.grew.take() {
                    return resize_by(self.size, -grew);
                }
            }
            FilesMsg::Tab(tab) => {
                let Some(panel) = files.media.as_mut() else { return Task::none() };
                *panel = MediaPanel { tab, loading: true, ..MediaPanel::default() };
                return load_shared(backend, conversation, tab, None, false);
            }
            FilesMsg::Shared(tab, append, result) => {
                let Some(panel) = files.media.as_mut().filter(|panel| panel.tab == tab) else {
                    return Task::none();
                };
                panel.loading = false;
                match result {
                    Ok(page) => {
                        if append {
                            panel.items.extend(page.items);
                        } else {
                            panel.items = page.items;
                        }
                        panel.more = page.more;
                        panel.error = None;
                        return self.want_pictures();
                    }
                    Err(err) => panel.error = Some(err),
                }
            }
            FilesMsg::MoreShared => {
                let Some(panel) = files.media.as_mut().filter(|panel| !panel.loading && panel.more) else {
                    return Task::none();
                };
                panel.loading = true;
                let before = panel.items.last().map(|item| item.message.clone());
                return load_shared(backend, conversation, panel.tab, before, true);
            }
            FilesMsg::ShowInChat(message) => {
                self.menu = None;
                // Where the history and the column do not fit side by side, the history comes back.
                if !media_beside(self.size.width)
                    && let Some(chat) = self.chat.as_mut()
                {
                    chat.files.media = None;
                }
                let Some(chat) = self.chat.as_mut() else { return Task::none() };
                if let Some(at) = chat.messages.iter().position(|held| held.id == message) {
                    chat.files.highlight = Some(message);
                    chat.at_bottom = false;
                    return snap_to_share(at, chat.messages.len());
                }
                return Task::perform(backend.messages_around(conversation, message.clone()), move |result| {
                    wrap(FilesMsg::Around(message.clone(), result))
                });
            }
            FilesMsg::Around(message, result) => match result {
                Ok(page) => {
                    chat.messages = page.messages;
                    chat.more = page.more;
                    chat.at_bottom = false;
                    files.highlight = Some(message.clone());
                    let at = chat.messages.iter().position(|held| held.id == message).unwrap_or(0);
                    let snap = snap_to_share(at, chat.messages.len());
                    return Task::batch([snap, self.want_pictures()]);
                }
                Err(err) => self.file_note(file_failed(&err)),
            },
            FilesMsg::Split(event) => {
                let limits = media_limits(self.size.width);
                if files.split.update(event, limits) && !demo {
                    return ui_state::save_split_task(AppId::CHAT, MEDIA_SPLIT.to_owned(), files.split.chosen);
                }
            }
            FilesMsg::Stored(widths) => files.split.restore(widths.get(MEDIA_SPLIT).copied()),
            FilesMsg::ItemMenu(index, at) => self.menu = Some(Menu::Shared { index, at }),
        }
        Task::none()
    }

    /// Room for `more` files above the composer: at most `torro_ipc::chat::MAX_FILES` go in one
    /// message; beyond that the calm line says so.
    fn room_for(&mut self, more: usize) -> bool {
        let held = self.chat.as_ref().map_or(0, |chat| chat.files.attached.len());
        if held + more <= torro_ipc::chat::MAX_FILES {
            return true;
        }
        let most = torro_ipc::chat::MAX_FILES;
        self.file_note(trf("At most {count} files go in one message.", &[("count", &most)]));
        false
    }

    /// The files at `paths`, chosen for sending: each to the core with `chat.prepare_file`.
    fn prepare_paths(&mut self, paths: Vec<PathBuf>) -> Task<Message> {
        let writable = self.chat.as_ref().is_some_and(|chat| !chat.gone)
            && !self
                .conversations
                .iter()
                .any(|c| self.chat.as_ref().is_some_and(|chat| chat.id == c.id) && c.read_only);
        if paths.is_empty() || !writable || !self.room_for(paths.len()) {
            return Task::none();
        }
        let account = self.chat_account().unwrap_or_default();
        let backend = self.backend.clone();
        let Some(chat) = self.chat.as_mut() else { return Task::none() };
        chat.note = None;
        let tasks = paths.into_iter().map(|path| {
            let key = chat.files.key();
            let name = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            chat.files.attached.push(Attached::Preparing { key, name });
            let task = backend.clone().prepare_file(account.clone(), path.to_string_lossy().into_owned());
            Task::perform(task, move |result| wrap(FilesMsg::Prepared(key, result)))
        });
        Task::batch(tasks.collect::<Vec<_>>())
    }

    /// `chat.save_attachment` of `(message, file)` to `to`, unless it is busy already.
    fn save(&mut self, message: String, file: String, to: SaveTo) -> Task<Message> {
        self.menu = None;
        let backend = self.backend.clone();
        let Some(chat) = self.chat.as_mut() else { return Task::none() };
        let key = (message.clone(), file.clone());
        if chat.files.busy.contains_key(&key) {
            return Task::none();
        }
        chat.files.busy.insert(key, Busy::Saving);
        Task::perform(backend.save_attachment(message.clone(), file.clone(), to), move |result| {
            wrap(FilesMsg::Saved(message.clone(), file.clone(), result))
        })
    }

    /// Enter with files above the composer: the ready ones go with the text as caption
    /// (`chat.send_message` with `files`). Waits while one is still being prepared.
    pub(crate) fn send_files(&mut self) -> Task<Message> {
        let backend = self.backend.clone();
        let Some(chat) = self.chat.as_mut() else { return Task::none() };
        let ids = chat.files.ready();
        if ids.is_empty() || chat.files.preparing() || chat.sending || chat.gone || chat.files.upload.is_some() {
            return Task::none();
        }
        chat.sending = true;
        chat.send_error = None;
        let outgoing = Outgoing {
            conversation: chat.id.clone(),
            text: timeline::outgoing(&chat.editor.text(), &chat.picked),
            reply_to: chat.reply.as_ref().map(|quote| quote.id.clone()),
            silent: false,
        };
        Task::perform(backend.send_files(outgoing, ids), |result| wrap(FilesMsg::Started(result)))
    }

    /// Asks for the previews the history and the shared media show and do not have yet: pictures
    /// and videos with a preview, pictures of link cards (small).
    pub(crate) fn want_pictures(&mut self) -> Task<Message> {
        let backend = self.backend.clone();
        let Some(chat) = self.chat.as_mut() else { return Task::none() };
        let mut wanted: Vec<PictureKey> = Vec::new();
        for message in &chat.messages {
            for file in &message.files {
                if crate::view::has_preview(file) {
                    wanted.push((message.id.clone(), Some(file.id.clone())));
                }
            }
            if message.cards.first().is_some_and(|card| card.image) {
                wanted.push((message.id.clone(), None));
            }
        }
        if let Some(panel) = &chat.files.media {
            for item in panel.items.iter().filter(|item| item.preview && item.kind == SharedKind::Media) {
                wanted.push((item.message.clone(), item.file.clone()));
            }
        }
        let mut tasks: Vec<Task<Message>> = Vec::new();
        for key in wanted {
            if chat.files.pictures.contains_key(&key) {
                continue;
            }
            chat.files.pictures.insert(key.clone(), Shown::Loading);
            let task = backend.clone().preview(key.0.clone(), key.1.clone(), PreviewSize::Small);
            tasks.push(Task::perform(task, move |result| wrap(FilesMsg::Picture(key.clone(), result))));
        }
        Task::batch(tasks)
    }

    /// The demo states of stage 4 that need the first page (`--open attach | upload | media | drop`).
    pub(crate) fn apply_files_open(&mut self, open: crate::launch::Open) -> Task<Message> {
        use crate::launch::Open;
        match open {
            Open::Attach => {
                let paths = ["/home/anna/Pictures/Header.png", "/home/anna/Documents/Brief.pdf", "/home/anna/disk.iso"];
                let task = self.prepare_paths(paths.into_iter().map(PathBuf::from).collect());
                if let Some(chat) = self.chat.as_mut() {
                    chat.set_text("Here is the new header", "");
                }
                task
            }
            Open::Upload => {
                let Some(chat) = self.chat.as_ref() else { return Task::none() };
                let Some(id) = self.backend.stalled_upload(&chat.id) else { return Task::none() };
                if let Some(chat) = self.chat.as_mut() {
                    chat.files.upload = Some(Upload {
                        id,
                        name: "Screenshot.png".into(),
                        others: 0,
                        thumb: None,
                        progress: None,
                        asking: false,
                        cancelling: false,
                        error: None,
                    });
                }
                self.files_update(FilesMsg::Poll)
            }
            Open::Media => self.files_update(FilesMsg::ToggleMedia),
            Open::Drop => {
                if let Some(chat) = self.chat.as_mut() {
                    chat.files.dropping = true;
                }
                Task::none()
            }
            _ => Task::none(),
        }
    }
}

/// `chat.release_files` of `ids`, without waiting for the answer.
fn release(backend: crate::backend::Backend, account: String, ids: Vec<String>) -> Task<Message> {
    Task::perform(backend.release_files(account, ids), |_| wrap(FilesMsg::Done))
}

/// `chat.list_shared` of `tab`, older than `before`.
fn load_shared(
    backend: crate::backend::Backend,
    conversation: String,
    tab: MediaTab,
    before: Option<String>,
    append: bool,
) -> Task<Message> {
    Task::perform(backend.shared(conversation, tab.kind(), before), move |result| {
        wrap(FilesMsg::Shared(tab, append, result))
    })
}

/// The history scrolled so the message `at` of `count` stands about in view.
fn snap_to_share(at: usize, count: usize) -> Task<Message> {
    let share = if count <= 1 { 0.0 } else { at as f32 / (count - 1) as f32 };
    operation::snap_to(HISTORY, RelativeOffset { x: None, y: Some(share) })
}

/// The window `by` wider (narrower when negative); a tiling manager may keep its size.
fn resize_by(size: Size, by: f32) -> Task<Message> {
    let target = Size::new((size.width + by).max(size::props()), size.height);
    iced::window::latest().and_then(move |id| iced::window::resize(id, target))
}
