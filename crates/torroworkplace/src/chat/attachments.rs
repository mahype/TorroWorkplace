//! Reading files and media of the chat (stage 4 of `docs/chat.md`): previews (`chat.get_preview`),
//! files fetched for opening (`chat.get_attachment`) or saved (`chat.save_attachment`), what a
//! conversation shared (`chat.list_shared`) and link cards (`cards` of `chat.get_messages`,
//! `chat.show_card`).
//!
//! - **Only files of the message.** A file is named with its message; it must be one of that
//!   message's files as the core reads it from the server ([`ChatService::located`]), else
//!   `not-found` before anything is fetched. Id and path on the server come from there, never from
//!   the request, so no client opens the rest of the person's storage.
//! - **Picture content** needs [`torro_ipc::api::Rights::image_content`] for a client: checked by
//!   the kind the server names before fetching and by the bytes after ([`super::media::sniff`]); a
//!   kind nobody can tell counts as a picture. Without it `blurhash` is left out too.
//! - **`hide-download`** makes fetching and saving `forbidden` for everyone; files in conversations
//!   with people of other servers are `unsupported` to preview, fetch or save.
//! - **Pictures only from the account's own server**, only in memory ([`PREVIEWS_PER_ACCOUNT`]);
//!   never an SVG. Fetched files lie in the chat's own folder, `<cache>/chat-attachments/<account
//!   hash>/` (0700, files 0600), and go after [`FETCHED_FOR`].
//! - **Link cards** of links to the account's own server are resolved by themselves when an app
//!   reads messages. Any other follows Settings › Privacy (issue #672), because the server fetches
//!   the address and its owner might learn when the message was read: by itself for a provider
//!   allowed or at "Always show"; for one not asked yet the message names it (`foreign`) and the card
//!   comes with `chat.show_card`, a person's click ("Show once"); never for one blocked or at "Never
//!   show", where a card held from before is left out too. A client's read resolves nothing.
//! - **YouTube videos** (issue #673): the card of a link to a video carries its id (`video`), also
//!   when the server knows no card for it; so the chat window can play exactly what Settings ›
//!   Privacy let load, and a person's "Show once" counts for playing that video too.

use std::collections::{BTreeMap, VecDeque};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD;
use chrono::Local;
use torro_chat::{ChatError, ForeignProvider, LinkCard, MessagePage, split_message};
use torro_core::i18n::{tr, trf};
use torro_core::privacy::Decision;
use torro_ipc::Response;
use torro_ipc::chat::{
    ChatAttachment, ChatPicture, FileData, MAX_APP_ATTACHMENT_BYTES, MAX_CLIENT_ATTACHMENT_BYTES, MAX_SHARED,
    PreviewSize, SHARED, SharedItem, SharedKind,
};
use torro_mail::SaveTargetSpec;
use torro_nextcloud::RoomToken;

use super::ChatService;
use super::media::{clean_name, file_refusal, is_picture_content, refused, sniff};

/// The name of a file of the server on this computer: [`clean_name`] (its last part, one line), then
/// the rules of every system a folder may be on ([`torro_mail::save_target::clean_file_name`]): no
/// `:` that would name a drive or a stream on Windows (`C:evil.exe` would replace the whole folder
/// in `Path::join`), no reserved name such as `CON`, nothing invisible. `file` when nothing is left.
fn local_name(name: &str) -> String {
    let name = torro_mail::save_target::clean_file_name(&clean_name(name));
    if name.is_empty() { torro_core::i18n::tr("file").to_string() } else { name }
}
use super::remote::{Caps, DavPath, Picture, Remote, RemoteMessage, RemoteRoom};
use super::service::{Caller, error_response};
use crate::pim::Reach;

/// The most bytes of previews kept per account.
pub const PREVIEWS_PER_ACCOUNT: u64 = 64 * 1024 * 1024;

/// The most link cards kept.
pub const CARDS_KEPT: usize = 2000;

/// The most cards resolved by one read of an app: links to the own server and to providers allowed.
const CARDS_PER_READ: usize = 5;

/// How long a fetched file stays in the chat's folder.
pub const FETCHED_FOR: Duration = Duration::from_secs(24 * 60 * 60);

/// A link card the core holds, and the id of its picture on the account's server.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Card {
    /// `None`: asked, the server knows none (so it is not asked again).
    card: Option<LinkCard>,
    image: Option<String>,
}

/// Previews and link cards in memory.
#[derive(Debug, Default)]
pub struct Held {
    /// Per account: the previews by key (`<file id>:<edge>`), the oldest first, and their bytes.
    previews: BTreeMap<String, (VecDeque<(String, Picture)>, u64)>,
    /// By message id, the oldest first.
    cards: BTreeMap<String, Card>,
    order: VecDeque<String>,
}

impl Held {
    fn preview(&self, account: &str, key: &str) -> Option<Picture> {
        let (list, _) = self.previews.get(account)?;
        list.iter().find(|(held, _)| held == key).map(|(_, picture)| picture.clone())
    }

    /// Keeps `picture`; the oldest of the account go beyond [`PREVIEWS_PER_ACCOUNT`].
    fn keep_preview(&mut self, account: &str, key: String, picture: Picture) {
        let (list, bytes) = self.previews.entry(account.to_string()).or_default();
        *bytes += picture.bytes.len() as u64;
        list.push_back((key, picture));
        while *bytes > PREVIEWS_PER_ACCOUNT {
            match list.pop_front() {
                Some((_, gone)) => *bytes -= gone.bytes.len() as u64,
                None => break,
            }
        }
    }

    fn keep_card(&mut self, message: &str, card: Card) {
        if self.cards.insert(message.to_string(), card).is_none() {
            self.order.push_back(message.to_string());
        }
        while self.order.len() > CARDS_KEPT {
            if let Some(oldest) = self.order.pop_front() {
                self.cards.remove(&oldest);
            }
        }
    }

    /// Drops everything of `account`.
    pub fn forget_account(&mut self, account: &str) {
        self.previews.remove(account);
        let prefix = format!("{account}/");
        self.cards.retain(|id, _| !id.starts_with(&prefix));
        self.order.retain(|id| !id.starts_with(&prefix));
    }
}

/// Where `chat.save_attachment` puts a file, as the core resolved it.
#[derive(Debug, Clone)]
pub enum Place {
    /// Downloads or a folder a person chose: a taken name gets a number.
    Folder(PathBuf),
    /// Exactly this file, which a person chose in the system's dialog.
    File(PathBuf),
    /// A save target: below its folder after its schemes.
    Target(SaveTargetSpec),
}

/// A file of a message as the core read it.
struct Located {
    account: String,
    remote: Remote,
    user: String,
    room: RemoteRoom,
    message: RemoteMessage,
    file: torro_chat::FileRef,
    path: Option<String>,
}

/// Whether `caller` may see picture content.
fn sees_pictures(caller: Caller<'_>) -> bool {
    match caller.reach {
        Reach::Suite => true,
        Reach::Client(rights) => rights.image_content,
    }
}

fn no_picture_right() -> Response {
    refused("forbidden", tr("This key may not see pictures and videos."))
}

/// The first `https` link of `text`, without the punctuation after it.
pub fn first_link(text: &str) -> Option<String> {
    let start = text.find("https://")?;
    let link: String = text[start..].chars().take_while(|c| !c.is_whitespace() && !c.is_control()).collect();
    let link = link.trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}', '>', '"', '\'']);
    let url = url::Url::parse(link).ok()?;
    (url.scheme() == "https" && url.host_str().is_some()).then(|| link.to_string())
}

/// What a message's first link is (issue #672).
enum Link {
    /// To the account's own server: its card is resolved for an app by itself.
    Own,
    /// To another server: what Settings › Privacy decides for its provider.
    Other(Decision),
}

impl Link {
    /// Whether its card is resolved by itself when an app reads the message.
    fn loads(&self) -> bool {
        matches!(self, Link::Own | Link::Other(Decision::Load))
    }
}

/// Whether Settings › Privacy turned the card of `link` off (issue #672): a link to another server than
/// `server` whose provider is blocked, or any such link at "Never show". An empty link (a card without
/// one) counts as another server's.
fn turned_off(privacy: &torro_core::privacy::Privacy, link: &str, server: &str) -> bool {
    !torro_nextcloud::same_origin(link, server) && privacy.decide(&host_of(link)) == Decision::Skip
}

/// The host of `link` as the suite compares hosts ([`torro_core::privacy::normalize_host`]); empty when it
/// has none.
fn host_of(link: &str) -> String {
    let host = url::Url::parse(link).ok().and_then(|url| url.host_str().map(str::to_string)).unwrap_or_default();
    torro_core::privacy::normalize_host(&host)
}

/// The card of `link` from what the server resolved; a YouTube video carries its id (issue #673).
fn card_of(link: &str, reference: torro_nextcloud::Reference) -> Card {
    Card {
        card: Some(LinkCard {
            url: link.to_string(),
            title: reference.title,
            description: reference.description,
            domain: domain_of(link),
            image: reference.image.is_some(),
            video: torro_chat::video_of(link),
        }),
        image: reference.image,
    }
}

/// The card of `link` when the server knows none: a video card without title or picture for a
/// YouTube video (issue #673), so the video still plays; nothing for any other link.
fn unresolved(link: &str) -> Card {
    let card = torro_chat::video_of(link).map(|video| LinkCard {
        url: link.to_string(),
        title: String::new(),
        description: None,
        domain: domain_of(link),
        image: false,
        video: Some(video),
    });
    Card { card, image: None }
}

/// The host of `link` as it stands there (`youtu.be`); empty when it has none.
fn domain_of(link: &str) -> String {
    url::Url::parse(link).ok().and_then(|url| url.host_str().map(str::to_string)).unwrap_or_default()
}

/// `bytes` as a picture answer with its size, when they are a PNG, JPEG, GIF or WebP.
fn picture_answer(picture: Picture) -> Response {
    let kind = sniff(&picture.bytes);
    if !matches!(kind, "image/png" | "image/jpeg" | "image/gif" | "image/webp") {
        return error_response(&ChatError::NotFound);
    }
    let size = image::ImageReader::new(std::io::Cursor::new(&picture.bytes))
        .with_guessed_format()
        .ok()
        .and_then(|reader| reader.into_dimensions().ok());
    Response::ChatPicture {
        picture: ChatPicture {
            content_type: kind.to_string(),
            base64: FileData(STANDARD.encode(&picture.bytes)),
            width: size.map(|(width, _)| width),
            height: size.map(|(_, height)| height),
        },
    }
}

impl ChatService {
    /// Previews and cards; a lock a panicking thread left is taken over.
    pub(super) fn held(&self) -> std::sync::MutexGuard<'_, Held> {
        self.held.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The file `file` of the message `id` as the server holds it; `not-found` for a file that is not
    /// one of that message's.
    async fn located(self: &Arc<Self>, id: &str, file: &str) -> Result<Located, Response> {
        let (account, token, number) = super::service::message_of(id).map_err(|err| error_response(&err))?;
        let (remote, caps, user) = self.ready(account).await.map_err(|err| error_response(&err))?;
        let room = self.room(account, &token).await.map_err(|err| error_response(&err))?;
        let message =
            self.message(account, &token, number, &remote, &caps).await.map_err(|err| error_response(&err))?;
        let found = message.files.iter().find(|held| held.id == file).cloned();
        let file = found.ok_or_else(|| error_response(&ChatError::NotFound))?;
        let path = message.paths.get(&file.id).cloned();
        Ok(Located { account: account.to_string(), remote, user, room, message, file, path })
    }

    /// `chat.get_preview`.
    pub async fn get_preview(
        self: &Arc<Self>,
        id: &str,
        file: Option<&str>,
        card: bool,
        size: PreviewSize,
        caller: Caller<'_>,
    ) -> Response {
        if card {
            return self.card_preview(id, caller).await;
        }
        let Some(file) = file else { return refused("error", tr("Name a file of the message, or `card`.")) };
        let located = match self.located(id, file).await {
            Ok(located) => located,
            Err(refusal) => return refusal,
        };
        if located.room.federated {
            return error_response(&unsupported("Files of conversations with other servers open in Nextcloud only."));
        }
        // A preview is a picture of the content, of a PDF page as much as of a photo.
        if !sees_pictures(caller) {
            return no_picture_right();
        }
        if !located.file.preview || located.file.content_type.contains("svg") {
            return error_response(&ChatError::NotFound);
        }
        let Ok(file_id) = located.file.id.parse::<u64>() else { return error_response(&ChatError::NotFound) };
        let key = format!("{file_id}:{}", size.edge());
        if let Some(picture) = self.held().preview(&located.account, &key) {
            return picture_answer(picture);
        }
        match located.remote.preview(file_id, size.edge()).await {
            Ok(picture) => {
                self.held().keep_preview(&located.account, key, picture.clone());
                picture_answer(picture)
            }
            Err(err) => file_refusal(&err),
        }
    }

    /// The picture of the link card of the message `id`, from the account's own server.
    async fn card_preview(self: &Arc<Self>, id: &str, caller: Caller<'_>) -> Response {
        let Ok((account, token, _)) = super::service::message_of(id) else {
            return error_response(&ChatError::NotFound);
        };
        if let Err(err) = self.room(account, &token).await {
            return error_response(&err);
        }
        if !sees_pictures(caller) {
            return no_picture_right();
        }
        let remote = match self.remote(account).await {
            Ok(remote) => remote,
            Err(err) => return error_response(&err),
        };
        // No picture of a card Settings › Privacy turned off (issue #672): it is left out of the messages.
        let privacy = self.privacy();
        let server = remote.server();
        let image = self.held().cards.get(id).and_then(|card| {
            let link = card.card.as_ref().map(|card| card.url.as_str()).unwrap_or_default();
            if turned_off(&privacy, link, &server) { None } else { card.image.clone() }
        });
        let Some(image) = image else { return error_response(&ChatError::NotFound) };
        let key = format!("card:{image}");
        if let Some(picture) = self.held().preview(account, &key) {
            return picture_answer(picture);
        }
        match remote.card_picture(&image).await {
            Ok(picture) => {
                self.held().keep_preview(account, key, picture.clone());
                picture_answer(picture)
            }
            Err(err) => file_refusal(&err),
        }
    }

    /// The bytes of the file `file` of the message `id` for `caller`, with what was located: every
    /// check of fetching and saving.
    async fn fetched(
        self: &Arc<Self>,
        id: &str,
        file: &str,
        caller: Caller<'_>,
    ) -> Result<(Located, Vec<u8>), Response> {
        let located = self.located(id, file).await?;
        if located.room.federated {
            return Err(error_response(&unsupported(
                "Files of conversations with other servers open in Nextcloud only.",
            )));
        }
        if located.file.hide_download {
            return Err(refused("forbidden", tr("This file was shared to be looked at in Nextcloud only.")));
        }
        let pictures = sees_pictures(caller);
        if !pictures && is_picture_content(&located.file.content_type) {
            return Err(no_picture_right());
        }
        let max = match caller.client {
            Some(_) => MAX_CLIENT_ATTACHMENT_BYTES,
            None => MAX_APP_ATTACHMENT_BYTES,
        };
        if located.file.size > max {
            let limit = torro_gui::units::format_bytes(max);
            return Err(refused(
                "error",
                &trf("This file is larger than {limit}. Open it in Nextcloud.", &[("limit", &limit)]),
            ));
        }
        let path = located.path.as_deref().and_then(DavPath::parse).filter(|path| !path.segments().is_empty());
        let Some(path) = path else { return Err(error_response(&ChatError::NotFound)) };
        let bytes = located.remote.get_file(&located.user, &path, max).await.map_err(|err| file_refusal(&err))?;
        // What the bytes say counts too: a picture named a PDF is still a picture.
        if !pictures && is_picture_content(sniff(&bytes)) {
            return Err(no_picture_right());
        }
        Ok((located, bytes))
    }

    /// `chat.get_attachment`: the file in the chat's own folder.
    pub async fn get_attachment(self: &Arc<Self>, id: &str, file: &str, caller: Caller<'_>) -> Response {
        let (located, bytes) = match self.fetched(id, file, caller).await {
            Ok(fetched) => fetched,
            Err(refusal) => return refusal,
        };
        let Some(root) = self.attachments.clone() else {
            return refused("error", tr("There is no folder for chat files on this computer."));
        };
        let dir = root.join(crate::audit::hashed(&located.account));
        let name = local_name(&located.file.name);
        let kind = sniff(&bytes).to_string();
        let size = bytes.len() as u64;
        let written = tokio::task::spawn_blocking(move || {
            remove_old(&root, SystemTime::now());
            crate::clients::create_private_dir(&dir)?;
            write_new(&dir, &name, &bytes, true)
        })
        .await;
        match written {
            Ok(Ok(path)) => Response::ChatAttachment {
                attachment: ChatAttachment {
                    name: path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default(),
                    path: path.to_string_lossy().into_owned(),
                    size,
                    content_type: kind,
                    untrusted: true,
                },
            },
            _ => {
                eprintln!("torroworkplace: a chat file of {} could not be written", located.account);
                refused("error", tr("The file could not be written."))
            }
        }
    }

    /// `chat.save_attachment` to `place` as the core resolved it.
    pub async fn save_attachment(self: &Arc<Self>, id: &str, file: &str, place: Place, caller: Caller<'_>) -> Response {
        let (located, bytes) = match self.fetched(id, file, caller).await {
            Ok(fetched) => fetched,
            Err(refusal) => return refusal,
        };
        let name = local_name(&located.file.name);
        let place = match place {
            Place::Target(spec) => {
                let facts = torro_mail::FileFacts {
                    mail_date: located.message.at.with_timezone(&Local).date_naive(),
                    file_name: Some(name.clone()),
                    content_type: located.file.content_type.clone(),
                    sender_name: Some(located.message.author_name.clone()),
                    sender_address: String::new(),
                    account: located.account.clone(),
                    subject: located.room.name.clone(),
                    month_names: crate::mail::save::month_names(),
                };
                match torro_mail::save_target::resolve(&spec, &facts) {
                    Ok(resolved) => Target::Below(PathBuf::from(spec.folder), resolved.folders, resolved.file),
                    Err(_) => return refused("error", tr("The save target's scheme cannot be filled for this file.")),
                }
            }
            Place::Folder(dir) => Target::Below(dir, Vec::new(), name),
            Place::File(path) => Target::Exactly(path),
        };
        let written = tokio::task::spawn_blocking(move || place.write(&bytes)).await;
        match written {
            Ok(Ok(path)) => Response::ChatSaved { path: path.to_string_lossy().into_owned() },
            Ok(Err(refusal)) => refusal,
            Err(_) => refused("error", tr("The file could not be written.")),
        }
    }

    /// `chat.list_shared`.
    pub async fn list_shared(
        self: &Arc<Self>,
        conversation: &str,
        kind: Option<SharedKind>,
        before: Option<&str>,
        limit: Option<u32>,
        caller: Caller<'_>,
    ) -> Response {
        let (account, token) = match super::service::conversation_of(conversation) {
            Ok(found) => found,
            Err(err) => return error_response(&err),
        };
        let before = match before.map(|message| super::service::number_in(conversation, message)).transpose() {
            Ok(before) => before,
            Err(err) => return error_response(&err),
        };
        let limit = limit.unwrap_or(SHARED).clamp(1, MAX_SHARED);
        let (remote, caps, _) = match self.ready(account).await {
            Ok(ready) => ready,
            Err(err) => return error_response(&err),
        };
        if let Err(err) = self.room(account, &token).await {
            return error_response(&err);
        }
        let pictures = sees_pictures(caller);
        if kind == Some(SharedKind::Link) {
            return self.shared_links(conversation, before, limit, &remote.server());
        }
        if !caps.shared {
            return error_response(&unsupported("This server cannot list what a conversation shared."));
        }
        let found = match kind {
            Some(kind) => remote.shared(token.as_str(), kind.as_str(), before, limit).await.map(|messages| {
                messages.into_iter().map(|message| (kind.as_str().to_string(), message)).collect::<Vec<_>>()
            }),
            None => remote.overview(token.as_str(), limit.min(20)).await,
        };
        let found = match self.asked(account, found).await {
            Ok(found) => found,
            Err(err) => return error_response(&err),
        };
        let more = kind.is_some() && found.len() >= limit as usize;
        let items = found
            .into_iter()
            .flat_map(|(kind, message)| shared_items(conversation, &kind, message, pictures))
            .take(limit as usize)
            .collect();
        Response::ChatShared { items, more }
    }

    /// The links with a card in `conversation`, from the cards the core holds (Talk does not list links).
    fn shared_links(&self, conversation: &str, before: Option<u64>, limit: u32, server: &str) -> Response {
        // A card of a provider Settings › Privacy blocks, or of any other at "Never show", is left out as
        // in the messages (issue #672).
        let privacy = self.privacy();
        let held = self.held();
        let mut items: Vec<(u64, SharedItem)> = held
            .cards
            .iter()
            .filter_map(|(id, card)| {
                let (of, number) = split_message(id)?;
                let card = card.card.as_ref()?;
                if turned_off(&privacy, &card.url, server) {
                    return None;
                }
                (of == conversation && before.is_none_or(|before| number < before)).then(|| {
                    let item = SharedItem {
                        message: id.clone(),
                        kind: SharedKind::Link,
                        file: None,
                        name: card.url.clone(),
                        size: 0,
                        content_type: String::new(),
                        author: String::new(),
                        at: chrono::DateTime::<chrono::Utc>::UNIX_EPOCH,
                        preview: card.image,
                        blurhash: None,
                        untrusted: true,
                    };
                    (number, item)
                })
            })
            .collect();
        drop(held);
        // The time and author come from the messages the cache holds.
        let cache = self.cache();
        if let Some(thread) = cache.peek(conversation) {
            for (number, item) in &mut items {
                if let Some(message) = thread.get(*number) {
                    item.author.clone_from(&message.author_name);
                    item.at = message.at;
                }
            }
        }
        items.sort_by_key(|(number, _)| std::cmp::Reverse(*number));
        let more = items.len() > limit as usize;
        Response::ChatShared { items: items.into_iter().take(limit as usize).map(|(_, item)| item).collect(), more }
    }

    /// `page` of `conversation` as `caller` gets it: the cards the core holds on its messages, and
    /// for an app first the cards of links to the account's own server and to providers Settings ›
    /// Privacy allows (issue #672), and the provider of a link not asked about yet (`foreign`); a card
    /// of a provider it blocks is left out for everyone. Without the right to picture content no
    /// `blurhash`.
    pub(super) async fn decorate(
        self: &Arc<Self>,
        conversation: &str,
        mut page: MessagePage,
        caller: Caller<'_>,
    ) -> MessagePage {
        let privacy = self.privacy();
        let server = match torro_chat::split_conversation(conversation) {
            Some((account, _)) => self.remote(account).await.ok().map(|remote| (remote.server(), remote)),
            None => None,
        };
        // What each message's first link is, by message id: one to the own server, or the decision
        // for its provider. Without the server (signed out) every link counts as another's.
        let links: BTreeMap<String, (String, Link)> = page
            .messages
            .iter()
            .filter_map(|message| {
                let link = first_link(&message.text)?;
                let own = server.as_ref().is_some_and(|(server, _)| torro_nextcloud::same_origin(&link, server));
                let kind = match own {
                    true => Link::Own,
                    false => Link::Other(privacy.decide(&host_of(&link))),
                };
                Some((message.id.clone(), (link, kind)))
            })
            .collect();
        if matches!(caller.reach, Reach::Suite)
            && let Some((_, remote)) = &server
        {
            // In the order of the page, as before the settings (the map is ordered by the ids' text).
            let open: Vec<(String, String)> = {
                let held = self.held();
                page.messages
                    .iter()
                    .filter_map(|message| Some((&message.id, links.get(&message.id)?)))
                    .filter(|(id, (_, kind))| kind.loads() && !held.cards.contains_key(*id))
                    .map(|(id, (link, _))| (id.clone(), link.clone()))
                    .take(CARDS_PER_READ)
                    .collect()
            };
            for (id, link) in open {
                match remote.resolve(&link).await {
                    Ok(Some(reference)) => self.held().keep_card(&id, card_of(&link, reference)),
                    Ok(None) => self.held().keep_card(&id, unresolved(&link)),
                    // Not reachable now: asked again with the next read.
                    Err(_) => {}
                }
            }
        }
        let pictures = sees_pictures(caller);
        let app = matches!(caller.reach, Reach::Suite);
        let held = self.held();
        for message in &mut page.messages {
            let kind = links.get(&message.id).map(|(_, kind)| kind);
            let blocked = matches!(kind, Some(Link::Other(Decision::Skip)));
            match held.cards.get(&message.id) {
                Some(card) if !blocked => {
                    if let Some(card) = &card.card {
                        message.cards = vec![card.clone()];
                    }
                }
                // Asked about only where a person can answer, and only while no card is there.
                None if app => {
                    if let Some(Link::Other(Decision::Ask(provider))) = kind {
                        message.foreign =
                            Some(ForeignProvider { key: provider.key.clone(), name: provider.name.clone() });
                    }
                }
                _ => {}
            }
            if !pictures {
                message.files.iter_mut().for_each(|file| file.blurhash = None);
            }
        }
        drop(held);
        page
    }

    /// `chat.show_card`: the card of the first link of the message `id`, whatever server it is on;
    /// `unsupported` for a provider Settings › Privacy blocks or while it says "Never show" (issue
    /// #672).
    pub async fn show_card(self: &Arc<Self>, id: &str) -> Response {
        let (account, token, number) = match super::service::message_of(id) {
            Ok(found) => found,
            Err(err) => return error_response(&err),
        };
        let (remote, caps, _) = match self.ready(account).await {
            Ok(ready) => ready,
            Err(err) => return error_response(&err),
        };
        if let Err(err) = self.room(account, &token).await {
            return error_response(&err);
        }
        let message = match self.message(account, &token, number, &remote, &caps).await {
            Ok(message) => message,
            Err(err) => return error_response(&err),
        };
        let Some(link) = first_link(&message.text) else { return error_response(&ChatError::NotFound) };
        let own = torro_nextcloud::same_origin(&link, &remote.server());
        if !own && self.privacy().decide(&host_of(&link)) == Decision::Skip {
            return refused("unsupported", tr("Content from this provider is turned off in Settings › Privacy."));
        }
        let card = match remote.resolve(&link).await {
            Ok(Some(reference)) => card_of(&link, reference),
            Ok(None) => unresolved(&link),
            Err(err) => return file_refusal(&err),
        };
        let shown = card.card.clone();
        self.held().keep_card(id, card);
        Response::ChatCard { card: shown }
    }

    /// `chat.get_messages` with `around`: the message `number` with up to half of `limit` messages
    /// before and after it, from the server (capability `chat-get-context`).
    pub(super) async fn around(
        self: &Arc<Self>,
        conversation: &str,
        token: &RoomToken,
        number: u64,
        limit: u32,
        remote: &Remote,
        caps: &Caps,
    ) -> Result<Vec<RemoteMessage>, ChatError> {
        if !caps.context {
            return Err(unsupported("This server cannot show messages around another one."));
        }
        let (account, _) = torro_chat::split_conversation(conversation).ok_or(ChatError::NotFound)?;
        let found = remote.around(token.as_str(), number, (limit / 2).max(1)).await;
        Ok(self.asked(account, found).await?.into_iter().filter(|message| !message.hidden).collect())
    }
}

/// The items `message` of `conversation` shared as `kind`: one per file, else one for the message.
fn shared_items(conversation: &str, kind: &str, message: RemoteMessage, pictures: bool) -> Vec<SharedItem> {
    let kind = SharedKind::parse(kind).unwrap_or(SharedKind::Other);
    let id = torro_chat::message_id(conversation, message.number);
    if message.files.is_empty() {
        return vec![SharedItem {
            message: id,
            kind,
            file: None,
            name: message.text,
            size: 0,
            content_type: String::new(),
            author: message.author_name,
            at: message.at,
            preview: false,
            blurhash: None,
            untrusted: true,
        }];
    }
    message
        .files
        .into_iter()
        .map(|file| SharedItem {
            message: id.clone(),
            kind,
            file: Some(file.id),
            name: file.name,
            size: file.size,
            content_type: file.content_type,
            author: message.author_name.clone(),
            at: message.at,
            preview: file.preview,
            blurhash: file.blurhash.filter(|_| pictures),
            untrusted: true,
        })
        .collect()
}

/// Where a saved file goes, resolved.
enum Target {
    /// Below a folder that must exist, in these subfolders (made one at a time, each checked to lie
    /// below it), under a name that is never taken over.
    Below(PathBuf, Vec<String>, String),
    /// Exactly this file.
    Exactly(PathBuf),
}

impl Target {
    /// Writes `bytes` and answers where; runs on a blocking thread.
    fn write(self, bytes: &[u8]) -> Result<PathBuf, Response> {
        let missing = || refused("not-found", tr("The folder is missing."));
        let failed = |_| refused("error", tr("The file could not be written."));
        match self {
            Target::Exactly(path) => {
                if !path.parent().is_some_and(Path::is_dir) {
                    return Err(missing());
                }
                std::fs::write(&path, bytes).map_err(failed)?;
                Ok(path)
            }
            Target::Below(root, folders, name) => {
                let root = root.canonicalize().map_err(|_| missing())?;
                if !root.is_dir() {
                    return Err(missing());
                }
                let mut dir = root.clone();
                for folder in folders {
                    dir = dir.join(folder);
                    match std::fs::create_dir(&dir) {
                        Ok(()) => {}
                        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {}
                        Err(err) => return Err(failed(err)),
                    }
                    // A link planted below never makes a folder elsewhere.
                    dir = dir.canonicalize().map_err(failed)?;
                    if !dir.starts_with(&root) {
                        return Err(refused("error", tr("The save target's folder leads outside it.")));
                    }
                }
                write_new(&dir, &name, bytes, false).map_err(failed)
            }
        }
    }
}

/// Writes `bytes` as a new file `name` in `dir`, numbered („name (2).ext“) while the name is taken;
/// `private` makes it 0600 on Unix.
fn write_new(dir: &Path, name: &str, bytes: &[u8], private: bool) -> std::io::Result<PathBuf> {
    for copy in 1..=99 {
        let path = dir.join(torro_nextcloud::numbered(name, copy));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        if private {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        #[cfg(not(unix))]
        let _ = private;
        match options.open(&path) {
            Ok(mut file) => {
                file.write_all(bytes)?;
                return Ok(path);
            }
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(err),
        }
    }
    Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "every name is taken"))
}

/// Removes the fetched files under `root` older than [`FETCHED_FOR`] at `now`.
fn remove_old(root: &Path, now: SystemTime) {
    let Ok(accounts) = std::fs::read_dir(root) else { return };
    for account in accounts.flatten() {
        let Ok(files) = std::fs::read_dir(account.path()) else { continue };
        for file in files.flatten() {
            let old = file
                .metadata()
                .and_then(|meta| meta.modified())
                .is_ok_and(|modified| now.duration_since(modified).is_ok_and(|age| age >= FETCHED_FOR));
            if old {
                let _ = std::fs::remove_file(file.path());
            }
        }
    }
}

fn unsupported(text: &'static str) -> ChatError {
    ChatError::Unsupported(tr(text).to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Review of #643: a name from the server never leaves the folder it is saved in, on any system.
    #[test]
    fn a_name_from_the_server_stays_in_its_folder() {
        for (name, kept) in
            [("C:evil.exe", "C_evil.exe"), ("plan.pdf:stream", "plan.pdf_stream"), ("../../x.txt", "x.txt")]
        {
            assert_eq!(local_name(name), kept, "{name}");
        }
        assert_ne!(local_name("CON"), "CON", "a reserved name on Windows");
        assert_eq!(local_name(".."), torro_core::i18n::tr("file"));
        let dir = std::path::Path::new("/home/anna/chat");
        assert!(dir.join(local_name("C:evil.exe")).starts_with(dir));
    }

    #[test]
    fn the_first_link_is_https_only_without_trailing_punctuation() {
        assert_eq!(first_link("see https://example.org/a."), Some("https://example.org/a".into()));
        assert_eq!(first_link("(https://example.org/a?x=1)"), Some("https://example.org/a?x=1".into()));
        assert_eq!(first_link("http://example.org"), None);
        assert_eq!(first_link("nothing here"), None);
    }

    #[test]
    fn previews_stay_within_their_budget() {
        let mut held = Held::default();
        let picture = |n: usize| Picture { bytes: vec![0; n], content_type: "image/png".into() };
        let half = (PREVIEWS_PER_ACCOUNT / 2) as usize;
        held.keep_preview("a", "1".into(), picture(half));
        held.keep_preview("a", "2".into(), picture(half));
        held.keep_preview("a", "3".into(), picture(10));
        assert!(held.preview("a", "1").is_none() && held.preview("a", "3").is_some());
        held.keep_card("a/tok/1", Card { card: None, image: None });
        held.forget_account("a");
        assert!(held.preview("a", "3").is_none() && held.cards.is_empty());
    }

    #[test]
    fn a_saved_name_is_never_taken_over_and_stays_below_its_folder() {
        let dir = tempfile::tempdir().expect("temp dir");
        let first = write_new(dir.path(), "a.txt", b"one", false).expect("written");
        let second = write_new(dir.path(), "a.txt", b"two", false).expect("written");
        assert_eq!(second.file_name().and_then(|n| n.to_str()), Some("a (2).txt"));
        assert_eq!(std::fs::read(&first).expect("read"), b"one");
        let below = Target::Below(dir.path().to_path_buf(), vec!["2026".into(), "11".into()], "x.pdf".into());
        let path = below.write(b"pdf").expect("written");
        assert!(path.ends_with("2026/11/x.pdf"));
        let missing = Target::Below(dir.path().join("gone"), Vec::new(), "x".into());
        assert!(matches!(missing.write(b"x"), Err(Response::Error { code, .. }) if code == "not-found"));
    }
}
